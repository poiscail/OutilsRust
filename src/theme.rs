//! Personnalisation : couleurs, transparence, zoom et image de fond.
//! Réglages dans %APPDATA%\Outils\theme.json.
use eframe::egui::{self, Color32, Pos2, Rect};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    sombre: bool,
    accent: [u8; 3],
    fond: [u8; 3],
    panneau: [u8; 3],
    texte: [u8; 3],
    opacite: f32,   // des panneaux, 0..1
    assombrir: f32, // voile noir sur l'image, 0..1
    zoom: f32,
    image: Option<PathBuf>,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { sombre: true, accent: [90, 140, 220], fond: [20, 20, 24], panneau: [27, 27, 27], texte: [210, 210, 210], opacite: 1.0, assombrir: 0.3, zoom: 1.0, image: None }
    }
}

/// (nom, réglages) des thèmes prêts à l'emploi (l'image reste celle choisie)
fn prereglages() -> Vec<(&'static str, Reglages)> {
    let d = Reglages::default();
    vec![
        ("Sombre", d.clone()),
        ("Clair", Reglages { sombre: false, accent: [40, 110, 210], fond: [235, 235, 240], panneau: [248, 248, 248], texte: [30, 30, 30], ..d.clone() }),
        ("Sakura nuit", Reglages { accent: [255, 95, 162], fond: [18, 10, 16], panneau: [30, 16, 26], texte: [245, 220, 232], opacite: 0.78, assombrir: 0.35, ..d.clone() }),
        ("Néon Tokyo", Reglages { accent: [255, 60, 200], fond: [10, 8, 20], panneau: [20, 14, 34], texte: [235, 225, 255], opacite: 0.72, assombrir: 0.4, ..d }),
    ]
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("Outils").join("theme.json")
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn melange(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Zone de l'image (en coordonnées 0..1) à afficher pour couvrir l'écran sans déformer.
fn uv_couvrir(ecran: [f32; 2], image: [f32; 2]) -> Rect {
    let echelle = (ecran[0] / image[0]).max(ecran[1] / image[1]);
    let (lu, lv) = (ecran[0] / (image[0] * echelle), ecran[1] / (image[1] * echelle));
    Rect::from_min_max(Pos2::new(0.5 - lu / 2.0, 0.5 - lv / 2.0), Pos2::new(0.5 + lu / 2.0, 0.5 + lv / 2.0))
}

pub struct Theme {
    r: Reglages,
    a_appliquer: bool,
    texture: Option<egui::TextureHandle>,
    texture_de: Option<PathBuf>,
    dossier_fonds: PathBuf,
    erreur: Option<String>,
}

impl Theme {
    pub fn new() -> Self {
        let r = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let exe = std::env::current_exe().unwrap_or_default();
        let dossier_fonds = exe.parent().unwrap_or(Path::new(".")).join("fonds");
        Theme { r, a_appliquer: true, texture: None, texture_de: None, dossier_fonds, erreur: None }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(s) = serde_json::to_string_pretty(&self.r) {
            let _ = std::fs::write(f, s);
        }
    }

    fn appliquer(&self, ctx: &egui::Context) {
        let r = &self.r;
        let mut v = if r.sombre { egui::Visuals::dark() } else { egui::Visuals::light() };
        let alpha = (r.opacite.clamp(0.0, 1.0) * 255.0) as u8;
        let p = r.panneau;
        v.panel_fill = Color32::from_rgba_unmultiplied(p[0], p[1], p[2], alpha);
        v.window_fill = rgb(p);
        v.extreme_bg_color = rgb(melange(p, r.fond, 0.6));
        v.faint_bg_color = rgb(melange(p, r.texte, 0.04));
        v.override_text_color = Some(rgb(r.texte));
        v.hyperlink_color = rgb(r.accent);
        v.selection.bg_fill = rgb(melange(p, r.accent, 0.75));
        v.selection.stroke.color = rgb(r.texte);
        let w = &mut v.widgets;
        w.inactive.weak_bg_fill = rgb(melange(p, r.accent, 0.18));
        w.inactive.bg_fill = w.inactive.weak_bg_fill;
        w.hovered.weak_bg_fill = rgb(melange(p, r.accent, 0.45));
        w.hovered.bg_fill = w.hovered.weak_bg_fill;
        w.hovered.bg_stroke.color = rgb(r.accent);
        w.active.weak_bg_fill = rgb(r.accent);
        w.active.bg_fill = rgb(r.accent);
        w.open.weak_bg_fill = rgb(melange(p, r.accent, 0.3));
        ctx.set_visuals(v);
        ctx.set_zoom_factor(r.zoom.clamp(0.6, 2.0));
    }

    fn charger_image(&mut self, ctx: &egui::Context) {
        if self.texture_de == self.r.image {
            return;
        }
        self.texture_de = self.r.image.clone();
        self.texture = None;
        let Some(p) = &self.r.image else { return };
        match image::open(p) {
            Ok(img) => {
                let img = img.to_rgba8();
                let taille = [img.width() as usize, img.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(taille, img.as_raw());
                self.texture = Some(ctx.load_texture("fond", ci, egui::TextureOptions::LINEAR));
                self.erreur = None;
            }
            Err(e) => self.erreur = Some(format!("Image illisible : {e}")),
        }
    }

    /// À appeler au début de chaque image, avant les panneaux : peint le fond dessous.
    pub fn peindre(&mut self, ctx: &egui::Context) {
        if self.a_appliquer {
            self.appliquer(ctx);
            self.a_appliquer = false;
        }
        self.charger_image(ctx);
        let ecran = ctx.screen_rect();
        let peintre = ctx.layer_painter(egui::LayerId::background());
        peintre.rect_filled(ecran, 0.0, rgb(self.r.fond));
        if let Some(t) = &self.texture {
            let s = t.size_vec2();
            peintre.image(t.id(), ecran, uv_couvrir([ecran.width(), ecran.height()], [s.x, s.y]), Color32::WHITE);
            let voile = (self.r.assombrir.clamp(0.0, 1.0) * 255.0) as u8;
            peintre.rect_filled(ecran, 0.0, Color32::from_black_alpha(voile));
        }
    }

    fn fonds_dispo(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.dossier_fonds).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")))
            .collect();
        v.sort();
        v
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.r.clone();
        ui.heading("Personnaliser");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Thèmes :");
            for (nom, p) in prereglages() {
                if ui.button(nom).clicked() {
                    let image = self.r.image.take();
                    self.r = Reglages { image, ..p };
                }
            }
        });
        ui.separator();
        egui::Grid::new("couleurs").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Mode");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.r.sombre, true, "Sombre");
                ui.radio_value(&mut self.r.sombre, false, "Clair");
            });
            ui.end_row();
            for (nom, c) in [("Accent", &mut self.r.accent), ("Fond", &mut self.r.fond), ("Panneaux", &mut self.r.panneau), ("Texte", &mut self.r.texte)] {
                ui.label(nom);
                ui.color_edit_button_srgb(c);
                ui.end_row();
            }
            ui.label("Opacité des panneaux");
            ui.add(egui::Slider::new(&mut self.r.opacite, 0.0..=1.0));
            ui.end_row();
            ui.label("Assombrir l'image");
            ui.add(egui::Slider::new(&mut self.r.assombrir, 0.0..=1.0));
            ui.end_row();
            ui.label("Taille de l'interface");
            ui.add(egui::Slider::new(&mut self.r.zoom, 0.6..=2.0).step_by(0.05));
            ui.end_row();
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Image de fond :");
            ui.label(self.r.image.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(|| "aucune".into()));
            if ui.button("Choisir…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                    self.r.image = Some(p);
                }
            }
            if self.r.image.is_some() && ui.button("Retirer").clicked() {
                self.r.image = None;
            }
            if ui.button("Dossier des fonds").clicked() {
                let _ = std::fs::create_dir_all(&self.dossier_fonds);
                let _ = std::process::Command::new("explorer").arg(&self.dossier_fonds).spawn();
            }
        });
        if self.r.opacite > 0.95 && self.r.image.is_some() {
            ui.colored_label(Color32::GRAY, "Baissez l'opacité des panneaux pour voir l'image.");
        }
        if let Some(e) = &self.erreur {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let fonds = self.fonds_dispo();
        if !fonds.is_empty() {
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for p in fonds {
                        let choisi = self.r.image.as_ref() == Some(&p);
                        let img = egui::Image::new(format!("file://{}", p.display())).fit_to_exact_size(egui::vec2(192.0, 108.0));
                        if ui.add(egui::ImageButton::new(img).selected(choisi)).on_hover_text(p.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                            self.r.image = Some(p);
                        }
                    }
                });
            });
        }
        if self.r != avant {
            self.a_appliquer = true;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn couvrir() {
        // image plus large que l'écran : on coupe les côtés
        let r = uv_couvrir([100.0, 100.0], [200.0, 100.0]);
        assert!((r.min.x - 0.25).abs() < 1e-6 && (r.max.x - 0.75).abs() < 1e-6);
        assert!((r.min.y - 0.0).abs() < 1e-6 && (r.max.y - 1.0).abs() < 1e-6);
        // même ratio : image entière
        let r = uv_couvrir([1920.0, 1080.0], [3840.0, 2160.0]);
        assert!((r.width() - 1.0).abs() < 1e-6 && (r.height() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reglages_json() {
        let r: Reglages = serde_json::from_str(r#"{"accent":[1,2,3]}"#).unwrap();
        assert_eq!(r.accent, [1, 2, 3]);
        assert!(r.sombre && r.image.is_none());
        assert_eq!(melange([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
    }
}
