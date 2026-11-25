//! Le téléphone au service du PC : manette (13), souris et clavier (14), écran du PC (16),
//! webcam et micro (15), traduction avec l'appareil photo (12).
use crate::android::{self, Camera, Capteurs, MicroBrut};
use crate::commun::*;
use crate::jeux_tel::appuis;
use crate::monpc::{encoder, Pc};
use crate::outil::{charger, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT_ENTREES: u16 = 8644;
pub const PORT_SON: u16 = 8645;

/// Configuration du PC associé (relue toutes les 5 s au plus : ces écrans se redessinent 60 fois par seconde).
fn pc() -> Option<Pc> {
    static CACHE: Mutex<Option<(Instant, Option<Pc>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    if c.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(5)) {
        *c = Some((Instant::now(), charger::<Option<Pc>>("pc").filter(|p| !p.ip.is_empty())));
    }
    c.as_ref().and_then(|x| x.1.clone())
}

fn sans_pc(ui: &mut egui::Ui) {
    ui.label(RichText::new("Associe d'abord ton PC dans « 🖥 Mon PC ».").size(18.0));
    ui.weak("Outils doit être ouvert sur le PC (Téléphone › Démarrer), sur le même Wi-Fi.");
}

/// Envoie des commandes de souris et de clavier au PC (paquets UDP, très rapides).
pub struct Envoi {
    s: UdpSocket,
    ip: String,
    cle: String,
}

impl Envoi {
    pub fn new(p: &Pc) -> Option<Envoi> {
        Some(Envoi { s: UdpSocket::bind("0.0.0.0:0").ok()?, ip: p.ip.clone(), cle: p.cle.clone() })
    }

    pub fn envoyer(&self, cmds: &[String]) {
        if cmds.is_empty() {
            return;
        }
        let m = format!("{}|{}", self.cle, cmds.join("\n"));
        let _ = self.s.send_to(m.as_bytes(), (self.ip.as_str(), PORT_ENTREES));
    }
}

/// Tourne une image d'un quart de tour vers la droite (le capteur photo est « couché »).
pub fn tourner(l: usize, h: usize, px: &[u8], canaux: usize) -> Vec<u8> {
    let mut o = vec![0u8; l * h * canaux];
    for y in 0..h {
        for x in 0..l {
            let (src, dst) = ((y * l + x) * canaux, (x * h + (h - 1 - y)) * canaux);
            o[dst..dst + canaux].copy_from_slice(&px[src..src + canaux]);
        }
    }
    o
}

// ===================================================================================================
// 13 · Manette
// ===================================================================================================

/// (bouton, touche « Jeux PC », touche « Flèches »)
pub const BOUTONS: [(&str, &str, &str); 12] = [
    ("▲", "w", "haut"), ("▼", "s", "bas"), ("◀", "a", "gauche"), ("▶", "d", "droite"),
    ("A", "espace", "entree"), ("B", "maj", "echap"), ("X", "e", "espace"), ("Y", "r", "tab"),
    ("L", "q", "q"), ("R", "f", "f"), ("Start", "echap", "echap"), ("Select", "m", "m"),
];

/// Profil « vraie manette Xbox » : le PC crée une manette Xbox 360 reconnue par Windows et les jeux.
pub const XBOX: [&str; 12] = ["xbox:stick_haut", "xbox:stick_bas", "xbox:stick_gauche", "xbox:stick_droite", "xbox:a", "xbox:b", "xbox:x", "xbox:y", "xbox:lb", "xbox:rb", "xbox:start", "xbox:back"];

/// Emplacement de chaque bouton (en fraction de la zone).
fn place(k: usize) -> (f32, f32, f32) {
    match k {
        0 => (0.18, 0.38, 0.09),
        1 => (0.18, 0.78, 0.09),
        2 => (0.08, 0.58, 0.09),
        3 => (0.28, 0.58, 0.09),
        4 => (0.82, 0.78, 0.08),
        5 => (0.92, 0.58, 0.08),
        6 => (0.72, 0.58, 0.08),
        7 => (0.82, 0.38, 0.08),
        8 => (0.12, 0.1, 0.07),
        9 => (0.88, 0.1, 0.07),
        10 => (0.58, 0.2, 0.06),
        _ => (0.42, 0.2, 0.06),
    }
}

/// Une disposition de manette : touche envoyée et place de chaque bouton, jeu associé, vibration.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct ProfilManette {
    pub nom: String,
    /// bout du nom du jeu (sur le PC) pour lequel ce profil se choisit tout seul ; vide = aucun
    pub jeu: String,
    pub touches: Vec<String>,
    pub places: Vec<(f32, f32, f32)>,
    /// 0 = pas de vibration, 1 = légère, 2 = forte
    pub vibration: u8,
}

impl Default for ProfilManette {
    fn default() -> Self {
        profil_de_base("Jeux PC (ZQSD, Espace…)", 1)
    }
}

fn profil_de_base(nom: &str, colonne: usize) -> ProfilManette {
    ProfilManette {
        nom: nom.into(),
        jeu: String::new(),
        touches: BOUTONS.iter().enumerate().map(|(k, b)| match colonne {
            1 => b.1,
            2 => b.2,
            _ => XBOX[k],
        }.to_string()).collect(),
        places: (0..BOUTONS.len()).map(place).collect(),
        vibration: 1,
    }
}

/// Profils enregistrés (les deux de base s'ils n'existent pas encore), toujours complets.
pub fn profils_manette() -> Vec<ProfilManette> {
    let mut v: Vec<ProfilManette> = charger("manette_profils");
    if v.is_empty() {
        v = vec![profil_de_base("Jeux PC (ZQSD, Espace…)", 1), profil_de_base("Flèches, Entrée", 2)];
    }
    if !v.iter().any(|p| p.touches.iter().any(|t| t.starts_with("xbox:"))) {
        v.push(profil_de_base("🎮 Vraie manette Xbox", 3));
    }
    for p in &mut v {
        let base = profil_de_base("", 1);
        p.touches.resize(BOUTONS.len(), String::new());
        p.places.resize(BOUTONS.len(), (0.5, 0.5, 0.07));
        for k in 0..BOUTONS.len() {
            if p.touches[k].is_empty() {
                p.touches[k].clone_from(&base.touches[k]);
            }
        }
    }
    v
}

