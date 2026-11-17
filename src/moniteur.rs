//! Moniteur Jeu : CPU/GPU/RAM/températures/FPS, mode jeu automatique (priorité haute, plan
//! performances, applis en pause, services arrêtés, RAM vidée) et historique.
//! Mêmes fichiers que l'ancien outil : `MoniteurJeu\reglages.ini` et `MoniteurJeu\historique\`.
//! Les mesures tournent dans un thread : le mode jeu marche aussi quand l'onglet n'est pas affiché.
use eframe::egui::{self, Color32, RichText};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use std::fs;
use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, SYSTEMTIME};

const N: usize = 300; // points de l'historique récent
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RAPPORT: &str = include_str!("moniteur_rapport.html");
const BEST_PERF: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);
const DXGKRNL: GUID = GUID::from_u128(0x802ec45a_1e99_4b83_9920_87c98277ba9d);
const NOM_ETW: &str = "MoniteurJeu_FPS";

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------- réglages (reglages.ini, même format que l'ancien outil) ----------

#[derive(Clone, PartialEq, Debug)]
pub struct Reglages {
    alerte_cpu_c: i32,
    alerte_gpu_c: i32,
    alerte_ram: i32,
    jeux: String,
    apps_en_pause: String,
    services: String,
    vider_ram: bool,
    forcer_nvidia: bool,
    overlay: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            alerte_cpu_c: 95, alerte_gpu_c: 87, alerte_ram: 92,
            jeux: "GTAProcess;GTA5;javaw".into(), apps_en_pause: "OneDrive".into(), services: "SysMain;WSearch;DiagTrack".into(),
            vider_ram: true, forcer_nvidia: true, overlay: true,
        }
    }
}

impl Reglages {
    fn lire(texte: &str) -> Reglages {
        let mut r = Reglages::default();
        for l in texte.lines() {
            let Some((k, v)) = l.trim_start_matches('\u{feff}').split_once('=') else { continue };
            let v = v.trim();
            let (n, b) = (v.parse::<i32>().ok(), match v.to_ascii_lowercase().as_str() {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            });
            match k.trim() {
                "AlerteCpuC" => r.alerte_cpu_c = n.unwrap_or(r.alerte_cpu_c),
                "AlerteGpuC" => r.alerte_gpu_c = n.unwrap_or(r.alerte_gpu_c),
                "AlerteRam" => r.alerte_ram = n.unwrap_or(r.alerte_ram),
                "Jeux" => r.jeux = v.into(),
                "AppsEnPause" => r.apps_en_pause = v.into(),
                "Services" => r.services = v.into(),
                "ViderRam" => r.vider_ram = b.unwrap_or(r.vider_ram),
                "ForcerNvidia" => r.forcer_nvidia = b.unwrap_or(r.forcer_nvidia),
                "Overlay" => r.overlay = b.unwrap_or(r.overlay),
                _ => {}
            }
        }
        r
    }

    fn texte(&self) -> String {
        let b = |v: bool| if v { "True" } else { "False" };
        format!("AlerteCpuC={}\r\nAlerteGpuC={}\r\nAlerteRam={}\r\nJeux={}\r\nAppsEnPause={}\r\nServices={}\r\nViderRam={}\r\nForcerNvidia={}\r\nOverlay={}\r\n",
            self.alerte_cpu_c, self.alerte_gpu_c, self.alerte_ram, self.jeux, self.apps_en_pause, self.services,
            b(self.vider_ram), b(self.forcer_nvidia), b(self.overlay))
    }

    fn charger(f: &Path) -> Reglages {
        match fs::read_to_string(f) {
            Ok(t) => Reglages::lire(&t),
            Err(_) => {
                let r = Reglages::default();
                let _ = fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                let _ = fs::write(f, r.texte());
                r
            }
        }
    }
}

/// "GTA5.exe; javaw" -> ["gta5", "javaw"]
fn liste(s: &str) -> Vec<String> {
    s.split(';').map(|x| {
        let x = x.trim().to_lowercase();
        x.strip_suffix(".exe").map(str::to_string).unwrap_or(x)
    }).filter(|x| !x.is_empty()).collect()
}

// ---------- dates ----------

fn local() -> SYSTEMTIME {
    let mut t: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    t
}

fn heure_txt(t: &SYSTEMTIME) -> String {
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute)
}

/// "yyyy-MM-dd HH:mm" -> minutes depuis 1970 (heure locale, sans fuseau, comme l'ancien outil)
fn minute(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 16 || b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b':' {
        return None;
    }
    let n = |a: usize, z: usize| s.get(a..z)?.parse::<i64>().ok();
    let (y, m, d, h, mi) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || mi > 59 {
        return None;
    }
    Some(crate::pressepapiers::jours(y, m, d) * 1440 + h * 60 + mi)
}

// ---------- historique ----------

fn ajouter(f: &Path, entete: &str, ligne: &str) {
    let _ = fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
    let neuf = !f.exists();
    if let Ok(mut o) = fs::OpenOptions::new().create(true).append(true).open(f) {
        if neuf {
            let _ = write!(o, "{entete}\r\n");
        }
        let _ = write!(o, "{ligne}\r\n");
    }
}

/// Lignes d'un CSV sans l'en-tête.
fn lignes(f: &Path) -> Vec<String> {
    let t = fs::read(f).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    t.lines().skip(1).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn echapper(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Page rapport.html : 8 derniers jours de mesures + 50 dernières sessions.
fn rapport(dir: &Path, maintenant: i64) -> String {
    let mut fichiers: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        n.len() == 11 && n.ends_with(".csv") && n.as_bytes()[4] == b'-'
    }).collect();
    fichiers.sort();
    let v = |s: &str| s.trim().parse::<i32>().map_or("null".to_string(), |v| v.to_string());
    let mut data = String::from("[");
    for f in &fichiers {
        for l in lignes(f) {
            let c: Vec<&str> = l.split(';').collect();
            let Some(t) = (c.len() >= 7).then(|| minute(c[0])).flatten() else { continue };
            if t < maintenant - 8 * 1440 {
                continue;
            }
            // [minute, cpu, cpu°C, ram, gpu, gpu°C, en jeu]
            data += &format!("[{t},{},{},{},{},{},{}],", v(c[1]), v(c[3]), v(c[4]), v(c[5]), v(c[6]), (c.len() > 7 && !c[7].is_empty()) as u8);
        }
    }
    data.push(']');
    let mut rows = String::new();
    for l in lignes(&dir.join("sessions.csv")).iter().rev().take(50) {
        rows += "<tr>";
        for c in l.split(';') {
            rows += &format!("<td>{}</td>", echapper(c));
        }
        rows += "</tr>";
    }
    if rows.is_empty() {
        rows = "<tr><td colspan=8 class=empty>Aucune session pour l'instant</td></tr>".into();
    }
    RAPPORT.replace("/*D*/", &data).replace("<!--S-->", &rows)
}

