//! Outils s'occupe de lui-même : ce qu'il consomme, mode sourdine pendant les jeux, et « Santé d'Outils »
//! (réglages abîmés, raccourcis pris par une autre appli, ports occupés, place sur le disque…).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Moment où Outils a démarré, et durée jusqu'à la première image (ms).
pub static DEMARRE: OnceLock<Instant> = OnceLock::new();
pub static PREMIERE_IMAGE: AtomicUsize = AtomicUsize::new(0);
/// Outils chargés à la demande déjà ouverts / en tout.
pub static CHARGES: AtomicUsize = AtomicUsize::new(0);
pub static A_LA_DEMANDE: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Reglages {
    /// Pendant un jeu (Outils en arrière-plan) : priorité basse, moins d'images, tâches lourdes en pause.
    pub sourdine: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { sourdine: true }
    }
}

static SOURDINE: AtomicBool = AtomicBool::new(true);
static SOURDINE_LUE: std::sync::Once = std::sync::Once::new();

pub fn sourdine_active() -> bool {
    SOURDINE_LUE.call_once(|| SOURDINE.store(charger::<Reglages>("outils_moi").sourdine, Relaxed));
    SOURDINE.load(Relaxed)
}

/// Passe Outils en sourdine (ou l'en sort) : priorité du processus et drapeau lu par les tâches de fond.
pub fn appliquer_discret(discret: bool) {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS};
    if crate::outil::DISCRET.swap(discret, Relaxed) != discret {
        unsafe { SetPriorityClass(GetCurrentProcess(), if discret { BELOW_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS }) };
    }
}

// ---------- mesures du processus ----------

#[derive(Clone, Copy, Default)]
struct Mesure {
    cpu_100ns: u64,
    ram: usize,
    privee: usize,
    poignees: u32,
}

fn mesurer() -> Mesure {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount, GetProcessTimes};
    unsafe {
        let h = GetCurrentProcess();
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u);
        GetProcessMemoryInfo(h, &mut m, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32);
        let mut p = 0;
        GetProcessHandleCount(h, &mut p);
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        Mesure { cpu_100ns: ft(k) + ft(u), ram: m.WorkingSetSize, privee: m.PagefileUsage, poignees: p }
    }
}

fn fils() -> u32 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    let moi = std::process::id();
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if s.is_null() || s as isize == -1 {
            return 0;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut n = 0;
        let mut ok = Process32FirstW(s, &mut e) != 0;
        while ok {
            if e.th32ProcessID == moi {
                n = e.cntThreads;
                break;
            }
            ok = Process32NextW(s, &mut e) != 0;
        }
        CloseHandle(s);
        n
    }
}

fn mo(o: usize) -> String {
    format!("{:.0} Mo", o as f64 / 1048576.0)
}

pub struct Ressources {
    r: Reglages,
    derniere: Option<(Instant, Mesure)>,
    cpu: Vec<f32>,
    ram: Vec<f32>,
    actuelle: Mesure,
    nb_fils: u32,
    message: String,
    vu: Instant,
}

impl Ressources {
    pub fn new() -> Self {
        Ressources { r: charger("outils_moi"), derniere: None, cpu: vec![], ram: vec![], actuelle: Mesure::default(), nb_fils: 0, message: String::new(), vu: Instant::now() }
    }

    fn echantillon(&mut self) {
        let m = mesurer();
        let coeurs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
        if let Some((t, p)) = self.derniere {
            let dt = t.elapsed().as_secs_f32().max(0.01);
            let pc = (m.cpu_100ns.saturating_sub(p.cpu_100ns)) as f32 / 1e7 / dt / coeurs * 100.0;
            self.cpu.push(pc);
            self.ram.push(m.ram as f32 / 1048576.0);
            for v in [&mut self.cpu, &mut self.ram] {
                if v.len() > 90 {
                    v.remove(0);
                }
            }
        }
        self.derniere = Some((Instant::now(), m));
        self.actuelle = m;
        self.nb_fils = fils();
    }
}

fn courbe(ui: &mut egui::Ui, v: &[f32], max: f32, couleur: Color32) {
    let (r, p) = ui.allocate_painter(egui::vec2(ui.available_width().min(420.0), 60.0), egui::Sense::hover());
    let r = r.rect;
    p.rect_filled(r, 4.0, ui.visuals().extreme_bg_color);
    if v.len() < 2 {
        return;
    }
    let max = v.iter().cloned().fold(max, f32::max);
    let pts: Vec<egui::Pos2> = v.iter().enumerate().map(|(i, x)| egui::pos2(r.left() + r.width() * i as f32 / 89.0, r.bottom() - r.height() * (x / max).clamp(0.0, 1.0))).collect();
    p.add(egui::Shape::line(pts, egui::Stroke::new(1.5_f32, couleur)));
}

