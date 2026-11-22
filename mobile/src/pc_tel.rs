//! Le téléphone au service du PC : manette (13), souris et clavier (14), écran du PC (16),
//! webcam et micro (15), traduction avec l'appareil photo (12).
use crate::android::{self, Camera, Capteurs, MicroBrut};
use crate::commun::*;
use crate::jeux_tel::appuis;
use crate::monpc::{encoder, Pc};
use crate::outil::{charger, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Stroke, Vec2};
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

pub struct Manette {
    envoi: Option<Envoi>,
    doigts: HashMap<u64, Pos2>,
    appuyes: HashSet<usize>,
    preset: usize,
    volant: bool,
    capteurs: Option<Capteurs>,
    rappel: Instant,
}

impl Manette {
    pub fn new() -> Self {
        Manette { envoi: None, doigts: HashMap::new(), appuyes: HashSet::new(), preset: 0, volant: false, capteurs: None, rappel: Instant::now() }
    }

    fn touche(&self, k: usize) -> &'static str {
        if self.preset == 0 { BOUTONS[k].1 } else { BOUTONS[k].2 }
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
        // l'outil n'est plus à l'écran : on relâche tout
        if !self.appuyes.is_empty() && self.rappel.elapsed() > Duration::from_secs(1) {
            if let Some(e) = &self.envoi {
                let l: Vec<String> = self.appuyes.iter().map(|k| format!("k {} up", self.touche(*k))).collect();
                e.envoyer(&l);
            }
            self.appuyes.clear();
            self.capteurs = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(p) = pc() else {
            sans_pc(ui);
            return;
        };
        if self.envoi.as_ref().is_none_or(|e| e.ip != p.ip) {
            self.envoi = Envoi::new(&p);
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.preset, 0, "Jeux PC (ZQSD, Espace…)");
            ui.selectable_value(&mut self.preset, 1, "Flèches, Entrée");
            ui.checkbox(&mut self.volant, "🚗 Volant (incliner)");
        });
        ui.weak("Tourne le téléphone à l'horizontale. Les touches arrivent sur le PC comme un vrai clavier.");
        let h = (ui.ctx().screen_rect().height() * 0.62).max(260.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click_and_drag());
        appuis(ui, &mut self.doigts);
        let mut maintenant: HashSet<usize> = HashSet::new();
        let rayon = |k: usize| place(k).2 * zone.width().min(zone.height() * 1.6);
        let centre = |k: usize| zone.min + Vec2::new(place(k).0 * zone.width(), place(k).1 * zone.height());
        for d in self.doigts.values() {
            for k in 0..BOUTONS.len() {
                if centre(k).distance(*d) < rayon(k) * 1.25 {
                    maintenant.insert(k);
                }
            }
        }
        if self.volant {
            let c = self.capteurs.get_or_insert_with(|| Capteurs::demarrer(true, false, false));
            let a = c.m.lock().unwrap().acc;
            let pente = if zone.width() > zone.height() { a[1] } else { -a[0] };
            if pente > 2.0 {
                maintenant.insert(3);
            } else if pente < -2.0 {
                maintenant.insert(2);
            }
        } else {
            self.capteurs = None;
        }
        let mut cmds = vec![];
        for k in maintenant.difference(&self.appuyes) {
            cmds.push(format!("k {} down", self.touche(*k)));
            android::vibrer(12);
        }
        for k in self.appuyes.difference(&maintenant) {
            cmds.push(format!("k {} up", self.touche(*k)));
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
        let pt = ui.painter_at(zone);
        pt.rect_filled(zone, 18.0, Color32::from_rgb(28, 24, 40));
        for (k, (nom, ..)) in BOUTONS.iter().enumerate() {
            let on = self.appuyes.contains(&k);
            let c = match *nom {
                "A" => Color32::from_rgb(90, 200, 120),
                "B" => Color32::from_rgb(230, 80, 90),
                "X" => Color32::from_rgb(80, 150, 240),
                "Y" => Color32::from_rgb(240, 200, 70),
                _ => Color32::from_gray(90),
            };
            pt.circle_filled(centre(k), rayon(k), if on { c } else { c.gamma_multiply(0.45) });
            pt.circle_stroke(centre(k), rayon(k), Stroke::new(2.0_f32, Color32::from_white_alpha(60)));
            pt.text(centre(k), Align2::CENTER_CENTER, nom, FontId::proportional(rayon(k) * 0.7), Color32::WHITE);
        }
        ui.ctx().request_repaint();
        ui.weak(format!("A = {} · B = {} · X = {} · Y = {} · L = {} · R = {}", self.touche(4), self.touche(5), self.touche(6), self.touche(7), self.touche(8), self.touche(9)));
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
        ui.horizontal(|ui| {
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
            ui.horizontal(|ui| {
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
    fn rotation() {
        // 3×2 → 2×3, un quart de tour vers la droite
        let px = [1, 2, 3, 4, 5, 6];
        assert_eq!(tourner(3, 2, &px, 1), vec![4, 1, 5, 2, 6, 3]);
        let rgb: Vec<u8> = (0..18).collect();
        assert_eq!(tourner(3, 2, &rgb, 3).len(), 18);
        assert!(BOUTONS.iter().all(|b| !b.1.is_empty()));
    }
}
