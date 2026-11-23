//! Système et réseau, suite : alerte programme gourmand (29), pilotes à jour (30), bilan du PC sur 100 (31),
//! test de vitesse programmé (34), partage de dossier (35), région et VPN pour jouer (36), ping dans l'overlay (37).
use crate::optimiseur::{est_admin, ps, relancer_admin};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SANS_FENETRE: u32 = 0x0800_0000;

type Etat = Arc<Mutex<(bool, String)>>;

fn en_fond(e: &Etat, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, "En cours…".into());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    o
}

fn admin_requis(ui: &mut egui::Ui, quoi: &str) -> bool {
    let a = est_admin();
    if !a {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(255, 190, 80), format!("⚠ Droits administrateur nécessaires pour {quoi}."));
            if ui.small_button("Relancer en admin").clicked() && relancer_admin() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
    a
}

fn maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn date_heure(t: i64) -> String {
    let l = t + crate::pressepapiers::decalage();
    let (a, m, j) = crate::quotidien2::date_de_jours(l.div_euclid(86_400));
    format!("{j:02}/{m:02}/{a} {:02}h{:02}", l.rem_euclid(86_400) / 3600, l.rem_euclid(3600) / 60)
}

// ---------------------------------------------------------------------------------------------------
// 29 · Alerte programme gourmand
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub nom: String,
    pub cpu: f32,
    pub ram: u64,
}

/// Temps processeur cumulé (100 ns) et mémoire de chaque processus.
fn instantane() -> HashMap<u32, (String, u64, u64)> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::*;
    let mut m = HashMap::new();
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(s, &mut e);
        while ok != 0 {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
            if !h.is_null() {
                let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                let (mut c, mut x, mut k, mut u) = (z, z, z, z);
                let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                mem.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                let v = |f: &FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
                if GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0 {
                    K32GetProcessMemoryInfo(h, &mut mem, mem.cb);
                    let l = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(260);
                    m.insert(e.th32ProcessID, (String::from_utf16_lossy(&e.szExeFile[..l]), v(&k) + v(&u), mem.WorkingSetSize as u64));
                }
                CloseHandle(h);
            }
            ok = Process32NextW(s, &mut e);
        }
        CloseHandle(s);
    }
    m
}

/// % de processeur entre deux instantanés (sur tous les cœurs : 100 % = PC entier).
pub fn pourcent(avant: u64, apres: u64, intervalle_s: f64, coeurs: usize) -> f32 {
    ((apres.saturating_sub(avant)) as f64 / (intervalle_s * 10_000_000.0 * coeurs.max(1) as f64) * 100.0) as f32
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageGourmand {
    pub actif: bool,
    pub cpu: f32,
    pub minutes: u32,
    pub ram_go: f32,
    pub ignores: Vec<String>,
}

impl Default for ReglageGourmand {
    fn default() -> Self {
        ReglageGourmand { actif: true, cpu: 40.0, minutes: 3, ram_go: 4.0, ignores: vec!["MsMpEng.exe".into(), "TiWorker.exe".into(), "System".into()] }
    }
}

pub struct Gourmands {
    r: Arc<Mutex<ReglageGourmand>>,
    top: Arc<Mutex<Vec<Proc>>>,
    message: String,
}

impl Gourmands {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageGourmand>> = Arc::new(Mutex::new(charger("gourmands")));
        let top: Arc<Mutex<Vec<Proc>>> = Arc::default();
        let (r2, t2) = (r.clone(), top.clone());
        std::thread::spawn(move || {
            let coeurs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let mut avant = instantane();
            let mut t0 = Instant::now();
            let mut depuis: HashMap<u32, Instant> = HashMap::new();
            let mut prevenus: HashMap<String, Instant> = HashMap::new();
            loop {
                std::thread::sleep(Duration::from_secs(10));
                let apres = instantane();
                let dt = t0.elapsed().as_secs_f64();
                t0 = Instant::now();
                let mut v: Vec<Proc> = apres.iter().filter(|(p, _)| **p > 4).map(|(pid, (n, t, ram))| Proc { pid: *pid, nom: n.clone(), cpu: avant.get(pid).map(|a| pourcent(a.1, *t, dt, coeurs)).unwrap_or(0.0), ram: *ram }).collect();
                v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
                avant = apres;
                let c = r2.lock().unwrap().clone();
                if c.actif && crate::outil::jeu_actif().is_none() {
                    for p in &v {
                        if c.ignores.iter().any(|i| i.eq_ignore_ascii_case(&p.nom)) {
                            continue;
                        }
                        let trop_cpu = p.cpu >= c.cpu;
                        let trop_ram = p.ram as f64 >= c.ram_go as f64 * 1_073_741_824.0;
                        if trop_cpu {
                            depuis.entry(p.pid).or_insert_with(Instant::now);
                        } else {
                            depuis.remove(&p.pid);
                        }
                        let longtemps = depuis.get(&p.pid).is_some_and(|t| t.elapsed() >= Duration::from_secs(c.minutes as u64 * 60));
                        if (longtemps || trop_ram) && prevenus.get(&p.nom).is_none_or(|t| t.elapsed() > Duration::from_secs(1800)) {
                            prevenus.insert(p.nom.clone(), Instant::now());
                            let pourquoi = if longtemps { format!("{:.0} % du processeur depuis {} min", p.cpu, c.minutes) } else { format!("{:.1} Go de mémoire", p.ram as f64 / 1_073_741_824.0) };
                            crate::systeme::notifier("Programme gourmand", &format!("{} utilise {pourquoi}", p.nom));
                        }
                    }
                }
                v.truncate(15);
                *t2.lock().unwrap() = v;
            }
        });
        Gourmands { r, top, message: String::new() }
    }
}