// ---------- Windows : fonctions sans bibliothèque d'import ----------

#[repr(C)]
struct TokPriv {
    n: u32,
    luid: [u32; 2],
    attr: u32,
}

#[repr(C)]
struct PdhValeur {
    statut: u32,
    v: f64,
}

#[link(name = "advapi32")]
extern "system" {
    fn StartTraceW(h: *mut u64, nom: *const u16, props: *mut u8) -> u32;
    fn ControlTraceW(h: u64, nom: *const u16, props: *mut u8, code: u32) -> u32;
    fn EnableTraceEx2(h: u64, guid: *const GUID, code: u32, niveau: u8, any: u64, all: u64, timeout: u32, p: *const c_void) -> u32;
    fn OpenTraceW(logfile: *mut u8) -> u64;
    fn ProcessTrace(h: *const u64, n: u32, debut: *const FILETIME, fin: *const FILETIME) -> u32;
    fn CloseTrace(h: u64) -> u32;
    fn OpenProcessToken(p: HANDLE, acces: u32, tok: *mut HANDLE) -> i32;
    fn LookupPrivilegeValueW(sys: *const u16, nom: *const u16, luid: *mut [u32; 2]) -> i32;
    fn AdjustTokenPrivileges(tok: HANDLE, tout: i32, nouv: *const TokPriv, taille: u32, prec: *mut c_void, ret: *mut u32) -> i32;
}

#[link(name = "pdh")]
extern "system" {
    fn PdhOpenQueryW(src: *const u16, data: usize, q: *mut isize) -> u32;
    fn PdhAddEnglishCounterW(q: isize, chemin: *const u16, data: usize, c: *mut isize) -> u32;
    fn PdhCollectQueryData(q: isize) -> u32;
    fn PdhGetFormattedCounterValue(c: isize, fmt: u32, typ: *mut u32, v: *mut PdhValeur) -> u32;
    fn PdhGetFormattedCounterArrayW(c: isize, fmt: u32, taille: *mut u32, n: *mut u32, items: *mut u8) -> u32;
}
const PDH_FMT_DOUBLE: u32 = 0x200;

/// Fonction d'une DLL chargée à la demande (nvml, powrprof, ntdll).
unsafe fn fonction<T: Copy>(dll: &str, nom: &str) -> Option<T> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let h = LoadLibraryW(w(dll).as_ptr());
    if h.is_null() {
        return None;
    }
    let n: Vec<u8> = nom.bytes().chain([0]).collect();
    let p = GetProcAddress(h, n.as_ptr())?;
    Some(std::mem::transmute_copy(&p))
}

type FnHandle = unsafe extern "system" fn(HANDLE) -> i32;

fn est_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn exe_de(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW};
    unsafe {
        let h = OpenProcess(0x1000, 0, pid); // PROCESS_QUERY_LIMITED_INFORMATION
        if h.is_null() {
            return None;
        }
        let mut b = [0u16; 520];
        let mut n = b.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, b.as_mut_ptr(), &mut n) != 0;
        CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&b[..n as usize]))
    }
}

fn nom_sans_ext(chemin: &str) -> String {
    Path::new(chemin).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase()
}

/// Pids dont le nom d'exe (sans .exe) est dans `noms`.
fn pids_par_nom(noms: &[String]) -> Vec<u32> {
    let mut pids = vec![0u32; 4096];
    let mut n = 0u32;
    unsafe { windows_sys::Win32::System::ProcessStatus::K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut n) };
    pids.truncate(n as usize / 4);
    pids.into_iter().filter(|&p| p != 0 && exe_de(p).is_some_and(|e| noms.contains(&nom_sans_ext(&e)))).collect()
}

fn sur_batterie() -> bool {
    let mut s: windows_sys::Win32::System::Power::SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::Power::GetSystemPowerStatus(&mut s) != 0 && s.ACLineStatus == 0 }
}

fn ram() -> Option<(f32, f64, f64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then(|| {
        let go = 1073741824.0;
        (m.dwMemoryLoad as f32, (m.ullTotalPhys - m.ullAvailPhys) as f64 / go, m.ullTotalPhys as f64 / go)
    })
}

/// Vide la mémoire "en veille" (cache) pour laisser de la RAM libre au jeu. Admin requis.
fn purger() {
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let mut tok: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), 0x28, &mut tok) == 0 {
            return; // ADJUST_PRIVILEGES | QUERY
        }
        let mut p = TokPriv { n: 1, luid: [0; 2], attr: 2 }; // SE_PRIVILEGE_ENABLED
        if LookupPrivilegeValueW(null(), w("SeProfileSingleProcessPrivilege").as_ptr(), &mut p.luid) != 0 {
            AdjustTokenPrivileges(tok, 0, &p, 0, null_mut(), null_mut());
        }
        CloseHandle(tok);
        type FnInfo = unsafe extern "system" fn(u32, *mut c_void, u32) -> i32;
        if let Some(f) = fonction::<FnInfo>("ntdll.dll", "NtSetSystemInformation") {
            let mut cmd = 4i32; // MemoryPurgeStandbyList
            f(80, &mut cmd as *mut i32 as *mut c_void, 4); // SystemMemoryListInformation
        }
    }
}

/// Ajoute l'exe dans les préférences GPU de Windows (carte puissante), sans écraser un choix existant.
fn forcer_gpu(exe: &str) {
    use windows_sys::Win32::System::Registry::*;
    unsafe {
        let mut k: HKEY = null_mut();
        let sous = w(r"Software\Microsoft\DirectX\UserGpuPreferences");
        if RegCreateKeyExW(HKEY_CURRENT_USER, sous.as_ptr(), 0, null(), REG_OPTION_NON_VOLATILE, KEY_QUERY_VALUE | KEY_SET_VALUE, null(), &mut k, null_mut()) != 0 {
            return;
        }
        let nom = w(exe);
        if RegQueryValueExW(k, nom.as_ptr(), null(), null_mut(), null_mut(), null_mut()) != 0 {
            let v = w("GpuPreference=2;");
            RegSetValueExW(k, nom.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        }
        RegCloseKey(k);
    }
}

fn cmd(prog: &str, args: &[&str]) -> bool {
    Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW).status().is_ok_and(|s| s.success())
}

fn arreter_services(noms: &[String], arretes: &Mutex<Vec<String>>) {
    let mut a = arretes.lock().unwrap();
    for n in noms {
        if cmd("net", &["stop", n]) {
            a.push(n.clone());
        }
    }
}

fn relancer_services(arretes: &Mutex<Vec<String>>) {
    let mut a = arretes.lock().unwrap();
    for n in a.drain(..) {
        cmd("net", &["start", &n]);
    }
}

