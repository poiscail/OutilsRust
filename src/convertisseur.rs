//! Convertisseur : images en lot (format, qualité, redimensionnement) avec la crate image,
//! vidéos et sons avec ffmpeg (compression, extraction audio, GIF).
use eframe::egui;
use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageFormat};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

pub const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff", "ico"];
pub const MEDIAS: &[&str] = &["mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mp3", "wav", "flac", "ogg", "m4a", "aac", "opus", "wma"];
const SORTIES_IMAGE: &[&str] = &["png", "jpg", "webp", "bmp", "ico"];
const SORTIES_MEDIA: &[&str] = &["mp4", "mkv", "webm", "gif", "mp3", "m4a", "ogg", "flac", "wav"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Genre {
    Image,
    Media,
    Autre,
}

pub fn genre(p: &Path) -> Genre {
    let e = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if IMAGES.contains(&e.as_str()) {
        Genre::Image
    } else if MEDIAS.contains(&e.as_str()) {
        Genre::Media
    } else {
        Genre::Autre
    }
}

#[derive(Clone)]
pub struct Options {
    pub format_image: String,
    pub qualite: u8, // JPEG
    pub largeur_max: u32,
    pub hauteur_max: u32, // 0 = pas de limite (images et vidéos)
    pub format_media: String,
    pub crf: u8,          // qualité vidéo : plus bas = meilleur
    pub audio_kbps: u32,
    pub gif_fps: u32,
    pub dossier: Option<PathBuf>, // None = à côté de l'original
}

impl Default for Options {
    fn default() -> Self {
        Options {
            format_image: "jpg".into(), qualite: 88, largeur_max: 0, hauteur_max: 0, format_media: "mp4".into(), crf: 23, audio_kbps: 192, gif_fps: 15, dossier: None,
        }
    }
}

/// Chemin de sortie libre : nom.ext, sinon nom (2).ext, etc. Jamais l'original.
pub fn sortie(entree: &Path, dossier: Option<&Path>, ext: &str) -> PathBuf {
    let d = dossier.map(Path::to_path_buf).unwrap_or_else(|| entree.parent().map(Path::to_path_buf).unwrap_or_default());
    let base = entree.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let mut p = d.join(format!("{base}.{ext}"));
    let mut i = 2;
    while p.exists() || p == entree {
        p = d.join(format!("{base} ({i}).{ext}"));
        i += 1;
    }
    p
}

/// Taille finale en gardant les proportions, sans jamais agrandir.
pub fn dimensions(w: u32, h: u32, lmax: u32, hmax: u32) -> (u32, u32) {
    let mut f = 1f64;
    if lmax > 0 && w > lmax {
        f = f.min(lmax as f64 / w as f64);
    }
    if hmax > 0 && h > hmax {
        f = f.min(hmax as f64 / h as f64);
    }
    (((w as f64 * f).round() as u32).max(1), ((h as f64 * f).round() as u32).max(1))
}

pub fn convertir_image(entree: &Path, o: &Options) -> Result<PathBuf, String> {
    let mut img = image::open(entree).map_err(|e| e.to_string())?;
    let (w, h) = dimensions(img.width(), img.height(), o.largeur_max, o.hauteur_max);
    let ext = o.format_image.as_str();
    let (w, h) = if ext == "ico" { dimensions(w, h, 256, 256) } else { (w, h) };
    if (w, h) != (img.width(), img.height()) {
        img = img.resize_exact(w, h, FilterType::Lanczos3);
    }
    let dest = sortie(entree, o.dossier.as_deref(), ext);
    let f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
    let mut ecrivain = std::io::BufWriter::new(f);
    let r = match ext {
        "jpg" => DynamicImage::ImageRgb8(img.to_rgb8()).write_with_encoder(JpegEncoder::new_with_quality(&mut ecrivain, o.qualite.clamp(1, 100))),
        "png" => img.write_to(&mut ecrivain, ImageFormat::Png),
        "webp" => DynamicImage::ImageRgba8(img.to_rgba8()).write_to(&mut ecrivain, ImageFormat::WebP), // sans perte
        "bmp" => DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut ecrivain, ImageFormat::Bmp),
        "ico" => DynamicImage::ImageRgba8(img.to_rgba8()).write_to(&mut ecrivain, ImageFormat::Ico),
        _ => return Err(format!("format inconnu : {ext}")),
    };
    drop(ecrivain);
    if let Err(e) = r {
        let _ = std::fs::remove_file(&dest);
        return Err(e.to_string());
    }
    Ok(dest)
}

