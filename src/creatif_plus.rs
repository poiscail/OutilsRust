//! Créatif : piano sur le clavier (6), dessin symétrique (7), art ASCII (8), générateur de QR codes (17).
use crate::musique::{jouer_court, wav, TAUX};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Vec2};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::time::Instant;

// ---------------------------------------------------------------------------------------------------
// 6 · Piano sur le clavier
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Instrument {
    Piano,
    Orgue,
    Synthe,
    Bit8,
    Guitare,
}

/// Fréquence d'une note MIDI (69 = la 440 Hz).
pub fn frequence(midi: i32) -> f32 {
    440.0 * 2f32.powf((midi - 69) as f32 / 12.0)
}

/// Son d'une note.
pub fn son_note(midi: i32, inst: Instrument) -> Vec<f32> {
    let f = frequence(midi);
    let duree = match inst {
        Instrument::Orgue => 0.9,
        Instrument::Bit8 => 0.5,
        _ => 1.6,
    };
    let n = (duree * TAUX as f32) as usize;
    let mut v = Vec::with_capacity(n);
    // guitare : corde pincée (Karplus-Strong)
    if inst == Instrument::Guitare {
        let periode = (TAUX as f32 / f) as usize;
        let mut corde: Vec<f32> = (0..periode.max(2)).map(|k| ((k as u32).wrapping_mul(2654435761) % 2000) as f32 / 1000.0 - 1.0).collect();
        for i in 0..n {
            let a = corde[i % corde.len()];
            let b = corde[(i + 1) % corde.len()];
            let m = corde.len();
            corde[i % m] = (a + b) * 0.498;
            v.push(a * 0.5);
        }
        return v;
    }
    for i in 0..n {
        let t = i as f32 / TAUX as f32;
        let attaque = (t / 0.004).min(1.0);
        let s = match inst {
            Instrument::Piano => {
                let env = (-t * 3.2).exp();
                ((TAU * f * t).sin() + 0.45 * (TAU * 2.0 * f * t).sin() * (-t * 2.0).exp() + 0.2 * (TAU * 3.0 * f * t).sin() * (-t * 4.0).exp()) * env * 0.45
            }
            Instrument::Orgue => {
                let env = 1.0 - (t / duree).powi(4);
                ((TAU * f * t).sin() + 0.5 * (TAU * 2.0 * f * t).sin() + 0.3 * (TAU * 4.0 * f * t).sin()) * env * 0.3
            }
            Instrument::Synthe => {
                let p = (f * t).fract();
                let scie = 2.0 * p - 1.0;
                let env = (-t * 2.0).exp();
                (scie * 0.6 + (TAU * f * 1.005 * t).sin() * 0.4) * env * 0.35
            }
            _ => {
                let env = (-t * 4.0).exp();
                (if (f * t).fract() < 0.5 { 0.5 } else { -0.5 }) * env * 0.5
            }
        };
        v.push(s * attaque);
    }
    v
}

/// Touches du clavier (disposition AZERTY) → note : rangée du milieu = touches blanches, du haut = noires.
pub const TOUCHES_PIANO: [(egui::Key, &str, i32); 17] = [
    (egui::Key::Q, "Q", 0), (egui::Key::Z, "Z", 1), (egui::Key::S, "S", 2), (egui::Key::E, "E", 3), (egui::Key::D, "D", 4), (egui::Key::F, "F", 5),
    (egui::Key::T, "T", 6), (egui::Key::G, "G", 7), (egui::Key::Y, "Y", 8), (egui::Key::H, "H", 9), (egui::Key::U, "U", 10), (egui::Key::J, "J", 11),
    (egui::Key::K, "K", 12), (egui::Key::O, "O", 13), (egui::Key::L, "L", 14), (egui::Key::P, "P", 15), (egui::Key::M, "M", 16),
];