// ---------- démarrage avec Windows (tâche planifiée, comme l'ancien outil) ----------

fn tache_existe() -> bool {
    cmd("schtasks", &["/Query", "/TN", "MoniteurJeu"])
}

/// Tâche planifiée (et pas la clé Run) : démarre en admin sans fenêtre UAC, aussi sur batterie.
fn basculer_auto(actif: bool) -> bool {
    if !actif {
        cmd("schtasks", &["/Delete", "/TN", "MoniteurJeu", "/F"]);
    } else {
        let user = echapper(&format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default()));
        let exe = echapper(&std::env::current_exe().unwrap_or_default().display().to_string());
        let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\
<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>\
<Principals><Principal id=\"Author\"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>\
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\
<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><Priority>7</Priority></Settings>\
<Actions Context=\"Author\"><Exec><Command>{exe}</Command></Exec></Actions></Task>");
        let tmp = std::env::temp_dir().join("MoniteurJeu_task.xml");
        let octets: Vec<u8> = [0xFEFFu16].into_iter().chain(xml.encode_utf16()).flat_map(u16::to_le_bytes).collect();
        if fs::write(&tmp, octets).is_ok() {
            cmd("schtasks", &["/Create", "/TN", "MoniteurJeu", "/XML", &tmp.display().to_string(), "/F"]);
            let _ = fs::remove_file(&tmp);
        }
    }
    tache_existe()
}

// ---------- FPS : ETW DxgKrnl (comme PresentMon), admin requis ----------

static FPS_PID: AtomicU32 = AtomicU32::new(0);
static N184: AtomicU32 = AtomicU32::new(0);
static N215: AtomicU32 = AtomicU32::new(0);

// Present_Info (184) et PresentHistoryDetailed (215) : selon l'API, l'un ou l'autre suit chaque image.
unsafe extern "system" fn evenement(r: *const u8) {
    if (r.add(12) as *const u32).read_unaligned() != FPS_PID.load(Relaxed) {
        return; // EVENT_HEADER.ProcessId
    }
    match (r.add(40) as *const u16).read_unaligned() { // EventDescriptor.Id
        184 => N184.fetch_add(1, Relaxed),
        215 => N215.fetch_add(1, Relaxed),
        _ => 0,
    };
}

fn props_init(p: &mut [u64]) {
    p.fill(0);
    let b = p.as_mut_ptr() as *mut u8;
    for (off, v) in [(0, 1024u32), (40, 1), (44, 0x20000), (48, 64), (64, 0x110), (68, 250), (116, 120)] {
        // BufferSize, ClientContext QPC, TRACED_GUID, BufferSize Ko, REAL_TIME|MS_FLUSH, FlushTimer, LoggerNameOffset
        unsafe { (b.add(off) as *mut u32).write(v) };
    }
}

struct Fps {
    props: Vec<u64>,
    logfile: Vec<u64>,
    nom: Vec<u16>,
    session: u64,
    trace: u64,
    fini: mpsc::Receiver<()>,
    depuis: Instant,
}

impl Fps {
    fn demarrer(pid: u32) -> Option<Fps> {
        FPS_PID.store(pid, Relaxed);
        N184.store(0, Relaxed);
        N215.store(0, Relaxed);
        let nom = w(NOM_ETW);
        let mut props = vec![0u64; 128];
        props_init(&mut props);
        let mut session = 0u64;
        unsafe {
            let pp = props.as_mut_ptr() as *mut u8;
            let mut rc = StartTraceW(&mut session, nom.as_ptr(), pp);
            if rc == 183 {
                // reste d'un plantage
                ControlTraceW(0, nom.as_ptr(), pp, 1);
                props_init(&mut props);
                rc = StartTraceW(&mut session, nom.as_ptr(), props.as_mut_ptr() as *mut u8);
            }
            if rc != 0 {
                return None;
            }
            EnableTraceEx2(session, &DXGKRNL, 1, 4, 0x800_0000, 0, 0, null()); // mot-clé Present
            let mut logfile = vec![0u64; 56];
            let lb = logfile.as_mut_ptr() as *mut u8;
            (lb.add(8) as *mut *const u16).write(nom.as_ptr()); // LoggerName
            (lb.add(28) as *mut u32).write(0x1000_0100); // REAL_TIME | EVENT_RECORD
            (lb.add(424) as *mut usize).write(evenement as unsafe extern "system" fn(*const u8) as usize); // EventRecordCallback
            let trace = OpenTraceW(lb);
            if trace == u64::MAX {
                ControlTraceW(session, null(), props.as_mut_ptr() as *mut u8, 1);
                return None;
            }
            let (tx, fini) = mpsc::channel();
            thread::spawn(move || {
                ProcessTrace(&trace, 1, null(), null());
                let _ = tx.send(());
            });
            Some(Fps { props, logfile, nom, session, trace, fini, depuis: Instant::now() })
        }
    }

    fn prendre(&mut self) -> f32 {
        let s = self.depuis.elapsed().as_secs_f64();
        self.depuis = Instant::now();
        let (a, b) = (N184.swap(0, Relaxed), N215.swap(0, Relaxed));
        if s > 0.2 { (a.max(b) as f64 / s) as f32 } else { -1.0 }
    }
}

impl Drop for Fps {
    fn drop(&mut self) {
        unsafe {
            ControlTraceW(self.session, null(), self.props.as_mut_ptr() as *mut u8, 1);
            CloseTrace(self.trace);
        }
        if self.fini.recv_timeout(Duration::from_secs(2)).is_err() {
            // ProcessTrace tourne encore : ne pas libérer ce qu'il peut lire
            std::mem::forget(std::mem::take(&mut self.props));
            std::mem::forget(std::mem::take(&mut self.logfile));
            std::mem::forget(std::mem::take(&mut self.nom));
        }
    }
}

// ---------- capteurs ----------

/// Compteurs Windows : température (zones thermiques) et bridage du processeur.
struct Pdh {
    q: isize,
    zones: isize,
    limite: isize,
}

impl Pdh {
    fn ouvrir() -> Option<Pdh> {
        let mut p = Pdh { q: 0, zones: 0, limite: 0 };
        unsafe {
            if PdhOpenQueryW(null(), 0, &mut p.q) != 0 {
                return None;
            }
            PdhAddEnglishCounterW(p.q, w(r"\Thermal Zone Information(*)\High Precision Temperature").as_ptr(), 0, &mut p.zones);
            PdhAddEnglishCounterW(p.q, w(r"\Processor Information(_Total)\% Performance Limit").as_ptr(), 0, &mut p.limite);
        }
        Some(p)
    }

    fn relever(&self) {
        unsafe { PdhCollectQueryData(self.q) };
    }

