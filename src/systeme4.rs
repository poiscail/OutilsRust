//! Système, suite : historique des périphériques USB (33), désinstalleur complet (34), alerte de surchauffe (35),
//! éteindre quand… (36), luminosité et lumière bleue (37), polices (38), mémoire en veille (39), fiche du PC (40).
use crate::optimiseur::{est_admin, exp, ps, reg_dword, reg_sous_cles, reg_texte, relancer_admin};
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SANS_FENETRE: u32 = 0x0800_0000;

type Etat = Arc<Mutex<(bool, String)>>;

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

fn taille(o: u64) -> String {
    crate::disque::taille_lisible(o)
}

fn json<T: for<'de> Deserialize<'de>>(script: &str) -> Result<T, String> {
    let s = ps(script)?;
    let s = s.trim();
    if s.is_empty() || s == "null" {
        return serde_json::from_str("[]").map_err(|e| e.to_string());
    }
    // un seul objet → tableau d'un élément
    let s = if s.starts_with('{') { format!("[{s}]") } else { s.to_string() };
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------------
// 33 · Historique des périphériques USB
// ---------------------------------------------------------------------------------------------------

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Peripherique {
    #[serde(rename = "N")]
    pub nom: String,
    #[serde(rename = "C")]
    pub classe: String,
    #[serde(rename = "P")]
    pub present: bool,
    #[serde(rename = "F")]
    pub premier: String,
    #[serde(rename = "D")]
    pub dernier: String,
}

const PS_USB: &str = r#"$l = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object { ($_.InstanceId -like 'USBSTOR*' -or $_.InstanceId -like 'SWD\WPDBUSENUM*' -or ($_.InstanceId -like 'USB\*' -and $_.Class -in 'Image','Camera','Media','AudioEndpoint','Bluetooth','WPD','Printer','Net')) -or ($_.InstanceId -like 'HID\*' -and $_.Class -in 'Mouse','Keyboard') }
$o = foreach ($d in $l) {
  $p = Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName 'DEVPKEY_Device_FirstInstallDate','DEVPKEY_Device_LastArrivalDate' -ErrorAction SilentlyContinue
  $f = ($p | Where-Object KeyName -eq 'DEVPKEY_Device_FirstInstallDate').Data
  $a = ($p | Where-Object KeyName -eq 'DEVPKEY_Device_LastArrivalDate').Data
  [pscustomobject]@{ N = "$($d.FriendlyName)"; C = "$($d.Class)"; P = ($d.Status -eq 'OK'); F = $(if ($f) { $f.ToString('yyyy-MM-dd HH:mm') } else { '' }); D = $(if ($a) { $a.ToString('yyyy-MM-dd HH:mm') } else { '' }) }
}
ConvertTo-Json -Compress -InputObject @($o | Where-Object N)"#;

fn classe_lisible(c: &str) -> &'static str {
    match c {
        "DiskDrive" | "WPD" => "💾 Stockage",
        "Mouse" => "🖱 Souris",
        "Keyboard" => "⌨ Clavier",
        "Image" | "Camera" => "📷 Caméra",
        "Media" | "AudioEndpoint" => "🎧 Audio",
        "Bluetooth" => "📶 Bluetooth",
        "Printer" => "🖨 Imprimante",
        "Net" => "🌐 Réseau",
        _ => "🔌 Autre",
    }
}

pub struct HistoriqueUsb {
    l: Arc<Mutex<Option<Result<Vec<Peripherique>, String>>>>,
    filtre: String,
    alerte: Arc<AtomicBool>,
}

impl HistoriqueUsb {
    pub fn new() -> Self {
        let alerte = Arc::new(AtomicBool::new(charger::<Option<bool>>("alerte_usb").unwrap_or(false)));
        let a = alerte.clone();
        // nouvelle clé / disque branché → notification
        std::thread::spawn(move || {
            let lettres = || unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
            let mut avant = lettres();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let m = lettres();
                let nouveaux = m & !avant;
                if nouveaux != 0 && a.load(Relaxed) {
                    let l: Vec<String> = (0..26).filter(|i| nouveaux & (1 << i) != 0).map(|i| format!("{}:", (b'A' + i as u8) as char)).collect();
                    crate::systeme::notifier("Périphérique branché", &format!("Nouveau lecteur : {}", l.join(", ")));
                }
                avant = m;
            }
        });
        HistoriqueUsb { l: Arc::default(), filtre: String::new(), alerte }
    }
}

