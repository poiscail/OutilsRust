//! Guide en jeu (1) : tes notes, une soluce ou une carte affichées par-dessus le jeu, dans un coin,
//! sans prendre la souris. Ctrl+Alt+G affiche / cache, Ctrl+Alt+N passe à la fiche suivante.
use crate::calque::{zone_travail, Calque};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Fiche {
    pub titre: String,
    /// jeu associé (vide : toujours proposée)
    pub jeu: String,
    pub texte: String,
    pub image: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub fiches: Vec<Fiche>,
    pub coin: u8, // 0 haut gauche, 1 haut droit, 2 bas gauche, 3 bas droit
    pub largeur: u32,
    pub opacite: f32,
    pub taille_texte: f32,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            fiches: vec![Fiche { titre: "Exemple".into(), jeu: String::new(), texte: "Ctrl+Alt+G : afficher / cacher\nCtrl+Alt+N : fiche suivante\n\nÉcris ici ta soluce, tes codes, ta liste de quêtes…".into(), image: None }],
            coin: 1,
            largeur: 420,
            opacite: 0.82,
            taille_texte: 18.0,
        }
    }
}

/// Fiches à montrer : celles du jeu en cours, sinon celles sans jeu.
pub fn fiches_pour(r: &Reglages, jeu: Option<&str>) -> Vec<usize> {
    let du_jeu: Vec<usize> = match jeu {
        Some(j) => r.fiches.iter().enumerate().filter(|(_, f)| !f.jeu.trim().is_empty() && j.to_lowercase().contains(&f.jeu.trim().to_lowercase())).map(|(k, _)| k).collect(),
        None => Vec::new(),
    };
    if !du_jeu.is_empty() {
        return du_jeu;
    }
    let v: Vec<usize> = r.fiches.iter().enumerate().filter(|(_, f)| f.jeu.trim().is_empty()).map(|(k, _)| k).collect();
    if v.is_empty() { (0..r.fiches.len()).collect() } else { v }
}

/// Image de la fiche : un panneau sombre arrondi avec le titre, le texte et l'image.
pub fn rendre(f: &Fiche, r: &Reglages, police: Option<&ab_glyph::FontVec>, n: (usize, usize)) -> RgbaImage {
    let l = r.largeur.clamp(200, 1200);
    let t = r.taille_texte.clamp(10.0, 40.0);
    let marge = 14.0;
    let lignes = match police {
        Some(p) => crate::atelier::couper(p, t, &f.texte, l as f32 - 2.0 * marge),
        None => f.texte.lines().map(str::to_string).collect(),
    };
    let img_fiche = f.image.as_ref().and_then(|p| image::open(p).ok()).map(|i| {
        let i = i.to_rgba8();
        let lm = l - 2 * marge as u32;
        image::imageops::resize(&i, lm, (lm as f32 * i.height() as f32 / i.width().max(1) as f32) as u32, image::imageops::FilterType::Triangle)
    });
    let h_texte = if f.texte.trim().is_empty() { 0.0 } else { lignes.len() as f32 * t * 1.25 + marge };
    let h_img = img_fiche.as_ref().map(|i| i.height() as f32 + marge).unwrap_or(0.0);
    let h = (marge * 2.0 + t * 1.6 + h_texte + h_img).min(1400.0) as u32;
    let a = (r.opacite.clamp(0.2, 1.0) * 255.0) as u8;
    let mut img = RgbaImage::from_pixel(l, h, Rgba([18, 18, 26, a]));
    // coins arrondis
    let rr = 12i32;
    for y in 0..rr {
        for x in 0..rr {
            if (rr - x - 1).pow(2) + (rr - y - 1).pow(2) > rr * rr {
                for (px, py) in [(x, y), (l as i32 - 1 - x, y), (x, h as i32 - 1 - y), (l as i32 - 1 - x, h as i32 - 1 - y)] {
                    img.put_pixel(px as u32, py as u32, Rgba([0, 0, 0, 0]));
                }
            }
        }
    }
    let Some(p) = police else { return img };
    let titre = if n.1 > 1 { format!("{}  ({}/{})", f.titre, n.0 + 1, n.1) } else { f.titre.clone() };
    crate::creatif::ecrire(&mut img, p, t * 1.15, marge, marge, &titre, [255, 200, 90], None);
    let mut y = marge + t * 1.6;
    if let Some(i) = &img_fiche {
        image::imageops::overlay(&mut img, i, marge as i64, y as i64);
        y += i.height() as f32 + marge;
    }
    for li in &lignes {
        if y + t > h as f32 {
            break;
        }
        crate::creatif::ecrire(&mut img, p, t, marge, y, li, [235, 235, 240], None);
        y += t * 1.25;
    }
    img
}

pub struct GuideJeu {
    r: Arc<Mutex<Reglages>>,
    choix: usize,
    visible: Arc<Mutex<(bool, usize)>>,
    pris: Arc<std::sync::atomic::AtomicBool>,
}

