//! Outils pratiques communs à Windows et Linux : sorties de jeux à venir, réglages graphiques conseillés,
//! abonnements, résumé de la journée, traduction de documents en gardant la mise en page.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn aujourdhui() -> String {
    crate::minecraft::horodatage_iso().get(..10).unwrap_or("").to_string()
}

/// Jours entre deux dates « AAAA-MM-JJ » (b − a).
pub fn jours_entre(a: &str, b: &str) -> Option<i64> {
    let (x, y) = (crate::quotidien2::lire_date(a)?, crate::quotidien2::lire_date(b)?);
    Some(crate::quotidien2::jours_depuis_1970(y.0, y.1, y.2) - crate::quotidien2::jours_depuis_1970(x.0, x.1, x.2))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Outils").build()
}

/// Sites qu'on peut ouvrir à distance depuis la télécommande TV du téléphone.
pub fn url_autorisee(u: &str) -> bool {
    const S: &[&str] = &["https://www.youtube.com", "https://www.netflix.com", "https://www.primevideo.com", "https://www.disneyplus.com", "https://www.twitch.tv", "https://www.crunchyroll.com", "https://www.molotov.tv", "https://www.france.tv", "https://open.spotify.com", "https://www.deezer.com"];
    S.iter().any(|s| u == *s || u.starts_with(&format!("{s}/")))
}

// ===================================================================================================
// 8 · Sorties à venir
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Sortie {
    pub nom: String,
    pub appid: u32,
    /// « AAAA-MM-JJ » si connue, sinon le texte de Steam (« 2026 », « Bientôt »…)
    pub date: String,
    pub texte_date: String,
    pub prevenu: bool,
}

const MOIS: [(&str, u32); 24] = [
    ("jan", 1), ("feb", 2), ("mar", 3), ("apr", 4), ("may", 5), ("jun", 6), ("jul", 7), ("aug", 8), ("sep", 9), ("oct", 10), ("nov", 11), ("dec", 12),
    ("janv", 1), ("févr", 2), ("mars", 3), ("avr", 4), ("mai", 5), ("juin", 6), ("juil", 7), ("août", 8), ("sept", 9), ("oct", 10), ("nov", 11), ("déc", 12),
];

/// Date de Steam (« 12 Mar, 2026 », « Mar 12, 2026 », « 12 mars 2026 ») → « 2026-03-12 ».
pub fn lire_date_steam(t: &str) -> Option<String> {
    let mots: Vec<String> = t.replace(',', " ").replace('.', " ").split_whitespace().map(|m| m.to_lowercase()).collect();
    let annee = mots.iter().find_map(|m| m.parse::<i32>().ok().filter(|a| *a > 1990))?;
    let jour = mots.iter().find_map(|m| m.parse::<u32>().ok().filter(|j| (1..=31).contains(j)))?;
    let mois = mots.iter().find_map(|m| MOIS.iter().filter(|(p, _)| m.starts_with(p)).max_by_key(|(p, _)| p.len()).map(|x| x.1))?;
    Some(format!("{annee:04}-{mois:02}-{jour:02}"))
}

fn infos_steam(appid: u32) -> Option<(String, String, bool)> {
    let j: serde_json::Value = agent().get("https://store.steampowered.com/api/appdetails").query("appids", &appid.to_string()).query("l", "french").query("cc", "FR").call().ok()?.into_json().ok()?;
    let d = &j[appid.to_string()]["data"];
    Some((d["name"].as_str()?.to_string(), d["release_date"]["date"].as_str().unwrap_or("").to_string(), d["release_date"]["coming_soon"].as_bool().unwrap_or(false)))
}

pub struct Sorties {
    l: Arc<Mutex<Vec<Sortie>>>,
    ajout: String,
    message: Arc<Mutex<String>>,
    verifie: bool,
}

impl Sorties {
    pub fn new() -> Self {
        Sorties { l: Arc::new(Mutex::new(charger("sorties"))), ajout: String::new(), message: Arc::default(), verifie: false }
    }
}

fn appid_de(t: &str) -> Option<u32> {
    let t = t.trim();
    t.parse().ok().or_else(|| t.split("/app/").nth(1)?.split('/').next()?.parse().ok())
}

