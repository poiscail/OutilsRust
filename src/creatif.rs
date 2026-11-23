//! Créatif : mini-éditeur d'images (139), créateur de GIF (140), mèmes (141), palettes de couleurs (142),
//! icônes .ico (143), icônes et couleurs de dossiers (144).
use crate::outil::{Cat, Outil};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense};
use image::{imageops, DynamicImage, Rgba, RgbaImage};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"];

/// Police Windows (première trouvée).
pub(crate) fn police(noms: &[&str]) -> Option<FontVec> {
    let d = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join("Fonts");
    noms.iter().find_map(|n| FontVec::try_from_vec(std::fs::read(d.join(n)).ok()?).ok())
}

/// Largeur en pixels d'un texte.
pub(crate) fn largeur(f: &FontVec, taille: f32, t: &str) -> f32 {
    let s = f.as_scaled(PxScale::from(taille));
    t.chars().map(|c| s.h_advance(f.glyph_id(c))).sum()
}

/// Écrit `t` à (x, y) (haut-gauche), avec un contour optionnel.
pub fn ecrire(img: &mut RgbaImage, f: &FontVec, taille: f32, x: f32, y: f32, t: &str, couleur: [u8; 3], contour: Option<([u8; 3], i32)>) {
    let s = f.as_scaled(PxScale::from(taille));
    let mut passes: Vec<(i32, i32, [u8; 3])> = vec![];
    if let Some((c, e)) = contour {
        for dy in -e..=e {
            for dx in -e..=e {
                if dx * dx + dy * dy <= e * e && (dx, dy) != (0, 0) {
                    passes.push((dx, dy, c));
                }
            }
        }
    }
    passes.push((0, 0, couleur));
    for (dx, dy, c) in passes {
        let mut cx = x;
        for ch in t.chars() {
            let id = f.glyph_id(ch);
            let g = id.with_scale_and_position(PxScale::from(taille), ab_glyph::point(cx + dx as f32, y + s.ascent() + dy as f32));
            if let Some(o) = f.outline_glyph(g) {
                let b = o.px_bounds();
                o.draw(|gx, gy, cov| {
                    let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() && cov > 0.0 {
                        let p = img.get_pixel_mut(px as u32, py as u32);
                        for k in 0..3 {
                            p[k] = (p[k] as f32 * (1.0 - cov) + c[k] as f32 * cov) as u8;
                        }
                        p[3] = p[3].max((cov * 255.0) as u8);
                    }
                });
            }
            cx += s.h_advance(id);
        }
    }
}

/// Coupe un texte en lignes qui tiennent dans `max` pixels.
pub fn couper(f: &FontVec, taille: f32, t: &str, max: f32) -> Vec<String> {
    let mut lignes = vec![];
    let mut l = String::new();
    for mot in t.split_whitespace() {
        let essai = if l.is_empty() { mot.to_string() } else { format!("{l} {mot}") };
        if largeur(f, taille, &essai) > max && !l.is_empty() {
            lignes.push(std::mem::take(&mut l));
            l = mot.to_string();
        } else {
            l = essai;
        }
    }
    if !l.is_empty() {
        lignes.push(l);
    }
    lignes
}

fn texture(ctx: &egui::Context, img: &RgbaImage, nom: &str) -> egui::TextureHandle {
    ctx.load_texture(nom, egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR)
}

fn ouvrir_image() -> Option<(PathBuf, RgbaImage)> {
    let p = rfd::FileDialog::new().add_filter("Images", IMAGES).pick_file()?;
    let i = image::open(&p).ok()?.to_rgba8();
    Some((p, i))
}

fn enregistrer_sous(img: &RgbaImage, nom: &str) -> Option<String> {
    let p = rfd::FileDialog::new().add_filter("PNG", &["png"]).add_filter("JPEG", &["jpg"]).set_file_name(nom).save_file()?;
    let r = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")) { DynamicImage::ImageRgba8(img.clone()).to_rgb8().save(&p) } else { img.save(&p) };
    Some(match r {
        Ok(()) => format!("✔ {}", p.display()),
        Err(e) => format!("✖ {e}"),
    })
}