impl Outil for Gourmands {
    fn nom(&self) -> &'static str {
        "🐷 Programmes gourmands"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programmes gourmands");
        ui.weak("Prévient quand un programme mange le processeur longtemps ou trop de mémoire (onglet de navigateur bloqué, appli qui plante en boucle…). Jamais pendant un jeu.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.checkbox(&mut r.actif, "Surveiller");
        ui.horizontal(|ui| {
            ui.label("Prévenir au-delà de");
            ui.add(egui::DragValue::new(&mut r.cpu).range(10.0..=100.0).suffix(" % du processeur"));
            ui.label("pendant");
            ui.add(egui::DragValue::new(&mut r.minutes).range(1..=60).suffix(" min"));
            ui.label("ou de");
            ui.add(egui::DragValue::new(&mut r.ram_go).range(0.5..=64.0).speed(0.1).suffix(" Go de mémoire"));
        });
        let top = self.top.lock().unwrap().clone();
        if top.is_empty() {
            ui.spinner();
            ui.weak("première mesure dans 10 s…");
        }
        let mut fermer = None;
        let mut ignorer = None;
        egui::Grid::new("gourmands").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
            for p in &top {
                ui.label(RichText::new(&p.nom).strong());
                ui.add(egui::ProgressBar::new((p.cpu / 100.0).min(1.0)).desired_width(120.0).text(format!("{:.0} %", p.cpu)));
                ui.label(crate::disque::taille_lisible(p.ram));
                if ui.small_button("Fermer").clicked() {
                    fermer = Some(p.clone());
                }
                if ui.small_button("Ignorer").on_hover_text("Ne plus prévenir pour ce programme").clicked() {
                    ignorer = Some(p.nom.clone());
                }
                ui.end_row();
            }
        });
        if let Some(p) = fermer {
            let ok = std::process::Command::new("taskkill").args(["/PID", &p.pid.to_string()]).creation_flags(SANS_FENETRE).status().is_ok_and(|s| s.success());
            self.message = if ok { format!("{} fermé.", p.nom) } else { format!("{} refuse de se fermer (droits admin ?).", p.nom) };
        }
        if let Some(n) = ignorer {
            if !r.ignores.contains(&n) {
                r.ignores.push(n);
            }
        }
        if !r.ignores.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak("Ignorés :");
                let mut retirer = None;
                for (k, i) in r.ignores.iter().enumerate() {
                    if ui.small_button(format!("{i} ✖")).clicked() {
                        retirer = Some(k);
                    }
                }
                if let Some(k) = retirer {
                    r.ignores.remove(k);
                }
            });
        }
        ui.label(&self.message);
        if r != avant {
            enregistrer("gourmands", &r);
            *self.r.lock().unwrap() = r;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(5));
    }
}

