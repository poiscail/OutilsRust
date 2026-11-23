//! Créatif, suite : captures stylées (67), générateur de fonds d'écran (68), quiz (75).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------- dessin ----------

fn melanger(p: &mut Rgba<u8>, c: [u8; 3], a: f32) {
    let a = a.clamp(0.0, 1.0);
    for k in 0..3 {
        p.0[k] = (p.0[k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
    }
    p.0[3] = 255;
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|k| (a[k] as f32 + (b[k] as f32 - a[k] as f32) * t) as u8)
}

fn degrade(l: u32, h: u32, haut: [u8; 3], bas: [u8; 3], diagonal: bool) -> RgbaImage {
    RgbaImage::from_fn(l, h, |x, y| {
        let t = if diagonal { (x as f32 / l as f32 + y as f32 / h as f32) / 2.0 } else { y as f32 / h as f32 };
        let c = lerp(haut, bas, t);
        Rgba([c[0], c[1], c[2], 255])
    })
}

fn disque_doux(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3], a: f32, flou: f32) {
    let (l, h) = (img.width() as i32, img.height() as i32);
    let rr = r + flou;
    for y in ((cy - rr) as i32).max(0)..((cy + rr) as i32 + 1).min(h) {
        for x in ((cx - rr) as i32).max(0)..((cx + rr) as i32 + 1).min(l) {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let k = if d <= r { 1.0 } else if d < rr { 1.0 - (d - r) / flou.max(0.01) } else { 0.0 };
            if k > 0.0 {
                melanger(img.get_pixel_mut(x as u32, y as u32), c, a * k);
            }
        }
    }
}

/// Distance signée à un rectangle aux coins arrondis centré en (0, 0), demi-tailles (hx, hy).
pub fn sdf_rect(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = px.abs() - hx + r;
    let qy = py.abs() - hy + r;
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r
}

struct Hasard(u64);

impl Hasard {
    fn suivant(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn entre(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.suivant()
    }
}

// ---------------------------------------------------------------------------------------------------
// 67 · Captures stylées
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Style {
    pub fond: usize,
    pub marge: u32,
    pub rayon: u32,
    pub ombre: bool,
    pub barre: bool,
    pub legende: String,
}

impl Default for Style {
    fn default() -> Self {
        Style { fond: 0, marge: 64, rayon: 14, ombre: true, barre: true, legende: String::new() }
    }
}

pub const FONDS: [(&str, [u8; 3], [u8; 3]); 7] = [
    ("Sakura", [255, 120, 170], [90, 40, 120]),
    ("Nuit", [30, 20, 60], [10, 10, 20]),
    ("Océan", [40, 140, 220], [20, 40, 110]),
    ("Coucher de soleil", [255, 170, 80], [200, 60, 120]),
    ("Menthe", [120, 230, 190], [40, 120, 140]),
    ("Gris", [80, 80, 90], [40, 40, 48]),
    ("Blanc", [245, 245, 250], [215, 215, 225]),
];

pub fn styliser(img: &RgbaImage, s: &Style, police: Option<&ab_glyph::FontVec>) -> RgbaImage {
    let barre = if s.barre { 34 } else { 0 };
    let legende_h = if s.legende.trim().is_empty() || police.is_none() { 0 } else { 56 };
    let (iw, ih) = (img.width(), img.height() + barre);
    let (l, h) = (iw + 2 * s.marge, ih + 2 * s.marge + legende_h);
    let (_, a, b) = FONDS[s.fond.min(FONDS.len() - 1)];
    let mut c = degrade(l, h, a, b, true);
    let (cx, cy) = (s.marge as f32 + iw as f32 / 2.0, s.marge as f32 + ih as f32 / 2.0);
    let (hx, hy, r) = (iw as f32 / 2.0, ih as f32 / 2.0, s.rayon as f32);
    if s.ombre {
        let flou = (s.marge as f32 * 0.6).max(8.0);
        for (x, y, p) in c.enumerate_pixels_mut() {
            let d = sdf_rect(x as f32 - cx, y as f32 - (cy + flou * 0.35), hx, hy, r);
            if d > -1.0 && d < flou {
                let k = (1.0 - d.max(0.0) / flou).powi(2) * 0.45;
                melanger(p, [0, 0, 0], k);
            }
        }
    }
    let barre_c = [36u8, 36, 42];
    for y in 0..ih {
        for x in 0..iw {
            let (px, py) = (x as f32 + 0.5 - hx, y as f32 + 0.5 - hy);
            let d = sdf_rect(px, py, hx, hy, r);
            if d >= 0.5 {
                continue;
            }
            let a = (0.5 - d).min(1.0);
            let col = if y < barre {
                barre_c
            } else {
                let q = img.get_pixel(x, y - barre).0;
                [q[0], q[1], q[2]]
            };
            melanger(c.get_pixel_mut(x + s.marge, y + s.marge), col, a);
        }
    }
    if s.barre {
        for (k, col) in [[255u8, 95, 87], [254, 188, 46], [40, 200, 64]].into_iter().enumerate() {
            disque_doux(&mut c, s.marge as f32 + 20.0 + k as f32 * 20.0, s.marge as f32 + 17.0, 6.0, col, 1.0, 1.0);
        }
    }
    if let (Some(f), true) = (police, legende_h > 0) {
        let t = s.legende.trim();
        let taille = 30.0;
        let lt = crate::creatif::largeur(f, taille, t);
        let clair = a.iter().map(|x| *x as u32).sum::<u32>() > 600;
        crate::creatif::ecrire(&mut c, f, taille, (l as f32 - lt) / 2.0, (s.marge + ih) as f32 + 22.0, t, if clair { [30, 30, 40] } else { [255, 255, 255] }, None);
    }
    c
}

pub struct CapturesStylees {
    source: Option<RgbaImage>,
    style: Style,
    rendu: Option<(RgbaImage, egui::TextureHandle)>,
    sale: bool,
    /// capture différée : (en attente, image reçue)
    attente: Arc<Mutex<(bool, Option<RgbaImage>)>>,
    police: Option<ab_glyph::FontVec>,
    message: String,
}

impl CapturesStylees {
    pub fn new() -> Self {
        CapturesStylees { source: None, style: charger("captures_stylees"), rendu: None, sale: false, attente: Arc::default(), police: None, message: String::new() }
    }
}

impl CapturesStylees {
    /// La fenêtre est réduite : egui ne se redessine plus, donc l'attente et la capture se font dans un fil.
    fn differer(&self, ctx: &egui::Context, fenetre: bool) {
        self.attente.lock().unwrap().0 = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        let (a, ctx) = (self.attente.clone(), ctx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            let img = crate::capture::win::capturer().map(|e| {
                let (x, y, l, h) = if fenetre { crate::capture::win::fenetre_active(&e).unwrap_or((0, 0, e.w, e.h)) } else { (0, 0, e.w, e.h) };
                e.rgba(x, y, l, h)
            });
            *a.lock().unwrap() = (false, img);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.request_repaint();
        });
    }
}