impl Outil for HistoriqueUsb {
    fn nom(&self) -> &'static str {
        "🔌 Historique des périphériques"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Historique des périphériques branchés");
        ui.weak("Toutes les clés USB, disques, téléphones, souris, claviers… déjà branchés sur ce PC, avec la date de la première et de la dernière fois.");
        let mut a = self.alerte.load(Relaxed);
        if ui.checkbox(&mut a, "Me prévenir quand une clé ou un disque est branché").changed() {
            self.alerte.store(a, Relaxed);
            enregistrer("alerte_usb", &Some(a));
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 filtrer").desired_width(200.0));
            if ui.small_button("⟳").clicked() {
                *self.l.lock().unwrap() = None;
            }
        });
        let l = self.l.lock().unwrap().clone();
        match l {
            None => {
                *self.l.lock().unwrap() = Some(Err("lecture (quelques secondes)…".into()));
                let c = self.l.clone();
                std::thread::spawn(move || {
                    let r = json::<Vec<Peripherique>>(PS_USB).map(|mut v| {
                        v.sort_by(|a, b| b.dernier.cmp(&a.dernier));
                        v
                    });
                    *c.lock().unwrap() = Some(r);
                });
            }
            Some(Err(e)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            Some(Ok(v)) => {
                let f = self.filtre.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("usb").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        ui.strong("Type");
                        ui.strong("Nom");
                        ui.strong("Première fois");
                        ui.strong("Dernière fois");
                        ui.end_row();
                        for p in v.iter().filter(|p| f.is_empty() || p.nom.to_lowercase().contains(&f)) {
                            ui.label(classe_lisible(&p.classe));
                            ui.label(if p.present { RichText::new(format!("● {}", p.nom)).color(Color32::LIGHT_GREEN) } else { RichText::new(&p.nom) });
                            ui.weak(&p.premier);
                            ui.label(&p.dernier);
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 34 · Désinstalleur complet
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Programme {
    pub nom: String,
    pub editeur: String,
    pub version: String,
    pub dossier: String,
    pub desinstaller: String,
    pub taille: u64,
    pub date: String,
    pub cle: String,
}

fn programmes() -> Vec<Programme> {
    let mut v: Vec<Programme> = vec![];
    for base in [r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall"] {
        for k in reg_sous_cles(base) {
            let c = format!(r"{base}\{k}");
            let t = |n: &str| reg_texte(&c, n).unwrap_or_default();
            let nom = t("DisplayName");
            if nom.is_empty() || reg_dword(&c, "SystemComponent") == Some(1) || !t("ParentKeyName").is_empty() {
                continue;
            }
            let des = t("UninstallString");
            if des.is_empty() {
                continue;
            }
            let d = t("InstallDate");
            v.push(Programme {
                nom,
                editeur: t("Publisher"),
                version: t("DisplayVersion"),
                dossier: t("InstallLocation").trim_matches('"').to_string(),
                desinstaller: des,
                taille: reg_dword(&c, "EstimatedSize").unwrap_or(0) as u64 * 1024,
                date: if d.len() == 8 { format!("{}/{}/{}", &d[6..8], &d[4..6], &d[..4]) } else { String::new() },
                cle: c,
            });
        }
    }
    v.sort_by_key(|p| p.nom.to_lowercase());
    v.dedup_by(|a, b| a.nom == b.nom && a.version == b.version);
    v
}

/// Mots significatifs d'un nom (pour retrouver les restes).
pub fn mots_cles(nom: &str, editeur: &str) -> Vec<String> {
    let vides = ["inc", "ltd", "llc", "corporation", "corp", "gmbh", "software", "the", "for", "and", "edition", "version", "x64", "x86", "64-bit", "32-bit", "bit", "update", "setup", "launcher"];
    let mut v: Vec<String> = format!("{nom} {editeur}").split(|c: char| !c.is_alphanumeric()).map(|m| m.to_lowercase()).filter(|m| m.len() >= 4 && !vides.contains(&m.as_str()) && !m.chars().all(|c| c.is_ascii_digit())).collect();
    v.sort();
    v.dedup();
    v
}

/// Un dossier ressemble-t-il au programme ? (nom de dossier = un mot clé du nom, ou nom complet sans espaces)
pub fn ressemble(dossier: &str, nom: &str, mots: &[String]) -> bool {
    const GENERIQUES: [&str; 14] = ["microsoft", "windows", "common files", "windowsapps", "packages", "temp", "intel", "nvidia", "amd", "google", "apple", "programs", "microsoft corporation", "package cache"];
    let d = dossier.to_lowercase();
    if GENERIQUES.contains(&d.as_str()) {
        return false;
    }
    let n: String = nom.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    let dn: String = d.chars().filter(|c| c.is_alphanumeric()).collect();
    (!n.is_empty() && (dn == n || (dn.len() >= 5 && n.starts_with(&dn) && dn.len() * 10 >= n.len() * 6))) || mots.iter().any(|m| d == *m)
}

fn restes(p: &Programme) -> Vec<PathBuf> {
    let mots = mots_cles(&p.nom, &p.editeur);
    let mut v = vec![];
    if !p.dossier.is_empty() && Path::new(&p.dossier).is_dir() {
        v.push(PathBuf::from(&p.dossier));
    }
    for base in ["%ProgramFiles%", "%ProgramFiles(x86)%", "%APPDATA%", "%LOCALAPPDATA%", "%ProgramData%", "%LOCALAPPDATA%\\Programs"] {
        let b = PathBuf::from(exp(base));
        for e in std::fs::read_dir(&b).into_iter().flatten().flatten() {
            if e.path().is_dir() && ressemble(&e.file_name().to_string_lossy(), &p.nom, &mots) && !v.contains(&e.path()) {
                v.push(e.path());
            }
        }
    }
    // jamais de dossiers système
    v.retain(|d| {
        let s = d.to_string_lossy().to_lowercase();
        !s.ends_with("\\microsoft") && !s.ends_with("\\windows") && !s.ends_with("\\common files") && s.matches('\\').count() >= 2
    });
    v
}

fn taille_dossier(p: &Path) -> u64 {
    std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| match e.metadata() {
        Ok(m) if m.is_dir() => taille_dossier(&e.path()),
        Ok(m) => m.len(),
        _ => 0,
    }).sum()
}

pub struct Desinstalleur {
    liste: Vec<Programme>,
    filtre: String,
    nettoyage: Option<(Programme, Vec<(PathBuf, u64, bool)>)>,
    attente: Option<Arc<Mutex<Option<(Programme, Vec<(PathBuf, u64, bool)>)>>>>,
    e: Etat,
}

impl Desinstalleur {
    pub fn new() -> Self {
        Desinstalleur { liste: vec![], filtre: String::new(), nettoyage: None, attente: None, e: Arc::default() }
    }
}

impl Outil for Desinstalleur {
    fn nom(&self) -> &'static str {
        "🧹 Désinstalleur complet"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Désinstalleur complet");
        ui.weak("Désinstalle un programme avec son propre désinstalleur, puis cherche les dossiers qu'il a laissés (Program Files, AppData, ProgramData) pour les mettre à la corbeille.");
        let occupe = etat_ui(ui, &self.e);
        if let Some((p, l)) = &mut self.nettoyage {
            ui.strong(format!("Restes de « {} »", p.nom));
            if l.is_empty() {
                ui.label("✔ Aucun reste trouvé.");
            }
            for (d, t, c) in l.iter_mut() {
                ui.checkbox(c, format!("{} ({})", d.display(), taille(*t)));
            }
            let choisis: Vec<PathBuf> = l.iter().filter(|x| x.2).map(|x| x.0.clone()).collect();
            let (mut jeter, mut fini) = (false, false);
            ui.horizontal(|ui| {
                jeter = ui.add_enabled(!choisis.is_empty(), egui::Button::new(format!("🗑 Mettre {} dossier(s) à la corbeille", choisis.len()))).clicked();
                fini = ui.button("Terminé").clicked();
            });
            if jeter {
                let n = choisis.iter().filter(|d| crate::disque::corbeille(d)).count();
                *self.e.lock().unwrap() = (false, format!("✔ {n} dossier(s) à la corbeille."));
            }
            if jeter || fini {
                self.nettoyage = None;
                self.liste.clear();
            }
            return;
        }
        if self.liste.is_empty() {
            self.liste = programmes();
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 chercher un programme").desired_width(240.0));
            ui.label(format!("{} programmes", self.liste.len()));
            if ui.small_button("⟳").clicked() {
                self.liste = programmes();
            }
        });
        let f = self.filtre.to_lowercase();
        let mut choisi = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("progs").striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                for p in self.liste.iter().filter(|p| f.is_empty() || p.nom.to_lowercase().contains(&f) || p.editeur.to_lowercase().contains(&f)) {
                    ui.label(RichText::new(&p.nom).strong());
                    ui.weak(&p.editeur);
                    ui.weak(if p.taille > 0 { taille(p.taille) } else { String::new() });
                    ui.weak(&p.date);
                    if ui.add_enabled(!occupe, egui::Button::new("Désinstaller")).clicked() {
                        choisi = Some(p.clone());
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(p) = choisi {
            let confirm = rfd::MessageDialog::new().set_title("Désinstaller").set_description(format!("Désinstaller « {} » ?\n\nSon désinstalleur va s'ouvrir ; ensuite Outils cherchera les restes.", p.nom))
                .set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes;
            if confirm {
                let e = self.e.clone();
                *e.lock().unwrap() = (true, format!("Désinstallation de {}… (suis les étapes de son désinstalleur)", p.nom));
                let ctx = ui.ctx().clone();
                let attente: Arc<Mutex<Option<(Programme, Vec<(PathBuf, u64, bool)>)>>> = Arc::default();
                let a2 = attente.clone();
                std::thread::spawn(move || {
                    let _ = std::process::Command::new("cmd").raw_arg(format!("/c \"{}\"", p.desinstaller)).creation_flags(SANS_FENETRE).status();
                    // certains désinstalleurs rendent la main tout de suite : on attend que la clé disparaisse (max 10 min)
                    let t = Instant::now();
                    while t.elapsed() < Duration::from_secs(600) && reg_texte(&p.cle, "DisplayName").is_some() {
                        std::thread::sleep(Duration::from_secs(2));
                    }
                    let encore = reg_texte(&p.cle, "DisplayName").is_some();
                    let r: Vec<(PathBuf, u64, bool)> = restes(&p).into_iter().map(|d| {
                        let t = taille_dossier(&d);
                        (d, t, !encore)
                    }).collect();
                    *e.lock().unwrap() = (false, if encore { "⚠ Le programme semble encore installé (désinstallation annulée ?).".into() } else { "✔ Désinstallé.".into() });
                    *a2.lock().unwrap() = Some((p, r));
                    ctx.request_repaint();
                });
                // récupération du résultat à la prochaine image
                self.attente = Some(attente);
            }
        }
        if let Some(a) = &self.attente {
            if let Some(n) = a.lock().unwrap().take() {
                self.nettoyage = Some(n);
            }
        }
        if self.nettoyage.is_some() {
            self.attente = None;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 35 · Alerte de surchauffe
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageChaleur {
    pub actif: bool,
    pub cpu: f32,
    pub gpu: f32,
}

impl Default for ReglageChaleur {
    fn default() -> Self {
        ReglageChaleur { actif: false, cpu: 90.0, gpu: 85.0 }
    }
}

pub struct Surchauffe {
    r: Arc<Mutex<ReglageChaleur>>,
    journal: Arc<Mutex<Vec<String>>>,
    lecteur: crate::moniteur::Lecteur,
    max: Arc<Mutex<(f32, f32)>>,
}

impl Surchauffe {
    pub fn new(lecteur: crate::moniteur::Lecteur) -> Self {
        let r: Arc<Mutex<ReglageChaleur>> = Arc::new(Mutex::new(charger("surchauffe")));
        let journal: Arc<Mutex<Vec<String>>> = Arc::default();
        let max: Arc<Mutex<(f32, f32)>> = Arc::default();
        let (r2, j2, l2, m2) = (r.clone(), journal.clone(), lecteur.clone(), max.clone());
        std::thread::spawn(move || {
            let mut derniere: Option<Instant> = None;
            let mut au_dessus = 0u32;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let c = r2.lock().unwrap().clone();
                if !c.actif {
                    continue;
                }
                let (m, jeu) = l2(true);
                {
                    let mut x = m2.lock().unwrap();
                    x.0 = x.0.max(m.cpu_t);
                    x.1 = x.1.max(m.gpu_t);
                }
                let trop = (m.cpu_t >= c.cpu && m.cpu_t > 0.0) || (m.gpu_t >= c.gpu && m.gpu_t > 0.0);
                au_dessus = if trop { au_dessus + 1 } else { 0 };
                // 3 mesures de suite (15 s) pour éviter les pics d'une seconde ; une alerte toutes les 10 min au plus
                if au_dessus >= 3 && derniere.is_none_or(|t| t.elapsed() > Duration::from_secs(600)) {
                    derniere = Some(Instant::now());
                    let t = format!("CPU {:.0} °C · GPU {:.0} °C{}", m.cpu_t, m.gpu_t, jeu.map(|j| format!(" (pendant {j})")).unwrap_or_default());
                    crate::systeme::notifier_toujours("🔥 Le PC chauffe trop", &t);
                    let mut jj = j2.lock().unwrap();
                    jj.push(format!("{} — {t}", crate::minecraft::date_lisible()));
                    let n = jj.len();
                    if n > 50 {
                        jj.drain(..n - 50);
                    }
                }
            }
        });
        Surchauffe { r, journal, lecteur, max }
    }
}

impl Outil for Surchauffe {
    fn nom(&self) -> &'static str {
        "🌡 Alerte de surchauffe"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Alerte de surchauffe");
        ui.weak("Surveille la température du processeur et de la carte graphique en arrière-plan, et prévient si elle reste trop haute (poussière, ventilateur, pâte thermique…).");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.checkbox(&mut r.actif, "Surveiller en permanence");
        ui.horizontal(|ui| {
            ui.label("Prévenir au-dessus de : processeur");
            ui.add(egui::DragValue::new(&mut r.cpu).range(60.0..=105.0).suffix(" °C"));
            ui.label("carte graphique");
            ui.add(egui::DragValue::new(&mut r.gpu).range(60.0..=100.0).suffix(" °C"));
        });
        let (m, _) = self.lecteur.as_ref()(true);
        let c = |t: f32, s: f32| if t >= s { Color32::LIGHT_RED } else if t >= s - 10.0 { Color32::from_rgb(250, 210, 90) } else { Color32::LIGHT_GREEN };
        ui.horizontal(|ui| {
            if m.cpu_t > 0.0 {
                ui.label(RichText::new(format!("Processeur {:.0} °C", m.cpu_t)).size(20.0).color(c(m.cpu_t, r.cpu)));
            } else {
                ui.weak("Température du processeur illisible (lance Outils en admin).");
            }
            if m.gpu_t > 0.0 {
                ui.label(RichText::new(format!("Carte graphique {:.0} °C", m.gpu_t)).size(20.0).color(c(m.gpu_t, r.gpu)));
            }
        });
        let (mc, mg) = *self.max.lock().unwrap();
        if mc > 0.0 || mg > 0.0 {
            ui.weak(format!("Maximum depuis le lancement : processeur {mc:.0} °C · carte graphique {mg:.0} °C"));
        }
        let j = self.journal.lock().unwrap().clone();
        if !j.is_empty() {
            ui.separator();
            ui.strong("Alertes");
            for l in j.iter().rev() {
                ui.label(l);
            }
        }
        if r != avant {
            enregistrer("surchauffe", &r);
            *self.r.lock().unwrap() = r;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 36 · Éteindre quand…
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Condition {
    Telechargement,
    Inactif,
    Programme,
    Heure,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Eteindre,
    Veille,
    Hibernation,
    Redemarrer,
}

fn octets_recus() -> u64 {
    use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
    unsafe {
        let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut t) != 0 || t.is_null() {
            return 0;
        }
        let n = (*t).NumEntries as usize;
        let lignes = std::slice::from_raw_parts((*t).Table.as_ptr(), n);
        // interfaces physiques connectées seulement (pas les boucles locales ni doublons filtrés)
        let s = lignes.iter().filter(|l| l.Type != 24 && l.OperStatus == 1 && l.InterfaceAndOperStatusFlags._bitfield & 1 != 0).map(|l| l.InOctets).sum();
        FreeMibTable(t as *const _);
        s
    }
}

fn cpu_ticks() -> (u64, u64) {
    use windows_sys::Win32::Foundation::FILETIME;
    let mut f = [FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 }; 3];
    unsafe { windows_sys::Win32::System::Threading::GetSystemTimes(&mut f[0], &mut f[1], &mut f[2]) };
    let v = |x: &FILETIME| ((x.dwHighDateTime as u64) << 32) | x.dwLowDateTime as u64;
    (v(&f[0]), v(&f[1]) + v(&f[2]))
}

fn processus_existe(nom: &str) -> bool {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let n = nom.to_lowercase();
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(s, &mut e);
        let mut trouve = false;
        while ok != 0 {
            let l = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(260);
            if String::from_utf16_lossy(&e.szExeFile[..l]).to_lowercase() == n {
                trouve = true;
                break;
            }
            ok = Process32NextW(s, &mut e);
        }
        windows_sys::Win32::Foundation::CloseHandle(s);
        trouve
    }
}

fn agir(a: Action) {
    let args: &[&str] = match a {
        Action::Eteindre => &["/s", "/t", "0"],
        Action::Redemarrer => &["/r", "/t", "0"],
        Action::Hibernation => &["/h"],
        Action::Veille => {
            unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
            return;
        }
    };
    let _ = std::process::Command::new("shutdown").args(args).creation_flags(SANS_FENETRE).spawn();
}

#[derive(Default)]
struct Surveillance {
    actif: bool,
    texte: String,
    compte: Option<Instant>,
}

pub struct EteindreQuand {
    cond: Condition,
    action: Action,
    seuil_ko: u32,
    minutes: u32,
    programme: String,
    heure: (u32, u32),
    s: Arc<Mutex<Surveillance>>,
    annuler: Arc<AtomicBool>,
}

impl EteindreQuand {
    pub fn new() -> Self {
        EteindreQuand { cond: Condition::Telechargement, action: Action::Eteindre, seuil_ko: 100, minutes: 3, programme: String::new(), heure: (23, 0), s: Arc::default(), annuler: Arc::default() }
    }

    fn lancer(&mut self) {
        self.annuler.store(false, Relaxed);
        *self.s.lock().unwrap() = Surveillance { actif: true, ..Default::default() };
        let (cond, action, seuil, minutes, prog, heure, s, an) = (self.cond, self.action, self.seuil_ko as u64 * 1024, self.minutes as u64, self.programme.trim().to_string(), self.heure, self.s.clone(), self.annuler.clone());
        std::thread::spawn(move || {
            let mut calme_depuis: Option<Instant> = None;
            let (mut o0, mut c0) = (octets_recus(), cpu_ticks());
            let mut vu_programme = false;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if an.load(Relaxed) {
                    *s.lock().unwrap() = Surveillance { texte: "Annulé.".into(), ..Default::default() };
                    return;
                }
                let (o, c) = (octets_recus(), cpu_ticks());
                let debit = o.saturating_sub(o0) / 5;
                let cpu = 100.0 - (c.0.saturating_sub(c0.0) as f64 * 100.0 / (c.1.saturating_sub(c0.1)).max(1) as f64);
                (o0, c0) = (o, c);
                let (h, m) = {
                    let t = crate::minecraft::horodatage();
                    let n = |a: usize| t.get(a..a + 2).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
                    (n(11), n(14))
                };
                let (pret, texte) = match cond {
                    Condition::Telechargement => {
                        let calme = debit < seuil;
                        (calme, format!("Réception : {}/s", taille(debit)))
                    }
                    Condition::Inactif => (cpu < 10.0, format!("Processeur : {cpu:.0} %")),
                    Condition::Programme => {
                        let la = processus_existe(&prog);
                        vu_programme |= la;
                        (vu_programme && !la, if la { format!("{prog} tourne encore") } else if vu_programme { format!("{prog} est fermé") } else { format!("{prog} n'est pas lancé") })
                    }
                    Condition::Heure => ((h, m) == heure, format!("Il est {h}h{m:02}")),
                };
                if pret {
                    calme_depuis.get_or_insert_with(Instant::now);
                } else {
                    calme_depuis = None;
                }
                let attente = if matches!(cond, Condition::Telechargement | Condition::Inactif) { minutes * 60 } else { 0 };
                let ecoule = calme_depuis.map(|t| t.elapsed().as_secs()).unwrap_or(0);
                s.lock().unwrap().texte = if calme_depuis.is_some() && attente > 0 { format!("{texte} — condition remplie depuis {} s / {attente} s", ecoule) } else { texte };
                if calme_depuis.is_some() && ecoule >= attente {
                    crate::systeme::notifier_toujours("Le PC va s'arrêter", "Dans 60 secondes. Ouvre Outils pour annuler.");
                    s.lock().unwrap().compte = Some(Instant::now());
                    for _ in 0..60 {
                        std::thread::sleep(Duration::from_secs(1));
                        if an.load(Relaxed) {
                            *s.lock().unwrap() = Surveillance { texte: "Annulé.".into(), ..Default::default() };
                            return;
                        }
                    }
                    *s.lock().unwrap() = Surveillance::default();
                    agir(action);
                    return;
                }
            }
        });
    }
}

impl Outil for EteindreQuand {
    fn nom(&self) -> &'static str {
        "⏻ Éteindre quand…"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn resume(&self) -> Option<String> {
        self.s.lock().unwrap().actif.then(|| "⏻ Arrêt automatique programmé".into())
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éteindre le PC quand…");
        let actif = self.s.lock().unwrap().actif;
        ui.add_enabled_ui(!actif, |ui| {
            ui.radio_value(&mut self.cond, Condition::Telechargement, "⬇ le téléchargement est fini (internet calme)");
            if self.cond == Condition::Telechargement {
                ui.horizontal(|ui| {
                    ui.label("    moins de");
                    ui.add(egui::DragValue::new(&mut self.seuil_ko).range(10..=10_000).suffix(" Ko/s"));
                    ui.label("pendant");
                    ui.add(egui::DragValue::new(&mut self.minutes).range(1..=60).suffix(" min"));
                });
            }
            ui.radio_value(&mut self.cond, Condition::Inactif, "💤 le PC ne fait plus rien (processeur < 10 %)");
            if self.cond == Condition::Inactif {
                ui.horizontal(|ui| {
                    ui.label("    pendant");
                    ui.add(egui::DragValue::new(&mut self.minutes).range(1..=120).suffix(" min"));
                });
            }
            ui.radio_value(&mut self.cond, Condition::Programme, "🪟 un programme se ferme (rendu vidéo, installation…)");
            if self.cond == Condition::Programme {
                ui.horizontal(|ui| {
                    ui.label("    nom du programme");
                    ui.add(egui::TextEdit::singleline(&mut self.programme).hint_text("ex. HandBrake.exe").desired_width(180.0));
                });
            }
            ui.radio_value(&mut self.cond, Condition::Heure, "⏰ à une heure précise");
            if self.cond == Condition::Heure {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.heure.0).range(0..=23).suffix(" h"));
                    ui.add(egui::DragValue::new(&mut self.heure.1).range(0..=59).suffix(" min"));
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Puis :");
                ui.selectable_value(&mut self.action, Action::Eteindre, "Éteindre");
                ui.selectable_value(&mut self.action, Action::Veille, "Veille");
                ui.selectable_value(&mut self.action, Action::Hibernation, "Veille prolongée");
                ui.selectable_value(&mut self.action, Action::Redemarrer, "Redémarrer");
            });
        });
        let s = self.s.lock().unwrap();
        let (texte, compte) = (s.texte.clone(), s.compte);
        drop(s);
        if actif {
            if let Some(t) = compte {
                ui.label(RichText::new(format!("⏻ Arrêt dans {} s", 60u64.saturating_sub(t.elapsed().as_secs()))).size(24.0).color(Color32::LIGHT_RED));
            }
            ui.label(texte);
            if ui.button(RichText::new("✖ Annuler").size(16.0)).clicked() {
                self.annuler.store(true, Relaxed);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        } else {
            if !texte.is_empty() {
                ui.label(texte);
            }
            let ok = self.cond != Condition::Programme || !self.programme.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new(RichText::new("▶ Surveiller").size(16.0))).clicked() {
                self.lancer();
            }
            ui.weak("Un compte à rebours de 60 s (avec notification) laisse toujours le temps d'annuler.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 37 · Luminosité et lumière bleue
// ---------------------------------------------------------------------------------------------------

/// Rampe gamma chaude : facteurs (rouge, vert, bleu) selon la chaleur 0 (neutre) → 1 (très chaud).
pub fn facteurs_chaleur(k: f32) -> (f32, f32, f32) {
    let k = k.clamp(0.0, 1.0);
    (1.0, 1.0 - 0.28 * k, 1.0 - 0.62 * k)
}

fn appliquer_chaleur(k: f32) -> bool {
    use windows_sys::Win32::Graphics::Gdi::*;
    let (r, g, b) = facteurs_chaleur(k);
    let mut rampe = [0u16; 768];
    for i in 0..256 {
        let v = i as f32 * 257.0;
        rampe[i] = (v * r).min(65535.0) as u16;
        rampe[256 + i] = (v * g).min(65535.0) as u16;
        rampe[512 + i] = (v * b).min(65535.0) as u16;
    }
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let ok = windows_sys::Win32::UI::ColorSystem::SetDeviceGammaRamp(dc, rampe.as_ptr() as *const _) != 0;
        ReleaseDC(std::ptr::null_mut(), dc);
        ok
    }
}

/// Écrans externes (DDC/CI) : (poignée, min, actuel, max).
fn ecrans_ddc() -> Vec<(isize, u32, u32, u32)> {
    use windows_sys::Win32::Devices::Display::*;
    use windows_sys::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    unsafe extern "system" fn rappel(m: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
        (*(l as *mut Vec<HMONITOR>)).push(m);
        1
    }
    let mut moniteurs: Vec<HMONITOR> = vec![];
    let mut v = vec![];
    unsafe {
        EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(rappel), &mut moniteurs as *mut _ as LPARAM);
        for m in moniteurs {
            let mut n = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(m, &mut n) == 0 || n == 0 {
                continue;
            }
            let mut p: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(m, n, p.as_mut_ptr()) == 0 {
                continue;
            }
            for x in p {
                let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
                if GetMonitorBrightness(x.hPhysicalMonitor, &mut a, &mut b, &mut c) != 0 {
                    v.push((x.hPhysicalMonitor as isize, a, b, c));
                }
            }
        }
    }
    v
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageLumiere {
    pub chaleur: f32,
    pub auto: bool,
    pub debut: u32,
    pub fin: u32,
}

impl Default for ReglageLumiere {
    fn default() -> Self {
        ReglageLumiere { chaleur: 0.5, auto: false, debut: 21, fin: 7 }
    }
}

/// Le filtre doit-il être actif à cette heure ? (plage qui peut passer minuit)
pub fn dans_plage(h: u32, debut: u32, fin: u32) -> bool {
    if debut <= fin { h >= debut && h < fin } else { h >= debut || h < fin }
}

/// Filtre forcé (Some) ou selon l'horaire (None) ; partagé avec les profils.
pub static MANUEL: Mutex<Option<bool>> = Mutex::new(None);

/// Utilisé par les profils : active ou coupe le filtre tout de suite.
pub fn forcer_lumiere(oui: bool) {
    *MANUEL.lock().unwrap() = Some(oui);
    let r: ReglageLumiere = charger("lumiere_bleue");
    appliquer_chaleur(if oui { r.chaleur } else { 0.0 });
}

pub struct Lumiere {
    r: Arc<Mutex<ReglageLumiere>>,
    ecrans: Vec<(isize, u32, u32, u32)>,
    portable: Option<u32>,
    charge: bool,
}

impl Lumiere {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageLumiere>> = Arc::new(Mutex::new(charger("lumiere_bleue")));
        let r2 = r.clone();
        std::thread::spawn(move || {
            let mut applique: Option<f32> = None;
            loop {
                let c = r2.lock().unwrap().clone();
                let h = crate::minecraft::horodatage_iso().get(11..13).and_then(|x| x.parse().ok()).unwrap_or(12);
                let voulu = match *MANUEL.lock().unwrap() {
                    Some(true) => c.chaleur,
                    Some(false) => 0.0,
                    None if c.auto && dans_plage(h, c.debut, c.fin) => c.chaleur,
                    None => 0.0,
                };
                // réappliqué régulièrement : Windows remet parfois la rampe à zéro (veille, changement d'écran)
                if applique.is_none_or(|a| (a - voulu).abs() > 0.001) || voulu > 0.0 {
                    if voulu > 0.0 || applique.is_some_and(|a| a > 0.0) {
                        appliquer_chaleur(voulu);
                    }
                    applique = Some(voulu);
                }
                std::thread::sleep(Duration::from_secs(20));
            }
        });
        Lumiere { r, ecrans: vec![], portable: None, charge: false }
    }
}

impl Outil for Lumiere {
    fn nom(&self) -> &'static str {
        "🔆 Luminosité et lumière bleue"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Luminosité et lumière bleue");
        if !self.charge {
            self.charge = true;
            self.ecrans = ecrans_ddc();
            self.portable = ps("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1).CurrentBrightness").ok().and_then(|s| s.trim().parse().ok());
        }
        ui.strong("☀ Luminosité");
        if let Some(mut b) = self.portable {
            ui.horizontal(|ui| {
                ui.label("Écran du portable");
                if ui.add(egui::Slider::new(&mut b, 0..=100).suffix(" %")).drag_stopped() {
                    let _ = ps(&format!("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1) | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=1;Brightness={b}}}"));
                }
                self.portable = Some(b);
            });
        }
        for (k, (h, min, cur, max)) in self.ecrans.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("Écran {}", k + 1));
                if ui.add(egui::Slider::new(cur, *min..=*max)).drag_stopped() {
                    unsafe { windows_sys::Win32::Devices::Display::SetMonitorBrightness(*h as _, *cur) };
                }
            });
        }
        if self.portable.is_none() && self.ecrans.is_empty() {
            ui.weak("Aucun écran réglable trouvé (l'écran doit accepter le DDC/CI, option souvent dans son menu).");
        }
        if ui.small_button("⟳ Écrans").clicked() {
            self.charge = false;
        }
        ui.separator();
        ui.strong("🌙 Filtre de lumière bleue");
        ui.weak("Rend l'écran plus chaud le soir : moins fatigant pour les yeux et pour s'endormir.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.label("Chaleur");
            ui.add(egui::Slider::new(&mut r.chaleur, 0.1..=1.0).show_value(false));
        });
        let m = *MANUEL.lock().unwrap();
        ui.horizontal(|ui| {
            if ui.selectable_label(m == Some(true), "Activer maintenant").clicked() {
                *MANUEL.lock().unwrap() = Some(true);
                appliquer_chaleur(r.chaleur);
            }
            if ui.selectable_label(m == Some(false), "Désactiver").clicked() {
                *MANUEL.lock().unwrap() = Some(false);
                appliquer_chaleur(0.0);
            }
            if ui.selectable_label(m.is_none(), "Selon l'horaire").clicked() {
                *MANUEL.lock().unwrap() = None;
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.auto, "Automatique de");
            ui.add(egui::DragValue::new(&mut r.debut).range(0..=23).suffix(" h"));
            ui.label("à");
            ui.add(egui::DragValue::new(&mut r.fin).range(0..=23).suffix(" h"));
        });
        if r != avant {
            if r.chaleur != avant.chaleur && m == Some(true) {
                appliquer_chaleur(r.chaleur);
            }
            enregistrer("lumiere_bleue", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 38 · Polices installées
// ---------------------------------------------------------------------------------------------------

fn polices() -> Vec<(String, PathBuf, bool)> {
    let mut v = vec![];
    let sys = PathBuf::from(exp("%WINDIR%\\Fonts"));
    let perso = PathBuf::from(exp("%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts"));
    for (d, perso_) in [(sys, false), (perso, true)] {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| ["ttf", "otf", "ttc"].contains(&x.to_string_lossy().to_lowercase().as_str())) {
                v.push((p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p, perso_));
            }
        }
    }
    v.sort_by_key(|x| x.0.to_lowercase());
    v
}

