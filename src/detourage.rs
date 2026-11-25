//! Détourage automatique (28) : une petite IA (U²-Net, 4,5 Mo, sur le PC) trouve le sujet d'une photo
//! et enlève le fond : transparent, couleur, flou ou autre image. Une image ou tout un lot.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::{GrayImage, Rgba, RgbaImage};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use tract_onnx::prelude::*;

const TAILLE: usize = 320;
const MODELE: &str = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx";

type Plan = Arc<TypedRunnableModel>;

/// Charge le modèle ONNX (corrige un mode de redimensionnement que tract ne connaît pas sous ce nom).
pub fn charger_modele(chemin: &std::path::Path) -> Result<Plan, String> {
    let onnx = tract_onnx::onnx();
    let mut proto = onnx.proto_model_for_path(chemin).map_err(|e| e.to_string())?;
    if let Some(g) = proto.graph.as_mut() {
        for n in g.node.iter_mut() {
            for a in n.attribute.iter_mut() {
                // identique à half_pixel dès que la sortie fait plus d'un pixel (toujours le cas ici)
                if a.s == b"pytorch_half_pixel" {
                    a.s = b"half_pixel".to_vec();
                }
            }
        }
    }
    onnx.model_for_proto_model(&proto)
        .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, TAILLE, TAILLE]).into()))
        .and_then(|m| m.into_typed())
        .and_then(|m| m.into_decluttered())
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(|e| e.to_string())
}

/// Masque (0 = fond, 255 = sujet) à la taille de l'image.
pub fn masque(plan: &Plan, img: &RgbaImage) -> Result<GrayImage, String> {
    let r = image::imageops::resize(img, TAILLE as u32, TAILLE as u32, image::imageops::FilterType::Triangle);
    let (moy, et) = ([0.485f32, 0.456, 0.406], [0.229f32, 0.224, 0.225]);
    let entree: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, TAILLE, TAILLE), |(_, c, y, x)| (r.get_pixel(x as u32, y as u32)[c] as f32 / 255.0 - moy[c]) / et[c]).into();
    let s = plan.run(tvec!(entree.into())).map_err(|e| e.to_string())?;
    let o = s[0].to_plain_array_view::<f32>().map_err(|e| e.to_string())?;
    let (mn, mx) = o.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let petit = GrayImage::from_fn(TAILLE as u32, TAILLE as u32, |x, y| image::Luma([(((o[[0, 0, y as usize, x as usize]] - mn) / (mx - mn).max(1e-6)) * 255.0) as u8]));
    Ok(image::imageops::resize(&petit, img.width(), img.height(), image::imageops::FilterType::Triangle))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Fond {
    Transparent,
    Couleur([u8; 3]),
    Flou,
    Image,
}

/// Image finale : sujet sur le fond choisi. `net` durcit les bords.
pub fn composer(img: &RgbaImage, m: &GrayImage, fond: Fond, image_fond: Option<&RgbaImage>, net: bool) -> RgbaImage {
    let alpha = |v: u8| -> f32 {
        let a = v as f32 / 255.0;
        if net { ((a - 0.3) / 0.4).clamp(0.0, 1.0) } else { a }
    };
    let arriere: Option<RgbaImage> = match fond {
        Fond::Transparent => None,
        Fond::Couleur(c) => Some(RgbaImage::from_pixel(img.width(), img.height(), Rgba([c[0], c[1], c[2], 255]))),
        Fond::Flou => {
            let petite = image::imageops::thumbnail(img, (img.width() / 4).max(1), (img.height() / 4).max(1));
            let floue = image::imageops::blur(&petite, 6.0);
            Some(image::imageops::resize(&floue, img.width(), img.height(), image::imageops::FilterType::Triangle))
        }
        Fond::Image => image_fond.map(|f| crate::createur::couvrir(f, img.width(), img.height())),
    };
    RgbaImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = alpha(m.get_pixel(x, y)[0]);
        match &arriere {
            None => Rgba([p[0], p[1], p[2], (a * 255.0) as u8]),
            Some(f) => {
                let q = f.get_pixel(x, y);
                let mel = |k: usize| (p[k] as f32 * a + q[k] as f32 * (1.0 - a)) as u8;
                Rgba([mel(0), mel(1), mel(2), 255])
            }
        }
    })
}