impl Outil for Sorties {
    fn nom(&self) -> &'static str {
        "📅 Sorties à venir"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.verifie {
            return;
        }
        self.verifie = true;
        // une fois par lancement : dates mises à jour et alerte le jour de la sortie
        let l = self.l.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            let ids: Vec<u32> = l.lock().unwrap().iter().map(|s| s.appid).collect();
            for id in ids {
                if let Some((_, texte, _)) = infos_steam(id) {
                    let mut g = l.lock().unwrap();
                    if let Some(s) = g.iter_mut().find(|s| s.appid == id) {
                        s.texte_date = texte.clone();
                        if let Some(d) = lire_date_steam(&texte) {
                            s.date = d;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
            let auj = aujourdhui();
            let mut g = l.lock().unwrap();
            for s in g.iter_mut().filter(|s| !s.prevenu && !s.date.is_empty() && jours_entre(&s.date, &auj).is_some_and(|j| j >= 0)) {
                crate::systeme::notifier_toujours("🎮 Sortie du jour", &format!("{} est sorti ! Bon jeu 🎉", s.nom));
                s.prevenu = true;
            }
            enregistrer("sorties", &*g);
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sorties à venir");
        ui.weak("Les jeux que tu attends, avec un compte à rebours. Outils te prévient le jour de la sortie (et sur le téléphone).");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.ajout).hint_text("lien Steam du jeu (ou son numéro)").desired_width(300.0));
            if ui.button("➕ Suivre").clicked() {
                match appid_de(&self.ajout) {
                    Some(id) => {
                        let (l, m) = (self.l.clone(), self.message.clone());
                        *m.lock().unwrap() = "Recherche…".into();
                        std::thread::spawn(move || {
                            *m.lock().unwrap() = match infos_steam(id) {
                                Some((nom, texte, _)) => {
                                    let date = lire_date_steam(&texte).unwrap_or_default();
                                    let mut g = l.lock().unwrap();
                                    if !g.iter().any(|s| s.appid == id) {
                                        g.push(Sortie { nom: nom.clone(), appid: id, date, texte_date: texte, prevenu: false });
                                    }
                                    enregistrer("sorties", &*g);
                                    format!("✔ {nom} ajouté")
                                }
                                None => "Jeu introuvable sur Steam.".into(),
                            };
                        });
                        self.ajout.clear();
                    }
                    None => *self.message.lock().unwrap() = "Colle le lien de la page Steam du jeu.".into(),
                }
            }
            ui.label(self.message.lock().unwrap().clone());
        });
        let auj = aujourdhui();
        let mut l = self.l.lock().unwrap().clone();
        l.sort_by(|a, b| match (a.date.is_empty(), b.date.is_empty()) {
            (false, false) => a.date.cmp(&b.date),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => a.nom.cmp(&b.nom),
        });
        let mut retirer = None;
        for s in &l {
            ui.horizontal(|ui| {
                let j = (!s.date.is_empty()).then(|| jours_entre(&auj, &s.date)).flatten();
                let (t, c) = match j {
                    Some(j) if j > 1 => (format!("dans {j} jours"), Color32::from_rgb(120, 180, 255)),
                    Some(1) => ("demain !".into(), Color32::from_rgb(255, 200, 100)),
                    Some(0) => ("aujourd'hui ! 🎉".into(), Color32::LIGHT_GREEN),
                    Some(_) => ("sorti".into(), Color32::GRAY),
                    None => (if s.texte_date.is_empty() { "date inconnue".into() } else { s.texte_date.clone() }, Color32::GRAY),
                };
                ui.add_sized([130.0, 20.0], egui::Label::new(RichText::new(t).color(c).strong()));
                if ui.link(&s.nom).clicked() {
                    crate::outil::ouvrir(Path::new(&format!("https://store.steampowered.com/app/{}", s.appid)));
                }
                ui.weak(&s.texte_date);
                if ui.small_button("✖").clicked() {
                    retirer = Some(s.appid);
                }
            });
        }
        if let Some(id) = retirer {
            let mut g = self.l.lock().unwrap();
            g.retain(|s| s.appid != id);
            enregistrer("sorties", &*g);
        }
        if l.is_empty() {
            ui.weak("Aucun jeu suivi. Exemple : colle https://store.steampowered.com/app/1234560/… d'un jeu pas encore sorti.");
        }
    }
    fn resume(&self) -> Option<String> {
        let auj = aujourdhui();
        let l = self.l.lock().ok()?;
        let s = l.iter().filter_map(|s| Some((jours_entre(&auj, &s.date)?, s))).filter(|(j, _)| *j >= 0).min_by_key(|(j, _)| *j)?;
        Some(if s.0 == 0 { format!("{} sort aujourd'hui !", s.1.nom) } else { format!("{} dans {} jours", s.1.nom, s.0) })
    }
}

// ===================================================================================================
// 9 · Réglages graphiques conseillés
// ===================================================================================================

/// Niveau approximatif d'une carte graphique (100 ≈ RTX 4090), d'après son nom.
pub fn niveau_gpu(nom: &str) -> Option<u32> {
    let n = nom.to_lowercase().replace("geforce ", "").replace("radeon ", "").replace("nvidia ", "").replace("amd ", "");
    const T: &[(&str, u32)] = &[
        ("rtx 5090", 125), ("rtx 5080", 100), ("rtx 5070 ti", 85), ("rtx 5070", 72), ("rtx 5060 ti", 56), ("rtx 5060", 48),
        ("rtx 4090", 100), ("rtx 4080", 84), ("rtx 4070 ti", 70), ("rtx 4070", 60), ("rtx 4060 ti", 48), ("rtx 4060", 42),
        ("rtx 3090", 70), ("rtx 3080", 63), ("rtx 3070 ti", 54), ("rtx 3070", 50), ("rtx 3060 ti", 45), ("rtx 3060", 36), ("rtx 3050", 24),
        ("rtx 2080 ti", 52), ("rtx 2080", 44), ("rtx 2070", 38), ("rtx 2060", 30), ("gtx 1660", 22), ("gtx 1650", 16),
        ("gtx 1080 ti", 38), ("gtx 1080", 30), ("gtx 1070", 25), ("gtx 1060", 18), ("gtx 1050", 10), ("gtx 970", 14), ("gtx 960", 9),
        ("rx 9070 xt", 82), ("rx 9070", 74), ("rx 9060 xt", 50), ("rx 7900 xtx", 86), ("rx 7900 xt", 76), ("rx 7800 xt", 60), ("rx 7700 xt", 52), ("rx 7600", 40),
        ("rx 6950", 68), ("rx 6900", 65), ("rx 6800 xt", 62), ("rx 6800", 55), ("rx 6750", 49), ("rx 6700", 46), ("rx 6650", 38), ("rx 6600", 35), ("rx 6500", 16),
        ("rx 5700", 35), ("rx 5600", 28), ("rx 590", 16), ("rx 580", 15), ("rx 570", 13),
        ("arc b580", 42), ("arc a770", 38), ("arc a750", 34), ("arc a380", 14),
        ("rtx 4050", 32), ("rtx 3050 laptop", 18),
        ("uhd", 4), ("iris", 7), ("vega", 7), ("radeon(tm)", 8), ("780m", 14), ("680m", 11), ("steam deck", 9), ("van gogh", 9),
    ];
    T.iter().find(|(k, _)| n.contains(k)).map(|x| x.1)
}

