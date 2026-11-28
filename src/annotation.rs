//! Dessiner sur l'écran (10) : pour un exposé, un tuto ou un appel vidéo. On dessine par-dessus
//! n'importe quelle fenêtre (stylo, surligneur, flèche, rectangle, cercle), avec une loupe et un
//! projecteur qui assombrit tout sauf autour de la souris. Tout se pilote par raccourcis.
use crate::calque::{bouton_gauche, souris, Calque};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Forme {
    #[default]
    Stylo,
    Surligneur,
    Fleche,
    Rectangle,
    Cercle,
}

const FORMES: [(Forme, &str); 5] = [(Forme::Stylo, "✏ Stylo"), (Forme::Surligneur, "🖍 Surligneur"), (Forme::Fleche, "➡ Flèche"), (Forme::Rectangle, "▭ Rectangle"), (Forme::Cercle, "◯ Cercle")];
const COULEURS: [[u8; 3]; 6] = [[235, 50, 50], [255, 210, 0], [40, 200, 80], [40, 140, 255], [255, 255, 255], [20, 20, 20]];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Reglages {
    forme: Forme,
    couleur: [u8; 3],
    epaisseur: f32,
    zoom: f32,
    rayon_projecteur: f32,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { forme: Forme::Stylo, couleur: COULEURS[0], epaisseur: 5.0, zoom: 2.0, rayon_projecteur: 170.0 }
    }
}

#[derive(Clone, PartialEq)]
pub struct Trait {
    forme: Forme,
    couleur: [u8; 3],
    epaisseur: f32,
    points: Vec<(f32, f32)>,
}

#[derive(Default)]
struct Etat {
    dessin: bool,
    loupe: bool,
    projecteur: bool,
    traits: Vec<Trait>,
    en_cours: Option<Trait>,
    effacer: bool,
    annuler: bool,
}

/// Segments à tracer pour un trait (le cercle et le rectangle sont définis par 2 coins).
pub fn segments(t: &Trait) -> Vec<((f32, f32), (f32, f32))> {
    let p = &t.points;
    let (Some(&a), Some(&b)) = (p.first(), p.last()) else { return vec![] };
    match t.forme {
        Forme::Stylo | Forme::Surligneur => {
            if p.len() == 1 {
                return vec![(a, a)];
            }
            p.windows(2).map(|w| (w[0], w[1])).collect()
        }
        Forme::Fleche => {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let l = (dx * dx + dy * dy).sqrt().max(1e-3);
            let (ux, uy) = (dx / l, dy / l);
            let pointe = (4.0 * t.epaisseur + 14.0).min(l * 0.6);
            let (c, s) = (0.42f32.cos(), 0.42f32.sin());
            let g = (b.0 - pointe * (ux * c - uy * s), b.1 - pointe * (uy * c + ux * s));
            let d = (b.0 - pointe * (ux * c + uy * s), b.1 - pointe * (uy * c - ux * s));
            vec![(a, b), (b, g), (b, d)]
        }
        Forme::Rectangle => {
            let (c1, c2) = ((b.0, a.1), (a.0, b.1));
            vec![(a, c1), (c1, b), (b, c2), (c2, a)]
        }
        Forme::Cercle => {
            let (cx, cy, rx, ry) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, (b.0 - a.0).abs() / 2.0, (b.1 - a.1).abs() / 2.0);
            let n = 72;
            let pt = |k: usize| {
                let t = k as f32 / n as f32 * std::f32::consts::TAU;
                (cx + rx * t.cos(), cy + ry * t.sin())
            };
            (0..n).map(|k| (pt(k), pt(k + 1))).collect()
        }
    }
}

/// Pose `c` (alpha `a` 0..1) sur un pixel RGBA non prémultiplié.
fn poser(p: &mut [u8], c: [u8; 3], a: f32) {
    let da = p[3] as f32 / 255.0;
    let oa = a + da * (1.0 - a);
    if oa <= 0.0 {
        return;
    }
    for k in 0..3 {
        p[k] = ((c[k] as f32 * a + p[k] as f32 * da * (1.0 - a)) / oa).round() as u8;
    }
    p[3] = (oa * 255.0).round() as u8;
}