/// Affiche l'image ajustée ; renvoie (zone, échelle, réponse) pour les interactions.
fn afficher(ui: &mut egui::Ui, t: &egui::TextureHandle, l: u32, h: u32) -> (Rect, f32, egui::Response) {
    let dispo = ui.available_size();
    let e = (dispo.x / l as f32).min((dispo.y - 10.0) / h as f32).min(1.0).max(0.05);
    let (zone, rep) = ui.allocate_exact_size(egui::vec2(l as f32 * e, h as f32 * e), Sense::drag());
    ui.painter().image(t.id(), zone, Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
    (zone, e, rep)
}

// ---------------------------------------------------------------------------------------------------
// 139 · Mini-éditeur d'images
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub struct Reglages {
    pub luminosite: i32,
    pub contraste: f32,
    pub saturation: f32,
    /// Netteté (0 = rien) : rattrape une photo un peu floue.
    pub nettete: f32,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { luminosite: 0, contraste: 0.0, saturation: 1.0, nettete: 0.0 }
    }
}

pub fn appliquer_reglages(img: &RgbaImage, r: Reglages) -> RgbaImage {
    let mut d = DynamicImage::ImageRgba8(img.clone());
    if r.luminosite != 0 {
        d = d.brighten(r.luminosite);
    }
    if r.contraste != 0.0 {
        d = d.adjust_contrast(r.contraste);
    }
    if r.nettete > 0.05 {
        d = d.unsharpen(0.6 + r.nettete, 1);
    }
    let mut o = d.to_rgba8();
    if (r.saturation - 1.0).abs() > 0.01 {
        for p in o.pixels_mut() {
            let g = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
            for k in 0..3 {
                p[k] = (g + (p[k] as f32 - g) * r.saturation).clamp(0.0, 255.0) as u8;
            }
        }
    }
    o
}

pub struct Editeur {
    chemin: Option<PathBuf>,
    base: Option<RgbaImage>,
    r: Reglages,
    tex: Option<egui::TextureHandle>,
    sale: bool,
    recadre: Option<(Pos2, Pos2)>,
    mode_recadre: bool,
    texte: String,
    taille_texte: f32,
    couleur: [u8; 3],
    police: Option<FontVec>,
    largeur_cible: u32,
    message: String,
}

impl Editeur {
    pub fn new() -> Self {
        Editeur { chemin: None, base: None, r: Reglages::default(), tex: None, sale: true, recadre: None, mode_recadre: false, texte: String::new(), taille_texte: 48.0,
            couleur: [255, 255, 255], police: None, largeur_cible: 0, message: String::new() }
    }

    fn resultat(&self) -> Option<RgbaImage> {
        self.base.as_ref().map(|b| appliquer_reglages(b, self.r))
    }

    fn figer(&mut self) {
        if let Some(i) = self.resultat() {
            self.base = Some(i);
            self.r = Reglages::default();
            self.sale = true;
        }
    }
}

