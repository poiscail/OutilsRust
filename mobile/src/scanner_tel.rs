//! Scanner de documents : photo d'une page avec l'appareil photo, coins trouvés tout seuls (et
//! ajustables au doigt), page redressée, rendu « document » (noir et blanc net), PDF de plusieurs pages.
use crate::commun::*;
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, Pos2, RichText, Vec2};
use image::{GrayImage, RgbImage};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Orientation EXIF d'un JPEG (1 = normale, 6 = tournée de 90°, 3 = 180°, 8 = 270°).
pub fn orientation_exif(d: &[u8]) -> u16 {
    let mut i = 2;
    while i + 4 < d.len() && d[0] == 0xFF && d[1] == 0xD8 {
        if d[i] != 0xFF {
            return 1;
        }
        let m = d[i + 1];
        let n = u16::from_be_bytes([d[i + 2], d[i + 3]]) as usize;
        if m == 0xE1 && i + 10 < d.len() && &d[i + 4..i + 10] == b"Exif\0\0" {
            let t = &d[i + 10..(i + 2 + n).min(d.len())];
            if t.len() < 8 {
                return 1;
            }
            let le = &t[..2] == b"II";
            let u16_ = |o: usize| if o + 2 > t.len() { 0 } else if le { u16::from_le_bytes([t[o], t[o + 1]]) } else { u16::from_be_bytes([t[o], t[o + 1]]) };
            let u32_ = |o: usize| if o + 4 > t.len() { 0 } else if le { u32::from_le_bytes([t[o], t[o + 1], t[o + 2], t[o + 3]]) } else { u32::from_be_bytes([t[o], t[o + 1], t[o + 2], t[o + 3]]) } as usize;
            let ifd = u32_(4);
            let nb = u16_(ifd) as usize;
            for k in 0..nb {
                let e = ifd + 2 + k * 12;
                if u16_(e) == 0x0112 {
                    return u16_(e + 8);
                }
            }
            return 1;
        }
        if m == 0xDA {
            break;
        }
        i += 2 + n;
    }
    1
}

/// Coins probables d'une feuille claire sur un fond plus sombre : (haut-gauche, haut-droite, bas-droite, bas-gauche), en fraction.
pub fn trouver_coins(g: &GrayImage) -> [Pos2; 4] {
    let (l, h) = g.dimensions();
    let moyenne = g.pixels().map(|p| p[0] as u64).sum::<u64>() / (l as u64 * h as u64).max(1);
    let seuil = ((moyenne as u32 + 255) / 2).min(235) as u8;
    let mut c = [(f32::MAX, Pos2::ZERO), (f32::MIN, Pos2::ZERO), (f32::MIN, Pos2::ZERO), (f32::MAX, Pos2::ZERO)];
    let mut n = 0;
    for y in (0..h).step_by(2) {
        for x in (0..l).step_by(2) {
            if g.get_pixel(x, y)[0] < seuil {
                continue;
            }
            n += 1;
            let (fx, fy) = (x as f32 / l as f32, y as f32 / h as f32);
            let p = Pos2::new(fx, fy);
            if fx + fy < c[0].0 {
                c[0] = (fx + fy, p);
            }
            if fx - fy > c[1].0 {
                c[1] = (fx - fy, p);
            }
            if fx + fy > c[2].0 {
                c[2] = (fx + fy, p);
            }
            if fx - fy < c[3].0 {
                c[3] = (fx - fy, p);
            }
        }
    }
    if n < (l * h / 40) as usize {
        return [Pos2::new(0.05, 0.05), Pos2::new(0.95, 0.05), Pos2::new(0.95, 0.95), Pos2::new(0.05, 0.95)];
    }
    [c[0].1, c[1].1, c[2].1, c[3].1]
}