impl Outil for Ressources {
    fn nom(&self) -> &'static str {
        "⚡ Ressources d'Outils"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        // mesures seulement quand l'onglet est regardé (Outils ne se mesure pas pour rien)
        if self.vu.elapsed() < Duration::from_secs(5) && self.derniere.is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(2)) {
            self.echantillon();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        ui.heading("Ressources d'Outils");
        ui.weak("Ce qu'Outils lui-même consomme sur ton PC, et le mode sourdine pendant les jeux.");
        ui.add_space(6.0);
        let m = self.actuelle;
        let cpu = self.cpu.last().copied().unwrap_or(0.0);
        egui::Grid::new("ress").num_columns(2).spacing([20.0, 6.0]).show(ui, |ui| {
            ui.label("Processeur");
            ui.label(RichText::new(format!("{cpu:.1} %")).strong().color(if cpu > 10.0 { Color32::from_rgb(255, 170, 80) } else { Color32::LIGHT_GREEN }));
            ui.end_row();
            ui.label("Mémoire utilisée");
            ui.label(RichText::new(mo(m.ram)).strong());
            ui.end_row();
            ui.label("Mémoire réservée");
            ui.label(mo(m.privee));
            ui.end_row();
            ui.label("Fils d'exécution");
            ui.label(self.nb_fils.to_string());
            ui.end_row();
            ui.label("Poignées ouvertes");
            ui.label(m.poignees.to_string());
            ui.end_row();
            let ms = PREMIERE_IMAGE.load(Relaxed);
            ui.label("Démarrage");
            ui.label(if ms > 0 { format!("{:.2} s jusqu'à la fenêtre", ms as f32 / 1000.0) } else { "—".into() });
            ui.end_row();
            ui.label("Ouvert depuis");
            let s = DEMARRE.get().map(|d| d.elapsed().as_secs()).unwrap_or(0);
            ui.label(format!("{} h {:02} min", s / 3600, s / 60 % 60));
            ui.end_row();
            ui.label("Outils chargés");
            let (c, t) = (CHARGES.load(Relaxed), A_LA_DEMANDE.load(Relaxed));
            ui.label(format!("{c} sur {t} chargés à la demande (les autres ne coûtent rien tant que tu ne les ouvres pas)"));
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.label("Processeur (3 dernières minutes)");
        courbe(ui, &self.cpu, 5.0, Color32::from_rgb(120, 180, 255));
        ui.label("Mémoire");
        courbe(ui, &self.ram, 100.0, Color32::from_rgb(170, 120, 255));
        ui.horizontal(|ui| {
            if ui.button("🧹 Libérer la mémoire d'Outils").on_hover_text("Rend à Windows la mémoire qu'Outils n'utilise pas en ce moment").clicked() {
                let avant = mesurer().ram;
                unsafe { windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(windows_sys::Win32::System::Threading::GetCurrentProcess()) };
                let apres = mesurer().ram;
                self.message = format!("✔ {} rendus à Windows", mo(avant.saturating_sub(apres)));
            }
            ui.label(&self.message);
        });
        if m.ram > 800 << 20 {
            ui.colored_label(Color32::from_rgb(255, 170, 80), "Outils utilise beaucoup de mémoire : fermer et relancer Outils la libère complètement.");
        }
        ui.separator();
        ui.strong("🎮 Mode sourdine pendant les jeux");
        if ui.checkbox(&mut self.r.sourdine, "Quand un jeu tourne et qu'Outils est en arrière-plan, Outils se fait discret").changed() {
            SOURDINE.store(self.r.sourdine, Relaxed);
            enregistrer("outils_moi", &self.r);
        }
        ui.weak("Priorité basse (le jeu passe toujours avant), fenêtre redessinée moins souvent, et l'indexation des fichiers attend la fin de la partie. Les raccourcis, l'overlay et le moniteur continuent de marcher.");
        let d = crate::outil::DISCRET.load(Relaxed);
        ui.label(if d { "État : 🔇 en sourdine (un jeu tourne)" } else { "État : normal" });
    }
}

// ---------- Santé d'Outils ----------

#[derive(Clone)]
enum Reparer {
    Rien,
    Reglage(PathBuf),
    Terminer(u32, String),
    OuvrirOutil(&'static str),
}

#[derive(Clone)]
struct Souci {
    grave: bool,
    titre: String,
    detail: String,
    reparer: Reparer,
}

pub struct SanteOutils {
    soucis: Arc<Mutex<Option<Vec<Souci>>>>,
    en_cours: Arc<AtomicBool>,
    message: String,
}

impl SanteOutils {
    pub fn new() -> Self {
        SanteOutils { soucis: Arc::default(), en_cours: Arc::default(), message: String::new() }
    }

    fn analyser(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (s, e) = (self.soucis.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let v = verifier();
            *s.lock().unwrap() = Some(v);
            e.store(false, Relaxed);
        });
    }
}

fn dossier() -> PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.to_path_buf()).unwrap_or_default()
}

