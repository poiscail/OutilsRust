//! Animal de bureau (35) : un petit compagnon en pixel art qui se promène sur la barre des tâches,
//! s'assoit, s'endort, suit la souris, aime les caresses (clic) et se laisse porter.
use crate::calque::{bouton_gauche, souris, zone_travail, Calque};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, RichText};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Espece {
    Chat,
    Chien,
    Slime,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub espece: Espece,
    pub couleur: [u8; 3],
    pub nom: String,
    pub taille: u32,
    pub suivre: bool,
    pub cacher_en_jeu: bool,
    pub rappels: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: false, espece: Espece::Chat, couleur: [240, 160, 70], nom: "Caramel".into(), taille: 6, suivre: true, cacher_en_jeu: true, rappels: true }
    }
}

// Sprites 16×12, tournés vers la gauche. X contour, o corps, w clair, b foncé, e œil, p rose, n truffe.
const CHAT: [&str; 12] = [
    "................",
    ".X...X..........",
    "XoX.XoX.........",
    "XoooooX.........",
    "XoeoeoX......X..",
    "XoopooX.....XoX.",
    ".XoooooXXXXXoX..",
    "..XooooooooooX..",
    "..XowwwwwwoooX..",
    "..XoXooXXooXoX..",
    "..XX.XX..XX.XX..",
    "................",
];
const CHIEN: [&str; 12] = [
    "................",
    "................",
    ".XXXXXXX........",
    "XbooooobX.......",
    "XbeooeobX....X..",
    "XboonoobX...XoX.",
    ".XoooooXXXXXoX..",
    "..XooooooooooX..",
    "..XowwwwwwoooX..",
    "..XoXooXXooXoX..",
    "..XX.XX..XX.XX..",
    "................",
];
const SLIME: [&str; 12] = [
    "................",
    "................",
    "................",
    "................",
    ".....XXXXXX.....",
    "...XXooooooXX...",
    "..XoowooooooX...",
    ".XoowwoooooooX..",
    ".XooeooooeoooX..",
    ".XoooooppoooooX.",
    "..XXXXXXXXXXXX..",
    "................",
];
const PATTES_B: &str = ".XX..XX..XX..XX.";
const PATTES_ASSIS: [&str; 2] = ["..XooooooooooX..", "..XXXXXXXXXXXX.."];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pose {
    Marche(bool), // pas A / pas B
    Assis,
    Dort,
}

/// Grille du sprite pour une pose.
pub fn sprite(e: Espece, pose: Pose) -> Vec<String> {
    let base = match e {
        Espece::Chat => CHAT,
        Espece::Chien => CHIEN,
        Espece::Slime => SLIME,
    };
    let mut v: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    match (e, pose) {
        (Espece::Slime, Pose::Marche(true)) => {
            // rebond : tout descend d'un cran
            v.rotate_right(1);
        }
        (Espece::Slime, _) => {}
        (_, Pose::Marche(true)) => v[10] = PATTES_B.into(),
        (_, Pose::Assis | Pose::Dort) => {
            v[9] = PATTES_ASSIS[0].into();
            v[10] = PATTES_ASSIS[1].into();
        }
        _ => {}
    }
    if pose == Pose::Dort {
        for l in v.iter_mut() {
            *l = l.replace('e', "X");
        }
    }
    v
}

fn eclaircir(c: [u8; 3], f: f32) -> Rgba<u8> {
    let m = |v: u8| (v as f32 + (255.0 - v as f32) * f) as u8;
    Rgba([m(c[0]), m(c[1]), m(c[2]), 255])
}

fn assombrir(c: [u8; 3], f: f32) -> Rgba<u8> {
    let m = |v: u8| (v as f32 * (1.0 - f)) as u8;
    Rgba([m(c[0]), m(c[1]), m(c[2]), 255])
}

/// Dessine le sprite (agrandi `z` fois, tourné vers la droite si `droite`) dans `img` en (x0, y0).
pub fn peindre(img: &mut RgbaImage, s: &[String], couleur: [u8; 3], z: u32, x0: u32, y0: u32, droite: bool) {
    for (y, l) in s.iter().enumerate() {
        for (x, ch) in l.chars().enumerate() {
            let c = match ch {
                'X' => Rgba([30, 25, 35, 255]),
                'o' => Rgba([couleur[0], couleur[1], couleur[2], 255]),
                'w' => eclaircir(couleur, 0.6),
                'b' => assombrir(couleur, 0.35),
                'e' => Rgba([20, 20, 20, 255]),
                'p' => Rgba([255, 140, 170, 255]),
                'n' => Rgba([15, 15, 15, 255]),
                _ => continue,
            };
            let xx = if droite { 15 - x } else { x } as u32;
            for dy in 0..z {
                for dx in 0..z {
                    let (px, py) = (x0 + xx * z + dx, y0 + y as u32 * z + dy);
                    if px < img.width() && py < img.height() {
                        img.put_pixel(px, py, c);
                    }
                }
            }
        }
    }
}