fn damier(l: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(l, h, |x, y| if (x / 12 + y / 12) % 2 == 0 { Rgba([200, 200, 200, 255]) } else { Rgba([150, 150, 150, 255]) })
}

pub struct Detourage {
    plan: Arc<Mutex<Option<Plan>>>,
    image: Option<(PathBuf, RgbaImage)>,
    masque: Option<GrayImage>,
    fond: Fond,
    image_fond: Option<RgbaImage>,
    net: bool,
    apercu: Option<egui::TextureHandle>,
    sale: bool,
    message: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    resultat: Arc<Mutex<Option<GrayImage>>>,
}

impl Detourage {
    pub fn new() -> Self {
        Detourage { plan: Arc::default(), image: None, masque: None, fond: Fond::Transparent, image_fond: None, net: true, apercu: None, sale: false, message: Arc::default(), occupe: Arc::default(), resultat: Arc::default() }
    }

    /// Charge le modèle (téléchargé la première fois) dans le fil courant.
    fn preparer(plan: &Arc<Mutex<Option<Plan>>>, msg: &Arc<Mutex<String>>) -> Result<(), String> {
        if plan.lock().unwrap().is_some() {
            return Ok(());
        }
        let p = crate::ia::fichier(MODELE, "u2netp.onnx", msg, "l'IA de détourage (4,5 Mo, une seule fois)")?;
        *msg.lock().unwrap() = "Préparation de l'IA…".into();
        *plan.lock().unwrap() = Some(charger_modele(&p)?);
        Ok(())
    }

    fn analyser(&mut self) {
        let Some((_, img)) = self.image.clone() else { return };
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (pl, m, o, r) = (self.plan.clone(), self.message.clone(), self.occupe.clone(), self.resultat.clone());
        std::thread::spawn(move || {
            let res = Detourage::preparer(&pl, &m).and_then(|_| {
                *m.lock().unwrap() = "Détourage…".into();
                masque(pl.lock().unwrap().as_ref().unwrap(), &img)
            });
            match res {
                Ok(x) => {
                    *r.lock().unwrap() = Some(x);
                    *m.lock().unwrap() = String::new();
                }
                Err(e) => *m.lock().unwrap() = format!("✖ {e}"),
            }
            o.store(false, Relaxed);
        });
    }

    fn lot(&self) {
        let Some(fichiers) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_files() else { return };
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (pl, m, o, fond, net, fi) = (self.plan.clone(), self.message.clone(), self.occupe.clone(), self.fond, self.net, self.image_fond.clone());
        std::thread::spawn(move || {
            let r = Detourage::preparer(&pl, &m).map(|_| {
                let mut ok = 0;
                for (k, f) in fichiers.iter().enumerate() {
                    *m.lock().unwrap() = format!("Détourage {}/{}…", k + 1, fichiers.len());
                    let Ok(i) = image::open(f) else { continue };
                    let i = i.to_rgba8();
                    let Ok(ma) = masque(pl.lock().unwrap().as_ref().unwrap(), &i) else { continue };
                    let res = composer(&i, &ma, fond, fi.as_ref(), net);
                    let sortie = f.with_file_name(format!("{} détouré.png", f.file_stem().unwrap_or_default().to_string_lossy()));
                    if res.save(&sortie).is_ok() {
                        ok += 1;
                    }
                }
                format!("✔ {ok} image(s) détourée(s), enregistrées à côté des originales.")
            });
            *m.lock().unwrap() = r.unwrap_or_else(|e| format!("✖ {e}"));
            o.store(false, Relaxed);
        });
    }
}