/// Le profil dont le jeu correspond au jeu lancé sur le PC.
pub fn profil_pour(profils: &[ProfilManette], jeu: &str) -> Option<usize> {
    let j = jeu.to_lowercase();
    profils.iter().position(|p| !p.jeu.trim().is_empty() && j.contains(&p.jeu.trim().to_lowercase()))
}

/// Touches qu'on peut donner à un bouton (noms compris par Outils sur PC).
pub const TOUCHES: &[&str] = &[
    "espace", "entree", "echap", "tab", "maj", "ctrl", "alt", "retour", "haut", "bas", "gauche", "droite", "w", "a", "s", "d", "q", "e", "r", "f", "z", "x", "c", "v",
    "t", "g", "b", "y", "h", "n", "u", "j", "m", "i", "k", "o", "l", "p", "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "f1", "f2", "f3", "f4", "f5", "f6",
    "f7", "f8", "f9", "f10", "f11", "f12", "xbox:a", "xbox:b", "xbox:x", "xbox:y", "xbox:lb", "xbox:rb", "xbox:lt", "xbox:rt", "xbox:start", "xbox:back",
    "xbox:l3", "xbox:r3", "xbox:haut", "xbox:bas", "xbox:gauche", "xbox:droite", "xbox:stick_haut", "xbox:stick_bas", "xbox:stick_gauche", "xbox:stick_droite",
];

/// Nom lisible d'une touche (« w » est la touche Z d'un clavier AZERTY : même position).
pub fn libelle_touche(t: &str) -> String {
    if let Some(x) = t.strip_prefix("xbox:") {
        return format!("🎮 {}", match x {
            "stick_haut" => "stick ↑",
            "stick_bas" => "stick ↓",
            "stick_gauche" => "stick ←",
            "stick_droite" => "stick →",
            "haut" => "croix ↑",
            "bas" => "croix ↓",
            "gauche" => "croix ←",
            "droite" => "croix →",
            autre => autre,
        }.to_uppercase());
    }
    match t {
        "w" => "Z".into(),
        "a" => "Q".into(),
        "q" => "A".into(),
        "z" => "W".into(),
        "m" => ",".into(),
        x if x.len() == 1 => x.to_uppercase(),
        x => x.to_string(),
    }
}

pub struct Manette {
    envoi: Option<Envoi>,
    doigts: HashMap<u64, Pos2>,
    appuyes: HashSet<usize>,
    profils: Vec<ProfilManette>,
    profil: usize,
    volant: bool,
    capteurs: Option<Capteurs>,
    rappel: Instant,
    edition: bool,
    choisi: Option<usize>,
    jeu_pc: Arc<Mutex<Option<String>>>,
    fil_jeu: Arc<AtomicBool>,
    vu: Instant,
    auto: Option<String>,
}

impl Manette {
    pub fn new() -> Self {
        let profils = profils_manette();
        let profil = charger::<usize>("manette_profil").min(profils.len() - 1);
        Manette {
            envoi: None, doigts: HashMap::new(), appuyes: HashSet::new(), profils, profil, volant: false, capteurs: None, rappel: Instant::now(), edition: false, choisi: None,
            jeu_pc: Arc::default(), fil_jeu: Arc::default(), vu: Instant::now(), auto: None,
        }
    }

    fn p(&self) -> &ProfilManette {
        &self.profils[self.profil.min(self.profils.len() - 1)]
    }

    fn touche(&self, k: usize) -> String {
        self.p().touches.get(k).cloned().unwrap_or_default()
    }

    fn sauver(&self) {
        crate::outil::enregistrer("manette_profils", &self.profils);
        crate::outil::enregistrer("manette_profil", &self.profil);
    }

    fn relacher(&mut self) {
        if let Some(e) = &self.envoi {
            let l: Vec<String> = self.appuyes.iter().map(|k| format!("k {} up", self.touche(*k))).collect();
            e.envoyer(&l);
        }
        self.appuyes.clear();
    }