/// Trace un trait épais et lissé. Le masque évite que le surligneur fonce là où il repasse.
pub fn tracer(img: &mut RgbaImage, t: &Trait) {
    let segs = segments(t);
    if segs.is_empty() {
        return;
    }
    let (r, opacite) = match t.forme {
        Forme::Surligneur => (t.epaisseur * 2.5, 0.4),
        _ => (t.epaisseur / 2.0, 1.0),
    };
    let (w, h) = (img.width() as i64, img.height() as i64);
    let bord = |f: fn(f32, f32) -> f32, init: f32, axe: usize| segs.iter().fold(init, |m, (a, b)| f(m, f(if axe == 0 { a.0 } else { a.1 }, if axe == 0 { b.0 } else { b.1 })));
    let x0 = ((bord(f32::min, f32::MAX, 0) - r - 2.0).floor() as i64).max(0);
    let y0 = ((bord(f32::min, f32::MAX, 1) - r - 2.0).floor() as i64).max(0);
    let x1 = ((bord(f32::max, f32::MIN, 0) + r + 2.0).ceil() as i64).min(w);
    let y1 = ((bord(f32::max, f32::MIN, 1) + r + 2.0).ceil() as i64).min(h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let lm = (x1 - x0) as usize;
    let mut masque = vec![0f32; lm * (y1 - y0) as usize];
    for (a, b) in &segs {
        let sx0 = ((a.0.min(b.0) - r - 2.0).floor() as i64).max(x0);
        let sx1 = ((a.0.max(b.0) + r + 2.0).ceil() as i64).min(x1);
        let sy0 = ((a.1.min(b.1) - r - 2.0).floor() as i64).max(y0);
        let sy1 = ((a.1.max(b.1) + r + 2.0).ceil() as i64).min(y1);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l2 = (dx * dx + dy * dy).max(1e-6);
        for y in sy0..sy1 {
            for x in sx0..sx1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let u = (((px - a.0) * dx + (py - a.1) * dy) / l2).clamp(0.0, 1.0);
                let (ex, ey) = (px - a.0 - u * dx, py - a.1 - u * dy);
                let couv = (r + 0.5 - (ex * ex + ey * ey).sqrt()).clamp(0.0, 1.0);
                let m = &mut masque[(y - y0) as usize * lm + (x - x0) as usize];
                *m = m.max(couv);
            }
        }
    }
    for y in y0..y1 {
        for x in x0..x1 {
            let m = masque[(y - y0) as usize * lm + (x - x0) as usize];
            if m > 0.0 {
                let i = ((y * w + x) * 4) as usize;
                poser(&mut img.as_mut()[i..i + 4], t.couleur, m * opacite);
            }
        }
    }
}

/// Image finale : projecteur, puis dessins, puis loupe.
fn composer(l: u32, h: u32, couche: &RgbaImage, en_cours: Option<&Trait>, projecteur: Option<(f32, f32, f32)>, loupe: Option<(&RgbaImage, f32, f32, f32)>) -> RgbaImage {
    let mut img = match projecteur {
        Some((cx, cy, r)) => RgbaImage::from_fn(l, h, |x, y| {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let a = ((d - r) / 30.0).clamp(0.0, 1.0) * 200.0;
            image::Rgba([0, 0, 0, a as u8])
        }),
        None => RgbaImage::new(l, h),
    };
    for (o, s) in img.as_mut().chunks_exact_mut(4).zip(couche.as_raw().chunks_exact(4)) {
        if s[3] > 0 {
            poser(o, [s[0], s[1], s[2]], s[3] as f32 / 255.0);
        }
    }
    if let Some(t) = en_cours {
        tracer(&mut img, t);
    }
    if let Some((ecran, cx, cy, zoom)) = loupe {
        let r = 170.0f32;
        let (x0, x1) = ((cx - r - 4.0).max(0.0) as u32, ((cx + r + 4.0) as u32).min(l));
        let (y0, y1) = ((cy - r - 4.0).max(0.0) as u32, ((cy + r + 4.0) as u32).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                let d = (dx * dx + dy * dy).sqrt();
                let p = img.get_pixel_mut(x, y);
                if d < r {
                    let (sx, sy) = ((cx + dx / zoom) as u32, (cy + dy / zoom) as u32);
                    if sx < ecran.width() && sy < ecran.height() {
                        let s = ecran.get_pixel(sx, sy);
                        *p = image::Rgba([s[0], s[1], s[2], 255]);
                    }
                } else if d < r + 4.0 {
                    *p = image::Rgba([255, 255, 255, 230]);
                }
            }
        }
    }
    img
}

