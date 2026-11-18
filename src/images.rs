//! Images : doublons visuels (79), visionneuse (80), renommage par date de prise de vue (83),
//! suppression des métadonnées EXIF / GPS (84).
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EXT: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tif", "tiff"];

fn est_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn images_de(d: &Path, recursif: bool) -> Vec<PathBuf> {
    let mut v = vec![];
    let mut pile = vec![d.to_path_buf()];
    while let Some(x) = pile.pop() {
        for e in std::fs::read_dir(&x).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                if recursif {
                    pile.push(p);
                }
            } else if est_image(&p) {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

// ---------------------------------------------------------------------------------------------------
// 79 · Doublons visuels (empreinte « dHash » : même image même redimensionnée ou recompressée)
// ---------------------------------------------------------------------------------------------------

pub fn dhash(img: &image::DynamicImage) -> u64 {
    let p = img.resize_exact(9, 8, image::imageops::FilterType::Triangle).to_luma8();
    let mut h = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            h = (h << 1) | (p.get_pixel(x, y)[0] < p.get_pixel(x + 1, y)[0]) as u64;
        }
    }
    h
}

/// Groupes d'indices dont les empreintes diffèrent d'au plus `seuil` bits.
pub fn grouper(empreintes: &[u64], seuil: u32) -> Vec<Vec<usize>> {
    let n = empreintes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn racine(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            if (empreintes[i] ^ empreintes[j]).count_ones() <= seuil {
                let (a, b) = (racine(&mut parent, i), racine(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut g: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = racine(&mut parent, i);
        g.entry(r).or_default().push(i);
    }
    g.into_values().filter(|v| v.len() > 1).collect()
}

#[derive(Default)]
struct EtatDoublons {
    groupes: Vec<Vec<(PathBuf, u64, (u32, u32))>>, // chemin, taille, dimensions
    progression: (usize, usize),
    occupe: bool,
}

pub struct DoublonsImages {
    dossier: Option<PathBuf>,
    seuil: u32,
    e: Arc<Mutex<EtatDoublons>>,
}

impl DoublonsImages {
    pub fn new() -> Self {
        DoublonsImages { dossier: None, seuil: 4, e: Arc::default() }
    }
}

impl Outil for DoublonsImages {
    fn nom(&self) -> &'static str {
        "🖼 Images en double"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Images en double ou presque");
        ui.weak("Retrouve les mêmes photos même redimensionnées, recompressées ou converties.");
        let (occupe, prog) = {
            let x = self.e.lock().unwrap();
            (x.occupe, x.progression)
        };
        ui.horizontal(|ui| {
            if ui.button("📁 Dossier…").clicked() {
                self.dossier = rfd::FileDialog::new().pick_folder();
            }
            if let Some(d) = &self.dossier {
                ui.monospace(d.display().to_string());
            }
            ui.add(egui::Slider::new(&mut self.seuil, 0..=12).text("tolérance"));
            if ui.add_enabled(self.dossier.is_some() && !occupe, egui::Button::new("🔍 Chercher")).clicked() {
                let (d, seuil, e) = (self.dossier.clone().unwrap(), self.seuil, self.e.clone());
                e.lock().unwrap().occupe = true;
                std::thread::spawn(move || {
                    let fichiers = images_de(&d, true);
                    let mut emp = vec![];
                    let mut infos = vec![];
                    for (i, f) in fichiers.iter().enumerate() {
                        e.lock().unwrap().progression = (i + 1, fichiers.len());
                        if let Ok(img) = image::open(f) {
                            emp.push(dhash(&img));
                            infos.push((f.clone(), f.metadata().map(|m| m.len()).unwrap_or(0), (img.width(), img.height())));
                        }
                    }
                    let groupes = grouper(&emp, seuil).into_iter().map(|g| g.into_iter().map(|i| infos[i].clone()).collect()).collect();
                    let mut x = e.lock().unwrap();
                    x.groupes = groupes;
                    x.occupe = false;
                });
            }
        });
        if occupe {
            ui.add(egui::ProgressBar::new(prog.0 as f32 / prog.1.max(1) as f32).text(format!("{} / {}", prog.0, prog.1)));
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        let mut supprime = None;
        egui::ScrollArea::vertical().id_salt("dbl_img").show(ui, |ui| {
            let x = self.e.lock().unwrap();
            if !occupe && x.groupes.is_empty() && x.progression.1 > 0 {
                ui.label("Aucun doublon trouvé.");
            }
            for (gi, g) in x.groupes.iter().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (k, (p, t, (w, h))) in g.iter().enumerate() {
                            ui.vertical(|ui| {
                                ui.add(egui::Image::new(format!("file://{}", p.display())).fit_to_exact_size(egui::vec2(150.0, 110.0)));
                                ui.weak(format!("{w}×{h} · {}", crate::disque::taille_lisible(*t)));
                                ui.label(p.file_name().unwrap_or_default().to_string_lossy()).on_hover_text(p.display().to_string());
                                ui.horizontal(|ui| {
                                    if ui.small_button("📂").clicked() {
                                        crate::disque::montrer_dans_explorateur(p);
                                    }
                                    if ui.small_button("🗑").on_hover_text("Corbeille").clicked() {
                                        supprime = Some((gi, k, p.clone()));
                                    }
                                });
                            });
                        }
                    });
                });
            }
        });
        if let Some((gi, k, p)) = supprime {
            if crate::disque::corbeille(&p) {
                let mut x = self.e.lock().unwrap();
                x.groupes[gi].remove(k);
                if x.groupes[gi].len() < 2 {
                    x.groupes.remove(gi);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 80 · Visionneuse
// ---------------------------------------------------------------------------------------------------

pub struct Visionneuse {
    liste: Vec<PathBuf>,
    i: usize,
    texture: Option<(PathBuf, egui::TextureHandle, (u32, u32))>,
    zoom_reel: bool,
    rotation: u8,
    diaporama: Option<Instant>,
}

impl Visionneuse {
    pub fn new() -> Self {
        Visionneuse { liste: vec![], i: 0, texture: None, zoom_reel: false, rotation: 0, diaporama: None }
    }

    fn ouvrir(&mut self, p: PathBuf) {
        let d = if p.is_dir() { p.clone() } else { p.parent().map(Path::to_path_buf).unwrap_or_default() };
        self.liste = images_de(&d, false);
        self.i = self.liste.iter().position(|x| *x == p).unwrap_or(0);
        self.rotation = 0;
    }
}

impl Outil for Visionneuse {
    fn nom(&self) -> &'static str {
        "🖼 Visionneuse"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.ouvrir(p);
        }
        let n = self.liste.len();
        ui.horizontal(|ui| {
            if ui.button("📂 Ouvrir…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", EXT).pick_file() {
                    self.ouvrir(p);
                }
            }
            if n > 0 {
                let avant = ui.button("◀").clicked() || ui.input(|i| i.key_pressed(egui::Key::ArrowLeft));
                let apres = ui.button("▶").clicked() || ui.input(|i| i.key_pressed(egui::Key::ArrowRight));
                if avant {
                    self.i = (self.i + n - 1) % n;
                    self.rotation = 0;
                }
                if apres {
                    self.i = (self.i + 1) % n;
                    self.rotation = 0;
                }
                ui.label(format!("{} / {n}", self.i + 1));
                ui.toggle_value(&mut self.zoom_reel, "100 %");
                if ui.button("⟳").clicked() {
                    self.rotation = (self.rotation + 1) % 4;
                }
                let mut diapo = self.diaporama.is_some();
                if ui.toggle_value(&mut diapo, "▶ Diaporama").changed() {
                    self.diaporama = diapo.then(Instant::now);
                }
                if ui.button("🗑").on_hover_text("Corbeille").clicked() || ui.input(|i| i.key_pressed(egui::Key::Delete)) {
                    let p = self.liste[self.i].clone();
                    if crate::disque::corbeille(&p) {
                        self.liste.remove(self.i);
                        self.i = self.i.min(self.liste.len().saturating_sub(1));
                    }
                }
            }
        });
        if let Some(t) = self.diaporama {
            if t.elapsed() > Duration::from_secs(4) && n > 0 {
                self.i = (self.i + 1) % n;
                self.diaporama = Some(Instant::now());
            }
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        let Some(p) = self.liste.get(self.i).cloned() else {
            ui.weak("Ouvre une image ou glisse-la ici (flèches ← → pour défiler, Suppr pour la corbeille).");
            return;
        };
        if self.texture.as_ref().is_none_or(|t| t.0 != p) {
            self.texture = image::open(&p).ok().map(|img| {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
                (p.clone(), ui.ctx().load_texture("vision", ci, egui::TextureOptions::LINEAR), (w, h))
            });
        }
        let Some((_, t, (w, h))) = &self.texture else {
            ui.colored_label(Color32::LIGHT_RED, "Image illisible.");
            return;
        };
        ui.weak(format!("{} · {w}×{h} · {}", p.file_name().unwrap_or_default().to_string_lossy(), crate::disque::taille_lisible(p.metadata().map(|m| m.len()).unwrap_or(0))));
        let (iw, ih) = if self.rotation % 2 == 1 { (*h as f32, *w as f32) } else { (*w as f32, *h as f32) };
        let dispo = ui.available_size();
        let echelle = if self.zoom_reel { 1.0 } else { (dispo.x / iw).min(dispo.y / ih).min(1.0) };
        egui::ScrollArea::both().id_salt("vision").show(ui, |ui| {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(iw, ih) * echelle).rotate(self.rotation as f32 * std::f32::consts::FRAC_PI_2, egui::vec2(0.5, 0.5)));
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 83 / 84 · Date de prise de vue, métadonnées
// ---------------------------------------------------------------------------------------------------

/// Date de prise de vue « AAAA-MM-JJ HH.MM.SS » lue dans l'EXIF.
pub fn date_exif(p: &Path) -> Option<String> {
    let f = std::fs::File::open(p).ok()?;
    let e = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)).ok()?;
    let c = e.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).or_else(|| e.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let s = c.display_value().to_string(); // 2024-05-03 14:22:10
    let s = s.trim_matches('"').replace(':', ".");
    let (d, h) = s.split_once(' ')?;
    Some(format!("{} {h}", d.replace('.', "-")))
}

/// JPEG sans les segments APP1 (EXIF, XMP) ni APP13 (IPTC) : l'image elle-même n'est pas recompressée.
pub fn jpeg_sans_meta(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let m = b[i + 1];
        if m == 0xDA {
            out.extend(&b[i..]); // début des données d'image : on recopie tout
            return Some(out);
        }
        let l = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if i + 2 + l > b.len() {
            return None;
        }
        if m != 0xE1 && m != 0xED {
            out.extend(&b[i..i + 2 + l]);
        }
        i += 2 + l;
    }
    None
}

