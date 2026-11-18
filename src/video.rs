//! Vidéo rapide (ffmpeg) : compresser sous une taille pour Discord (85, images aussi), couper le début et la fin
//! sans réencoder (86), extraire le son en MP3 (87).
use crate::outil::{Cat, Outil};
use eframe::egui::{self, RichText};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const SANS_FENETRE: u32 = 0x0800_0000;

/// « 1:23 », « 01:02:03.5 », « 83 » → secondes.
pub fn lire_temps(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    t.split(':').try_fold(0.0, |acc, p| p.trim().parse::<f64>().ok().map(|v| acc * 60.0 + v))
}

/// Débit vidéo (kbit/s) pour tenir dans `mo` Mo avec 5 % de marge.
pub fn debit_video(mo: f64, duree: f64, audio_kbps: f64) -> f64 {
    let total = mo * 8.0 * 1024.0 * 0.95 / duree.max(1.0); // kbit/s (Mo → Mibit… arrondi prudent)
    (total - audio_kbps).max(100.0)
}

fn duree(p: &Path) -> Option<f64> {
    let o = Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"]).arg(p).creation_flags(SANS_FENETRE).output().ok()?;
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

fn ffmpeg(args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let o = Command::new("ffmpeg").args(["-y", "-v", "error"]).args(args).creation_flags(SANS_FENETRE).output()
        .map_err(|_| "ffmpeg introuvable : winget install ffmpeg".to_string())?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec").to_string()) }
}

fn sortie(p: &Path, s: &str, ext: &str) -> PathBuf {
    p.with_file_name(format!("{} {s}.{ext}", p.file_stem().unwrap_or_default().to_string_lossy()))
}

/// Image ramenée sous `max` octets : JPEG de qualité décroissante, puis réduction.
pub fn image_sous(p: &Path, max: u64) -> Result<PathBuf, String> {
    let mut img = image::open(p).map_err(|e| e.to_string())?;
    let dest = sortie(p, "(compressée)", "jpg");
    for _ in 0..12 {
        for q in [90u8, 80, 70, 60] {
            let mut buf = vec![];
            image::DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, q)).map_err(|e| e.to_string())?;
            if buf.len() as u64 <= max {
                std::fs::write(&dest, buf).map_err(|e| e.to_string())?;
                return Ok(dest);
            }
        }
        img = img.resize((img.width() * 4 / 5).max(1), (img.height() * 4 / 5).max(1), image::imageops::FilterType::Lanczos3);
    }
    Err("impossible d'atteindre cette taille".into())
}

fn video_sous(p: &Path, mo: f64) -> Result<PathBuf, String> {
    let d = duree(p).ok_or("durée illisible (ffprobe introuvable ?)")?;
    let v = debit_video(mo, d, 96.0);
    let dest = sortie(p, "(compressée)", "mp4");
    let passe = std::env::temp_dir().join("outils_passe");
    let b = format!("{}k", v as u64);
    let commun = |n: &str| -> Vec<std::ffi::OsString> {
        ["-i".into(), p.as_os_str().to_owned(), "-c:v".into(), "libx264".into(), "-preset".into(), "medium".into(), "-b:v".into(), b.clone().into(),
         "-pass".into(), n.into(), "-passlogfile".into(), passe.as_os_str().to_owned(), "-vf".into(), "scale='min(1920,iw)':-2".into()].into_iter().collect()
    };
    let mut a1 = commun("1");
    a1.extend(["-an".into(), "-f".into(), "mp4".into(), "NUL".into()]);
    ffmpeg(&a1.iter().map(|s| s.as_os_str()).collect::<Vec<_>>())?;
    let mut a2 = commun("2");
    a2.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "96k".into(), "-movflags".into(), "+faststart".into(), dest.as_os_str().to_owned()]);
    ffmpeg(&a2.iter().map(|s| s.as_os_str()).collect::<Vec<_>>())?;
    Ok(dest)
}

pub struct VideoRapide {
    fichier: Option<PathBuf>,
    mo: f64,
    debut: String,
    fin: String,
    precis: bool,
    etat: Arc<Mutex<(bool, String)>>,
}

impl VideoRapide {
    pub fn new() -> Self {
        VideoRapide { fichier: None, mo: 10.0, debut: "0:00".into(), fin: String::new(), precis: false, etat: Arc::new(Mutex::new((false, String::new()))) }
    }

    fn lancer(&self, f: impl FnOnce() -> Result<PathBuf, String> + Send + 'static) {
        let e = self.etat.clone();
        *e.lock().unwrap() = (true, "En cours…".into());
        std::thread::spawn(move || {
            let r = f();
            *e.lock().unwrap() = (false, match r {
                Ok(p) => format!("✔ {} ({})", p.display(), crate::disque::taille_lisible(p.metadata().map(|m| m.len()).unwrap_or(0))),
                Err(m) => format!("✖ {m}"),
            });
        });
    }
}

