//! Optimiseur Windows : nettoyage, confidentialité, performances, confort ; désinstallation
//! d'applications ; programmes au démarrage et applis en arrière-plan. Port de OptimiseurWindows.ps1.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use eframe::egui;
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, MutexGuard};
use windows_sys::Win32::System::Registry::*;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
type R = Result<(), String>;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------- journal partagé avec les threads de travail ----------

#[derive(Default)]
struct Etat {
    journal: Vec<String>,
    fichier: PathBuf,
    occupe: bool,
    tailles: Vec<Option<i64>>,
    point_fait: bool,
    apps: Option<Vec<Appli>>,
    entrees: Option<Vec<Entree>>,
}

#[derive(Clone)]
struct Log(Arc<Mutex<Etat>>);

impl Log {
    fn etat(&self) -> MutexGuard<'_, Etat> {
        self.0.lock().unwrap()
    }
    fn ecrire(&self, m: impl AsRef<str>) {
        let t = local();
        let l = format!("[{:02}:{:02}:{:02}] {}", t.wHour, t.wMinute, t.wSecond, m.as_ref());
        let mut e = self.etat();
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&e.fichier) {
            let _ = writeln!(f, "{l}");
        }
        e.journal.push(l);
    }
}

fn local() -> windows_sys::Win32::Foundation::SYSTEMTIME {
    let mut t = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    t
}

// ---------- outils généraux ----------

/// Remplace les %VAR% connues ; laisse les autres telles quelles.
fn exp(s: &str) -> String {
    let mut r = String::new();
    let mut reste = s;
    while let Some(i) = reste.find('%') {
        let apres = &reste[i + 1..];
        let Some(j) = apres.find('%') else { break };
        let nom = &apres[..j];
        match std::env::var(nom) {
            Ok(v) if !nom.is_empty() => {
                r.push_str(&reste[..i]);
                r.push_str(&v);
                reste = &apres[j + 1..];
            }
            _ => {
                r.push_str(&reste[..=i]);
                reste = apres;
            }
        }
    }
    r.push_str(reste);
    r
}

/// Joker * et ?, sans casse (comme -like de PowerShell).
fn like(s: &str, motif: &str) -> bool {
    fn f(s: &[char], m: &[char]) -> bool {
        match m.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| f(&s[i..], &m[1..])),
            Some('?') => !s.is_empty() && f(&s[1..], &m[1..]),
            Some(c) => s.first() == Some(c) && f(&s[1..], &m[1..]),
        }
    }
    let s: Vec<char> = s.to_lowercase().chars().collect();
    let m: Vec<char> = motif.to_lowercase().chars().collect();
    f(&s, &m)
}

fn groupe_milliers(n: i64) -> String {
    let s = n.abs().to_string();
    let mut r = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            r.push('\u{a0}');
        }
        r.push(c);
    }
    if n < 0 { format!("-{r}") } else { r }
}

fn format_taille(o: i64) -> String {
    const K: f64 = 1024.0;
    let f = o as f64;
    if f >= K * K * K {
        format!("{:.2} Go", f / (K * K * K)).replace('.', ",")
    } else if f >= K * K {
        format!("{} Mo", groupe_milliers((f / (K * K)).round() as i64))
    } else if f >= K {
        format!("{} Ko", groupe_milliers((f / K).round() as i64))
    } else {
        format!("{} o", groupe_milliers(o))
    }
}

/// Chemins correspondant au motif (variables d'environnement et jokers dans les composants).
fn chemins(motif: &str) -> Vec<PathBuf> {
    let m = exp(motif);
    let mut res = vec![PathBuf::new()];
    for (i, c) in m.split('\\').filter(|c| !c.is_empty()).enumerate() {
        let mut suiv = vec![];
        for p in &res {
            if i == 0 {
                suiv.push(PathBuf::from(format!("{c}\\")));
            } else if c.contains(['*', '?']) {
                if let Ok(rd) = fs::read_dir(p) {
                    suiv.extend(rd.flatten().filter(|e| like(&e.file_name().to_string_lossy(), c)).map(|e| e.path()));
                }
            } else {
                let q = p.join(c);
                if fs::symlink_metadata(&q).is_ok() {
                    suiv.push(q);
                }
            }
        }
        res = suiv;
    }
    res
}

const REPARSE: u32 = 0x400;
const DOSSIER: u32 = 0x10;

fn poids_dossier(d: &Path) -> i64 {
    let Ok(r) = fs::read_dir(d) else { return 0 };
    r.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.file_attributes() & REPARSE != 0 => 0,
            Ok(m) if m.file_attributes() & DOSSIER != 0 => poids_dossier(&e.path()),
            Ok(m) => m.len() as i64,
            _ => 0,
        })
        .sum()
}

fn poids_chemin(p: &Path) -> i64 {
    match fs::symlink_metadata(p) {
        Ok(m) if m.file_attributes() & REPARSE != 0 => 0,
        Ok(m) if m.is_dir() => poids_dossier(p),
        Ok(m) => m.len() as i64,
        _ => 0,
    }
}

fn poids(motifs: &[&str]) -> i64 {
    motifs.iter().flat_map(|m| chemins(m)).map(|p| poids_chemin(&p)).sum()
}

fn suppr_fichier(p: &Path, m: &fs::Metadata) {
    if m.permissions().readonly() {
        let mut pm = m.permissions();
        pm.set_readonly(false);
        let _ = fs::set_permissions(p, pm);
    }
    let _ = fs::remove_file(p);
}

/// Vide un dossier sans jamais suivre les liens (on supprime le lien lui-même).
fn vider_contenu(d: &Path) {
    let Ok(r) = fs::read_dir(d) else { return };
    for e in r.flatten() {
        let p = e.path();
        let Ok(m) = e.metadata() else { continue };
        let a = m.file_attributes();
        if a & REPARSE != 0 {
            let _ = if a & DOSSIER != 0 { fs::remove_dir(&p) } else { fs::remove_file(&p) };
        } else if a & DOSSIER != 0 {
            vider_contenu(&p);
            let _ = fs::remove_dir(&p);
        } else {
            suppr_fichier(&p, &m);
        }
    }
}

fn vider(motifs: &[&str]) -> R {
    for p in motifs.iter().flat_map(|m| chemins(m)) {
        if p.to_string_lossy().trim_end_matches('\\').len() <= 3 {
            continue; // jamais la racine d'un disque
        }
        let Ok(m) = fs::symlink_metadata(&p) else { continue };
        if m.file_attributes() & REPARSE != 0 {
            continue;
        }
        if m.is_dir() { vider_contenu(&p) } else { suppr_fichier(&p, &m) }
    }
    Ok(())
}

fn lancer(prog: &str, args: &[&str]) -> Result<i32, String> {
    Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW).status().map(|s| s.code().unwrap_or(-1)).map_err(|e| format!("{prog} : {e}"))
}

fn lancer_ok(prog: &str, args: &[&str]) -> R {
    match lancer(prog, args)? {
        0 => Ok(()),
        c => Err(format!("{prog} a renvoyé le code {c}")),
    }
}

/// PowerShell caché ; script encodé pour éviter tout souci de guillemets.
fn ps(script: &str) -> Result<String, String> {
    let s = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'; $ErrorActionPreference='Stop'\ntry {{\n{script}\n}} catch {{ [Console]::Out.Write('ERREUR:' + $_.Exception.Message); exit 1 }}");
    let u: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let o = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &B64.encode(u)])
        .stdin(Stdio::null()).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| format!("PowerShell : {e}"))?;
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.success() {
        Ok(out)
    } else {
        Err(out.split_once("ERREUR:").map_or("PowerShell a échoué", |(_, e)| e).trim().to_string())
    }
}

fn q(s: &str) -> String {
    s.replace('\'', "''")
}

fn svc_off(noms: &[&str]) -> R {
    for n in noms {
        if lancer("sc.exe", &["query", n])? != 0 {
            continue; // service absent
        }
        let _ = lancer("sc.exe", &["stop", n]);
        lancer_ok("sc.exe", &["config", n, "start=", "disabled"]).map_err(|e| format!("service {n} : {e}"))?;
    }
    Ok(())
}

fn est_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn relancer_admin() -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let Ok(exe) = std::env::current_exe() else { return false };
    let (op, f) = (w("runas"), w(&exe.to_string_lossy()));
    unsafe { ShellExecuteW(null_mut(), op.as_ptr(), f.as_ptr(), null(), null(), 1) as isize > 32 }
}

fn confirmer(m: &str) -> bool {
    rfd::MessageDialog::new().set_title("Optimiseur Windows").set_description(m).set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::OkCancel).show() == rfd::MessageDialogResult::Ok
}

fn oui_non(m: &str) -> bool {
    rfd::MessageDialog::new().set_title("Optimiseur Windows").set_description(m).set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes
}

fn info(m: &str) {
    rfd::MessageDialog::new().set_title("Optimiseur Windows").set_description(m).set_buttons(rfd::MessageButtons::Ok).show();
}

