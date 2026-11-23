//! Outils qui utilisent le matériel du téléphone : lampe (12), boussole et niveau (11), sonomètre (14),
//! podomètre et chrono de sport (15), scanner de QR codes (13).
use crate::android::{self, Camera, Capteurs, Micro};
use crate::commun::*;
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Arrête le matériel quand l'outil n'est plus à l'écran.
struct Vu(Instant);

impl Vu {
    fn maintenant(&mut self) {
        self.0 = Instant::now();
    }
    fn cache(&self) -> bool {
        self.0.elapsed() > Duration::from_millis(800)
    }
}

// ---------------------------------------------------------------------------------------------------
// 12 · Lampe torche
// ---------------------------------------------------------------------------------------------------

pub struct Lampe {
    allumee: bool,
    mode: Arc<AtomicU32>, // 0 fixe, 1 stroboscope, 2 SOS
    vitesse: Arc<AtomicU32>,
    fil: bool,
    erreur: String,
}

impl Lampe {
    pub fn new() -> Self {
        Lampe { allumee: false, mode: Arc::default(), vitesse: Arc::new(AtomicU32::new(5)), fil: false, erreur: String::new() }
    }

    fn lancer_fil(&mut self) {
        if self.fil {
            return;
        }
        self.fil = true;
        let (m, v) = (self.mode.clone(), self.vitesse.clone());
        std::thread::spawn(move || {
            // SOS : ... --- ...
            let sos: Vec<(bool, u64)> = [1, 1, 1, 3, 3, 3, 1, 1, 1].iter().enumerate().flat_map(|(k, d)| [(true, d * 200), (false, if k % 3 == 2 { 600 } else { 200 })]).chain([(false, 1200)]).collect();
            let mut k = 0;
            loop {
                match m.load(Relaxed) {
                    1 => {
                        let p = 1000 / (v.load(Relaxed).max(1) as u64 * 2);
                        let _ = android::lampe(true);
                        std::thread::sleep(Duration::from_millis(p));
                        let _ = android::lampe(false);
                        std::thread::sleep(Duration::from_millis(p));
                    }
                    2 => {
                        let (on, d) = sos[k % sos.len()];
                        let _ = android::lampe(on);
                        std::thread::sleep(Duration::from_millis(d));
                        k += 1;
                    }
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        });
    }
}

impl Outil for Lampe {
    fn nom(&self) -> &'static str {
        "🔦 Lampe torche"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let t = ui.available_width().min(260.0);
        ui.vertical_centered(|ui| {
            let (r, rep) = ui.allocate_exact_size(Vec2::splat(t), egui::Sense::click());
            let c = if self.allumee { Color32::from_rgb(255, 230, 120) } else { Color32::from_gray(60) };
            ui.painter().circle_filled(r.center(), t / 2.0, c);
            ui.painter().text(r.center(), Align2::CENTER_CENTER, if self.allumee { "ALLUMÉE" } else { "ÉTEINTE" }, FontId::proportional(28.0), if self.allumee { Color32::BLACK } else { Color32::WHITE });
            if rep.clicked() {
                self.allumee = !self.allumee;
                self.mode.store(0, Relaxed);
                self.erreur = android::lampe(self.allumee).err().unwrap_or_default();
                android::vibrer(20);
            }
        });
        if !self.erreur.is_empty() {
            ui.colored_label(ROUGE, &self.erreur);
        }
        ui.separator();
        let mode = self.mode.load(Relaxed);
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(mode == 1, RichText::new("⚡ Stroboscope").size(18.0)).clicked() {
                self.lancer_fil();
                self.mode.store(if mode == 1 { 0 } else { 1 }, Relaxed);
                self.allumee = false;
                let _ = android::lampe(false);
            }
            if ui.selectable_label(mode == 2, RichText::new("🆘 SOS").size(18.0)).clicked() {
                self.lancer_fil();
                self.mode.store(if mode == 2 { 0 } else { 2 }, Relaxed);
                self.allumee = false;
                let _ = android::lampe(false);
            }
        });
        if mode == 1 {
            let mut v = self.vitesse.load(Relaxed);
            if ui.add(egui::Slider::new(&mut v, 1..=15).text("éclairs par seconde")).changed() {
                self.vitesse.store(v, Relaxed);
            }
        }
        if mode != 0 && ui.button("⏹ Arrêter").clicked() {
            self.mode.store(0, Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 11 · Boussole et niveau à bulle
// ---------------------------------------------------------------------------------------------------

pub struct Boussole {
    c: Option<Capteurs>,
    vu: Vu,
    lisse: f32,
    niveau: bool,
}

impl Boussole {
    pub fn new() -> Self {
        Boussole { c: None, vu: Vu(Instant::now()), lisse: 0.0, niveau: false }
    }
}

impl Outil for Boussole {
    fn nom(&self) -> &'static str {
        "🧭 Boussole et niveau"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.cache() {
            self.c = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu.maintenant();
        let c = self.c.get_or_insert_with(|| Capteurs::demarrer(true, true, false));
        let m = *c.m.lock().unwrap();
        ui.ctx().request_repaint_after(Duration::from_millis(33));
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.niveau, false, RichText::new("🧭 Boussole").size(18.0));
            ui.selectable_value(&mut self.niveau, true, RichText::new("🫧 Niveau").size(18.0));
        });
        if !m.recu {
            ui.weak("En attente des capteurs…");
            return;
        }
        let t = ui.available_width().min(340.0);
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), t), egui::Sense::hover());
        let centre = r.center();
        let p = ui.painter();
        let rayon = t / 2.0 - 8.0;
        if !self.niveau {
            let Some(cap) = android::cap(m.acc, m.mag) else {
                ui.weak("Tiens le téléphone à plat, loin des objets en métal.");
                return;
            };
            // lissage (sans sauter de 359° à 0°)
            let mut d = cap - self.lisse;
            if d > 180.0 {
                d -= 360.0;
            }
            if d < -180.0 {
                d += 360.0;
            }
            self.lisse = (self.lisse + d * 0.15 + 360.0) % 360.0;
            p.circle_filled(centre, rayon, ui.visuals().extreme_bg_color);
            p.circle_stroke(centre, rayon, Stroke::new(3.0_f32, Color32::GRAY));
            for (k, n) in ["N", "E", "S", "O"].iter().enumerate() {
                let a = (k as f32 * 90.0 - self.lisse).to_radians();
                let pos = centre + Vec2::new(a.sin(), -a.cos()) * (rayon - 24.0);
                p.text(pos, Align2::CENTER_CENTER, *n, FontId::proportional(26.0), if k == 0 { ROUGE } else { Color32::WHITE });
            }
            for k in 0..36 {
                let a = (k as f32 * 10.0 - self.lisse).to_radians();
                let dir = Vec2::new(a.sin(), -a.cos());
                p.line_segment([centre + dir * (rayon - if k % 9 == 0 { 0.0 } else { 8.0 }), centre + dir * rayon], Stroke::new(2.0_f32, Color32::GRAY));
            }
            p.line_segment([centre, centre - Vec2::new(0.0, rayon - 40.0)], Stroke::new(4.0_f32, ROSE));
            let dirs = ["Nord", "Nord-Est", "Est", "Sud-Est", "Sud", "Sud-Ouest", "Ouest", "Nord-Ouest"];
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("{:.0}° {}", self.lisse, dirs[((self.lisse + 22.5) / 45.0) as usize % 8])).size(30.0).strong());
            });
            ui.weak("Si la boussole se trompe, fais un grand 8 en l'air avec le téléphone pour la recalibrer.");
        } else {
            let g = (m.acc[0].powi(2) + m.acc[1].powi(2) + m.acc[2].powi(2)).sqrt().max(0.1);
            let (ax, ay) = ((m.acc[0] / g).asin().to_degrees(), (m.acc[1] / g).asin().to_degrees());
            p.circle_filled(centre, rayon, Color32::from_rgb(40, 70, 40));
            p.circle_stroke(centre, rayon * 0.15, Stroke::new(2.0_f32, Color32::WHITE));
            let b = centre + Vec2::new(-ax, ay) / 45.0 * rayon;
            let a_plat = ax.abs() < 1.0 && ay.abs() < 1.0;
            p.circle_filled(b, rayon * 0.12, if a_plat { VERT } else { Color32::from_rgb(200, 240, 120) });
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if a_plat { "✔ D'aplomb".to_string() } else { format!("{ax:+.1}°  ·  {ay:+.1}°") }).size(28.0).strong());
            });
            ui.weak("Pose le téléphone sur une étagère ou un cadre : la bulle doit être au centre.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 14 · Sonomètre
// ---------------------------------------------------------------------------------------------------

pub struct Sonometre {
    micro: Option<Micro>,
    vu: Vu,
    max: f32,
}

impl Sonometre {
    pub fn new() -> Self {
        Sonometre { micro: None, vu: Vu(Instant::now()), max: 0.0 }
    }
}

pub fn ambiance(db: f32) -> &'static str {
    match db as u32 {
        0..=30 => "très calme (chambre la nuit)",
        31..=45 => "calme (bibliothèque)",
        46..=60 => "conversation",
        61..=75 => "bruyant (cantine, rue)",
        76..=90 => "très bruyant : protège tes oreilles si ça dure",
        _ => "dangereux pour l'audition (concert, sirène)",
    }
}