fn est_noire(n: i32) -> bool {
    matches!(n.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

fn nom_note(n: i32) -> String {
    const N: [&str; 12] = ["Do", "Do#", "Ré", "Ré#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si"];
    format!("{}{}", N[n.rem_euclid(12) as usize], n.div_euclid(12) - 1)
}

/// Mélange des notes enregistrées (temps en s, note MIDI, instrument).
pub fn rendre_morceau(notes: &[(f32, i32, Instrument)]) -> Vec<f32> {
    let fin = notes.iter().map(|n| n.0).fold(0.0f32, f32::max) + 2.0;
    let mut v = vec![0.0f32; (fin * TAUX as f32) as usize];
    for (t, m, i) in notes {
        let d = (*t * TAUX as f32) as usize;
        for (k, s) in son_note(*m, *i).into_iter().enumerate() {
            if let Some(x) = v.get_mut(d + k) {
                *x += s;
            }
        }
    }
    let pic = v.iter().fold(0.0f32, |a, x| a.max(x.abs()));
    if pic > 0.95 {
        for x in &mut v {
            *x *= 0.95 / pic;
        }
    }
    v
}

pub struct Piano {
    instrument: Instrument,
    octave: i32,
    allumees: Vec<(i32, Instant)>,
    enregistrement: Option<Instant>,
    morceau: Vec<(f32, i32, Instrument)>,
    message: String,
}

impl Piano {
    pub fn new() -> Self {
        Piano { instrument: Instrument::Piano, octave: 4, allumees: Vec::new(), enregistrement: None, morceau: Vec::new(), message: String::new() }
    }

    fn jouer(&mut self, midi: i32) {
        jouer_court(son_note(midi, self.instrument));
        self.allumees.push((midi, Instant::now()));
        if let Some(d) = self.enregistrement {
            self.morceau.push((d.elapsed().as_secs_f32(), midi, self.instrument));
        }
    }
}

impl Outil for Piano {
    fn nom(&self) -> &'static str {
        "🎹 Piano sur le clavier"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Piano");
            for (i, n) in [(Instrument::Piano, "🎹 Piano"), (Instrument::Orgue, "⛪ Orgue"), (Instrument::Synthe, "🎛 Synthé"), (Instrument::Bit8, "👾 8-bit"), (Instrument::Guitare, "🎸 Guitare")] {
                ui.selectable_value(&mut self.instrument, i, n);
            }
            ui.label("Octave");
            if ui.small_button("−").clicked() {
                self.octave = (self.octave - 1).max(1);
            }
            ui.label(self.octave.to_string());
            if ui.small_button("+").clicked() {
                self.octave = (self.octave + 1).min(7);
            }
        });
        ui.weak("Joue avec ton clavier : Q S D F G H J K L M pour les touches blanches, Z E T Y U O P pour les noires. Ou clique sur les touches.");
        let base = (self.octave + 1) * 12; // Do de l'octave choisie
        // clavier de l'ordinateur (pas pendant qu'on écrit dans un champ)
        if !ui.ctx().wants_keyboard_input() {
            for (k, _, d) in TOUCHES_PIANO {
                if ui.input(|i| i.key_pressed(k)) {
                    self.jouer(base + d);
                }
            }
        }
        // clavier dessiné : 17 demi-tons
        let blanches: Vec<i32> = (0..17).filter(|n| !est_noire(*n)).collect();
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(760.0), 200.0), Sense::click());
        let p = ui.painter_at(zone);
        let lb = zone.width() / blanches.len() as f32;
        let allume = |n: i32, a: &Vec<(i32, Instant)>| a.iter().any(|(m, t)| *m == base + n && t.elapsed().as_millis() < 250);
        let mut rects: Vec<(i32, Rect, bool)> = Vec::new();
        for (k, n) in blanches.iter().enumerate() {
            rects.push((*n, Rect::from_min_size(zone.min + Vec2::new(k as f32 * lb, 0.0), Vec2::new(lb - 2.0, zone.height())), false));
        }
        for n in 0..17 {
            if est_noire(n) {
                let gauche = blanches.iter().position(|b| *b == n - 1).unwrap_or(0) as f32;
                rects.push((n, Rect::from_min_size(zone.min + Vec2::new((gauche + 1.0) * lb - lb * 0.3, 0.0), Vec2::new(lb * 0.6, zone.height() * 0.6)), true));
            }
        }
        for (n, r, noire) in &rects {
            let on = allume(*n, &self.allumees);
            let c = match (noire, on) {
                (false, false) => Color32::from_gray(240),
                (true, false) => Color32::from_gray(25),
                (_, true) => Color32::from_rgb(120, 190, 255),
            };
            p.rect_filled(*r, 4.0, c);
            let lettre = TOUCHES_PIANO.iter().find(|t| t.2 == *n).map(|t| t.1).unwrap_or("");
            p.text(r.center_bottom() - Vec2::new(0.0, 14.0), egui::Align2::CENTER_CENTER, lettre, FontId::proportional(13.0), if *noire { Color32::WHITE } else { Color32::DARK_GRAY });
        }
        if rep.clicked() {
            if let Some(h) = rep.interact_pointer_pos() {
                // les noires sont dessus : on les teste d'abord
                if let Some((n, ..)) = rects.iter().rev().find(|(_, r, _)| r.contains(h)) {
                    let n = *n;
                    self.jouer(base + n);
                }
            }
        }
        if let Some((m, _)) = self.allumees.last() {
            ui.label(RichText::new(nom_note(*m)).size(20.0).strong());
        }
        self.allumees.retain(|(_, t)| t.elapsed().as_millis() < 400);
        ui.horizontal_wrapped(|ui| {
            match self.enregistrement {
                None => {
                    if ui.button("⏺ Enregistrer").clicked() {
                        self.morceau.clear();
                        self.enregistrement = Some(Instant::now());
                    }
                }
                Some(d) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("● {:.0} s", d.elapsed().as_secs_f32()));
                    if ui.button("⏹ Arrêter").clicked() {
                        self.enregistrement = None;
                    }
                }
            }
            if !self.morceau.is_empty() && self.enregistrement.is_none() {
                if ui.button("▶ Réécouter").clicked() {
                    jouer_court(rendre_morceau(&self.morceau));
                }
                if ui.button("💾 Enregistrer en WAV…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name("mon_morceau.wav").save_file() {
                        self.message = std::fs::write(&p, wav(&rendre_morceau(&self.morceau))).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                    }
                }
                ui.weak(format!("{} notes", self.morceau.len()));
            }
        });
        ui.label(&self.message);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
}