    /// Zone la plus chaude en °C, -1 si inconnu.
    fn temperature(&self) -> f32 {
        if self.zones == 0 {
            return -1.0;
        }
        unsafe {
            let (mut taille, mut n) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(self.zones, PDH_FMT_DOUBLE, &mut taille, &mut n, null_mut());
            if taille == 0 {
                return -1.0;
            }
            let mut b = vec![0u64; taille as usize / 8 + 1];
            if PdhGetFormattedCounterArrayW(self.zones, PDH_FMT_DOUBLE, &mut taille, &mut n, b.as_mut_ptr() as *mut u8) != 0 {
                return -1.0;
            }
            // item = { nom: *u16, valeur: PdhValeur } = 24 octets
            let best = (0..n as usize).map(|i| (b.as_ptr() as *const u8).add(i * 24 + 8) as *const PdhValeur)
                .filter(|v| (**v).statut <= 1).map(|v| ((*v).v / 10.0 - 273.15) as f32).fold(-1.0f32, f32::max);
            if best > 0.0 { best } else { -1.0 }
        }
    }

    fn bride(&self) -> bool {
        let mut v = PdhValeur { statut: 1, v: 100.0 };
        // shortcut: seuil fixe 70 %, à affiner si fausses alertes
        self.limite != 0 && unsafe { PdhGetFormattedCounterValue(self.limite, PDH_FMT_DOUBLE, null_mut(), &mut v) } == 0 && v.statut <= 1 && v.v < 70.0
    }
}

/// Carte NVIDIA via nvml.dll (installé avec le pilote).
struct Nvml {
    dev: *mut c_void,
    temp: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> u32,
    util: unsafe extern "C" fn(*mut c_void, *mut [u32; 2]) -> u32,
    bride: Option<unsafe extern "C" fn(*mut c_void, *mut u64) -> u32>,
}

impl Nvml {
    fn ouvrir() -> Option<Nvml> {
        unsafe {
            let init: unsafe extern "C" fn() -> u32 = fonction("nvml.dll", "nvmlInit_v2")?;
            let par_index: unsafe extern "C" fn(u32, *mut *mut c_void) -> u32 = fonction("nvml.dll", "nvmlDeviceGetHandleByIndex_v2")?;
            let mut dev = null_mut();
            if init() != 0 || par_index(0, &mut dev) != 0 {
                return None; // pas de carte NVIDIA
            }
            Some(Nvml {
                dev,
                temp: fonction("nvml.dll", "nvmlDeviceGetTemperature")?,
                util: fonction("nvml.dll", "nvmlDeviceGetUtilizationRates")?,
                bride: fonction("nvml.dll", "nvmlDeviceGetCurrentClocksThrottleReasons"),
            })
        }
    }

    /// (charge %, °C, bridée)
    fn lire(&self) -> (f32, f32, bool) {
        unsafe {
            let (mut t, mut u, mut pourquoi) = (0u32, [0u32; 2], 0u64);
            let t = if (self.temp)(self.dev, 0, &mut t) == 0 { t as f32 } else { -1.0 };
            let u = if (self.util)(self.dev, &mut u) == 0 { u[0] as f32 } else { -1.0 };
            if let Some(f) = self.bride {
                f(self.dev, &mut pourquoi);
            }
            (u, t, pourquoi & 0xE8 != 0) // HW slowdown | SW thermal | HW thermal | HW power brake
        }
    }
}

// ---------- état partagé avec l'interface ----------

#[derive(Clone, Copy)]
struct Mesures {
    cpu: f32,
    cpu_t: f32,
    ram: f32,
    ram_go: f64,
    ram_tot: f64,
    gpu: f32,
    gpu_t: f32,
    fps: f32,
    cpu_bride: bool,
    gpu_bride: bool,
}

impl Default for Mesures {
    fn default() -> Self {
        Mesures { cpu: 0.0, cpu_t: -1.0, ram: 0.0, ram_go: 0.0, ram_tot: 0.0, gpu: -1.0, gpu_t: -1.0, fps: -1.0, cpu_bride: false, gpu_bride: false }
    }
}

#[derive(Default)]
struct Etat {
    m: Mesures,
    batterie: bool,
    jeu: Option<String>,
    h: [VecDeque<f32>; 4], // CPU %, GPU %, CPU °C, GPU °C (-1 = inconnu)
    journal: VecDeque<String>,
    vu: Option<Instant>, // dernière fois que l'onglet était affiché
    cfg: Reglages,
    auto: Option<bool>,
}

impl Etat {
    fn noter(&mut self, texte: String) {
        let t = local();
        self.journal.push_front(format!("{:02}:{:02}  {texte}", t.wHour, t.wMinute));
        self.journal.truncate(30);
    }
}

// ---------- moteur (thread de mesure + mode jeu) ----------

struct Jeu {
    h: HANDLE,
    nom: String,
    debut: Instant,
    debut_txt: String,
    fps_s: f64,
    fps_n: u32,
    cpu_t_max: f32,
    gpu_t_max: f32,
    bride: u32,
    alertes: u32,
    en_pause: Vec<HANDLE>,
    fps: Option<Fps>,
    ancien_plan: Option<GUID>,
}

/// Cumuls de la minute en cours pour l'historique.
#[derive(Default)]
struct Minute {
    s_cpu: f32,
    m_cpu: f32,
    m_cpu_t: f32,
    s_ram: f32,
    s_gpu: f32,
    m_gpu_t: f32,
    n: u32,
    n_gpu: u32,
}

struct Moteur {
    hist: PathBuf,
    admin: bool,
    mon_pid: u32,
    tick: u64,
    prec: (u64, u64, u64),
    m: Mesures,
    pdh: Option<Pdh>,
    nvml: Option<Option<Nvml>>, // None = pas encore essayé
    jeu: Option<Jeu>,
    arretes: Arc<Mutex<Vec<String>>>,
    alertes: HashMap<&'static str, Instant>,
    minute: Minute,
    ecrit: Instant,
    ecrit_txt: String,
    notes: Vec<String>,
}

impl Moteur {
    fn new(hist: PathBuf) -> Moteur {
        Moteur {
            hist, admin: est_admin(), mon_pid: std::process::id(), tick: 0, prec: (0, 0, 0), m: Mesures::default(),
            pdh: Pdh::ouvrir(), nvml: None, jeu: None, arretes: Arc::default(), alertes: HashMap::new(),
            minute: Minute { m_cpu_t: -1.0, m_gpu_t: -1.0, ..Default::default() }, ecrit: Instant::now(), ecrit_txt: heure_txt(&local()), notes: Vec::new(),
        }
    }