impl Outil for Editeur {
    fn nom(&self) -> &'static str {
        "🖌 Éditeur d'images"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éditeur d'images");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            if let Ok(i) = image::open(&p) {
                self.base = Some(i.to_rgba8());
                self.chemin = Some(p);
                self.sale = true;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("📂 Ouvrir…").clicked() {
                if let Some((p, i)) = ouvrir_image() {
                    self.base = Some(i);
                    self.chemin = Some(p);
                    self.r = Reglages::default();
                    self.sale = true;
                }
            }
            if self.base.is_some() {
                if ui.button("⟲").on_hover_text("Pivoter à gauche").clicked() {
                    self.figer();
                    self.base = self.base.take().map(|b| imageops::rotate270(&b));
                }
                if ui.button("⟳").on_hover_text("Pivoter à droite").clicked() {
                    self.figer();
                    self.base = self.base.take().map(|b| imageops::rotate90(&b));
                }
                if ui.button("⇋").on_hover_text("Miroir").clicked() {
                    self.figer();
                    self.base = self.base.take().map(|b| imageops::flip_horizontal(&b));
                }
                ui.toggle_value(&mut self.mode_recadre, "✂ Recadrer");
                if ui.button("💾 Enregistrer sous…").clicked() {
                    if let Some(i) = self.resultat() {
                        let n = self.chemin.as_ref().and_then(|p| p.file_stem()).map(|s| format!("{} (modifiée).png", s.to_string_lossy())).unwrap_or("image.png".into());
                        if let Some(m) = enregistrer_sous(&i, &n) {
                            self.message = m;
                        }
                    }
                }
                if ui.button("📋 Copier").clicked() {
                    if let Some(i) = self.resultat() {
                        crate::pressepapiers::win::ecrire_dib(&crate::pressepapiers::rgba_dib(i.width(), i.height(), i.as_raw()));
                        self.message = "Copiée.".into();
                    }
                }
            }
        });
        let Some(base) = self.base.clone() else {
            ui.weak("Ouvre une image ou glisse-la ici.");
            return;
        };
        let avant = self.r;
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::Slider::new(&mut self.r.luminosite, -100..=100).text("luminosité"));
            ui.add(egui::Slider::new(&mut self.r.contraste, -60.0..=60.0).text("contraste"));
            ui.add(egui::Slider::new(&mut self.r.nettete, 0.0..=3.0).text("netteté (photo floue)"));
            ui.add(egui::Slider::new(&mut self.r.saturation, 0.0..=2.0).text("couleurs"));
            if ui.small_button("Réinitialiser").clicked() {
                self.r = Reglages::default();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.texte).hint_text("texte à ajouter").desired_width(200.0));
            ui.add(egui::Slider::new(&mut self.taille_texte, 12.0..=200.0).text("taille"));
            ui.color_edit_button_srgb(&mut self.couleur);
            ui.weak("puis clique sur l'image pour le placer");
            ui.separator();
            ui.add(egui::DragValue::new(&mut self.largeur_cible).range(0..=10000).prefix("largeur ").suffix(" px"));
            if ui.add_enabled(self.largeur_cible > 0, egui::Button::new("Redimensionner")).clicked() {
                self.figer();
                let l = self.largeur_cible;
                self.base = self.base.take().map(|b| {
                    let h = (b.height() as f32 * l as f32 / b.width() as f32).round().max(1.0) as u32;
                    imageops::resize(&b, l, h, imageops::FilterType::Lanczos3)
                });
            }
            ui.weak(format!("{} × {}", base.width(), base.height()));
        });
        if self.r != avant {
            self.sale = true;
        }
        if self.sale || self.tex.is_none() {
            if let Some(i) = self.resultat() {
                self.tex = Some(texture(ui.ctx(), &i, "editeur"));
            }
            self.sale = false;
        }
        let Some(t) = self.tex.clone() else { return };
        let (l, h) = (self.base.as_ref().unwrap().width(), self.base.as_ref().unwrap().height());
        let (zone, e, rep) = afficher(ui, &t, l, h);
        let vers_img = |p: Pos2| egui::pos2(((p.x - zone.left()) / e).clamp(0.0, l as f32), ((p.y - zone.top()) / e).clamp(0.0, h as f32));
        if self.mode_recadre {
            if rep.drag_started() {
                self.recadre = rep.interact_pointer_pos().map(|p| (vers_img(p), vers_img(p)));
            }
            if let (Some((a, _)), Some(p)) = (self.recadre, rep.interact_pointer_pos()) {
                if rep.dragged() {
                    self.recadre = Some((a, vers_img(p)));
                }
            }
            if let Some((a, b)) = self.recadre {
                let r = Rect::from_two_pos(zone.min + a.to_vec2() * e, zone.min + b.to_vec2() * e);
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(2.0f32, Color32::from_rgb(255, 95, 162)), egui::StrokeKind::Middle);
                if rep.drag_stopped() && r.width() > 4.0 && r.height() > 4.0 {
                    self.figer();
                    let (x0, y0) = (a.x.min(b.x) as u32, a.y.min(b.y) as u32);
                    let (w, hh) = ((a.x - b.x).abs() as u32, (a.y - b.y).abs() as u32);
                    self.base = self.base.take().map(|bimg| imageops::crop_imm(&bimg, x0, y0, w.max(1), hh.max(1)).to_image());
                    self.recadre = None;
                    self.mode_recadre = false;
                    self.sale = true;
                }
            }
        } else if rep.clicked() && !self.texte.trim().is_empty() {
            if self.police.is_none() {
                self.police = police(&["segoeuib.ttf", "arialbd.ttf", "arial.ttf"]);
            }
            if let (Some(f), Some(p)) = (self.police.take(), rep.interact_pointer_pos()) {
                let pos = vers_img(p);
                let t = self.texte.clone();
                let (taille, c) = (self.taille_texte, self.couleur);
                self.figer();
                if let Some(b) = self.base.as_mut() {
                    ecrire(b, &f, taille, pos.x, pos.y - taille / 2.0, &t, c, Some(([0, 0, 0], (taille / 24.0).ceil() as i32)));
                }
                self.police = Some(f);
                self.sale = true;
            }
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 141 · Mèmes
// ---------------------------------------------------------------------------------------------------

pub fn meme(base: &RgbaImage, f: &FontVec, haut: &str, bas: &str) -> RgbaImage {
    let mut img = base.clone();
    let (l, h) = (img.width() as f32, img.height() as f32);
    let taille = (h / 9.0).max(18.0);
    let marge = l * 0.04;
    for (texte, en_haut) in [(haut, true), (bas, false)] {
        let t = texte.trim().to_uppercase();
        if t.is_empty() {
            continue;
        }
        let lignes = couper(f, taille, &t, l - 2.0 * marge);
        let hl = taille * 1.05;
        let y0 = if en_haut { h * 0.03 } else { h - h * 0.03 - hl * lignes.len() as f32 };
        for (i, li) in lignes.iter().enumerate() {
            let x = (l - largeur(f, taille, li)) / 2.0;
            ecrire(&mut img, f, taille, x, y0 + i as f32 * hl, li, [255, 255, 255], Some(([0, 0, 0], (taille / 16.0).ceil() as i32)));
        }
    }
    img
}

pub struct Memes {
    base: Option<RgbaImage>,
    haut: String,
    bas: String,
    police: Option<FontVec>,
    tex: Option<(String, egui::TextureHandle)>,
    message: String,
}

impl Memes {
    pub fn new() -> Self {
        Memes { base: None, haut: "Quand le code compile".into(), bas: "du premier coup".into(), police: None, tex: None, message: String::new() }
    }
}

impl Outil for Memes {
    fn nom(&self) -> &'static str {
        "😂 Créateur de mèmes"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Créateur de mèmes");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.base = image::open(p).ok().map(|i| i.to_rgba8());
            self.tex = None;
        }
        if self.police.is_none() {
            self.police = police(&["impact.ttf", "arialbd.ttf", "arial.ttf"]);
        }
        ui.horizontal(|ui| {
            if ui.button("📂 Image…").clicked() {
                self.base = ouvrir_image().map(|x| x.1);
                self.tex = None;
            }
            ui.add(egui::TextEdit::singleline(&mut self.haut).hint_text("texte du haut").desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut self.bas).hint_text("texte du bas").desired_width(220.0));
        });
        let (Some(b), Some(f)) = (&self.base, &self.police) else {
            ui.weak("Choisis une image (ou glisse-la ici).");
            return;
        };
        let cle = format!("{}|{}", self.haut, self.bas);
        let img = meme(b, f, &self.haut, &self.bas);
        if self.tex.as_ref().is_none_or(|t| t.0 != cle) {
            self.tex = Some((cle, texture(ui.ctx(), &img, "meme")));
        }
        ui.horizontal(|ui| {
            if ui.button("💾 Enregistrer…").clicked() {
                if let Some(m) = enregistrer_sous(&img, "meme.png") {
                    self.message = m;
                }
            }
            if ui.button("📋 Copier (à coller dans Discord)").clicked() {
                crate::pressepapiers::win::ecrire_dib(&crate::pressepapiers::rgba_dib(img.width(), img.height(), img.as_raw()));
                self.message = "Copié.".into();
            }
            ui.label(&self.message);
        });
        if let Some((_, t)) = &self.tex {
            afficher(ui, t, img.width(), img.height());
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 142 · Palettes
// ---------------------------------------------------------------------------------------------------

/// Couleurs dominantes (k-moyennes sur une version réduite), de la plus présente à la moins présente.
pub fn palette(img: &RgbaImage, k: usize) -> Vec<([u8; 3], f32)> {
    let petite = imageops::resize(img, 96, 96, imageops::FilterType::Triangle);
    let px: Vec<[f32; 3]> = petite.pixels().filter(|p| p[3] > 128).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    if px.is_empty() {
        return vec![];
    }
    let k = k.min(px.len());
    // centres de départ répartis dans la liste triée par luminosité : résultat stable
    let mut tri = px.clone();
    tri.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
    let mut c: Vec<[f32; 3]> = (0..k).map(|i| tri[(i * 2 + 1) * tri.len() / (2 * k)]).collect();
    let mut groupe = vec![0usize; px.len()];
    for _ in 0..12 {
        for (i, p) in px.iter().enumerate() {
            groupe[i] = (0..k).min_by(|&a, &b| {
                let d = |c: &[f32; 3]| (0..3).map(|j| (p[j] - c[j]).powi(2)).sum::<f32>();
                d(&c[a]).total_cmp(&d(&c[b]))
            }).unwrap();
        }
        for (j, cj) in c.iter_mut().enumerate() {
            let membres: Vec<&[f32; 3]> = px.iter().zip(&groupe).filter(|(_, g)| **g == j).map(|(p, _)| p).collect();
            if !membres.is_empty() {
                for x in 0..3 {
                    cj[x] = membres.iter().map(|p| p[x]).sum::<f32>() / membres.len() as f32;
                }
            }
        }
    }
    let mut r: Vec<([u8; 3], f32)> = c.iter().enumerate().map(|(j, cj)| {
        ([cj[0] as u8, cj[1] as u8, cj[2] as u8], groupe.iter().filter(|g| **g == j).count() as f32 / px.len() as f32)
    }).filter(|x| x.1 > 0.0).collect();
    r.sort_by(|a, b| b.1.total_cmp(&a.1));
    r
}

pub struct Palettes {
    img: Option<(RgbaImage, egui::TextureHandle)>,
    n: usize,
    couleurs: Vec<([u8; 3], f32)>,
}

impl Palettes {
    pub fn new() -> Self {
        Palettes { img: None, n: 6, couleurs: vec![] }
    }
}

impl Outil for Palettes {
    fn nom(&self) -> &'static str {
        "🎨 Palette d'une image"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Palette de couleurs d'une image");
        let depose = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())).and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
        let mut nouvelle = depose;
        ui.horizontal(|ui| {
            if ui.button("📂 Image…").clicked() {
                nouvelle = ouvrir_image().map(|x| x.1);
            }
            if ui.add(egui::Slider::new(&mut self.n, 3..=12).text("couleurs")).changed() {
                if let Some((i, _)) = &self.img {
                    self.couleurs = palette(i, self.n);
                }
            }
        });
        if let Some(i) = nouvelle {
            self.couleurs = palette(&i, self.n);
            let t = texture(ui.ctx(), &i, "palette");
            self.img = Some((i, t));
        }
        if !self.couleurs.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (c, part) in &self.couleurs {
                    let hex = crate::capture::hex(*c);
                    ui.vertical(|ui| {
                        let (r, rep) = ui.allocate_exact_size(egui::vec2(90.0, 70.0), Sense::click());
                        ui.painter().rect_filled(r, 8.0, Color32::from_rgb(c[0], c[1], c[2]));
                        if rep.on_hover_text("Clic : copier").clicked() {
                            ui.ctx().copy_text(hex.clone());
                        }
                        ui.label(RichText::new(&hex).monospace());
                        ui.weak(format!("{:.0} %", part * 100.0));
                    });
                }
            });
            if ui.button("📋 Copier toute la palette").clicked() {
                ui.ctx().copy_text(self.couleurs.iter().map(|(c, _)| crate::capture::hex(*c)).collect::<Vec<_>>().join(", "));
            }
        }
        if let Some((i, t)) = &self.img {
            afficher(ui, t, i.width(), i.height());
        } else {
            ui.weak("Choisis une image (ou glisse-la ici) : ses couleurs principales s'affichent, clic pour copier le code.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 143 · Icônes .ico / 144 · Dossiers personnalisés
// ---------------------------------------------------------------------------------------------------

/// Fichier .ico avec plusieurs tailles (chaque image enregistrée en PNG, comme le fait Windows).
pub fn ico(img: &RgbaImage, tailles: &[u32]) -> Vec<u8> {
    let carre = {
        let c = img.width().max(img.height());
        let mut fond = RgbaImage::from_pixel(c, c, Rgba([0, 0, 0, 0]));
        imageops::overlay(&mut fond, img, ((c - img.width()) / 2) as i64, ((c - img.height()) / 2) as i64);
        fond
    };
    let images: Vec<Vec<u8>> = tailles.iter().map(|&t| {
        let r = imageops::resize(&carre, t, t, imageops::FilterType::Lanczos3);
        let mut b = std::io::Cursor::new(vec![]);
        let _ = r.write_to(&mut b, image::ImageFormat::Png);
        b.into_inner()
    }).collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((tailles.len() as u16).to_le_bytes());
    let mut decalage = 6 + 16 * tailles.len() as u32;
    for (t, d) in tailles.iter().zip(&images) {
        let c = if *t >= 256 { 0 } else { *t as u8 };
        out.extend([c, c, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((d.len() as u32).to_le_bytes());
        out.extend(decalage.to_le_bytes());
        decalage += d.len() as u32;
    }
    for d in images {
        out.extend(d);
    }
    out
}

/// Icône de dossier dessinée dans une couleur (256 × 256).
pub fn dossier_colore(c: [u8; 3]) -> RgbaImage {
    let mut img = RgbaImage::from_pixel(256, 256, Rgba([0, 0, 0, 0]));
    let fonce = [(c[0] as f32 * 0.75) as u8, (c[1] as f32 * 0.75) as u8, (c[2] as f32 * 0.75) as u8];
    let arrondi = |x: i32, y: i32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32| -> bool {
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).pow(2) + (y - cy).pow(2) <= r * r
    };
    for y in 0..256 {
        for x in 0..256 {
            let (xi, yi) = (x as i32, y as i32);
            let p = if arrondi(xi, yi, 20, 70, 236, 220, 18) {
                Some(c)
            } else if arrondi(xi, yi, 20, 44, 120, 100, 14) || arrondi(xi, yi, 20, 56, 236, 110, 16) {
                Some(fonce)
            } else {
                None
            };
            if let Some(p) = p {
                img.put_pixel(x, y, Rgba([p[0], p[1], p[2], 255]));
            }
        }
    }
    img
}

pub fn appliquer_icone_dossier(dossier: &Path, icone: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let ini = dossier.join("desktop.ini");
    let _ = std::process::Command::new("attrib").args(["-s", "-h"]).arg(&ini).creation_flags(0x0800_0000).status();
    let contenu = format!("[.ShellClassInfo]\r\nIconResource={},0\r\n[ViewState]\r\nMode=\r\nVid=\r\nFolderType=Generic\r\n", icone.display());
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(contenu.encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
    std::fs::write(&ini, utf16).map_err(|e| e.to_string())?;
    let _ = std::process::Command::new("attrib").args(["+s", "+h"]).arg(&ini).creation_flags(0x0800_0000).status();
    let _ = std::process::Command::new("attrib").arg("+r").arg(dossier).creation_flags(0x0800_0000).status();
    unsafe { windows_sys::Win32::UI::Shell::SHChangeNotify(0x0800_0000 /* SHCNE_ASSOCCHANGED */, 0, std::ptr::null(), std::ptr::null()) };
    Ok(())
}

pub struct Icones {
    img: Option<(RgbaImage, egui::TextureHandle)>,
    dossier: Option<PathBuf>,
    couleur: [u8; 3],
    message: String,
}

impl Icones {
    pub fn new() -> Self {
        Icones { img: None, dossier: None, couleur: [255, 95, 162], message: String::new() }
    }
}

impl Outil for Icones {
    fn nom(&self) -> &'static str {
        "🗂 Icônes et dossiers"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Icônes et dossiers personnalisés");
        ui.strong("1. Créer une icône (.ico)");
        ui.horizontal(|ui| {
            if ui.button("📂 À partir d'une image…").clicked() {
                if let Some((_, i)) = ouvrir_image() {
                    let t = texture(ui.ctx(), &i, "ico");
                    self.img = Some((i, t));
                }
            }
            ui.label("ou un dossier coloré :");
            ui.color_edit_button_srgb(&mut self.couleur);
            for c in [[255, 95, 162], [170, 120, 255], [90, 170, 255], [80, 200, 140], [255, 200, 60], [255, 110, 80], [120, 120, 130]] {
                let (r, rep) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                ui.painter().circle_filled(r.center(), 8.0, Color32::from_rgb(c[0], c[1], c[2]));
                if rep.clicked() {
                    self.couleur = c;
                }
            }
            if ui.button("🎨 Dossier de cette couleur").clicked() {
                let i = dossier_colore(self.couleur);
                let t = texture(ui.ctx(), &i, "ico");
                self.img = Some((i, t));
            }
        });
        let Some((img, t)) = &self.img else { return };
        ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(128.0, 128.0)));
        let donnees = ico(img, &[16, 24, 32, 48, 64, 128, 256]);
        ui.horizontal(|ui| {
            if ui.button("💾 Enregistrer le .ico…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Icône", &["ico"]).set_file_name("icone.ico").save_file() {
                    self.message = std::fs::write(&p, &donnees).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
        });
        ui.separator();
        ui.strong("2. La mettre sur un dossier");
        ui.horizontal(|ui| {
            if ui.button("📁 Choisir le dossier…").clicked() {
                self.dossier = rfd::FileDialog::new().pick_folder();
            }
            if let Some(d) = &self.dossier {
                ui.monospace(d.display().to_string());
                if ui.button("Appliquer l'icône").clicked() {
                    // l'icône est rangée dans le dossier d'Outils pour ne pas encombrer le dossier
                    let rep = crate::outil::base().join("icones");
                    let _ = std::fs::create_dir_all(&rep);
                    let f = rep.join(format!("dossier_{:x}.ico", crate::outil::hasard(u64::MAX)));
                    self.message = std::fs::write(&f, &donnees).map_err(|e| e.to_string()).and_then(|_| appliquer_icone_dossier(d, &f))
                        .map(|_| "✔ Icône appliquée (l'explorateur peut mettre quelques secondes à la montrer).".to_string()).unwrap_or_else(|e| e);
                }
                if ui.button("Remettre l'icône normale").clicked() {
                    use std::os::windows::process::CommandExt;
                    let ini = d.join("desktop.ini");
                    let _ = std::process::Command::new("attrib").args(["-s", "-h"]).arg(&ini).creation_flags(0x0800_0000).status();
                    let _ = std::fs::remove_file(ini);
                    self.message = "Icône d'origine remise.".into();
                }
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 140 · Créateur de GIF (depuis une vidéo)
// ---------------------------------------------------------------------------------------------------

pub struct CreateurGif {
    video: Option<PathBuf>,
    debut: String,
    duree: f32,
    fps: u32,
    largeur: u32,
    etat: Arc<Mutex<(bool, String)>>,
}

impl CreateurGif {
    pub fn new() -> Self {
        CreateurGif { video: None, debut: "0:00".into(), duree: 4.0, fps: 15, largeur: 480, etat: Arc::new(Mutex::new((false, String::new()))) }
    }
}

impl Outil for CreateurGif {
    fn nom(&self) -> &'static str {
        "🎞 Créateur de GIF"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Créateur de GIF");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.video = Some(p);
        }
        ui.horizontal(|ui| {
            if ui.button("📂 Vidéo…").clicked() {
                self.video = rfd::FileDialog::new().pick_file();
            }
            if let Some(v) = &self.video {
                ui.label(v.file_name().unwrap_or_default().to_string_lossy());
            } else {
                ui.weak("ou glisse une vidéo, un clip, un replay…");
            }
        });
        let Some(v) = self.video.clone() else { return };
        egui::Grid::new("gif").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Début");
            ui.add(egui::TextEdit::singleline(&mut self.debut).desired_width(70.0));
            ui.end_row();
            ui.label("Durée");
            ui.add(egui::Slider::new(&mut self.duree, 0.5..=20.0).suffix(" s"));
            ui.end_row();
            ui.label("Images par seconde");
            ui.add(egui::Slider::new(&mut self.fps, 5..=30));
            ui.end_row();
            ui.label("Largeur");
            ui.horizontal(|ui| {
                for l in [320, 480, 640, 800] {
                    ui.selectable_value(&mut self.largeur, l, format!("{l} px"));
                }
            });
            ui.end_row();
        });
        let (occupe, m) = self.etat.lock().unwrap().clone();
        if ui.add_enabled(!occupe, egui::Button::new(RichText::new("🎞 Créer le GIF").size(16.0))).clicked() {
            let debut = crate::video::lire_temps(&self.debut).unwrap_or(0.0);
            let (d, f, l, e) = (self.duree, self.fps, self.largeur, self.etat.clone());
            *e.lock().unwrap() = (true, "Création…".into());
            std::thread::spawn(move || {
                
                let dest = v.with_file_name(format!("{}.gif", v.file_stem().unwrap_or_default().to_string_lossy()));
                let filtre = format!("fps={f},scale={l}:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4");
                let o = crate::dependances::commande("ffmpeg").map_err(std::io::Error::other).and_then(|mut c| c.args(["-y", "-v", "error", "-ss", &debut.to_string(), "-t", &d.to_string(), "-i"]).arg(&v)
                    .args(["-vf", &filtre, "-loop", "0"]).arg(&dest).output());
                *e.lock().unwrap() = (false, match o {
                    Ok(o) if o.status.success() => format!("✔ {} ({})", dest.display(), crate::disque::taille_lisible(dest.metadata().map(|m| m.len()).unwrap_or(0))),
                    Ok(o) => format!("✖ {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec")),
                    Err(e) => format!("✖ {e}"),
                });
            });
        }
        if occupe {
            ui.spinner();
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        ui.label(m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fichier_ico() {
        let img = RgbaImage::from_pixel(40, 20, Rgba([255, 0, 0, 255]));
        let d = ico(&img, &[16, 256]);
        assert_eq!(&d[..6], &[0, 0, 1, 0, 2, 0]);
        assert_eq!(d[6], 16); // largeur de la 1re image
        assert_eq!(d[22], 0); // 256 s'écrit 0
        let off = u32::from_le_bytes(d[18..22].try_into().unwrap()) as usize;
        assert_eq!(&d[off..off + 4], b"\x89PNG");
    }

    #[test]
    fn couleurs() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([250, 10, 10, 255]));
        for y in 0..100 {
            for x in 0..30 {
                img.put_pixel(x, y, Rgba([10, 10, 250, 255]));
            }
        }
        let p = palette(&img, 2);
        assert_eq!(p.len(), 2);
        assert!(p[0].0[0] > 200 && p[0].1 > 0.6); // rouge majoritaire
        assert!(p[1].0[2] > 200);
        let r = appliquer_reglages(&RgbaImage::from_pixel(1, 1, Rgba([100, 150, 200, 255])), Reglages { saturation: 0.0, ..Default::default() });
        let g = r.get_pixel(0, 0);
        assert!(g[0] == g[1] && g[1] == g[2]);
        assert_eq!(dossier_colore([255, 0, 0]).get_pixel(128, 150)[0], 255);
    }
}
