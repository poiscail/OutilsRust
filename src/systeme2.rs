//! Système : processus (93), services (94), pilotes (95), infos du PC (96), test de la RAM (99), batterie (100),
//! plans d'alimentation (101), historique des plantages (108), réparation de Windows (110).
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SANS_FENETRE: u32 = 0x0800_0000;

/// Lance un script PowerShell qui renvoie du JSON, dans un fil ; le résultat arrive dans `cible`.
fn charger_json<T: for<'de> Deserialize<'de> + Send + 'static>(script: &'static str, cible: Arc<Mutex<Option<Result<T, String>>>>) {
    std::thread::spawn(move || {
        let r = crate::optimiseur::ps(script).and_then(|s| serde_json::from_str::<T>(s.trim()).map_err(|e| format!("{e}")));
        *cible.lock().unwrap() = Some(r);
    });
}

fn admin_requis(ui: &mut egui::Ui) {
    if !crate::optimiseur::est_admin() {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "Certaines actions demandent les droits administrateur.");
            if ui.small_button("Relancer en admin").clicked() && crate::optimiseur::relancer_admin() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

fn cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(prog).args(args).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    if o.status.success() { Ok(s) } else { Err(s.trim().to_string()) }
}

// ---------------------------------------------------------------------------------------------------
// 93 · Processus
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Processus {
    pub pid: u32,
    pub nom: String,
    pub memoire: u64,
    pub temps_cpu: u64, // 100 ns cumulées
    pub cpu: f32,       // % depuis la dernière mesure
}

fn lister_processus() -> Vec<Processus> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ};
    let mut v = vec![];
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if s == INVALID_HANDLE_VALUE {
            return v;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(s, &mut e) != 0;
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let nom = String::from_utf16_lossy(&e.szExeFile[..n]);
            let (mut memoire, mut temps) = (0, 0);
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, e.th32ProcessID);
            if !h.is_null() {
                let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
                if K32GetProcessMemoryInfo(h, &mut m, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32) != 0 {
                    memoire = m.WorkingSetSize as u64;
                }
                let (mut a, mut b, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed());
                if GetProcessTimes(h, &mut a, &mut b, &mut k, &mut u) != 0 {
                    let f = |t: FILETIME| (t.dwHighDateTime as u64) << 32 | t.dwLowDateTime as u64;
                    temps = f(k) + f(u);
                }
                CloseHandle(h);
            }
            v.push(Processus { pid: e.th32ProcessID, nom, memoire, temps_cpu: temps, cpu: 0.0 });
            ok = Process32NextW(s, &mut e) != 0;
        }
        CloseHandle(s);
    }
    v
}

fn tuer(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let r = TerminateProcess(h, 1) != 0;
        windows_sys::Win32::Foundation::CloseHandle(h);
        r
    }
}

pub struct GestProcessus {
    liste: Vec<Processus>,
    avant: (Option<Instant>, HashMap<u32, u64>),
    tri: u8,
    recherche: String,
    grouper: bool,
}

impl GestProcessus {
    pub fn new() -> Self {
        GestProcessus { liste: vec![], avant: (None, HashMap::new()), tri: 1, recherche: String::new(), grouper: true }
    }

    fn actualiser(&mut self) {
        let mut l = lister_processus();
        let coeurs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
        if let Some(t) = self.avant.0 {
            let dt = t.elapsed().as_secs_f32() * 10_000_000.0 * coeurs;
            for p in &mut l {
                if let Some(a) = self.avant.1.get(&p.pid) {
                    p.cpu = (p.temps_cpu.saturating_sub(*a) as f32 / dt * 100.0).min(100.0);
                }
            }
        }
        self.avant = (Some(Instant::now()), l.iter().map(|p| (p.pid, p.temps_cpu)).collect());
        self.liste = l;
    }
}