fn installer_police(p: &Path) -> Result<String, String> {
    let d = PathBuf::from(exp("%LOCALAPPDATA%\\Microsoft\\Windows\\Fonts"));
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let n = p.file_name().ok_or("nom ?")?;
    let dest = d.join(n);
    std::fs::copy(p, &dest).map_err(|e| e.to_string())?;
    let nom = format!("{} (TrueType)", p.file_stem().unwrap_or_default().to_string_lossy());
    let ok = std::process::Command::new("reg").args(["add", r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts", "/v", &nom, "/t", "REG_SZ", "/d", &dest.display().to_string(), "/f"]).creation_flags(SANS_FENETRE).status().is_ok_and(|s| s.success());
    unsafe {
        let w: Vec<u16> = dest.as_os_str().encode_wide_z();
        windows_sys::Win32::Graphics::Gdi::AddFontResourceW(w.as_ptr());
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(0xffff as _, windows_sys::Win32::UI::WindowsAndMessaging::WM_FONTCHANGE, 0, 0, 0, 1000, std::ptr::null_mut());
    }
    if ok { Ok(format!("✔ {} installée", n.to_string_lossy())) } else { Err("inscription dans le registre impossible".into()) }
}

trait Large {
    fn encode_wide_z(&self) -> Vec<u16>;
}

impl Large for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain([0]).collect()
    }
}