fn touche(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

pub struct Annotation {
    r: Arc<Mutex<Reglages>>,
    etat: Arc<Mutex<Etat>>,
    pris: Arc<std::sync::atomic::AtomicBool>,
}

impl Annotation {
    pub fn new() -> Self {
        let a = Annotation { r: Arc::new(Mutex::new(charger("annotation"))), etat: Arc::default(), pris: Arc::default() };
        let (r, etat, pris) = (a.r.clone(), a.etat.clone(), a.pris.clone());
        std::thread::spawn(move || {
            let m = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
            // 0 dessin, 1 projecteur, 2 loupe, 3 tout effacer, 4 annuler, 5 couleur suivante, 6..10 formes
            let mut touches = vec![(m, b'D' as u32), (m, b'P' as u32), (m, b'Z' as u32), (m, b'X' as u32), (m, b'A' as u32), (m, b'C' as u32)];
            touches.extend((b'1'..=b'5').map(|k| (m, k as u32)));
            let vue = Calque::new("OutilsAnnotationVue", true, &touches);
            let saisie = Calque::new("OutilsAnnotationSaisie", false, &[]);
            std::thread::sleep(Duration::from_millis(300));
            pris.store(vue.raccourci_pris.load(std::sync::atomic::Ordering::Relaxed), std::sync::atomic::Ordering::Relaxed);
            let (l, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN).max(1) as u32, GetSystemMetrics(SM_CYSCREEN).max(1) as u32) };
            let mut couche = RgbaImage::new(l, h);
            let mut traits_dessines = 0usize;
            let mut ecran: Option<RgbaImage> = None;
            let mut saisie_montree = false;
            let mut dernier: Option<(usize, Option<Trait>, bool, bool, (i32, i32))> = None;
            let mut actif = false;
            loop {
                // au repos (rien à l'écran) : on regarde les raccourcis 10 fois par seconde seulement
                std::thread::sleep(Duration::from_millis(if actif { 12 } else { 100 }));
                {
                    let e = etat.lock().unwrap();
                    actif = e.dessin || e.loupe || e.projecteur || e.en_cours.is_some() || e.effacer || e.annuler;
                }
                let cfg = r.lock().unwrap().clone();
                let mut e = etat.lock().unwrap();
                // raccourcis
                if vue.appuis(0) % 2 == 1 {
                    e.dessin = !e.dessin;
                }
                if vue.appuis(1) % 2 == 1 {
                    e.projecteur = !e.projecteur;
                }
                if vue.appuis(2) % 2 == 1 {
                    e.loupe = !e.loupe;
                }
                if vue.appuis(3) > 0 {
                    e.effacer = true;
                }
                for _ in 0..vue.appuis(4) {
                    e.annuler = true;
                }
                let n = vue.appuis(5) as usize;
                let mut nouv = cfg.clone();
                if n > 0 {
                    let k = COULEURS.iter().position(|c| *c == cfg.couleur).map(|k| k + 1).unwrap_or(0);
                    nouv.couleur = COULEURS[(k + n - 1) % COULEURS.len()];
                }
                for (k, (f, _)) in FORMES.iter().enumerate() {
                    if vue.appuis(6 + k) > 0 {
                        nouv.forme = *f;
                        e.dessin = true;
                    }
                }
                if nouv != cfg {
                    *r.lock().unwrap() = nouv.clone();
                    enregistrer("annotation", &nouv);
                }
                let cfg = nouv;
                // Échap ou clic droit : on arrête de dessiner (les dessins restent)
                if e.dessin && (touche(0x1B) || touche(0x02)) {
                    e.dessin = false;
                    e.en_cours = None;
                }
                if e.effacer {
                    e.effacer = false;
                    e.traits.clear();
                    e.en_cours = None;
                }
                if e.annuler {
                    e.annuler = false;
                    e.traits.pop();
                }
                // tracé à la souris
                let (sx, sy) = souris();
                let p = (sx as f32, sy as f32);
                if e.dessin && bouton_gauche() {
                    match &mut e.en_cours {
                        Some(t) => match t.forme {
                            Forme::Stylo | Forme::Surligneur => {
                                let d = t.points.last().map(|q| (q.0 - p.0).hypot(q.1 - p.1)).unwrap_or(9.0);
                                if d >= 2.0 {
                                    t.points.push(p);
                                }
                            }
                            _ => {
                                t.points.truncate(1);
                                t.points.push(p);
                            }
                        },
                        None => e.en_cours = Some(Trait { forme: cfg.forme, couleur: cfg.couleur, epaisseur: cfg.epaisseur, points: vec![p] }),
                    }
                } else if let Some(t) = e.en_cours.take() {
                    e.traits.push(t);
                }
                // la fenêtre de saisie (presque invisible) n'existe que pendant le dessin
                if e.dessin != saisie_montree {
                    saisie_montree = e.dessin;
                    if e.dessin {
                        saisie.montrer(RgbaImage::from_pixel(l, h, image::Rgba([0, 0, 0, 1])), 0, 0);
                    } else {
                        saisie.cacher();
                    }
                }
                if !e.loupe {
                    ecran = None;
                } else if ecran.is_none() {
                    // l'écran est figé au moment où la loupe s'ouvre
                    vue.cacher();
                    drop(e);
                    std::thread::sleep(Duration::from_millis(60));
                    ecran = crate::capture::win::capturer_zone(0, 0, l as i32, h as i32);
                    dernier = None;
                    continue;
                }
                // la couche des traits finis n'est refaite que si la liste change
                if e.traits.len() != traits_dessines {
                    if e.traits.len() < traits_dessines {
                        couche = RgbaImage::new(l, h);
                        for t in &e.traits {
                            tracer(&mut couche, t);
                        }
                    } else {
                        for t in &e.traits[traits_dessines..] {
                            tracer(&mut couche, t);
                        }
                    }
                    traits_dessines = e.traits.len();
                }
                let suit_souris = e.loupe || e.projecteur;
                let etat_vu = (e.traits.len(), e.en_cours.clone(), e.loupe, e.projecteur, if suit_souris { (sx, sy) } else { (0, 0) });
                if dernier.as_ref() == Some(&etat_vu) {
                    continue;
                }
                dernier = Some(etat_vu);
                if e.traits.is_empty() && e.en_cours.is_none() && !e.loupe && !e.projecteur {
                    vue.cacher();
                    continue;
                }
                let proj = e.projecteur.then_some((p.0, p.1, cfg.rayon_projecteur));
                let img = composer(l, h, &couche, e.en_cours.as_ref(), proj, ecran.as_ref().filter(|_| e.loupe).map(|s| (s, p.0, p.1, cfg.zoom)));
                drop(e);
                vue.montrer(img, 0, 0);
            }
        });
        a
    }
}