// ---------------------------------------------------------------------------------------------------
// 7 · Dessin symétrique (mandalas, kaléidoscope)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Trait {
    pub points: Vec<(f32, f32)>, // relatif au centre, en fraction du rayon
    pub couleur: [u8; 3],
    pub epaisseur: f32,
    pub arc_en_ciel: bool,
}

/// Les copies d'un point : `n` rotations, et leurs reflets si `miroir`.
pub fn copies(p: (f32, f32), n: u32, miroir: bool) -> Vec<(f32, f32)> {
    let mut v = Vec::new();
    for k in 0..n.max(1) {
        let a = TAU * k as f32 / n.max(1) as f32;
        let (s, c) = a.sin_cos();
        v.push((p.0 * c - p.1 * s, p.0 * s + p.1 * c));
        if miroir {
            v.push((-p.0 * c - p.1 * s, -p.0 * s + p.1 * c));
        }
    }
    v
}

fn teinte(h: f32) -> [u8; 3] {
    let c = Color32::from(egui::ecolor::Hsva::new(h.rem_euclid(1.0), 0.85, 1.0, 1.0));
    [c.r(), c.g(), c.b()]
}

pub struct Symetrie {
    traits: Vec<Trait>,
    branches: u32,
    miroir: bool,
    couleur: [u8; 3],
    epaisseur: f32,
    arc_en_ciel: bool,
    fond: [u8; 3],
    en_cours: Option<Trait>,
    message: String,
}

impl Symetrie {
    pub fn new() -> Self {
        Symetrie { traits: charger("symetrie"), branches: 8, miroir: true, couleur: [120, 200, 255], epaisseur: 3.0, arc_en_ciel: true, fond: [12, 12, 24], en_cours: None, message: String::new() }
    }

    /// Image finale (carrée) des traits.
    fn image(&self, cote: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(cote, cote, Rgba([self.fond[0], self.fond[1], self.fond[2], 255]));
        let c = cote as f32 / 2.0;
        let echelle = cote as f32 / 600.0;
        for t in &self.traits {
            let n = t.points.len().max(1);
            for (k, w) in t.points.windows(2).enumerate() {
                let col = if t.arc_en_ciel { teinte(k as f32 / n as f32) } else { t.couleur };
                for (a, b) in copies(w[0], self.branches, self.miroir).into_iter().zip(copies(w[1], self.branches, self.miroir)) {
                    let (ax, ay, bx, by) = (c + a.0 * c, c + a.1 * c, c + b.0 * c, c + b.1 * c);
                    let pas = (((bx - ax).powi(2) + (by - ay).powi(2)).sqrt() / 1.5).max(1.0) as usize;
                    for s in 0..=pas {
                        let f = s as f32 / pas as f32;
                        let r = t.epaisseur * echelle;
                        crate::atelier::ellipse(&mut img, ax + (bx - ax) * f, ay + (by - ay) * f, r, r, Rgba([col[0], col[1], col[2], 255]));
                    }
                }
            }
        }
        img
    }
}