/// Arguments ffmpeg (après -i entrée) pour le format voulu.
pub fn args_media(o: &Options) -> Vec<String> {
    let s = |x: &str| x.to_string();
    let echelle = (o.hauteur_max > 0).then(|| format!("scale=-2:'min({},ih)'", o.hauteur_max));
    let ab = format!("{}k", o.audio_kbps);
    let mut a: Vec<String> = vec![];
    match o.format_media.as_str() {
        "mp4" | "mkv" => {
            a.extend([s("-c:v"), s("libx264"), s("-preset"), s("medium"), s("-crf"), o.crf.to_string(), s("-c:a"), s("aac"), s("-b:a"), ab]);
            if o.format_media == "mp4" {
                a.extend([s("-movflags"), s("+faststart"), s("-pix_fmt"), s("yuv420p")]);
            }
            if let Some(e) = echelle {
                a.extend([s("-vf"), e]);
            }
        }
        "webm" => {
            a.extend([s("-c:v"), s("libvpx-vp9"), s("-crf"), (o.crf + 8).min(63).to_string(), s("-b:v"), s("0"), s("-c:a"), s("libopus"), s("-b:a"), ab]);
            if let Some(e) = echelle {
                a.extend([s("-vf"), e]);
            }
        }
        "gif" => {
            let h = if o.hauteur_max > 0 { o.hauteur_max } else { 480 };
            a.extend([s("-vf"), format!("fps={},scale=-2:'min({h},ih)':flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse", o.gif_fps.max(1)), s("-an")]);
        }
        "mp3" => a.extend([s("-vn"), s("-c:a"), s("libmp3lame"), s("-b:a"), ab]),
        "m4a" => a.extend([s("-vn"), s("-c:a"), s("aac"), s("-b:a"), ab]),
        "ogg" => a.extend([s("-vn"), s("-c:a"), s("libvorbis"), s("-b:a"), ab]),
        "flac" => a.extend([s("-vn"), s("-c:a"), s("flac")]),
        "wav" => a.extend([s("-vn"), s("-c:a"), s("pcm_s16le")]),
        _ => {}
    }
    a
}