impl Outil for CapturesStylees {
    fn nom(&self) -> &'static str {
        "✨ Captures stylées"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Captures stylées");
        ui.weak("Une capture ou une image, posée sur un joli fond avec coins arrondis et ombre : parfait pour Discord, un tuto ou un exposé.");
        if self.police.is_none() {
            self.police = crate::creatif::police(&["segoeuib.ttf", "arialbd.ttf", "arial.ttf"]);
        }
        {
            let mut a = self.attente.lock().unwrap();
            if let Some(img) = a.1.take() {
                self.source = Some(img);
                self.sale = true;
            }
            if a.0 {
                ui.label(RichText::new("📸 Capture dans 3 s… mets la fenêtre voulue au premier plan").size(18.0));
                return;
            }
        }
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            if let Ok(i) = image::open(&p) {
                self.source = Some(i.to_rgba8());
                self.sale = true;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("🪟 Capturer une fenêtre (3 s)").clicked() {
                self.differer(ui.ctx(), true);
            }
            if ui.button("🖥 Tout l'écran (3 s)").clicked() {
                self.differer(ui.ctx(), false);
            }
            if ui.button("🖼 Ouvrir une image…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                    match image::open(&p) {
                        Ok(i) => {
                            self.source = Some(i.to_rgba8());
                            self.sale = true;
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
        });
        let avant = self.style.clone();
        ui.horizontal_wrapped(|ui| {
            for (i, (n, a, _)) in FONDS.iter().enumerate() {
                if ui.selectable_label(self.style.fond == i, RichText::new(format!("● {n}")).color(Color32::from_rgb(a[0], a[1], a[2]))).clicked() {
                    self.style.fond = i;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Marge");
            ui.add(egui::Slider::new(&mut self.style.marge, 0..=200));
            ui.label("Coins");
            ui.add(egui::Slider::new(&mut self.style.rayon, 0..=40));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.style.ombre, "Ombre");
            ui.checkbox(&mut self.style.barre, "Barre de fenêtre");
            ui.add(egui::TextEdit::singleline(&mut self.style.legende).hint_text("légende (facultatif)").desired_width(260.0));
        });
        if self.style != avant {
            enregistrer("captures_stylees", &self.style);
            self.sale = true;
        }
        let Some(src) = &self.source else {
            ui.weak("Capture une fenêtre, ouvre une image ou glisse-la ici.");
            return;
        };
        if self.sale {
            self.sale = false;
            // aperçu : image réduite pour rester fluide
            let i = styliser(src, &self.style, self.police.as_ref());
            let ap = image::imageops::thumbnail(&i, 900.min(i.width()), (900.min(i.width()) as f32 * i.height() as f32 / i.width() as f32) as u32);
            let t = ui.ctx().load_texture("capture_stylee", egui::ColorImage::from_rgba_unmultiplied([ap.width() as usize, ap.height() as usize], ap.as_raw()), Default::default());
            self.rendu = Some((i, t));
        }
        if let Some((i, t)) = &self.rendu {
            ui.horizontal(|ui| {
                if ui.button("📋 Copier").clicked() {
                    self.message = if crate::capture::win::copier_image(i) { "Copié : colle-le dans Discord (Ctrl+V).".into() } else { "Copie impossible.".into() };
                }
                if ui.button("💾 Enregistrer en PNG…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("capture.png").save_file() {
                        self.message = i.save(&p).map(|_| format!("Enregistré : {}", p.display())).unwrap_or_else(|e| e.to_string());
                    }
                }
                ui.weak(format!("{} × {}", i.width(), i.height()));
            });
            ui.label(&self.message);
            let l = ui.available_width().min(t.size()[0] as f32);
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(l, l * t.size()[1] as f32 / t.size()[0] as f32)));
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 68 · Générateur de fonds d'écran
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Motif {
    #[default]
    Montagnes,
    Ville,
    Sakura,
    Bokeh,
    Vagues,
    Polygones,
    Degrade,
}

impl Motif {
    pub const TOUS: [Motif; 7] = [Motif::Montagnes, Motif::Ville, Motif::Sakura, Motif::Bokeh, Motif::Vagues, Motif::Polygones, Motif::Degrade];
    fn nom(self) -> &'static str {
        match self {
            Motif::Montagnes => "🏔 Montagnes",
            Motif::Ville => "🌃 Ville de nuit",
            Motif::Sakura => "🌸 Pétales",
            Motif::Bokeh => "✨ Bokeh",
            Motif::Vagues => "🌊 Vagues",
            Motif::Polygones => "🔷 Polygones",
            Motif::Degrade => "🎨 Dégradé",
        }
    }
}

