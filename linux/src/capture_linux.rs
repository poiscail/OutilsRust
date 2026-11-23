//! Captures d'écran et enregistrement vidéo sous Linux (X11 et Wayland), avec les outils du bureau :
//! GNOME, KDE (Spectacle), grim/slurp (Sway, Hyprland), scrot, ffmpeg, wf-recorder.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn existe(prog: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(prog).is_file())
}

fn wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

fn bureau() -> String {
    std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase()
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Reglages {
    dossier: PathBuf,
    delai: u32,
}

impl Default for Reglages {
    fn default() -> Self {
        let images = Command::new("xdg-user-dir").arg("PICTURES").output().ok().map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim())).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Images"));
        Reglages { dossier: images.join("Captures"), delai: 0 }
    }
}

/// Commande de capture selon l'environnement : (programme, arguments). `zone` : choisir une zone.
pub fn commande_capture(zone: bool, dest: &str, wl: bool, de: &str, dispo: &dyn Fn(&str) -> bool) -> Option<(String, Vec<String>)> {
    if de.contains("gnome") && dispo("gnome-screenshot") {
        let mut a = vec!["-f".to_string(), dest.to_string()];
        if zone {
            a.insert(0, "-a".into());
        }
        return Some(("gnome-screenshot".into(), a));
    }
    if de.contains("kde") && dispo("spectacle") {
        return Some(("spectacle".into(), vec!["-b".into(), "-n".into(), if zone { "-r".into() } else { "-f".into() }, "-o".into(), dest.into()]));
    }
    if wl && dispo("grim") {
        if zone && dispo("slurp") {
            return Some(("sh".into(), vec!["-c".into(), "grim -g \"$(slurp)\" \"$1\"".into(), "capture".into(), dest.into()]));
        }
        return Some(("grim".into(), vec![dest.into()]));
    }
    if dispo("scrot") {
        let mut a = vec![dest.to_string()];
        if zone {
            a.insert(0, "-s".into());
        }
        return Some(("scrot".into(), a));
    }
    if dispo("import") && !wl {
        return Some(("import".into(), if zone { vec![dest.into()] } else { vec!["-window".into(), "root".into(), dest.into()] }));
    }
    if dispo("gnome-screenshot") {
        return Some(("gnome-screenshot".into(), vec!["-f".into(), dest.into()]));
    }
    None
}

/// Commande d'enregistrement vidéo de l'écran.
fn commande_video(dest: &str) -> Option<(String, Vec<String>)> {
    if wayland() {
        if existe("wf-recorder") {
            return Some(("wf-recorder".into(), vec!["-f".into(), dest.into()]));
        }
        return None;
    }
    if existe("ffmpeg") {
        let ecran = std::env::var("DISPLAY").unwrap_or(":0".into());
        return Some(("ffmpeg".into(), vec!["-y", "-f", "x11grab", "-framerate", "30", "-i", &ecran, "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", dest].into_iter().map(String::from).collect()));
    }
    None
}

pub struct Capture {
    r: Reglages,
    derniere: Option<PathBuf>,
    message: String,
    video: Option<(Child, Instant, PathBuf)>,
}

impl Capture {
    pub fn new() -> Self {
        Capture { r: charger("capture_linux"), derniere: None, message: String::new(), video: None }
    }

    fn capturer(&mut self, zone: bool) {
        let _ = std::fs::create_dir_all(&self.r.dossier);
        let dest = self.r.dossier.join(format!("Capture_{}.png", crate::minecraft::horodatage()));
        let Some((p, a)) = commande_capture(zone, &dest.display().to_string(), wayland(), &bureau(), &existe) else {
            self.message = "Aucun outil de capture trouvé : installe gnome-screenshot, spectacle, grim+slurp ou scrot.".into();
            return;
        };
        if self.r.delai > 0 {
            std::thread::sleep(Duration::from_secs(self.r.delai as u64));
        }
        match Command::new(&p).args(&a).status() {
            Ok(_) if dest.is_file() => {
                self.message = format!("✔ {}", dest.display());
                self.derniere = Some(dest);
            }
            _ => self.message = "Capture annulée ou impossible.".into(),
        }
    }
}

