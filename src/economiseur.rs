//! Économiseur d'écran : les fonds japonais qui défilent avec les effets (pétales, pluie, neige, lucioles)
//! et une horloge. Installé comme vrai économiseur Windows : copie d'Outils.exe en %APPDATA%\Outils\Outils.scr,
//! que Windows lance avec « /s » après le délai d'inactivité. Réglages dans %APPDATA%\Outils\economiseur.json.
use crate::pressepapiers::{civil, decalage, maintenant_utc};
use crate::theme::Theme;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub minutes: u32,
    pub horloge: bool,
    pub secondes_image: u32,
    pub aleatoire: bool,
    pub dossier_fonds: PathBuf, // écrit par Outils (l'économiseur tourne depuis une copie ailleurs)
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: false, minutes: 10, horloge: true, secondes_image: 30, aleatoire: true, dossier_fonds: PathBuf::new() }
    }
}

fn dossier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils")
}

fn fichier() -> PathBuf {
    dossier().join("economiseur.json")
}

fn scr() -> PathBuf {
    dossier().join("Outils.scr")
}

fn lire() -> Reglages {
    std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// "HH:MM" et "mercredi 18 novembre" en heure locale.
pub fn horloge(utc: i64, off: i64) -> (String, String) {
    const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
    const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
    let l = utc + off;
    let j = l.div_euclid(86400);
    let (_, m, d) = civil(j);
    let s = l.rem_euclid(86400);
    (format!("{:02}:{:02}", s / 3600, s / 60 % 60), format!("{} {d} {}", JOURS[(j + 3).rem_euclid(7) as usize], MOIS[(m - 1) as usize]))
}

fn reg(args: &[&str]) {
    let _ = Command::new("reg.exe").args(args).creation_flags(CREATE_NO_WINDOW).status();
}

const BUREAU: &str = r"HKCU\Control Panel\Desktop";

/// Installe (ou retire) l'économiseur auprès de Windows.
fn appliquer(r: &Reglages) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETSCREENSAVEACTIVE, SPI_SETSCREENSAVETIMEOUT};
    if r.actif {
        copier_scr(true)?;
        reg(&["add", BUREAU, "/v", "SCRNSAVE.EXE", "/t", "REG_SZ", "/f", "/d", &scr().to_string_lossy()]);
        reg(&["add", BUREAU, "/v", "ScreenSaveActive", "/t", "REG_SZ", "/f", "/d", "1"]);
        reg(&["add", BUREAU, "/v", "ScreenSaveTimeOut", "/t", "REG_SZ", "/f", "/d", &(r.minutes.max(1) * 60).to_string()]);
        unsafe {
            SystemParametersInfoW(SPI_SETSCREENSAVETIMEOUT, r.minutes.max(1) * 60, std::ptr::null_mut(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE);
            SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE, 1, std::ptr::null_mut(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE);
        }
    } else {
        reg(&["delete", BUREAU, "/v", "SCRNSAVE.EXE", "/f"]);
        unsafe { SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE, 0, std::ptr::null_mut(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    }
    Ok(())
}

/// Copie Outils.exe en Outils.scr (si absent, ou différent après une mise à jour).
fn copier_scr(forcer: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe == scr() {
        return Ok(());
    }
    let taille = |p: &PathBuf| p.metadata().map(|m| m.len()).ok();
    if forcer || taille(&exe) != taille(&scr()) {
        std::fs::create_dir_all(dossier()).map_err(|e| e.to_string())?;
        std::fs::copy(&exe, scr()).map_err(|e| format!("copie de l'économiseur : {e}"))?;
    }
    Ok(())
}

// ---------- l'économiseur lui-même (Outils.scr /s) ----------

struct Ecran {
    theme: Theme,
    r: Reglages,
    images: Vec<PathBuf>,
    index: usize,
    change: Instant,
    debut: Instant,
    souris: Option<egui::Pos2>,
}

impl eframe::App for Ecran {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if !self.images.is_empty() && (self.theme.image().is_none() || self.change.elapsed() >= Duration::from_secs(self.r.secondes_image.max(5) as u64)) {
            self.index = if self.r.aleatoire && self.images.len() > 1 {
                let mut i = (crate::pressepapiers::maintenant_utc() as usize).wrapping_mul(2_654_435_761) % self.images.len();
                if i == self.index {
                    i = (i + 1) % self.images.len();
                }
                i
            } else {
                self.index.wrapping_add(1) % self.images.len()
            };
            self.theme.choisir_image(Some(self.images[self.index].clone()));
            self.change = Instant::now();
        }
        self.theme.peindre(ctx);
        ctx.set_cursor_icon(egui::CursorIcon::None);
        if self.r.horloge {
            let (h, d) = horloge(maintenant_utc(), decalage());
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("horloge")));
            let e = ctx.screen_rect();
            let pos = egui::pos2(e.left() + 70.0, e.bottom() - 90.0);
            for (dx, c) in [(3.0, Color32::from_black_alpha(140)), (0.0, Color32::from_rgb(255, 235, 245))] {
                p.text(pos + egui::vec2(dx, dx), egui::Align2::LEFT_BOTTOM, &h, egui::FontId::proportional(110.0), c);
                p.text(pos + egui::vec2(6.0 + dx, 30.0 + dx), egui::Align2::LEFT_BOTTOM, &d, egui::FontId::proportional(30.0), c);
            }
        }
        // n'importe quelle action ferme l'économiseur (après une petite seconde, le temps que tout se pose)
        let fermer = self.debut.elapsed() > Duration::from_millis(800) && ctx.input(|i| {
            let bouge = match (i.pointer.latest_pos(), self.souris) {
                (Some(p), Some(s)) => p.distance(s) > 10.0,
                _ => false,
            };
            bouge || i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_) | egui::Event::MouseWheel { .. }))
        });
        if self.souris.is_none() {
            self.souris = ctx.input(|i| i.pointer.latest_pos());
        }
        if fermer {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(33));
    }
}