pub struct Polices {
    liste: Vec<(String, PathBuf, bool)>,
    filtre: String,
    texte: String,
    apercus: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
    message: String,
}

impl Polices {
    pub fn new() -> Self {
        Polices { liste: vec![], filtre: String::new(), texte: "Le vif renard brun saute 123".into(), apercus: Default::default(), message: String::new() }
    }

    fn apercu(&mut self, ctx: &egui::Context, p: &Path) -> Option<egui::TextureHandle> {
        if let Some(t) = self.apercus.get(p) {
            return t.clone();
        }
        let t = (|| {
            let b = std::fs::read(p).ok()?;
            let f = ab_glyph::FontVec::try_from_vec(b).ok()?;
            let taille = 30.0;
            let l = (crate::creatif::largeur(&f, taille, &self.texte) + 10.0).min(1600.0) as u32;
            let mut img = image::RgbaImage::from_pixel(l.max(10), 42, image::Rgba([0, 0, 0, 0]));
            crate::creatif::ecrire(&mut img, &f, taille, 4.0, 2.0, &self.texte, [235, 225, 235], None);
            Some(ctx.load_texture(p.display().to_string(), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), Default::default()))
        })();
        self.apercus.insert(p.to_path_buf(), t.clone());
        t
    }
}

impl Outil for Polices {
    fn nom(&self) -> &'static str {
        "🔠 Polices installées"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Polices installées");
        if self.liste.is_empty() {
            self.liste = polices();
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 chercher").desired_width(180.0));
            if ui.add(egui::TextEdit::singleline(&mut self.texte).desired_width(260.0)).changed() {
                self.apercus.clear();
            }
            if ui.button("➕ Installer une police…").clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Polices", &["ttf", "otf"]).pick_files() {
                    self.message = v.iter().map(|p| installer_police(p).unwrap_or_else(|e| format!("✖ {e}"))).collect::<Vec<_>>().join(" · ");
                    self.liste = polices();
                }
            }
        });
        ui.weak(format!("{} polices · installation pour ton compte, sans droits administrateur. Glisse des .ttf ici pour les installer.", self.liste.len()));
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !deposes.is_empty() {
            self.message = deposes.iter().filter(|p| p.extension().is_some_and(|x| x == "ttf" || x == "otf")).map(|p| installer_police(p).unwrap_or_else(|e| format!("✖ {e}"))).collect::<Vec<_>>().join(" · ");
            self.liste = polices();
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let f = self.filtre.to_lowercase();
        let liste: Vec<(String, PathBuf, bool)> = self.liste.iter().filter(|x| f.is_empty() || x.0.to_lowercase().contains(&f)).cloned().collect();
        let mut retirer = None;
        egui::ScrollArea::vertical().show_rows(ui, 52.0, liste.len(), |ui, lignes| {
            for k in lignes {
                let (n, p, perso) = &liste[k];
                ui.horizontal(|ui| {
                    ui.add_sized([180.0, 46.0], egui::Label::new(RichText::new(n).strong()).truncate());
                    if let Some(t) = self.apercu(ui.ctx(), p) {
                        ui.add(egui::Image::new(&t).fit_to_original_size(1.0));
                    } else {
                        ui.weak("(aperçu impossible)");
                    }
                    if *perso && ui.small_button("🗑").on_hover_text("Désinstaller (police perso)").clicked() {
                        retirer = Some(p.clone());
                    }
                });
            }
        });
        if let Some(p) = retirer {
            unsafe {
                let w = p.as_os_str().encode_wide_z();
                windows_sys::Win32::Graphics::Gdi::RemoveFontResourceW(w.as_ptr());
            }
            let nom = format!("{} (TrueType)", p.file_stem().unwrap_or_default().to_string_lossy());
            let _ = std::process::Command::new("reg").args(["delete", r"HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts", "/v", &nom, "/f"]).creation_flags(SANS_FENETRE).status();
            self.message = if std::fs::remove_file(&p).is_ok() { "✔ Police retirée.".into() } else { "Police encore utilisée : elle partira au prochain redémarrage.".into() };
            self.liste = polices();
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 39 · Mémoire en veille
// ---------------------------------------------------------------------------------------------------

#[repr(C)]
#[derive(Default)]
struct ListesMemoire {
    zero: usize,
    libre: usize,
    modifiee: usize,
    modifiee_sans_ecriture: usize,
    mauvaise: usize,
    veille: [usize; 8],
    reutilisee: [usize; 8],
    modifiee_fichier: usize,
}

type NtSet = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32) -> i32;
type NtQuery = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;

fn ntdll(nom: &str) -> Option<usize> {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    unsafe {
        let m = GetModuleHandleW("ntdll.dll\0".encode_utf16().collect::<Vec<_>>().as_ptr());
        let n = format!("{nom}\0");
        GetProcAddress(m, n.as_ptr()).map(|f| f as usize)
    }
}

/// (veille, libre) en octets.
fn memoire_listes() -> Option<(u64, u64)> {
    let f: NtQuery = unsafe { std::mem::transmute(ntdll("NtQuerySystemInformation")?) };
    let mut l = ListesMemoire::default();
    let r = unsafe { f(80, &mut l as *mut _ as *mut _, std::mem::size_of::<ListesMemoire>() as u32, std::ptr::null_mut()) };
    (r == 0).then(|| (l.veille.iter().sum::<usize>() as u64 * 4096, (l.libre + l.zero) as u64 * 4096))
}

fn privilege(nom: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut t: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut t) == 0 {
            return false;
        }
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        let w: Vec<u16> = nom.encode_utf16().chain([0]).collect();
        let ok = LookupPrivilegeValueW(std::ptr::null(), w.as_ptr(), &mut luid) != 0 && {
            let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
            AdjustTokenPrivileges(t, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) != 0 && windows_sys::Win32::Foundation::GetLastError() == 0
        };
        CloseHandle(t);
        ok
    }
}