/// (ciel haut, ciel bas, accent, sombre)
pub const PALETTES: [(&str, [[u8; 3]; 4]); 6] = [
    ("Rose sombre", [[40, 12, 40], [140, 50, 110], [255, 140, 190], [20, 8, 22]]),
    ("Coucher de soleil", [[60, 30, 90], [250, 130, 90], [255, 220, 140], [40, 20, 50]]),
    ("Océan", [[10, 30, 70], [40, 120, 190], [150, 230, 255], [5, 15, 35]]),
    ("Forêt", [[20, 50, 40], [90, 160, 120], [220, 240, 180], [10, 25, 20]]),
    ("Néon", [[15, 5, 40], [60, 10, 120], [0, 255, 220], [5, 0, 15]]),
    ("Aube", [[150, 190, 240], [255, 210, 200], [255, 255, 255], [90, 100, 140]]),
];

pub fn generer(m: Motif, pal: usize, graine: u64, l: u32, h: u32) -> RgbaImage {
    let [haut, bas, accent, sombre] = PALETTES[pal.min(PALETTES.len() - 1)].1;
    let mut r = Hasard(graine.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (lf, hf) = (l as f32, h as f32);
    let e = lf / 1920.0; // échelle
    let mut img = degrade(l, h, haut, bas, false);
    match m {
        Motif::Degrade => {
            img = degrade(l, h, haut, bas, true);
            for _ in 0..3 {
                let (x, y, rr) = (r.entre(0.0, lf), r.entre(0.0, hf), r.entre(300.0, 700.0) * e);
                disque_doux(&mut img, x, y, 0.0, accent, 0.35, rr);
            }
        }
        Motif::Montagnes => {
            let (sx, sy) = (r.entre(0.2, 0.8) * lf, r.entre(0.25, 0.45) * hf);
            disque_doux(&mut img, sx, sy, 0.0, accent, 0.35, 380.0 * e);
            disque_doux(&mut img, sx, sy, 90.0 * e, accent, 0.95, 2.0);
            for c in 0..5 {
                let t = c as f32 / 4.0;
                let col = lerp(lerp(bas, accent, 0.25), sombre, 0.35 + t * 0.65);
                let base = hf * (0.5 + t * 0.12);
                let amp = hf * (0.16 - t * 0.02);
                let ph: Vec<(f32, f32, f32)> = (0..4).map(|k| (r.entre(0.0, std::f32::consts::TAU), r.entre(1.0, 3.0) * (k + 1) as f32, 1.0 / (k + 1) as f32)).collect();
                for x in 0..l {
                    let u = x as f32 / lf;
                    let y0 = base - amp * ph.iter().map(|(p, f, a)| (u * f * std::f32::consts::TAU + p).sin() * a).sum::<f32>().abs();
                    for y in (y0.max(0.0) as u32)..h {
                        melanger(img.get_pixel_mut(x, y), col, 1.0);
                    }
                }
            }
        }
        Motif::Ville => {
            for _ in 0..(400.0 * e * e) as usize {
                let (x, y) = (r.entre(0.0, lf), r.entre(0.0, hf * 0.6));
                disque_doux(&mut img, x, y, r.entre(0.3, 1.4) * e, [255, 255, 255], r.entre(0.3, 1.0), 1.0);
            }
            let (lx, ly) = (r.entre(0.6, 0.9) * lf, 0.18 * hf);
            disque_doux(&mut img, lx, ly, 0.0, accent, 0.3, 200.0 * e);
            disque_doux(&mut img, lx, ly, 55.0 * e, [250, 240, 230], 1.0, 2.0);
            for couche in 0..2 {
                let col = if couche == 0 { lerp(sombre, bas, 0.35) } else { sombre };
                let mut x = 0.0;
                while x < lf {
                    let bl = r.entre(40.0, 120.0) * e;
                    let bh = r.entre(0.12, 0.42 - couche as f32 * 0.12) * hf;
                    let y0 = hf - bh;
                    for yy in (y0 as u32)..h {
                        for xx in (x as u32)..((x + bl) as u32).min(l) {
                            melanger(img.get_pixel_mut(xx, yy), col, 1.0);
                        }
                    }
                    if couche == 1 {
                        let mut wy = y0 + 12.0 * e;
                        while wy < hf - 10.0 * e {
                            let mut wx = x + 8.0 * e;
                            while wx < x + bl - 10.0 * e {
                                if r.suivant() < 0.28 {
                                    for yy in wy as u32..(wy + 7.0 * e) as u32 {
                                        for xx in wx as u32..((wx + 5.0 * e) as u32).min(l) {
                                            melanger(img.get_pixel_mut(xx, yy.min(h - 1)), accent, 0.85);
                                        }
                                    }
                                }
                                wx += 14.0 * e;
                            }
                            wy += 18.0 * e;
                        }
                    }
                    x += bl + r.entre(0.0, 10.0) * e;
                }
            }
        }
        Motif::Sakura | Motif::Bokeh => {
            let n = if m == Motif::Bokeh { 70 } else { 40 };
            for _ in 0..n {
                let (x, y, rr) = (r.entre(0.0, lf), r.entre(0.0, hf), r.entre(30.0, 160.0) * e);
                disque_doux(&mut img, x, y, rr * 0.6, lerp(accent, [255, 255, 255], r.suivant() * 0.4), r.entre(0.08, 0.25), rr * 0.4);
            }
            if m == Motif::Sakura {
                for _ in 0..(260.0 * e) as usize {
                    let (cx, cy) = (r.entre(0.0, lf), r.entre(0.0, hf));
                    let (a, b) = (r.entre(9.0, 18.0) * e, r.entre(5.0, 9.0) * e);
                    let ang = r.entre(0.0, std::f32::consts::PI);
                    let col = lerp([255, 170, 200], [255, 230, 240], r.suivant());
                    let (ca, sa) = (ang.cos(), ang.sin());
                    for y in ((cy - a) as i32).max(0)..((cy + a) as i32).min(h as i32) {
                        for x in ((cx - a) as i32).max(0)..((cx + a) as i32).min(l as i32) {
                            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                            let (u, v) = (dx * ca + dy * sa, -dx * sa + dy * ca);
                            let d = (u / a).powi(2) + (v / b).powi(2);
                            if d < 1.0 {
                                melanger(img.get_pixel_mut(x as u32, y as u32), col, 0.85 * (1.0 - d).sqrt().min(1.0));
                            }
                        }
                    }
                }
            }
        }
        Motif::Vagues => {
            for c in 0..6 {
                let t = c as f32 / 5.0;
                let col = lerp(accent, sombre, 0.2 + t * 0.8);
                let (ph, f, amp) = (r.entre(0.0, std::f32::consts::TAU), r.entre(1.0, 2.5), hf * 0.04);
                let base = hf * (0.45 + t * 0.1);
                for x in 0..l {
                    let u = x as f32 / lf;
                    let y0 = base + amp * (u * f * std::f32::consts::TAU + ph).sin() + amp * 0.4 * (u * f * 15.0 + ph * 2.0).sin();
                    for y in (y0.max(0.0) as u32)..h {
                        melanger(img.get_pixel_mut(x, y), col, 0.55 + t * 0.45);
                    }
                }
            }
        }
        Motif::Polygones => {
            let pas = 160.0 * e;
            let (nx, ny) = ((lf / pas) as usize + 2, (hf / pas) as usize + 2);
            let pts: Vec<Vec<(f32, f32)>> = (0..ny).map(|j| (0..nx).map(|i| (i as f32 * pas + r.entre(-0.4, 0.4) * pas, j as f32 * pas + r.entre(-0.4, 0.4) * pas)).collect()).collect();
            let mut tri = |a: (f32, f32), b: (f32, f32), c: (f32, f32), r: &mut Hasard| {
                let (gx, gy) = ((a.0 + b.0 + c.0) / 3.0, (a.1 + b.1 + c.1) / 3.0);
                let base = lerp(lerp(haut, bas, gy / hf), accent, (gx / lf) * 0.35);
                let col = lerp(base, if r.suivant() < 0.5 { sombre } else { [255, 255, 255] }, r.entre(0.0, 0.15));
                let (x0, x1) = (a.0.min(b.0).min(c.0).max(0.0) as u32, (a.0.max(b.0).max(c.0) + 1.0).min(lf) as u32);
                let (y0, y1) = (a.1.min(b.1).min(c.1).max(0.0) as u32, (a.1.max(b.1).max(c.1) + 1.0).min(hf) as u32);
                let aire = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                if aire.abs() < 1e-3 {
                    return;
                }
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / aire;
                        let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / aire;
                        if w0 >= -0.001 && w1 >= -0.001 && w0 + w1 <= 1.001 {
                            melanger(img.get_pixel_mut(x, y), col, 1.0);
                        }
                    }
                }
            };
            for j in 0..ny - 1 {
                for i in 0..nx - 1 {
                    let (a, b, c, d) = (pts[j][i], pts[j][i + 1], pts[j + 1][i], pts[j + 1][i + 1]);
                    tri(a, b, c, &mut r);
                    tri(b, d, c, &mut r);
                }
            }
        }
    }
    // léger grain pour éviter les bandes de couleur
    for p in img.pixels_mut() {
        let g = (r.suivant() - 0.5) * 6.0;
        for k in 0..3 {
            p.0[k] = (p.0[k] as f32 + g).clamp(0.0, 255.0) as u8;
        }
    }
    img
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ReglageFonds {
    pub motif: Motif,
    pub palette: usize,
    pub graine: u64,
    pub chaque_jour: bool,
    pub jour: String,
}