impl Outil for Detourage {
    fn nom(&self) -> &'static str {
        "✂ Détourage automatique"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Détourage automatique");
        ui.weak("Enlève le fond d'une photo en un clic : une petite IA trouve le sujet (personne, animal, objet). Tout se passe sur ton PC.");
        if let Some(m) = self.resultat.lock().unwrap().take() {
            self.masque = Some(m);
            self.sale = true;
        }
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("🖼 Choisir une photo…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                    if let Ok(i) = image::open(&p) {
                        self.image = Some((p, i.to_rgba8()));
                        self.masque = None;
                        self.apercu = None;
                        self.analyser();
                    }
                }
            }
            if ui.add_enabled(!occupe, egui::Button::new("📚 Tout un lot de photos…")).on_hover_text("Chaque photo est enregistrée en « nom détouré.png » à côté de l'originale").clicked() {
                self.lot();
            }
            if occupe {
                ui.spinner();
            }
            ui.label(self.message.lock().unwrap().clone());
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Fond :");
            let avant = self.fond;
            ui.selectable_value(&mut self.fond, Fond::Transparent, "▦ Transparent");
            let couleur = if let Fond::Couleur(c) = self.fond { c } else { [255, 255, 255] };
            if ui.selectable_label(matches!(self.fond, Fond::Couleur(_)), "🎨 Couleur").clicked() {
                self.fond = Fond::Couleur(couleur);
            }
            if let Fond::Couleur(c) = &mut self.fond {
                ui.color_edit_button_srgb(c);
            }
            ui.selectable_value(&mut self.fond, Fond::Flou, "🌫 Flou");
            if ui.selectable_label(self.fond == Fond::Image, "🏞 Autre image…").clicked() {
                if let Some(i) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file().and_then(|p| image::open(p).ok()) {
                    self.image_fond = Some(i.to_rgba8());
                    self.fond = Fond::Image;
                }
            }
            self.sale |= ui.checkbox(&mut self.net, "Bords nets").changed();
            self.sale |= avant != self.fond;
        });
        if let (Some((_, img)), Some(m)) = (&self.image, &self.masque) {
            if self.sale || self.apercu.is_none() {
                self.sale = false;
                let petite = image::imageops::thumbnail(img, 900.min(img.width()), (900.min(img.width()) as f32 * img.height() as f32 / img.width() as f32) as u32);
                let mpetit = image::imageops::resize(m, petite.width(), petite.height(), image::imageops::FilterType::Triangle);
                let mut r = composer(&petite, &mpetit, self.fond, self.image_fond.as_ref(), self.net);
                if self.fond == Fond::Transparent {
                    let mut d = damier(r.width(), r.height());
                    image::imageops::overlay(&mut d, &r, 0, 0);
                    r = d;
                }
                let c = egui::ColorImage::from_rgba_unmultiplied([r.width() as usize, r.height() as usize], r.as_raw());
                self.apercu = Some(ui.ctx().load_texture("detourage", c, Default::default()));
            }
            if let Some(t) = &self.apercu {
                let l = ui.available_width().min(700.0);
                let s = t.size_vec2();
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(l, l * s.y / s.x.max(1.0))));
            }
            if ui.button(RichText::new("💾 Enregistrer…").strong()).clicked() {
                let (p, img) = self.image.clone().unwrap();
                let nom = format!("{} détouré.png", p.file_stem().unwrap_or_default().to_string_lossy());
                if let Some(d) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(nom).save_file() {
                    let r = composer(&img, self.masque.as_ref().unwrap(), self.fond, self.image_fond.as_ref(), self.net);
                    *self.message.lock().unwrap() = r.save(&d).map(|_| format!("✔ {}", d.display())).unwrap_or_else(|e| e.to_string());
                }
            }
        } else if self.image.is_none() {
            ui.colored_label(Color32::GRAY, "Choisis une photo pour commencer.");
        }
        if occupe {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition() {
        let img = RgbaImage::from_pixel(4, 2, Rgba([200, 10, 10, 255]));
        let m = GrayImage::from_fn(4, 2, |x, _| image::Luma([if x < 2 { 255 } else { 0 }]));
        let t = composer(&img, &m, Fond::Transparent, None, true);
        assert_eq!(t.get_pixel(0, 0)[3], 255);
        assert_eq!(t.get_pixel(3, 0)[3], 0);
        let c = composer(&img, &m, Fond::Couleur([0, 0, 255]), None, true);
        assert_eq!(*c.get_pixel(3, 1), Rgba([0, 0, 255, 255]));
        assert_eq!(*c.get_pixel(0, 1), Rgba([200, 10, 10, 255]));
    }
}