impl Outil for GestProcessus {
    fn nom(&self) -> &'static str {
        "⚙ Processus"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Processus");
        if self.avant.0.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.actualiser();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍").desired_width(180.0));
            ui.label("Trier par");
            ui.selectable_value(&mut self.tri, 0, "nom");
            ui.selectable_value(&mut self.tri, 1, "processeur");
            ui.selectable_value(&mut self.tri, 2, "mémoire");
            ui.checkbox(&mut self.grouper, "regrouper par appli");
        });
        let q = self.recherche.to_lowercase();
        let mut v: Vec<(String, Vec<u32>, f32, u64)> = vec![];
        for p in self.liste.iter().filter(|p| p.pid > 4 && (q.is_empty() || p.nom.to_lowercase().contains(&q))) {
            match v.iter_mut().find(|x| self.grouper && x.0.eq_ignore_ascii_case(&p.nom)) {
                Some(x) => {
                    x.1.push(p.pid);
                    x.2 += p.cpu;
                    x.3 += p.memoire;
                }
                None => v.push((p.nom.clone(), vec![p.pid], p.cpu, p.memoire)),
            }
        }
        match self.tri {
            0 => v.sort_by_key(|x| x.0.to_lowercase()),
            1 => v.sort_by(|a, b| b.2.total_cmp(&a.2)),
            _ => v.sort_by(|a, b| b.3.cmp(&a.3)),
        }
        let total_mem: u64 = self.liste.iter().map(|p| p.memoire).sum();
        ui.weak(format!("{} processus · {} de mémoire utilisée par les applis", self.liste.len(), crate::disque::taille_lisible(total_mem)));
        let mut a_tuer = None;
        egui::ScrollArea::vertical().id_salt("proc").show(ui, |ui| {
            egui::Grid::new("g_proc").num_columns(4).striped(true).spacing([16.0, 4.0]).show(ui, |ui| {
                for (nom, pids, cpu, mem) in v.iter().take(200) {
                    ui.label(if pids.len() > 1 { format!("{nom} ({})", pids.len()) } else { nom.clone() });
                    ui.label(format!("{cpu:.1} %"));
                    ui.label(crate::disque::taille_lisible(*mem));
                    if ui.small_button("✖ Fermer").clicked() {
                        a_tuer = Some((nom.clone(), pids.clone()));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((nom, pids)) = a_tuer {
            let n = pids.iter().filter(|p| tuer(**p)).count();
            ui.ctx().debug_text(format!("{nom} : {n} fermé(s)"));
            self.avant.0 = None;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 94 · Services
// ---------------------------------------------------------------------------------------------------

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct Service {
    #[serde(rename = "Name")]
    nom: String,
    #[serde(rename = "DisplayName")]
    affiche: String,
    #[serde(rename = "State")]
    etat: String,
    #[serde(rename = "StartMode")]
    demarrage: String,
    #[serde(rename = "Description")]
    description: Option<String>,
}

type Charge<T> = Arc<Mutex<Option<Result<T, String>>>>;

pub struct Services {
    d: Charge<Vec<Service>>,
    recherche: String,
    message: String,
}

const SCRIPT_SERVICES: &str = "Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode,Description | ConvertTo-Json -Compress";

impl Services {
    pub fn new() -> Self {
        Services { d: Arc::default(), recherche: String::new(), message: String::new() }
    }
}

impl Outil for Services {
    fn nom(&self) -> &'static str {
        "🧩 Services Windows"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Services Windows");
        admin_requis(ui);
        let charge = self.d.lock().unwrap().clone();
        if charge.is_none() {
            *self.d.lock().unwrap() = Some(Err("chargement…".into()));
            charger_json(SCRIPT_SERVICES, self.d.clone());
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍 nom ou description").desired_width(240.0));
            if ui.button("⟳").clicked() {
                *self.d.lock().unwrap() = Some(Err("chargement…".into()));
                charger_json(SCRIPT_SERVICES, self.d.clone());
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let liste = match charge {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(400));
                return;
            }
            None => return,
        };
        let q = self.recherche.to_lowercase();
        let mut action = None;
        egui::ScrollArea::vertical().id_salt("svc").show(ui, |ui| {
            for s in liste.iter().filter(|s| q.is_empty() || s.affiche.to_lowercase().contains(&q) || s.nom.to_lowercase().contains(&q) || s.description.as_deref().unwrap_or("").to_lowercase().contains(&q)).take(300) {
                ui.horizontal(|ui| {
                    let c = if s.etat == "Running" { Color32::LIGHT_GREEN } else { Color32::GRAY };
                    ui.colored_label(c, "●");
                    ui.label(&s.affiche).on_hover_text(format!("{}\n\n{}", s.nom, s.description.as_deref().unwrap_or("(pas de description)")));
                    ui.weak(match s.demarrage.as_str() { "Auto" => "auto", "Manual" => "manuel", "Disabled" => "désactivé", x => x });
                    if s.etat == "Running" {
                        if ui.small_button("⏹").on_hover_text("Arrêter").clicked() {
                            action = Some(("stop", s.nom.clone(), ""));
                        }
                    } else if ui.small_button("▶").on_hover_text("Démarrer").clicked() {
                        action = Some(("start", s.nom.clone(), ""));
                    }
                    egui::ComboBox::from_id_salt(("dem", &s.nom)).selected_text("démarrage…").width(100.0).show_ui(ui, |ui| {
                        for (m, n) in [("auto", "automatique"), ("demand", "manuel"), ("disabled", "désactivé")] {
                            if ui.selectable_label(false, n).clicked() {
                                action = Some(("config", s.nom.clone(), m));
                            }
                        }
                    });
                });
            }
        });
        if let Some((a, nom, mode)) = action {
            let r = match a {
                "config" => cmd("sc.exe", &["config", &nom, "start=", mode]),
                _ => cmd("sc.exe", &[a, &nom]),
            };
            self.message = match r {
                Ok(_) => format!("✔ {nom} : fait"),
                Err(e) => format!("✖ {nom} : {}", e.lines().last().unwrap_or("")),
            };
            *self.d.lock().unwrap() = Some(Err("chargement…".into()));
            charger_json(SCRIPT_SERVICES, self.d.clone());
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 95 · Pilotes / 96 · Infos du PC
// ---------------------------------------------------------------------------------------------------

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct Pilote {
    nom: Option<String>,
    version: Option<String>,
    date: Option<String>, // AAAA-MM-JJ
    fabricant: Option<String>,
    classe: Option<String>,
}

const SCRIPT_PILOTES: &str = "Get-CimInstance Win32_PnPSignedDriver | Where-Object DeviceName | ForEach-Object { [pscustomobject]@{ nom=$_.DeviceName; version=$_.DriverVersion; date=$(if($_.DriverDate){$_.DriverDate.ToString('yyyy-MM-dd')}); fabricant=$_.Manufacturer; classe=$_.DeviceClass } } | ConvertTo-Json -Compress";

/// Lien vers la page de pilotes du fabricant de la carte graphique.
pub fn lien_pilote(fabricant: &str) -> Option<&'static str> {
    let f = fabricant.to_lowercase();
    if f.contains("nvidia") {
        Some("https://www.nvidia.com/fr-fr/drivers/")
    } else if f.contains("amd") || f.contains("advanced micro") {
        Some("https://www.amd.com/fr/support/download/drivers.html")
    } else if f.contains("intel") {
        Some("https://www.intel.fr/content/www/fr/fr/support/detect.html")
    } else {
        None
    }
}

pub struct Pilotes {
    d: Charge<Vec<Pilote>>,
    lance: bool,
    seulement_vieux: bool,
}

impl Pilotes {
    pub fn new() -> Self {
        Pilotes { d: Arc::default(), lance: false, seulement_vieux: false }
    }
}

impl Outil for Pilotes {
    fn nom(&self) -> &'static str {
        "🛠 Pilotes"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pilotes");
        if !self.lance {
            self.lance = true;
            charger_json(SCRIPT_PILOTES, self.d.clone());
        }
        let d = self.d.lock().unwrap().clone();
        let Some(r) = d else {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(400));
            return;
        };
        let l = match r {
            Ok(l) => l,
            Err(e) => {
                ui.colored_label(Color32::LIGHT_RED, e);
                return;
            }
        };
        let annee = crate::quotidien::date_locale(crate::pressepapiers::maintenant_utc(), 0)[..4].parse::<i32>().unwrap_or(2026);
        let vieux = |p: &Pilote| p.date.as_deref().and_then(|d| d.get(..4)?.parse::<i32>().ok()).is_some_and(|a| annee - a >= 3);
        for g in l.iter().filter(|p| p.classe.as_deref() == Some("DISPLAY")) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("🎮 {}", g.nom.as_deref().unwrap_or("?")));
                    ui.label(format!("version {} du {}", g.version.as_deref().unwrap_or("?"), g.date.as_deref().unwrap_or("?")));
                    if let Some(u) = lien_pilote(g.fabricant.as_deref().unwrap_or("")) {
                        ui.hyperlink_to("Télécharger le dernier pilote", u);
                    }
                });
            });
        }
        ui.checkbox(&mut self.seulement_vieux, "Seulement les pilotes de plus de 3 ans");
        egui::ScrollArea::vertical().id_salt("pilotes").show(ui, |ui| {
            egui::Grid::new("g_pil").num_columns(4).striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                for p in l.iter().filter(|p| !self.seulement_vieux || vieux(p)) {
                    ui.label(p.nom.as_deref().unwrap_or("?"));
                    ui.weak(p.classe.as_deref().unwrap_or(""));
                    ui.label(p.version.as_deref().unwrap_or(""));
                    let d = p.date.as_deref().unwrap_or("");
                    if vieux(p) { ui.colored_label(Color32::from_rgb(255, 190, 80), d) } else { ui.label(d) };
                    ui.end_row();
                }
            });
        });
        ui.weak("Les vieux pilotes de puces système sont souvent normaux ; ceux de la carte graphique valent le coup d'être à jour pour les jeux.");
    }
}