/// (carte graphique, Go de mémoire, fils du processeur).
pub fn materiel() -> (String, f32, u32) {
    let fils = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let o = std::process::Command::new("powershell").args(["-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController | Sort-Object AdapterRAM -Descending | Select-Object -First 1).Name; [math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB)"]).creation_flags(0x0800_0000).output();
        let t = o.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let mut l = t.lines().map(|x| x.trim()).filter(|x| !x.is_empty());
        let gpu = l.next().unwrap_or("").to_string();
        let ram = l.next().and_then(|x| x.replace(',', ".").parse().ok()).unwrap_or(8.0);
        return (gpu, ram, fils);
    }
    #[allow(unreachable_code)]
    {
        let lspci = std::process::Command::new("lspci").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let gpus: Vec<String> = lspci.lines().filter(|l| l.contains("VGA") || l.contains("3D controller")).map(|l| l.split_once(": ").map(|x| x.1).unwrap_or(l).to_string()).collect();
        // la carte dédiée passe avant la puce intégrée
        let gpu = gpus.iter().max_by_key(|g| niveau_gpu(g).unwrap_or(0)).cloned().unwrap_or_default();
        let mi = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let ram = mi.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|x| x.split_whitespace().next()?.parse::<f32>().ok()).unwrap_or(8e6) / 1048576.0;
        (gpu, ram, fils)
    }
}

/// (jeu, exigence de 1 = très léger à 10 = très lourd, a le DLSS/FSR)
pub const JEUX: &[(&str, u32, bool)] = &[
    ("Minecraft (Java)", 2, false), ("Minecraft avec shaders", 7, false), ("Fortnite", 4, true), ("Valorant", 2, false), ("Counter-Strike 2", 3, false),
    ("League of Legends", 1, false), ("Rocket League", 2, false), ("Roblox", 1, false), ("Apex Legends", 4, false), ("Overwatch 2", 3, false),
    ("Call of Duty : Warzone", 6, true), ("GTA V", 4, true), ("Red Dead Redemption 2", 7, true), ("Cyberpunk 2077", 9, true), ("Elden Ring", 6, false),
    ("Baldur's Gate 3", 6, true), ("Hogwarts Legacy", 7, true), ("Forza Horizon 5", 5, true), ("The Finals", 6, true), ("Marvel Rivals", 6, true),
    ("Helldivers 2", 6, true), ("Palworld", 5, true), ("Rainbow Six Siege", 3, false), ("EA Sports FC", 4, false), ("Sea of Thieves", 4, false),
    ("Black Myth : Wukong", 9, true), ("Alan Wake 2", 10, true), ("Stardew Valley", 1, false), ("Rust", 6, false), ("Escape from Tarkov", 7, true),
];

/// Conseil : (préréglage, définition, réglage d'upscaling, FPS attendus).
pub fn conseil(niveau: u32, ram: f32, exigence: u32, upscaling: bool) -> (&'static str, &'static str, &'static str, &'static str) {
    let mut marge = niveau as i32 - exigence as i32 * 8;
    if ram < 12.0 {
        marge -= 15;
    }
    let preset = match marge {
        m if m >= 40 => "Ultra",
        m if m >= 20 => "Élevé",
        m if m >= 0 => "Moyen",
        m if m >= -15 => "Bas",
        _ => "Très bas (le jeu risque de ramer)",
    };
    let def = match niveau {
        n if n >= 70 && marge >= 20 => "1440p ou 4K",
        n if n >= 40 && marge >= 10 => "1440p",
        _ => "1080p",
    };
    let up = if !upscaling { "—" } else if marge < 10 { "DLSS / FSR en mode Performance" } else if marge < 30 { "DLSS / FSR en mode Qualité" } else { "pas besoin (ou DLAA pour une image plus nette)" };
    let fps = match marge {
        m if m >= 30 => "plus de 100 FPS",
        m if m >= 10 => "60 à 100 FPS",
        m if m >= -10 => "autour de 60 FPS",
        _ => "moins de 45 FPS",
    };
    (preset, def, up, fps)
}

pub struct ReglagesGraphiques {
    mat: Arc<Mutex<Option<(String, f32, u32)>>>,
    gpu_manuel: String,
    filtre: String,
}