// ---------------------------------------------------------------------------------------------------
// 30 · Pilotes à jour
// ---------------------------------------------------------------------------------------------------

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct Carte {
    #[serde(rename = "N")]
    nom: String,
    #[serde(rename = "V")]
    version: String,
    #[serde(rename = "D")]
    date: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct MajPilote {
    #[serde(rename = "T")]
    titre: String,
    #[serde(rename = "C")]
    classe: String,
    #[serde(rename = "D")]
    date: String,
}

const PS_CARTES: &str = r#"ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_VideoController | ForEach-Object { [pscustomobject]@{ N = $_.Name; V = $_.DriverVersion; D = $(if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } else { '' }) } })"#;
const PS_CHERCHER: &str = r#"$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and Type='Driver'")
ConvertTo-Json -Compress -InputObject @($r.Updates | ForEach-Object { [pscustomobject]@{ T = $_.Title; C = "$($_.DriverClass)"; D = $(if ($_.DriverVerDate) { ([datetime]$_.DriverVerDate).ToString('yyyy-MM-dd') } else { '' }) } })"#;
const PS_INSTALLER: &str = r#"$s = New-Object -ComObject Microsoft.Update.Session
$r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and Type='Driver'")
if ($r.Updates.Count -eq 0) { 'rien'; exit }
$c = New-Object -ComObject Microsoft.Update.UpdateColl
foreach ($u in $r.Updates) { if (-not $u.EulaAccepted) { $u.AcceptEula() }; [void]$c.Add($u) }
$d = $s.CreateUpdateDownloader(); $d.Updates = $c; [void]$d.Download()
$i = $s.CreateUpdateInstaller(); $i.Updates = $c; $res = $i.Install()
"$($c.Count) $($res.ResultCode) $($res.RebootRequired)""#;

/// Âge en jours d'une date AAAA-MM-JJ.
pub fn age_jours(date: &str, aujourdhui: &str) -> Option<i64> {
    let (a, b) = (crate::quotidien2::lire_date(date)?, crate::quotidien2::lire_date(aujourdhui)?);
    Some(crate::quotidien2::jours_depuis_1970(b.0, b.1, b.2) - crate::quotidien2::jours_depuis_1970(a.0, a.1, a.2))
}

pub fn page_fabricant(carte: &str) -> Option<(&'static str, &'static str)> {
    let c = carte.to_lowercase();
    if c.contains("nvidia") || c.contains("geforce") || c.contains("rtx") || c.contains("gtx") {
        Some(("NVIDIA App (mises à jour automatiques)", "https://www.nvidia.com/fr-fr/software/nvidia-app/"))
    } else if c.contains("amd") || c.contains("radeon") {
        Some(("AMD Adrenalin (détection automatique)", "https://www.amd.com/fr/support/download/drivers.html"))
    } else if c.contains("intel") {
        Some(("Intel Driver & Support Assistant", "https://www.intel.fr/content/www/fr/fr/support/detect.html"))
    } else {
        None
    }
}

pub struct Pilotes {
    cartes: Arc<Mutex<Option<Vec<Carte>>>>,
    majs: Arc<Mutex<Option<Result<Vec<MajPilote>, String>>>>,
    e: Etat,
}

impl Pilotes {
    pub fn new() -> Self {
        Pilotes { cartes: Arc::default(), majs: Arc::default(), e: Arc::default() }
    }
}

impl Outil for Pilotes {
    fn nom(&self) -> &'static str {
        "🆙 Pilotes à jour"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pilotes à jour");
        if self.cartes.lock().unwrap().is_none() {
            *self.cartes.lock().unwrap() = Some(vec![]);
            let c = self.cartes.clone();
            std::thread::spawn(move || *c.lock().unwrap() = Some(ps(PS_CARTES).ok().and_then(|s| serde_json::from_str::<Vec<Carte>>(s.trim()).ok()).unwrap_or_default()));
        }
        let aujourdhui = crate::minecraft::horodatage_iso().get(..10).unwrap_or("").to_string();
        ui.strong("🎮 Carte graphique (le pilote le plus important pour les jeux)");
        for c in self.cartes.lock().unwrap().clone().unwrap_or_default() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(&c.nom).strong());
                let age = age_jours(&c.date, &aujourdhui);
                ui.horizontal(|ui| {
                    ui.label(format!("Pilote {} du {}", c.version, c.date));
                    match age {
                        Some(a) if a > 180 => ui.colored_label(Color32::from_rgb(250, 210, 90), format!("⚠ vieux de {} mois", a / 30)),
                        Some(a) => ui.colored_label(Color32::LIGHT_GREEN, format!("✔ récent ({} j)", a)),
                        None => ui.label(""),
                    };
                });
                if let Some((t, u)) = page_fabricant(&c.nom) {
                    if ui.button(format!("⬇ {t}")).clicked() {
                        let _ = std::process::Command::new("explorer").arg(u).spawn();
                    }
                }
            });
        }
        ui.separator();
        ui.strong("🧩 Autres pilotes (Windows Update)");
        let admin = admin_requis(ui, "installer des pilotes");
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("🔎 Chercher des pilotes à mettre à jour")).clicked() {
                let m = self.majs.clone();
                *m.lock().unwrap() = Some(Err("recherche (peut prendre 1 à 2 minutes)…".into()));
                std::thread::spawn(move || *m.lock().unwrap() = Some(ps(PS_CHERCHER).and_then(|s| {
                    let s = s.trim();
                    if s.is_empty() || s == "null" { Ok(vec![]) } else { serde_json::from_str::<Vec<MajPilote>>(s).map_err(|e| e.to_string()) }
                })));
            }
        });
        match self.majs.lock().unwrap().clone() {
            Some(Ok(v)) if v.is_empty() => {
                ui.colored_label(Color32::LIGHT_GREEN, "✔ Tous les pilotes proposés par Windows sont à jour.");
            }
            Some(Ok(v)) => {
                for m in &v {
                    ui.label(format!("• {} ({}{})", m.titre, m.classe, if m.date.is_empty() { String::new() } else { format!(", {}", m.date) }));
                }
                if ui.add_enabled(admin && !occupe, egui::Button::new(RichText::new(format!("⬇ Tout installer ({})", v.len())).strong())).clicked() {
                    let m = self.majs.clone();
                    en_fond(&self.e, move || {
                        let r = ps(PS_INSTALLER)?;
                        let p: Vec<&str> = r.split_whitespace().collect();
                        *m.lock().unwrap() = None;
                        match p.as_slice() {
                            ["rien"] => Ok("Rien à installer.".into()),
                            [n, code, reboot] => Ok(format!("{} {n} pilote(s){}", if *code == "2" { "✔ Installé :" } else { "⚠ Installation partielle :" }, if *reboot == "True" { " — redémarre le PC pour finir" } else { "" })),
                            _ => Ok(r),
                        }
                    });
                }
            }
            Some(Err(e)) => {
                ui.horizontal(|ui| {
                    if e.contains("recherche") {
                        ui.spinner();
                    }
                    ui.weak(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 31 · Bilan du PC sur 100
// ---------------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Point {
    pub nom: &'static str,
    pub gagne: u32,
    pub sur: u32,
    pub detail: String,
    pub conseil: Option<(&'static str, &'static str)>, // (texte, outil à ouvrir)
}

const PS_BILAN: &str = r#"$os = Get-CimInstance Win32_OperatingSystem
$c = Get-PSDrive C
$sys = Get-PhysicalDisk | Where-Object { (Get-Partition -DiskNumber $_.DeviceId -ErrorAction SilentlyContinue | Where-Object DriveLetter -eq 'C') } | Select-Object -First 1
$demarrage = @(Get-CimInstance Win32_StartupCommand -ErrorAction SilentlyContinue).Count
$tmp = (Get-ChildItem $env:TEMP -Recurse -Force -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum
$mp = Get-MpComputerStatus -ErrorAction SilentlyContinue
$reboot = (Test-Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired')
$g = Get-CimInstance Win32_VideoController | Select-Object -First 1
$sante = @(Get-PhysicalDisk | Where-Object HealthStatus -ne 'Healthy').Count
ConvertTo-Json -Compress -InputObject ([pscustomobject]@{
 libre = [double]$c.Free; total = [double]($c.Used + $c.Free); ssd = "$($sys.MediaType)"; ram = [double]$os.TotalVisibleMemorySize * 1KB
 demarrage = $demarrage; tmp = [double]$tmp; antivirus = [bool]$mp.RealTimeProtectionEnabled; sig = [int]$mp.AntivirusSignatureAge
 reboot = $reboot; allume = [int]((Get-Date) - $os.LastBootUpTime).TotalDays; pilote = $(if ($g.DriverDate) { $g.DriverDate.ToString('yyyy-MM-dd') } else { '' }); disques_malades = $sante })"#;

/// Note de chaque critère d'après les mesures (JSON du script).
pub fn noter_bilan(j: &serde_json::Value, aujourdhui: &str) -> Vec<Point> {
    let f = |k: &str| j[k].as_f64().unwrap_or(0.0);
    let go = 1_073_741_824.0;
    let mut v = vec![];
    let libre = f("libre") / f("total").max(1.0);
    v.push(Point { nom: "Place libre sur C:", gagne: if libre > 0.2 { 15 } else if libre > 0.1 { 8 } else { 0 }, sur: 15, detail: format!("{:.0} Go libres ({:.0} %)", f("libre") / go, libre * 100.0), conseil: (libre <= 0.2).then_some(("Faire de la place", "💽 Espace disque")) });
    let ssd = j["ssd"].as_str().unwrap_or("");
    v.push(Point { nom: "Windows sur un SSD", gagne: if ssd == "SSD" || ssd.is_empty() { 15 } else { 0 }, sur: 15, detail: if ssd.is_empty() { "type inconnu".into() } else { ssd.into() }, conseil: (ssd == "HDD").then_some(("Un SSD rend le PC bien plus rapide", "🏎 Vitesse des disques")) });
    let ram = f("ram") / go;
    v.push(Point { nom: "Mémoire", gagne: if ram >= 15.0 { 10 } else if ram >= 7.5 { 6 } else { 2 }, sur: 10, detail: format!("{:.0} Go", ram.round()), conseil: (ram < 15.0).then_some(("16 Go conseillés pour jouer", "🧠 Mémoire en veille")) });
    let dem = f("demarrage") as u32;
    v.push(Point { nom: "Programmes au démarrage", gagne: if dem <= 6 { 10 } else if dem <= 12 { 5 } else { 0 }, sur: 10, detail: format!("{dem} programme(s)"), conseil: (dem > 6).then_some(("En retarder ou en couper", "🐢 Démarrage retardé")) });
    let tmp = f("tmp") / go;
    v.push(Point { nom: "Fichiers temporaires", gagne: if tmp < 1.0 { 5 } else if tmp < 5.0 { 3 } else { 0 }, sur: 5, detail: format!("{tmp:.1} Go"), conseil: (tmp >= 1.0).then_some(("Nettoyer", "🚀 Optimiseur")) });
    let av = j["antivirus"].as_bool().unwrap_or(false);
    let sig = f("sig");
    v.push(Point { nom: "Antivirus", gagne: if av && sig <= 3.0 { 15 } else if av { 10 } else { 0 }, sur: 15, detail: if av { format!("actif, définitions de {} j", sig as i64) } else { "protection en temps réel coupée".into() }, conseil: (!av || sig > 3.0).then_some(("Vérifier l'antivirus", "🛡 Analyse antivirus")) });
    let reboot = j["reboot"].as_bool().unwrap_or(false);
    let allume = f("allume") as i64;
    v.push(Point { nom: "Mises à jour et redémarrage", gagne: if !reboot && allume < 7 { 10 } else if !reboot { 6 } else { 2 }, sur: 10, detail: format!("{}allumé depuis {allume} j", if reboot { "redémarrage en attente, " } else { "" }), conseil: (reboot || allume >= 7).then_some(("Redémarrer", "🔄 Rappel de redémarrage")) });
    let age = age_jours(j["pilote"].as_str().unwrap_or(""), aujourdhui).unwrap_or(0);
    v.push(Point { nom: "Pilote de la carte graphique", gagne: if age < 180 { 10 } else if age < 365 { 5 } else { 0 }, sur: 10, detail: format!("{} mois", age / 30), conseil: (age >= 180).then_some(("Mettre à jour", "🆙 Pilotes à jour")) });
    let malades = f("disques_malades") as u32;
    v.push(Point { nom: "Santé des disques", gagne: if malades == 0 { 10 } else { 0 }, sur: 10, detail: if malades == 0 { "bonne".into() } else { format!("{malades} disque(s) en mauvais état") }, conseil: (malades > 0).then_some(("Sauvegarde vite tes fichiers", "🩺 Santé des disques")) });
    v
}

pub struct Bilan {
    r: Arc<Mutex<Option<Result<Vec<Point>, String>>>>,
}

impl Bilan {
    pub fn new() -> Self {
        Bilan { r: Arc::default() }
    }

    fn lancer(&self) {
        let r = self.r.clone();
        *r.lock().unwrap() = Some(Err("analyse du PC…".into()));
        std::thread::spawn(move || {
            let auj = crate::minecraft::horodatage_iso().get(..10).unwrap_or("").to_string();
            let res = ps(PS_BILAN).and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim()).map_err(|e| e.to_string())).map(|j| noter_bilan(&j, &auj));
            if let Ok(v) = &res {
                let note: u32 = v.iter().map(|p| p.gagne).sum();
                let mut h: Vec<(String, u32)> = charger("bilan_historique");
                h.push((auj, note));
                if h.len() > 30 {
                    h.remove(0);
                }
                enregistrer("bilan_historique", &h);
            }
            *r.lock().unwrap() = Some(res);
        });
    }
}

impl Outil for Bilan {
    fn nom(&self) -> &'static str {
        "💯 Bilan du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bilan du PC");
        let etat = self.r.lock().unwrap().clone();
        match etat {
            None => {
                ui.weak("Une note sur 100 et des conseils, en 20 secondes.");
                if ui.button(RichText::new("▶ Faire le bilan").size(18.0)).clicked() {
                    self.lancer();
                }
            }
            Some(Err(e)) => {
                ui.horizontal(|ui| {
                    if e.contains("analyse") {
                        ui.spinner();
                    }
                    ui.label(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            Some(Ok(v)) => {
                let note: u32 = v.iter().map(|p| p.gagne).sum();
                let c = if note >= 80 { Color32::LIGHT_GREEN } else if note >= 55 { Color32::from_rgb(250, 210, 90) } else { Color32::LIGHT_RED };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{note}")).size(64.0).strong().color(c));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("/ 100").size(22.0));
                        ui.label(match note {
                            90.. => "Excellent état 🎉",
                            75..=89 => "Bon état",
                            55..=74 => "Quelques points à améliorer",
                            _ => "Ton PC a besoin d'un coup de main",
                        });
                    });
                });
                let h: Vec<(String, u32)> = charger("bilan_historique");
                if h.len() > 1 {
                    ui.weak(format!("Avant : {}", h.iter().rev().skip(1).take(5).map(|(d, n)| format!("{n} ({})", d.get(5..).unwrap_or(d))).collect::<Vec<_>>().join(" · ")));
                }
                egui::Grid::new("bilan").striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
                    for p in &v {
                        ui.label(if p.gagne == p.sur { "✅" } else if p.gagne > 0 { "🟡" } else { "🔴" });
                        ui.label(RichText::new(p.nom).strong());
                        ui.label(format!("{}/{}", p.gagne, p.sur));
                        ui.weak(&p.detail);
                        match p.conseil {
                            Some((t, o)) => {
                                if ui.small_button(format!("➡ {t}")).clicked() {
                                    crate::interface2::ouvrir_outil(o);
                                }
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
                if ui.button("⟳ Refaire le bilan").clicked() {
                    self.lancer();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 34 · Test de vitesse programmé
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct MesureVitesse {
    pub quand: i64,
    pub descente: f64, // Mbit/s
    pub montee: f64,
    pub ping: u32,
}

fn mesurer_vitesse() -> Result<MesureVitesse, String> {
    let t = Instant::now();
    ureq::get("https://speed.cloudflare.com/__down?bytes=0").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?;
    let ping = t.elapsed().as_millis() as u32;
    let t = Instant::now();
    let mut lu = 0u64;
    let mut r = ureq::get("https://speed.cloudflare.com/__down?bytes=50000000").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?.into_reader();
    let mut buf = vec![0u8; 1 << 16];
    while t.elapsed() < Duration::from_secs(10) {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        lu += n as u64;
    }
    let descente = lu as f64 * 8.0 / 1e6 / t.elapsed().as_secs_f64();
    let donnees = vec![0u8; 8_000_000];
    let t = Instant::now();
    ureq::post("https://speed.cloudflare.com/__up").set("User-Agent", "Outils").send_bytes(&donnees).map_err(|e| e.to_string())?;
    let montee = donnees.len() as f64 * 8.0 / 1e6 / t.elapsed().as_secs_f64();
    Ok(MesureVitesse { quand: maintenant(), descente, montee, ping })
}

/// Fichier CSV des mesures (pour l'opérateur).
pub fn csv(v: &[MesureVitesse]) -> String {
    let mut s = String::from("date;descendant (Mbit/s);montant (Mbit/s);ping (ms)\n");
    for m in v {
        s.push_str(&format!("{};{:.1};{:.1};{}\n", date_heure(m.quand), m.descente, m.montee, m.ping).replace('.', ","));
    }
    s
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageVitesse {
    pub actif: bool,
    pub heures: u32,
    pub promis: f64,
    pub mesures: Vec<MesureVitesse>,
}

impl Default for ReglageVitesse {
    fn default() -> Self {
        ReglageVitesse { actif: false, heures: 3, promis: 0.0, mesures: vec![] }
    }
}

pub struct VitesseProgrammee {
    r: Arc<Mutex<ReglageVitesse>>,
    e: Etat,
}

impl VitesseProgrammee {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageVitesse>> = Arc::new(Mutex::new(charger("vitesse_programmee")));
        let r2 = r.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(300));
            let c = r2.lock().unwrap().clone();
            let derniere = c.mesures.last().map(|m| m.quand).unwrap_or(0);
            // pas pendant un jeu (la mesure ferait ramer la partie)
            if c.actif && maintenant() - derniere >= c.heures as i64 * 3600 && crate::outil::jeu_actif().is_none() {
                if let Ok(m) = mesurer_vitesse() {
                    let mut x = r2.lock().unwrap();
                    x.mesures.push(m);
                    if x.mesures.len() > 2000 {
                        x.mesures.remove(0);
                    }
                    enregistrer("vitesse_programmee", &*x);
                }
            }
        });
        VitesseProgrammee { r, e: Arc::default() }
    }
}

impl Outil for VitesseProgrammee {
    fn nom(&self) -> &'static str {
        "📶 Test de vitesse programmé"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test de vitesse programmé");
        ui.weak("Mesure ta connexion toute seule, régulièrement, et garde l'historique : la preuve à montrer à ton opérateur si le débit promis n'est pas là.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Mesurer toutes les");
            ui.add(egui::DragValue::new(&mut r.heures).range(1..=24).suffix(" h"));
            ui.label("· débit promis");
            ui.add(egui::DragValue::new(&mut r.promis).range(0.0..=10_000.0).suffix(" Mbit/s"));
        });
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("▶ Mesurer maintenant")).clicked() {
                let rr = self.r.clone();
                en_fond(&self.e, move || {
                    let m = mesurer_vitesse()?;
                    let t = format!("✔ ↓ {:.0} Mbit/s · ↑ {:.0} Mbit/s · ping {} ms", m.descente, m.montee, m.ping);
                    let mut x = rr.lock().unwrap();
                    x.mesures.push(m);
                    enregistrer("vitesse_programmee", &*x);
                    Ok(t)
                });
            }
            if !r.mesures.is_empty() && ui.button("💾 Exporter (Excel / CSV)…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name("mesures debit.csv").save_file() {
                    let _ = std::fs::write(f, "\u{feff}".to_string() + &csv(&r.mesures));
                }
            }
        });
        let v = &r.mesures;
        if v.len() >= 2 {
            let moy = v.iter().map(|m| m.descente).sum::<f64>() / v.len() as f64;
            let min = v.iter().map(|m| m.descente).fold(f64::MAX, f64::min);
            ui.label(format!("{} mesures · moyenne ↓ {moy:.0} Mbit/s · plus bas {min:.0} Mbit/s{}", v.len(), if r.promis > 0.0 { format!(" · {:.0} % du débit promis", moy / r.promis * 100.0) } else { String::new() }));
            let max = v.iter().map(|m| m.descente).fold(r.promis, f64::max).max(1.0);
            let (z, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(640.0), 140.0), egui::Sense::hover());
            ui.painter().rect_filled(z, 4.0, Color32::from_black_alpha(50));
            let n = v.len().min(200);
            let pts: Vec<egui::Pos2> = v[v.len() - n..].iter().enumerate().map(|(k, m)| egui::pos2(z.left() + k as f32 / (n - 1).max(1) as f32 * z.width(), z.bottom() - (m.descente / max) as f32 * z.height())).collect();
            ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.0_f32, Color32::from_rgb(120, 170, 255))));
            if r.promis > 0.0 {
                let y = z.bottom() - (r.promis / max) as f32 * z.height();
                ui.painter().hline(z.x_range(), y, egui::Stroke::new(1.0_f32, Color32::from_rgb(255, 120, 170)));
            }
        }
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for m in v.iter().rev().take(50) {
                ui.label(format!("{} — ↓ {:.0} · ↑ {:.0} Mbit/s · {} ms", date_heure(m.quand), m.descente, m.montee, m.ping));
            }
        });
        if r != avant {
            let mut x = self.r.lock().unwrap();
            r.mesures = x.mesures.clone();
            *x = r;
            enregistrer("vitesse_programmee", &*x);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 35 · Partage de dossier (page web locale)
// ---------------------------------------------------------------------------------------------------

/// Chemin demandé → chemin sûr dans le dossier partagé (refuse « .. »).
pub fn chemin_sur(racine: &Path, demande: &str) -> Option<PathBuf> {
    let d = crate::telephone::decoder_url(demande.split('?').next().unwrap_or(""));
    let mut p = racine.to_path_buf();
    for morceau in d.split('/').filter(|x| !x.is_empty()) {
        if morceau == ".." || morceau == "." || morceau.contains('\\') || morceau.contains(':') {
            return None;
        }
        p.push(morceau);
    }
    Some(p)
}

fn encoder_url(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn echapper(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page_dossier(racine: &Path, p: &Path, rel: &str, cle: &str) -> String {
    let mut lignes = String::new();
    if !rel.trim_matches('/').is_empty() {
        let parent = rel.trim_end_matches('/').rsplit_once('/').map(|x| x.0).unwrap_or("");
        lignes.push_str(&format!("<a class=d href=\"/{}/?k={cle}\">⬆ Dossier parent</a>", encoder_url(parent.trim_start_matches('/'))));
    }
    let mut v: Vec<(bool, String, u64)> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| {
        let m = e.metadata().ok();
        (m.as_ref().is_some_and(|m| m.is_dir()), e.file_name().to_string_lossy().into_owned(), m.map(|m| m.len()).unwrap_or(0))
    }).filter(|x| !x.1.starts_with('.')).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    for (dossier, n, t) in v {
        let lien = format!("/{}{}{}", encoder_url(rel.trim_matches('/')), if rel.trim_matches('/').is_empty() { "" } else { "/" }, encoder_url(&n));
        if dossier {
            lignes.push_str(&format!("<a class=d href=\"{lien}/?k={cle}\">📁 {}</a>", echapper(&n)));
        } else {
            lignes.push_str(&format!("<a href=\"{lien}?k={cle}\">📄 {} <small>{}</small></a>", echapper(&n), crate::disque::taille_lisible(t)));
        }
    }
    let titre = racine.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
    format!(r#"<!doctype html><html lang=fr><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1"><title>{t}</title>
<style>body{{margin:0;font-family:system-ui,sans-serif;background:#120a10;color:#f5dce8;padding:16px}}h1{{font-size:20px}}a{{display:block;padding:12px 14px;margin:6px 0;border-radius:12px;background:#1e1019;border:1px solid #3a1f33;color:#f5dce8;text-decoration:none}}a.d{{font-weight:600}}small{{color:#b49aaa;float:right}}</style></head>
<body><h1>📂 {t}{s}</h1>{lignes}</body></html>"#, t = echapper(&titre), s = echapper(rel))
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ReglagePartage {
    pub dossier: String,
    pub cle: String,
    pub port: u16,
}

pub struct Partage {
    r: ReglagePartage,
    marche: Arc<AtomicBool>,
    arret: Arc<AtomicBool>,
    journal: Arc<Mutex<Vec<String>>>,
    qr: Option<(String, egui::TextureHandle)>,
    erreur: Arc<Mutex<String>>,
}

impl Partage {
    pub fn new() -> Self {
        let mut r: ReglagePartage = charger("partage_dossier");
        if r.port == 0 {
            r.port = 8096;
        }
        if r.cle.is_empty() {
            r.cle = crate::telephone::nouvelle_cle();
        }
        Partage { r, marche: Arc::default(), arret: Arc::default(), journal: Arc::default(), qr: None, erreur: Arc::default() }
    }

    fn demarrer(&mut self) {
        self.arret = Arc::default();
        let (racine, cle, port, m, a, j, err) = (PathBuf::from(&self.r.dossier), self.r.cle.clone(), self.r.port, self.marche.clone(), self.arret.clone(), self.journal.clone(), self.erreur.clone());
        std::thread::spawn(move || {
            let l = match std::net::TcpListener::bind(("0.0.0.0", port)) {
                Ok(l) => l,
                Err(e) => {
                    *err.lock().unwrap() = format!("port {port} indisponible : {e}");
                    return;
                }
            };
            let _ = l.set_nonblocking(true);
            m.store(true, Relaxed);
            err.lock().unwrap().clear();
            while !a.load(Relaxed) {
                let Ok((s, adr)) = l.accept() else {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                let (racine, cle, j) = (racine.clone(), cle.clone(), j.clone());
                std::thread::spawn(move || {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
                    let Ok(c) = s.try_clone() else { return };
                    let mut lect = std::io::BufReader::new(c);
                    let mut ligne = String::new();
                    if lect.read_line(&mut ligne).is_err() {
                        return;
                    }
                    loop {
                        let mut h = String::new();
                        if lect.read_line(&mut h).map_or(true, |n| n == 0) || h.trim().is_empty() {
                            break;
                        }
                    }
                    let mut s = s;
                    let chemin = ligne.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let ok = chemin.contains(&format!("k={cle}"));
                    if !ok {
                        let _ = write!(s, "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\nLien incomplet : demande le lien complet (avec la clé).");
                        return;
                    }
                    let Some(p) = chemin_sur(&racine, &chemin) else { return };
                    let rel = crate::telephone::decoder_url(chemin.split('?').next().unwrap_or("/"));
                    if p.is_dir() {
                        let corps = page_dossier(&racine, &p, &rel, &cle);
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}", corps.len());
                    } else if let Ok(mut f) = std::fs::File::open(&p) {
                        let n = f.metadata().map(|m| m.len()).unwrap_or(0);
                        let nom = p.file_name().unwrap_or_default().to_string_lossy().replace('"', "");
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {n}\r\nContent-Disposition: attachment; filename=\"{nom}\"\r\nConnection: close\r\n\r\n");
                        if std::io::copy(&mut f, &mut s).is_ok() {
                            let mut jj = j.lock().unwrap();
                            jj.push(format!("⬇ {nom} → {}", adr.ip()));
                            if jj.len() > 50 {
                                jj.remove(0);
                            }
                        }
                    } else {
                        let _ = write!(s, "HTTP/1.1 404 Not Found\r\nConnection: close\r\n\r\n");
                    }
                });
            }
            m.store(false, Relaxed);
        });
    }
}

impl Outil for Partage {
    fn nom(&self) -> &'static str {
        "🤝 Partager un dossier"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Partager un dossier à la maison");
        ui.weak("Un dossier consultable depuis un autre PC, une tablette ou un téléphone de la maison, dans le navigateur (lecture seule). Rien à installer de l'autre côté.");
        let marche = self.marche.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!marche, egui::Button::new("📁 Choisir le dossier…")).clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.r.dossier = d.display().to_string();
                    enregistrer("partage_dossier", &self.r);
                }
            }
            ui.label(&self.r.dossier);
        });
        if self.r.dossier.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            if marche {
                if ui.button("⏹ Arrêter le partage").clicked() {
                    self.arret.store(true, Relaxed);
                }
            } else if ui.button(RichText::new("▶ Partager").strong()).clicked() {
                self.demarrer();
            }
            if !marche && ui.small_button("🔑 Nouveau lien").on_hover_text("Les anciens liens ne marcheront plus").clicked() {
                self.r.cle = crate::telephone::nouvelle_cle();
                enregistrer("partage_dossier", &self.r);
            }
        });
        let e = self.erreur.lock().unwrap().clone();
        if !e.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if marche {
            let ip = crate::telephone::ip_locale().unwrap_or("localhost".into());
            let url = format!("http://{ip}:{}/?k={}", self.r.port, self.r.cle);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&url).monospace());
                if ui.small_button("📋").clicked() {
                    ui.ctx().copy_text(url.clone());
                }
            });
            if self.qr.as_ref().is_none_or(|(u, _)| *u != url) {
                if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
                    let w = code.width();
                    let n = w + 8;
                    let c = code.to_colors();
                    let mut px = vec![255u8; n * n];
                    for y in 0..w {
                        for x in 0..w {
                            if c[y * w + x] == qrcode::Color::Dark {
                                px[(y + 4) * n + x + 4] = 0;
                            }
                        }
                    }
                    self.qr = Some((url.clone(), ui.ctx().load_texture("qr_partage", egui::ColorImage::from_gray([n, n], &px), egui::TextureOptions::NEAREST)));
                }
            }
            if let Some((_, t)) = &self.qr {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(180.0, 180.0)));
            }
            ui.weak("Le lien contient une clé : seuls ceux à qui tu le donnes peuvent ouvrir le dossier.");
            for l in self.journal.lock().unwrap().iter().rev().take(10) {
                ui.label(l);
            }
            ui.ctx().request_repaint_after(Duration::from_secs(2));
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 36 · Région et VPN pour jouer / 37 · Ping dans l'overlay
// ---------------------------------------------------------------------------------------------------