impl Outil for Symetrie {
    fn nom(&self) -> &'static str {
        "🌸 Dessin symétrique"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.traits.len();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Dessin symétrique");
            ui.label("Branches");
            ui.add(egui::Slider::new(&mut self.branches, 1..=24));
            ui.checkbox(&mut self.miroir, "Miroir");
            ui.checkbox(&mut self.arc_en_ciel, "🌈 Arc-en-ciel");
            if !self.arc_en_ciel {
                ui.color_edit_button_srgb(&mut self.couleur);
            }
            ui.label("Épaisseur");
            ui.add(egui::Slider::new(&mut self.epaisseur, 1.0..=14.0).show_value(false));
            ui.label("Fond");
            ui.color_edit_button_srgb(&mut self.fond);
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.traits.is_empty(), egui::Button::new("↶ Annuler")).clicked() {
                self.traits.pop();
            }
            if ui.button("🧹 Effacer").clicked() {
                self.traits.clear();
            }
            if ui.button("💾 Image (2000×2000)…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("mandala.png").save_file() {
                    self.message = self.image(2000).save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            ui.label(&self.message);
        });
        let cote = ui.available_width().min(ui.available_height()).max(250.0);
        let (zone, rep) = ui.allocate_exact_size(Vec2::splat(cote), Sense::drag());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 8.0, Color32::from_rgb(self.fond[0], self.fond[1], self.fond[2]));
        let c = zone.center();
        let rayon = cote / 2.0;
        // repères des branches
        for (x, y) in copies((0.0, -1.0), self.branches, false) {
            p.line_segment([c, c + Vec2::new(x, y) * rayon], Stroke::new(1.0_f32, Color32::from_white_alpha(14)));
        }
        if let Some(h) = rep.interact_pointer_pos() {
            let rel = ((h.x - c.x) / rayon, (h.y - c.y) / rayon);
            if rep.drag_started() {
                self.en_cours = Some(Trait { points: vec![rel], couleur: self.couleur, epaisseur: self.epaisseur, arc_en_ciel: self.arc_en_ciel });
            } else if let Some(t) = &mut self.en_cours {
                if t.points.last().is_none_or(|l| (l.0 - rel.0).abs() + (l.1 - rel.1).abs() > 0.004) {
                    t.points.push(rel);
                }
            }
        }
        if rep.drag_stopped() {
            if let Some(t) = self.en_cours.take() {
                if t.points.len() > 1 {
                    self.traits.push(t);
                }
            }
        }
        let echelle = cote / 600.0;
        for t in self.traits.iter().chain(self.en_cours.iter()) {
            let n = t.points.len().max(1);
            for (k, w) in t.points.windows(2).enumerate() {
                let col = if t.arc_en_ciel { teinte(k as f32 / n as f32) } else { t.couleur };
                let s = Stroke::new(t.epaisseur * 2.0 * echelle, Color32::from_rgb(col[0], col[1], col[2]));
                for (a, b) in copies(w[0], self.branches, self.miroir).into_iter().zip(copies(w[1], self.branches, self.miroir)) {
                    p.line_segment([c + Vec2::new(a.0, a.1) * rayon, c + Vec2::new(b.0, b.1) * rayon], s);
                }
            }
        }
        if self.traits.len() != avant {
            enregistrer("symetrie", &self.traits);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 8 · Art ASCII
// ---------------------------------------------------------------------------------------------------

pub const RAMPES: [(&str, &str); 3] = [("Classique", " .:-=+*#%@"), ("Détaillé", " .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"), ("Blocs", " ░▒▓█")];

/// Image → texte : `colonnes` caractères de large (les caractères sont deux fois plus hauts que larges).
pub fn image_ascii(img: &RgbaImage, colonnes: u32, rampe: &str, inverser: bool) -> String {
    let lignes = ((colonnes as f32 * img.height() as f32 / img.width().max(1) as f32) * 0.5).max(1.0) as u32;
    let petite = image::imageops::resize(img, colonnes.max(1), lignes, image::imageops::FilterType::Triangle);
    let car: Vec<char> = rampe.chars().collect();
    let mut s = String::new();
    for y in 0..lignes {
        for x in 0..colonnes {
            let p = petite.get_pixel(x, y);
            let mut l = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0 * p[3] as f32 / 255.0;
            // texte sombre sur fond clair : les zones sombres prennent les caractères pleins
            if !inverser {
                l = 1.0 - l;
            }
            s.push(car[((l * (car.len() - 1) as f32).round() as usize).min(car.len() - 1)]);
        }
        s.push('\n');
    }
    s
}

/// Texte en grandes lettres faites de caractères : chaque caractère est une case deux fois plus haute
/// que large, donc les lettres sont dessinées directement à cette taille (pas de réduction qui efface les traits).
pub fn banniere(t: &str, police: &ab_glyph::FontVec, hauteur: u32, plein: char) -> String {
    use ab_glyph::{Font, ScaleFont};
    // dessin fin aux vraies proportions, puis chaque case = moyenne d'un bloc F de large × 2F de haut
    const F: usize = 4;
    let s = hauteur as f32 * 2.0 * F as f32 * 1.15;
    let echelle = ab_glyph::PxScale::from(s);
    let sf = police.as_scaled(echelle);
    let (larg, haut) = ((sf.h_advance(police.glyph_id('W')) * t.chars().count() as f32 + 4.0) as usize, (sf.ascent() - sf.descent() + 2.0) as usize);
    let mut grille = vec![0f32; larg * haut];
    let mut x = 1.0;
    let mut precedent: Option<ab_glyph::GlyphId> = None;
    for c in t.chars() {
        let id = police.glyph_id(c);
        if let Some(p) = precedent {
            x += sf.kern(p, id);
        }
        let g = id.with_scale_and_position(echelle, ab_glyph::point(x, sf.ascent()));
        if let Some(o) = police.outline_glyph(g) {
            let r = o.px_bounds();
            o.draw(|gx, gy, v| {
                let (px, py) = (r.min.x as i32 + gx as i32, r.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && (px as usize) < larg && (py as usize) < haut {
                    let k = py as usize * larg + px as usize;
                    grille[k] = grille[k].max(v);
                }
            });
        }
        x += sf.h_advance(id);
        precedent = Some(id);
    }
    let (cl, ch) = (larg / F, haut / (2 * F));
    let case = |cx: usize, cy: usize| -> f32 {
        let mut s = 0.0;
        for y in cy * 2 * F..(cy + 1) * 2 * F {
            for x in cx * F..(cx + 1) * F {
                s += grille[y * larg + x];
            }
        }
        s / (2 * F * F) as f32
    };
    let mut lignes: Vec<String> = (0..ch).map(|y| (0..cl).map(|x| if case(x, y) > 0.35 { plein } else { ' ' }).collect::<String>().trim_end().to_string()).collect();
    while lignes.first().is_some_and(|l| l.is_empty()) {
        lignes.remove(0);
    }
    while lignes.last().is_some_and(|l| l.is_empty()) {
        lignes.pop();
    }
    lignes.join("\n") + "\n"
}

pub struct ArtAscii {
    mode: u8,
    image: Option<RgbaImage>,
    colonnes: u32,
    rampe: usize,
    inverser: bool,
    texte: String,
    hauteur: u32,
    plein: String,
    resultat: String,
    message: String,
}

impl ArtAscii {
    pub fn new() -> Self {
        ArtAscii { mode: 1, image: None, colonnes: 100, rampe: 0, inverser: false, texte: "OUTILS".into(), hauteur: 10, plein: "█".into(), resultat: String::new(), message: String::new() }
    }
}

impl Outil for ArtAscii {
    fn nom(&self) -> &'static str {
        "🔤 Art ASCII"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Art ASCII");
            ui.selectable_value(&mut self.mode, 1, "🔠 Texte en grand");
            ui.selectable_value(&mut self.mode, 0, "🖼 Image en caractères");
        });
        let mut refaire = false;
        if self.mode == 0 {
            ui.horizontal_wrapped(|ui| {
                if ui.button("🖼 Choisir une image…").clicked() {
                    if let Some(i) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"]).pick_file().and_then(|p| image::open(p).ok()) {
                        self.image = Some(i.to_rgba8());
                        refaire = true;
                    }
                }
                ui.label("Largeur");
                refaire |= ui.add(egui::Slider::new(&mut self.colonnes, 20..=240).suffix(" car.")).changed();
                for (k, (n, _)) in RAMPES.iter().enumerate() {
                    refaire |= ui.selectable_value(&mut self.rampe, k, *n).changed();
                }
                refaire |= ui.checkbox(&mut self.inverser, "Pour fond sombre").changed();
            });
            if refaire {
                if let Some(i) = &self.image {
                    self.resultat = image_ascii(i, self.colonnes, RAMPES[self.rampe].1, self.inverser);
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                refaire |= ui.add(egui::TextEdit::singleline(&mut self.texte).desired_width(220.0)).changed();
                ui.label("Hauteur");
                refaire |= ui.add(egui::Slider::new(&mut self.hauteur, 4..=24)).changed();
                ui.label("Caractère");
                refaire |= ui.add(egui::TextEdit::singleline(&mut self.plein).desired_width(30.0)).changed();
            });
            if refaire || self.resultat.is_empty() {
                if let Some(f) = crate::creatif::police(&["arialbd.ttf", "impact.ttf", "DejaVuSans-Bold.ttf", "LiberationSans-Bold.ttf"]) {
                    self.resultat = banniere(&self.texte, &f, self.hauteur, self.plein.chars().next().unwrap_or('█'));
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button("📋 Copier").clicked() {
                ui.ctx().copy_text(self.resultat.clone());
                self.message = "Copié !".into();
            }
            if ui.button("💾 Fichier .txt…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Texte", &["txt"]).set_file_name("ascii.txt").save_file() {
                    self.message = std::fs::write(&p, &self.resultat).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            ui.label(&self.message);
        });
        egui::ScrollArea::both().show(ui, |ui| {
            ui.label(RichText::new(&self.resultat).monospace().size(if self.mode == 0 { 7.0 } else { 11.0 }));
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 17 · Générateur de QR codes
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum GenreQr {
    Lien,
    Wifi,
    Contact,
    Email,
    Sms,
    Telephone,
    Texte,
}

/// Échappe les caractères spéciaux des formats WIFI: et MECARD:.
fn echapper(s: &str) -> String {
    s.chars().flat_map(|c| if matches!(c, '\\' | ';' | ',' | ':' | '"') { vec!['\\', c] } else { vec![c] }).collect()
}

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChampsQr {
    pub a: String,
    pub b: String,
    pub c: String,
    pub d: String,
    pub securite: u8, // Wi-Fi : 0 WPA, 1 WEP, 2 aucune
}

/// Texte encodé dans le QR code selon son type.
pub fn contenu_qr(g: GenreQr, c: &ChampsQr) -> String {
    match g {
        GenreQr::Lien | GenreQr::Texte => c.a.clone(),
        GenreQr::Wifi => format!("WIFI:T:{};S:{};P:{};;", ["WPA", "WEP", "nopass"][c.securite.min(2) as usize], echapper(&c.a), echapper(&c.b)),
        GenreQr::Contact => format!("MECARD:N:{};TEL:{};EMAIL:{};ADR:{};;", echapper(&c.a), echapper(&c.b), echapper(&c.c), echapper(&c.d)),
        GenreQr::Email => format!("mailto:{}?subject={}&body={}", c.a, encoder(&c.b), encoder(&c.c)),
        GenreQr::Sms => format!("SMSTO:{}:{}", c.a, c.b),
        GenreQr::Telephone => format!("tel:{}", c.a),
    }
}

fn encoder(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Image du QR code : `px` pixels par module, marge de 4 modules, couleurs, logo au centre.
pub fn image_qr(texte: &str, px: u32, avant: [u8; 3], fond: [u8; 3], logo: Option<&RgbaImage>) -> Option<RgbaImage> {
    let niveau = if logo.is_some() { qrcode::EcLevel::H } else { qrcode::EcLevel::M };
    let code = qrcode::QrCode::with_error_correction_level(texte.as_bytes(), niveau).ok()?;
    let w = code.width() as u32;
    let n = (w + 8) * px;
    let mut img = RgbaImage::from_pixel(n, n, Rgba([fond[0], fond[1], fond[2], 255]));
    let couleurs = code.to_colors();
    for y in 0..w {
        for x in 0..w {
            if couleurs[(y * w + x) as usize] == qrcode::Color::Dark {
                for dy in 0..px {
                    for dx in 0..px {
                        img.put_pixel((x + 4) * px + dx, (y + 4) * px + dy, Rgba([avant[0], avant[1], avant[2], 255]));
                    }
                }
            }
        }
    }
    if let Some(l) = logo {
        // le logo couvre au plus 20 % de la largeur (le niveau H corrige jusqu'à 30 %)
        let t = (w * px) / 5;
        let r = image::imageops::resize(l, t, t, image::imageops::FilterType::Triangle);
        let o = (n - t) / 2;
        crate::atelier::rectangle(&mut img, o as f32 - px as f32, o as f32 - px as f32, (t + 2 * px) as f32, (t + 2 * px) as f32, Rgba([fond[0], fond[1], fond[2], 255]));
        image::imageops::overlay(&mut img, &r, o as i64, o as i64);
    }
    Some(img)
}

pub struct QrCodes {
    genre: GenreQr,
    champs: ChampsQr,
    avant: [u8; 3],
    fond: [u8; 3],
    logo: Option<RgbaImage>,
    apercu: Option<(String, egui::TextureHandle)>,
    message: String,
}

impl QrCodes {
    pub fn new() -> Self {
        QrCodes { genre: GenreQr::Lien, champs: ChampsQr { a: "https://".into(), ..Default::default() }, avant: [0, 0, 0], fond: [255, 255, 255], logo: None, apercu: None, message: String::new() }
    }
}

impl Outil for QrCodes {
    fn nom(&self) -> &'static str {
        "🔳 Générateur de QR codes"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur de QR codes");
        ui.horizontal_wrapped(|ui| {
            for (g, n) in [(GenreQr::Lien, "🔗 Lien"), (GenreQr::Wifi, "📶 Wi-Fi"), (GenreQr::Contact, "👤 Contact"), (GenreQr::Email, "✉ E-mail"), (GenreQr::Sms, "💬 SMS"), (GenreQr::Telephone, "📞 Téléphone"), (GenreQr::Texte, "📝 Texte")] {
                if ui.selectable_label(self.genre == g, n).clicked() && self.genre != g {
                    self.genre = g;
                    self.champs = ChampsQr::default();
                    if g == GenreQr::Lien {
                        self.champs.a = "https://".into();
                    }
                }
            }
        });
        let c = &mut self.champs;
        let champ = |ui: &mut egui::Ui, nom: &str, v: &mut String, mdp: bool| {
            ui.horizontal(|ui| {
                ui.add_sized([120.0, 20.0], egui::Label::new(nom));
                ui.add(egui::TextEdit::singleline(v).password(mdp).desired_width(320.0));
            });
        };
        match self.genre {
            GenreQr::Lien => champ(ui, "Adresse", &mut c.a, false),
            GenreQr::Texte => {
                ui.add(egui::TextEdit::multiline(&mut c.a).desired_rows(4).desired_width(440.0));
            }
            GenreQr::Wifi => {
                champ(ui, "Nom du Wi-Fi", &mut c.a, false);
                champ(ui, "Mot de passe", &mut c.b, false);
                ui.horizontal(|ui| {
                    ui.add_sized([120.0, 20.0], egui::Label::new("Sécurité"));
                    for (k, n) in ["WPA/WPA2/WPA3", "WEP", "Aucune"].iter().enumerate() {
                        ui.selectable_value(&mut c.securite, k as u8, *n);
                    }
                });
                ui.weak("Tes invités scannent et se connectent sans taper le mot de passe.");
            }
            GenreQr::Contact => {
                champ(ui, "Nom", &mut c.a, false);
                champ(ui, "Téléphone", &mut c.b, false);
                champ(ui, "E-mail", &mut c.c, false);
                champ(ui, "Adresse", &mut c.d, false);
            }
            GenreQr::Email => {
                champ(ui, "Destinataire", &mut c.a, false);
                champ(ui, "Objet", &mut c.b, false);
                champ(ui, "Message", &mut c.c, false);
            }
            GenreQr::Sms => {
                champ(ui, "Numéro", &mut c.a, false);
                champ(ui, "Message", &mut c.b, false);
            }
            GenreQr::Telephone => champ(ui, "Numéro", &mut c.a, false),
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Couleurs");
            ui.color_edit_button_srgb(&mut self.avant);
            ui.color_edit_button_srgb(&mut self.fond);
            if ui.button("🖼 Logo au centre…").clicked() {
                self.logo = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file().and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
                self.apercu = None;
            }
            if self.logo.is_some() && ui.small_button("✖ logo").clicked() {
                self.logo = None;
                self.apercu = None;
            }
        });
        let texte = contenu_qr(self.genre, &self.champs);
        let cle = format!("{texte}|{:?}|{:?}|{}", self.avant, self.fond, self.logo.is_some());
        if self.apercu.as_ref().is_none_or(|(k, _)| *k != cle) {
            self.apercu = image_qr(&texte, 8, self.avant, self.fond, self.logo.as_ref()).map(|i| {
                let ci = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
                (cle.clone(), ui.ctx().load_texture("qr", ci, egui::TextureOptions::NEAREST))
            });
        }
        match &self.apercu {
            Some((_, t)) => {
                ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::splat(280.0)));
            }
            None => {
                ui.colored_label(Color32::LIGHT_RED, "Texte trop long pour un QR code.");
            }
        }
        let contraste = |c: [u8; 3]| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        if contraste(self.fond) - contraste(self.avant) < 100.0 {
            ui.colored_label(Color32::from_rgb(255, 190, 90), "⚠ Pas assez de contraste : certains téléphones ne le liront pas (il faut un dessin foncé sur fond clair).");
        }
        ui.horizontal(|ui| {
            if ui.button("💾 Enregistrer en PNG (grand)…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("qr_code.png").save_file() {
                    self.message = match image_qr(&texte, 24, self.avant, self.fond, self.logo.as_ref()) {
                        Some(i) => i.save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string()),
                        None => "texte trop long".into(),
                    };
                }
            }
            ui.label(&self.message);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creatif() {
        assert!((frequence(69) - 440.0).abs() < 1e-3);
        assert!((frequence(60) - 261.63).abs() < 0.01);
        for i in [Instrument::Piano, Instrument::Orgue, Instrument::Synthe, Instrument::Bit8, Instrument::Guitare] {
            let s = son_note(60, i);
            assert!(!s.is_empty() && s.iter().all(|x| x.is_finite() && x.abs() <= 1.5), "{i:?}");
        }
        assert_eq!(nom_note(60), "Do4");
        assert!(est_noire(1) && !est_noire(4));
        let m = rendre_morceau(&[(0.0, 60, Instrument::Piano), (0.5, 64, Instrument::Piano)]);
        assert!(m.len() > TAUX as usize * 2 && m.iter().all(|x| x.abs() <= 1.0));
        let c = copies((0.5, 0.0), 4, true);
        assert_eq!(c.len(), 8);
        assert!((c[2].0).abs() < 1e-5 && (c[2].1 - 0.5).abs() < 1e-5); // quart de tour
        let img = RgbaImage::from_fn(20, 10, |x, _| if x < 10 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let a = image_ascii(&img, 20, " #", false);
        let l = a.lines().next().unwrap();
        assert!(l.starts_with('#') && l.ends_with(' '));
        assert_eq!(contenu_qr(GenreQr::Wifi, &ChampsQr { a: "Maison;1".into(), b: "abc".into(), ..Default::default() }), "WIFI:T:WPA;S:Maison\\;1;P:abc;;");
        assert!(contenu_qr(GenreQr::Email, &ChampsQr { a: "a@b.fr".into(), b: "Salut toi".into(), ..Default::default() }).contains("subject=Salut%20toi"));
        let q = image_qr("https://exemple.fr", 4, [0, 0, 0], [255, 255, 255], None).unwrap();
        assert_eq!(q.width(), q.height());
        assert_eq!(*q.get_pixel(0, 0), Rgba([255, 255, 255, 255])); // marge blanche
        let logo = RgbaImage::from_pixel(10, 10, Rgba([255, 0, 0, 255]));
        assert!(image_qr("https://exemple.fr", 4, [0, 0, 0], [255, 255, 255], Some(&logo)).is_some());
    }
}
