//! Atelier : créateur de BD (22) et créateur de polices d'écriture (25, vrai fichier .ttf).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText, Sense};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn texture(ctx: &egui::Context, nom: &str, img: &RgbaImage, nette: bool) -> egui::TextureHandle {
    let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    ctx.load_texture(nom, c, if nette { egui::TextureOptions::NEAREST } else { egui::TextureOptions::LINEAR })
}

// ---------------------------------------------------------------------------------------------------
// Dessin simple dans une image
// ---------------------------------------------------------------------------------------------------

fn melanger(img: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>) {
    if x < 0 || y < 0 || x >= img.width() as i64 || y >= img.height() as i64 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let a = c[3] as u32;
    for k in 0..3 {
        p[k] = ((c[k] as u32 * a + p[k] as u32 * (255 - a)) / 255) as u8;
    }
    p[3] = p[3].max(c[3]);
}

pub fn ellipse(img: &mut RgbaImage, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba<u8>) {
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    for y in (cy - ry).floor() as i64..=(cy + ry).ceil() as i64 {
        let dy = (y as f32 + 0.5 - cy) / ry;
        if dy.abs() > 1.0 {
            continue;
        }
        let dx = rx * (1.0 - dy * dy).sqrt();
        for x in (cx - dx).round() as i64..(cx + dx).round() as i64 {
            melanger(img, x, y, c);
        }
    }
}