impl ReglagesGraphiques {
    pub fn new() -> Self {
        ReglagesGraphiques { mat: Arc::default(), gpu_manuel: String::new(), filtre: String::new() }
    }
}

impl Outil for ReglagesGraphiques {
    fn nom(&self) -> &'static str {
        "🎛 Réglages graphiques conseillés"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Réglages graphiques conseillés");
        ui.weak("Les réglages à choisir dans chaque jeu pour ton PC : un bon équilibre entre beauté et fluidité.");
        if self.mat.lock().unwrap().is_none() {
            *self.mat.lock().unwrap() = Some((String::new(), 0.0, 0));
            let m = self.mat.clone();
            std::thread::spawn(move || *m.lock().unwrap() = Some(materiel()));
        }
        let (gpu, ram, fils) = self.mat.lock().unwrap().clone().unwrap_or_default();
        if ram == 0.0 {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        }
        let nom_gpu = if self.gpu_manuel.trim().is_empty() { gpu.clone() } else { self.gpu_manuel.clone() };
        let niveau = niveau_gpu(&nom_gpu);
        ui.label(format!("🎮 {} · 🧠 {ram:.0} Go · 🧮 {fils} fils", if nom_gpu.is_empty() { "carte graphique inconnue" } else { &nom_gpu }));
        if niveau.is_none() {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(255, 200, 100), "Carte non reconnue : écris son modèle →");
                ui.add(egui::TextEdit::singleline(&mut self.gpu_manuel).hint_text("ex. RTX 3060").desired_width(160.0));
            });
        }
        let n = niveau.unwrap_or(20);
        ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 chercher un jeu").desired_width(200.0));
        let f = self.filtre.to_lowercase();
        egui::Grid::new("graph").striped(true).num_columns(5).spacing([14.0, 6.0]).show(ui, |ui| {
            for t in ["Jeu", "Préréglage", "Définition", "DLSS / FSR", "Attendu"] {
                ui.strong(t);
            }
            ui.end_row();
            for (j, e, up) in JEUX.iter().filter(|j| f.is_empty() || j.0.to_lowercase().contains(&f)) {
                let (p, d, u, fps) = conseil(n, ram, *e, *up);
                ui.label(*j);
                ui.label(RichText::new(p).strong());
                ui.label(d);
                ui.label(u);
                ui.label(fps);
                ui.end_row();
            }
        });
        ui.weak("Dans les jeux compétitifs (Valorant, CS2, Fortnite), baisse ombres et effets pour gagner des FPS : la fluidité compte plus que la beauté.");
    }
}

// ===================================================================================================
// 19 · Abonnements
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Abonnement {
    pub nom: String,
    pub prix: f64,
    pub annuel: bool,
    /// prochaine date de paiement « AAAA-MM-JJ »
    pub prochain: String,
    pub prevenu: String,
}

pub const COURANTS: &[(&str, f64, bool)] = &[
    ("Netflix", 13.49, false), ("Spotify", 11.12, false), ("Disney+", 9.99, false), ("Prime", 69.90, true), ("Deezer", 11.99, false), ("YouTube Premium", 12.99, false),
    ("Xbox Game Pass Ultimate", 22.99, false), ("PlayStation Plus", 71.99, true), ("Nintendo Switch Online", 19.99, true), ("Discord Nitro", 9.99, false), ("Crunchyroll", 6.99, false), ("Canal+", 22.99, false),
];

/// Coût par mois d'un abonnement.
pub fn par_mois(a: &Abonnement) -> f64 {
    if a.annuel { a.prix / 12.0 } else { a.prix }
}