impl Outil for Sonometre {
    fn nom(&self) -> &'static str {
        "📢 Sonomètre"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.cache() {
            self.micro = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu.maintenant();
        if !android::a_permission("android.permission.RECORD_AUDIO") {
            ui.label("Le sonomètre a besoin du micro (rien n'est enregistré).");
            if gros_bouton(ui, "🎙 Autoriser le micro") {
                android::demander(&["android.permission.RECORD_AUDIO"]);
            }
            ui.ctx().request_repaint_after(Duration::from_secs(1));
            return;
        }
        let m = self.micro.get_or_insert_with(Micro::demarrer);
        let e = m.erreur.lock().unwrap().clone();
        if !e.is_empty() {
            ui.colored_label(ROUGE, e);
        }
        let v = m.niveau.lock().unwrap().clone();
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        let db = v.last().copied().unwrap_or(0.0);
        self.max = self.max.max(db);
        let moy = if v.is_empty() { 0.0 } else { v.iter().sum::<f32>() / v.len() as f32 };
        let c = if db > 85.0 { ROUGE } else if db > 70.0 { ORANGE } else { VERT };
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("{db:.0} dB")).size(64.0).strong().color(c));
            ui.label(RichText::new(ambiance(db)).size(18.0));
        });
        ui.add(egui::ProgressBar::new(db / 120.0).fill(c));
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 140.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 8.0, ui.visuals().extreme_bg_color);
        let pts: Vec<egui::Pos2> = v.iter().enumerate().map(|(k, x)| egui::pos2(r.left() + k as f32 / 299.0 * r.width(), r.bottom() - x / 120.0 * r.height())).collect();
        ui.painter().add(egui::Shape::line(pts, Stroke::new(2.0_f32, ROSE)));
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("max {:.0} dB · moyenne {:.0} dB", self.max, moy));
            if ui.small_button("Remettre à zéro").clicked() {
                self.max = 0.0;
                m.niveau.lock().unwrap().clear();
            }
        });
        ui.weak("Valeurs approchées : le micro d'un téléphone n'est pas un appareil de mesure.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 15 · Podomètre et chrono de sport
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Pas {
    jour: String,
    base: f32,
    aujourdhui: u32,
    objectif: u32,
    historique: Vec<(String, u32)>,
}

pub struct Sport {
    c: Option<Capteurs>,
    vu: Vu,
    p: Pas,
    onglet: usize,
    debut: Option<Instant>,
    cumul: Duration,
    tours: Vec<Duration>,
    // fractionné
    effort: u32,
    repos: u32,
    series: u32,
    frac: Option<(Instant, u32)>,
    dernier_bip: i64,
}

impl Sport {
    pub fn new() -> Self {
        let mut p: Pas = charger("podometre");
        if p.objectif == 0 {
            p.objectif = 10_000;
        }
        Sport { c: None, vu: Vu(Instant::now()), p, onglet: 0, debut: None, cumul: Duration::ZERO, tours: vec![], effort: 30, repos: 15, series: 8, frac: None, dernier_bip: -1 }
    }

    fn ecoule(&self) -> Duration {
        self.cumul + self.debut.map_or(Duration::ZERO, |d| d.elapsed())
    }
}

fn chrono(d: Duration) -> String {
    let c = d.as_millis() / 10;
    format!("{:02}:{:02}.{:02}", c / 6000, c / 100 % 60, c % 100)
}

impl Outil for Sport {
    fn nom(&self) -> &'static str {
        "👟 Sport : pas et chrono"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        // le podomètre reste actif tant que l'outil est ouvert ou que le chrono tourne
        if self.vu.cache() && self.debut.is_none() && self.frac.is_none() {
            self.c = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu.maintenant();
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        onglets(ui, &mut self.onglet, &["👣 Pas", "⏱ Chrono", "🔁 Fractionné"]);
        match self.onglet {
            0 => {
                if android::version_android() >= 29 && !android::a_permission("android.permission.ACTIVITY_RECOGNITION") {
                    ui.label("Le podomètre a besoin de l'autorisation « Activité physique ».");
                    if gros_bouton(ui, "👣 Autoriser") {
                        android::demander(&["android.permission.ACTIVITY_RECOGNITION"]);
                    }
                    return;
                }
                let c = self.c.get_or_insert_with(|| Capteurs::demarrer(false, false, true));
                let m = *c.m.lock().unwrap();
                let jour = crate::minecraft::horodatage()[..8].to_string();
                if let Some(total) = m.pas {
                    if self.p.jour != jour {
                        if !self.p.jour.is_empty() {
                            self.p.historique.push((self.p.jour.clone(), self.p.aujourdhui));
                            let n = self.p.historique.len();
                            if n > 30 {
                                self.p.historique.drain(..n - 30);
                            }
                        }
                        self.p.jour = jour;
                        self.p.base = total;
                        self.p.aujourdhui = 0;
                    }
                    // le compteur repart à zéro quand le téléphone redémarre
                    if total < self.p.base {
                        self.p.base = total - self.p.aujourdhui as f32;
                    }
                    let n = (total - self.p.base).max(0.0) as u32;
                    if n != self.p.aujourdhui {
                        self.p.aujourdhui = n;
                        enregistrer("podometre", &self.p);
                    }
                }
                let n = self.p.aujourdhui;
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("{n}")).size(64.0).strong().color(ROSE));
                    ui.label(RichText::new("pas aujourd'hui").size(18.0));
                    ui.label(format!("≈ {:.2} km · {} kcal", n as f32 * 0.75 / 1000.0, n * 4 / 100));
                });
                ui.add(egui::ProgressBar::new((n as f32 / self.p.objectif as f32).min(1.0)).fill(VERT).text(format!("objectif {}", self.p.objectif)));
                ui.add(egui::Slider::new(&mut self.p.objectif, 2000..=20000).step_by(500.0).text("objectif"));
                if m.pas.is_none() {
                    ui.weak("Les pas comptent quand tu marches avec le téléphone (le capteur peut mettre un peu de temps à démarrer).");
                }
                if !self.p.historique.is_empty() {
                    ui.label(RichText::new("Derniers jours").strong());
                    for (j, p) in self.p.historique.iter().rev().take(7) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("{}/{}", &j[6..8], &j[4..6]));
                            ui.add(egui::ProgressBar::new((*p as f32 / self.p.objectif as f32).min(1.0)).text(p.to_string()));
                        });
                    }
                }
            }
            1 => {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(chrono(self.ecoule())).size(56.0).monospace().strong());
                });
                ui.horizontal_wrapped(|ui| {
                    let marche = self.debut.is_some();
                    if ui.add_sized([ui.available_width() / 2.0 - 6.0, 60.0], egui::Button::new(RichText::new(if marche { "⏸ Pause" } else { "▶ Départ" }).size(22.0))).clicked() {
                        match self.debut.take() {
                            Some(d) => self.cumul += d.elapsed(),
                            None => self.debut = Some(Instant::now()),
                        }
                        android::vibrer(30);
                    }
                    if marche {
                        if ui.add_sized([ui.available_width(), 60.0], egui::Button::new(RichText::new("🏁 Tour").size(22.0))).clicked() {
                            self.tours.push(self.ecoule());
                            android::vibrer(30);
                        }
                    } else if ui.add_sized([ui.available_width(), 60.0], egui::Button::new(RichText::new("↺ Zéro").size(22.0))).clicked() {
                        self.cumul = Duration::ZERO;
                        self.tours.clear();
                    }
                });
                let mut prec = Duration::ZERO;
                for (k, t) in self.tours.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Tour {}", k + 1));
                        ui.strong(chrono(*t - prec));
                        ui.weak(chrono(*t));
                    });
                    prec = *t;
                }
            }
            _ => {
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::DragValue::new(&mut self.effort).range(5..=600).suffix(" s effort"));
                    ui.add(egui::DragValue::new(&mut self.repos).range(0..=600).suffix(" s repos"));
                    ui.add(egui::DragValue::new(&mut self.series).range(1..=50).suffix(" séries"));
                });
                let cycle = (self.effort + self.repos).max(1);
                match self.frac {
                    None => {
                        if gros_bouton(ui, "▶ Démarrer") {
                            self.frac = Some((Instant::now(), 0));
                            self.dernier_bip = -1;
                        }
                    }
                    Some((d, _)) => {
                        let s = d.elapsed().as_secs() as u32;
                        let serie = s / cycle;
                        if serie >= self.series {
                            ui.label(RichText::new("🎉 Terminé, bravo !").size(28.0).strong());
                            if self.dernier_bip != -2 {
                                android::vibrer(800);
                                self.dernier_bip = -2;
                            }
                            if gros_bouton(ui, "Recommencer") {
                                self.frac = None;
                            }
                        } else {
                            let dans = s % cycle;
                            let effort = dans < self.effort;
                            let reste = if effort { self.effort - dans } else { cycle - dans };
                            // vibration à chaque changement de phase
                            let phase = (serie * 2 + if effort { 0 } else { 1 }) as i64;
                            if phase != self.dernier_bip {
                                android::vibrer(if effort { 400 } else { 150 });
                                self.dernier_bip = phase;
                            }
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new(if effort { "💪 EFFORT" } else { "😮‍💨 REPOS" }).size(34.0).strong().color(if effort { ROUGE } else { VERT }));
                                ui.label(RichText::new(format!("{reste}")).size(80.0).strong());
                                ui.label(format!("série {} / {}", serie + 1, self.series));
                            });
                            if gros_bouton(ui, "⏹ Arrêter") {
                                self.frac = None;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 13 · Scanner de QR codes
// ---------------------------------------------------------------------------------------------------

pub struct ScannerQr {
    cam: Option<Camera>,
    vu: Vu,
    texture: Option<egui::TextureHandle>,
    resultat: Arc<Mutex<Option<String>>>,
    decode: Arc<AtomicBool>,
    historique: Vec<String>,
    message: String,
}

impl ScannerQr {
    pub fn new() -> Self {
        ScannerQr { cam: None, vu: Vu(Instant::now()), texture: None, resultat: Arc::default(), decode: Arc::default(), historique: charger("qr_historique"), message: String::new() }
    }
}

impl Outil for ScannerQr {
    fn nom(&self) -> &'static str {
        "🔳 Scanner QR"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.cache() {
            self.cam = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu.maintenant();
        if !android::a_permission("android.permission.CAMERA") {
            ui.label("Le scanner a besoin de l'appareil photo.");
            if gros_bouton(ui, "📷 Autoriser l'appareil photo") {
                android::demander(&["android.permission.CAMERA"]);
            }
            ui.ctx().request_repaint_after(Duration::from_secs(1));
            return;
        }
        // un QR code vient d'être lu
        if let Some(t) = self.resultat.lock().unwrap().take() {
            android::vibrer(80);
            self.historique.retain(|x| *x != t);
            self.historique.insert(0, t.clone());
            self.historique.truncate(20);
            enregistrer("qr_historique", &self.historique);
            self.cam = None; // on arrête l'appareil photo une fois le code lu
            self.message = if crate::monpc::lire_adresse(&t).is_some() && crate::monpc::MonPc::associer_adresse(&t) { "✔ PC associé : ouvre « Mon PC »".into() } else { String::new() };
        }
        let scanner = self.cam.is_some();
        if !scanner {
            if gros_bouton(ui, "📷 Scanner un QR code") {
                self.cam = Some(Camera::demarrer());
                self.message.clear();
            }
        } else {
            let cam = self.cam.as_ref().unwrap();
            let e = cam.erreur.lock().unwrap().clone();
            if !e.is_empty() {
                ui.colored_label(ROUGE, e);
            }
            let img = cam.image.lock().unwrap().clone();
            if let Some((l, h, y)) = img {
                // aperçu tourné d'un quart de tour (le capteur est couché)
                let mut px = vec![0u8; l * h];
                for yy in 0..h {
                    for x in 0..l {
                        px[x * h + (h - 1 - yy)] = y[yy * l + x];
                    }
                }
                let ci = egui::ColorImage::from_gray([h, l], &px);
                match &mut self.texture {
                    Some(t) => t.set(ci, Default::default()),
                    None => self.texture = Some(ui.ctx().load_texture("camera", ci, Default::default())),
                }
                if !self.decode.swap(true, Relaxed) {
                    let (d, r) = (self.decode.clone(), self.resultat.clone());
                    std::thread::spawn(move || {
                        if let Some(t) = android::lire_qr(l, h, &y) {
                            *r.lock().unwrap() = Some(t);
                        }
                        d.store(false, Relaxed);
                    });
                }
            }
            if let Some(t) = &self.texture {
                let w = ui.available_width().min(360.0);
                let r = ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::new(w, w * 4.0 / 3.0))).rect;
                let cadre = egui::Rect::from_center_size(r.center(), Vec2::splat(w * 0.65));
                ui.painter().rect_stroke(cadre, 12.0, Stroke::new(3.0_f32, ROSE), egui::StrokeKind::Inside);
            }
            ui.weak("Vise le QR code : il est lu tout seul.");
            if ui.button("Arrêter").clicked() {
                self.cam = None;
            }
            ui.ctx().request_repaint_after(Duration::from_millis(80));
        }
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).strong().color(VERT));
        }
        for (k, t) in self.historique.clone().iter().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(t).size(if k == 0 { 18.0 } else { 15.0 }).strong());
                ui.horizontal_wrapped(|ui| {
                    if t.starts_with("http://") || t.starts_with("https://") {
                        if ui.button("🌐 Ouvrir").clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(t));
                        }
                    }
                    if let Some(w) = t.strip_prefix("WIFI:") {
                        let mdp = w.split(';').find_map(|x| x.strip_prefix("P:")).unwrap_or("");
                        ui.weak(format!("Wi-Fi · mot de passe : {mdp}"));
                    }
                    if ui.button("📋 Copier").clicked() {
                        android::presse_ecrire(t);
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesures() {
        assert_eq!(chrono(Duration::from_millis(61_230)), "01:01.23");
        assert_eq!(ambiance(55.0), "conversation");
        // à plat, l'écran vers le haut, le haut du téléphone vers le nord magnétique
        let c = android::cap([0.0, 0.0, 9.8], [0.0, 20.0, -40.0]).unwrap();
        assert!(c < 1.0 || c > 359.0, "{c}");
        let e = android::cap([0.0, 0.0, 9.8], [20.0, 0.0, -40.0]).unwrap();
        assert!((e - 270.0).abs() < 1.0, "{e}");
        assert!(android::decibels(&[0.5; 100]) > android::decibels(&[0.01; 100]));
    }
}