impl Outil for Annotation {
    fn nom(&self) -> &'static str {
        "✍ Dessiner sur l'écran"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dessiner sur l'écran");
        ui.weak("Pour un exposé, un tuto, un partage d'écran ou un appel vidéo : dessine par-dessus n'importe quelle fenêtre, puis continue à travailler normalement.");
        let mut cfg = self.r.lock().unwrap().clone();
        let mut e = self.etat.lock().unwrap();
        ui.horizontal_wrapped(|ui| {
            ui.toggle_value(&mut e.dessin, RichText::new("✍ Dessiner").strong());
            ui.toggle_value(&mut e.projecteur, "🔦 Projecteur");
            ui.toggle_value(&mut e.loupe, "🔍 Loupe");
            if ui.button("↩ Annuler").clicked() {
                e.annuler = true;
            }
            if ui.button("🗑 Tout effacer").clicked() {
                e.effacer = true;
            }
            ui.weak(format!("{} dessin(s) à l'écran", e.traits.len()));
        });
        drop(e);
        ui.horizontal_wrapped(|ui| {
            for (f, n) in FORMES {
                ui.selectable_value(&mut cfg.forme, f, n);
            }
        });
        ui.horizontal(|ui| {
            for c in COULEURS {
                let (rect, rep) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                ui.painter().circle_filled(rect.center(), 11.0, Color32::from_rgb(c[0], c[1], c[2]));
                if cfg.couleur == c {
                    ui.painter().circle_stroke(rect.center(), 13.0, egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color()));
                }
                if rep.clicked() {
                    cfg.couleur = c;
                }
            }
            ui.color_edit_button_srgb(&mut cfg.couleur);
        });
        ui.add(egui::Slider::new(&mut cfg.epaisseur, 1.0..=30.0).text("épaisseur"));
        ui.add(egui::Slider::new(&mut cfg.zoom, 1.5..=6.0).text("zoom de la loupe"));
        ui.add(egui::Slider::new(&mut cfg.rayon_projecteur, 60.0..=500.0).text("taille du projecteur"));
        if cfg != *self.r.lock().unwrap() {
            enregistrer("annotation", &cfg);
            *self.r.lock().unwrap() = cfg;
        }
        ui.separator();
        ui.strong("Raccourcis (marchent partout)");
        egui::Grid::new("raccourcis_annotation").striped(true).show(ui, |ui| {
            for (t, d) in [
                ("Ctrl+Alt+D", "dessiner / arrêter de dessiner"),
                ("Ctrl+Alt+1 à 5", "stylo, surligneur, flèche, rectangle, cercle"),
                ("Ctrl+Alt+C", "couleur suivante"),
                ("Ctrl+Alt+A", "annuler le dernier dessin"),
                ("Ctrl+Alt+X", "tout effacer"),
                ("Ctrl+Alt+P", "projecteur (assombrit tout sauf autour de la souris)"),
                ("Ctrl+Alt+Z", "loupe (l'écran est figé pendant le zoom)"),
                ("Échap ou clic droit", "arrêter de dessiner (les dessins restent)"),
            ] {
                ui.label(RichText::new(t).strong());
                ui.label(d);
                ui.end_row();
            }
        });
        if self.pris.load(std::sync::atomic::Ordering::Relaxed) {
            ui.colored_label(Color32::from_rgb(230, 160, 60), "⚠ Un autre logiciel utilise déjà certains de ces raccourcis : utilise les boutons ci-dessus.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traits() {
        let mut img = RgbaImage::new(100, 100);
        let t = Trait { forme: Forme::Stylo, couleur: [255, 0, 0], epaisseur: 6.0, points: vec![(10.0, 50.0), (90.0, 50.0)] };
        tracer(&mut img, &t);
        assert_eq!(img.get_pixel(50, 50).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(50, 60)[3], 0);
        // le surligneur ne fonce pas quand il repasse sur lui-même
        let mut img = RgbaImage::new(100, 100);
        let s = Trait { forme: Forme::Surligneur, couleur: [255, 255, 0], epaisseur: 4.0, points: vec![(10.0, 50.0), (90.0, 50.0), (10.0, 50.0)] };
        tracer(&mut img, &s);
        assert!((img.get_pixel(50, 50)[3] as i32 - 102).abs() <= 1);
        // rectangle : 4 côtés, l'intérieur reste vide
        let r = Trait { forme: Forme::Rectangle, couleur: [0, 0, 255], epaisseur: 2.0, points: vec![(20.0, 20.0), (80.0, 70.0)] };
        assert_eq!(segments(&r).len(), 4);
        let mut img = RgbaImage::new(100, 100);
        tracer(&mut img, &r);
        assert_eq!(img.get_pixel(50, 45)[3], 0);
        assert!(img.get_pixel(50, 20)[3] > 200);
        // flèche : un corps et deux branches qui partent de la pointe
        let f = Trait { forme: Forme::Fleche, couleur: [0, 0, 0], epaisseur: 3.0, points: vec![(0.0, 0.0), (100.0, 0.0)] };
        let s = segments(&f);
        assert_eq!(s.len(), 3);
        assert!(s[1].1 .0 < 100.0 && s[2].1 .0 < 100.0 && s[1].1 .1 * s[2].1 .1 < 0.0);
        // projecteur : le centre reste clair, les bords sont sombres
        let img = composer(200, 200, &RgbaImage::new(200, 200), None, Some((100.0, 100.0, 40.0)), None);
        assert_eq!(img.get_pixel(100, 100)[3], 0);
        assert_eq!(img.get_pixel(5, 5)[3], 200);
    }
}