/// Date suivante après un paiement (un mois ou un an plus tard).
pub fn suivante(date: &str, annuel: bool) -> Option<String> {
    let (a, m, j) = crate::quotidien2::lire_date(date)?;
    let (a, m) = if annuel { (a + 1, m) } else if m == 12 { (a + 1, 1) } else { (a, m + 1) };
    let max = match m {
        2 => if (a % 4 == 0 && a % 100 != 0) || a % 400 == 0 { 29 } else { 28 },
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    Some(format!("{a:04}-{m:02}-{:02}", j.min(max)))
}

pub struct Abonnements {
    l: Vec<Abonnement>,
    nouveau: Abonnement,
    prix: String,
    verifie: bool,
}

impl Abonnements {
    pub fn new() -> Self {
        Abonnements { l: charger("abonnements"), nouveau: Abonnement::default(), prix: String::new(), verifie: false }
    }
}

impl Outil for Abonnements {
    fn nom(&self) -> &'static str {
        "💳 Abonnements"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.verifie {
            return;
        }
        self.verifie = true;
        let auj = aujourdhui();
        let mut change = false;
        for a in &mut self.l {
            // dates passées : on avance au prochain paiement
            while jours_entre(&a.prochain, &auj).is_some_and(|j| j > 0) {
                match suivante(&a.prochain, a.annuel) {
                    Some(s) => a.prochain = s,
                    None => break,
                }
                change = true;
            }
            if jours_entre(&auj, &a.prochain).is_some_and(|j| (0..=3).contains(&j)) && a.prevenu != a.prochain {
                crate::systeme::notifier_toujours("💳 Abonnement", &format!("{} : {:.2} € prélevés le {}. Tu t'en sers encore ?", a.nom, a.prix, a.prochain));
                a.prevenu = a.prochain.clone();
                change = true;
            }
        }
        if change {
            enregistrer("abonnements", &self.l);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Abonnements");
        let mois: f64 = self.l.iter().map(par_mois).sum();
        ui.label(RichText::new(format!("{mois:.2} € par mois · {:.0} € par an", mois * 12.0)).size(20.0).strong());
        ui.weak("Outils te prévient 3 jours avant chaque prélèvement : le moment de résilier ce qui ne sert plus.");
        let auj = aujourdhui();
        let mut l = self.l.clone();
        l.sort_by(|a, b| a.prochain.cmp(&b.prochain));
        let mut retirer = None;
        egui::Grid::new("abos").striped(true).num_columns(4).spacing([14.0, 6.0]).show(ui, |ui| {
            for a in &l {
                ui.strong(&a.nom);
                ui.label(format!("{:.2} € / {}", a.prix, if a.annuel { "an" } else { "mois" }));
                let j = jours_entre(&auj, &a.prochain);
                ui.label(match j {
                    Some(0) => "aujourd'hui".to_string(),
                    Some(j) if j > 0 => format!("le {} (dans {j} j)", a.prochain),
                    _ => a.prochain.clone(),
                });
                if ui.small_button("✖").clicked() {
                    retirer = Some(a.nom.clone());
                }
                ui.end_row();
            }
        });
        if let Some(n) = retirer {
            self.l.retain(|a| a.nom != n);
            enregistrer("abonnements", &self.l);
        }
        ui.separator();
        ui.strong("➕ Ajouter");
        ui.horizontal_wrapped(|ui| {
            for (n, p, an) in COURANTS {
                if ui.small_button(*n).clicked() {
                    self.nouveau.nom = n.to_string();
                    self.nouveau.annuel = *an;
                    self.prix = format!("{p:.2}");
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.nom).hint_text("nom").desired_width(150.0));
            ui.add(egui::TextEdit::singleline(&mut self.prix).hint_text("prix €").desired_width(70.0));
            ui.selectable_value(&mut self.nouveau.annuel, false, "par mois");
            ui.selectable_value(&mut self.nouveau.annuel, true, "par an");
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.prochain).hint_text("prochain paiement AAAA-MM-JJ").desired_width(190.0));
            let prix = self.prix.replace(',', ".").trim().parse::<f64>().ok();
            if ui.add_enabled(!self.nouveau.nom.is_empty() && prix.is_some(), egui::Button::new("Ajouter")).clicked() {
                let mut a = std::mem::take(&mut self.nouveau);
                a.prix = prix.unwrap_or(0.0);
                if crate::quotidien2::lire_date(&a.prochain).is_none() {
                    a.prochain = suivante(&auj, a.annuel).unwrap_or_default();
                }
                self.l.push(a);
                self.prix.clear();
                enregistrer("abonnements", &self.l);
            }
        });
    }
    fn resume(&self) -> Option<String> {
        (!self.l.is_empty()).then(|| format!("{:.2} € par mois", self.l.iter().map(par_mois).sum::<f64>()))
    }
}

// ===================================================================================================
// 18 · Résumé de la journée
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ReglagesResume {
    pub actif: bool,
    pub heure: u32,
    pub dernier: String,
}

impl Default for ReglagesResume {
    fn default() -> Self {
        ReglagesResume { actif: true, heure: 7, dernier: String::new() }
    }
}

/// Lignes du résumé du jour (météo, agenda, devoirs, comptes à rebours, abonnements, sorties).
pub fn lignes_du_jour() -> Vec<String> {
    let auj = aujourdhui();
    let mut v = vec![];
    if let Some(m) = crate::quotidien2::meteo_du_jour() {
        v.push(m);
    }
    let ev: Vec<crate::quotidien2::Evenement> = charger("agenda");
    let mut du_jour: Vec<&crate::quotidien2::Evenement> = ev.iter().filter(|e| crate::quotidien2::a_lieu(e, &auj)).collect();
    du_jour.sort_by_key(|e| e.heure.unwrap_or(0));
    for e in du_jour.iter().take(5) {
        v.push(format!("📅 {}{}", e.heure.map(|h| format!("{}h{:02} ", h / 60, h % 60)).unwrap_or_default(), e.titre));
    }
    let s: crate::quotidien3::Scolaire = charger("ecole");
    let demain = suivant_jour(&auj);
    let devoirs: Vec<&crate::quotidien3::Devoir> = s.devoirs.iter().filter(|d| !d.fait && (d.pour == auj || d.pour == demain)).collect();
    for d in devoirs.iter().take(5) {
        v.push(format!("🎒 {} pour {} : {}", d.matiere, if d.pour == auj { "aujourd'hui" } else { "demain" }, d.texte));
    }
    let r: Vec<crate::quotidien3::Rebours> = charger("rebours");
    for x in r.iter().filter_map(|x| Some((jours_entre(&auj, &x.date)?, x))).filter(|(j, _)| (0..=7).contains(j)) {
        v.push(format!("{} {} : {}", x.1.emoji, x.1.nom, if x.0 == 0 { "aujourd'hui !".to_string() } else { format!("dans {} jours", x.0) }));
    }
    let abos: Vec<Abonnement> = charger("abonnements");
    for a in abos.iter().filter(|a| jours_entre(&auj, &a.prochain).is_some_and(|j| (0..=3).contains(&j))) {
        v.push(format!("💳 {} : {:.2} € le {}", a.nom, a.prix, a.prochain));
    }
    let sorties: Vec<Sortie> = charger("sorties");
    for s in sorties.iter().filter(|s| jours_entre(&auj, &s.date).is_some_and(|j| (0..=2).contains(&j))) {
        v.push(format!("🎮 {} sort {}", s.nom, if s.date == auj { "aujourd'hui" } else { "bientôt" }));
    }
    v
}