/// Polygone plein (règle pair-impair, balayage ligne par ligne).
pub fn polygone(img: &mut RgbaImage, pts: &[(f32, f32)], c: Rgba<u8>) {
    if pts.len() < 3 {
        return;
    }
    let (y0, y1) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    for y in y0.floor() as i64..=y1.ceil() as i64 {
        let yc = y as f32 + 0.5;
        let mut xs = Vec::new();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            if (a.1 <= yc) != (b.1 <= yc) {
                xs.push(a.0 + (yc - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        for p in xs.chunks_exact(2) {
            for x in p[0].round() as i64..p[1].round() as i64 {
                melanger(img, x, y, c);
            }
        }
    }
}

pub fn rectangle(img: &mut RgbaImage, x: f32, y: f32, l: f32, h: f32, c: Rgba<u8>) {
    polygone(img, &[(x, y), (x + l, y), (x + l, y + h), (x, y + h)], c);
}

/// Découpe un texte en lignes de largeur maximale `max`.
pub fn couper(f: &ab_glyph::FontVec, taille: f32, t: &str, max: f32) -> Vec<String> {
    let mut lignes = Vec::new();
    for para in t.lines() {
        let mut l = String::new();
        for mot in para.split_whitespace() {
            let essai = if l.is_empty() { mot.to_string() } else { format!("{l} {mot}") };
            if crate::creatif::largeur(f, taille, &essai) > max && !l.is_empty() {
                lignes.push(std::mem::replace(&mut l, mot.to_string()));
            } else {
                l = essai;
            }
        }
        lignes.push(l);
    }
    lignes
}

// ---------------------------------------------------------------------------------------------------
// 22 · Créateur de BD
// ---------------------------------------------------------------------------------------------------

pub const PL: u32 = 1240;
pub const PH: u32 = 1754;
const MARGE: f32 = 50.0;
const GOUTTIERE: f32 = 24.0;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Forme {
    Parole,
    Pensee,
    Cri,
    Recit,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Bulle {
    pub texte: String,
    pub forme: Forme,
    /// centre, relatif à la case (0..1)
    pub x: f32,
    pub y: f32,
    /// pointe de la queue, relative à la case
    pub qx: f32,
    pub qy: f32,
    pub taille: f32,
}

impl Default for Bulle {
    fn default() -> Self {
        Bulle { texte: "Salut !".into(), forme: Forme::Parole, x: 0.5, y: 0.25, qx: 0.4, qy: 0.6, taille: 34.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Perso {
    pub image: PathBuf,
    pub x: f32,
    pub echelle: f32,
    pub miroir: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Case {
    pub fond: Option<PathBuf>,
    pub couleur: [u8; 3],
    pub persos: Vec<Perso>,
    pub bulles: Vec<Bulle>,
}

impl Default for Case {
    fn default() -> Self {
        Case { fond: None, couleur: [235, 240, 250], persos: Vec::new(), bulles: Vec::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Page {
    pub disposition: usize,
    pub cases: Vec<Case>,
}

impl Default for Page {
    fn default() -> Self {
        Page { disposition: 3, cases: vec![Case::default(); 4] }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Bd {
    pub titre: String,
    pub pages: Vec<Page>,
}

/// Dispositions : nom, rangées (chaque rangée = nombre de cases).
pub const DISPOSITIONS: [(&str, &[usize]); 7] = [
    ("1 grande case", &[1]),
    ("2 cases", &[1, 1]),
    ("3 bandes", &[1, 1, 1]),
    ("4 cases", &[2, 2]),
    ("6 cases", &[2, 2, 2]),
    ("1 + 2", &[1, 2]),
    ("2 + 1 + 2", &[2, 1, 2]),
];

/// Rectangles (x, y, l, h) des cases d'une disposition.
pub fn rectangles(disposition: usize) -> Vec<(f32, f32, f32, f32)> {
    let rangees = DISPOSITIONS[disposition.min(DISPOSITIONS.len() - 1)].1;
    let haut = (PH as f32 - 2.0 * MARGE - GOUTTIERE * (rangees.len() - 1) as f32) / rangees.len() as f32;
    let mut v = Vec::new();
    for (r, &n) in rangees.iter().enumerate() {
        let larg = (PL as f32 - 2.0 * MARGE - GOUTTIERE * (n - 1) as f32) / n as f32;
        for c in 0..n {
            v.push((MARGE + c as f32 * (larg + GOUTTIERE), MARGE + r as f32 * (haut + GOUTTIERE), larg, haut));
        }
    }
    v
}

fn charger_image(p: &PathBuf, cache: &mut Vec<(PathBuf, RgbaImage)>) -> Option<RgbaImage> {
    if let Some((_, i)) = cache.iter().find(|(q, _)| q == p) {
        return Some(i.clone());
    }
    let i = image::open(p).ok()?.to_rgba8();
    // les grandes photos sont réduites pour aller vite
    let i = if i.width() > 1600 { image::imageops::thumbnail(&i, 1600, (1600.0 * i.height() as f32 / i.width() as f32) as u32) } else { i };
    cache.push((p.clone(), i.clone()));
    Some(i)
}

fn dessiner_bulle(img: &mut RgbaImage, b: &Bulle, (x, y, l, h): (f32, f32, f32, f32), f: Option<&ab_glyph::FontVec>) {
    let noir = Rgba([0, 0, 0, 255]);
    let fond = if b.forme == Forme::Recit { Rgba([255, 240, 170, 255]) } else { Rgba([255, 255, 255, 255]) };
    let (cx, cy) = (x + b.x * l, y + b.y * h);
    let max = l * 0.55;
    let lignes = match f {
        Some(f) => couper(f, b.taille, &b.texte, max),
        None => vec![b.texte.clone()],
    };
    let tl = lignes.iter().map(|t| f.map(|f| crate::creatif::largeur(f, b.taille, t)).unwrap_or(t.len() as f32 * b.taille * 0.5)).fold(0.0f32, f32::max);
    let th = lignes.len() as f32 * b.taille * 1.15;
    let (rx, ry) = ((tl / 2.0 + b.taille * 0.9).max(b.taille * 1.5), (th / 2.0 + b.taille * 0.7).max(b.taille));
    let (qx, qy) = (x + b.qx * l, y + b.qy * h);
    match b.forme {
        Forme::Parole | Forme::Cri => {
            // queue : triangle du bord de la bulle vers la pointe
            let ang = (qy - cy).atan2(qx - cx);
            let (bx, by) = (cx + ang.cos() * rx * 0.7, cy + ang.sin() * ry * 0.7);
            let n = (-(ang.sin()), ang.cos());
            let largeur = b.taille * 0.6;
            let tri = |e: f32| vec![(bx + n.0 * (largeur + e), by + n.1 * (largeur + e)), (bx - n.0 * (largeur + e), by - n.1 * (largeur + e)), (qx, qy)];
            if b.forme == Forme::Cri {
                let pts = |e: f32| (0..24).map(|k| {
                    let a = k as f32 / 24.0 * std::f32::consts::TAU;
                    let r = if k % 2 == 0 { 1.25 } else { 1.0 };
                    (cx + a.cos() * (rx * r + e), cy + a.sin() * (ry * r + e))
                }).collect::<Vec<_>>();
                polygone(img, &tri(4.0), noir);
                polygone(img, &pts(4.0), noir);
                polygone(img, &tri(0.0), fond);
                polygone(img, &pts(0.0), fond);
            } else {
                polygone(img, &tri(4.0), noir);
                ellipse(img, cx, cy, rx + 4.0, ry + 4.0, noir);
                polygone(img, &tri(0.0), fond);
                ellipse(img, cx, cy, rx, ry, fond);
            }
        }
        Forme::Pensee => {
            for k in 0..3 {
                let t = 0.35 + k as f32 * 0.22;
                let (px, py) = (qx + (cx - qx) * t, qy + (cy - qy) * t);
                let r = b.taille * (0.25 + k as f32 * 0.15);
                ellipse(img, px, py, r + 3.0, r + 3.0, noir);
                ellipse(img, px, py, r, r, fond);
            }
            // nuage : plusieurs bosses autour de l'ellipse
            for k in 0..10 {
                let a = k as f32 / 10.0 * std::f32::consts::TAU;
                ellipse(img, cx + a.cos() * rx * 0.85, cy + a.sin() * ry * 0.85, rx * 0.35 + 4.0, ry * 0.45 + 4.0, noir);
            }
            ellipse(img, cx, cy, rx + 4.0, ry + 4.0, noir);
            for k in 0..10 {
                let a = k as f32 / 10.0 * std::f32::consts::TAU;
                ellipse(img, cx + a.cos() * rx * 0.85, cy + a.sin() * ry * 0.85, rx * 0.35, ry * 0.45, fond);
            }
            ellipse(img, cx, cy, rx, ry, fond);
        }
        Forme::Recit => {
            let (l2, h2) = (tl + b.taille, th + b.taille * 0.6);
            rectangle(img, cx - l2 / 2.0 - 3.0, cy - h2 / 2.0 - 3.0, l2 + 6.0, h2 + 6.0, noir);
            rectangle(img, cx - l2 / 2.0, cy - h2 / 2.0, l2, h2, fond);
        }
    }
    if let Some(f) = f {
        let y0 = cy - th / 2.0;
        for (k, t) in lignes.iter().enumerate() {
            let w = crate::creatif::largeur(f, b.taille, t);
            crate::creatif::ecrire(img, f, b.taille, cx - w / 2.0, y0 + k as f32 * b.taille * 1.15, t, [20, 20, 20], None);
        }
    }
}

pub fn dessiner_page(p: &Page, f: Option<&ab_glyph::FontVec>, cache: &mut Vec<(PathBuf, RgbaImage)>) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(PL, PH, Rgba([255, 255, 255, 255]));
    for (k, r) in rectangles(p.disposition).into_iter().enumerate() {
        let c = p.cases.get(k).cloned().unwrap_or_default();
        let (x, y, l, h) = r;
        let mut case = RgbaImage::from_pixel(l as u32, h as u32, Rgba([c.couleur[0], c.couleur[1], c.couleur[2], 255]));
        if let Some(i) = c.fond.as_ref().and_then(|p| charger_image(p, cache)) {
            case = crate::createur::couvrir(&i, l as u32, h as u32);
        }
        for pe in &c.persos {
            if let Some(i) = charger_image(&pe.image, cache) {
                let ph = (h * pe.echelle).max(1.0) as u32;
                let pl = ((i.width() as f32 * ph as f32 / i.height() as f32).max(1.0)) as u32;
                let mut r = image::imageops::resize(&i, pl, ph, image::imageops::FilterType::Triangle);
                if pe.miroir {
                    r = image::imageops::flip_horizontal(&r);
                }
                image::imageops::overlay(&mut case, &r, (pe.x * l - pl as f32 / 2.0) as i64, h as i64 - ph as i64);
            }
        }
        image::imageops::overlay(&mut img, &case, x as i64, y as i64);
        // cadre
        for e in 0..5 {
            let e = e as f32;
            rectangle(&mut img, x - e, y - e, l + 2.0 * e, 1.0, Rgba([0, 0, 0, 255]));
            rectangle(&mut img, x - e, y + h + e - 1.0, l + 2.0 * e, 1.0, Rgba([0, 0, 0, 255]));
            rectangle(&mut img, x - e, y - e, 1.0, h + 2.0 * e, Rgba([0, 0, 0, 255]));
            rectangle(&mut img, x + l + e - 1.0, y - e, 1.0, h + 2.0 * e, Rgba([0, 0, 0, 255]));
        }
        for b in &c.bulles {
            dessiner_bulle(&mut img, b, r, f);
        }
    }
    img
}

/// PDF minimal : une image JPEG par page.
pub fn pdf_images(pages: &[RgbaImage]) -> Vec<u8> {
    let mut objets: Vec<Vec<u8>> = Vec::new();
    let n = pages.len();
    // 1 : catalogue, 2 : pages, puis pour chaque page : page, contenu, image
    objets.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..n).map(|k| format!("{} 0 R", 3 + k * 3)).collect();
    objets.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
    for (k, p) in pages.iter().enumerate() {
        let (w, h) = (595.0f32, 595.0 * p.height() as f32 / p.width() as f32);
        let base = 3 + k * 3;
        objets.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.0} {h:.0}] /Contents {} 0 R /Resources << /XObject << /Im{k} {} 0 R >> >> >>", base + 1, base + 2).into_bytes());
        let flux = format!("q {w:.0} 0 0 {h:.0} 0 0 cm /Im{k} Do Q");
        objets.push(format!("<< /Length {} >>\nstream\n{flux}\nendstream", flux.len()).into_bytes());
        let mut jpg = Vec::new();
        let rgb = image::DynamicImage::ImageRgba8(p.clone()).to_rgb8();
        let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 88).encode_image(&rgb);
        let mut o = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", p.width(), p.height(), jpg.len()).into_bytes();
        o.extend_from_slice(&jpg);
        o.extend_from_slice(b"\nendstream");
        objets.push(o);
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut pos = Vec::new();
    for (k, o) in objets.iter().enumerate() {
        pos.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", k + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objets.len() + 1).as_bytes());
    for p in pos {
        out.extend_from_slice(format!("{p:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objets.len() + 1).as_bytes());
    out
}

pub struct CreateurBd {
    bd: Bd,
    page: usize,
    case: usize,
    bulle: Option<usize>,
    police: Option<ab_glyph::FontVec>,
    cache: Vec<(PathBuf, RgbaImage)>,
    apercu: Option<egui::TextureHandle>,
    sale: bool,
    message: String,
}

impl CreateurBd {
    pub fn new() -> Self {
        let mut bd: Bd = charger("bd");
        if bd.pages.is_empty() {
            let mut p = Page::default();
            p.cases[0].bulles.push(Bulle::default());
            bd.pages.push(p);
        }
        CreateurBd { bd, page: 0, case: 0, bulle: Some(0), police: None, cache: Vec::new(), apercu: None, sale: true, message: String::new() }
    }
}

fn choisir_image() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file()
}

impl Outil for CreateurBd {
    fn nom(&self) -> &'static str {
        "📖 Créateur de BD"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.police.is_none() {
            self.police = crate::creatif::police(&["comicbd.ttf", "comic.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf", "LiberationSans-Bold.ttf", "NotoSans-Bold.ttf"]);
        }
        let avant = self.bd.clone();
        ui.heading("Créateur de BD");
        ui.horizontal_wrapped(|ui| {
            ui.label("Page");
            for k in 0..self.bd.pages.len() {
                if ui.selectable_label(self.page == k, (k + 1).to_string()).clicked() {
                    self.page = k;
                    self.case = 0;
                    self.bulle = None;
                    self.sale = true;
                }
            }
            if ui.small_button("➕ page").clicked() {
                self.bd.pages.push(Page::default());
                self.page = self.bd.pages.len() - 1;
                self.sale = true;
            }
            if self.bd.pages.len() > 1 && ui.small_button("🗑 page").clicked() {
                self.bd.pages.remove(self.page);
                self.page = self.page.saturating_sub(1);
                self.sale = true;
            }
            ui.separator();
            let p = &mut self.bd.pages[self.page];
            egui::ComboBox::from_id_salt("dispo_bd").selected_text(DISPOSITIONS[p.disposition].0).show_ui(ui, |ui| {
                for (k, (n, _)) in DISPOSITIONS.iter().enumerate() {
                    ui.selectable_value(&mut p.disposition, k, *n);
                }
            });
            let n = rectangles(p.disposition).len();
            p.cases.resize(n.max(p.cases.len()), Case::default());
        });
        let rects = rectangles(self.bd.pages[self.page].disposition);
        self.case = self.case.min(rects.len() - 1);
        ui.horizontal_top(|ui| {
            // aperçu cliquable
            let echelle = (ui.available_height().max(500.0) / PH as f32).min(0.42);
            let taille = egui::vec2(PL as f32 * echelle, PH as f32 * echelle);
            if self.sale {
                self.sale = false;
                let img = dessiner_page(&self.bd.pages[self.page], self.police.as_ref(), &mut self.cache);
                let petite = image::imageops::thumbnail(&img, taille.x as u32, taille.y as u32);
                self.apercu = Some(texture(ui.ctx(), "bd_page", &petite, false));
            }
            let (r, rep) = ui.allocate_exact_size(taille, Sense::click_and_drag());
            if let Some(t) = &self.apercu {
                ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let vers_ecran = |x: f32, y: f32| r.min + egui::vec2(x * echelle, y * echelle);
            let (cx, cy, cl, ch) = rects[self.case];
            ui.painter().rect_stroke(egui::Rect::from_min_max(vers_ecran(cx, cy), vers_ecran(cx + cl, cy + ch)), 0.0, egui::Stroke::new(3.0_f32, Color32::from_rgb(255, 140, 0)), egui::StrokeKind::Outside);
            if let Some(p) = rep.interact_pointer_pos() {
                let (px, py) = ((p.x - r.min.x) / echelle, (p.y - r.min.y) / echelle);
                if rep.clicked() || rep.drag_started() {
                    if let Some(k) = rects.iter().position(|&(x, y, l, h)| px >= x && px <= x + l && py >= y && py <= y + h) {
                        if k != self.case {
                            self.case = k;
                            self.bulle = None;
                        }
                    }
                }
                // glisser déplace la bulle choisie (clic droit + glisser : sa pointe)
                if rep.dragged() {
                    let (x, y, l, h) = rects[self.case];
                    let case = &mut self.bd.pages[self.page].cases[self.case];
                    if let Some(b) = self.bulle.and_then(|k| case.bulles.get_mut(k)) {
                        let (rx, ry) = (((px - x) / l).clamp(0.0, 1.0), ((py - y) / h).clamp(0.0, 1.0));
                        if ui.input(|i| i.pointer.secondary_down()) {
                            (b.qx, b.qy) = (rx, ry);
                        } else {
                            (b.x, b.y) = (rx, ry);
                        }
                    }
                }
            }
            if rep.drag_stopped() {
                self.sale = true;
            }
            ui.vertical(|ui| {
                ui.set_max_width(320.0);
                ui.strong(format!("Case {}", self.case + 1));
                let case = &mut self.bd.pages[self.page].cases[self.case];
                ui.horizontal_wrapped(|ui| {
                    ui.color_edit_button_srgb(&mut case.couleur);
                    if ui.button("🖼 Décor…").clicked() {
                        case.fond = choisir_image();
                    }
                    if case.fond.is_some() && ui.small_button("✖ décor").clicked() {
                        case.fond = None;
                    }
                });
                if ui.button("🧍 Ajouter un personnage (PNG détouré)…").clicked() {
                    if let Some(p) = choisir_image() {
                        case.persos.push(Perso { image: p, x: 0.3 + 0.4 * (case.persos.len() % 2) as f32, echelle: 0.75, miroir: case.persos.len() % 2 == 1 });
                    }
                }
                let mut retirer = None;
                for (k, p) in case.persos.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("🧍{}", k + 1));
                        ui.add(egui::Slider::new(&mut p.x, 0.0..=1.0).show_value(false));
                        ui.add(egui::Slider::new(&mut p.echelle, 0.2..=1.2).show_value(false));
                        ui.checkbox(&mut p.miroir, "↔");
                        if ui.small_button("✖").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
                if let Some(k) = retirer {
                    case.persos.remove(k);
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong("Bulles");
                    if ui.small_button("➕").clicked() {
                        case.bulles.push(Bulle::default());
                        self.bulle = Some(case.bulles.len() - 1);
                    }
                });
                for k in 0..case.bulles.len() {
                    let t: String = case.bulles[k].texte.chars().take(18).collect();
                    if ui.selectable_label(self.bulle == Some(k), format!("💬 {t}")).clicked() {
                        self.bulle = Some(k);
                    }
                }
                if let Some(k) = self.bulle.filter(|k| *k < case.bulles.len()) {
                    let b = &mut case.bulles[k];
                    ui.add(egui::TextEdit::multiline(&mut b.texte).desired_rows(2).desired_width(300.0));
                    ui.horizontal_wrapped(|ui| {
                        for (f, n) in [(Forme::Parole, "💬 Parole"), (Forme::Pensee, "💭 Pensée"), (Forme::Cri, "💥 Cri"), (Forme::Recit, "📜 Récit")] {
                            ui.selectable_value(&mut b.forme, f, n);
                        }
                    });
                    ui.add(egui::Slider::new(&mut b.taille, 18.0..=70.0).text("taille"));
                    ui.weak("Glisse dans la case pour placer la bulle ; clic droit + glisser pour la pointe.");
                    if ui.small_button("🗑 Supprimer la bulle").clicked() {
                        case.bulles.remove(k);
                        self.bulle = None;
                    }
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.button("💾 Page en PNG…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(format!("bd_page{}.png", self.page + 1)).save_file() {
                            let img = dessiner_page(&self.bd.pages[self.page], self.police.as_ref(), &mut self.cache);
                            self.message = img.save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                        }
                    }
                    if ui.button("📄 Toute la BD en PDF…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("ma_bd.pdf").save_file() {
                            let pages: Vec<RgbaImage> = self.bd.pages.clone().iter().map(|pg| dessiner_page(pg, self.police.as_ref(), &mut self.cache)).collect();
                            self.message = std::fs::write(&p, pdf_images(&pages)).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                        }
                    }
                });
                ui.label(&self.message);
            });
        });
        if self.bd != avant {
            enregistrer("bd", &self.bd);
            if !ui.input(|i| i.pointer.any_down()) {
                self.sale = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 25 · Créateur de polices (.ttf)
// ---------------------------------------------------------------------------------------------------

pub const COLS: usize = 8;
pub const LIGNES: usize = 12; // 9 au-dessus de la ligne de base, 3 en dessous
pub const BASE: usize = 9;
const UNITE: i16 = 100;
pub const CARACTERES: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.,;:!?'\"-+=()/&@#%*éèêàâçùô";

pub type Grille = Vec<[bool; COLS]>;

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct MaPolice {
    pub nom: String,
    pub glyphes: Vec<(char, Grille)>,
}

/// Rangées de pixels d'une police installée, pour partir de quelque chose.
pub fn depuis_police(f: &ab_glyph::FontVec, c: char) -> Grille {
    use ab_glyph::{Font, ScaleFont};
    let echelle = ab_glyph::PxScale::from(LIGNES as f32 * 1.0);
    let sf = f.as_scaled(echelle);
    let mut g = vec![[false; COLS]; LIGNES];
    // rendu en grand puis réduit (moyenne) pour un trait propre
    let k = 8.0;
    let gros = ab_glyph::PxScale::from(LIGNES as f32 * k);
    let glyphe = f.glyph_id(c).with_scale_and_position(gros, ab_glyph::point(k, sf.ascent() * k));
    let mut somme = vec![[0f32; COLS]; LIGNES];
    if let Some(o) = f.outline_glyph(glyphe) {
        let b = o.px_bounds();
        o.draw(|x, y, v| {
            let (px, py) = ((b.min.x + x as f32) / k, (b.min.y + y as f32) / k);
            // ligne de base de la grille : ascent de la police ramené à BASE
            let py = py - (sf.ascent() - BASE as f32);
            if px >= 0.0 && py >= 0.0 && (px as usize) < COLS && (py as usize) < LIGNES {
                somme[py as usize][px as usize] += v / (k * k);
            }
        });
    }
    for y in 0..LIGNES {
        for x in 0..COLS {
            g[y][x] = somme[y][x] > 0.4;
        }
    }
    g
}

/// Rectangles de pixels allumés (fusionnés par rangées) : (x0, x1, y) en unités de grille.
fn traits(g: &Grille) -> Vec<(usize, usize, usize)> {
    let mut v = Vec::new();
    for (y, l) in g.iter().enumerate() {
        let mut x = 0;
        while x < COLS {
            if l[x] {
                let d = x;
                while x < COLS && l[x] {
                    x += 1;
                }
                v.push((d, x, y));
            } else {
                x += 1;
            }
        }
    }
    v
}

fn largeur_avance(g: &Grille) -> u16 {
    let max = g.iter().flat_map(|l| l.iter().enumerate().filter(|(_, v)| **v).map(|(x, _)| x)).max();
    match max {
        Some(m) => ((m + 2) as i16 * UNITE) as u16,
        None => (4 * UNITE) as u16,
    }
}

fn glyf(g: &Grille) -> Vec<u8> {
    let t = traits(g);
    if t.is_empty() {
        return Vec::new();
    }
    // chaque trait = un rectangle dans le sens des aiguilles (y vers le haut)
    let mut pts: Vec<(i16, i16)> = Vec::new();
    let mut fins = Vec::new();
    for (x0, x1, y) in &t {
        let (gx0, gx1) = (*x0 as i16 * UNITE, *x1 as i16 * UNITE);
        let haut = (BASE as i16 - *y as i16) * UNITE;
        let bas = haut - UNITE;
        pts.extend([(gx0, bas), (gx0, haut), (gx1, haut), (gx1, bas)]);
        fins.push(pts.len() as u16 - 1);
    }
    let (xmin, xmax) = pts.iter().fold((i16::MAX, i16::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (ymin, ymax) = pts.iter().fold((i16::MAX, i16::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    let mut o = Vec::new();
    o.extend((fins.len() as i16).to_be_bytes());
    for v in [xmin, ymin, xmax, ymax] {
        o.extend(v.to_be_bytes());
    }
    for f in &fins {
        o.extend(f.to_be_bytes());
    }
    o.extend(0u16.to_be_bytes()); // pas d'instructions
    o.extend(std::iter::repeat_n(1u8, pts.len())); // points sur la courbe, coordonnées sur 2 octets
    let mut prec = (0i16, 0i16);
    for p in &pts {
        o.extend((p.0 - prec.0).to_be_bytes());
        prec.0 = p.0;
    }
    for p in &pts {
        o.extend((p.1 - prec.1).to_be_bytes());
        prec.1 = p.1;
    }
    while o.len() % 4 != 0 {
        o.push(0);
    }
    o
}

fn somme_controle(d: &[u8]) -> u32 {
    let mut s = 0u32;
    for c in d.chunks(4) {
        let mut b = [0u8; 4];
        b[..c.len()].copy_from_slice(c);
        s = s.wrapping_add(u32::from_be_bytes(b));
    }
    s
}

fn chaine_nom(id: u16, texte: &str, data: &mut Vec<u8>, entrees: &mut Vec<[u16; 6]>) {
    let utf16: Vec<u8> = texte.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
    entrees.push([3, 1, 0x409, id, utf16.len() as u16, data.len() as u16]);
    data.extend(utf16);
}

/// Fabrique un fichier TrueType (.ttf) à partir des grilles.
pub fn ttf(nom: &str, glyphes: &[(char, Grille)]) -> Vec<u8> {
    let nom: String = nom.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').collect::<String>().trim().to_string();
    let nom = if nom.is_empty() { "Ma police".to_string() } else { nom };
    let mut tous: Vec<(char, Grille)> = glyphes.iter().filter(|(c, _)| *c != ' ').cloned().collect();
    tous.sort_by_key(|(c, _)| *c);
    tous.dedup_by_key(|(c, _)| *c);
    // glyphe 0 : .notdef (un cadre), 1 : espace
    let mut cadre = vec![[false; COLS]; LIGNES];
    for (y, l) in cadre.iter_mut().enumerate().take(BASE).skip(2) {
        for (x, v) in l.iter_mut().enumerate().take(6).skip(1) {
            *v = y == 2 || y == BASE - 1 || x == 1 || x == 5;
        }
    }
    let mut grilles: Vec<Grille> = vec![cadre, vec![[false; COLS]; LIGNES]];
    grilles.extend(tous.iter().map(|(_, g)| g.clone()));
    let n = grilles.len() as u16;
    let mut glyf_t = Vec::new();
    let mut loca = Vec::new();
    for g in &grilles {
        loca.extend((glyf_t.len() as u32).to_be_bytes());
        glyf_t.extend(glyf(g));
    }
    loca.extend((glyf_t.len() as u32).to_be_bytes());
    let avances: Vec<u16> = grilles.iter().enumerate().map(|(k, g)| if k == 1 { 4 * UNITE as u16 } else { largeur_avance(g) }).collect();
    let mut hmtx = Vec::new();
    for (k, g) in grilles.iter().enumerate() {
        hmtx.extend(avances[k].to_be_bytes());
        let lsb = traits(g).iter().map(|t| t.0 as i16 * UNITE).min().unwrap_or(0);
        hmtx.extend(lsb.to_be_bytes());
    }
    // cmap format 4 : un segment par caractère
    let mut seg: Vec<(u16, u16)> = vec![(' ' as u16, 1)];
    for (k, (c, _)) in tous.iter().enumerate() {
        if (*c as u32) < 0xFFFF {
            seg.push((*c as u32 as u16, k as u16 + 2));
        }
    }
    seg.sort();
    seg.push((0xFFFF, 0));
    let sc = seg.len() as u16;
    let mut f4 = Vec::new();
    let longueur = 16 + 8 * sc;
    for v in [4u16, longueur, 0, sc * 2] {
        f4.extend(v.to_be_bytes());
    }
    let puiss = 2u16.pow((sc as f32).log2().floor() as u32);
    for v in [puiss * 2, (puiss as f32).log2() as u16, sc * 2 - puiss * 2] {
        f4.extend(v.to_be_bytes());
    }
    for (c, _) in &seg {
        f4.extend(c.to_be_bytes());
    }
    f4.extend(0u16.to_be_bytes());
    for (c, _) in &seg {
        f4.extend(c.to_be_bytes());
    }
    for (c, g) in &seg {
        let delta = if *c == 0xFFFF { 1u16 } else { g.wrapping_sub(*c) };
        f4.extend(delta.to_be_bytes());
    }
    for _ in &seg {
        f4.extend(0u16.to_be_bytes());
    }
    let mut cmap = Vec::new();
    for v in [0u16, 1, 3, 1] {
        cmap.extend(v.to_be_bytes());
    }
    cmap.extend(12u32.to_be_bytes());
    cmap.extend(f4);
    let (asc, desc) = (BASE as i16 * UNITE, -((LIGNES - BASE) as i16) * UNITE);
    let em = LIGNES as u16 * UNITE as u16;
    let mut head = Vec::new();
    head.extend(0x0001_0000u32.to_be_bytes());
    head.extend(0x0001_0000u32.to_be_bytes());
    head.extend(0u32.to_be_bytes()); // checkSumAdjustment, rempli à la fin
    head.extend(0x5F0F_3CF5u32.to_be_bytes());
    head.extend(0x000Bu16.to_be_bytes());
    head.extend(em.to_be_bytes());
    // dates : secondes depuis 1904
    let date = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64 + 2_082_844_800;
    head.extend(date.to_be_bytes());
    head.extend(date.to_be_bytes());
    for v in [0i16, desc, (COLS as i16 + 1) * UNITE, asc] {
        head.extend(v.to_be_bytes());
    }
    for v in [0u16, 8, 2] {
        head.extend(v.to_be_bytes());
    }
    head.extend(1i16.to_be_bytes()); // loca longue
    head.extend(0i16.to_be_bytes());
    let mut hhea = Vec::new();
    hhea.extend(0x0001_0000u32.to_be_bytes());
    for v in [asc, desc, 0i16] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(avances.iter().max().copied().unwrap_or(0).to_be_bytes());
    for v in [0i16, 0, (COLS as i16) * UNITE, 1, 0, 0, 0, 0, 0, 0, 0] {
        hhea.extend(v.to_be_bytes());
    }
    hhea.extend(n.to_be_bytes());
    let mut maxp = Vec::new();
    maxp.extend(0x0001_0000u32.to_be_bytes());
    maxp.extend(n.to_be_bytes());
    let max_pts = grilles.iter().map(|g| traits(g).len() * 4).max().unwrap_or(0) as u16;
    let max_ctr = grilles.iter().map(|g| traits(g).len()).max().unwrap_or(0) as u16;
    for v in [max_pts, max_ctr, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
        maxp.extend(v.to_be_bytes());
    }
    let mut os2 = Vec::new();
    os2.extend(4u16.to_be_bytes());
    let moyenne = (avances.iter().map(|a| *a as u32).sum::<u32>() / avances.len() as u32) as i16;
    for v in [moyenne, 400, 5, 0] {
        os2.extend(v.to_be_bytes());
    }
    for v in [UNITE * 6, UNITE * 7, 0, UNITE * 2, UNITE * 6, UNITE * 7, 0, UNITE * 4, UNITE / 2, UNITE * 5, 0] {
        os2.extend(v.to_be_bytes());
    }
    os2.extend([0u8; 10]); // panose
    os2.extend([0u8; 16]); // plages unicode
    os2.extend(*b"OUTL");
    os2.extend(0x0040u16.to_be_bytes()); // Regular
    os2.extend(32u16.to_be_bytes());
    os2.extend((seg.iter().rev().nth(1).map(|s| s.0).unwrap_or(32)).to_be_bytes());
    for v in [asc, desc, 0] {
        os2.extend(v.to_be_bytes());
    }
    os2.extend((asc as u16).to_be_bytes());
    os2.extend((-desc as u16).to_be_bytes());
    os2.extend([0u8; 8]); // pages de code
    for v in [UNITE * 6, UNITE * 7, 0, 32, 0] {
        os2.extend(v.to_be_bytes());
    }
    let mut data = Vec::new();
    let mut entrees = Vec::new();
    let ps: String = nom.chars().filter(|c| *c != ' ').collect();
    for (id, t) in [(1u16, nom.as_str()), (2, "Regular"), (3, &format!("{nom} Outils")), (4, nom.as_str()), (5, "Version 1.0"), (6, ps.as_str())] {
        chaine_nom(id, t, &mut data, &mut entrees);
    }
    let mut name = Vec::new();
    for v in [0u16, entrees.len() as u16, 6 + 12 * entrees.len() as u16] {
        name.extend(v.to_be_bytes());
    }
    for e in &entrees {
        for v in e {
            name.extend(v.to_be_bytes());
        }
    }
    name.extend(data);
    let mut post = Vec::new();
    post.extend(0x0003_0000u32.to_be_bytes());
    post.extend([0u8; 28]);
    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"OS/2", os2), (b"cmap", cmap), (b"glyf", glyf_t), (b"head", head), (b"hhea", hhea), (b"hmtx", hmtx), (b"loca", loca), (b"maxp", maxp), (b"name", name), (b"post", post)];
    tables.sort_by_key(|t| *t.0);
    let nt = tables.len() as u16;
    let mut out = Vec::new();
    out.extend(0x0001_0000u32.to_be_bytes());
    let puiss = 2u16.pow((nt as f32).log2().floor() as u32);
    for v in [nt, puiss * 16, (puiss as f32).log2() as u16, nt * 16 - puiss * 16] {
        out.extend(v.to_be_bytes());
    }
    let mut decal = 12 + 16 * tables.len();
    let mut corps = Vec::new();
    let mut pos_head = 0;
    for (tag, d) in &tables {
        out.extend(*tag);
        out.extend(somme_controle(d).to_be_bytes());
        out.extend((decal as u32).to_be_bytes());
        out.extend((d.len() as u32).to_be_bytes());
        if *tag == b"head" {
            pos_head = decal;
        }
        corps.extend(d);
        while corps.len() % 4 != 0 {
            corps.push(0);
        }
        decal = 12 + 16 * tables.len() + corps.len();
    }
    out.extend(corps);
    let ajust = 0xB1B0_AFBAu32.wrapping_sub(somme_controle(&out));
    out[pos_head + 8..pos_head + 12].copy_from_slice(&ajust.to_be_bytes());
    out
}

pub struct CreateurPolice {
    p: MaPolice,
    lettre: char,
    essai: String,
    apercu: Option<(String, egui::TextureHandle)>,
    message: String,
    dessin: Option<bool>,
}

impl CreateurPolice {
    pub fn new() -> Self {
        let mut p: MaPolice = charger("ma_police");
        if p.nom.is_empty() {
            p.nom = "Ma police".into();
        }
        CreateurPolice { p, lettre: 'A', essai: "Bonjour le monde !".into(), apercu: None, message: String::new(), dessin: None }
    }

    fn grille(&mut self, c: char) -> &mut Grille {
        if !self.p.glyphes.iter().any(|g| g.0 == c) {
            self.p.glyphes.push((c, vec![[false; COLS]; LIGNES]));
        }
        &mut self.p.glyphes.iter_mut().find(|g| g.0 == c).unwrap().1
    }
}

/// Aperçu d'un texte écrit avec les grilles (pixels de `z` px).
pub fn rendu_texte(p: &MaPolice, t: &str, z: u32) -> RgbaImage {
    let vide = vec![[false; COLS]; LIGNES];
    let mut x = 0u32;
    let largeurs: Vec<u32> = t.chars().map(|c| p.glyphes.iter().find(|g| g.0 == c).map(|g| largeur_avance(&g.1) as u32 / UNITE as u32).unwrap_or(4)).collect();
    let total = largeurs.iter().sum::<u32>().max(1);
    let mut img = RgbaImage::from_pixel(total * z + 2 * z, LIGNES as u32 * z, Rgba([255, 255, 255, 255]));
    for (c, l) in t.chars().zip(largeurs) {
        let g = p.glyphes.iter().find(|g| g.0 == c).map(|g| &g.1).unwrap_or(&vide);
        for (yy, ligne) in g.iter().enumerate() {
            for (xx, v) in ligne.iter().enumerate() {
                if *v {
                    for dy in 0..z {
                        for dx in 0..z {
                            img.put_pixel(z + (x + xx as u32) * z + dx, yy as u32 * z + dy, Rgba([20, 20, 30, 255]));
                        }
                    }
                }
            }
        }
        x += l;
    }
    img
}

impl Outil for CreateurPolice {
    fn nom(&self) -> &'static str {
        "🔤 Créateur de polices"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.p.clone();
        ui.heading("Créateur de polices");
        ui.weak("Dessine chaque lettre en pixels : Outils fabrique un vrai fichier .ttf, à installer et à utiliser dans Word, Photoshop, tes miniatures…");
        ui.horizontal_wrapped(|ui| {
            ui.label("Nom");
            ui.add(egui::TextEdit::singleline(&mut self.p.nom).desired_width(160.0));
            ui.menu_button("✨ Partir d'une police installée", |ui| {
                for (n, f) in [("Arial", "arial.ttf"), ("Arial gras", "arialbd.ttf"), ("Comic Sans", "comic.ttf"), ("Verdana", "verdana.ttf"), ("DejaVu Sans", "DejaVuSans.ttf"), ("Liberation Sans", "LiberationSans-Regular.ttf")] {
                    if ui.button(n).clicked() {
                        if let Some(fv) = crate::creatif::police(&[f]) {
                            self.p.glyphes = CARACTERES.chars().map(|c| (c, depuis_police(&fv, c))).collect();
                            self.message = format!("Lettres tirées de {n} : retouche-les à ta façon.");
                        } else {
                            self.message = format!("{n} n'est pas installée ici.");
                        }
                        ui.close_menu();
                    }
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            for c in CARACTERES.chars() {
                let fait = self.p.glyphes.iter().any(|g| g.0 == c && g.1.iter().any(|l| l.iter().any(|v| *v)));
                let t = RichText::new(c.to_string()).monospace();
                if ui.selectable_label(self.lettre == c, if fait { t.strong() } else { t.weak() }).clicked() {
                    self.lettre = c;
                }
            }
        });
        ui.horizontal_top(|ui| {
            let z = 30.0;
            let lettre = self.lettre;
            let g = self.grille(lettre).clone();
            let (r, rep) = ui.allocate_exact_size(egui::vec2(COLS as f32 * z, LIGNES as f32 * z), Sense::click_and_drag());
            let p = ui.painter_at(r);
            for y in 0..LIGNES {
                for x in 0..COLS {
                    let case = egui::Rect::from_min_size(r.min + egui::vec2(x as f32 * z, y as f32 * z), egui::vec2(z, z)).shrink(1.0);
                    let fond = if g[y][x] { Color32::from_gray(235) } else if y >= BASE { Color32::from_rgb(40, 36, 52) } else { Color32::from_gray(45) };
                    p.rect_filled(case, 2.0, fond);
                }
            }
            p.line_segment([r.min + egui::vec2(0.0, BASE as f32 * z), r.min + egui::vec2(r.width(), BASE as f32 * z)], egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 120, 80)));
            if let Some(pos) = rep.interact_pointer_pos() {
                let (x, y) = (((pos.x - r.min.x) / z) as usize, ((pos.y - r.min.y) / z) as usize);
                if x < COLS && y < LIGNES {
                    let mode = *self.dessin.get_or_insert(!g[y][x]);
                    self.grille(lettre)[y][x] = mode;
                }
            }
            if !rep.is_pointer_button_down_on() {
                self.dessin = None;
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("Lettre « {} »", self.lettre)).size(20.0));
                ui.weak("La ligne orange est la ligne d'écriture : les jambes de g, p, q, y descendent dessous.");
                ui.horizontal(|ui| {
                    if ui.button("🧹 Effacer").clicked() {
                        *self.grille(lettre) = vec![[false; COLS]; LIGNES];
                    }
                    if ui.button("⬅").on_hover_text("Décaler à gauche").clicked() {
                        for l in self.grille(lettre).iter_mut() {
                            l.rotate_left(1);
                        }
                    }
                    if ui.button("➡").on_hover_text("Décaler à droite").clicked() {
                        for l in self.grille(lettre).iter_mut() {
                            l.rotate_right(1);
                        }
                    }
                });
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut self.essai).desired_width(280.0));
                let cle = format!("{}|{:?}", self.essai, self.p.glyphes.len());
                if self.apercu.as_ref().is_none_or(|(k, _)| *k != cle) || self.p != avant {
                    let img = rendu_texte(&self.p, &self.essai, 3);
                    self.apercu = Some((cle, texture(ui.ctx(), "police_apercu", &img, true)));
                }
                if let Some((_, t)) = &self.apercu {
                    ui.add(egui::Image::new(t).max_width(ui.available_width()));
                }
                ui.separator();
                if ui.button("💾 Créer le fichier .ttf…").clicked() {
                    let n: String = self.p.nom.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect();
                    if let Some(p) = rfd::FileDialog::new().add_filter("Police TrueType", &["ttf"]).set_file_name(format!("{}.ttf", n.trim())).save_file() {
                        self.message = std::fs::write(&p, ttf(&self.p.nom, &self.p.glyphes)).map(|_| format!("✔ {} — double-clique dessus puis « Installer ».", p.display())).unwrap_or_else(|e| e.to_string());
                    }
                }
                ui.label(&self.message);
            });
        });
        if self.p != avant {
            enregistrer("ma_police", &self.p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bd() {
        assert_eq!(rectangles(3).len(), 4);
        assert_eq!(rectangles(6).len(), 5);
        let r = rectangles(0)[0];
        assert!((r.2 - (PL as f32 - 2.0 * MARGE)).abs() < 0.5);
        let mut p = Page::default();
        p.cases[0].bulles.push(Bulle::default());
        p.cases[1].bulles.push(Bulle { forme: Forme::Pensee, ..Default::default() });
        p.cases[2].bulles.push(Bulle { forme: Forme::Cri, ..Default::default() });
        p.cases[3].bulles.push(Bulle { forme: Forme::Recit, ..Default::default() });
        let img = dessiner_page(&p, None, &mut Vec::new());
        assert_eq!((img.width(), img.height()), (PL, PH));
        // le centre de la bulle est blanc, le cadre noir
        let (x, y, l, h) = rectangles(3)[0];
        assert_eq!(img.get_pixel((x + 0.5 * l) as u32, (y + 0.25 * h) as u32)[0], 255);
        assert_eq!(img.get_pixel(x as u32 - 2, (y + 10.0) as u32)[0], 0);
        let pdf = pdf_images(&[img.clone(), img]);
        assert!(pdf.starts_with(b"%PDF") && pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn police_ttf() {
        use ab_glyph::Font;
        let mut a = vec![[false; COLS]; LIGNES];
        for (y, l) in a.iter_mut().enumerate().take(BASE).skip(2) {
            l[1] = true;
            l[5] = true;
            if y == 2 || y == 5 {
                for v in l.iter_mut().take(6).skip(1) {
                    *v = true;
                }
            }
        }
        let mut g = vec![[false; COLS]; LIGNES];
        g[10][2] = true;
        let f = ttf("Test Pixel", &[('A', a), ('g', g), ('é', vec![[false; COLS]; LIGNES])]);
        let fv = ab_glyph::FontVec::try_from_vec(f).expect("ttf lisible");
        let id = fv.glyph_id('A');
        assert_ne!(id.0, 0);
        assert!(fv.outline(id).is_some());
        assert_ne!(fv.glyph_id('g').0, 0);
        assert_eq!(fv.glyph_id('Z').0, 0);
        let o = fv.outline(fv.glyph_id('g')).unwrap();
        assert!(o.bounds.min.y < 0.0); // sous la ligne de base
        assert!(fv.h_advance_unscaled(fv.glyph_id(' ')) > 0.0);
        let p = MaPolice { nom: "x".into(), glyphes: vec![('A', fv_vide())] };
        assert!(rendu_texte(&p, "AA", 2).width() > 0);
    }

    fn fv_vide() -> Grille {
        vec![[false; COLS]; LIGNES]
    }
}