fn warp_cli() -> Option<PathBuf> {
    let p = PathBuf::from(crate::optimiseur::exp(r"%ProgramFiles%\Cloudflare\Cloudflare WARP\warp-cli.exe"));
    p.exists().then_some(p)
}

fn warp(args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(warp_cli().ok_or("WARP n'est pas installé")?).args(args).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string() + &String::from_utf8_lossy(&o.stderr))
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ReglagePing {
    pub overlay: bool,
    pub region: usize,
}

pub struct Region {
    r: Arc<Mutex<ReglagePing>>,
    statut: Arc<Mutex<String>>,
    e: Etat,
}

impl Region {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglagePing>> = Arc::new(Mutex::new(charger("ping_overlay")));
        let r2 = r.clone();
        // ping vers la région choisie, affiché dans l'overlay en jeu
        std::thread::spawn(move || loop {
            let c = r2.lock().unwrap().clone();
            let ligne = if c.overlay {
                let (nom, hote) = crate::reseau3::REGIONS[c.region.min(crate::reseau3::REGIONS.len() - 1)];
                let ms = (|| {
                    use std::net::ToSocketAddrs;
                    let a = (hote, 443).to_socket_addrs().ok()?.next()?;
                    let t = Instant::now();
                    std::net::TcpStream::connect_timeout(&a, Duration::from_secs(1)).ok()?;
                    Some(t.elapsed().as_millis() as u32)
                })();
                let region = nom.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
                Some(match ms {
                    Some(m) => (format!("📶 {m} ms ({region})"), if m < 50 { [120, 255, 160] } else if m < 100 { [250, 210, 90] } else { [255, 100, 110] }),
                    None => (format!("📶 — ({region})"), [255, 100, 110]),
                })
            } else {
                None
            };
            {
                let mut o = crate::jeux5::OVERLAY.lock().unwrap();
                o.retain(|x| !x.0.starts_with('📶'));
                o.extend(ligne);
            }
            std::thread::sleep(Duration::from_secs(2));
        });
        Region { r, statut: Arc::default(), e: Arc::default() }
    }
}