fn suivant_jour(d: &str) -> String {
    let Some((a, m, j)) = crate::quotidien2::lire_date(d) else { return String::new() };
    let n = crate::quotidien2::jours_depuis_1970(a, m, j) + 1;
    // retour en date (algorithme civil)
    let z = n + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let jj = doy - (153 * mp + 2) / 5 + 1;
    let mm = if mp < 10 { mp + 3 } else { mp - 9 };
    let aa = yoe + era * 400 + if mm <= 2 { 1 } else { 0 };
    format!("{aa:04}-{mm:02}-{jj:02}")
}

pub struct ResumeJour {
    r: ReglagesResume,
    lignes: Arc<Mutex<Option<Vec<String>>>>,
    en_cours: Arc<AtomicBool>,
}

impl ResumeJour {
    pub fn new() -> Self {
        ResumeJour { r: charger("resume_jour"), lignes: Arc::default(), en_cours: Arc::default() }
    }

    fn preparer(&self, notifier: bool) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (l, e) = (self.lignes.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let v = lignes_du_jour();
            if notifier {
                let t = if v.is_empty() { "Rien de prévu aujourd'hui. Bonne journée !".to_string() } else { v.join("\n") };
                crate::systeme::notifier_toujours("☀ Ta journée", &t);
            }
            *l.lock().unwrap() = Some(v);
            e.store(false, Relaxed);
        });
    }
}

impl Outil for ResumeJour {
    fn nom(&self) -> &'static str {
        "☀ Résumé de la journée"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if !self.r.actif {
            return;
        }
        let h = crate::minecraft::horodatage_iso();
        let heure: u32 = h.get(11..13).and_then(|x| x.parse().ok()).unwrap_or(0);
        let auj = h.get(..10).unwrap_or("").to_string();
        if heure >= self.r.heure && self.r.dernier != auj && heure < 23 {
            self.r.dernier = auj;
            enregistrer("resume_jour", &self.r);
            self.preparer(true);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ta journée");
        if self.lignes.lock().unwrap().is_none() {
            self.preparer(false);
        }
        match self.lignes.lock().unwrap().clone() {
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            Some(v) if v.is_empty() => {
                ui.label("Rien de prévu aujourd'hui. Choisis ta ville dans Météo, et remplis ton agenda, tes devoirs ou tes abonnements pour les voir ici.");
            }
            Some(v) => {
                for l in v {
                    ui.label(RichText::new(l).size(16.0));
                }
            }
        }
        if ui.button("⟳").clicked() {
            *self.lignes.lock().unwrap() = None;
        }
        ui.separator();
        let mut change = ui.checkbox(&mut self.r.actif, "Recevoir ce résumé chaque matin (notification, aussi sur le téléphone)").changed();
        ui.horizontal(|ui| {
            ui.label("À partir de");
            change |= ui.add(egui::DragValue::new(&mut self.r.heure).range(4..=12).suffix(" h")).changed();
        });
        if change {
            enregistrer("resume_jour", &self.r);
        }
    }
}

// ===================================================================================================
// 22 · Traduction de documents (mise en page gardée)
// ===================================================================================================