pub fn convertir_media(entree: &Path, o: &Options, stop: &AtomicBool) -> Result<PathBuf, String> {
    let dest = sortie(entree, o.dossier.as_deref(), &o.format_media);
    let mut enfant = crate::dependances::commande("ffmpeg")?.args(["-y", "-v", "error", "-i"]).arg(entree).args(args_media(o)).arg(&dest)
        .stderr(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).spawn()
        .map_err(|e| e.to_string())?;
    loop {
        if let Some(st) = enfant.try_wait().map_err(|e| e.to_string())? {
            if st.success() {
                return Ok(dest);
            }
            let mut err = String::new();
            if let Some(mut e) = enfant.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            let _ = std::fs::remove_file(&dest);
            return Err(err.lines().last().unwrap_or("échec de ffmpeg").to_string());
        }
        if stop.load(Relaxed) {
            let _ = enfant.kill();
            let _ = enfant.wait();
            let _ = std::fs::remove_file(&dest);
            return Err("annulé".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[derive(Clone, PartialEq)]
enum Etat {
    Attente,
    EnCours,
    Fait(PathBuf),
    Erreur(String),
}

pub struct Convertisseur {
    fichiers: Arc<Mutex<Vec<(PathBuf, Etat)>>>,
    o: Options,
    stop: Arc<AtomicBool>,
    actif: Arc<AtomicBool>,
}

impl Convertisseur {
    pub fn new() -> Self {
        Convertisseur { fichiers: Arc::default(), o: Options::default(), stop: Arc::default(), actif: Arc::default() }
    }

    fn ajouter(&self, chemins: impl IntoIterator<Item = PathBuf>) {
        let mut f = self.fichiers.lock().unwrap();
        for p in chemins {
            if genre(&p) != Genre::Autre && !f.iter().any(|(x, _)| x == &p) {
                f.push((p, Etat::Attente));
            }
        }
    }

    fn lancer(&self) {
        if self.actif.swap(true, Relaxed) {
            return;
        }
        self.stop.store(false, Relaxed);
        let (fichiers, o, stop, actif) = (self.fichiers.clone(), self.o.clone(), self.stop.clone(), self.actif.clone());
        std::thread::spawn(move || {
            loop {
                let suivant = {
                    let mut f = fichiers.lock().unwrap();
                    f.iter_mut().find(|(_, e)| *e == Etat::Attente).map(|(p, e)| {
                        *e = Etat::EnCours;
                        p.clone()
                    })
                };
                let Some(p) = suivant else { break };
                if stop.load(Relaxed) {
                    if let Some(x) = fichiers.lock().unwrap().iter_mut().find(|(x, _)| x == &p) {
                        x.1 = Etat::Attente;
                    }
                    break;
                }
                let r = match genre(&p) {
                    Genre::Image => convertir_image(&p, &o),
                    _ => convertir_media(&p, &o, &stop),
                };
                if let Some(x) = fichiers.lock().unwrap().iter_mut().find(|(x, _)| x == &p) {
                    x.1 = match r {
                        Ok(d) => Etat::Fait(d),
                        Err(e) => Etat::Erreur(e),
                    };
                }
            }
            actif.store(false, Relaxed);
            let n = fichiers.lock().unwrap().iter().filter(|(_, e)| matches!(e, Etat::Fait(_))).count();
            crate::systeme::notifier("Convertisseur", &format!("{n} fichier(s) converti(s)."));
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Convertisseur");
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        self.ajouter(deposes);
        let actif = self.actif.load(Relaxed);
        if actif {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter des fichiers…").clicked() {
                let mut tous: Vec<&str> = IMAGES.to_vec();
                tous.extend(MEDIAS);
                if let Some(v) = rfd::FileDialog::new().add_filter("Images, vidéos, sons", &tous).pick_files() {
                    self.ajouter(v);
                }
            }
            if ui.button("Vider la liste").clicked() && !actif {
                self.fichiers.lock().unwrap().clear();
            }
            ui.weak("ou glisse-les sur la fenêtre");
        });
        ui.separator();
        egui::Grid::new("conv_opts").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.strong("Images vers");
            ui.horizontal(|ui| {
                for f in SORTIES_IMAGE {
                    ui.selectable_value(&mut self.o.format_image, f.to_string(), f.to_uppercase());
                }
                if self.o.format_image == "jpg" {
                    ui.add(egui::Slider::new(&mut self.o.qualite, 40..=100).text("qualité"));
                }
                if self.o.format_image == "webp" {
                    ui.weak("(sans perte)");
                }
            });
            ui.end_row();
            ui.strong("Vidéos / sons vers");
            ui.horizontal(|ui| {
                for f in SORTIES_MEDIA {
                    ui.selectable_value(&mut self.o.format_media, f.to_string(), f.to_uppercase());
                }
            });
            ui.end_row();
            ui.label("");
            ui.horizontal(|ui| match self.o.format_media.as_str() {
                "mp4" | "mkv" | "webm" => {
                    ui.add(egui::Slider::new(&mut self.o.crf, 16..=35).text("compression (CRF : bas = meilleure qualité)"));
                }
                "gif" => {
                    ui.add(egui::Slider::new(&mut self.o.gif_fps, 5..=30).text("images/s"));
                }
                "flac" | "wav" => {
                    ui.weak("sans perte");
                }
                _ => {
                    ui.add(egui::Slider::new(&mut self.o.audio_kbps, 64..=320).text("kbit/s"));
                }
            });
            ui.end_row();
            ui.strong("Taille max");
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.o.largeur_max).range(0..=16000).suffix(" px de large"));
                ui.add(egui::DragValue::new(&mut self.o.hauteur_max).range(0..=16000).suffix(" px de haut"));
                for (n, h) in [("720p", 720), ("1080p", 1080), ("Original", 0)] {
                    if ui.small_button(n).clicked() {
                        self.o.hauteur_max = h;
                        self.o.largeur_max = 0;
                    }
                }
                ui.weak("0 = pas de limite (vidéos : hauteur seulement)");
            });
            ui.end_row();
            ui.strong("Enregistrer dans");
            ui.horizontal(|ui| {
                ui.label(self.o.dossier.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "le dossier d'origine".into()));
                if ui.button("Choisir…").clicked() {
                    self.o.dossier = rfd::FileDialog::new().pick_folder();
                }
                if self.o.dossier.is_some() && ui.small_button("✖").clicked() {
                    self.o.dossier = None;
                }
            });
            ui.end_row();
        });
        ui.separator();
        let (n_attente, n_total) = {
            let f = self.fichiers.lock().unwrap();
            (f.iter().filter(|(_, e)| *e == Etat::Attente).count(), f.len())
        };
        ui.horizontal(|ui| {
            if actif {
                ui.spinner();
                if ui.button("⏹ Arrêter").clicked() {
                    self.stop.store(true, Relaxed);
                }
            } else if ui.add_enabled(n_attente > 0, egui::Button::new(format!("▶ Convertir ({n_attente})"))).clicked() {
                self.lancer();
            }
            if !actif && n_total > n_attente && ui.button("Reconvertir tout").clicked() {
                for (_, e) in self.fichiers.lock().unwrap().iter_mut() {
                    *e = Etat::Attente;
                }
            }
        });
        let mut retirer = None;
        egui::ScrollArea::vertical().id_salt("conv_liste").show(ui, |ui| {
            for (i, (p, e)) in self.fichiers.lock().unwrap().iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(if genre(p) == Genre::Image { "🖼" } else { "🎞" });
                    ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    match e {
                        Etat::Attente => {
                            ui.weak("en attente");
                            if !actif && ui.small_button("✖").clicked() {
                                retirer = Some(i);
                            }
                        }
                        Etat::EnCours => {
                            ui.spinner();
                        }
                        Etat::Fait(d) => {
                            ui.colored_label(egui::Color32::LIGHT_GREEN, "✔");
                            if ui.link(d.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                                crate::disque::montrer_dans_explorateur(d);
                            }
                            let avant = p.metadata().map(|m| m.len()).unwrap_or(0);
                            let apres = d.metadata().map(|m| m.len()).unwrap_or(0);
                            ui.weak(format!("{} → {}", crate::disque::taille_lisible(avant), crate::disque::taille_lisible(apres)));
                        }
                        Etat::Erreur(m) => {
                            ui.colored_label(egui::Color32::LIGHT_RED, m);
                        }
                    }
                });
            }
        });
        if let Some(i) = retirer {
            self.fichiers.lock().unwrap().remove(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genres() {
        assert_eq!(genre(Path::new("a.JPG")), Genre::Image);
        assert_eq!(genre(Path::new("a.mkv")), Genre::Media);
        assert_eq!(genre(Path::new("a.txt")), Genre::Autre);
    }

    #[test]
    fn tailles() {
        assert_eq!(dimensions(4000, 2000, 1000, 0), (1000, 500));
        assert_eq!(dimensions(4000, 2000, 1000, 250), (500, 250));
        assert_eq!(dimensions(800, 600, 1920, 1080), (800, 600)); // jamais agrandir
    }

    #[test]
    fn ffmpeg_args() {
        let mut o = Options { format_media: "mp3".into(), ..Default::default() };
        assert_eq!(args_media(&o).join(" "), "-vn -c:a libmp3lame -b:a 192k");
        o.format_media = "mp4".into();
        o.hauteur_max = 720;
        let a = args_media(&o).join(" ");
        assert!(a.contains("-crf 23") && a.contains("scale=-2:'min(720,ih)'"));
    }

    #[test]
    fn image_et_sortie() {
        let d = std::env::temp_dir().join(format!("outils_conv_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let src = d.join("photo.png");
        image::RgbaImage::from_pixel(400, 200, image::Rgba([200, 30, 120, 255])).save(&src).unwrap();
        let o = Options { format_image: "jpg".into(), largeur_max: 100, ..Default::default() };
        let r = convertir_image(&src, &o).unwrap();
        assert_eq!(r, d.join("photo.jpg"));
        assert_eq!(image::open(&r).unwrap().width(), 100);
        assert_eq!(convertir_image(&src, &o).unwrap(), d.join("photo (2).jpg"));
        let o = Options { format_image: "png".into(), ..Default::default() };
        assert_eq!(convertir_image(&src, &o).unwrap(), d.join("photo (2).png"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