    /// Une mesure ; renvoie l'attente avant la suivante.
    fn tic(&mut self, etat: &Mutex<Etat>) -> Duration {
        self.tick += 1;
        let (cfg, vu) = {
            let e = etat.lock().unwrap();
            (e.cfg.clone(), e.vu.is_some_and(|t| t.elapsed() < Duration::from_secs(2)))
        };
        let profond = vu || self.jeu.is_some(); // mesures complètes seulement si quelqu'un regarde ou en jeu
        let batterie = sur_batterie();

        let mut t = [FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }; 3];
        unsafe { windows_sys::Win32::System::Threading::GetSystemTimes(&mut t[0], &mut t[1], &mut t[2]) };
        let ft = |f: &FILETIME| (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64;
        let (idle, kernel, user) = (ft(&t[0]), ft(&t[1]), ft(&t[2]));
        let total = kernel.wrapping_sub(self.prec.1) + user.wrapping_sub(self.prec.2);
        if total > 0 {
            self.m.cpu = 100.0 * total.saturating_sub(idle.wrapping_sub(self.prec.0)) as f32 / total as f32;
        }
        self.prec = (idle, kernel, user);

        if let Some((r, go, tot)) = ram() {
            (self.m.ram, self.m.ram_go, self.m.ram_tot) = (r, go, tot);
        }
        if let Some(p) = &self.pdh {
            if profond || self.tick % 5 == 1 {
                p.relever();
                self.m.cpu_t = p.temperature();
                if profond {
                    self.m.cpu_bride = p.bride();
                }
            }
        }
        if profond {
            if let Some(n) = self.nvml.get_or_insert_with(Nvml::ouvrir) {
                (self.m.gpu, self.m.gpu_t, self.m.gpu_bride) = n.lire();
            }
        } else {
            (self.m.gpu, self.m.gpu_t, self.m.cpu_bride, self.m.gpu_bride) = (-1.0, -1.0, false, false);
        }
        self.m.fps = self.jeu.as_mut().and_then(|j| j.fps.as_mut()).map_or(-1.0, Fps::prendre);

        self.verifier_jeu(&cfg);
        let m = self.m;
        if let Some(j) = &mut self.jeu {
            if m.fps >= 0.0 {
                j.fps_s += m.fps as f64;
                j.fps_n += 1;
            }
            j.cpu_t_max = j.cpu_t_max.max(m.cpu_t);
            j.gpu_t_max = j.gpu_t_max.max(m.gpu_t);
            if m.cpu_bride || m.gpu_bride {
                j.bride += 1;
            }
        }

        let mi = &mut self.minute;
        mi.s_cpu += m.cpu;
        mi.m_cpu = mi.m_cpu.max(m.cpu);
        mi.m_cpu_t = mi.m_cpu_t.max(m.cpu_t);
        mi.s_ram += m.ram;
        mi.n += 1;
        if m.gpu >= 0.0 {
            mi.s_gpu += m.gpu;
            mi.n_gpu += 1;
            mi.m_gpu_t = mi.m_gpu_t.max(m.gpu_t);
        }
        if self.ecrit.elapsed() >= Duration::from_secs(60) {
            self.ecrire_historique();
        }

        if m.cpu_t >= cfg.alerte_cpu_c as f32 {
            self.alerte("cpu", format!("Processeur très chaud : {} °C", m.cpu_t as i32));
        }
        if m.gpu_t >= cfg.alerte_gpu_c as f32 {
            self.alerte("gpu", format!("Carte graphique très chaude : {} °C", m.gpu_t as i32));
        }
        if m.ram >= cfg.alerte_ram as f32 {
            self.alerte("ram", format!("Mémoire presque pleine : {} %", m.ram as i32));
        }
        if m.cpu_bride {
            self.alerte("bcpu", "Le processeur est bridé (chaleur ou alimentation) : il ralentit pour se protéger.".into());
        }
        if m.gpu_bride {
            self.alerte("bgpu", "La carte graphique est bridée (chaleur ou alimentation) : elle ralentit pour se protéger.".into());
        }

        let mut e = etat.lock().unwrap();
        e.m = m;
        e.batterie = batterie;
        e.jeu = self.jeu.as_ref().map(|j| j.nom.clone());
        for (h, v) in e.h.iter_mut().zip([m.cpu, m.gpu, m.cpu_t, m.gpu_t]) {
            h.push_back(v);
            if h.len() > N {
                h.pop_front();
            }
        }
        for n in self.notes.drain(..) {
            e.noter(n);
        }
        Duration::from_secs(if profond { 1 } else if batterie { 5 } else { 2 })
    }

    fn alerte(&mut self, genre: &'static str, texte: String) {
        if self.alertes.get(genre).is_some_and(|t| t.elapsed() < Duration::from_secs(600)) {
            return;
        }
        self.alertes.insert(genre, Instant::now());
        if let Some(j) = &mut self.jeu {
            j.alertes += 1;
        }
        self.notes.push(format!("⚠ {texte}"));
    }

    fn ecrire_historique(&mut self) {
        let mi = &self.minute;
        if mi.n > 0 {
            let opt = |v: f32| if v < 0.0 { String::new() } else { (v as i32).to_string() };
            let t = local();
            let ligne = [
                self.ecrit_txt.clone(),
                ((mi.s_cpu / mi.n as f32) as i32).to_string(),
                (mi.m_cpu as i32).to_string(),
                opt(mi.m_cpu_t),
                ((mi.s_ram / mi.n as f32) as i32).to_string(),
                if mi.n_gpu == 0 { String::new() } else { ((mi.s_gpu / mi.n_gpu as f32) as i32).to_string() },
                opt(mi.m_gpu_t),
                self.jeu.as_ref().map(|j| j.nom.clone()).unwrap_or_default(),
            ].join(";");
            ajouter(&self.hist.join(format!("{:04}-{:02}.csv", t.wYear, t.wMonth)),
                "heure;cpu_moy_%;cpu_max_%;cpu_temp_max;ram_moy_%;gpu_moy_%;gpu_temp_max;jeu", &ligne);
        }
        self.minute = Minute { m_cpu_t: -1.0, m_gpu_t: -1.0, ..Default::default() };
        self.ecrit = Instant::now();
        self.ecrit_txt = heure_txt(&local());
    }