fn dossier_fonds() -> PathBuf {
    crate::outil::chemin("fonds_generes")
}

fn appliquer_fond(r: &ReglageFonds) -> Result<PathBuf, String> {
    let (l, h) = crate::capture::win::ecran_principal();
    let img = generer(r.motif, r.palette, r.graine, l.max(800) as u32, h.max(600) as u32);
    let d = dossier_fonds();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    // un nom différent à chaque fois, sinon Windows garde l'ancienne image en cache
    for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(e.path());
    }
    let p = d.join(format!("fond_{}.png", r.graine));
    img.save(&p).map_err(|e| e.to_string())?;
    crate::systeme::fond_bureau(&p);
    Ok(p)
}

pub struct FondsEcran {
    r: Arc<Mutex<ReglageFonds>>,
    apercu: Option<(ReglageFonds, egui::TextureHandle)>,
    calcul: Arc<Mutex<Option<(ReglageFonds, RgbaImage)>>>,
    demande: Option<(Motif, usize, u64)>,
    e: Arc<Mutex<(bool, String)>>,
}

impl FondsEcran {
    pub fn new() -> Self {
        let mut r0: ReglageFonds = charger("fonds_generes");
        if r0.graine == 0 {
            r0.graine = crate::outil::hasard(u64::MAX - 1) + 1;
        }
        let r = Arc::new(Mutex::new(r0));
        let r2 = r.clone();
        std::thread::spawn(move || loop {
            let jour = crate::minecraft::horodatage_iso().get(..10).unwrap_or("").to_string();
            let x = r2.lock().unwrap().clone();
            if x.chaque_jour && x.jour != jour {
                let mut n = x.clone();
                n.graine = crate::outil::hasard(u64::MAX - 1) + 1;
                n.jour = jour;
                if appliquer_fond(&n).is_ok() {
                    enregistrer("fonds_generes", &n);
                    *r2.lock().unwrap() = n;
                }
            }
            std::thread::sleep(Duration::from_secs(600));
        });
        FondsEcran { r, apercu: None, calcul: Arc::default(), demande: None, e: Arc::default() }
    }
}