fn nom_touche(mods: u32, vk: u32) -> String {
    let mut s = String::new();
    if mods & 2 != 0 {
        s += "Ctrl+";
    }
    if mods & 1 != 0 {
        s += "Alt+";
    }
    if mods & 4 != 0 {
        s += "Maj+";
    }
    if mods & 8 != 0 {
        s += "Win+";
    }
    s += &match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x60..=0x69 => format!("Pavé {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x6A => "Pavé *".into(),
        0x6B => "Pavé +".into(),
        0x6E => "Pavé .".into(),
        _ => format!("touche {vk}"),
    };
    s
}

/// Lignes de `netstat -ano` : (protocole, port local, pid).
pub fn ports_ecoutes(sortie: &str) -> Vec<(String, u16, u32)> {
    let mut v = vec![];
    for l in sortie.lines() {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() < 4 {
            continue;
        }
        let proto = c[0].to_ascii_uppercase();
        let ecoute = match proto.as_str() {
            "TCP" => c[3..].iter().any(|x| {
                let u = x.to_uppercase();
                u.contains("LISTEN") || u.contains("COUTE")
            }),
            "UDP" => true,
            _ => false,
        };
        if !ecoute {
            continue;
        }
        let Some(port) = c[1].rsplit(':').next().and_then(|p| p.parse().ok()) else { continue };
        let Some(pid) = c.last().and_then(|p| p.parse().ok()) else { continue };
        v.push((proto, port, pid));
    }
    v
}

fn nom_processus(pid: u32) -> String {
    let o = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).creation_flags(0x0800_0000).output();
    o.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).split('"').nth(1).map(|s| s.to_string())).unwrap_or_else(|| format!("programme {pid}"))
}