    fn verifier_jeu(&mut self, cfg: &Reglages) {
        use windows_sys::Win32::System::Threading::GetExitCodeProcess;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
        if let Some(j) = &self.jeu {
            let mut code = 0u32;
            if unsafe { GetExitCodeProcess(j.h, &mut code) } == 0 || code != 259 {
                self.fin_jeu(); // 259 = encore en cours
            }
            return;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
        if pid == 0 || pid == self.mon_pid {
            return;
        }
        let Some(chemin) = exe_de(pid) else { return };
        let bas = nom_sans_ext(&chemin);
        let mut ok = liste(&cfg.jeux).iter().any(|g| bas.contains(g.as_str()));
        if !ok {
            let mut s = 0;
            ok = unsafe { windows_sys::Win32::UI::Shell::SHQueryUserNotificationState(&mut s) } == 0 && s == 3; // plein écran Direct3D
        }
        if ok {
            self.debut_jeu(pid, &chemin, cfg);
        }
    }

    fn debut_jeu(&mut self, pid: u32, chemin: &str, cfg: &Reglages) {
        use windows_sys::Win32::System::Threading::{OpenProcess, SetPriorityClass};
        // SYNCHRONIZE | QUERY_LIMITED | SET_INFORMATION ; sans SET_INFORMATION si le jeu est en admin et pas nous
        let mut h = unsafe { OpenProcess(0x100000 | 0x1000 | 0x0200, 0, pid) };
        if h.is_null() {
            h = unsafe { OpenProcess(0x100000 | 0x1000, 0, pid) };
        }
        if h.is_null() {
            return;
        }
        unsafe { SetPriorityClass(h, 0x80) }; // HIGH_PRIORITY_CLASS
        let nom = Path::new(chemin).file_name().and_then(|n| n.to_str()).unwrap_or(chemin).to_string();

        // N'a d'effet que sur le plan "Utilisation normale" ; sur "Haute performance" c'est déjà au max.
        let mut ancien_plan = None;
        unsafe {
            let get: Option<unsafe extern "system" fn(*mut GUID) -> u32> = fonction("powrprof.dll", "PowerGetActualOverlayScheme");
            let set: Option<unsafe extern "system" fn(GUID) -> u32> = fonction("powrprof.dll", "PowerSetActiveOverlayScheme");
            if let (Some(get), Some(set)) = (get, set) {
                let mut g: GUID = std::mem::zeroed();
                if get(&mut g) == 0 && !meme_guid(&g, &BEST_PERF) && set(BEST_PERF) == 0 {
                    ancien_plan = Some(g);
                }
            }
        }
        if cfg.forcer_nvidia {
            forcer_gpu(chemin);
        }
        let mut en_pause = Vec::new();
        if let Some(suspendre) = unsafe { fonction::<FnHandle>("ntdll.dll", "NtSuspendProcess") } {
            for p in pids_par_nom(&liste(&cfg.apps_en_pause)).into_iter().filter(|&p| p != pid) {
                let hp = unsafe { windows_sys::Win32::System::Threading::OpenProcess(0x0800, 0, p) }; // PROCESS_SUSPEND_RESUME
                if !hp.is_null() {
                    if unsafe { suspendre(hp) } == 0 { en_pause.push(hp) } else { unsafe { CloseHandle(hp) }; }
                }
            }
        }
        let mut fps = None;
        if self.admin {
            fps = Fps::demarrer(pid);
            let (purge, svc, arretes) = (cfg.vider_ram, liste(&cfg.services), self.arretes.clone());
            thread::spawn(move || {
                if purge {
                    purger();
                }
                arreter_services(&svc, &arretes);
            });
        }
        self.notes.push(format!("Mode jeu activé : {nom} : priorité haute{}{}",
            if en_pause.is_empty() { String::new() } else { format!(", {} appli(s) en pause", en_pause.len()) },
            if self.admin { format!(", services en pause{}", if cfg.vider_ram { ", RAM vidée" } else { "" }) } else { String::new() }));
        self.jeu = Some(Jeu {
            h, nom, debut: Instant::now(), debut_txt: heure_txt(&local()), fps_s: 0.0, fps_n: 0, cpu_t_max: -1.0, gpu_t_max: -1.0,
            bride: 0, alertes: 0, en_pause, fps, ancien_plan,
        });
    }

    /// Remet tout comme avant le jeu.
    fn nettoyer(&mut self, j: Jeu, relancer_attente: bool) {
        drop(j.fps);
        if let Some(reprendre) = unsafe { fonction::<FnHandle>("ntdll.dll", "NtResumeProcess") } {
            for h in &j.en_pause {
                unsafe { reprendre(*h) };
            }
        }
        for h in j.en_pause {
            unsafe { CloseHandle(h) };
        }
        if let Some(g) = j.ancien_plan {
            unsafe {
                if let Some(set) = fonction::<unsafe extern "system" fn(GUID) -> u32>("powrprof.dll", "PowerSetActiveOverlayScheme") {
                    set(g);
                }
            }
        }
        unsafe { CloseHandle(j.h) };
        let a = self.arretes.clone();
        if relancer_attente {
            relancer_services(&a);
        } else {
            thread::spawn(move || relancer_services(&a));
        }
    }

    fn fin_jeu(&mut self) {
        let Some(j) = self.jeu.take() else { return };
        let min = j.debut.elapsed().as_secs() / 60;
        let fps = if j.fps_n > 0 { ((j.fps_s / j.fps_n as f64) as i32).to_string() } else { String::new() };
        let deg = |v: f32| if v > 0.0 { (v as i32).to_string() } else { String::new() };
        let mut resume = format!("{min} min");
        if !fps.is_empty() {
            resume += &format!(", {fps} FPS moy.");
        }
        if j.cpu_t_max > 0.0 {
            resume += &format!(", CPU max {}°C", j.cpu_t_max as i32);
        }
        if j.gpu_t_max > 0.0 {
            resume += &format!(", GPU max {}°C", j.gpu_t_max as i32);
        }
        if j.bride > 0 {
            resume += &format!(", bridé {} s", j.bride);
        }
        if j.alertes > 0 {
            resume += &format!(", {} alerte(s)", j.alertes);
        }
        self.notes.push(format!("{} fermé : {resume}", j.nom));
        let ligne = [j.debut_txt.clone(), min.to_string(), j.nom.clone(), fps, deg(j.cpu_t_max), deg(j.gpu_t_max), j.bride.to_string(), j.alertes.to_string()].join(";");
        ajouter(&self.hist.join("sessions.csv"), "debut;duree_min;jeu;fps_moy;cpu_temp_max;gpu_temp_max;bride_s;alertes", &ligne);
        self.nettoyer(j, false);
    }

    /// En quittant Outils : tout remettre, services relancés avant de rendre la main.
    fn quitter(&mut self) {
        if let Some(j) = self.jeu.take() {
            self.nettoyer(j, true);
        } else {
            relancer_services(&self.arretes);
        }
        if self.minute.n > 0 {
            self.ecrire_historique();
        }
    }
}

fn meme_guid(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

fn moteur(etat: Arc<Mutex<Etat>>, hist: PathBuf, arret: mpsc::Receiver<()>) {
    let mut m = Moteur::new(hist);
    let auto = tache_existe();
    etat.lock().unwrap().auto = Some(auto);
    loop {
        let d = m.tic(&etat);
        if !matches!(arret.recv_timeout(d), Err(mpsc::RecvTimeoutError::Timeout)) {
            break;
        }
    }
    m.quitter();
}

// ---------- interface ----------

pub struct Moniteur {
    etat: Arc<Mutex<Etat>>,
    dossier: PathBuf,
    cfg: Reglages,
    admin: bool,
    temperatures: bool,
    reglages: bool,
    arret: Option<mpsc::Sender<()>>,
    fil: Option<JoinHandle<()>>,
}

impl Moniteur {
    pub fn new(racine: &Path) -> Moniteur {
        let dossier = racine.join("MoniteurJeu");
        let cfg = Reglages::charger(&dossier.join("reglages.ini"));
        let etat = Arc::new(Mutex::new(Etat { cfg: cfg.clone(), ..Default::default() }));
        let (tx, rx) = mpsc::channel();
        let (e, hist) = (etat.clone(), dossier.join("historique"));
        let fil = thread::spawn(move || moteur(e, hist, rx));
        Moniteur { etat, dossier, cfg, admin: est_admin(), temperatures: false, reglages: false, arret: Some(tx), fil: Some(fil) }
    }

    /// Overlay FPS en jeu (toujours au-dessus, clics traversants).
    /// shortcut: dessiné avec la fenêtre Outils, donc figé si Outils est réduit ; fenêtre native si gênant.
    pub fn tic(&mut self, ctx: &egui::Context) {
        let (m, montrer) = {
            let e = self.etat.lock().unwrap();
            (e.m, e.jeu.is_some() && e.cfg.overlay)
        };
        if !montrer {
            return;
        }
        let vp = egui::ViewportBuilder::default().with_title("Overlay").with_inner_size([196.0, 60.0]).with_position([8.0, 8.0])
            .with_decorations(false).with_always_on_top().with_mouse_passthrough(true).with_taskbar(false).with_resizable(false);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("moniteur_overlay"), vp, |ctx, _| {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(14, 16, 22)).inner_margin(8.0)).show(ctx, |ui| {
                let c = if m.fps >= 60.0 { Color32::from_rgb(52, 211, 153) } else if m.fps >= 30.0 { Color32::from_rgb(251, 191, 36) } else { Color32::from_rgb(244, 63, 94) };
                let fps = if m.fps >= 0.0 { format!("{} FPS", m.fps as i32) } else { "— FPS".into() };
                ui.label(RichText::new(fps).size(20.0).strong().color(if m.fps >= 0.0 { c } else { Color32::GRAY }));
                ui.label(RichText::new(format!("CPU {}% {}   GPU {} {}", m.cpu as i32, deg(m.cpu_t), pct(m.gpu), deg(m.gpu_t))).size(11.0));
            });
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let etat = self.etat.clone();
        let mut e = etat.lock().unwrap();
        e.vu = Some(Instant::now());
        let m = e.m;
        ui.heading("Moniteur Jeu");
        ui.horizontal(|ui| {
            match &e.jeu {
                Some(j) => ui.label(RichText::new(format!(" EN JEU · {j} ")).strong().color(Color32::WHITE).background_color(Color32::from_rgb(200, 40, 60))),
                None => ui.label(RichText::new(" PRÊT ").strong().color(Color32::BLACK).background_color(Color32::from_rgb(52, 211, 153))),
            };
            if e.batterie {
                ui.label(RichText::new(" BATTERIE ").strong().color(Color32::BLACK).background_color(Color32::from_rgb(251, 191, 36)));
            }
        });
        if !self.admin {
            ui.colored_label(Color32::GRAY, "Sans droits admin : pas de FPS, ni RAM vidée, ni services en pause (lance Outils en administrateur).");
        }
        ui.add_space(8.0);
        ui.columns(4, |c| {
            jauge(&mut c[0], "CPU", &format!("{} %", m.cpu as i32), m.cpu / 100.0,
                &format!("{}{}", deg(m.cpu_t), if m.cpu_bride { " · bridé" } else { "" }), Color32::from_rgb(79, 140, 255));
            jauge(&mut c[1], "GPU", &pct(m.gpu), m.gpu.max(0.0) / 100.0,
                &format!("{}{}", deg(m.gpu_t), if m.gpu_bride { " · bridée" } else { "" }), Color32::from_rgb(52, 211, 153));
            jauge(&mut c[2], "RAM", &format!("{} %", m.ram as i32), m.ram / 100.0,
                &format!("{:.1} / {:.1} Go", m.ram_go, m.ram_tot), Color32::from_rgb(167, 139, 250));
            jauge(&mut c[3], "FPS", &if m.fps >= 0.0 { (m.fps as i32).to_string() } else { "—".into() }, m.fps.max(0.0) / 144.0,
                if e.jeu.is_some() { "en jeu" } else { "hors jeu" }, Color32::from_rgb(233, 236, 242));
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.strong("Activité récente");
            ui.selectable_value(&mut self.temperatures, false, "Charge");
            ui.selectable_value(&mut self.temperatures, true, "Températures");
        });
        let series: [(usize, Color32); 2] = if self.temperatures {
            [(2, Color32::from_rgb(251, 191, 36)), (3, Color32::from_rgb(244, 63, 94))]
        } else {
            [(0, Color32::from_rgb(79, 140, 255)), (1, Color32::from_rgb(52, 211, 153))]
        };
        courbe(ui, &series.map(|(i, c)| (&e.h[i], c)), if self.temperatures { "°C" } else { "%" });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Vider la RAM").clicked() {
                if !self.admin {
                    e.noter("Lance Outils en administrateur pour vider la RAM.".into());
                } else {
                    let et = self.etat.clone();
                    thread::spawn(move || {
                        let avant = ram().map_or(0.0, |r| r.2 - r.1);
                        purger();
                        let libre = ram().map_or(0.0, |r| r.2 - r.1) - avant;
                        et.lock().unwrap().noter(format!("RAM vidée : {:.1} Go libérés", libre.max(0.0)));
                    });
                }
            }
            if ui.button("Historique").clicked() {
                let dir = self.dossier.join("historique");
                let _ = fs::create_dir_all(&dir);
                let f = dir.join("rapport.html");
                let t = local();
                let maintenant = crate::pressepapiers::jours(t.wYear as i64, t.wMonth as i64, t.wDay as i64) * 1440 + t.wHour as i64 * 60 + t.wMinute as i64;
                if fs::write(&f, rapport(&dir, maintenant)).is_err() || !crate::lanceur::ouvrir(&f.display().to_string(), None) {
                    e.noter("Impossible d'ouvrir l'historique.".into());
                }
            }
            ui.toggle_value(&mut self.reglages, "Réglages");
        });
        if self.reglages {
            ui.group(|ui| self.ui_reglages(ui, &mut e));
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for l in &e.journal {
                ui.label(l);
            }
        });
    }

    fn ui_reglages(&mut self, ui: &mut egui::Ui, e: &mut Etat) {
        let c = &mut self.cfg;
        egui::Grid::new("moniteur_reglages").num_columns(2).show(ui, |ui| {
            ui.label("Température CPU max (°C)");
            ui.add(egui::DragValue::new(&mut c.alerte_cpu_c).range(60..=105));
            ui.end_row();
            ui.label("Température GPU max (°C)");
            ui.add(egui::DragValue::new(&mut c.alerte_gpu_c).range(60..=100));
            ui.end_row();
            ui.label("RAM max (%)");
            ui.add(egui::DragValue::new(&mut c.alerte_ram).range(50..=99));
            ui.end_row();
            ui.label("Jeux (noms d'exe, séparés par ;)");
            ui.text_edit_singleline(&mut c.jeux);
            ui.end_row();
            ui.label("Applis en pause pendant le jeu");
            ui.text_edit_singleline(&mut c.apps_en_pause);
            ui.end_row();
            ui.label("Services arrêtés pendant le jeu (admin)");
            ui.text_edit_singleline(&mut c.services);
            ui.end_row();
        });
        ui.checkbox(&mut c.vider_ram, "Vider la RAM au lancement d'un jeu (admin)");
        ui.checkbox(&mut c.forcer_nvidia, "Forcer la carte graphique puissante pour les jeux");
        ui.checkbox(&mut c.overlay, "Overlay FPS en jeu");
        if let Some(mut a) = e.auto {
            if ui.checkbox(&mut a, "Démarrer avec Windows").changed() {
                let r = basculer_auto(a);
                e.auto = Some(r);
                if a && !r {
                    e.noter("Lance Outils en administrateur pour activer le démarrage automatique.".into());
                }
            }
        }
        if *c != e.cfg {
            let _ = fs::write(self.dossier.join("reglages.ini"), c.texte());
            e.cfg = c.clone();
        }
    }
}