/// PNG sans les blocs eXIf / tEXt / iTXt / zTXt.
pub fn png_sans_meta(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 8 || &b[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut out = b[..8].to_vec();
    let mut i = 8;
    while i + 12 <= b.len() {
        let l = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
        let t = &b[i + 4..i + 8];
        let fin = i + 12 + l;
        if fin > b.len() {
            return None;
        }
        if !matches!(t, b"eXIf" | b"tEXt" | b"iTXt" | b"zTXt") {
            out.extend(&b[i..fin]);
        }
        i = fin;
    }
    Some(out)
}

pub struct PhotosOutils {
    fichiers: Vec<PathBuf>,
    apercu: Vec<(PathBuf, Option<String>)>,
    message: String,
}

impl PhotosOutils {
    pub fn new() -> Self {
        PhotosOutils { fichiers: vec![], apercu: vec![], message: String::new() }
    }
}

impl Outil for PhotosOutils {
    fn nom(&self) -> &'static str {
        "📷 Photos : date et EXIF"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Photos : renommer par date, retirer les métadonnées");
        let mut nouveaux: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        ui.horizontal(|ui| {
            if ui.button("➕ Photos…").clicked() {
                nouveaux.extend(rfd::FileDialog::new().add_filter("Images", EXT).pick_files().unwrap_or_default());
            }
            if ui.button("📁 Dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    nouveaux.extend(images_de(&d, false));
                }
            }
            if ui.button("Vider").clicked() {
                self.fichiers.clear();
                self.apercu.clear();
            }
        });
        if !nouveaux.is_empty() {
            for p in nouveaux.into_iter().filter(|p| est_image(p)) {
                if !self.fichiers.contains(&p) {
                    let d = date_exif(&p);
                    self.apercu.push((p.clone(), d));
                    self.fichiers.push(p);
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.fichiers.is_empty(), egui::Button::new("📅 Renommer par date de prise de vue")).clicked() {
                let mut n = 0;
                for (p, d) in self.apercu.iter_mut() {
                    let Some(d) = d else { continue };
                    let ext = p.extension().unwrap_or_default().to_string_lossy().to_lowercase();
                    let dossier = p.parent().unwrap_or(Path::new(".")).to_path_buf();
                    let mut cible = dossier.join(format!("{d}.{ext}"));
                    let mut k = 2;
                    while cible.exists() && cible != *p {
                        cible = dossier.join(format!("{d} ({k}).{ext}"));
                        k += 1;
                    }
                    if cible != *p && std::fs::rename(&*p, &cible).is_ok() {
                        *p = cible;
                        n += 1;
                    }
                }
                self.fichiers = self.apercu.iter().map(|x| x.0.clone()).collect();
                self.message = format!("{n} photo(s) renommée(s).");
            }
            if ui.add_enabled(!self.fichiers.is_empty(), egui::Button::new("🧹 Retirer EXIF et position GPS")).clicked() {
                let (mut ok, mut ko) = (0, 0);
                for p in &self.fichiers {
                    let b = std::fs::read(p).unwrap_or_default();
                    let e = p.extension().unwrap_or_default().to_string_lossy().to_lowercase();
                    let r = match e.as_str() {
                        "jpg" | "jpeg" => jpeg_sans_meta(&b),
                        "png" => png_sans_meta(&b),
                        _ => None,
                    };
                    match r {
                        Some(n) if std::fs::write(p, &n).is_ok() => ok += 1,
                        _ => ko += 1,
                    }
                }
                self.message = format!("{ok} photo(s) nettoyée(s){}", if ko > 0 { format!(", {ko} ignorée(s) (seuls JPEG et PNG sont traités)") } else { String::new() });
                for x in self.apercu.iter_mut() {
                    x.1 = date_exif(&x.0);
                }
            }
        });
        ui.weak("Le nettoyage ne recompresse pas l'image (aucune perte de qualité) mais retire aussi l'orientation : une photo de téléphone peut apparaître couchée.");
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).strong());
        }
        egui::ScrollArea::vertical().id_salt("photos").show(ui, |ui| {
            for (p, d) in &self.apercu {
                ui.horizontal(|ui| {
                    ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    match d {
                        Some(d) => ui.weak(format!("prise le {d}")),
                        None => ui.weak("pas de date EXIF"),
                    };
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empreintes() {
        let mut a = image::RgbImage::new(64, 48);
        for (x, _, p) in a.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 4) as u8, 50, 100]);
        }
        let img = image::DynamicImage::ImageRgb8(a.clone());
        let petite = img.resize(32, 24, image::imageops::FilterType::Lanczos3);
        assert!((dhash(&img) ^ dhash(&petite)).count_ones() <= 4);
        let g = grouper(&[0b1111, 0b1110, u64::MAX, 0], 1);
        assert_eq!(g, vec![vec![0, 1]]);
    }

    #[test]
    fn metadonnees() {
        // SOI, APP1 (EXIF), DQT, SOS + données
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x04, 0xAA, 0xBB, 0xFF, 0xDB, 0x00, 0x03, 0x01, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22];
        assert_eq!(jpeg_sans_meta(&jpeg).unwrap(), vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x03, 0x01, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22]);
        assert!(jpeg_sans_meta(b"pas un jpeg").is_none());
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        for (t, d) in [(b"IHDR", &b"x"[..]), (b"tEXt", &b"gps"[..]), (b"IEND", &b""[..])] {
            png.extend((d.len() as u32).to_be_bytes());
            png.extend(t);
            png.extend(d);
            png.extend([0, 0, 0, 0]);
        }
        let n = png_sans_meta(&png).unwrap();
        assert!(!n.windows(4).any(|w| w == b"tEXt") && n.windows(4).any(|w| w == b"IEND"));
    }
}