impl Outil for FondsEcran {
    fn nom(&self) -> &'static str {
        "🖼 Générateur de fonds d'écran"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur de fonds d'écran");
        ui.weak("Des fonds uniques, dessinés à la résolution de ton écran. Chaque « 🎲 » en crée un nouveau.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal_wrapped(|ui| {
            for m in Motif::TOUS {
                ui.selectable_value(&mut r.motif, m, m.nom());
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (i, (n, c)) in PALETTES.iter().enumerate() {
                if ui.selectable_label(r.palette == i, RichText::new(format!("● {n}")).color(Color32::from_rgb(c[2][0], c[2][1], c[2][2]))).clicked() {
                    r.palette = i;
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button(RichText::new("🎲 Un autre").size(16.0)).clicked() {
                r.graine = crate::outil::hasard(u64::MAX - 1) + 1;
            }
            let (o, m) = self.e.lock().unwrap().clone();
            if ui.add_enabled(!o, egui::Button::new(RichText::new("🖥 Mettre en fond d'écran").size(16.0))).clicked() {
                let (x, e) = (r.clone(), self.e.clone());
                *e.lock().unwrap() = (true, "Dessin en pleine résolution…".into());
                std::thread::spawn(move || *e.lock().unwrap() = (false, appliquer_fond(&x).map(|_| "✔ Fond d'écran appliqué.".into()).unwrap_or_else(|e| format!("✖ {e}"))));
            }
            if ui.button("💾 Enregistrer…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("fond.png").save_file() {
                    let (x, e) = (r.clone(), self.e.clone());
                    *e.lock().unwrap() = (true, "Dessin en pleine résolution…".into());
                    std::thread::spawn(move || {
                        let (l, h) = crate::capture::win::ecran_principal();
                        let res = generer(x.motif, x.palette, x.graine, l.max(800) as u32, h.max(600) as u32).save(&p);
                        *e.lock().unwrap() = (false, res.map(|_| format!("✔ Enregistré : {}", p.display())).unwrap_or_else(|e| e.to_string()));
                    });
                }
            }
            if o {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            ui.label(m);
        });
        ui.checkbox(&mut r.chaque_jour, "📅 Un nouveau fond chaque jour (même style)");
        if r != avant {
            enregistrer("fonds_generes", &r);
            *self.r.lock().unwrap() = r.clone();
        }
        // aperçu calculé en fond
        if let Some((rr, img)) = self.calcul.lock().unwrap().take() {
            let t = ui.ctx().load_texture("fond_genere", egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), Default::default());
            self.apercu = Some((rr, t));
        }
        let cle = (r.motif, r.palette, r.graine);
        if self.demande != Some(cle) {
            self.demande = Some(cle);
            let (c, x, ctx) = (self.calcul.clone(), r.clone(), ui.ctx().clone());
            std::thread::spawn(move || {
                let img = generer(x.motif, x.palette, x.graine, 768, 432);
                *c.lock().unwrap() = Some((x, img));
                ctx.request_repaint();
            });
        }
        match &self.apercu {
            Some((_, t)) => {
                let l = ui.available_width().min(768.0);
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(l, l * 9.0 / 16.0)));
            }
            None => {
                ui.spinner();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 75 · Quiz
// ---------------------------------------------------------------------------------------------------

/// (thème, question, réponses, indice de la bonne)
pub const QUESTIONS: &[(&str, &str, [&str; 4], usize)] = &[
    ("Jeux vidéo", "Dans Minecraft, quel minerai faut-il pour fabriquer une table d'enchantement ?", ["Diamant", "Émeraude", "Or", "Fer"], 0),
    ("Jeux vidéo", "Quel monstre de Minecraft explose quand il s'approche ?", ["Creeper", "Enderman", "Zombie", "Squelette"], 0),
    ("Jeux vidéo", "Quelle dimension de Minecraft abrite l'Ender Dragon ?", ["L'End", "Le Nether", "La surface", "Les Abysses"], 0),
    ("Jeux vidéo", "Quelle entreprise a créé la console PlayStation ?", ["Sony", "Nintendo", "Microsoft", "Sega"], 0),
    ("Jeux vidéo", "En quelle année est sortie la première Nintendo Switch ?", ["2017", "2015", "2019", "2013"], 0),
    ("Jeux vidéo", "Combien de joueurs au maximum dans une partie classique de Fortnite Battle Royale ?", ["100", "50", "64", "150"], 0),
    ("Jeux vidéo", "Quel est le nom de la plateforme de jeux de Valve ?", ["Steam", "Origin", "Uplay", "Battle.net"], 0),
    ("Jeux vidéo", "Dans Minecraft, combien de blocs d'obsidienne faut-il au minimum pour un portail du Nether (sans les coins) ?", ["10", "14", "8", "12"], 0),
    ("Jeux vidéo", "Quel jeu de Riot Games oppose deux équipes de 5 sur la Faille de l'invocateur ?", ["League of Legends", "Valorant", "Dota 2", "Overwatch"], 0),
    ("Jeux vidéo", "Dans quel pays est née la série Pokémon ?", ["Japon", "Corée du Sud", "États-Unis", "Chine"], 0),
    ("Jeux vidéo", "Combien de cases compte un plateau d'échecs ?", ["64", "81", "100", "49"], 0),
    ("Géographie", "Quelle est la capitale du Japon ?", ["Tokyo", "Kyoto", "Osaka", "Séoul"], 0),
    ("Géographie", "Quel est le plus long fleuve de France ?", ["La Loire", "La Seine", "Le Rhône", "La Garonne"], 0),
    ("Géographie", "Quel est le plus grand océan du monde ?", ["Pacifique", "Atlantique", "Indien", "Arctique"], 0),
    ("Géographie", "Quelle est la plus haute montagne du monde ?", ["L'Everest", "Le K2", "Le Mont Blanc", "Le Kilimandjaro"], 0),
    ("Géographie", "Combien de pays compte l'Union européenne (2025) ?", ["27", "28", "25", "30"], 0),
    ("Géographie", "Quelle est la capitale du Canada ?", ["Ottawa", "Toronto", "Montréal", "Vancouver"], 0),
    ("Géographie", "Sur quel continent se trouve l'Égypte ?", ["Afrique", "Asie", "Europe", "Océanie"], 0),
    ("Géographie", "Quelle est la capitale de l'Australie ?", ["Canberra", "Sydney", "Melbourne", "Perth"], 0),
    ("Géographie", "Quel pays a la forme d'une botte ?", ["L'Italie", "L'Espagne", "La Grèce", "Le Portugal"], 0),
    ("Géographie", "Quel est le plus grand pays du monde par la superficie ?", ["La Russie", "Le Canada", "La Chine", "Les États-Unis"], 0),
    ("Sciences", "Quelle planète est la plus proche du Soleil ?", ["Mercure", "Vénus", "Mars", "La Terre"], 0),
    ("Sciences", "Quelle est la formule chimique de l'eau ?", ["H₂O", "CO₂", "O₂", "NaCl"], 0),
    ("Sciences", "Combien d'os compte le squelette d'un adulte ?", ["206", "180", "250", "300"], 0),
    ("Sciences", "À quelle température l'eau bout-elle au niveau de la mer ?", ["100 °C", "90 °C", "120 °C", "80 °C"], 0),
    ("Sciences", "Quelle est la plus grosse planète du système solaire ?", ["Jupiter", "Saturne", "Neptune", "Uranus"], 0),
    ("Sciences", "Quel gaz les plantes absorbent-elles pour la photosynthèse ?", ["Le dioxyde de carbone", "L'oxygène", "L'azote", "L'hélium"], 0),
    ("Sciences", "Combien de chromosomes possède une cellule humaine (hors gamètes) ?", ["46", "23", "48", "44"], 0),
    ("Sciences", "Quelle est la vitesse de la lumière dans le vide, environ ?", ["300 000 km/s", "30 000 km/s", "3 000 km/s", "3 000 000 km/s"], 0),
    ("Sciences", "Quel est l'organe qui pompe le sang ?", ["Le cœur", "Le foie", "Les poumons", "Les reins"], 0),
    ("Sciences", "Quel est le symbole chimique de l'or ?", ["Au", "Ag", "Or", "Fe"], 0),
    ("Sciences", "Combien de pattes a une araignée ?", ["8", "6", "10", "12"], 0),
    ("Informatique", "Que veut dire « CPU » ?", ["Processeur", "Carte graphique", "Mémoire vive", "Disque dur"], 0),
    ("Informatique", "Combien d'octets dans un kilooctet (au sens strict, kibioctet) ?", ["1024", "1000", "512", "2048"], 0),
    ("Informatique", "Quel raccourci copie la sélection sous Windows ?", ["Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+Z"], 0),
    ("Informatique", "Quelle entreprise développe Windows ?", ["Microsoft", "Apple", "Google", "IBM"], 0),
    ("Informatique", "Quel langage sert à structurer une page web ?", ["HTML", "Python", "C++", "SQL"], 0),
    ("Informatique", "Quel raccourci ouvre le gestionnaire des tâches directement ?", ["Ctrl+Maj+Échap", "Alt+F4", "Win+D", "Ctrl+Alt+T"], 0),
    ("Informatique", "Que signifie « Wi-Fi » techniquement ?", ["Un réseau sans fil", "Un câble réseau", "Un disque externe", "Un processeur"], 0),
    ("Histoire", "En quelle année a eu lieu la prise de la Bastille ?", ["1789", "1815", "1492", "1914"], 0),
    ("Histoire", "Qui a été le premier homme à marcher sur la Lune ?", ["Neil Armstrong", "Youri Gagarine", "Buzz Aldrin", "Thomas Pesquet"], 0),
    ("Histoire", "En quelle année a commencé la Première Guerre mondiale ?", ["1914", "1918", "1939", "1870"], 0),
    ("Histoire", "Quel empereur français a été exilé à Sainte-Hélène ?", ["Napoléon Ier", "Louis XIV", "Charlemagne", "Napoléon III"], 0),
    ("Histoire", "Quelle civilisation a construit les pyramides de Gizeh ?", ["Les Égyptiens", "Les Romains", "Les Mayas", "Les Grecs"], 0),
    ("Histoire", "En quelle année est tombé le mur de Berlin ?", ["1989", "1991", "1975", "2001"], 0),
    ("Histoire", "Quelle ville japonaise a été la première frappée par une bombe atomique ?", ["Hiroshima", "Nagasaki", "Tokyo", "Kyoto"], 0),
    ("Japon", "Comment dit-on « merci » en japonais ?", ["Arigatō", "Konnichiwa", "Sayonara", "Sumimasen"], 0),
    ("Japon", "Quel est l'arbre dont la floraison est célébrée au printemps au Japon ?", ["Le cerisier (sakura)", "L'érable", "Le bambou", "Le pin"], 0),
    ("Japon", "Quel est le nom du célèbre volcan près de Tokyo ?", ["Le mont Fuji", "Le mont Aso", "Le Sakurajima", "Le mont Ontake"], 0),
    ("Japon", "Comment s'appellent les bandes dessinées japonaises ?", ["Les mangas", "Les comics", "Les manhwas", "Les webtoons"], 0),
    ("Japon", "Quel plat japonais est fait de riz vinaigré et de poisson cru ?", ["Les sushis", "Les ramen", "Les gyozas", "Les tempuras"], 0),
    ("Japon", "Quelle est la monnaie du Japon ?", ["Le yen", "Le won", "Le yuan", "Le baht"], 0),
    ("Culture", "Qui a peint la Joconde ?", ["Léonard de Vinci", "Picasso", "Van Gogh", "Michel-Ange"], 0),
    ("Culture", "Combien de joueurs par équipe sur un terrain de football ?", ["11", "10", "9", "12"], 0),
    ("Culture", "Quel instrument a 88 touches ?", ["Le piano", "L'orgue", "L'accordéon", "Le synthétiseur"], 0),
    ("Culture", "Qui a écrit « Les Misérables » ?", ["Victor Hugo", "Émile Zola", "Molière", "Albert Camus"], 0),
    ("Culture", "Dans quelle ville se trouve la tour Eiffel ?", ["Paris", "Lyon", "Marseille", "Bruxelles"], 0),
    ("Culture", "Combien de minutes dure un match de football (hors prolongations) ?", ["90", "80", "60", "100"], 0),
    ("Culture", "Quel animal est le plus grand mammifère du monde ?", ["La baleine bleue", "L'éléphant", "La girafe", "Le requin baleine"], 0),
    ("Culture", "Combien de couleurs dans un arc-en-ciel ?", ["7", "6", "5", "8"], 0),
];

pub fn themes() -> Vec<&'static str> {
    let mut v: Vec<&str> = QUESTIONS.iter().map(|q| q.0).collect();
    v.dedup();
    let mut u = vec![];
    for t in v {
        if !u.contains(&t) {
            u.push(t);
        }
    }
    u
}

/// Ordre des réponses mélangé : renvoie la permutation (indices dans les réponses d'origine).
pub fn melange(graine: u64) -> [usize; 4] {
    let mut p = [0, 1, 2, 3];
    let mut r = Hasard(graine | 1);
    for i in (1..4).rev() {
        let j = (r.suivant() * (i + 1) as f32) as usize % (i + 1);
        p.swap(i, j);
    }
    p
}

struct Partie {
    ordre: Vec<usize>,
    i: usize,
    joueurs: Vec<(String, u32)>,
    tour: usize,
    debut: Instant,
    reponse: Option<(usize, bool)>,
    perm: [usize; 4],
}

pub struct Quiz {
    theme: Option<&'static str>,
    nb: usize,
    noms: String,
    partie: Option<Partie>,
    record: u32,
}

impl Quiz {
    pub fn new() -> Self {
        Quiz { theme: None, nb: 10, noms: String::new(), partie: None, record: charger::<Option<u32>>("quiz_record").unwrap_or(0) }
    }