impl Drop for Moniteur {
    fn drop(&mut self) {
        drop(self.arret.take()); // réveille le thread : il remet tout comme avant le jeu
        if let Some(f) = self.fil.take() {
            let _ = f.join();
        }
    }
}

fn deg(v: f32) -> String {
    if v >= 0.0 { format!("{} °C", v as i32) } else { "— °C".into() }
}

fn pct(v: f32) -> String {
    if v >= 0.0 { format!("{} %", v as i32) } else { "—".into() }
}

fn jauge(ui: &mut egui::Ui, titre: &str, valeur: &str, part: f32, sous: &str, c: Color32) {
    ui.group(|ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(titre).color(Color32::GRAY));
        ui.label(RichText::new(valeur).size(26.0).strong().color(c));
        ui.add(egui::ProgressBar::new(part.clamp(0.0, 1.0)).desired_height(6.0).fill(c));
        ui.label(RichText::new(sous).small().color(Color32::GRAY));
    });
}

/// Courbes des 300 dernières mesures (valeur < 0 = trou).
fn courbe(ui: &mut egui::Ui, series: &[(&VecDeque<f32>, Color32)], unite: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 160.0), egui::Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 8.0, Color32::from_rgb(22, 25, 34));
    let gris = Color32::from_rgb(39, 44, 57);
    for i in 0..=4 {
        let y = r.bottom() - r.height() * i as f32 / 4.0;
        p.line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], (1.0, gris));
        p.text(egui::pos2(r.left() + 4.0, y - 2.0), egui::Align2::LEFT_BOTTOM, format!("{}{unite}", i * 25), egui::FontId::proportional(10.0), Color32::GRAY);
    }
    for (h, c) in series {
        let x0 = r.right() - (h.len() as f32 - 1.0) * r.width() / (N as f32 - 1.0);
        let pt = |i: usize, v: f32| egui::pos2(x0 + i as f32 * r.width() / (N as f32 - 1.0), r.bottom() - r.height() * v.min(100.0) / 100.0);
        for i in 1..h.len() {
            if h[i - 1] >= 0.0 && h[i] >= 0.0 {
                p.line_segment([pt(i - 1, h[i - 1]), pt(i, h[i])], (2.0, *c));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reglages_ini() {
        let r = Reglages::lire("\u{feff}AlerteCpuC=90\r\nAlerteGpuC=x\r\nJeux=GTA5;javaw\r\nViderRam=False\r\nOverlay=true\r\nInconnu=1\r\n");
        assert_eq!((r.alerte_cpu_c, r.alerte_gpu_c, r.jeux.as_str(), r.vider_ram, r.overlay), (90, 87, "GTA5;javaw", false, true));
        assert_eq!(Reglages::lire(&r.texte()), r);
        assert!(Reglages::default().texte().starts_with("AlerteCpuC=95\r\nAlerteGpuC=87\r\nAlerteRam=92\r\nJeux=GTAProcess;GTA5;javaw\r\n"));
        assert!(Reglages::default().texte().ends_with("ViderRam=True\r\nForcerNvidia=True\r\nOverlay=True\r\n"));
    }

    #[test]
    fn liste_noms() {
        assert_eq!(liste(" GTAProcess; GTA5.EXE ;;javaw"), ["gtaprocess", "gta5", "javaw"]);
        assert!(liste("").is_empty());
    }

    #[test]
    fn minutes() {
        assert_eq!(minute("1970-01-02 01:01"), Some(1440 + 61));
        assert_eq!(minute("2026-11-17 2:00"), None);
        assert_eq!(minute("heure"), None);
    }

    #[test]
    fn page_rapport() {
        let d = std::env::temp_dir().join(format!("moniteur_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let entete = "heure;cpu_moy_%;cpu_max_%;cpu_temp_max;ram_moy_%;gpu_moy_%;gpu_temp_max;jeu";
        ajouter(&d.join("2026-11.csv"), entete, "2026-11-17 10:00;12;40;;55;;;");
        ajouter(&d.join("2026-11.csv"), entete, "2026-11-17 10:01;30;90;71;60;80;65;GTA5.exe");
        ajouter(&d.join("2026-11.csv"), entete, "2026-11-01 10:00;1;1;1;1;1;1;"); // trop vieux
        ajouter(&d.join("sessions.csv"), "debut;duree_min;jeu;fps_moy;cpu_temp_max;gpu_temp_max;bride_s;alertes", "2026-11-17 10:01;1;<a>;60;71;65;0;0");
        let t = minute("2026-11-17 10:01").unwrap();
        let p = rapport(&d, t);
        assert!(p.contains(&format!("const D=[[{},12,null,55,null,null,0],[{t},30,71,60,80,65,1],];", t - 1)));
        assert!(p.contains("<td>&lt;a&gt;</td>"));
        assert!(!p.contains("Aucune session"));
        let _ = fs::remove_dir_all(&d);
    }
}