    /// Demande au PC, tant que la manette est affichée, quel jeu tourne (profil automatique).
    fn suivre_jeu(&self, p: Pc) {
        if self.fil_jeu.swap(true, Relaxed) {
            return;
        }
        let (j, f) = (self.jeu_pc.clone(), self.fil_jeu.clone());
        std::thread::spawn(move || {
            while f.load(Relaxed) {
                if let Ok(v) = p.get_json("/api/etat") {
                    *j.lock().unwrap() = v["jeu"].as_str().map(|s| s.to_string());
                }
                for _ in 0..20 {
                    if !f.load(Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        });
    }

    fn editeur(&mut self, ui: &mut egui::Ui) {
        let pr = self.profil;
        ui.horizontal_wrapped(|ui| {
            ui.label("Nom :");
            ui.add(egui::TextEdit::singleline(&mut self.profils[pr].nom).desired_width(170.0));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Jeu associé :");
            ui.add(egui::TextEdit::singleline(&mut self.profils[pr].jeu).hint_text("ex. minecraft, fortnite").desired_width(150.0));
            if let Some(j) = self.jeu_pc.lock().unwrap().clone() {
                if ui.small_button(format!("= {j}")).clicked() {
                    self.profils[pr].jeu = j;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Vibration :");
            for (v, t) in [(0u8, "aucune"), (1, "légère"), (2, "forte")] {
                ui.selectable_value(&mut self.profils[pr].vibration, v, t);
            }
        });
        match self.choisi {
            Some(k) => {
                ui.label(RichText::new(format!("Bouton {} : glisse-le pour le déplacer", BOUTONS[k].0)).strong());
                ui.horizontal_wrapped(|ui| {
                    ui.label("Taille");
                    ui.add(egui::Slider::new(&mut self.profils[pr].places[k].2, 0.04..=0.16).show_value(false));
                    egui::ComboBox::from_id_salt("touche_manette").selected_text(format!("Touche : {}", libelle_touche(&self.profils[pr].touches[k]))).show_ui(ui, |ui| {
                        for t in TOUCHES {
                            ui.selectable_value(&mut self.profils[pr].touches[k], t.to_string(), libelle_touche(t));
                        }
                    });
                });
            }
            None => {
                ui.weak("Touche un bouton pour changer sa touche ou sa taille, glisse-le pour le déplacer.");
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("✔ Terminé").clicked() {
                self.edition = false;
                self.choisi = None;
                self.sauver();
            }
            if ui.button("↺ Places d'origine").clicked() {
                self.profils[pr].places = (0..BOUTONS.len()).map(place).collect();
            }
            if self.profils.len() > 1 && ui.button("🗑 Supprimer ce profil").clicked() {
                self.profils.remove(pr);
                self.profil = 0;
                self.edition = false;
                self.choisi = None;
                self.sauver();
            }
        });
    }
}

impl Outil for Manette {
    fn nom(&self) -> &'static str {
        "🎮 Manette pour le PC"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _: &egui::Context) {
        // l'outil n'est plus à l'écran : on relâche tout et on arrête de demander le jeu au PC
        if self.vu.elapsed() > Duration::from_secs(1) {
            if !self.appuyes.is_empty() {
                self.relacher();
            }
            self.capteurs = None;
            self.fil_jeu.store(false, Relaxed);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        if self.envoi.as_ref().is_none_or(|e| e.ip != p.ip) {
            self.envoi = Envoi::new(&p);
        }
        self.suivre_jeu(p.clone());
        // profil automatique quand le jeu change sur le PC
        let jeu = self.jeu_pc.lock().unwrap().clone();
        if jeu != self.auto {
            self.auto.clone_from(&jeu);
            if let Some(k) = jeu.as_deref().and_then(|j| profil_pour(&self.profils, j)) {
                if k != self.profil && !self.edition {
                    self.relacher();
                    self.profil = k;
                }
            }
        }
        let avant = self.profil;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("profil_manette").selected_text(self.p().nom.clone()).show_ui(ui, |ui| {
                for (k, pr) in self.profils.iter().enumerate() {
                    let t = if pr.jeu.trim().is_empty() { pr.nom.clone() } else { format!("{} · 🎮 {}", pr.nom, pr.jeu) };
                    ui.selectable_value(&mut self.profil, k, t);
                }
            });
            if !self.edition && ui.button("✏ Modifier").clicked() {
                self.edition = true;
            }
            if !self.edition && ui.button("➕ Nouveau").clicked() {
                let mut n = self.p().clone();
                n.nom = format!("Profil {}", self.profils.len() + 1);
                n.jeu = jeu.clone().unwrap_or_default();
                self.profils.push(n);
                self.profil = self.profils.len() - 1;
                self.edition = true;
            }
            ui.checkbox(&mut self.volant, "🚗 Volant (incliner)");
        });
        if self.profil != avant {
            self.relacher();
            self.sauver();
        }
        if let Some(j) = &jeu {
            ui.weak(format!("Sur le PC : 🎮 {j}"));
        }
        if self.edition {
            self.editeur(ui);
        } else {
            ui.weak("Tourne le téléphone à l'horizontale. Les touches arrivent sur le PC comme un vrai clavier.");
        }
        let h = (ui.ctx().screen_rect().height() * 0.62).max(260.0);
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click_and_drag());
        let places = self.p().places.clone();
        let rayon = |k: usize| places[k].2 * zone.width().min(zone.height() * 1.6);
        let centre = |k: usize| zone.min + Vec2::new(places[k].0 * zone.width(), places[k].1 * zone.height());
        let pt = ui.painter_at(zone);
        pt.rect_filled(zone, 18.0, Color32::from_rgb(28, 24, 40));
        if self.edition {
            // déplacer / choisir un bouton
            if let Some(pos) = rep.interact_pointer_pos() {
                if rep.drag_started() || rep.clicked() {
                    self.choisi = (0..BOUTONS.len()).filter(|k| centre(*k).distance(pos) < rayon(*k) * 1.3).min_by(|a, b| centre(*a).distance(pos).total_cmp(&centre(*b).distance(pos)));
                }
                if rep.dragged() {
                    if let Some(k) = self.choisi {
                        let pr = self.profil;
                        let x = ((pos.x - zone.min.x) / zone.width()).clamp(0.03, 0.97);
                        let y = ((pos.y - zone.min.y) / zone.height()).clamp(0.05, 0.95);
                        self.profils[pr].places[k].0 = x;
                        self.profils[pr].places[k].1 = y;
                    }
                }
            }
            self.doigts.clear();
        } else {
            appuis(ui, &mut self.doigts);
            let mut maintenant: HashSet<usize> = HashSet::new();
            for d in self.doigts.values() {
                for k in 0..BOUTONS.len() {
                    if centre(k).distance(*d) < rayon(k) * 1.25 {
                        maintenant.insert(k);
                    }
                }
            }
            let mut axe = None;
            if self.volant {
                let c = self.capteurs.get_or_insert_with(|| Capteurs::demarrer(true, false, false));
                let a = c.m.lock().unwrap().acc;
                let pente = if zone.width() > zone.height() { a[1] } else { -a[0] };
                if self.touche(2).starts_with("xbox:") {
                    // vraie manette : le volant pilote le stick gauche, en douceur
                    axe = Some((pente / 6.0).clamp(-1.0, 1.0));
                } else if pente > 2.0 {
                    maintenant.insert(3);
                } else if pente < -2.0 {
                    maintenant.insert(2);
                }
            } else {
                self.capteurs = None;
            }
            let mut cmds = vec![];
            let vib = match self.p().vibration {
                0 => 0,
                1 => 12,
                _ => 35,
            };
            for k in maintenant.difference(&self.appuyes) {
                cmds.push(format!("k {} down", self.touche(*k)));
                if vib > 0 {
                    android::vibrer(vib);
                }
            }
            for k in self.appuyes.difference(&maintenant) {
                cmds.push(format!("k {} up", self.touche(*k)));
            }
            if let Some(v) = axe {
                cmds.push(format!("x lx {v:.3}"));
            }
            // rappel régulier des touches tenues (le PC les relâche sinon au bout de 3 s)
            if self.rappel.elapsed() > Duration::from_millis(800) {
                self.rappel = Instant::now();
                cmds.extend(maintenant.iter().map(|k| format!("k {} down", self.touche(*k))));
            }
            if let Some(e) = &self.envoi {
                e.envoyer(&cmds);
            }
            self.appuyes = maintenant;
        }
        for (k, (nom, ..)) in BOUTONS.iter().enumerate() {
            let on = self.appuyes.contains(&k) || self.choisi == Some(k) && self.edition;
            let c = match *nom {
                "A" => Color32::from_rgb(90, 200, 120),
                "B" => Color32::from_rgb(230, 80, 90),
                "X" => Color32::from_rgb(80, 150, 240),
                "Y" => Color32::from_rgb(240, 200, 70),
                _ => Color32::from_gray(90),
            };
            pt.circle_filled(centre(k), rayon(k), if on { c } else { c.gamma_multiply(0.45) });
            pt.circle_stroke(centre(k), rayon(k), Stroke::new(if self.edition && self.choisi == Some(k) { 4.0_f32 } else { 2.0 }, Color32::from_white_alpha(if self.edition { 160 } else { 60 })));
            pt.text(centre(k), Align2::CENTER_CENTER, nom, FontId::proportional(rayon(k) * 0.7), Color32::WHITE);
            if self.edition {
                pt.text(centre(k) + Vec2::new(0.0, rayon(k) * 0.62), Align2::CENTER_CENTER, libelle_touche(&self.touche(k)), FontId::proportional((rayon(k) * 0.32).max(10.0)), Color32::from_white_alpha(200));
            }
        }
        ui.ctx().request_repaint();
        let t = |k: usize| libelle_touche(&self.touche(k));
        ui.weak(format!("A = {} · B = {} · X = {} · Y = {} · L = {} · R = {}", t(4), t(5), t(6), t(7), t(8), t(9)));
    }
}

// ===================================================================================================
// 14 · Souris et clavier, 16 · écran du PC
// ===================================================================================================

pub struct SourisClavier {
    envoi: Option<Envoi>,
    texte: String,
    sens: f32,
    reste: Vec2,
    ecran: bool,
    image: Arc<Mutex<Option<(u64, image::RgbImage)>>>,
    texture: Option<(u64, egui::TextureHandle)>,
    fil: Arc<AtomicBool>,
    vu: Instant,
    deux_doigts: Option<(Instant, f32)>,
}

impl SourisClavier {
    pub fn new() -> Self {
        SourisClavier { envoi: None, texte: String::new(), sens: 2.0, reste: Vec2::ZERO, ecran: false, image: Arc::default(), texture: None, fil: Arc::default(), vu: Instant::now(), deux_doigts: None }
    }

    /// Récupère des images de l'écran du PC tant que l'outil est affiché.
    fn lancer_ecran(&self, p: Pc, largeur: u32) {
        if self.fil.swap(true, Relaxed) {
            return;
        }
        let (img, fil) = (self.image.clone(), self.fil.clone());
        std::thread::spawn(move || {
            let mut n = 0u64;
            while fil.load(Relaxed) {
                if let Ok(r) = ureq::get(&p.url(&format!("/api/ecran?l={largeur}"))).timeout(Duration::from_secs(5)).call() {
                    let mut b = vec![];
                    if std::io::Read::read_to_end(&mut r.into_reader(), &mut b).is_ok() {
                        if let Ok(i) = image::load_from_memory(&b) {
                            n += 1;
                            *img.lock().unwrap() = Some((n, i.to_rgb8()));
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(350));
            }
        });
    }
}

impl Outil for SourisClavier {
    fn nom(&self) -> &'static str {
        "🖱 Souris, clavier et écran du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.elapsed() > Duration::from_secs(1) {
            self.fil.store(false, Relaxed);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        if self.envoi.as_ref().is_none_or(|e| e.ip != p.ip) {
            self.envoi = Envoi::new(&p);
        }
        let envoyer = |e: &Option<Envoi>, c: Vec<String>| {
            if let Some(e) = e {
                e.envoyer(&c);
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.ecran, false, RichText::new("🖱 Pavé tactile").size(17.0));
            ui.selectable_value(&mut self.ecran, true, RichText::new("📺 Écran du PC").size(17.0));
        });
        if self.ecran {
            let w = ui.available_width();
            self.lancer_ecran(p.clone(), ((w * ui.ctx().pixels_per_point()) as u32).clamp(480, 1280));
            if let Some((n, img)) = self.image.lock().unwrap().clone() {
                let ci = egui::ColorImage::from_rgb([img.width() as usize, img.height() as usize], img.as_raw());
                match &mut self.texture {
                    Some((m, t)) if *m != n => {
                        t.set(ci, Default::default());
                        *m = n;
                    }
                    Some(_) => {}
                    None => self.texture = Some((n, ui.ctx().load_texture("ecran_pc", ci, Default::default()))),
                }
            }
            match &self.texture {
                Some((_, t)) => {
                    let r = ui.add(egui::Image::new(t).max_width(w).sense(egui::Sense::click())).on_hover_text("Touche pour cliquer à cet endroit");
                    if r.clicked() {
                        if let Some(pos) = r.interact_pointer_pos() {
                            let (x, y) = ((pos.x - r.rect.left()) / r.rect.width(), (pos.y - r.rect.top()) / r.rect.height());
                            envoyer(&self.envoi, vec![format!("a {x:.4} {y:.4}"), "c g".into()]);
                        }
                    }
                    if r.secondary_clicked() || r.long_touched() {
                        if let Some(pos) = r.interact_pointer_pos() {
                            let (x, y) = ((pos.x - r.rect.left()) / r.rect.width(), (pos.y - r.rect.top()) / r.rect.height());
                            envoyer(&self.envoi, vec![format!("a {x:.4} {y:.4}"), "c d".into()]);
                        }
                    }
                    ui.weak("Touche = clic gauche · appui long = clic droit. Environ 3 images par seconde.");
                }
                None => {
                    ui.spinner();
                }
            }
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        } else {
            self.fil.store(false, Relaxed);
            ui.add(egui::Slider::new(&mut self.sens, 0.5..=5.0).text("vitesse"));
            let h = (ui.ctx().screen_rect().height() * 0.42).max(220.0);
            let (zone, rep) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click_and_drag());
            ui.painter().rect_filled(zone, 16.0, ui.visuals().extreme_bg_color);
            ui.painter().text(zone.center(), Align2::CENTER_CENTER, "Glisse pour bouger · touche pour cliquer\ndeux doigts : faire défiler", FontId::proportional(15.0), Color32::GRAY);
            let mt = ui.input(|i| i.multi_touch());
            let mut cmds = vec![];
            match mt {
                Some(m) if m.num_touches >= 2 => {
                    let d = self.deux_doigts.get_or_insert((Instant::now(), 0.0));
                    d.1 += m.translation_delta.y;
                    let crans = (d.1 / 25.0) as i32;
                    if crans != 0 {
                        cmds.push(format!("w {}", crans));
                        d.1 -= crans as f32 * 25.0;
                    }
                }
                _ => {
                    // deux doigts posés puis levés vite sans bouger : clic droit
                    if let Some((t, _)) = self.deux_doigts.take() {
                        if t.elapsed() < Duration::from_millis(250) {
                            cmds.push("c d".into());
                        }
                    }
                    if rep.dragged() {
                        let d = rep.drag_delta();
                        let acc = 1.0 + d.length() / 25.0;
                        self.reste += d * self.sens * acc;
                        let (dx, dy) = (self.reste.x.trunc() as i32, self.reste.y.trunc() as i32);
                        if dx != 0 || dy != 0 {
                            cmds.push(format!("m {dx} {dy}"));
                            self.reste -= Vec2::new(dx as f32, dy as f32);
                        }
                    }
                    if rep.clicked() {
                        cmds.push("c g".into());
                    }
                }
            }
            envoyer(&self.envoi, cmds);
            ui.horizontal_wrapped(|ui| {
                let l = ui.available_width() / 2.0 - 6.0;
                if ui.add_sized([l, 48.0], egui::Button::new("Clic gauche")).clicked() {
                    envoyer(&self.envoi, vec!["c g".into()]);
                }
                if ui.add_sized([l, 48.0], egui::Button::new("Clic droit")).clicked() {
                    envoyer(&self.envoi, vec!["c d".into()]);
                }
            });
        }
        ui.separator();
        ui.label("Clavier (ce que tu tapes part sur le PC) :");
        let avant = self.texte.clone();
        ui.add(egui::TextEdit::singleline(&mut self.texte).hint_text("tape ici…").desired_width(f32::INFINITY));
        if self.texte != avant {
            if self.texte.starts_with(&avant) {
                envoyer(&self.envoi, vec![format!("t {}", &self.texte[avant.len()..])]);
            } else {
                let n = avant.chars().count().saturating_sub(self.texte.chars().count()).max(1);
                envoyer(&self.envoi, vec!["k retour".to_string(); n]);
            }
            if self.texte.chars().count() > 40 {
                self.texte.clear();
            }
        }
        ui.horizontal_wrapped(|ui| {
            for (n, c) in [("Échap", "k echap"), ("Tab", "k tab"), ("Entrée", "k entree"), ("⌫", "k retour"), ("←", "k gauche"), ("↑", "k haut"), ("↓", "k bas"), ("→", "k droite"), ("Win", "k win"), ("Suppr", "k suppr")] {
                if ui.button(n).clicked() {
                    envoyer(&self.envoi, vec![c.into()]);
                    if c == "k entree" {
                        self.texte.clear();
                    }
                }
            }
            if ui.button("Alt+Tab").clicked() {
                envoyer(&self.envoi, vec!["k alt down".into(), "k tab".into(), "k alt up".into()]);
            }
            if ui.button("Copier").clicked() {
                envoyer(&self.envoi, vec!["k ctrl down".into(), "k c".into(), "k ctrl up".into()]);
            }
            if ui.button("Coller").clicked() {
                envoyer(&self.envoi, vec!["k ctrl down".into(), "k v".into(), "k ctrl up".into()]);
            }
        });
    }
}

// ===================================================================================================
// 15 · Webcam et micro pour le PC
// ===================================================================================================

pub struct WebcamMicro {
    cam: Option<Camera>,
    micro: Option<MicroBrut>,
    envoi_image: Arc<AtomicBool>,
    texture: Option<egui::TextureHandle>,
    vu: Instant,
}

impl WebcamMicro {
    pub fn new() -> Self {
        WebcamMicro { cam: None, micro: None, envoi_image: Arc::default(), texture: None, vu: Instant::now() }
    }
}

impl Outil for WebcamMicro {
    fn nom(&self) -> &'static str {
        "📷 Webcam et micro pour le PC"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _: &egui::Context) {
        // l'appli passe en arrière-plan : on coupe la caméra (le micro peut continuer)
        if self.cam.is_some() && self.vu.elapsed() > Duration::from_secs(2) {
            self.cam = None;
            self.envoi_image.store(false, Relaxed);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        let mut cam = self.cam.is_some();
        if ui.checkbox(&mut cam, RichText::new("📷 Envoyer la caméra au PC").size(18.0)).changed() {
            if cam {
                if !android::a_permission("android.permission.CAMERA") {
                    android::demander(&["android.permission.CAMERA"]);
                } else {
                    let c = Camera::demarrer_avec(true);
                    let (coul, actif, pc) = (c.couleur.clone(), self.envoi_image.clone(), p.clone());
                    actif.store(true, Relaxed);
                    std::thread::spawn(move || {
                        while actif.load(Relaxed) {
                            std::thread::sleep(Duration::from_millis(110));
                            let Some((l, h, rgb)) = coul.lock().unwrap().take() else { continue };
                            let t = tourner(l, h, &rgb, 3);
                            let Some(img) = image::RgbImage::from_raw(h as u32, l as u32, t) else { continue };
                            let mut b = vec![];
                            if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 65).encode_image(&img).is_ok() {
                                let _ = ureq::post(&pc.url("/api/webcam")).timeout(Duration::from_secs(3)).send_bytes(&b);
                            }
                        }
                    });
                    self.cam = Some(c);
                }
            } else {
                self.cam = None;
                self.envoi_image.store(false, Relaxed);
            }
        }
        if let Some(c) = &self.cam {
            if let Some((l, h, y)) = c.image.lock().unwrap().clone() {
                let t = tourner(l, h, &y, 1);
                let ci = egui::ColorImage::from_gray([h, l], &t);
                match &mut self.texture {
                    Some(tx) => tx.set(ci, Default::default()),
                    None => self.texture = Some(ui.ctx().load_texture("apercu_webcam", ci, Default::default())),
                }
            }
            if let Some(t) = &self.texture {
                ui.add(egui::Image::new(t).max_width(ui.available_width().min(320.0)));
            }
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let mut mic = self.micro.is_some();
        if ui.checkbox(&mut mic, RichText::new("🎙 Envoyer le micro au PC").size(18.0)).changed() {
            if mic {
                if !android::a_permission("android.permission.RECORD_AUDIO") {
                    android::demander(&["android.permission.RECORD_AUDIO"]);
                } else if let Ok(s) = UdpSocket::bind("0.0.0.0:0") {
                    let (ip, cle) = (p.ip.clone(), p.cle.clone());
                    self.micro = Some(MicroBrut::demarrer(move |ech| {
                        let mut b = cle.as_bytes().to_vec();
                        b.push(b'|');
                        for x in ech {
                            b.extend_from_slice(&x.to_le_bytes());
                        }
                        let _ = s.send_to(&b, (ip.as_str(), PORT_SON));
                    }));
                }
            } else {
                self.micro = None;
            }
        }
        if let Some(m) = &self.micro {
            let e = m.erreur.lock().unwrap().clone();
            if !e.is_empty() {
                ui.colored_label(ROUGE, e);
            }
        }
        ui.separator();
        ui.weak("Sur le PC, l'image apparaît dans Outils › Téléphone : dans OBS, ajoute une « capture de fenêtre » d'Outils (puis la caméra virtuelle d'OBS pour Discord). Le micro arrive dans le câble virtuel VB-CABLE (installable depuis Outils › Micro) : choisis « CABLE Output » comme micro.");
    }
}

// ===================================================================================================
// 12 · Traduire avec l'appareil photo (lecture du texte par le PC)
// ===================================================================================================

pub struct TraductionPhoto {
    cam: Option<Camera>,
    texture: Option<egui::TextureHandle>,
    langue: usize,
    e: Etat,
    resultat: Arc<Mutex<Option<(String, String)>>>,
    vu: Instant,
}

const LANGUES: [(&str, &str); 8] = [("Français", "fr"), ("Anglais", "en"), ("Espagnol", "es"), ("Allemand", "de"), ("Italien", "it"), ("Portugais", "pt"), ("Japonais", "ja"), ("Arabe", "ar")];

impl TraductionPhoto {
    pub fn new() -> Self {
        TraductionPhoto { cam: None, texture: None, langue: 0, e: Arc::default(), resultat: Arc::default(), vu: Instant::now() }
    }
}

impl Outil for TraductionPhoto {
    fn nom(&self) -> &'static str {
        "🌍 Traduire avec l'appareil photo"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.cam.is_some() && self.vu.elapsed() > Duration::from_secs(1) {
            self.cam = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(p) = pc() else {
            sans_pc(ui);
            ui.weak("La lecture du texte se fait sur le PC (lecteur de texte de Windows), puis la traduction par internet.");
            return;
        };
        if !android::a_permission("android.permission.CAMERA") {
            if gros_bouton(ui, "📷 Autoriser l'appareil photo") {
                android::demander(&["android.permission.CAMERA"]);
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Traduire en");
            for (k, (n, _)) in LANGUES.iter().enumerate() {
                ui.selectable_value(&mut self.langue, k, *n);
            }
        });
        let c = self.cam.get_or_insert_with(Camera::demarrer);
        let img = c.image.lock().unwrap().clone();
        if let Some((l, h, y)) = &img {
            let t = tourner(*l, *h, y, 1);
            let ci = egui::ColorImage::from_gray([*h, *l], &t);
            match &mut self.texture {
                Some(tx) => tx.set(ci, Default::default()),
                None => self.texture = Some(ui.ctx().load_texture("trad_cam", ci, Default::default())),
            }
        }
        if let Some(t) = &self.texture {
            ui.add(egui::Image::new(t).max_width(ui.available_width().min(360.0)));
        }
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        let occupe = etat_ui(ui, &self.e);
        if gros_bouton(ui, "📸 Traduire ce texte") && !occupe {
            if let Some((l, h, y)) = img {
                let t = tourner(l, h, &y, 1);
                let (pc, r, langue) = (p.clone(), self.resultat.clone(), LANGUES[self.langue].1);
                en_fond(&self.e, "Lecture et traduction…", move || {
                    let gris = image::GrayImage::from_raw(h as u32, l as u32, t).ok_or("image vide")?;
                    let mut b = vec![];
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 90).encode_image(&gris).map_err(|e| e.to_string())?;
                    let j: serde_json::Value = ureq::post(&pc.url(&format!("/api/ocr?langue={}", encoder(langue)))).timeout(Duration::from_secs(30)).send_bytes(&b).map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
                    let texte = j["texte"].as_str().unwrap_or("").to_string();
                    if texte.trim().is_empty() {
                        return Err("aucun texte lu : rapproche-toi et tiens le téléphone bien droit".into());
                    }
                    *r.lock().unwrap() = Some((texte, j["traduction"].as_str().unwrap_or("").to_string()));
                    Ok(String::new())
                });
            }
        }
        if let Some((texte, trad)) = self.resultat.lock().unwrap().clone() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(&trad).size(19.0).strong());
                ui.weak(&texte);
                if ui.button("📋 Copier la traduction").clicked() {
                    android::presse_ecrire(&trad);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profils_de_manette() {
        let base = profil_de_base("x", 1);
        assert_eq!(base.touches.len(), BOUTONS.len());
        assert_eq!(base.touches[4], "espace");
        let mut v = vec![base.clone(), profil_de_base("y", 2)];
        v[1].jeu = "Minecraft".into();
        assert_eq!(profil_pour(&v, "Minecraft Launcher"), Some(1));
        assert_eq!(profil_pour(&v, "Fortnite"), None);
        assert_eq!(libelle_touche("w"), "Z");
        assert_eq!(libelle_touche("espace"), "espace");
        assert!(TOUCHES.iter().all(|t| !t.is_empty()));
    }

    #[test]
    fn rotation() {
        // 3×2 → 2×3, un quart de tour vers la droite
        let px = [1, 2, 3, 4, 5, 6];
        assert_eq!(tourner(3, 2, &px, 1), vec![4, 1, 5, 2, 6, 3]);
        let rgb: Vec<u8> = (0..18).collect();
        assert_eq!(tourner(3, 2, &rgb, 3).len(), 18);
        assert!(BOUTONS.iter().all(|b| !b.1.is_empty()));
    }
}

// ===================================================================================================
// Télécommande TV : piloter le PC branché à la télé (Netflix, YouTube…)
// ===================================================================================================

pub const SITES_TV: &[(&str, &str)] = &[
    ("▶ YouTube", "https://www.youtube.com"),
    ("🎬 Netflix", "https://www.netflix.com"),
    ("📦 Prime Video", "https://www.primevideo.com"),
    ("🏰 Disney+", "https://www.disneyplus.com"),
    ("🟣 Twitch", "https://www.twitch.tv"),
    ("🍥 Crunchyroll", "https://www.crunchyroll.com"),
    ("🎵 Spotify", "https://open.spotify.com"),
];

pub struct TeleTv {
    envoi: Option<Envoi>,
    e: Etat,
}

impl TeleTv {
    pub fn new() -> Self {
        TeleTv { envoi: None, e: Arc::default() }
    }