    fn commencer(&mut self) {
        let mut ordre: Vec<usize> = (0..QUESTIONS.len()).filter(|&k| self.theme.map_or(true, |t| QUESTIONS[k].0 == t)).collect();
        let mut r = Hasard(crate::outil::hasard(u64::MAX - 1) + 1);
        for i in (1..ordre.len()).rev() {
            let j = (r.suivant() * (i + 1) as f32) as usize % (i + 1);
            ordre.swap(i, j);
        }
        let mut joueurs: Vec<(String, u32)> = self.noms.split(',').map(|n| n.trim()).filter(|n| !n.is_empty()).map(|n| (n.to_string(), 0)).collect();
        if joueurs.is_empty() {
            joueurs.push(("Toi".into(), 0));
        }
        ordre.truncate(self.nb * joueurs.len());
        self.partie = Some(Partie { ordre, i: 0, joueurs, tour: 0, debut: Instant::now(), reponse: None, perm: melange(r.0) });
    }
}

const TEMPS: f32 = 15.0;

impl Outil for Quiz {
    fn nom(&self) -> &'static str {
        "❓ Quiz"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Quiz");
        let Some(p) = &mut self.partie else {
            ui.weak(format!("{} questions : jeux vidéo, Japon, sciences, géographie, histoire… 15 secondes par question, plus tu réponds vite, plus tu gagnes de points.", QUESTIONS.len()));
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.theme, None, "Tous les thèmes");
                for t in themes() {
                    ui.selectable_value(&mut self.theme, Some(t), t);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Questions par joueur");
                ui.add(egui::DragValue::new(&mut self.nb).range(3..=30));
            });
            ui.horizontal(|ui| {
                ui.label("Joueurs (à plusieurs sur le même PC)");
                ui.add(egui::TextEdit::singleline(&mut self.noms).hint_text("Léa, Tom, Inès…").desired_width(240.0));
            });
            if ui.button(RichText::new("▶ Jouer").size(20.0)).clicked() {
                self.commencer();
            }
            if self.record > 0 {
                ui.label(format!("🏆 Record solo : {} points", self.record));
            }
            return;
        };
        if p.i >= p.ordre.len() {
            ui.label(RichText::new("🏁 Terminé !").size(26.0));
            let mut cl = p.joueurs.clone();
            cl.sort_by(|a, b| b.1.cmp(&a.1));
            for (k, (n, s)) in cl.iter().enumerate() {
                ui.label(RichText::new(format!("{} {n} : {s} points", ["🥇", "🥈", "🥉", "  "][k.min(3)])).size(18.0));
            }
            if cl.len() == 1 && cl[0].1 > self.record {
                self.record = cl[0].1;
                enregistrer("quiz_record", &Some(self.record));
                ui.colored_label(Color32::from_rgb(255, 200, 80), "🏆 Nouveau record !");
            }
            if ui.button("↻ Rejouer").clicked() {
                self.commencer();
            }
            if ui.button("⬅ Menu").clicked() {
                self.partie = None;
            }
            return;
        }
        let (theme, q, rep, bonne) = QUESTIONS[p.ordre[p.i]];
        let joueur = p.tour % p.joueurs.len();
        let ecoule = p.debut.elapsed().as_secs_f32();
        ui.horizontal(|ui| {
            ui.label(format!("Question {} / {}", p.i + 1, p.ordre.len()));
            ui.weak(theme);
            if p.joueurs.len() > 1 {
                ui.label(RichText::new(format!("🎮 À {} de jouer", p.joueurs[joueur].0)).strong());
            }
            ui.label(format!("Score : {}", p.joueurs[joueur].1));
        });
        if p.reponse.is_none() {
            ui.add(egui::ProgressBar::new(1.0 - (ecoule / TEMPS).min(1.0)).desired_width(400.0));
            if ecoule >= TEMPS {
                p.reponse = Some((usize::MAX, false));
            }
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        ui.add_space(8.0);
        ui.label(RichText::new(q).size(22.0));
        ui.add_space(8.0);
        for &k in &p.perm {
            let mut b = egui::Button::new(RichText::new(rep[k]).size(18.0)).min_size(egui::vec2(420.0, 40.0));
            if let Some((choix, _)) = p.reponse {
                if k == bonne {
                    b = b.fill(Color32::from_rgb(40, 140, 70));
                } else if k == choix {
                    b = b.fill(Color32::from_rgb(160, 50, 60));
                }
            }
            if ui.add(b).clicked() && p.reponse.is_none() {
                let ok = k == bonne;
                if ok {
                    p.joueurs[joueur].1 += 100 + ((TEMPS - ecoule).max(0.0) / TEMPS * 100.0) as u32;
                }
                p.reponse = Some((k, ok));
            }
        }
        if let Some((choix, ok)) = p.reponse {
            ui.add_space(6.0);
            ui.label(RichText::new(if ok { "✔ Bonne réponse !" } else if choix == usize::MAX { "⏱ Temps écoulé !" } else { "✖ Raté !" }).size(18.0));
            if ui.button(RichText::new("Suivante ➡").size(16.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                p.i += 1;
                p.tour += 1;
                p.reponse = None;
                p.debut = Instant::now();
                p.perm = melange(p.debut.elapsed().as_nanos() as u64 ^ (p.i as u64 * 0x9E37) ^ crate::outil::hasard(1 << 40));
            }
        }
        if ui.small_button("Abandonner").clicked() {
            self.partie = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dessin_et_quiz() {
        assert!(sdf_rect(0.0, 0.0, 10.0, 10.0, 2.0) < 0.0);
        assert!((sdf_rect(12.0, 0.0, 10.0, 10.0, 2.0) - 2.0).abs() < 1e-4);
        for m in Motif::TOUS {
            for p in 0..PALETTES.len() {
                let i = generer(m, p, 42, 160, 90);
                assert_eq!((i.width(), i.height()), (160, 90));
            }
        }
        assert_eq!(generer(Motif::Sakura, 0, 7, 64, 36), generer(Motif::Sakura, 0, 7, 64, 36));
        let img = RgbaImage::from_pixel(50, 30, Rgba([255, 0, 0, 255]));
        let s = styliser(&img, &Style::default(), None);
        assert_eq!((s.width(), s.height()), (50 + 128, 30 + 34 + 128));
        assert_eq!(s.get_pixel(64 + 25, 64 + 34 + 15).0[..3], [255, 0, 0]);
        for q in QUESTIONS {
            assert!(q.3 < 4);
            let mut r = q.2.to_vec();
            r.sort();
            r.dedup();
            assert_eq!(r.len(), 4, "{}", q.1);
        }
        let mut m = melange(12345);
        m.sort();
        assert_eq!(m, [0, 1, 2, 3]);
        assert!(themes().len() >= 6);
    }
}