fn verifier() -> Vec<Souci> {
    let mut v = vec![];
    // 1. réglages illisibles
    let d = dossier();
    if let Ok(l) = std::fs::read_dir(&d) {
        for e in l.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                let ok = std::fs::read(&p).map(|o| o.is_empty() || serde_json::from_slice::<serde_json::Value>(&o).is_ok()).unwrap_or(true);
                if !ok {
                    let n = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    v.push(Souci { grave: true, titre: format!("Réglages abîmés : {n}"), detail: "Ce fichier de réglages est illisible (coupure de courant pendant l'enregistrement ?) : l'outil repart des réglages par défaut.".into(), reparer: Reparer::Reglage(p) });
                }
            }
        }
    }
    // 2. raccourcis pris par une autre appli
    for (m, k) in crate::outil::RACCOURCIS_PRIS.lock().map(|g| g.clone()).unwrap_or_default() {
        v.push(Souci { grave: false, titre: format!("Raccourci {} déjà pris", nom_touche(m, k)), detail: "Une autre appli utilise déjà ce raccourci : celui d'Outils ne marche pas. Ferme l'autre appli, ou change le raccourci dans l'outil concerné (ou dans Système › Raccourcis clavier).".into(), reparer: Reparer::OuvrirOutil("⌨ Raccourcis clavier") });
    }
    // 3. ports nécessaires à Outils
    let net = std::process::Command::new("netstat").args(["-ano"]).creation_flags(0x0800_0000).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let moi = std::process::id();
    let besoins: &[(&str, u16, &str)] = &[
        ("TCP", 8642, "le lien avec le téléphone"),
        ("UDP", 8643, "la détection du PC par le téléphone"),
        ("UDP", 8644, "la manette et la souris du téléphone"),
        ("UDP", 8645, "le micro du téléphone"),
        ("TCP", 25565, "le serveur Minecraft"),
    ];
    let pris = ports_ecoutes(&net);
    for (proto, port, quoi) in besoins {
        if let Some((_, _, pid)) = pris.iter().find(|(p, n, pid)| p == proto && n == port && *pid != moi && *pid != 0) {
            let nom = nom_processus(*pid);
            if *port == 25565 && nom.to_lowercase().contains("java") {
                continue; // c'est sûrement le serveur Minecraft lui-même
            }
            v.push(Souci { grave: true, titre: format!("Port {port} ({proto}) occupé par {nom}"), detail: format!("Outils en a besoin pour {quoi}. Ferme {nom} puis relance Outils."), reparer: Reparer::Terminer(*pid, nom) });
        }
    }
    // 4. place sur le disque des réglages
    let lettre = d.to_string_lossy().chars().next().unwrap_or('C').to_ascii_uppercase();
    for (nom, libre, _) in crate::systeme::disques() {
        if nom.to_ascii_uppercase().starts_with(lettre) && libre < 2 << 30 {
            v.push(Souci { grave: libre < 300 << 20, titre: format!("Disque {lettre}: presque plein"), detail: format!("Il reste {:.1} Go : Outils risque de ne plus pouvoir enregistrer ses réglages.", libre as f64 / (1u64 << 30) as f64), reparer: Reparer::OuvrirOutil("💽 Espace disque") });
        }
    }
    // 5. dossier des réglages trop gros
    let mut total = 0u64;
    let mut gros: Option<(u64, String)> = None;
    if let Ok(l) = std::fs::read_dir(&d) {
        for e in l.flatten() {
            let t = e.metadata().map(|m| m.len()).unwrap_or(0);
            total += t;
            if gros.as_ref().is_none_or(|g| t > g.0) {
                gros = Some((t, e.file_name().to_string_lossy().to_string()));
            }
        }
    }
    if total > 500 << 20 {
        v.push(Souci { grave: false, titre: format!("Les données d'Outils pèsent {:.1} Go", total as f64 / (1u64 << 30) as f64), detail: format!("Le plus gros : {}. Tu peux faire du tri dans l'outil correspondant.", gros.map(|g| g.1).unwrap_or_default()), reparer: Reparer::Rien });
    }
    // 6. plantages récents
    let n = crate::coeur::nouveaux_plantages();
    if n > 0 {
        v.push(Souci { grave: false, titre: format!("{n} plantage(s) non lu(s)"), detail: "Un rapport est prêt : il aide à corriger le problème.".into(), reparer: Reparer::OuvrirOutil("📜 Journal et plantages") });
    }
    v
}

impl Outil for SanteOutils {
    fn nom(&self) -> &'static str {
        "🩹 Santé d'Outils"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        // première vérification discrète 2 minutes après le démarrage (pour le résumé de l'Accueil)
        let pret = DEMARRE.get().is_some_and(|d| d.elapsed() > Duration::from_secs(120));
        if pret && !crate::outil::DISCRET.load(Relaxed) && !self.en_cours.load(Relaxed) && self.soucis.lock().unwrap().is_none() {
            self.analyser();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Santé d'Outils");
        ui.weak("Vérifie qu'Outils marche bien : réglages abîmés, raccourcis pris par une autre appli, ports occupés, place sur le disque, plantages.");
        if self.soucis.lock().unwrap().is_none() {
            self.analyser();
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.en_cours.load(Relaxed), egui::Button::new("🔄 Vérifier à nouveau")).clicked() {
                self.analyser();
            }
            if self.en_cours.load(Relaxed) {
                ui.spinner();
            }
            ui.label(&self.message);
        });
        ui.separator();
        let l = self.soucis.lock().unwrap().clone();
        let Some(l) = l else { return };
        if l.is_empty() {
            ui.label(RichText::new("✔ Tout va bien : aucun souci trouvé.").color(Color32::LIGHT_GREEN).size(16.0));
            return;
        }
        let mut refaire = false;
        for s in &l {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("{} {}", if s.grave { "❗" } else { "⚠" }, s.titre)).strong().color(if s.grave { Color32::LIGHT_RED } else { Color32::from_rgb(255, 200, 100) }));
                ui.label(&s.detail);
                match &s.reparer {
                    Reparer::Reglage(p) => {
                        if ui.button("🛠 Réparer (repartir des réglages par défaut)").clicked() {
                            let abimes = dossier().join("abimes");
                            let _ = std::fs::create_dir_all(&abimes);
                            let dest = abimes.join(format!("{}-{}", crate::minecraft::horodatage(), p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()));
                            self.message = if std::fs::rename(p, &dest).is_ok() { "✔ Réglage remis à zéro (l'ancien est gardé dans le dossier « abimes »)".into() } else { "Impossible de déplacer le fichier.".into() };
                            refaire = true;
                        }
                    }
                    Reparer::Terminer(pid, nom) => {
                        if ui.button(format!("⛔ Fermer {nom}")).clicked() {
                            let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).creation_flags(0x0800_0000).status();
                            self.message = format!("{nom} fermé : relance Outils pour reprendre le port.");
                            refaire = true;
                        }
                    }
                    Reparer::OuvrirOutil(n) => {
                        if ui.button("Ouvrir l'outil").clicked() {
                            *crate::interface2::OUVRIR.lock().unwrap() = Some(n.to_string());
                        }
                    }
                    Reparer::Rien => {}
                }
            });
        }
        if refaire {
            self.analyser();
        }
    }
    fn resume(&self) -> Option<String> {
        let g = self.soucis.lock().ok()?;
        let n = g.as_ref()?.iter().filter(|s| s.grave).count();
        (n > 0).then(|| format!("{n} souci(s) à réparer"))
    }
}