    fn touche(&self, t: &str) {
        if let Some(e) = &self.envoi {
            e.envoyer(&[format!("k {t}")]);
            android::vibrer(15);
        }
    }

    fn cmd(&self, p: &Pc, c: &str) {
        let (p, c) = (p.clone(), c.to_string());
        android::vibrer(15);
        en_fond(&self.e, "", move || p.post(&c, b""));
    }
}

impl Outil for TeleTv {
    fn nom(&self) -> &'static str {
        "📺 Télécommande TV"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        if self.envoi.as_ref().is_none_or(|e| e.ip != p.ip) {
            self.envoi = Envoi::new(&p);
        }
        ui.weak("Pour le PC branché à la télé : ouvre un site, puis pilote la lecture depuis le canapé.");
        ui.horizontal_wrapped(|ui| {
            for (n, u) in SITES_TV {
                if ui.add(egui::Button::new(RichText::new(*n).size(17.0)).min_size(Vec2::new(110.0, 48.0))).clicked() {
                    self.cmd(&p, &format!("/cmd/ouvrir?texte={}", encoder(u)));
                }
            }
        });
        ui.separator();
        let l = (ui.available_width() / 3.0 - 10.0).min(130.0);
        let b = |ui: &mut egui::Ui, t: &str| ui.add(egui::Button::new(RichText::new(t).size(26.0)).min_size(Vec2::new(l, 64.0))).clicked();
        ui.horizontal(|ui| {
            ui.add_space(l + 10.0);
            if b(ui, "⬆") {
                self.touche("haut");
            }
        });
        ui.horizontal(|ui| {
            if b(ui, "⬅") {
                self.touche("gauche");
            }
            if b(ui, "OK") {
                self.touche("entree");
            }
            if b(ui, "➡") {
                self.touche("droite");
            }
        });
        ui.horizontal(|ui| {
            if b(ui, "↩") {
                self.touche("echap");
            }
            if b(ui, "⬇") {
                self.touche("bas");
            }
            if b(ui, "⛶") {
                self.touche("f");
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if b(ui, "⏪") {
                self.touche("gauche");
            }
            if b(ui, "⏯") {
                self.cmd(&p, "/cmd/lecture");
            }
            if b(ui, "⏩") {
                self.touche("droite");
            }
        });
        ui.horizontal(|ui| {
            if b(ui, "🔉") {
                self.cmd(&p, "/cmd/vol_moins");
            }
            if b(ui, "🔇") {
                self.cmd(&p, "/cmd/muet");
            }
            if b(ui, "🔊") {
                self.cmd(&p, "/cmd/vol_plus");
            }
        });
        etat_ui(ui, &self.e);
        ui.weak("⛶ = plein écran (touche F) · ⏪ ⏩ = reculer / avancer de quelques secondes. Pour taper une recherche : outil Souris et clavier.");
    }
}

// ===================================================================================================
// 33 · Écran du PC en grand (second écran)
// ===================================================================================================

/// Position (0..1) sur l'écran du PC d'un point de l'image affichée, avec zoom et décalage.
pub fn point_pc(pos: Pos2, zone: egui::Rect, zoom: f32, decalage: Vec2) -> (f32, f32) {
    // l'image fait zone × zoom, décalée de `decalage` (en pixels d'écran)
    let x = (pos.x - zone.min.x - decalage.x) / (zone.width() * zoom);
    let y = (pos.y - zone.min.y - decalage.y) / (zone.height() * zoom);
    (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0))
}