/// Point d'entrée quand Windows lance Outils.scr /s (ou « Aperçu »).
pub fn lancer() -> eframe::Result {
    let r = lire();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_fullscreen(true).with_decorations(false).with_always_on_top().with_title("Outils - économiseur"),
        ..Default::default()
    };
    eframe::run_native("Outils économiseur", options, Box::new(move |cc| {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let theme = Theme::pour_economiseur(r.dossier_fonds.clone());
        let images = theme.fonds();
        Ok(Box::new(Ecran { theme, r, images, index: usize::MAX, change: Instant::now(), debut: Instant::now(), souris: None }))
    }))
}

// ---------- onglet dans Outils ----------

pub struct Economiseur {
    r: Reglages,
    message: String,
}

impl Economiseur {
    pub fn new(dossier_fonds: PathBuf) -> Self {
        let mut r = lire();
        let mut e = Economiseur { r: r.clone(), message: String::new() };
        if r.dossier_fonds != dossier_fonds {
            r.dossier_fonds = dossier_fonds;
            e.r = r;
            e.enregistrer();
        }
        if e.r.actif {
            let _ = copier_scr(false); // remet à jour la copie après une mise à jour d'Outils
        }
        e
    }

    fn enregistrer(&self) {
        let _ = std::fs::create_dir_all(dossier());
        let _ = std::fs::write(fichier(), serde_json::to_string_pretty(&self.r).unwrap_or_default());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Économiseur d'écran");
        let avant = self.r.clone();
        ui.checkbox(&mut self.r.actif, "Utiliser l'économiseur d'Outils");
        ui.horizontal(|ui| {
            ui.label("Après");
            ui.add(egui::DragValue::new(&mut self.r.minutes).range(1..=240).suffix(" min"));
            ui.label("sans toucher au PC");
        });
        ui.checkbox(&mut self.r.horloge, "Afficher l'heure et la date");
        ui.horizontal(|ui| {
            ui.label("Changer d'image toutes les");
            ui.add(egui::DragValue::new(&mut self.r.secondes_image).range(5..=600).suffix(" s"));
            ui.checkbox(&mut self.r.aleatoire, "dans le désordre");
        });
        ui.weak("Les images viennent du dossier des fonds (onglet Personnaliser) ; les effets (pétales, pluie, neige, lucioles) et l'assombrissement aussi.");
        ui.horizontal(|ui| {
            if ui.button("👁 Aperçu").on_hover_text("Bouge la souris ou appuie sur une touche pour quitter").clicked() {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = Command::new(exe).arg("/s").spawn();
                }
            }
            if ui.button("Réglages Windows de l'écran de veille").clicked() {
                let _ = Command::new("control.exe").arg("desk.cpl,,@screensaver").spawn();
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if self.r != avant {
            self.message = match appliquer(&self.r) {
                Ok(()) if self.r.actif => format!("Économiseur actif après {} min d'inactivité.", self.r.minutes),
                Ok(()) => "Économiseur désactivé.".into(),
                Err(e) => format!("Échec : {e}"),
            };
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heure_et_date() {
        let utc = crate::pressepapiers::jours(2026, 11, 18) * 86400 + 19 * 3600 + 5 * 60;
        assert_eq!(horloge(utc, 3600), ("20:05".to_string(), "mercredi 18 novembre".to_string()));
    }
}