/// Vide la liste de veille (cache des fichiers). Admin requis.
pub(crate) fn vider_veille() -> Result<(), String> {
    if !privilege("SeProfileSingleProcessPrivilege") {
        return Err("privilège refusé (lance Outils en admin)".into());
    }
    let f: NtSet = unsafe { std::mem::transmute(ntdll("NtSetSystemInformation").ok_or("ntdll ?")?) };
    let mut cmd: u32 = 4; // MemoryPurgeStandbyList
    let r = unsafe { f(80, &mut cmd as *mut u32 as *mut _, 4) };
    if r == 0 { Ok(()) } else { Err(format!("refusé par Windows (0x{r:08X})")) }
}

fn ram_totale_libre() -> (u64, u64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    unsafe { GlobalMemoryStatusEx(&mut m) };
    (m.ullTotalPhys, m.ullAvailPhys)
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageMemoire {
    pub auto: bool,
    pub seuil_libre_mo: u64,
}

impl Default for ReglageMemoire {
    fn default() -> Self {
        ReglageMemoire { auto: false, seuil_libre_mo: 1024 }
    }
}

pub struct MemoireVeille {
    r: Arc<Mutex<ReglageMemoire>>,
    vidages: Arc<Mutex<u32>>,
    message: String,
}

impl MemoireVeille {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageMemoire>> = Arc::new(Mutex::new(charger("memoire_veille")));
        let vidages: Arc<Mutex<u32>> = Arc::default();
        let (r2, v2) = (r.clone(), vidages.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(10));
            let c = r2.lock().unwrap().clone();
            if !c.auto || !est_admin() {
                continue;
            }
            // vraiment libre (hors cache) sous le seuil, alors qu'il y a de la veille à récupérer
            if let Some((veille, libre)) = memoire_listes() {
                if libre < c.seuil_libre_mo << 20 && veille > 512 << 20 && vider_veille().is_ok() {
                    *v2.lock().unwrap() += 1;
                }
            }
        });
        MemoireVeille { r, vidages, message: String::new() }
    }
}