pub fn traduire(texte: &str, langue: &str) -> Result<String, String> {
    if texte.trim().is_empty() {
        return Ok(texte.to_string());
    }
    let mut essai = 0;
    loop {
        let r = agent().post("https://translate.googleapis.com/translate_a/single").query("client", "gtx").query("sl", "auto").query("tl", langue).query("dt", "t").send_form(&[("q", texte)]);
        match r {
            Ok(r) => {
                let v: serde_json::Value = r.into_json().map_err(|e| e.to_string())?;
                return Ok(v[0].as_array().into_iter().flatten().filter_map(|m| m[0].as_str()).collect());
            }
            Err(_) if essai < 4 => {
                essai += 1;
                std::thread::sleep(Duration::from_secs(2 * essai));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn echapper_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn dechapper_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Morceaux de texte d'un XML Office : (début, fin) du contenu de chaque balise `<w:t>`, `<a:t>` ou `<t>`.
pub fn morceaux_xml(xml: &str, balise: &str) -> Vec<(usize, usize)> {
    let mut v = vec![];
    let ouvre = format!("<{balise}");
    let ferme = format!("</{balise}>");
    let mut i = 0;
    while let Some(p) = xml[i..].find(&ouvre) {
        let debut_balise = i + p;
        let apres = &xml[debut_balise + ouvre.len()..];
        // vraie balise (« <w:t> » ou « <w:t xml:space=… »), pas « <w:tab/> »
        if !(apres.starts_with('>') || apres.starts_with(' ')) {
            i = debut_balise + ouvre.len();
            continue;
        }
        let Some(g) = xml[debut_balise..].find('>') else { break };
        let d = debut_balise + g + 1;
        if xml[debut_balise..d].ends_with("/>") {
            i = d;
            continue;
        }
        let Some(f) = xml[d..].find(&ferme) else { break };
        v.push((d, d + f));
        i = d + f + ferme.len();
    }
    v
}

/// Traduit les textes d'un XML en gardant toute la mise en page (on regroupe par lots pour aller vite).
pub fn traduire_xml(xml: &str, balise: &str, langue: &str, progres: &dyn Fn(usize, usize)) -> Result<String, String> {
    let m = morceaux_xml(xml, balise);
    let textes: Vec<String> = m.iter().map(|(a, b)| dechapper_xml(&xml[*a..*b])).collect();
    let mut traduits = textes.clone();
    // lots de lignes séparées par un repère que le traducteur laisse passer
    let mut k = 0;
    while k < textes.len() {
        let mut fin = k;
        let mut taille = 0;
        while fin < textes.len() && taille + textes[fin].len() < 3500 && fin - k < 60 {
            taille += textes[fin].len() + 8;
            fin += 1;
        }
        let fin = fin.max(k + 1);
        let lot: Vec<&str> = textes[k..fin].iter().map(|s| s.as_str()).collect();
        if lot.iter().any(|t| t.chars().any(|c| c.is_alphabetic())) {
            let joint = lot.iter().enumerate().map(|(i, t)| format!("[[{i}]] {t}")).collect::<Vec<_>>().join("\n");
            let r = traduire(&joint, langue)?;
            let mut parts: Vec<Option<String>> = vec![None; lot.len()];
            for l in r.lines() {
                let l = l.trim();
                if let Some(rest) = l.strip_prefix("[[") {
                    if let Some((n, t)) = rest.split_once("]]") {
                        if let Ok(n) = n.trim().parse::<usize>() {
                            if n < parts.len() {
                                parts[n] = Some(t.trim_start().to_string());
                            }
                        }
                    }
                }
            }
            for (i, p) in parts.into_iter().enumerate() {
                match p {
                    Some(t) => traduits[k + i] = t,
                    None if lot[i].chars().any(|c| c.is_alphabetic()) => traduits[k + i] = traduire(lot[i], langue)?,
                    None => {}
                }
            }
        }
        progres(fin, textes.len());
        k = fin;
    }
    let mut o = String::with_capacity(xml.len() + xml.len() / 4);
    let mut i = 0;
    for ((a, b), t) in m.iter().zip(traduits) {
        o.push_str(&xml[i..*a]);
        // garde les espaces de début et de fin du morceau d'origine
        let orig = &xml[*a..*b];
        let debut = if orig.starts_with(' ') && !t.starts_with(' ') { " " } else { "" };
        let fin = if orig.ends_with(' ') && !t.ends_with(' ') { " " } else { "" };
        o.push_str(&format!("{debut}{}{fin}", echapper_xml(&t)));
        i = *b;
    }
    o.push_str(&xml[i..]);
    Ok(o)
}

fn traduire_office(src: &Path, dest: &Path, langue: &str, etat: &Mutex<String>) -> Result<(), String> {
    let f = std::fs::File::open(src).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let mut sortie = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| e.to_string())?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| e.to_string())?;
        let nom = e.name().to_string();
        let mut b = vec![];
        e.read_to_end(&mut b).map_err(|e| e.to_string())?;
        let balise = if (nom.starts_with("word/") && (nom.contains("document") || nom.contains("header") || nom.contains("footer") || nom.contains("footnotes"))) && nom.ends_with(".xml") {
            Some("w:t")
        } else if nom.starts_with("ppt/slides/slide") && nom.ends_with(".xml") {
            Some("a:t")
        } else if nom == "xl/sharedStrings.xml" {
            Some("t")
        } else if nom == "content.xml" {
            Some("text:p")
        } else {
            None
        };
        if let Some(bal) = balise {
            *etat.lock().unwrap() = format!("Traduction de {nom}…");
            let t = String::from_utf8_lossy(&b).to_string();
            let n = nom.clone();
            b = traduire_xml(&t, bal, langue, &|a, tot| *etat.lock().unwrap() = format!("{n} : {a}/{tot}"))?.into_bytes();
        }
        sortie.start_file(nom, opt).map_err(|e| e.to_string())?;
        sortie.write_all(&b).map_err(|e| e.to_string())?;
    }
    sortie.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn traduire_texte_brut(t: &str, langue: &str, etat: &Mutex<String>) -> Result<String, String> {
    let mut o = String::new();
    let mut lot = String::new();
    let lignes: Vec<&str> = t.lines().collect();
    for (k, l) in lignes.iter().enumerate() {
        lot.push_str(l);
        lot.push('\n');
        if lot.len() > 3500 || k + 1 == lignes.len() {
            o.push_str(&traduire(&lot, langue)?);
            o.push('\n');
            lot.clear();
            *etat.lock().unwrap() = format!("{} / {} lignes", k + 1, lignes.len());
        }
    }
    Ok(o)
}

pub fn traduire_fichier(src: &Path, langue: &str, etat: &Mutex<String>) -> Result<PathBuf, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let base = src.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let dossier = src.parent().unwrap_or(Path::new("."));
    match ext.as_str() {
        "docx" | "pptx" | "xlsx" | "odt" => {
            let dest = dossier.join(format!("{base} ({langue}).{ext}"));
            traduire_office(src, &dest, langue, etat)?;
            Ok(dest)
        }
        "pdf" => {
            let doc = lopdf::Document::load(src).map_err(|e| e.to_string())?;
            let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
            let t = doc.extract_text(&pages).map_err(|e| e.to_string())?;
            let dest = dossier.join(format!("{base} ({langue}).txt"));
            std::fs::write(&dest, traduire_texte_brut(&t, langue, etat)?).map_err(|e| e.to_string())?;
            Ok(dest)
        }
        "txt" | "md" | "srt" | "csv" | "html" | "htm" => {
            let t = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
            let dest = dossier.join(format!("{base} ({langue}).{ext}"));
            std::fs::write(&dest, traduire_texte_brut(&t, langue, etat)?).map_err(|e| e.to_string())?;
            Ok(dest)
        }
        _ => Err("format non pris en charge (Word, PowerPoint, Excel, OpenDocument, PDF, texte)".into()),
    }
}

const LANGUES: [(&str, &str); 12] = [("Français", "fr"), ("Anglais", "en"), ("Espagnol", "es"), ("Allemand", "de"), ("Italien", "it"), ("Portugais", "pt"), ("Néerlandais", "nl"), ("Arabe", "ar"), ("Japonais", "ja"), ("Chinois", "zh-CN"), ("Coréen", "ko"), ("Russe", "ru")];

pub struct TraductionDocs {
    langue: usize,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
}

impl TraductionDocs {
    pub fn new() -> Self {
        TraductionDocs { langue: 0, etat: Arc::default(), occupe: Arc::default() }
    }
}

impl Outil for TraductionDocs {
    fn nom(&self) -> &'static str {
        "📑 Traduire un document"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Traduire un document");
        ui.weak("Word, PowerPoint, Excel et OpenDocument sont traduits en gardant la mise en page (titres, gras, tableaux, images). Les PDF et les textes donnent un fichier texte traduit. Le texte passe par Google Traduction.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Vers :");
            for (k, (n, _)) in LANGUES.iter().enumerate() {
                ui.selectable_value(&mut self.langue, k, *n);
            }
        });
        let occupe = self.occupe.load(Relaxed);
        if ui.add_enabled(!occupe, egui::Button::new(RichText::new("📂 Choisir le document…").size(16.0))).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Documents", &["docx", "pptx", "xlsx", "odt", "pdf", "txt", "md", "srt", "csv", "html"]).pick_file() {
                let (e, o, l) = (self.etat.clone(), self.occupe.clone(), LANGUES[self.langue].1.to_string());
                o.store(true, Relaxed);
                std::thread::spawn(move || {
                    let r = traduire_fichier(&p, &l, &e);
                    *e.lock().unwrap() = match r {
                        Ok(d) => format!("✔ Traduit : {}", d.display()),
                        Err(x) => format!("✖ {x}"),
                    };
                    o.store(false, Relaxed);
                });
            }
        }
        if occupe {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        ui.label(self.etat.lock().unwrap().clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pratique() {
        assert_eq!(lire_date_steam("12 Mar, 2026").as_deref(), Some("2026-03-12"));
        assert_eq!(lire_date_steam("Mar 12, 2026").as_deref(), Some("2026-03-12"));
        assert_eq!(lire_date_steam("5 sept. 2026").as_deref(), Some("2026-09-05"));
        assert_eq!(lire_date_steam("2026"), None);
        assert_eq!(appid_de("https://store.steampowered.com/app/1234560/Super_Jeu/"), Some(1234560));
        assert_eq!(suivante("2026-01-31", false).as_deref(), Some("2026-02-28"));
        assert_eq!(suivante("2026-12-15", false).as_deref(), Some("2027-01-15"));
        assert_eq!(suivante("2024-02-29", true).as_deref(), Some("2025-02-28"));
        assert_eq!(suivant_jour("2026-12-31"), "2027-01-01");
        assert_eq!(suivant_jour("2028-02-28"), "2028-02-29");
        assert_eq!(par_mois(&Abonnement { prix: 120.0, annuel: true, ..Default::default() }), 10.0);
        assert_eq!(conseil(100, 32.0, 2, false).0, "Ultra");
        assert_eq!(conseil(20, 8.0, 9, true).0, "Très bas (le jeu risque de ramer)");
        assert_eq!(niveau_gpu("NVIDIA GeForce RTX 3060"), Some(36));
        let x = r#"<w:p><w:r><w:t>Bonjour </w:t></w:r><w:r><w:tab/><w:t xml:space="preserve">le monde</w:t></w:r><w:r><w:t/></w:r></w:p>"#;
        let m = morceaux_xml(x, "w:t");
        assert_eq!(m.iter().map(|(a, b)| &x[*a..*b]).collect::<Vec<_>>(), vec!["Bonjour ", "le monde"]);
        assert_eq!(dechapper_xml("a &amp; b &lt;c&gt;"), "a & b <c>");
    }
}