/// Homographie qui envoie le rectangle (0,0)-(1,1) sur le quadrilatère `q` (coefficients a..h).
pub fn homographie(q: &[Pos2; 4]) -> Option<[f32; 8]> {
    let src = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let mut m = [[0f64; 9]; 8];
    for k in 0..4 {
        let (x, y) = src[k];
        let (u, v) = (q[k].x as f64, q[k].y as f64);
        m[2 * k] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * k + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    for c in 0..8 {
        let p = (c..8).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs()))?;
        if m[p][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, p);
        for r in 0..8 {
            if r != c {
                let f = m[r][c] / m[c][c];
                for k in c..9 {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    let mut h = [0f32; 8];
    for i in 0..8 {
        h[i] = (m[i][8] / m[i][i]) as f32;
    }
    Some(h)
}

/// Page redressée (format A4, `largeur` pixels de large).
pub fn redresser(img: &RgbImage, coins: &[Pos2; 4], largeur: u32) -> Option<RgbImage> {
    let (l, h) = img.dimensions();
    let q = coins.map(|c| Pos2::new(c.x * l as f32, c.y * h as f32));
    let k = homographie(&q)?;
    let hauteur = (largeur as f32 * 1.414) as u32;
    let mut o = RgbImage::new(largeur, hauteur);
    for y in 0..hauteur {
        for x in 0..largeur {
            let (u, v) = (x as f32 / largeur as f32, y as f32 / hauteur as f32);
            let w = k[6] * u + k[7] * v + 1.0;
            let sx = (k[0] * u + k[1] * v + k[2]) / w;
            let sy = (k[3] * u + k[4] * v + k[5]) / w;
            let (ix, iy) = (sx.clamp(0.0, l as f32 - 2.0), sy.clamp(0.0, h as f32 - 2.0));
            let (x0, y0) = (ix as u32, iy as u32);
            let (fx, fy) = (ix - x0 as f32, iy - y0 as f32);
            let p = |a: u32, b: u32| img.get_pixel(a, b).0;
            let (a, b, c, d) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
            let mut px = [0u8; 3];
            for i in 0..3 {
                let v = a[i] as f32 * (1.0 - fx) * (1.0 - fy) + b[i] as f32 * fx * (1.0 - fy) + c[i] as f32 * (1.0 - fx) * fy + d[i] as f32 * fx * fy;
                px[i] = v as u8;
            }
            o.put_pixel(x, y, image::Rgb(px));
        }
    }
    Some(o)
}

/// Rendu « document » : fond blanc, texte noir, même avec une ombre (seuil local par moyenne).
pub fn noir_et_blanc(img: &RgbImage) -> GrayImage {
    let g = image::DynamicImage::ImageRgb8(img.clone()).to_luma8();
    let (l, h) = g.dimensions();
    let mut s = vec![0u64; ((l + 1) * (h + 1)) as usize];
    for y in 0..h {
        let mut ligne = 0u64;
        for x in 0..l {
            ligne += g.get_pixel(x, y)[0] as u64;
            s[((y + 1) * (l + 1) + x + 1) as usize] = s[(y * (l + 1) + x + 1) as usize] + ligne;
        }
    }
    let r = (l / 24).max(4);
    GrayImage::from_fn(l, h, |x, y| {
        let (x0, y0, x1, y1) = (x.saturating_sub(r), y.saturating_sub(r), (x + r).min(l - 1) + 1, (y + r).min(h - 1) + 1);
        let n = ((x1 - x0) * (y1 - y0)) as u64;
        let somme = s[(y1 * (l + 1) + x1) as usize] + s[(y0 * (l + 1) + x0) as usize] - s[(y0 * (l + 1) + x1) as usize] - s[(y1 * (l + 1) + x0) as usize];
        let moy = somme / n.max(1);
        let v = g.get_pixel(x, y)[0] as u64;
        image::Luma([if v * 100 < moy * 88 { 0 } else { 255 }])
    })
}

/// PDF minimal : une image JPEG par page (A4).
pub fn faire_pdf(pages: &[(u32, u32, Vec<u8>, bool)]) -> Vec<u8> {
    let mut o: Vec<u8> = b"%PDF-1.4\n".to_vec();
    let mut pos: Vec<usize> = vec![];
    let n = pages.len();
    let mut obj = |o: &mut Vec<u8>, pos: &mut Vec<usize>, contenu: &[u8]| {
        pos.push(o.len());
        o.extend_from_slice(format!("{} 0 obj\n", pos.len()).as_bytes());
        o.extend_from_slice(contenu);
        o.extend_from_slice(b"\nendobj\n");
    };
    let kids: Vec<String> = (0..n).map(|k| format!("{} 0 R", 3 + k * 3)).collect();
    obj(&mut o, &mut pos, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(&mut o, &mut pos, format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).as_bytes());
    for (k, (l, h, jpeg, gris)) in pages.iter().enumerate() {
        let (page, image, contenu) = (3 + k * 3, 4 + k * 3, 5 + k * 3);
        obj(&mut o, &mut pos, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /XObject << /Im{k} {image} 0 R >> >> /Contents {contenu} 0 R >>").as_bytes());
        let mut im = format!("<< /Type /XObject /Subtype /Image /Width {l} /Height {h} /ColorSpace /{} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", if *gris { "DeviceGray" } else { "DeviceRGB" }, jpeg.len()).into_bytes();
        im.extend_from_slice(jpeg);
        im.extend_from_slice(b"\nendstream");
        obj(&mut o, &mut pos, &im);
        let flux = format!("q 595 0 0 842 0 0 cm /Im{k} Do Q");
        obj(&mut o, &mut pos, format!("<< /Length {} >>\nstream\n{flux}\nendstream", flux.len()).as_bytes());
        let _ = page;
    }
    let xref = o.len();
    o.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", pos.len() + 1).as_bytes());
    for p in &pos {
        o.extend_from_slice(format!("{p:010} 00000 n \n").as_bytes());
    }
    o.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", pos.len() + 1).as_bytes());
    o
}

#[derive(Clone, Copy, PartialEq)]
enum Rendu {
    Document,
    Gris,
    Couleur,
}

pub struct Scanner {
    attente: Option<(String, Instant)>,
    photo: Option<RgbImage>,
    coins: [Pos2; 4],
    tex: Option<egui::TextureHandle>,
    tenu: Option<usize>,
    rendu: Rendu,
    pages: Vec<(RgbImage, Rendu)>,
    apercus: Vec<egui::TextureHandle>,
    message: String,
}

impl Scanner {
    pub fn new() -> Self {
        Scanner { attente: None, photo: None, coins: [Pos2::ZERO; 4], tex: None, tenu: None, rendu: Rendu::Document, pages: vec![], apercus: vec![], message: String::new() }
    }

    fn charger_photo(&mut self, p: &PathBuf) -> bool {
        let Ok(b) = std::fs::read(p) else { return false };
        let Ok(i) = image::load_from_memory(&b) else { return false };
        let i = match orientation_exif(&b) {
            6 => i.rotate90(),
            3 => i.rotate180(),
            8 => i.rotate270(),
            _ => i,
        };
        let i = i.thumbnail(1800, 1800).to_rgb8();
        let petit = image::DynamicImage::ImageRgb8(i.clone()).thumbnail(300, 300).to_luma8();
        self.coins = trouver_coins(&petit);
        self.photo = Some(i);
        self.tex = None;
        let _ = std::fs::remove_file(p);
        true
    }
}

impl Outil for Scanner {
    fn nom(&self) -> &'static str {
        "📄 Scanner de documents"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        // la photo prise par l'appareil photo d'Android arrive dans le dossier de partage
        if let Some((nom, depuis)) = self.attente.clone() {
            let p = crate::android::dossier_partage().join(&nom);
            let pret = p.metadata().map(|m| m.len() > 10_000).unwrap_or(false) && depuis.elapsed() > Duration::from_secs(2);
            if pret && self.charger_photo(&p) {
                self.attente = None;
            } else if depuis.elapsed() > Duration::from_secs(300) {
                self.attente = None;
            }
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        if self.photo.is_none() {
            ui.label(RichText::new(format!("📄 {} page(s)", self.pages.len())).size(18.0).strong());
            if gros_bouton(ui, "📸 Photographier une page") {
                let nom = format!("scan_{}.jpg", crate::commun::maintenant());
                if crate::android::prendre_photo(&nom) {
                    self.attente = Some((nom, Instant::now()));
                    self.message.clear();
                } else {
                    self.message = "Impossible d'ouvrir l'appareil photo.".into();
                }
            }
            if self.attente.is_some() {
                ui.weak("Prends la photo, valide-la dans l'appareil photo, puis reviens ici.");
            }
            ui.weak("Pose la feuille sur un fond sombre, bien éclairée : les coins sont trouvés tout seuls.");
            if !self.pages.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for t in &self.apercus {
                        ui.add(egui::Image::new(t).max_height(110.0));
                    }
                });
                if gros_bouton(ui, &format!("📕 Créer le PDF ({} page(s))", self.pages.len())) {
                    let pages: Vec<(u32, u32, Vec<u8>, bool)> = self
                        .pages
                        .iter()
                        .map(|(i, r)| {
                            let mut b = vec![];
                            let gris = *r != Rendu::Couleur;
                            let res = if gris {
                                let g = if *r == Rendu::Document { noir_et_blanc(i) } else { image::DynamicImage::ImageRgb8(i.clone()).to_luma8() };
                                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 80).encode_image(&g)
                            } else {
                                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 82).encode_image(i)
                            };
                            let _ = res;
                            (i.width(), i.height(), b, gris)
                        })
                        .collect();
                    let d = dossier_outils();
                    let f = libre(&d, &format!("Scan {}.pdf", crate::minecraft::horodatage().replace('_', " ")));
                    self.message = match std::fs::write(&f, faire_pdf(&pages)) {
                        Ok(()) => {
                            crate::android::signaler_media(&[f.display().to_string()]);
                            self.pages.clear();
                            self.apercus.clear();
                            format!("✔ PDF enregistré : Téléchargements › Outils › {}", f.file_name().unwrap_or_default().to_string_lossy())
                        }
                        Err(e) => format!("✖ {e}"),
                    };
                }
                if ui.button("🗑 Recommencer").clicked() {
                    self.pages.clear();
                    self.apercus.clear();
                }
            }
            ui.label(&self.message);
            return;
        }
        // ajustement des coins
        let photo = self.photo.clone().unwrap();
        let tex = self.tex.get_or_insert_with(|| ui.ctx().load_texture("scan", egui::ColorImage::from_rgb([photo.width() as usize, photo.height() as usize], photo.as_raw()), Default::default())).clone();
        ui.weak("Glisse les coins roses sur les coins de la feuille.");
        let l = ui.available_width();
        let h = l * photo.height() as f32 / photo.width() as f32;
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(l, h), egui::Sense::drag());
        let p = ui.painter_at(zone);
        p.image(tex.id(), zone, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        let ecran = |c: Pos2| Pos2::new(zone.left() + c.x * zone.width(), zone.top() + c.y * zone.height());
        if rep.drag_started() {
            if let Some(m) = rep.interact_pointer_pos() {
                self.tenu = (0..4).min_by(|a, b| ecran(self.coins[*a]).distance(m).total_cmp(&ecran(self.coins[*b]).distance(m)));
            }
        }
        if rep.dragged() {
            if let (Some(k), Some(m)) = (self.tenu, rep.interact_pointer_pos()) {
                self.coins[k] = Pos2::new(((m.x - zone.left()) / zone.width()).clamp(0.0, 1.0), ((m.y - zone.top()) / zone.height()).clamp(0.0, 1.0));
            }
        }
        let pts: Vec<Pos2> = self.coins.iter().map(|c| ecran(*c)).collect();
        p.add(egui::Shape::closed_line(pts.clone(), egui::Stroke::new(2.5_f32, Color32::from_rgb(255, 95, 162))));
        for q in pts {
            p.circle_filled(q, 14.0, Color32::from_rgba_unmultiplied(255, 95, 162, 160));
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.rendu, Rendu::Document, "📃 Document");
            ui.selectable_value(&mut self.rendu, Rendu::Gris, "Gris");
            ui.selectable_value(&mut self.rendu, Rendu::Couleur, "🎨 Couleur");
        });
        ui.horizontal_wrapped(|ui| {
            if gros_bouton(ui, "✔ Garder cette page") {
                if let Some(page) = redresser(&photo, &self.coins, 1240) {
                    let apercu = image::DynamicImage::ImageRgb8(page.clone()).thumbnail(160, 230).to_rgb8();
                    self.apercus.push(ui.ctx().load_texture(format!("page{}", self.pages.len()), egui::ColorImage::from_rgb([apercu.width() as usize, apercu.height() as usize], apercu.as_raw()), Default::default()));
                    self.pages.push((page, self.rendu));
                    self.photo = None;
                    self.message = "Page ajoutée. Une autre ?".into();
                } else {
                    self.message = "Coins impossibles : écarte-les un peu.".into();
                }
            }
            if ui.button("✖ Reprendre").clicked() {
                self.photo = None;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner() {
        // feuille blanche au milieu d'un fond noir
        let g = GrayImage::from_fn(200, 200, |x, y| image::Luma([if (40..160).contains(&x) && (30..170).contains(&y) { 240 } else { 20 }]));
        let c = trouver_coins(&g);
        assert!((c[0].x - 0.2).abs() < 0.03 && (c[0].y - 0.15).abs() < 0.03, "{c:?}");
        assert!((c[2].x - 0.795).abs() < 0.03 && (c[2].y - 0.845).abs() < 0.03, "{c:?}");
        let q = [Pos2::new(10.0, 10.0), Pos2::new(110.0, 20.0), Pos2::new(100.0, 120.0), Pos2::new(5.0, 100.0)];
        let h = homographie(&q).unwrap();
        let w = h[6] + h[7] + 1.0;
        assert!(((h[0] + h[1] + h[2]) / w - 100.0).abs() < 0.01 && ((h[3] + h[4] + h[5]) / w - 120.0).abs() < 0.01);
        let img = RgbImage::from_pixel(100, 100, image::Rgb([200, 200, 200]));
        let r = redresser(&img, &[Pos2::new(0.1, 0.1), Pos2::new(0.9, 0.1), Pos2::new(0.9, 0.9), Pos2::new(0.1, 0.9)], 50).unwrap();
        assert_eq!(r.dimensions(), (50, 70));
        let nb = noir_et_blanc(&r);
        assert!(nb.pixels().all(|p| p[0] == 255));
        let pdf = faire_pdf(&[(2, 2, vec![0xFF, 0xD8, 0xFF, 0xD9], true)]);
        assert!(pdf.starts_with(b"%PDF-1.4") && pdf.ends_with(b"%%EOF\n"));
        assert_eq!(orientation_exif(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), 1);
    }
}