pub struct EcranPc {
    envoi: Option<Envoi>,
    image: Arc<Mutex<Option<(u64, image::RgbImage)>>>,
    texture: Option<(u64, egui::TextureHandle)>,
    fil: Arc<AtomicBool>,
    vu: Instant,
    zoom: f32,
    decalage: Vec2,
    appui: Option<(Instant, Pos2)>,
    long_fait: bool,
    qualite: u32,
}

impl EcranPc {
    pub fn new() -> Self {
        EcranPc { envoi: None, image: Arc::default(), texture: None, fil: Arc::default(), vu: Instant::now(), zoom: 1.0, decalage: Vec2::ZERO, appui: None, long_fait: false, qualite: 1280 }
    }

    fn lancer(&self, p: Pc) {
        if self.fil.swap(true, Relaxed) {
            return;
        }
        let (img, fil, l) = (self.image.clone(), self.fil.clone(), self.qualite);
        std::thread::spawn(move || {
            let mut n = 0u64;
            while fil.load(Relaxed) {
                let debut = Instant::now();
                if let Ok(r) = ureq::get(&p.url(&format!("/api/ecran?l={l}"))).timeout(Duration::from_secs(5)).call() {
                    let mut b = vec![];
                    if std::io::Read::read_to_end(&mut r.into_reader(), &mut b).is_ok() {
                        if let Ok(i) = image::load_from_memory(&b) {
                            n += 1;
                            *img.lock().unwrap() = Some((n, i.to_rgb8()));
                        }
                    }
                }
                // jusqu'à ~8 images par seconde
                std::thread::sleep(Duration::from_millis(125).saturating_sub(debut.elapsed()));
            }
        });
    }
}