fn coeur(img: &mut RgbaImage, x0: u32, y0: u32, z: u32) {
    const C: [&str; 6] = [".XX.XX.", "XXXXXXX", "XXXXXXX", ".XXXXX.", "..XXX..", "...X..."];
    for (y, l) in C.iter().enumerate() {
        for (x, ch) in l.chars().enumerate() {
            if ch == 'X' {
                for dy in 0..z {
                    for dx in 0..z {
                        let (px, py) = (x0 + x as u32 * z + dx, y0 + y as u32 * z + dy);
                        if px < img.width() && py < img.height() {
                            img.put_pixel(px, py, Rgba([240, 60, 90, 255]));
                        }
                    }
                }
            }
        }
    }
}

fn bulle(img: &mut RgbaImage, texte: &str, police: Option<&ab_glyph::FontVec>, centre: u32) {
    let Some(f) = police else { return };
    let t = 17.0;
    let l = crate::creatif::largeur(f, t, texte) + 20.0;
    let x = (centre as f32 - l / 2.0).clamp(0.0, img.width() as f32 - l);
    crate::atelier::rectangle(img, x - 2.0, 2.0, l + 4.0, t + 16.0, Rgba([30, 25, 35, 255]));
    crate::atelier::rectangle(img, x, 4.0, l, t + 12.0, Rgba([255, 255, 255, 255]));
    crate::atelier::polygone(img, &[(centre as f32 - 8.0, t + 17.0), (centre as f32 + 8.0, t + 17.0), (centre as f32, t + 28.0)], Rgba([255, 255, 255, 255]));
    crate::creatif::ecrire(img, f, t, x + 10.0, 8.0, texte, [30, 25, 35], None);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Etat {
    Marche,
    Assis,
    Dort,
    Porte,
    Tombe(f32),
}

struct Animal {
    x: f32,
    y: f32, // décalage au-dessus du sol (en l'air quand on le porte)
    dir: f32,
    etat: Etat,
    jusque: Instant,
    message: Option<(String, Instant)>,
    coeur: Option<Instant>,
    souris_avant: (i32, i32),
    immobile: Instant,
    derniere_heure: u32,
    session: Instant,
}

fn heure() -> (u32, u32) {
    let h = crate::minecraft::horodatage();
    (h.get(9..11).and_then(|v| v.parse().ok()).unwrap_or(12), h.get(11..13).and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn aleatoire(a: u64, b: u64) -> u64 {
    a + crate::outil::hasard(b - a + 1)
}

fn boucle(r: Arc<Mutex<Reglages>>, vivant: Arc<AtomicBool>) {
    let calque = Calque::new("OutilsCompagnon", false, &[]);
    let police = crate::creatif::police(&["segoeuib.ttf", "arialbd.ttf", "DejaVuSans-Bold.ttf"]);
    let (g, _, d, _) = zone_travail();
    let mut a = Animal { x: (g + d) as f32 / 2.0, y: 0.0, dir: -1.0, etat: Etat::Marche, jusque: Instant::now() + Duration::from_secs(5), message: None, coeur: None, souris_avant: souris(), immobile: Instant::now(), derniere_heure: heure().0, session: Instant::now() };
    let mut visible = false;
    let mut image_t = 0u64;
    while vivant.load(Relaxed) {
        std::thread::sleep(Duration::from_millis(80));
        image_t += 1;
        let cfg = r.lock().unwrap().clone();
        let en_jeu = cfg.cacher_en_jeu && crate::outil::jeu_actif().is_some();
        if !cfg.actif || en_jeu {
            if visible {
                calque.cacher();
                visible = false;
            }
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        let (gauche, _, droite, bas) = zone_travail();
        let z = cfg.taille.clamp(3, 10);
        let (sl, sh) = (16 * z, 12 * z);
        let (w, h) = ((sl + 120).max(240), sh + 60);
        let m = souris();
        if m != a.souris_avant {
            a.immobile = Instant::now();
        }
        a.souris_avant = m;
        let maintenant = Instant::now();
        // clic : caresse, ou on le prend si le bouton reste appuyé
        let clics = calque.clics();
        if clics > 0 {
            std::thread::sleep(Duration::from_millis(160));
            if bouton_gauche() {
                a.etat = Etat::Porte;
            } else {
                a.coeur = Some(maintenant);
                a.etat = Etat::Assis;
                a.jusque = maintenant + Duration::from_secs(3);
                let phrases = ["Ronron…", "Encore !", "Merci !", "Tu es gentil·le", "Hihi"];
                a.message = Some((phrases[crate::outil::hasard(phrases.len() as u64) as usize].to_string(), maintenant));
            }
        }
        match a.etat {
            Etat::Porte => {
                a.x = m.0 as f32;
                a.y = (bas - m.1) as f32 - sh as f32 / 2.0;
                if !bouton_gauche() {
                    a.etat = Etat::Tombe(0.0);
                }
            }
            Etat::Tombe(v) => {
                let v = v + 3.0;
                a.y -= v;
                if a.y <= 0.0 {
                    a.y = 0.0;
                    a.etat = Etat::Assis;
                    a.jusque = maintenant + Duration::from_secs(2);
                    a.message = Some(("Ouf !".into(), maintenant));
                } else {
                    a.etat = Etat::Tombe(v);
                }
            }
            Etat::Marche => {
                // suit la souris quand elle passe près du sol
                let proche = cfg.suivre && (bas - m.1) < 250 && (m.0 as f32 - a.x).abs() > 40.0 && a.immobile.elapsed() < Duration::from_secs(2);
                if proche {
                    a.dir = (m.0 as f32 - a.x).signum();
                }
                a.x += a.dir * z as f32 * if proche { 1.4 } else { 0.8 };
                if a.x < (gauche + sl as i32 / 2) as f32 || a.x > (droite - sl as i32 / 2) as f32 {
                    a.dir = -a.dir;
                    a.x = a.x.clamp((gauche + sl as i32 / 2) as f32, (droite - sl as i32 / 2) as f32);
                }
                if maintenant > a.jusque && !proche {
                    a.etat = Etat::Assis;
                    a.jusque = maintenant + Duration::from_secs(aleatoire(3, 8));
                }
            }
            Etat::Assis => {
                if maintenant > a.jusque {
                    if a.immobile.elapsed() > Duration::from_secs(60) {
                        a.etat = Etat::Dort;
                    } else {
                        a.etat = Etat::Marche;
                        a.dir = if crate::outil::hasard(2) == 0 { -1.0 } else { 1.0 };
                        a.jusque = maintenant + Duration::from_secs(aleatoire(4, 10));
                    }
                }
            }
            Etat::Dort => {
                if a.immobile.elapsed() < Duration::from_secs(1) && ((m.0 as f32 - a.x).abs() < 200.0) {
                    a.etat = Etat::Assis;
                    a.jusque = maintenant + Duration::from_secs(2);
                    a.message = Some(("Hein ? Quoi ?".into(), maintenant));
                }
            }
        }
        // rappels
        if cfg.rappels {
            let (hh, mm) = heure();
            if hh != a.derniere_heure && mm == 0 {
                a.derniere_heure = hh;
                let t = if hh >= 22 || hh < 6 { format!("Il est {hh} h… au lit !") } else { format!("Il est {hh} h !") };
                a.message = Some((t, maintenant));
            }
            if a.session.elapsed() > Duration::from_secs(3600) {
                a.session = maintenant;
                a.message = Some(("Une pause ? Bois de l'eau".into(), maintenant));
            }
        }
        if a.message.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(5)) {
            a.message = None;
        }
        // dessin
        let pose = match a.etat {
            Etat::Marche => Pose::Marche((image_t / 3) % 2 == 0),
            Etat::Dort => Pose::Dort,
            _ => Pose::Assis,
        };
        let mut img = RgbaImage::new(w, h);
        let sx = (w - sl) / 2;
        peindre(&mut img, &sprite(cfg.espece, pose), cfg.couleur, z, sx, h - sh, a.dir > 0.0);
        if a.etat == Etat::Dort && police.is_some() {
            let zz = ["z", "zZ", "zZz"][(image_t / 8 % 3) as usize];
            crate::creatif::ecrire(&mut img, police.as_ref().unwrap(), 20.0, (sx + sl) as f32 - 10.0, (h - sh) as f32 - 10.0, zz, [80, 120, 255], Some(([255, 255, 255], 2)));
        }
        if a.coeur.is_some_and(|t| t.elapsed() < Duration::from_millis(1500)) {
            let monte = (a.coeur.unwrap().elapsed().as_millis() / 60) as u32;
            coeur(&mut img, w / 2 - 14, (h - sh).saturating_sub(30 + monte.min(25)), 4);
        }
        if let Some((t, _)) = &a.message {
            let n = if cfg.nom.trim().is_empty() { t.clone() } else { format!("{} : {t}", cfg.nom.trim()) };
            bulle(&mut img, &n, police.as_ref(), w / 2);
        }
        let x = a.x as i32 - w as i32 / 2;
        let y = bas - h as i32 - a.y as i32 + z as i32; // les pattes touchent la barre des tâches
        calque.montrer(img, x, y);
        visible = true;
    }
    calque.cacher();
}

pub struct Compagnon {
    r: Arc<Mutex<Reglages>>,
    vivant: Arc<AtomicBool>,
    apercu: Option<(Reglages, egui::TextureHandle)>,
}

impl Compagnon {
    pub fn new() -> Self {
        let r: Reglages = charger("compagnon");
        let c = Compagnon { r: Arc::new(Mutex::new(r)), vivant: Arc::new(AtomicBool::new(true)), apercu: None };
        let (r, v) = (c.r.clone(), c.vivant.clone());
        std::thread::spawn(move || boucle(r, v));
        c
    }
}

impl Drop for Compagnon {
    fn drop(&mut self) {
        self.vivant.store(false, Relaxed);
    }
}

impl Outil for Compagnon {
    fn nom(&self) -> &'static str {
        "🐾 Animal de bureau"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Animal de bureau");
        ui.weak("Un petit compagnon se promène sur ta barre des tâches. Clique dessus pour le caresser, garde le bouton appuyé pour le porter ailleurs.");
        let mut r = self.r.lock().unwrap().clone();
        let avant = r.clone();
        ui.checkbox(&mut r.actif, RichText::new("Afficher mon animal").strong());
        ui.horizontal_wrapped(|ui| {
            for (e, n) in [(Espece::Chat, "🐱 Chat"), (Espece::Chien, "🐶 Chien"), (Espece::Slime, "🟢 Slime")] {
                ui.selectable_value(&mut r.espece, e, n);
            }
            ui.color_edit_button_srgb(&mut r.couleur);
        });
        ui.horizontal(|ui| {
            ui.label("Nom");
            ui.add(egui::TextEdit::singleline(&mut r.nom).desired_width(120.0));
            ui.label("Taille");
            ui.add(egui::Slider::new(&mut r.taille, 3..=10));
        });
        ui.checkbox(&mut r.suivre, "Il suit la souris quand elle passe près de lui");
        ui.checkbox(&mut r.cacher_en_jeu, "Il se cache pendant les jeux");
        ui.checkbox(&mut r.rappels, "Il donne l'heure et rappelle de faire une pause");
        if self.apercu.as_ref().is_none_or(|(a, _)| *a != r) {
            let mut img = RgbaImage::new(16 * 8 * 3 + 40, 12 * 8);
            for (k, p) in [Pose::Marche(false), Pose::Assis, Pose::Dort].into_iter().enumerate() {
                peindre(&mut img, &sprite(r.espece, p), r.couleur, 8, k as u32 * (16 * 8 + 20), 0, false);
            }
            let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.apercu = Some((r.clone(), ui.ctx().load_texture("compagnon", c, egui::TextureOptions::NEAREST)));
        }
        if let Some((_, t)) = &self.apercu {
            ui.add(egui::Image::new(t).fit_to_original_size(1.0));
        }
        if r != avant {
            enregistrer("compagnon", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprites() {
        for e in [Espece::Chat, Espece::Chien, Espece::Slime] {
            for p in [Pose::Marche(false), Pose::Marche(true), Pose::Assis, Pose::Dort] {
                let s = sprite(e, p);
                assert_eq!(s.len(), 12);
                assert!(s.iter().all(|l| l.chars().count() == 16), "{e:?} {p:?}");
            }
        }
        assert!(!sprite(Espece::Chat, Pose::Dort).concat().contains('e'));
        let mut img = RgbaImage::new(96, 72);
        peindre(&mut img, &sprite(Espece::Chat, Pose::Assis), [200, 100, 50], 6, 0, 0, true);
        assert!(img.pixels().any(|p| p[3] == 255));
    }
}