impl Outil for Capture {
    fn nom(&self) -> &'static str {
        "📸 Capture d'écran et vidéo"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Capture d'écran et vidéo");
        ui.horizontal_wrapped(|ui| {
            if ui.button(RichText::new("🖥 Tout l'écran").size(16.0)).clicked() {
                self.capturer(false);
            }
            if ui.button(RichText::new("✂ Une zone").size(16.0)).clicked() {
                self.capturer(true);
            }
            ui.label("Délai :");
            if ui.add(egui::DragValue::new(&mut self.r.delai).range(0..=10).suffix(" s")).changed() {
                enregistrer("capture_linux", &self.r);
            }
            if ui.button("📂 Dossier").clicked() {
                let _ = std::fs::create_dir_all(&self.r.dossier);
                crate::outil::ouvrir(&self.r.dossier);
            }
        });
        ui.separator();
        match &mut self.video {
            None => {
                if ui.button(RichText::new("⏺ Enregistrer l'écran").size(16.0)).clicked() {
                    let _ = std::fs::create_dir_all(&self.r.dossier);
                    let dest = self.r.dossier.join(format!("Video_{}.mp4", crate::minecraft::horodatage()));
                    match commande_video(&dest.display().to_string()) {
                        Some((p, a)) => match Command::new(p).args(a).stdin(std::process::Stdio::piped()).spawn() {
                            Ok(c) => self.video = Some((c, Instant::now(), dest)),
                            Err(e) => self.message = format!("✖ {e}"),
                        },
                        None => {
                            self.message = if wayland() {
                                if bureau().contains("gnome") { "Sous GNOME (Wayland), appuie sur Ctrl+Maj+Alt+R pour enregistrer l'écran, ou installe wf-recorder (Sway, Hyprland).".into() } else { "Installe wf-recorder pour enregistrer l'écran (ou OBS Studio).".into() }
                            } else {
                                "Installe ffmpeg pour enregistrer l'écran.".into()
                            }
                        }
                    }
                }
            }
            Some((c, t, dest)) => {
                let mut arreter = false;
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::LIGHT_RED, format!("● Enregistrement {} s", t.elapsed().as_secs()));
                    arreter = ui.button("⏹ Arrêter").clicked();
                });
                {
                    if arreter {
                        // ffmpeg s'arrête proprement avec « q », wf-recorder avec SIGINT
                        if let Some(mut i) = c.stdin.take() {
                            let _ = std::io::Write::write_all(&mut i, b"q");
                        }
                        unsafe { libc::kill(c.id() as i32, libc::SIGINT) };
                        let _ = c.wait();
                        self.message = format!("✔ {}", dest.display());
                        self.video = None;
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        }
        ui.label(&self.message);
        if let Some(p) = &self.derniere {
            let l = ui.available_width().min(560.0);
            ui.add(egui::Image::new(format!("file://{}", p.display())).max_width(l));
            if ui.button("📋 Copier l'image").clicked() {
                let outil = if wayland() { ("wl-copy", vec!["-t", "image/png"]) } else { ("xclip", vec!["-selection", "clipboard", "-t", "image/png"]) };
                if let Ok(mut c) = Command::new(outil.0).args(outil.1).stdin(std::process::Stdio::piped()).spawn() {
                    if let (Some(mut i), Ok(b)) = (c.stdin.take(), std::fs::read(p)) {
                        let _ = std::io::Write::write_all(&mut i, &b);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choix() {
        let tout = |_: &str| true;
        assert_eq!(commande_capture(true, "/t.png", true, "gnome", &tout).unwrap().0, "gnome-screenshot");
        assert_eq!(commande_capture(false, "/t.png", true, "kde", &tout).unwrap().0, "spectacle");
        assert_eq!(commande_capture(true, "/t.png", true, "sway", &tout).unwrap().0, "sh");
        let seulement_scrot = |p: &str| p == "scrot";
        assert_eq!(commande_capture(true, "/t.png", false, "xfce", &seulement_scrot).unwrap().1, vec!["-s", "/t.png"]);
        assert!(commande_capture(false, "/t.png", true, "x", &|_: &str| false).is_none());
    }
}