impl Outil for MemoireVeille {
    fn nom(&self) -> &'static str {
        "🧠 Mémoire en veille"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mémoire en veille (RAM)");
        ui.weak("Windows garde en mémoire des fichiers déjà lus (« en veille »). C'est normalement utile, mais en jeu ça peut provoquer des saccades quand la RAM vraiment libre devient trop basse : la vider aide alors.");
        let admin = admin_requis(ui, "vider la mémoire en veille");
        let (tot, dispo) = ram_totale_libre();
        ui.label(format!("RAM : {} au total, {} disponible", taille(tot), taille(dispo)));
        if let Some((veille, libre)) = memoire_listes() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("En veille : {}", taille(veille))).size(18.0));
                ui.label(RichText::new(format!("Vraiment libre : {}", taille(libre))).size(18.0));
            });
            let (r, _) = ui.allocate_exact_size(egui::vec2(420.0, 18.0), egui::Sense::hover());
            let utilise = tot.saturating_sub(dispo);
            let f = |x: u64| x as f32 / tot.max(1) as f32 * r.width();
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(f(utilise), 18.0)), 2.0, Color32::from_rgb(255, 120, 170));
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(f(utilise), 0.0), egui::vec2(f(veille), 18.0)), 2.0, Color32::from_rgb(120, 170, 255));
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(f(utilise) + f(veille), 0.0), egui::vec2(f(libre), 18.0)), 2.0, Color32::from_rgb(110, 220, 130));
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(255, 120, 170), "■ utilisée");
                ui.colored_label(Color32::from_rgb(120, 170, 255), "■ en veille");
                ui.colored_label(Color32::from_rgb(110, 220, 130), "■ libre");
            });
        }
        if ui.add_enabled(admin, egui::Button::new(RichText::new("🧹 Vider maintenant").size(16.0))).clicked() {
            let avant = memoire_listes().map(|x| x.1).unwrap_or(0);
            self.message = match vider_veille() {
                Ok(()) => format!("✔ {} libérés", taille(memoire_listes().map(|x| x.1).unwrap_or(0).saturating_sub(avant))),
                Err(e) => format!("✖ {e}"),
            };
        }
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.add_enabled(admin, egui::Checkbox::new(&mut r.auto, "Vider automatiquement quand la RAM libre passe sous"));
            ui.add(egui::DragValue::new(&mut r.seuil_libre_mo).range(256..=8192).suffix(" Mo"));
        });
        let n = *self.vidages.lock().unwrap();
        if n > 0 {
            ui.weak(format!("{n} vidage(s) automatique(s) depuis le lancement."));
        }
        ui.label(&self.message);
        if r != avant {
            enregistrer("memoire_veille", &r);
            *self.r.lock().unwrap() = r;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 40 · Fiche du PC
// ---------------------------------------------------------------------------------------------------

const PS_FICHE: &str = r#"$cs = Get-CimInstance Win32_ComputerSystem; $os = Get-CimInstance Win32_OperatingSystem; $b = Get-CimInstance Win32_BIOS; $mb = Get-CimInstance Win32_BaseBoard
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$o = [ordered]@{
 'Fabricant / modèle' = "$($cs.Manufacturer) $($cs.Model)"
 'Numéro de série' = "$($b.SerialNumber)"
 'Carte mère' = "$($mb.Manufacturer) $($mb.Product)"
 'BIOS' = "$($b.SMBIOSBIOSVersion) ($($b.ReleaseDate.ToString('yyyy-MM-dd')))"
 'Processeur' = "$($cpu.Name.Trim()) — $($cpu.NumberOfCores) cœurs / $($cpu.NumberOfLogicalProcessors) threads"
 'Mémoire' = "$([math]::Round($cs.TotalPhysicalMemory/1GB)) Go : " + ((Get-CimInstance Win32_PhysicalMemory | ForEach-Object { "$([math]::Round($_.Capacity/1GB)) Go $($_.Speed) MHz $($_.Manufacturer)".Trim() }) -join ' + ')
 'Carte graphique' = ((Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name) (pilote $($_.DriverVersion))" }) -join ' + ')
 'Écrans' = ((Get-CimInstance Win32_VideoController | Where-Object CurrentHorizontalResolution | ForEach-Object { "$($_.CurrentHorizontalResolution)×$($_.CurrentVerticalResolution) à $($_.CurrentRefreshRate) Hz" }) -join ' + ')
 'Disques' = ((Get-PhysicalDisk | ForEach-Object { "$($_.FriendlyName) $([math]::Round($_.Size/1GB)) Go $($_.MediaType) ($($_.HealthStatus))" }) -join ' + ')
 'Windows' = "$($os.Caption) $($os.DisplayVersion) build $($os.BuildNumber) ($($os.OSArchitecture))"
 'Installé le' = $os.InstallDate.ToString('yyyy-MM-dd')
 'Activation' = $(if ((Get-CimInstance SoftwareLicensingProduct -Filter "PartialProductKey IS NOT NULL AND Name LIKE 'Windows%'" | Select-Object -First 1).LicenseStatus -eq 1) { 'activé' } else { 'non activé' })
 'Réseau' = ((Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | ForEach-Object { "$($_.InterfaceDescription) $($_.LinkSpeed)" }) -join ' + ')
 'Batterie' = $(if ($bt = Get-CimInstance Win32_Battery) { "présente ($($bt.EstimatedChargeRemaining) %)" } else { 'aucune (PC fixe)' })
}
ConvertTo-Json -Compress -InputObject $o"#;

pub fn fiche_texte(l: &[(String, String)]) -> String {
    let largeur = l.iter().map(|x| x.0.chars().count()).max().unwrap_or(0);
    let mut s = String::from("Fiche du PC (Outils)\n====================\n\n");
    for (k, v) in l {
        s.push_str(&format!("{k:<largeur$} : {v}\n"));
    }
    s
}

pub struct FichePc {
    l: Arc<Mutex<Option<Result<Vec<(String, String)>, String>>>>,
    serie: bool,
    message: String,
}

impl FichePc {
    pub fn new() -> Self {
        FichePc { l: Arc::default(), serie: false, message: String::new() }
    }
}

impl Outil for FichePc {
    fn nom(&self) -> &'static str {
        "🧾 Fiche du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fiche du PC");
        ui.weak("Tout le matériel sur une page, à copier ou enregistrer : pour une revente, le SAV, ou demander de l'aide sur un forum.");
        let l = self.l.lock().unwrap().clone();
        let Some(r) = l else {
            *self.l.lock().unwrap() = Some(Err("lecture du matériel…".into()));
            let c = self.l.clone();
            std::thread::spawn(move || {
                let r = ps(PS_FICHE).and_then(|s| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s.trim()).map_err(|e| e.to_string()))
                    .map(|m| m.into_iter().map(|(k, v)| (k, v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))).filter(|x| !x.1.trim().is_empty()).collect());
                *c.lock().unwrap() = Some(r);
            });
            return;
        };
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(500));
                return;
            }
        };
        ui.checkbox(&mut self.serie, "Inclure le numéro de série (à éviter sur un forum public)");
        let affiche: Vec<(String, String)> = v.into_iter().filter(|x| self.serie || x.0 != "Numéro de série").collect();
        egui::Grid::new("fiche").striped(true).num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
            for (k, val) in &affiche {
                ui.strong(k);
                ui.label(val);
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("📋 Copier").clicked() {
                ui.ctx().copy_text(fiche_texte(&affiche));
                self.message = "Copié.".into();
            }
            if ui.button("💾 Enregistrer en .txt…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Texte", &["txt"]).set_file_name("Fiche PC.txt").save_file() {
                    self.message = std::fs::write(&p, fiche_texte(&affiche)).map(|_| {
                        ouvrir(&p);
                        format!("Enregistré : {}", p.display())
                    }).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.small_button("⟳").clicked() {
                *self.l.lock().unwrap() = None;
            }
        });
        ui.label(&self.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(mots_cles("Discord Inc.", "Discord Inc."), vec!["discord".to_string()]);
        let m = mots_cles("OBS Studio", "OBS Project");
        assert!(m.contains(&"studio".to_string()) && m.contains(&"project".to_string()));
        assert!(ressemble("Discord", "Discord", &mots_cles("Discord", "")));
        assert!(ressemble("obs-studio", "OBS Studio", &mots_cles("OBS Studio", "")));
        assert!(!ressemble("Microsoft", "Microsoft Edge", &mots_cles("Microsoft Edge", "Microsoft Corporation")));
        assert!(!ressemble("Common Files", "Steam", &mots_cles("Steam", "Valve")));
        assert!(dans_plage(22, 21, 7) && dans_plage(3, 21, 7) && !dans_plage(12, 21, 7));
        assert!(dans_plage(10, 9, 17) && !dans_plage(17, 9, 17));
        assert_eq!(facteurs_chaleur(0.0), (1.0, 1.0, 1.0));
        let (r, g, b) = facteurs_chaleur(1.0);
        assert!(r == 1.0 && g > b && b > 0.3);
        assert!(fiche_texte(&[("CPU".into(), "x".into()), ("Mémoire".into(), "y".into())]).contains("CPU     : x"));
    }
}