impl Outil for VideoRapide {
    fn nom(&self) -> &'static str {
        "🎞 Vidéo rapide (Discord, découpe, son)"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Vidéo rapide");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.fichier = Some(p);
        }
        let (occupe, msg) = self.etat.lock().unwrap().clone();
        ui.horizontal(|ui| {
            if ui.button("📂 Fichier…").clicked() {
                self.fichier = rfd::FileDialog::new().pick_file();
            }
            match &self.fichier {
                Some(p) => {
                    ui.strong(p.file_name().unwrap_or_default().to_string_lossy());
                    ui.weak(crate::disque::taille_lisible(p.metadata().map(|m| m.len()).unwrap_or(0)));
                }
                None => {
                    ui.weak("ou glisse une vidéo / image ici");
                }
            }
        });
        let Some(p) = self.fichier.clone() else { return };
        let est_image = crate::convertisseur::genre(&p) == crate::convertisseur::Genre::Image;
        ui.separator();
        ui.strong("📦 Compresser pour Discord");
        ui.horizontal(|ui| {
            for (n, v) in [("10 Mo (gratuit)", 10.0), ("50 Mo (Nitro Basic)", 50.0), ("500 Mo (Nitro)", 500.0)] {
                ui.selectable_value(&mut self.mo, v, n);
            }
            ui.add(egui::DragValue::new(&mut self.mo).range(1.0..=2000.0).suffix(" Mo"));
            if ui.add_enabled(!occupe, egui::Button::new("Compresser")).clicked() {
                let (p, mo) = (p.clone(), self.mo);
                self.lancer(move || if est_image { image_sous(&p, (mo * 1024.0 * 1024.0) as u64) } else { video_sous(&p, mo) });
            }
        });
        if !est_image {
            ui.separator();
            ui.strong("✂ Couper");
            ui.horizontal(|ui| {
                ui.label("de");
                ui.add(egui::TextEdit::singleline(&mut self.debut).desired_width(70.0));
                ui.label("à");
                ui.add(egui::TextEdit::singleline(&mut self.fin).hint_text("fin").desired_width(70.0));
                ui.checkbox(&mut self.precis, "précis à l'image près (plus lent, réencode)");
                if ui.add_enabled(!occupe, egui::Button::new("Couper")).clicked() {
                    let (d, f) = (lire_temps(&self.debut).unwrap_or(0.0), lire_temps(&self.fin));
                    let (p, precis) = (p.clone(), self.precis);
                    self.lancer(move || {
                        let ext = p.extension().unwrap_or_default().to_string_lossy().into_owned();
                        let dest = sortie(&p, "(coupé)", &ext);
                        // -ss et -to avant -i : positions dans le fichier d'origine
                        let mut a: Vec<String> = vec!["-ss".into(), format!("{d}")];
                        if let Some(f) = f {
                            a.extend(["-to".into(), format!("{f}")]);
                        }
                        a.extend(["-i".into(), p.display().to_string()]);
                        if precis {
                            a.extend(["-c:v".into(), "libx264".into(), "-crf".into(), "18".into(), "-c:a".into(), "aac".into()]);
                        } else {
                            a.extend(["-c".into(), "copy".into(), "-avoid_negative_ts".into(), "make_zero".into()]);
                        }
                        a.push(dest.display().to_string());
                        ffmpeg(&a.iter().map(std::ffi::OsStr::new).collect::<Vec<_>>()).map(|_| dest)
                    });
                }
            });
            ui.weak("Format des temps : 1:23, 0:01:05.5 ou 83 (secondes). Sans « précis », la coupe se cale sur l'image clé la plus proche.");
            ui.separator();
            ui.strong("🎵 Extraire le son");
            if ui.add_enabled(!occupe, egui::Button::new("En MP3")).clicked() {
                let p = p.clone();
                self.lancer(move || {
                    let dest = sortie(&p, "(son)", "mp3");
                    ffmpeg(&["-i".as_ref(), p.as_os_str(), "-vn".as_ref(), "-c:a".as_ref(), "libmp3lame".as_ref(), "-b:a".as_ref(), "192k".as_ref(), dest.as_os_str()]).map(|_| dest)
                });
            }
        }
        if occupe {
            ui.spinner();
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        if !msg.is_empty() {
            ui.label(RichText::new(msg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temps() {
        assert_eq!(lire_temps("1:23"), Some(83.0));
        assert_eq!(lire_temps("01:02:03.5"), Some(3723.5));
        assert_eq!(lire_temps("83"), Some(83.0));
        assert_eq!(lire_temps("a:b"), None);
        assert_eq!(lire_temps(""), None);
    }

    #[test]
    fn debit() {
        // 10 Mo pour 60 s, 96 kbit/s de son : environ 1200 kbit/s de vidéo
        let v = debit_video(10.0, 60.0, 96.0);
        assert!(v > 1100.0 && v < 1300.0, "{v}");
        assert_eq!(debit_video(1.0, 100_000.0, 96.0), 100.0);
    }
}