impl Outil for Region {
    fn nom(&self) -> &'static str {
        "🛰 Région, VPN et ping en jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Région, VPN et ping en jeu");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.strong("📶 Ping dans l'overlay en jeu");
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.overlay, "Afficher mon ping vers");
            egui::ComboBox::from_id_salt("region_ping").selected_text(crate::reseau3::REGIONS[r.region.min(crate::reseau3::REGIONS.len() - 1)].0).show_ui(ui, |ui| {
                for (k, (n, _)) in crate::reseau3::REGIONS.iter().enumerate() {
                    ui.selectable_value(&mut r.region, k, *n);
                }
            });
        });
        ui.weak("Active aussi l'overlay (Jeux › Overlay en jeu, Ctrl+Alt+H). L'outil « Ping des serveurs de jeux » trouve la région la plus rapide pour toi.");
        ui.separator();
        ui.strong("🛰 Changer d'itinéraire (VPN gratuit Cloudflare WARP)");
        ui.weak("Si un jeu lag alors que ta connexion va bien, passer par WARP change le chemin vers les serveurs et améliore parfois le ping. Pour jouer dans une autre région du monde, choisis-la dans les options du jeu : un VPN de pays (Proton VPN, payant ou limité) n'est utile que si le jeu bloque ta région.");
        let occupe = etat_ui(ui, &self.e);
        if warp_cli().is_none() {
            if ui.add_enabled(!occupe, egui::Button::new("📥 Installer Cloudflare WARP (gratuit)")).clicked() {
                en_fond(&self.e, || {
                    let o = std::process::Command::new("winget").args(["install", "--id", "Cloudflare.Warp", "-e", "--accept-package-agreements", "--accept-source-agreements"]).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
                    if o.status.success() { Ok("✔ WARP installé.".into()) } else { Err("installation impossible (winget absent ?) : télécharge-le sur 1.1.1.1".into()) }
                });
            }
        } else {
            if self.statut.lock().unwrap().is_empty() {
                *self.statut.lock().unwrap() = "…".into();
                let s = self.statut.clone();
                std::thread::spawn(move || *s.lock().unwrap() = warp(&["status"]).unwrap_or_else(|e| e));
            }
            let st = self.statut.lock().unwrap().clone();
            let connecte = st.contains("Connected") && !st.contains("Disconnected");
            ui.label(if connecte { RichText::new("🟢 WARP connecté").color(Color32::LIGHT_GREEN) } else { RichText::new("⚪ WARP déconnecté") });
            ui.horizontal(|ui| {
                if ui.add_enabled(!occupe, egui::Button::new(if connecte { "Déconnecter" } else { "Connecter" })).clicked() {
                    let s = self.statut.clone();
                    en_fond(&self.e, move || {
                        // première fois : enregistrement et acceptation des conditions
                        if !connecte {
                            let _ = warp(&["--accept-tos", "registration", "new"]);
                        }
                        let r = warp(&[if connecte { "disconnect" } else { "connect" }])?;
                        std::thread::sleep(Duration::from_secs(2));
                        *s.lock().unwrap() = warp(&["status"]).unwrap_or_default();
                        Ok(r)
                    });
                }
                if ui.small_button("⟳").clicked() {
                    self.statut.lock().unwrap().clear();
                }
            });
        }
        if r != avant {
            enregistrer("ping_overlay", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert!((pourcent(0, 10_000_000, 1.0, 4) - 25.0).abs() < 1e-3);
        assert_eq!(age_jours("2026-01-01", "2026-01-31"), Some(30));
        assert!(page_fabricant("NVIDIA GeForce RTX 4060").is_some());
        assert!(page_fabricant("Microsoft Basic Display").is_none());
        let j: serde_json::Value = serde_json::from_str(r#"{"libre":200e9,"total":500e9,"ssd":"SSD","ram":34e9,"demarrage":4,"tmp":1e8,"antivirus":true,"sig":1,"reboot":false,"allume":2,"pilote":"2026-10-01","disques_malades":0}"#).unwrap();
        let v = noter_bilan(&j, "2026-11-21");
        assert_eq!(v.iter().map(|p| p.gagne).sum::<u32>(), 100);
        assert_eq!(v.iter().map(|p| p.sur).sum::<u32>(), 100);
        let j2: serde_json::Value = serde_json::from_str(r#"{"libre":10e9,"total":500e9,"ssd":"HDD","ram":8e9,"demarrage":20,"tmp":9e9,"antivirus":false,"reboot":true,"allume":30,"pilote":"2024-01-01","disques_malades":1}"#).unwrap();
        assert!(noter_bilan(&j2, "2026-11-21").iter().map(|p| p.gagne).sum::<u32>() < 20);
        let c = csv(&[MesureVitesse { quand: 0, descente: 100.25, montee: 20.0, ping: 12 }]);
        assert!(c.contains(";100,2;20,0;12") || c.contains(";100,3;20,0;12"));
        let racine = Path::new("/partage");
        assert_eq!(chemin_sur(racine, "/a/b.txt?k=1"), Some(PathBuf::from("/partage/a/b.txt")));
        assert_eq!(chemin_sur(racine, "/a/../../secret?k=1"), None);
        assert_eq!(chemin_sur(racine, "/%2e%2e/x"), None);
        assert_eq!(encoder_url("Mes photos/été.jpg"), "Mes%20photos/%C3%A9t%C3%A9.jpg");
    }
}