const SCRIPT_INFOS: &str = r#"
$os = Get-CimInstance Win32_OperatingSystem; $cs = Get-CimInstance Win32_ComputerSystem
$l = @()
$l += "Windows : $($os.Caption) $($os.Version) (build $($os.BuildNumber))"
$l += "PC : $($cs.Manufacturer) $($cs.Model)"
foreach ($c in Get-CimInstance Win32_Processor) { $l += "Processeur : $($c.Name.Trim()) · $($c.NumberOfCores) cœurs / $($c.NumberOfLogicalProcessors) threads · $($c.MaxClockSpeed) MHz" }
foreach ($g in Get-CimInstance Win32_VideoController) { $l += "Carte graphique : $($g.Name) · pilote $($g.DriverVersion) · $($g.CurrentHorizontalResolution)x$($g.CurrentVerticalResolution) $($g.CurrentRefreshRate) Hz" }
$ram = Get-CimInstance Win32_PhysicalMemory
$l += "Mémoire : $([math]::Round(($ram | Measure-Object Capacity -Sum).Sum / 1GB)) Go en $($ram.Count) barrette(s) · $(($ram | Select-Object -First 1).ConfiguredClockSpeed) MHz · $(($ram | Select-Object -First 1).Manufacturer)"
$b = Get-CimInstance Win32_BaseBoard; $l += "Carte mère : $($b.Manufacturer) $($b.Product)"
$bios = Get-CimInstance Win32_BIOS; $l += "BIOS : $($bios.Manufacturer) $($bios.SMBIOSBIOSVersion) ($($bios.ReleaseDate.ToString('yyyy-MM-dd')))"
foreach ($d in Get-CimInstance Win32_DiskDrive) { $l += "Disque : $($d.Model) · $([math]::Round($d.Size / 1GB)) Go" }
foreach ($n in Get-CimInstance Win32_NetworkAdapter -Filter 'PhysicalAdapter=True AND NetEnabled=True') { $l += "Réseau : $($n.Name)" }
$l += "Démarré le : $($os.LastBootUpTime.ToString('dd/MM/yyyy HH:mm'))"
ConvertTo-Json -InputObject @($l) -Compress
"#;