impl Outil for EcranPc {
    fn nom(&self) -> &'static str {
        "🖥 Écran du PC en grand"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.elapsed() > Duration::from_secs(1) {
            self.fil.store(false, Relaxed);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        if self.envoi.as_ref().is_none_or(|e| e.ip != p.ip) {
            self.envoi = Envoi::new(&p);
        }
        self.lancer(p);
        ui.horizontal_wrapped(|ui| {
            ui.weak("Touche = clic · appui long = clic droit · deux doigts = zoom et déplacement");
            if ui.small_button("🔍 1:1").clicked() {
                self.zoom = 1.0;
                self.decalage = Vec2::ZERO;
            }
        });
        if let Some((n, i)) = self.image.lock().unwrap().clone() {
            if self.texture.as_ref().is_none_or(|(m, _)| *m != n) {
                let c = egui::ColorImage::from_rgb([i.width() as usize, i.height() as usize], i.as_raw());
                match &mut self.texture {
                    Some((m, t)) => {
                        t.set(c, egui::TextureOptions::LINEAR);
                        *m = n;
                    }
                    None => self.texture = Some((n, ui.ctx().load_texture("ecran_pc", c, egui::TextureOptions::LINEAR))),
                }
            }
        }
        let Some((_, t)) = &self.texture else {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        };
        let s = t.size_vec2();
        let l = ui.available_width();
        let h = (l * s.y / s.x).min(ui.available_height());
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(h * s.x / s.y, h), egui::Sense::click_and_drag());
        // zoom et déplacement à deux doigts
        if let Some(mt) = ui.input(|i| i.multi_touch()) {
            let avant = self.zoom;
            self.zoom = (self.zoom * mt.zoom_delta).clamp(1.0, 5.0);
            let c = mt.center_pos - zone.min;
            // garder le point sous les doigts au même endroit
            self.decalage = c - (c - self.decalage) * (self.zoom / avant) + mt.translation_delta;
            self.appui = None;
            self.long_fait = true;
        }
        let max = Vec2::new(zone.width() * (self.zoom - 1.0), zone.height() * (self.zoom - 1.0));
        self.decalage = Vec2::new(self.decalage.x.clamp(-max.x, 0.0), self.decalage.y.clamp(-max.y, 0.0));
        let peint = ui.painter_at(zone);
        let img = egui::Rect::from_min_size(zone.min + self.decalage, zone.size() * self.zoom);
        peint.image(t.id(), img, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        let envoyer = |e: &Option<Envoi>, c: Vec<String>| {
            if let Some(e) = e {
                e.envoyer(&c);
            }
        };
        // un doigt : clic (court) ou clic droit (long)
        if rep.is_pointer_button_down_on() && ui.input(|i| i.multi_touch().is_none()) {
            if let Some(pos) = rep.interact_pointer_pos() {
                if self.appui.is_none() && !self.long_fait {
                    self.appui = Some((Instant::now(), pos));
                }
                if let Some((t0, p0)) = self.appui {
                    if !self.long_fait && t0.elapsed() > Duration::from_millis(600) && p0.distance(pos) < 20.0 {
                        let (x, y) = point_pc(p0, zone, self.zoom, self.decalage);
                        envoyer(&self.envoi, vec![format!("a {x:.4} {y:.4}"), "c d".into()]);
                        android::vibrer(30);
                        self.long_fait = true;
                    }
                }
            }
        } else {
            if let Some((t0, p0)) = self.appui.take() {
                if !self.long_fait && t0.elapsed() < Duration::from_millis(600) {
                    let (x, y) = point_pc(p0, zone, self.zoom, self.decalage);
                    envoyer(&self.envoi, vec![format!("a {x:.4} {y:.4}"), "c g".into()]);
                    android::vibrer(10);
                }
            }
            if !ui.input(|i| i.any_touches()) {
                self.long_fait = false;
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }
}

#[cfg(test)]
mod tests_ecran {
    use super::*;

    #[test]
    fn point_sur_le_pc() {
        let zone = egui::Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 100.0));
        assert_eq!(point_pc(Pos2::new(110.0, 70.0), zone, 1.0, Vec2::ZERO), (0.5, 0.5));
        // zoom ×2 sur le coin en haut à gauche : le centre de la zone montre le point (0,25 ; 0,25)
        let (x, y) = point_pc(Pos2::new(110.0, 70.0), zone, 2.0, Vec2::ZERO);
        assert!((x - 0.25).abs() < 1e-5 && (y - 0.25).abs() < 1e-5);
        let (x, _) = point_pc(Pos2::new(110.0, 70.0), zone, 2.0, Vec2::new(-200.0, 0.0));
        assert!((x - 0.75).abs() < 1e-5);
    }
}