impl GuideJeu {
    pub fn new() -> Self {
        let r: Reglages = charger("guide_jeu");
        let g = GuideJeu { r: Arc::new(Mutex::new(r)), choix: 0, visible: Arc::default(), pris: Arc::default() };
        let (r, vis, pris) = (g.r.clone(), g.visible.clone(), g.pris.clone());
        std::thread::spawn(move || {
            let m = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
            let c = Calque::new("OutilsGuideJeu", true, &[(m, b'G' as u32), (m, b'N' as u32)]);
            std::thread::sleep(Duration::from_millis(300));
            pris.store(c.raccourci_pris.load(std::sync::atomic::Ordering::Relaxed), std::sync::atomic::Ordering::Relaxed);
            let police = crate::creatif::police(&["segoeui.ttf", "arial.ttf"]);
            let mut dernier: Option<(Reglages, bool, usize, Option<String>)> = None;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                {
                    let mut v = vis.lock().unwrap();
                    if c.appuis(0) % 2 == 1 {
                        v.0 = !v.0;
                    }
                    v.1 += c.appuis(1) as usize;
                }
                let (montre, page) = *vis.lock().unwrap();
                let cfg = r.lock().unwrap().clone();
                let jeu = crate::outil::jeu_actif();
                let etat = (cfg.clone(), montre, page, jeu.clone());
                if dernier.as_ref() == Some(&etat) {
                    continue;
                }
                dernier = Some(etat);
                let liste = fiches_pour(&cfg, jeu.as_deref());
                if !montre || liste.is_empty() {
                    c.cacher();
                    continue;
                }
                let k = liste[page % liste.len()];
                let img = rendre(&cfg.fiches[k], &cfg, police.as_ref(), (page % liste.len(), liste.len()));
                let (g, hh, d, b) = zone_travail();
                let (l, h) = (img.width() as i32, img.height() as i32);
                let (x, y) = match cfg.coin {
                    0 => (g + 20, hh + 20),
                    1 => (d - l - 20, hh + 20),
                    2 => (g + 20, b - h - 20),
                    _ => (d - l - 20, b - h - 20),
                };
                c.montrer(img, x, y);
            }
        });
        g
    }
}

impl Outil for GuideJeu {
    fn nom(&self) -> &'static str {
        "🧭 Guide en jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Guide en jeu");
        ui.weak("Tes notes, ta soluce ou une carte, affichées par-dessus le jeu (en plein écran fenêtré ou fenêtré). Les clics passent à travers.");
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Ctrl+Alt+G").strong());
            ui.label("afficher / cacher ·");
            ui.label(RichText::new("Ctrl+Alt+N").strong());
            ui.label("fiche suivante");
            let mut v = self.visible.lock().unwrap();
            if ui.button(if v.0 { "🙈 Cacher" } else { "👁 Afficher maintenant" }).clicked() {
                v.0 = !v.0;
            }
        });
        if self.pris.load(std::sync::atomic::Ordering::Relaxed) {
            ui.colored_label(Color32::from_rgb(255, 190, 90), "Un des raccourcis est déjà pris par un autre programme : utilise le bouton ci-dessus.");
        }
        let mut r = self.r.lock().unwrap().clone();
        let avant = r.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Coin");
            for (k, n) in ["↖", "↗", "↙", "↘"].iter().enumerate() {
                ui.selectable_value(&mut r.coin, k as u8, *n);
            }
            ui.label("Largeur");
            ui.add(egui::Slider::new(&mut r.largeur, 250..=900));
            ui.label("Opacité");
            ui.add(egui::Slider::new(&mut r.opacite, 0.3..=1.0).show_value(false));
            ui.label("Texte");
            ui.add(egui::Slider::new(&mut r.taille_texte, 12.0..=32.0).show_value(false));
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (k, f) in r.fiches.iter().enumerate() {
                let t = if f.jeu.trim().is_empty() { f.titre.clone() } else { format!("{} · 🎮 {}", f.titre, f.jeu) };
                if ui.selectable_label(self.choix == k, t).clicked() {
                    self.choix = k;
                }
            }
            if ui.button("➕ Fiche").clicked() {
                r.fiches.push(Fiche { titre: format!("Fiche {}", r.fiches.len() + 1), jeu: crate::outil::jeu_actif().unwrap_or_default(), ..Default::default() });
                self.choix = r.fiches.len() - 1;
            }
        });
        if let Some(f) = r.fiches.get_mut(self.choix) {
            ui.horizontal(|ui| {
                ui.label("Titre");
                ui.add(egui::TextEdit::singleline(&mut f.titre).desired_width(180.0));
                ui.label("Jeu (vide : tous)");
                ui.add(egui::TextEdit::singleline(&mut f.jeu).desired_width(160.0).hint_text("ex. Elden Ring"));
            });
            ui.add(egui::TextEdit::multiline(&mut f.texte).desired_rows(10).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.button("🖼 Image (carte, schéma)…").clicked() {
                    f.image = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file();
                }
                if let Some(p) = &f.image {
                    ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    if ui.small_button("✖").clicked() {
                        f.image = None;
                    }
                }
            });
        }
        if r.fiches.len() > 1 && ui.small_button("🗑 Supprimer cette fiche").clicked() {
            r.fiches.remove(self.choix.min(r.fiches.len() - 1));
            self.choix = 0;
        }
        if r != avant {
            enregistrer("guide_jeu", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide() {
        let mut r = Reglages::default();
        r.fiches.push(Fiche { titre: "Boss".into(), jeu: "elden ring".into(), texte: "Esquive à gauche".into(), image: None });
        assert_eq!(fiches_pour(&r, Some("ELDEN RING")), vec![1]);
        assert_eq!(fiches_pour(&r, Some("Minecraft")), vec![0]);
        assert_eq!(fiches_pour(&r, None), vec![0]);
        let i = rendre(&r.fiches[0], &r, None, (0, 1));
        assert_eq!(i.width(), 420);
        assert_eq!(i.get_pixel(0, 0)[3], 0); // coin arrondi
    }
}