pub struct InfosPc {
    d: Charge<Vec<String>>,
    lance: bool,
}

impl InfosPc {
    pub fn new() -> Self {
        InfosPc { d: Arc::default(), lance: false }
    }
}

impl Outil for InfosPc {
    fn nom(&self) -> &'static str {
        "💻 Infos du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Infos du PC");
        if !self.lance {
            self.lance = true;
            charger_json(SCRIPT_INFOS, self.d.clone());
        }
        match self.d.lock().unwrap().clone() {
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            Some(Ok(l)) => {
                if ui.button("📋 Copier la fiche (pour demander de l'aide)").clicked() {
                    ui.ctx().copy_text(l.join("\n"));
                }
                ui.add_space(6.0);
                for x in &l {
                    let (a, b) = x.split_once(" : ").unwrap_or(("", x));
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(a);
                        ui.label(b);
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 99 · RAM, 100 · Batterie, 101 · Alimentation
// ---------------------------------------------------------------------------------------------------

const SCRIPT_RAM: &str = "@(Get-WinEvent -FilterHashtable @{LogName='System'; ProviderName='Microsoft-Windows-MemoryDiagnostics-Results'} -MaxEvents 5 -ErrorAction SilentlyContinue | ForEach-Object { $_.TimeCreated.ToString('dd/MM/yyyy HH:mm') + ' : ' + $_.Message }) | ConvertTo-Json -Compress";

const SCRIPT_BATTERIE: &str = r#"
$b = Get-CimInstance Win32_Battery
if (-not $b) { '{"presente":false}'; exit }
$plein = (Get-CimInstance -Namespace root\wmi BatteryFullChargedCapacity -ErrorAction SilentlyContinue | Select-Object -First 1).FullChargedCapacity
$prevu = (Get-CimInstance -Namespace root\wmi BatteryStaticData -ErrorAction SilentlyContinue | Select-Object -First 1).DesignedCapacity
$cycles = (Get-CimInstance -Namespace root\wmi BatteryCycleCount -ErrorAction SilentlyContinue | Select-Object -First 1).CycleCount
[pscustomobject]@{ presente=$true; charge=$b.EstimatedChargeRemaining; minutes=$b.EstimatedRunTime; secteur=($b.BatteryStatus -eq 2); plein=$plein; prevu=$prevu; cycles=$cycles } | ConvertTo-Json -Compress
"#;

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct Batterie {
    presente: bool,
    charge: Option<u32>,
    minutes: Option<u32>,
    secteur: bool,
    plein: Option<u64>,
    prevu: Option<u64>,
    cycles: Option<u64>,
}

/// Plans de `powercfg /list` : (guid, nom, actif).
pub fn lire_plans(s: &str) -> Vec<(String, String, bool)> {
    s.lines().filter_map(|l| {
        let i = l.find(':')?;
        let reste = l[i + 1..].trim();
        let guid = reste.split_whitespace().next()?;
        if guid.len() != 36 {
            return None;
        }
        let nom = reste.split_once('(')?.1.split_once(')')?.0.to_string();
        Some((guid.to_string(), nom, reste.trim_end().ends_with('*')))
    }).collect()
}

pub struct Energie {
    ram: Charge<Vec<String>>,
    batterie: Charge<Batterie>,
    plans: Vec<(String, String, bool)>,
    lance: bool,
    message: String,
}

impl Energie {
    pub fn new() -> Self {
        Energie { ram: Arc::default(), batterie: Arc::default(), plans: vec![], lance: false, message: String::new() }
    }
}

impl Outil for Energie {
    fn nom(&self) -> &'static str {
        "🔋 Batterie, alimentation, RAM"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Batterie, alimentation et mémoire");
        if !self.lance {
            self.lance = true;
            charger_json(SCRIPT_RAM, self.ram.clone());
            charger_json(SCRIPT_BATTERIE, self.batterie.clone());
            self.plans = lire_plans(&cmd("powercfg", &["/list"]).unwrap_or_default());
        }
        ui.strong("⚡ Plan d'alimentation");
        ui.horizontal_wrapped(|ui| {
            for (g, n, actif) in self.plans.clone() {
                if ui.selectable_label(actif, &n).clicked() {
                    self.message = cmd("powercfg", &["/setactive", &g]).map(|_| format!("Plan « {n} » activé.")).unwrap_or_else(|e| e);
                    self.plans = lire_plans(&cmd("powercfg", &["/list"]).unwrap_or_default());
                }
            }
            if !self.plans.iter().any(|p| p.0 == "e9a42b02-d5df-448d-aa00-03f14749eb61" || p.1.contains("optimales") || p.1.contains("Ultimate")) && ui.small_button("➕ Performances optimales").on_hover_text("Plan caché de Windows, pour les PC fixes de jeu").clicked() {
                self.message = cmd("powercfg", &["-duplicatescheme", "e9a42b02-d5df-448d-aa00-03f14749eb61"]).map(|_| "Plan ajouté.".into()).unwrap_or_else(|e| e);
                self.plans = lire_plans(&cmd("powercfg", &["/list"]).unwrap_or_default());
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.separator();
        ui.strong("🔋 Batterie");
        match self.batterie.lock().unwrap().clone() {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.weak(e);
            }
            Some(Ok(b)) if !b.presente => {
                ui.weak("Pas de batterie (PC fixe).");
            }
            Some(Ok(b)) => {
                ui.label(RichText::new(format!("{} % {}", b.charge.unwrap_or(0), if b.secteur { "· branché" } else { "" })).size(22.0));
                if let Some(m) = b.minutes.filter(|m| *m < 10_000 && !b.secteur) {
                    ui.label(format!("Autonomie estimée : {} h {:02}", m / 60, m % 60));
                }
                if let (Some(p), Some(d)) = (b.plein, b.prevu.filter(|d| *d > 0)) {
                    let sante = p as f32 / d as f32;
                    ui.add(egui::ProgressBar::new(sante.min(1.0)).desired_width(260.0).text(format!("Santé {:.0} % ({p} / {d} mWh)", sante * 100.0)));
                }
                if let Some(c) = b.cycles.filter(|c| *c > 0) {
                    ui.label(format!("{c} cycles de charge"));
                }
                if ui.button("Rapport complet de Windows").clicked() {
                    let f = std::env::temp_dir().join("rapport_batterie.html");
                    let _ = cmd("powercfg", &["/batteryreport", "/output", &f.to_string_lossy()]);
                    crate::outil::ouvrir(&f);
                }
            }
        }
        ui.separator();
        ui.strong("🧠 Test de la mémoire vive");
        ui.weak("Le diagnostic de Windows redémarre le PC et teste la RAM avant Windows (quelques minutes). Le résultat apparaît ici ensuite.");
        if ui.button("Lancer le diagnostic mémoire (redémarre le PC)…").clicked() {
            let _ = Command::new("mdsched.exe").spawn();
        }
        match self.ram.lock().unwrap().clone() {
            Some(Ok(l)) if !l.is_empty() => {
                for x in l {
                    ui.label(x);
                }
            }
            Some(_) => {
                ui.weak("Aucun résultat de test enregistré.");
            }
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 108 · Plantages
// ---------------------------------------------------------------------------------------------------

const SCRIPT_PLANTAGES: &str = r#"
$debut = (Get-Date).AddDays(-90); $r = @()
Get-WinEvent -FilterHashtable @{LogName='System'; Id=1001,41; StartTime=$debut} -ErrorAction SilentlyContinue | Where-Object { $_.Id -eq 41 -or $_.ProviderName -match 'WER-SystemErrorReporting|BugCheck' } | ForEach-Object {
  $r += [pscustomobject]@{ t=$_.TimeCreated.ToString('dd/MM/yyyy HH:mm'); genre=$(if($_.Id -eq 41){'arret'}else{'ecran_bleu'}); appli=''; detail=$(if($_.Id -eq 1001){[string]$_.Properties[0].Value}else{''}) } }
Get-WinEvent -FilterHashtable @{LogName='Application'; Id=1000,1002; StartTime=$debut} -MaxEvents 300 -ErrorAction SilentlyContinue | ForEach-Object {
  $r += [pscustomobject]@{ t=$_.TimeCreated.ToString('dd/MM/yyyy HH:mm'); genre=$(if($_.Id -eq 1000){'plantage'}else{'gel'}); appli=[string]$_.Properties[0].Value; detail=$(if($_.Id -eq 1000){[string]$_.Properties[3].Value}else{''}) } }
ConvertTo-Json -InputObject @($r) -Compress
"#;

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
struct Plantage {
    t: String,
    genre: String,
    appli: String,
    detail: String,
}

/// Explication courte des codes d'écran bleu les plus fréquents.
pub fn ecran_bleu(code: &str) -> &'static str {
    let c = code.trim().trim_start_matches("0x").trim_start_matches('0').to_lowercase();
    match c.as_str() {
        "a" => "IRQL_NOT_LESS_OR_EQUAL : souvent un pilote ou la RAM",
        "d1" => "DRIVER_IRQL_NOT_LESS_OR_EQUAL : pilote fautif (réseau, carte graphique…)",
        "1a" => "MEMORY_MANAGEMENT : souvent la RAM (lance le test mémoire)",
        "3b" => "SYSTEM_SERVICE_EXCEPTION : pilote ou logiciel système",
        "50" => "PAGE_FAULT_IN_NONPAGED_AREA : RAM, disque ou pilote",
        "7e" | "1000007e" => "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED : pilote",
        "9f" => "DRIVER_POWER_STATE_FAILURE : pilote et mise en veille",
        "116" | "117" => "VIDEO_TDR : la carte graphique ne répond plus (pilote, chauffe, overclock)",
        "124" => "WHEA_UNCORRECTABLE_ERROR : erreur matérielle (processeur, chauffe, overclock, alimentation)",
        "133" => "DPC_WATCHDOG_VIOLATION : pilote de stockage ou firmware du SSD",
        "139" => "KERNEL_SECURITY_CHECK_FAILURE : pilote ou RAM",
        "ef" => "CRITICAL_PROCESS_DIED : fichier système abîmé (lance la réparation)",
        _ => "",
    }
}

pub struct Plantages {
    d: Charge<Vec<Plantage>>,
    lance: bool,
}

impl Plantages {
    pub fn new() -> Self {
        Plantages { d: Arc::default(), lance: false }
    }
}

impl Outil for Plantages {
    fn nom(&self) -> &'static str {
        "💥 Historique des plantages"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Historique des plantages (90 jours)");
        if !self.lance {
            self.lance = true;
            charger_json(SCRIPT_PLANTAGES, self.d.clone());
        }
        let l = match self.d.lock().unwrap().clone() {
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
                return;
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
                return;
            }
            Some(Ok(l)) => l,
        };
        let compte = |g: &str| l.iter().filter(|p| p.genre == g).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("💙 {} écran(s) bleu(s)", compte("ecran_bleu"))).strong());
            ui.label(RichText::new(format!("⏻ {} arrêt(s) brutal(aux)", compte("arret"))).strong());
            ui.label(RichText::new(format!("💥 {} appli(s) plantée(s)", compte("plantage"))).strong());
            ui.label(RichText::new(format!("🧊 {} gel(s)", compte("gel"))).strong());
        });
        let mut par_appli: HashMap<String, usize> = HashMap::new();
        for p in l.iter().filter(|p| !p.appli.is_empty()) {
            *par_appli.entry(p.appli.clone()).or_default() += 1;
        }
        let mut v: Vec<_> = par_appli.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        if !v.is_empty() {
            ui.weak(format!("Applis qui plantent le plus : {}", v.iter().take(5).map(|(a, n)| format!("{a} ({n})")).collect::<Vec<_>>().join(", ")));
        }
        ui.separator();
        egui::ScrollArea::vertical().id_salt("plantages").show(ui, |ui| {
            for p in &l {
                ui.horizontal_wrapped(|ui| {
                    ui.weak(&p.t);
                    match p.genre.as_str() {
                        "ecran_bleu" => {
                            ui.colored_label(Color32::from_rgb(110, 160, 255), format!("Écran bleu {}", p.detail));
                            ui.weak(ecran_bleu(&p.detail));
                        }
                        "arret" => {
                            ui.colored_label(Color32::from_rgb(255, 190, 80), "Arrêt brutal (coupure, bouton, gel complet)");
                        }
                        "gel" => {
                            ui.label(format!("🧊 {} ne répondait plus", p.appli));
                        }
                        _ => {
                            ui.label(format!("💥 {} a planté", p.appli));
                            if !p.detail.is_empty() {
                                ui.weak(format!("(module {})", p.detail));
                            }
                        }
                    }
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 110 · Réparation de Windows
// ---------------------------------------------------------------------------------------------------

/// Sortie de sfc (UTF-16 sur la console) en texte.
pub fn texte_console(b: &[u8]) -> String {
    let impairs: Vec<u8> = b.iter().skip(1).step_by(2).take(200).copied().collect();
    let utf16 = impairs.len() >= 2 && impairs.iter().filter(|&&x| x == 0).count() * 10 >= impairs.len() * 6;
    let s = if utf16 { String::from_utf16_lossy(&b.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect::<Vec<_>>()) } else { String::from_utf8_lossy(b).into_owned() };
    s.replace('\0', "").replace('\r', "")
}

pub struct Reparation {
    etat: Arc<Mutex<(bool, Vec<String>)>>,
}

impl Reparation {
    pub fn new() -> Self {
        Reparation { etat: Arc::new(Mutex::new((false, vec![]))) }
    }

    fn lancer(&self, etapes: Vec<(&'static str, &'static str, Vec<&'static str>)>) {
        let e = self.etat.clone();
        e.lock().unwrap().0 = true;
        std::thread::spawn(move || {
            for (titre, prog, args) in etapes {
                e.lock().unwrap().1.push(format!("▶ {titre}…"));
                let r = Command::new(prog).args(&args).creation_flags(SANS_FENETRE).output();
                let ligne = match r {
                    Ok(o) => {
                        let t = texte_console(&o.stdout);
                        let fin: Vec<&str> = t.lines().map(str::trim).filter(|l| !l.is_empty() && !l.contains('%')).collect();
                        format!("{} {titre} : {}", if o.status.success() { "✔" } else { "⚠" }, fin.iter().rev().take(2).rev().cloned().collect::<Vec<_>>().join(" "))
                    }
                    Err(x) => format!("✖ {titre} : {x}"),
                };
                e.lock().unwrap().1.push(ligne);
            }
            e.lock().unwrap().0 = false;
        });
    }
}

impl Outil for Reparation {
    fn nom(&self) -> &'static str {
        "🩹 Réparer Windows"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Réparer Windows");
        ui.weak("Vérifie et répare les fichiers système abîmés (outils officiels SFC et DISM). Compte 10 à 30 minutes ; le PC reste utilisable.");
        admin_requis(ui);
        let (occupe, journal) = self.etat.lock().unwrap().clone();
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new(RichText::new("🩹 Réparation complète").size(16.0))).clicked() {
                self.lancer(vec![
                    ("Réparation de l'image Windows (DISM)", "DISM.exe", vec!["/Online", "/Cleanup-Image", "/RestoreHealth"]),
                    ("Vérification des fichiers système (SFC)", "sfc.exe", vec!["/scannow"]),
                ]);
            }
            if ui.add_enabled(!occupe, egui::Button::new("Seulement SFC")).clicked() {
                self.lancer(vec![("Vérification des fichiers système (SFC)", "sfc.exe", vec!["/scannow"])]);
            }
            if ui.add_enabled(!occupe, egui::Button::new("Analyser le disque C:")).clicked() {
                self.lancer(vec![("Analyse du disque C: (chkdsk en ligne)", "chkdsk.exe", vec!["C:", "/scan"])]);
            }
        });
        if occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("En cours, ne ferme pas Outils…");
            });
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        for l in &journal {
            ui.label(l);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans() {
        let s = "Modes de gestion de l'alimentation existants (* Actif)\n-----------------------------------\nGUID du mode de gestion de l'alimentation : 381b4222-f694-41f0-9685-ff5bb260df2e  (Utilisation normale) *\nGUID du mode de gestion de l'alimentation : 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Performances élevées)\n";
        let p = lire_plans(s);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], ("381b4222-f694-41f0-9685-ff5bb260df2e".into(), "Utilisation normale".into(), true));
        assert!(!p[1].2);
    }

    #[test]
    fn divers() {
        assert!(ecran_bleu("0x00000124").starts_with("WHEA"));
        assert_eq!(lien_pilote("NVIDIA"), Some("https://www.nvidia.com/fr-fr/drivers/"));
        let u: Vec<u8> = "Réparé\r\n".encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        assert_eq!(texte_console(&u), "Réparé\n");
        assert_eq!(texte_console(b"ok\r\n"), "ok\n");
    }
}