fn disque() -> Option<(u64, u64)> {
    let d = w(&exp(r"%SystemDrive%\"));
    let (mut libre, mut total, mut l2) = (0u64, 0u64, 0u64);
    (unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(d.as_ptr(), &mut libre, &mut total, &mut l2) } != 0).then_some((l2, total))
}

// ---------- registre ("HKCU\...", "HKLM\...", "HKU\...") ----------

struct Cle(HKEY);
impl Drop for Cle {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

fn err_reg(p: &str, e: u32) -> String {
    format!("{p} : {}", std::io::Error::from_raw_os_error(e as i32))
}

fn ouvrir(p: &str, creer: bool, acces: u32) -> Result<Cle, String> {
    let (r, s) = p.split_once('\\').unwrap_or((p, ""));
    let h = match r {
        "HKCU" => HKEY_CURRENT_USER,
        "HKLM" => HKEY_LOCAL_MACHINE,
        _ => HKEY_USERS,
    };
    let s = w(s);
    let mut k: HKEY = null_mut();
    let e = unsafe {
        if creer {
            RegCreateKeyExW(h, s.as_ptr(), 0, null(), REG_OPTION_NON_VOLATILE, acces, null(), &mut k, null_mut())
        } else {
            RegOpenKeyExW(h, s.as_ptr(), 0, acces, &mut k)
        }
    };
    if e == 0 { Ok(Cle(k)) } else { Err(err_reg(p, e)) }
}

fn existe(p: &str) -> bool {
    ouvrir(p, false, KEY_READ).is_ok()
}

fn reg_set(p: &str, nom: &str, typ: REG_VALUE_TYPE, data: &[u8]) -> R {
    let k = ouvrir(p, true, KEY_READ | KEY_SET_VALUE)?;
    let n = w(nom);
    match unsafe { RegSetValueExW(k.0, n.as_ptr(), 0, typ, data.as_ptr(), data.len() as u32) } {
        0 => Ok(()),
        e => Err(err_reg(p, e)),
    }
}

fn octets(u: &[u16]) -> Vec<u8> {
    u.iter().flat_map(|c| c.to_le_bytes()).collect()
}

fn reg_d(p: &str, nom: &str, v: u32) -> R {
    reg_set(p, nom, REG_DWORD, &v.to_le_bytes())
}

fn reg_s(p: &str, nom: &str, v: &str) -> R {
    reg_set(p, nom, REG_SZ, &octets(&w(v)))
}

fn reg_b(p: &str, nom: &str, v: &[u8]) -> R {
    reg_set(p, nom, REG_BINARY, v)
}

fn reg_m(p: &str, nom: &str, l: &[String]) -> R {
    let mut u = vec![];
    for s in l {
        u.extend(s.encode_utf16());
        u.push(0);
    }
    u.push(0);
    reg_set(p, nom, REG_MULTI_SZ, &octets(&u))
}

/// Plusieurs DWORD dans la même clé.
fn dw(p: &str, l: &[(&str, u32)]) -> R {
    l.iter().try_for_each(|(n, v)| reg_d(p, n, *v))
}

fn reg_lire(p: &str, nom: &str) -> Option<(u32, Vec<u8>)> {
    let k = ouvrir(p, false, KEY_READ).ok()?;
    let n = w(nom);
    let (mut t, mut taille) = (0u32, 0u32);
    unsafe {
        if RegQueryValueExW(k.0, n.as_ptr(), null(), &mut t, null_mut(), &mut taille) != 0 {
            return None;
        }
        let mut b = vec![0u8; taille as usize];
        if RegQueryValueExW(k.0, n.as_ptr(), null(), &mut t, b.as_mut_ptr(), &mut taille) != 0 {
            return None;
        }
        b.truncate(taille as usize);
        Some((t, b))
    }
}

fn utf16(b: &[u8]) -> String {
    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&u)
}

fn reg_dword(p: &str, nom: &str) -> Option<u32> {
    let (t, b) = reg_lire(p, nom)?;
    (t == REG_DWORD && b.len() >= 4).then(|| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn reg_texte(p: &str, nom: &str) -> Option<String> {
    let (t, b) = reg_lire(p, nom)?;
    let s = utf16(&b).trim_end_matches('\0').to_string();
    match t {
        REG_SZ => Some(s),
        REG_EXPAND_SZ => Some(exp(&s)),
        _ => None,
    }
}

fn reg_multi(p: &str, nom: &str) -> Vec<String> {
    match reg_lire(p, nom) {
        Some((REG_MULTI_SZ, b)) => utf16(&b).split('\0').filter(|s| !s.is_empty()).map(String::from).collect(),
        _ => vec![],
    }
}

fn reg_suppr(p: &str, nom: &str) -> R {
    if reg_lire(p, nom).is_none() {
        return Ok(());
    }
    let k = ouvrir(p, false, KEY_SET_VALUE)?;
    let n = w(nom);
    match unsafe { RegDeleteValueW(k.0, n.as_ptr()) } {
        0 => Ok(()),
        e => Err(err_reg(p, e)),
    }
}

fn reg_sous_cles(p: &str) -> Vec<String> {
    let Ok(k) = ouvrir(p, false, KEY_READ) else { return vec![] };
    let mut v = vec![];
    let mut buf = [0u16; 256];
    for i in 0.. {
        let mut n = buf.len() as u32;
        if unsafe { RegEnumKeyExW(k.0, i, buf.as_mut_ptr(), &mut n, null(), null_mut(), null_mut(), null_mut()) } != 0 {
            break;
        }
        v.push(String::from_utf16_lossy(&buf[..n as usize]));
    }
    v
}

fn reg_noms(p: &str) -> Vec<String> {
    let Ok(k) = ouvrir(p, false, KEY_READ) else { return vec![] };
    let mut v = vec![];
    let mut buf = vec![0u16; 16384];
    for i in 0.. {
        let mut n = buf.len() as u32;
        if unsafe { RegEnumValueW(k.0, i, buf.as_mut_ptr(), &mut n, null(), null_mut(), null_mut(), null_mut()) } != 0 {
            break;
        }
        v.push(String::from_utf16_lossy(&buf[..n as usize]));
    }
    v
}

// ---------- onglet 1 : tâches ----------

#[derive(Clone, Copy)]
struct Tache {
    groupe: &'static str,
    nom: &'static str,
    desc: &'static str,
    defaut: bool,
    run: fn(&Log) -> R,
    taille: Option<fn() -> i64>,
}

fn t(groupe: &'static str, nom: &'static str, desc: &'static str, defaut: bool, run: fn(&Log) -> R, taille: Option<fn() -> i64>) -> Tache {
    Tache { groupe, nom, desc, defaut, run, taille }
}

const GROUPES: [(&str, &str); 4] = [("clean", "Nettoyage"), ("privacy", "Confidentialité"), ("perf", "Performances"), ("comfort", "Confort")];

const TEMP: &[&str] = &[r"%TEMP%", r"%windir%\Temp"];
const WU: &[&str] = &[r"%windir%\SoftwareDistribution\Download"];
const DO_CACHE: &[&str] = &[r"%windir%\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache", r"%windir%\SoftwareDistribution\DeliveryOptimization"];
const WER: &[&str] = &[r"%ProgramData%\Microsoft\Windows\WER\ReportArchive", r"%ProgramData%\Microsoft\Windows\WER\ReportQueue", r"%windir%\Minidump", r"%windir%\MEMORY.DMP", r"%LOCALAPPDATA%\CrashDumps"];
const MINIATURES: &[&str] = &[r"%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db", r"%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db"];
const NAVIGATEURS: &[&str] = &[
    r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Cache", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\Code Cache", r"%LOCALAPPDATA%\Microsoft\Edge\User Data\*\GPUCache",
    r"%LOCALAPPDATA%\Google\Chrome\User Data\*\Cache", r"%LOCALAPPDATA%\Google\Chrome\User Data\*\Code Cache", r"%LOCALAPPDATA%\Google\Chrome\User Data\*\GPUCache",
    r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Cache", r"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data\*\Code Cache",
    r"%LOCALAPPDATA%\Vivaldi\User Data\*\Cache",
    r"%LOCALAPPDATA%\Opera Software\Opera Stable\Cache", r"%LOCALAPPDATA%\Opera Software\Opera GX Stable\Cache",
    r"%LOCALAPPDATA%\Mozilla\Firefox\Profiles\*\cache2",
];
const APPLIS: &[&str] = &[
    r"%APPDATA%\discord\Cache", r"%APPDATA%\discord\Code Cache", r"%APPDATA%\discord\GPUCache",
    r"%LOCALAPPDATA%\Spotify\Data", r"%LOCALAPPDATA%\Steam\htmlcache",
    r"%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\EBWebView\Default\Cache",
    r"%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\EBWebView\Default\Code Cache",
];
const SHADERS: &[&str] = &[r"%LOCALAPPDATA%\D3DSCache", r"%LOCALAPPDATA%\NVIDIA\DXCache", r"%LOCALAPPDATA%\NVIDIA\GLCache", r"%LOCALAPPDATA%\AMD\DxCache", r"%LOCALAPPDATA%\AMD\DxcCache", r"%LOCALAPPDATA%\AMD\GLCache"];
const JOURNAUX: &[&str] = &[r"%windir%\Logs\CBS\*.log", r"%windir%\Logs\CBS\*.cab", r"%windir%\Logs\DISM\*.log", r"%windir%\Logs\MeasuredBoot\*.log"];
const PILOTES: &[&str] = &[r"%ProgramData%\NVIDIA Corporation\Downloader", r"%SystemDrive%\AMD"];

const ADV: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const CDM: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const EDGE: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Edge";
const GAMEBAR: &str = r"HKCU\Software\Microsoft\GameBar";
const DESKTOP: &str = r"HKCU\Control Panel\Desktop";
const SVC_INUTILES: &[&str] = &["MapsBroker", "RetailDemo", "Fax", "WMPNetworkSvc"];

fn corbeille_taille() -> i64 {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    let masque = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| masque & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|d| unsafe { GetDriveTypeW(w(d).as_ptr()) } == 3) // DRIVE_FIXED
        .map(|d| poids_chemin(&Path::new(&d).join("$Recycle.Bin")))
        .sum()
}

fn taches() -> Vec<Tache> {
    vec![
        // ----- nettoyage -----
        t("clean", "Fichiers temporaires", "Dossiers Temp de l'utilisateur et de Windows.", true, |_| vider(TEMP), Some(|| poids(TEMP))),
        t("clean", "Cache de Windows Update", "Mises à jour déjà installées (téléchargements).", true, |_| {
            let _ = lancer("net.exe", &["stop", "wuauserv", "/y"]);
            let _ = lancer("net.exe", &["stop", "bits", "/y"]);
            vider(WU)?;
            let _ = lancer("net.exe", &["start", "bits"]);
            let _ = lancer("net.exe", &["start", "wuauserv"]);
            Ok(())
        }, Some(|| poids(WU))),
        t("clean", "Cache d'optimisation de distribution", "Morceaux de mises à jour partagés entre PC.", true, |_| ps("Delete-DeliveryOptimizationCache -Force").map(drop), Some(|| poids(DO_CACHE))),
        t("clean", "Rapports d'erreurs et vidages mémoire", "Rapports WER, minidumps, MEMORY.DMP, CrashDumps.", true, |_| vider(WER), Some(|| poids(WER))),
        t("clean", "Miniatures et cache d'icônes", "Recréés automatiquement par l'Explorateur.", true, |_| vider(MINIATURES), Some(|| poids(MINIATURES))),
        t("clean", "Cache des navigateurs", "Edge, Chrome, Brave, Vivaldi, Opera, Firefox (pas l'historique ni les mots de passe). Fermez-les avant.", true, |_| vider(NAVIGATEURS), Some(|| poids(NAVIGATEURS))),
        t("clean", "Cache des applications", "Discord, Spotify, Steam, Teams.", true, |_| vider(APPLIS), Some(|| poids(APPLIS))),
        t("clean", "Cache des shaders", "DirectX, NVIDIA, AMD. Les jeux recompileront leurs shaders.", false, |_| vider(SHADERS), Some(|| poids(SHADERS))),
        t("clean", "Journaux Windows", "Journaux CBS, DISM, MeasuredBoot.", true, |_| vider(JOURNAUX), Some(|| poids(JOURNAUX))),
        t("clean", "Installeurs de pilotes", "Fichiers d'installation NVIDIA / AMD déjà utilisés.", true, |_| vider(PILOTES), Some(|| poids(PILOTES))),
        t("clean", "Stockage réservé", "Libère l'espace réservé par Windows aux mises à jour (~7 Go).", false, |l| {
            let c = lancer("Dism.exe", &["/Online", "/Set-ReservedStorageState", "/State:Disabled"])?;
            l.ecrire(format!("   DISM : code {c}"));
            Ok(())
        }, None),
        t("clean", "Corbeille", "Vide la corbeille de tous les disques.", false, |_| {
            unsafe { windows_sys::Win32::UI::Shell::SHEmptyRecycleBinW(null_mut(), null(), 7) };
            Ok(())
        }, Some(corbeille_taille)),
        t("clean", "Composants Windows (WinSxS)", "Supprime les anciennes versions des composants (long).", true, |_| lancer_ok("Dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup", "/NoRestart"]), None),
        t("clean", "Ancienne installation (Windows.old)", "Empêche le retour à la version précédente de Windows.", false, |_| {
            let b = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches";
            for n in ["Previous Installations", "Temporary Setup Files", "Windows Upgrade Log Files"] {
                let p = format!(r"{b}\{n}");
                if existe(&p) {
                    reg_d(&p, "StateFlags0042", 2)?;
                }
            }
            lancer("cleanmgr.exe", &["/sagerun:42"]).map(drop)
        }, Some(|| if Path::new(&exp(r"%SystemDrive%\Windows.old")).is_dir() { poids(&[r"%SystemDrive%\Windows.old"]) } else { -1 })),
        t("clean", "Assistant de stockage", "Nettoyage automatique des fichiers temporaires.", true, |_| {
            dw(r"HKCU\Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy", &[("01", 1), ("04", 1), ("2048", 7)])
        }, None),
        // ----- confidentialité -----
        t("privacy", "Télémétrie", "Données de diagnostic au minimum, services et tâches de télémétrie coupés.", true, |_| {
            dw(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\DataCollection", &[("AllowTelemetry", 0), ("DoNotShowFeedbackNotifications", 1)])?;
            reg_d(r"HKCU\Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod", 0)?;
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", 0)?;
            reg_s(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment", "POWERSHELL_TELEMETRY_OPTOUT", "1")?;
            svc_off(&["DiagTrack", "dmwappushservice"])?;
            let _ = ps(r"Get-ScheduledTask -TaskPath '\Microsoft\Windows\Customer Experience Improvement Program\' -EA SilentlyContinue | Disable-ScheduledTask -EA SilentlyContinue | Out-Null
Get-ScheduledTask -TaskPath '\Microsoft\Windows\Application Experience\' -EA SilentlyContinue | Where-Object { $_.TaskName -like 'Microsoft Compatibility Appraiser*' } | Disable-ScheduledTask -EA SilentlyContinue | Out-Null");
            Ok(())
        }, None),
        t("privacy", "Identifiant publicitaire", "Plus de publicité ciblée dans les applis.", true, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0)?;
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "DisabledByGroupPolicy", 1)
        }, None),
        t("privacy", "Suggestions et applis installées sans demander", "Menu Démarrer, écran de verrouillage, conseils, raccourcis Edge.", true, |_| {
            for n in ["SilentInstalledAppsEnabled", "SystemPaneSuggestionsEnabled", "SoftLandingEnabled", "PreInstalledAppsEnabled", "OemPreInstalledAppsEnabled", "RotatingLockScreenOverlayEnabled",
                "SubscribedContent-338387Enabled", "SubscribedContent-338388Enabled", "SubscribedContent-338389Enabled", "SubscribedContent-310093Enabled", "SubscribedContent-353694Enabled", "SubscribedContent-353696Enabled"] {
                reg_d(CDM, n, 0)?;
            }
            dw(ADV, &[("Start_IrisRecommendations", 0), ("Start_AccountNotifications", 0), ("ShowSyncProviderNotifications", 0)])?;
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement", "ScoobeSystemSettingEnabled", 0)?;
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\EdgeUpdate", "CreateDesktopShortcutDefault", 0)?;
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\CloudContent", "DisableWindowsConsumerFeatures", 1)
        }, None),
        t("privacy", "Recherche Bing dans le menu Démarrer", "La recherche reste locale.", true, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search", "BingSearchEnabled", 0)?;
            reg_d(r"HKCU\Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1)
        }, None),
        t("privacy", "Historique et recherche dans le cloud", "Pas d'historique de recherche ni de résultats du compte Microsoft.", true, |_| {
            dw(r"HKCU\Software\Microsoft\Windows\CurrentVersion\SearchSettings", &[("IsDeviceSearchHistoryEnabled", 0), ("IsMSACloudSearchEnabled", 0), ("IsAADCloudSearchEnabled", 0)])
        }, None),
        t("privacy", "Liste des langues pour les sites", "Les sites ne voient plus vos langues installées.", true, |_| reg_d(r"HKCU\Control Panel\International\User Profile", "HttpAcceptLanguageOptOut", 1), None),
        t("privacy", "Localiser mon appareil", "Désactive Localiser mon appareil.", false, |_| reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\FindMyDevice", "AllowFindMyDevice", 0), None),
        t("privacy", "Widgets (actualités)", "Retire les widgets et actualités de la barre des tâches.", true, |_| reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests", 0), None),
        t("privacy", "Historique d'activités", "Windows ne garde plus la liste de ce que vous faites.", true, |_| {
            dw(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System", &[("EnableActivityFeed", 0), ("PublishUserActivities", 0), ("UploadUserActivities", 0)])?;
            reg_d(ADV, "Start_TrackProgs", 0)
        }, None),
        t("privacy", "Recall", "Pas de captures d'écran analysées par l'IA.", true, |_| {
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", 1)?;
            reg_d(r"HKCU\Software\Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", 1)
        }, None),
        t("privacy", "Copilot et IA intégrée", "Copilot, Click to Do, IA de Bloc-notes, Paint et Edge.", true, |_| {
            for r in [r"HKLM\SOFTWARE", r"HKCU\Software"] {
                reg_d(&format!(r"{r}\Policies\Microsoft\Windows\WindowsCopilot"), "TurnOffWindowsCopilot", 1)?;
                reg_d(&format!(r"{r}\Policies\Microsoft\Windows\WindowsAI"), "DisableClickToDo", 1)?;
            }
            reg_d(ADV, "ShowCopilotButton", 0)?;
            reg_d(EDGE, "HubsSidebarEnabled", 0)?;
            reg_d(r"HKLM\SOFTWARE\Policies\WindowsNotepad", "DisableAIFeatures", 1)?;
            dw(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Paint", &[("DisableCocreator", 1), ("DisableGenerativeFill", 1), ("DisableImageCreator", 1)])?;
            reg_s(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\systemAIModels", "Value", "Deny")?;
            svc_off(&["WSAIFabricSvc"])
        }, None),
        t("privacy", "Localisation", "Coupe la localisation et la mise à jour des cartes.", false, |_| {
            svc_off(&["lfsvc"])?;
            reg_s(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location", "Value", "Deny")?;
            reg_d(r"HKLM\SYSTEM\Maps", "AutoUpdateEnabled", 0)
        }, None),
        t("privacy", "Publicités et suivi d'Edge", "Recommandations, shopping, Rewards, contenu de la page d'accueil.", true, |_| {
            dw(EDGE, &[("ShowRecommendationsEnabled", 0), ("EdgeShoppingAssistantEnabled", 0), ("ShowMicrosoftRewards", 0), ("PersonalizationReportingEnabled", 0),
                ("NewTabPageContentEnabled", 0), ("DiagnosticData", 0), ("HideFirstRunExperience", 1)])
        }, None),
        // ----- performances -----
        t("perf", "Enregistrement en arrière-plan (Game DVR)", "Libère des ressources en jeu.", true, |_| {
            reg_d(r"HKCU\System\GameConfigStore", "GameDVR_Enabled", 0)?;
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\GameDVR", "AllowGameDVR", 0)?;
            reg_d(GAMEBAR, "ShowStartupPanel", 0)
        }, None),
        t("perf", "Mode jeu et planification GPU", "Mode jeu, planification GPU matérielle, optimisation des jeux fenêtrés.", false, |_| {
            dw(GAMEBAR, &[("AllowAutoGameMode", 1), ("AutoGameModeEnabled", 1)])?;
            reg_d(r"HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode", 2)?;
            let p = r"HKCU\Software\Microsoft\DirectX\UserGpuPreferences";
            let v = reg_texte(p, "DirectXUserGlobalSettings").unwrap_or_default();
            let v = Regex::new(r"SwapEffectUpgradeEnable=\d;?").unwrap().replace_all(&v, "").into_owned();
            reg_s(p, "DirectXUserGlobalSettings", &format!("{v}SwapEffectUpgradeEnable=1;"))
        }, None),
        t("perf", "Économie d'énergie des cartes réseau", "Windows ne coupe plus les cartes réseau (évite les déconnexions).", false, |_| {
            ps("Get-NetAdapter -Physical | Get-NetAdapterPowerManagement | ForEach-Object { $_.AllowComputerToTurnOffDevice = 'Disabled'; $_ | Set-NetAdapterPowerManagement }").map(drop)
        }, None),
        t("perf", "Services inutiles", "Cartes, démo magasin, fax, partage Windows Media.", true, |_| svc_off(SVC_INUTILES), None),
        t("perf", "Edge en arrière-plan", "Edge ne reste plus lancé après fermeture.", true, |_| dw(EDGE, &[("StartupBoostEnabled", 0), ("BackgroundModeEnabled", 0)]), None),
        t("perf", "Partage des mises à jour", "Les mises à jour ne sont plus envoyées à d'autres PC.", true, |_| reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", "DODownloadMode", 0), None),
        t("perf", "Délai au démarrage", "Les programmes de démarrage se lancent sans attendre.", true, |_| {
            dw(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize", &[("StartupDelayInMSec", 0), ("WaitForIdleState", 0)])
        }, None),
        t("perf", "Logiciels de fabricant automatiques", "Bloque les applis installées via les pilotes et le BIOS.", true, |_| {
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\Device Metadata", "PreventDeviceMetadataFromNetwork", 1)?;
            reg_d(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager", "DisableWpbtExecution", 1)
        }, None),
        t("perf", "Priorité aux jeux", "Plus de temps processeur et GPU pour les jeux.", false, |_| {
            let p = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
            dw(p, &[("SystemResponsiveness", 10), ("NetworkThrottlingIndex", 0xFFFF_FFFF)])?;
            let g = format!(r"{p}\Tasks\Games");
            dw(&g, &[("GPU Priority", 8), ("Priority", 6)])?;
            reg_s(&g, "Scheduling Category", "High")?;
            reg_s(&g, "SFIO Priority", "High")
        }, None),
        t("perf", "Indexation de la recherche", "Coupe l'indexation (recherche de fichiers plus lente).", false, |_| svc_off(&["WSearch"]), None),
        t("perf", "Démarrage rapide", "Désactive le démarrage rapide (vrai arrêt du PC).", false, |_| reg_d(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Power", "HiberbootEnabled", 0), None),
        t("perf", "Menus plus rapides", "Délai d'ouverture des menus réduit.", true, |_| reg_s(DESKTOP, "MenuShowDelay", "100"), None),
        t("perf", "Effets visuels réduits", "Moins d'animations et de transparence.", false, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects", "VisualFXSetting", 2)?;
            reg_b(DESKTOP, "UserPreferencesMask", &[0x90, 0x12, 0x03, 0x80, 0x10, 0, 0, 0])?;
            reg_s(DESKTOP, "FontSmoothing", "2")?;
            reg_s(r"HKCU\Control Panel\Desktop\WindowMetrics", "MinAnimate", "0")?;
            reg_d(ADV, "TaskbarAnimations", 0)?;
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "EnableTransparency", 0)?;
            reg_d(r"HKCU\Software\Microsoft\Windows\DWM", "EnableAeroPeek", 0)
        }, None),
        t("perf", "Mode d'alimentation performances optimales", "Active le plan Performances optimales.", false, |_| {
            const G: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
            if lancer("powercfg.exe", &["/setactive", G])? == 0 {
                return Ok(());
            }
            let o = Command::new("powercfg.exe").args(["/duplicatescheme", G]).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| e.to_string())?;
            let s = String::from_utf8_lossy(&o.stdout);
            let g = Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}").unwrap()
                .find(&s).ok_or_else(|| "plan d'alimentation introuvable".to_string())?.as_str().to_string();
            lancer_ok("powercfg.exe", &["/setactive", &g])
        }, None),
        t("perf", "Veille prolongée", "Supprime hiberfil.sys (plus de veille prolongée).", false, |_| lancer_ok("powercfg.exe", &["/hibernate", "off"]), Some(|| {
            fs::read_dir(exp(r"%SystemDrive%\")).ok()
                .and_then(|r| r.flatten().find(|e| e.file_name().eq_ignore_ascii_case("hiberfil.sys")))
                .and_then(|e| e.metadata().ok()).map_or(-1, |m| m.len() as i64)
        })),
        // ----- confort -----
        t("comfort", "Afficher les extensions de fichiers", "Voir .exe, .pdf... dans l'Explorateur.", true, |_| reg_d(ADV, "HideFileExt", 0), None),
        t("comfort", "Fin de tâche dans la barre des tâches", "Clic droit sur une appli : Fin de tâche.", true, |_| reg_d(&format!(r"{ADV}\TaskbarDeveloperSettings"), "TaskbarEndTask", 1), None),
        t("comfort", "Touches rémanentes", "Plus de fenêtre en appuyant 5 fois sur Maj.", true, |_| {
            reg_s(r"HKCU\Control Panel\Accessibility\StickyKeys", "Flags", "506")?;
            reg_s(r"HKCU\Control Panel\Accessibility\Keyboard Response", "Flags", "122")?;
            reg_s(r"HKCU\Control Panel\Accessibility\ToggleKeys", "Flags", "58")?;
            #[repr(C)]
            struct StickyKeys {
                cb: u32,
                flags: u32,
            }
            let mut s = StickyKeys { cb: 8, flags: 506 };
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(0x3B, 8, &mut s as *mut _ as *mut _, 3) };
            Ok(())
        }, None),
        t("comfort", "Pas de redémarrage auto pendant l'utilisation", "Windows Update attend que vous soyez déconnecté.", true, |_| {
            reg_d(r"HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU", "NoAutoRebootWithLoggedOnUsers", 1)
        }, None),
        t("comfort", "Ancien menu clic droit", "Menu contextuel complet de Windows 10.", false, |_| {
            reg_s(r"HKCU\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32", "", "")
        }, None),
        t("comfort", "Souris sans accélération", "Mouvement de la souris plus précis.", false, |_| {
            ["MouseSpeed", "MouseThreshold1", "MouseThreshold2"].iter().try_for_each(|n| reg_s(r"HKCU\Control Panel\Mouse", n, "0"))
        }, None),
        t("comfort", "Explorateur ouvert sur Ce PC", "Au lieu de l'Accueil.", false, |_| reg_d(ADV, "LaunchTo", 1), None),
        t("comfort", "Barre des tâches épurée", "Retire la zone de recherche et le bouton Vue des tâches.", false, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search", "SearchboxTaskbarMode", 0)?;
            reg_d(ADV, "ShowTaskViewButton", 0)
        }, None),
        t("comfort", "Retirer Accueil et Galerie", "De la colonne de gauche de l'Explorateur.", false, |_| {
            for c in ["{f874310e-b6b7-47dc-bc84-b9e6b38f5903}", "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"] {
                reg_d(&format!(r"HKCU\Software\Classes\CLSID\{c}"), "System.IsPinnedToNameSpaceTree", 0)?;
            }
            reg_d(ADV, "LaunchTo", 1)
        }, None),
        t("comfort", "Exécution automatique", "Rien ne se lance tout seul en branchant une clé USB.", true, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers", "DisableAutoplay", 1)
        }, None),
        t("comfort", "Fichiers récents dans l'Explorateur", "N'affiche plus les fichiers et dossiers récents.", false, |_| {
            dw(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer", &[("ShowRecent", 0), ("ShowFrequent", 0)])
        }, None),
        t("comfort", "Raccourcis sans « - Raccourci »", "Nom des nouveaux raccourcis.", false, |_| {
            reg_s(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\NamingTemplates", "ShortcutNameTemplate", "%s.lnk")
        }, None),
        t("comfort", "Imprimante par défaut fixe", "Windows ne change plus l'imprimante par défaut.", false, |_| {
            reg_d(r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\Windows", "LegacyDefaultPrinterMode", 1)
        }, None),
        t("comfort", "Chemins longs", "Autorise les chemins de plus de 260 caractères.", false, |_| reg_d(r"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled", 1), None),
        t("comfort", "Verr. Num au démarrage", "Pavé numérique actif dès l'écran de connexion.", false, |_| {
            reg_s(r"HKCU\Control Panel\Keyboard", "InitialKeyboardIndicators", "2")?;
            reg_s(r"HKU\.DEFAULT\Control Panel\Keyboard", "InitialKeyboardIndicators", "2")
        }, None),
    ]
}

fn point_restauration() -> R {
    reg_d(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore", "SystemRestorePointCreationFrequency", 0)?;
    ps("Enable-ComputerRestore -Drive \"$env:SystemDrive\\\"\nCheckpoint-Computer -Description 'Avant Optimiseur Windows' -RestorePointType MODIFY_SETTINGS").map(drop)
}

/// Point de restauration une fois par session ; false si l'utilisateur abandonne.
fn point_si_besoin(l: &Log, voulu: bool) -> bool {
    if !voulu || l.etat().point_fait {
        return true;
    }
    l.ecrire("Création du point de restauration...");
    match point_restauration() {
        Ok(()) => {
            l.etat().point_fait = true;
            l.ecrire("   OK");
            true
        }
        Err(e) => {
            l.ecrire(format!("   ERREUR : {e}"));
            oui_non("Le point de restauration n'a pas pu être créé. Continuer quand même ?")
        }
    }
}

fn analyser(l: &Log, taches: &[Tache]) {
    l.etat().tailles = vec![None; taches.len()];
    for (i, t) in taches.iter().enumerate() {
        if let Some(f) = t.taille {
            let v = f();
            l.etat().tailles[i] = Some(v);
        }
    }
}

// ---------- onglet 2 : applications ----------

const RECO: &[&str] = &[
    "Microsoft.BingNews", "Microsoft.BingWeather", "Microsoft.BingSearch", "Microsoft.Getstarted", "Microsoft.GetHelp",
    "Microsoft.MicrosoftSolitaireCollection", "Microsoft.WindowsFeedbackHub", "Microsoft.MicrosoftOfficeHub", "Clipchamp.Clipchamp",
    "Microsoft.WindowsMaps", "Microsoft.People", "Microsoft.ZuneVideo", "Microsoft.PowerAutomateDesktop", "Microsoft.SkypeApp",
    "Microsoft.549981C3F5F10", "Microsoft.MSPaint", "Microsoft.Microsoft3DViewer", "Microsoft.MixedReality.Portal", "Microsoft.Windows.DevHome",
    "*CandyCrush*", "*BubbleWitch*", "*MarchofEmpires*", "*Disney*", "*TikTok*", "*Facebook*", "*Instagram*", "*Twitter*", "*LinkedIn*",
];
const APPS_PROTEGEES: &[&str] = &[
    "Microsoft.WindowsStore", "Microsoft.StorePurchaseApp", "Microsoft.DesktopAppInstaller", "Microsoft.SecHealthUI",
    "Microsoft.Services.Store.Engagement", "Microsoft.MicrosoftEdge*", "Microsoft.WindowsAppRuntime*", "Microsoft.Winget.*", "Microsoft.VCLibs*",
    "Microsoft.UI.Xaml*", "Microsoft.NET.*", "*Extension", "*Extensions", "Microsoft.LanguageExperiencePack*",
    "MicrosoftWindows.Speech.*", "MicrosoftWindows.Voice.*", "Microsoft.ApplicationCompatibilityEnhancements", "Microsoft.XboxIdentityProvider",
];
const PROGRAMMES_PROTEGES: &[&str] = &[
    "Microsoft Visual C++*", "Microsoft .NET*", "Microsoft Windows Desktop Runtime*", "Microsoft ASP.NET*",
    "Microsoft Edge*", "Microsoft Update Health Tools", "Windows Software Development Kit*",
    "Microsoft Windows Application Compatibility*", "*Pilote graphique*", "*Graphics Driver*",
];
const GROUPES_APPS: [&str; 3] = [
    "Recommandé de retirer (préinstallé, inutile pour la plupart des gens)",
    "Applications du Microsoft Store (réinstallables depuis le Store)",
    "Programmes installés (l'assistant de désinstallation de chaque programme s'ouvrira)",
];

fn protege(n: &str, liste: &[&str]) -> bool {
    liste.iter().any(|m| like(n, m))
}

#[derive(Clone)]
struct Appli {
    nom: String,
    editeur: String,
    taille: i64,
    groupe: u8, // 0 reco, 1 store, 2 win32
    id: String,
    paquets: Vec<String>,
    commande: String,
    coche: bool,
}

/// Noms affichés du menu Démarrer, par nom de famille de paquet.
const PS_START: &str = r#"$start = @{}
try { Get-StartApps | ForEach-Object { if ($_.AppID -match '^([^!\\]+_[a-z0-9]{13})!') { if (-not $start.ContainsKey($matches[1])) { $start[$matches[1]] = $_.Name } } } } catch {}
"#;

const PS_APPS: &str = r#"try { $p = @(Get-AppxPackage -AllUsers) } catch { $p = @(Get-AppxPackage) }
$r = @($p | Where-Object { -not $_.IsFramework -and -not $_.IsResourcePackage -and -not $_.NonRemovable -and "$($_.SignatureKind)" -ne 'System' } | ForEach-Object {
  $s = $false
  try { if ($_.InstallLocation) { $m = Join-Path $_.InstallLocation 'AppxManifest.xml'; if (Test-Path -LiteralPath $m) { $s = (Get-Content -LiteralPath $m -Raw) -match '<(\w+:)?AllowExternalContent>\s*true' } } } catch {}
  [pscustomobject]@{ n = $_.Name; f = $_.PackageFullName; pub = "$($_.Publisher)"; loc = "$($_.InstallLocation)"; s = $s; d = $start[$_.PackageFamilyName] }
})
ConvertTo-Json -InputObject $r -Compress
"#;

fn editeur(publisher: &str) -> String {
    Regex::new(r"^CN=([^,]+).*").unwrap().replace(publisher, "$1").into_owned()
}

fn lister_apps() -> Vec<Appli> {
    let mut v = vec![];
    if let Ok(Value::Array(a)) = ps(&[PS_START, PS_APPS].concat()).map(|s| serde_json::from_str(s.trim()).unwrap_or(Value::Null)) {
        let mut par_nom: BTreeMap<String, (Appli, Vec<String>)> = BTreeMap::new();
        for p in &a {
            let s = |k: &str| p[k].as_str().unwrap_or("").to_string();
            let n = s("n");
            if n.is_empty() || p["s"].as_bool() == Some(true) || protege(&n, APPS_PROTEGEES) {
                continue;
            }
            let (app, locs) = par_nom.entry(n.clone()).or_insert_with(|| {
                let d = s("d");
                (Appli { nom: if d.is_empty() { n.clone() } else { d }, editeur: editeur(&s("pub")), taille: 0, groupe: if protege(&n, RECO) { 0 } else { 1 },
                    id: n.clone(), paquets: vec![], commande: String::new(), coche: false }, vec![])
            });
            app.paquets.push(s("f"));
            let loc = s("loc");
            if !loc.is_empty() && !locs.contains(&loc) {
                locs.push(loc);
            }
        }
        for (_, (mut app, locs)) in par_nom {
            app.taille = locs.iter().map(|l| poids_chemin(Path::new(l))).sum();
            v.push(app);
        }
    }
    let mut vus = HashSet::new();
    for base in [r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall"] {
        for k in reg_sous_cles(base) {
            let p = format!(r"{base}\{k}");
            let txt = |n: &str| reg_texte(&p, n).filter(|s| !s.trim().is_empty());
            let (Some(nom), Some(un)) = (txt("DisplayName"), txt("UninstallString")) else { continue };
            if reg_dword(&p, "SystemComponent") == Some(1) || txt("ParentKeyName").is_some()
                || txt("ReleaseType").is_some_and(|r| ["Update", "Hotfix", "Security Update"].contains(&r.as_str()))
                || protege(&nom, PROGRAMMES_PROTEGES) || !vus.insert(nom.to_lowercase()) {
                continue;
            }
            v.push(Appli { editeur: txt("Publisher").unwrap_or_default(), taille: reg_dword(&p, "EstimatedSize").map_or(-1, |k| k as i64 * 1024), groupe: 2,
                id: k, paquets: vec![], commande: txt("QuietUninstallString").unwrap_or(un), coche: false, nom });
        }
    }
    v.sort_by_key(|a| a.nom.to_lowercase());
    v
}

/// Sépare l'exe et ses arguments d'une commande de désinstallation.
fn split_commande(c: &str) -> (String, String) {
    let s = exp(c.trim());
    for re in [r#"^"([^"]+)"\s*(.*)$"#, r"(?i)^(.+?\.exe)\s*(.*)$", r"^(\S+)\s*(.*)$"] {
        if let Some(m) = Regex::new(re).unwrap().captures(&s) {
            return (m[1].to_string(), m[2].to_string());
        }
    }
    (s, String::new())
}

fn desinstaller_app(a: &Appli, l: &Log) -> R {
    if a.groupe < 2 {
        let paquets = a.paquets.iter().map(|p| format!("'{}'", q(p))).collect::<Vec<_>>().join(",");
        let script = format!(r#"$n = '{}'
foreach ($f in @({paquets})) {{ try {{ Remove-AppxPackage -Package $f -AllUsers }} catch {{ try {{ Remove-AppxPackage -Package $f }} catch {{}} }} }}
try {{ $r = @(Get-AppxPackage -AllUsers -Name $n) }} catch {{ $r = @(Get-AppxPackage -Name $n) }}
try {{ Get-AppxProvisionedPackage -Online | Where-Object {{ $_.DisplayName -eq $n }} | Remove-AppxProvisionedPackage -Online | Out-Null }} catch {{}}
$r.Count"#, q(&a.id));
        return match ps(&script)?.trim() {
            "0" => Ok(()),
            _ => Err(format!("Windows a refusé de retirer {}", a.nom)),
        };
    }
    let guid = Regex::new(r"(?i)\{[0-9a-f-]{36}\}").unwrap().find(&a.commande).map(|m| m.as_str().to_string());
    let st = match guid {
        Some(g) if a.commande.to_lowercase().contains("msiexec") => Command::new("msiexec.exe").args(["/x", &g, "/qb", "/norestart"]).status(),
        _ => {
            l.ecrire("   (suivez l'assistant de désinstallation qui s'ouvre)");
            let (exe, args) = split_commande(&a.commande);
            Command::new(exe).raw_arg(args).status()
        }
    };
    match st.map_err(|e| e.to_string())?.code().unwrap_or(-1) {
        0 | 1605 | 3010 => Ok(()),
        c => Err(format!("le désinstalleur a renvoyé le code {c}")),
    }
}

fn liste_noms(noms: &[String]) -> String {
    let mut s: Vec<String> = noms.iter().take(15).map(|n| format!(" - {n}")).collect();
    if noms.len() > 15 {
        s.push(format!(" ... et {} autre(s)", noms.len() - 15));
    }
    s.join("\n")
}

// ---------- onglet 3 : démarrage et arrière-plan ----------

const APPROUVE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
const BG: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications";
const POLITIQUE: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";
const REFUS: &str = "LetAppsRunInBackground_ForceDenyTheseApps";
const TACHES_PROTEGEES: &[&str] = &["MicrosoftEdgeUpdate*", "CreateExplorerShellUnelevatedTask", "RtkAud*"];
const GROUPES_DEMARRAGE: [&str; 3] = [
    "Lancés au démarrage de Windows",
    "Tâches planifiées (programmes de mise à jour, assistants...)",
    "Applications du Store autorisées à tourner en arrière-plan",
];

#[derive(Clone)]
enum Source {
    Approuve { cle: String, valeur: String },
    EtatStore(String),
    Planifiee(String),
    Fond(String),
}

#[derive(Clone)]
struct Entree {
    nom: String,
    type_: &'static str,
    detail: String,
    groupe: u8,
    desactive: bool,
    coche: bool,
    source: Source,
}

const PS_DEMARRAGE: &str = r#"$t = @()
try { $t = @(Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\Microsoft\*' } | ForEach-Object { [pscustomobject]@{ p = $_.TaskPath; n = $_.TaskName; e = ((@($_.Actions) | ForEach-Object { $_.Execute } | Where-Object { $_ }) -join ' ; '); d = ("$($_.State)" -eq 'Disabled') } }) } catch {}
$b = @()
try { $b = @(Get-AppxPackage | Where-Object { -not $_.IsFramework -and "$($_.SignatureKind)" -ne 'System' -and $start.ContainsKey($_.PackageFamilyName) } | ForEach-Object { [pscustomobject]@{ pfn = $_.PackageFamilyName; n = $_.Name } }) } catch {}
ConvertTo-Json -Compress -Depth 4 -InputObject ([pscustomobject]@{ start = $start; taches = $t; fond = $b })
"#;

fn sans_suffixe(pfn: &str) -> String {
    Regex::new(r"_[a-z0-9]{13}$").unwrap().replace(pfn, "").into_owned()
}

fn maj_liste(mut l: Vec<String>, pfn: &str, ajouter: bool) -> Vec<String> {
    l.retain(|x| x != pfn);
    if ajouter {
        l.push(pfn.to_string());
    }
    l
}

fn approuve_off(cle: &str, valeur: &str) -> bool {
    matches!(reg_lire(cle, valeur), Some((_, b)) if b.first().is_some_and(|x| x & 1 == 1))
}

fn lister_demarrage(l: &Log) -> Vec<Entree> {
    let j: Value = ps(&[PS_START, PS_DEMARRAGE].concat()).ok().and_then(|s| serde_json::from_str(s.trim()).ok()).unwrap_or(Value::Null);
    let start: HashMap<String, String> = j["start"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()).unwrap_or_default();
    let mut v = vec![];
    let entree = |nom: String, type_, detail, groupe, desactive, source| Entree { nom, type_, detail, groupe, desactive, coche: false, source };
    for (run, appr, type_) in [
        (r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run".to_string(), format!(r"HKCU\{APPROUVE}\Run"), "Démarrage (utilisateur)"),
        (r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run".to_string(), format!(r"HKLM\{APPROUVE}\Run"), "Démarrage (tous les comptes)"),
        (r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run".to_string(), format!(r"HKLM\{APPROUVE}\Run32"), "Démarrage (32 bits)"),
    ] {
        for n in reg_noms(&run) {
            if n.is_empty() || n == "SecurityHealth" {
                continue;
            }
            let cmd = reg_texte(&run, &n).unwrap_or_default();
            v.push(entree(n.clone(), type_, cmd, 0, approuve_off(&appr, &n), Source::Approuve { cle: appr.clone(), valeur: n }));
        }
    }
    for (dossier, appr, type_) in [
        (exp(r"%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup"), format!(r"HKCU\{APPROUVE}\StartupFolder"), "Dossier Démarrage"),
        (exp(r"%ProgramData%\Microsoft\Windows\Start Menu\Programs\StartUp"), format!(r"HKLM\{APPROUVE}\StartupFolder"), "Dossier Démarrage (tous)"),
    ] {
        let Ok(rd) = fs::read_dir(&dossier) else { continue };
        for e in rd.flatten().filter(|e| e.path().is_file()) {
            let f = e.file_name().to_string_lossy().into_owned();
            if f.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let nom = f.strip_suffix(".lnk").or_else(|| f.strip_suffix(".LNK")).unwrap_or(&f).to_string();
            v.push(entree(nom, type_, e.path().display().to_string(), 0, approuve_off(&appr, &f), Source::Approuve { cle: appr.clone(), valeur: f }));
        }
    }
    let base = r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData";
    for pfn in reg_sous_cles(base) {
        for t in reg_sous_cles(&format!(r"{base}\{pfn}")) {
            let p = format!(r"{base}\{pfn}\{t}");
            let Some(s) = reg_dword(&p, "State") else { continue };
            let nom = start.get(&pfn).cloned().unwrap_or_else(|| sans_suffixe(&pfn));
            v.push(entree(nom, "Démarrage (app du Store)", format!(r"{pfn}\{t}"), 0, s != 2 && s != 4, Source::EtatStore(p)));
        }
    }
    let nb_demarrage = v.len();
    for t in j["taches"].as_array().into_iter().flatten() {
        let (p, n) = (t["p"].as_str().unwrap_or(""), t["n"].as_str().unwrap_or(""));
        if n.is_empty() || protege(n, TACHES_PROTEGEES) {
            continue;
        }
        let e = t["e"].as_str().filter(|s| !s.is_empty()).unwrap_or(p).to_string();
        v.push(entree(n.to_string(), "Tâche planifiée", e, 1, t["d"].as_bool() == Some(true), Source::Planifiee(format!("{p}{n}"))));
    }
    let nb_taches = v.len() - nb_demarrage;
    let refus = reg_multi(POLITIQUE, REFUS);
    let tout_refuse = reg_dword(POLITIQUE, "LetAppsRunInBackground") == Some(2);
    if tout_refuse {
        l.ecrire("Note : une stratégie interdit déjà toutes les applis en arrière-plan.");
    }
    let mut vus = HashSet::new();
    for b in j["fond"].as_array().into_iter().flatten() {
        let (pfn, n) = (b["pfn"].as_str().unwrap_or(""), b["n"].as_str().unwrap_or(""));
        if pfn.is_empty() || protege(n, APPS_PROTEGEES) || !vus.insert(pfn.to_string()) {
            continue;
        }
        let off = reg_dword(&format!(r"{BG}\{pfn}"), "Disabled") == Some(1) || refus.iter().any(|r| r == pfn) || tout_refuse;
        let nom = start.get(pfn).cloned().unwrap_or_else(|| n.to_string());
        v.push(entree(nom, "Arrière-plan (app du Store)", pfn.to_string(), 2, off, Source::Fond(pfn.to_string())));
    }
    l.ecrire(format!("{nb_demarrage} programme(s) au démarrage, {nb_taches} tâche(s) planifiée(s), {} appli(s) du Store pouvant tourner en arrière-plan.", vus.len()));
    v
}

fn basculer(s: &Source, off: bool) -> R {
    match s {
        Source::Approuve { cle, valeur } => {
            let mut b = [0u8; 12];
            b[0] = if off { 3 } else { 2 };
            reg_b(cle, valeur, &b)
        }
        Source::EtatStore(p) => reg_d(p, "State", if off { 1 } else { 2 }),
        Source::Planifiee(chemin) => lancer_ok("schtasks.exe", &["/Change", "/TN", chemin, if off { "/DISABLE" } else { "/ENABLE" }]),
        Source::Fond(pfn) => {
            let k = format!(r"{BG}\{pfn}");
            reg_d(&k, "Disabled", off as u32)?;
            reg_d(&k, "DisabledByUser", off as u32)?;
            let avant = reg_multi(POLITIQUE, REFUS);
            let apres = maj_liste(avant.clone(), pfn, off);
            if apres == avant {
                Ok(())
            } else if apres.is_empty() {
                reg_suppr(POLITIQUE, REFUS)
            } else {
                reg_m(POLITIQUE, REFUS, &apres)
            }
        }
    }
}

// ---------- interface ----------

pub struct Optimiseur {
    log: Log,
    taches: Vec<Tache>,
    coches: Vec<bool>,
    onglet: u8,
    point: bool,
    filtre: String,
    admin: bool,
    analyse_lancee: bool,
    apps_lancees: bool,
    demarrage_lance: bool,
}

impl Optimiseur {
    pub fn new() -> Self {
        let taches = taches();
        let docs = reg_texte(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders", "Personal")
            .unwrap_or_else(|| exp(r"%USERPROFILE%\Documents"));
        let t = local();
        let fichier = Path::new(&docs).join(format!("OptimiseurWindows_{:04}-{:02}-{:02}_{:02}-{:02}.log", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute));
        let etat = Etat { fichier, tailles: vec![None; taches.len()], ..Default::default() };
        Optimiseur { log: Log(Arc::new(Mutex::new(etat))), coches: vec![false; taches.len()], taches, onglet: 0, point: true, filtre: String::new(),
            admin: est_admin(), analyse_lancee: false, apps_lancees: false, demarrage_lance: false }
    }

    /// Lance un travail en arrière-plan ; false si un autre tourne déjà.
    fn travail(&self, f: impl FnOnce(&Log) + Send + 'static) -> bool {
        {
            let mut e = self.log.etat();
            if e.occupe {
                return false;
            }
            e.occupe = true;
        }
        let l = self.log.clone();
        std::thread::spawn(move || {
            f(&l);
            l.etat().occupe = false;
        });
        true
    }

    fn analyser(&self) -> bool {
        let t = self.taches.clone();
        self.travail(move |l| analyser(l, &t))
    }

    fn optimiser(&self) {
        let tailles = self.log.etat().tailles.clone();
        let sel: Vec<(Tache, Option<i64>)> = (0..self.taches.len()).filter(|&i| self.coches[i]).map(|i| (self.taches[i], tailles[i])).collect();
        if sel.is_empty() || !confirmer(&format!("{} action(s) vont être appliquées.\n\nFermez vos navigateurs et enregistrez votre travail avant de continuer.", sel.len())) {
            return;
        }
        let (point, toutes) = (self.point, self.taches.clone());
        self.travail(move |l| {
            if !point_si_besoin(l, point) {
                return;
            }
            let mut total = 0;
            for (t, avant) in sel {
                l.ecrire(format!("-> {}", t.nom));
                match (t.run)(l) {
                    Ok(()) => {
                        let gain = match (t.taille, avant) {
                            (Some(f), Some(a)) if a > 0 => (a - f()).max(0),
                            _ => 0,
                        };
                        total += gain;
                        l.ecrire(if gain > 1 << 20 { format!("   OK  (+{})", format_taille(gain)) } else { "   OK".into() });
                    }
                    Err(e) => l.ecrire(format!("   ERREUR : {e}")),
                }
            }
            let f = l.etat().fichier.display().to_string();
            l.ecrire(format!("Terminé. Espace libéré : {}. Journal : {f}", format_taille(total)));
            crate::systeme::notifier("Optimiseur", &format!("Nettoyage terminé : {} libérés.", format_taille(total)));
            analyser(l, &toutes);
            if oui_non("Certains réglages ne s'appliquent qu'après un redémarrage.\n\nRedémarrer maintenant ?") {
                let _ = lancer("shutdown.exe", &["/r", "/t", "0"]);
            }
        });
    }

    fn desinstaller(&self) {
        let sel: Vec<Appli> = self.log.etat().apps.iter().flatten().filter(|a| a.coche).cloned().collect();
        let noms: Vec<String> = sel.iter().map(|a| a.nom.clone()).collect();
        if sel.is_empty() || !confirmer(&format!("Ces {} application(s) vont être désinstallées :\n\n{}", sel.len(), liste_noms(&noms))) {
            return;
        }
        let point = self.point;
        self.travail(move |l| {
            if !point_si_besoin(l, point) {
                return;
            }
            let mut ok = 0;
            for a in &sel {
                l.ecrire(format!("-> {}", a.nom));
                match desinstaller_app(a, l) {
                    Ok(()) => {
                        ok += 1;
                        l.ecrire("   OK");
                    }
                    Err(e) => l.ecrire(format!("   ERREUR : {e}")),
                }
            }
            l.ecrire(format!("{ok} / {} application(s) désinstallée(s).", sel.len()));
            if sel.iter().any(|a| a.groupe == 2) {
                info("Terminez les fenêtres de désinstallation encore ouvertes, puis cliquez sur OK pour actualiser la liste.");
            }
            let v = lister_apps();
            l.etat().apps = Some(v);
        });
    }

    fn charger_apps(&self) -> bool {
        self.travail(|l| {
            let v = lister_apps();
            l.etat().apps = Some(v);
        })
    }

    fn charger_demarrage(&self) -> bool {
        self.travail(|l| {
            let v = lister_demarrage(l);
            l.etat().entrees = Some(v);
        })
    }

    fn basculer_selection(&self, off: bool) {
        let sel: Vec<Entree> = self.log.etat().entrees.iter().flatten().filter(|e| e.coche && e.desactive != off).cloned().collect();
        if sel.is_empty() {
            return;
        }
        let point = self.point;
        self.travail(move |l| {
            if !point_si_besoin(l, point) {
                return;
            }
            for e in &sel {
                match basculer(&e.source, off) {
                    Ok(()) => l.ecrire(format!("-> {} : {}", e.nom, if off { "désactivé" } else { "réactivé" })),
                    Err(err) => l.ecrire(format!("-> {} : ERREUR : {err}", e.nom)),
                }
            }
            l.ecrire("Les changements prendront effet au prochain démarrage de Windows.");
            let v = lister_demarrage(l);
            l.etat().entrees = Some(v);
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.analyse_lancee {
            self.analyse_lancee = self.analyser();
        }
        ui.heading("Optimiseur Windows");
        if !self.admin {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::YELLOW, "Sans droits administrateur, les réglages système (HKLM, services, DISM, applis) échoueront.");
                if ui.button("Relancer en administrateur").clicked() && relancer_admin() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.onglet, 0, "Nettoyage et réglages");
            ui.selectable_value(&mut self.onglet, 1, "Applications");
            ui.selectable_value(&mut self.onglet, 2, "Démarrage et arrière-plan");
            ui.separator();
            ui.checkbox(&mut self.point, "Point de restauration avant toute modification");
        });
        ui.separator();
        match self.onglet {
            1 if !self.apps_lancees => self.apps_lancees = self.charger_apps(),
            2 if !self.demarrage_lance => self.demarrage_lance = self.charger_demarrage(),
            _ => {}
        }
        let occupe = self.log.etat().occupe;
        let h = (ui.available_height() - 190.0).max(120.0);
        match self.onglet {
            0 => self.ui_taches(ui, occupe, h),
            1 => self.ui_apps(ui, occupe, h),
            _ => self.ui_demarrage(ui, occupe, h),
        }
        ui.separator();
        let journal = self.log.etat().journal.clone();
        egui::ScrollArea::vertical().id_salt("journal_opti").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for l in &journal {
                ui.monospace(l);
            }
        });
    }

    fn ui_taches(&mut self, ui: &mut egui::Ui, occupe: bool, h: f32) {
        let tailles = self.log.etat().tailles.clone();
        let dispo = |i: usize| !matches!(tailles.get(i), Some(Some(v)) if *v < 0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("Optimiser maintenant")).clicked() {
                self.optimiser();
            }
            if ui.add_enabled(!occupe, egui::Button::new("Analyser")).clicked() {
                self.analyser();
            }
            if ui.button("Sélection recommandée").clicked() {
                for i in 0..self.taches.len() {
                    self.coches[i] = self.taches[i].defaut && dispo(i);
                }
            }
            if ui.button("Tout décocher").clicked() {
                self.coches.iter_mut().for_each(|c| *c = false);
            }
            if occupe {
                ui.spinner();
            }
        });
        if let Some((libre, total)) = disque() {
            ui.horizontal(|ui| {
                ui.label(format!("Disque {} {} libres sur {}", exp("%SystemDrive%"), format_taille(libre as i64), format_taille(total as i64)));
                ui.add(egui::ProgressBar::new(1.0 - libre as f32 / total.max(1) as f32).desired_width(220.0));
            });
        }
        let somme: i64 = (0..self.taches.len()).filter(|&i| self.coches[i]).filter_map(|i| tailles[i]).filter(|v| *v > 0).sum();
        let winsxs = self.taches.iter().zip(&self.coches).any(|(t, c)| *c && t.nom.contains("WinSxS"));
        ui.label(format!("Nettoyage sélectionné : {}{}. Analysez, cochez, puis optimisez.", format_taille(somme), if winsxs { " (+ WinSxS)" } else { "" }));
        egui::ScrollArea::vertical().id_salt("taches_opti").max_height(h).auto_shrink([false, true]).show(ui, |ui| {
            for (g, titre) in GROUPES {
                ui.add_space(6.0);
                ui.strong(titre);
                for i in (0..self.taches.len()).filter(|&i| self.taches[i].groupe == g) {
                    let t = self.taches[i];
                    if !dispo(i) {
                        self.coches[i] = false;
                    }
                    ui.horizontal(|ui| {
                        ui.add_enabled(dispo(i), egui::Checkbox::new(&mut self.coches[i], t.nom)).on_hover_text(t.desc);
                        let txt = match (t.taille, tailles[i]) {
                            (None, _) => String::new(),
                            (Some(_), None) => if occupe { "analyse...".into() } else { String::new() },
                            (Some(_), Some(v)) if v < 0 => "Non présent".into(),
                            (Some(_), Some(v)) => format_taille(v),
                        };
                        ui.weak(txt);
                    });
                }
            }
        });
    }

    fn ui_apps(&mut self, ui: &mut egui::Ui, occupe: bool, h: f32) {
        let mut action = 0;
        {
            let mut e = self.log.etat();
            let Some(apps) = e.apps.as_mut() else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Chargement des applications...");
                });
                return;
            };
            let n = apps.iter().filter(|a| a.coche).count();
            let poids: i64 = apps.iter().filter(|a| a.coche && a.taille > 0).map(|a| a.taille).sum();
            ui.horizontal(|ui| {
                if ui.add_enabled(!occupe && n > 0, egui::Button::new("Désinstaller la sélection")).clicked() {
                    action = 1;
                }
                if ui.add_enabled(!occupe, egui::Button::new("Actualiser")).clicked() {
                    action = 2;
                }
                if ui.button("Sélection recommandée").clicked() {
                    apps.iter_mut().for_each(|a| a.coche = a.groupe == 0);
                }
                if ui.button("Tout décocher").clicked() {
                    apps.iter_mut().for_each(|a| a.coche = false);
                }
                if occupe {
                    ui.spinner();
                }
            });
            ui.horizontal(|ui| {
                ui.label("Filtrer :");
                ui.text_edit_singleline(&mut self.filtre);
                ui.label(format!("{n} application(s) cochée(s) - environ {}", format_taille(poids)));
            });
            let f = self.filtre.to_lowercase();
            egui::ScrollArea::vertical().id_salt("apps_opti").max_height(h).auto_shrink([false, true]).show(ui, |ui| {
                for (g, titre) in GROUPES_APPS.iter().enumerate() {
                    let mut lignes = apps.iter_mut().filter(|a| a.groupe == g as u8 && (a.nom.to_lowercase().contains(&f) || a.editeur.to_lowercase().contains(&f))).peekable();
                    if lignes.peek().is_none() {
                        continue;
                    }
                    ui.add_space(6.0);
                    ui.strong(*titre);
                    for a in lignes {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut a.coche, &a.nom);
                            ui.weak(&a.editeur);
                            if a.taille > 0 {
                                ui.weak(format_taille(a.taille));
                            }
                        });
                    }
                }
            });
        }
        match action {
            1 => self.desinstaller(),
            2 => {
                self.charger_apps();
            }
            _ => {}
        }
    }

    fn ui_demarrage(&mut self, ui: &mut egui::Ui, occupe: bool, h: f32) {
        let mut action = 0;
        {
            let mut e = self.log.etat();
            let Some(entrees) = e.entrees.as_mut() else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Chargement...");
                });
                return;
            };
            ui.horizontal(|ui| {
                if ui.add_enabled(!occupe, egui::Button::new("Désactiver la sélection")).clicked() {
                    action = 1;
                }
                if ui.add_enabled(!occupe, egui::Button::new("Réactiver la sélection")).clicked() {
                    action = 2;
                }
                if ui.add_enabled(!occupe, egui::Button::new("Actualiser")).clicked() {
                    action = 3;
                }
                if ui.button("Gestionnaire des tâches").clicked() {
                    let _ = Command::new("taskmgr.exe").args(["/7", "/startup"]).spawn();
                }
                if occupe {
                    ui.spinner();
                }
            });
            egui::ScrollArea::vertical().id_salt("demarrage_opti").max_height(h).auto_shrink([false, true]).show(ui, |ui| {
                for (g, titre) in GROUPES_DEMARRAGE.iter().enumerate() {
                    ui.add_space(6.0);
                    ui.strong(*titre);
                    egui::Grid::new(("grille_demarrage", g)).striped(true).num_columns(4).show(ui, |ui| {
                        ui.weak("Nom");
                        ui.weak("État");
                        ui.weak("Type");
                        ui.weak("Commande / identifiant");
                        ui.end_row();
                        for en in entrees.iter_mut().filter(|en| en.groupe == g as u8) {
                            ui.checkbox(&mut en.coche, &en.nom);
                            if en.desactive {
                                ui.colored_label(egui::Color32::GRAY, "Désactivé");
                            } else {
                                ui.colored_label(egui::Color32::LIGHT_GREEN, "Actif");
                            }
                            ui.label(en.type_);
                            ui.add(egui::Label::new(egui::RichText::new(&en.detail).weak()).truncate()).on_hover_text(&en.detail);
                            ui.end_row();
                        }
                    });
                }
            });
        }
        match action {
            1 => self.basculer_selection(true),
            2 => self.basculer_selection(false),
            3 => {
                self.charger_demarrage();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailles() {
        assert_eq!(format_taille(512), "512 o");
        assert_eq!(format_taille(2048), "2 Ko");
        assert_eq!(format_taille(1023 * 1024 * 1024 - 1), "1\u{a0}023 Mo");
        assert_eq!(format_taille(3 * 1024 * 1024 * 1024 / 2), "1,50 Go");
    }

    #[test]
    fn jokers() {
        assert!(like("thumbcache_256.db", "THUMBCACHE_*.db"));
        assert!(like("Microsoft.WindowsAppRuntime.1.5", "Microsoft.WindowsAppRuntime*"));
        assert!(like("Foo.HEVCVideoExtension", "*Extension"));
        assert!(!like("Microsoft.BingNews", "Microsoft.BingWeather"));
        assert!(like("a.b", "a?b"));
    }

    #[test]
    fn variables() {
        std::env::set_var("OPTI_TEST", "x");
        assert_eq!(exp("a%OPTI_TEST%b%INCONNU_ZZ%"), "axb%INCONNU_ZZ%");
        assert_eq!(exp("100%"), "100%");
    }

    #[test]
    fn commandes() {
        assert_eq!(split_commande(r#""C:\Program Files\X\unins000.exe" /SILENT"#), (r"C:\Program Files\X\unins000.exe".into(), "/SILENT".into()));
        assert_eq!(split_commande(r"C:\Program Files\Y\Uninst.EXE /S"), (r"C:\Program Files\Y\Uninst.EXE".into(), "/S".into()));
        assert_eq!(split_commande("foo bar"), ("foo".into(), "bar".into()));
    }

    #[test]
    fn paquets() {
        assert_eq!(sans_suffixe("Microsoft.WindowsTerminal_8wekyb3d8bbwe"), "Microsoft.WindowsTerminal");
        assert_eq!(editeur("CN=Microsoft Corporation, O=Microsoft Corporation, C=US"), "Microsoft Corporation");
        assert_eq!(maj_liste(vec!["a".into(), "b".into()], "a", false), vec!["b".to_string()]);
        assert_eq!(maj_liste(vec!["a".into()], "a", true), vec!["a".to_string()]);
        assert!(protege("Microsoft.VCLibs.140.00", APPS_PROTEGEES));
        assert!(protege("king.com.CandyCrushSaga", RECO));
    }

    #[test]
    fn noms() {
        let l: Vec<String> = (0..17).map(|i| i.to_string()).collect();
        let s = liste_noms(&l);
        assert!(s.starts_with(" - 0\n"));
        assert!(s.ends_with(" ... et 2 autre(s)"));
    }

    #[test]
    fn vidage() {
        let d = std::env::temp_dir().join("opti_test_vidage");
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("sous")).unwrap();
        fs::write(d.join("a.db"), b"12345").unwrap();
        fs::write(d.join("sous").join("b.txt"), b"123").unwrap();
        let m = d.join("*.db").display().to_string();
        assert_eq!(poids(&[&m]), 5);
        assert_eq!(poids(&[&d.display().to_string()]), 8);
        vider(&[&d.display().to_string()]).unwrap();
        assert!(d.is_dir());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&d);
        assert!(vider(&["C:\\"]).is_ok()); // racine ignorée
    }
}