// ---------- accueil personnalisable ----------

pub const SECTIONS: &[(&str, &str, &str)] = &[
    ("favoris", "⭐ Favoris", "⭐ Favorites"),
    ("recents", "🕘 Récents", "🕘 Recent"),
    ("disques", "💽 Disques", "💽 Disks"),
    ("resumes", "📋 Résumés des outils", "📋 Tool summaries"),
    ("tous", "🧰 Tous les outils", "🧰 All tools"),
];

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Accueil {
    pub ordre: Vec<String>,
    pub cachees: Vec<String>,
    pub resumes_caches: Vec<String>,
}

impl Default for Accueil {
    fn default() -> Self {
        Accueil { ordre: SECTIONS.iter().map(|s| s.0.to_string()).collect(), cachees: vec!["recents".into()], resumes_caches: vec![] }
    }
}

impl Accueil {
    pub fn charger() -> Self {
        let mut a: Accueil = charger("accueil");
        for s in SECTIONS {
            if !a.ordre.iter().any(|o| o == s.0) {
                a.ordre.push(s.0.to_string());
            }
        }
        a.ordre.retain(|o| SECTIONS.iter().any(|s| s.0 == o));
        a
    }

    pub fn enregistrer(&self) {
        enregistrer("accueil", self);
    }

    /// Réglage des sections : afficher, monter, descendre. Vrai si quelque chose a changé.
    pub fn editeur(&mut self, ui: &mut egui::Ui) -> bool {
        let mut change = false;
        let mut bouger: Option<(usize, bool)> = None;
        for (k, id) in self.ordre.clone().iter().enumerate() {
            let Some(s) = SECTIONS.iter().find(|s| s.0 == id) else { continue };
            ui.horizontal(|ui| {
                let mut voir = !self.cachees.contains(id);
                if ui.checkbox(&mut voir, if crate::menu::en() { s.2 } else { s.1 }).changed() {
                    if voir {
                        self.cachees.retain(|c| c != id);
                    } else {
                        self.cachees.push(id.clone());
                    }
                    change = true;
                }
                if ui.add_enabled(k > 0, egui::Button::new("⬆").small()).clicked() {
                    bouger = Some((k, true));
                }
                if ui.add_enabled(k + 1 < self.ordre.len(), egui::Button::new("⬇").small()).clicked() {
                    bouger = Some((k, false));
                }
            });
        }
        if let Some((k, haut)) = bouger {
            let j = if haut { k - 1 } else { k + 1 };
            self.ordre.swap(k, j);
            change = true;
        }
        if !self.resumes_caches.is_empty() && ui.small_button(format!("Réafficher les {} résumé(s) masqué(s)", self.resumes_caches.len())).clicked() {
            self.resumes_caches.clear();
            change = true;
        }
        if change {
            self.enregistrer();
        }
        change
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netstat() {
        let s = "  Proto  Adresse locale         Adresse distante       État\n  TCP    0.0.0.0:8642           0.0.0.0:0              LISTENING       4321\n  TCP    192.168.1.2:50000      1.2.3.4:443            ESTABLISHED     99\n  UDP    0.0.0.0:8643           *:*                                    4321\n  TCP    [::]:25565             [::]:0                 LISTENING       77";
        let v = ports_ecoutes(s);
        assert_eq!(v, vec![("TCP".into(), 8642, 4321), ("UDP".into(), 8643, 4321), ("TCP".into(), 25565, 77)]);
        assert_eq!(nom_touche(3, b'K' as u32), "Ctrl+Alt+K");
        assert_eq!(nom_touche(6, 0x7B), "Ctrl+Maj+F12");
    }
}
