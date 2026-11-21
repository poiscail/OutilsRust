//! Fun, suite : échecs contre l'ordi (69), solitaire (70), mots mêlés (71), vitesse de frappe (73),
//! clicker (68), jeux à deux sur le même PC (74) et en réseau local (65).
use crate::minijeux2::{gagnant3, gagnant4, jouer4, Plateau4, C4H, C4L};
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ROSE: Color32 = Color32::from_rgb(255, 95, 162);
const OR: Color32 = Color32::from_rgb(250, 210, 90);

fn h(n: usize) -> usize {
    hasard(n as u64) as usize
}

fn melanger<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, h(i + 1));
    }
}

/// Glyphes (pièces d'échecs, couleurs de cartes) dessinés avec la police de symboles de Windows,
/// car la police de l'interface ne les a pas tous.
#[derive(Default)]
struct Glyphes {
    police: Option<Option<ab_glyph::FontVec>>,
    cache: HashMap<(char, [u8; 3], u32), Option<egui::TextureHandle>>,
}

impl Glyphes {
    fn dessiner(&mut self, ui: &egui::Ui, r: Rect, c: char, couleur: [u8; 3], contour: Option<[u8; 3]>) {
        let taille = r.height().round() as u32;
        let p = self.police.get_or_insert_with(|| crate::creatif::police(&["seguisym.ttf", "seguiemj.ttf", "arialuni.ttf", "DejaVuSans.ttf"]));
        let cle = (c, couleur, taille);
        if !self.cache.contains_key(&cle) {
            let t = p.as_ref().map(|f| {
                let s = taille.max(8);
                let mut img = image::RgbaImage::new(s + 8, s + 8);
                let lt = crate::creatif::largeur(f, s as f32 * 0.85, &c.to_string());
                crate::creatif::ecrire(&mut img, f, s as f32 * 0.85, ((s + 8) as f32 - lt) / 2.0, 2.0, &c.to_string(), couleur, contour.map(|x| (x, 2)));
                ui.ctx().load_texture(format!("glyphe_{c}_{couleur:?}_{s}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), Default::default())
            });
            self.cache.insert(cle, t);
        }
        match &self.cache[&cle] {
            Some(t) => {
                ui.painter().image(t.id(), r.expand(4.0), Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().text(r.center(), Align2::CENTER_CENTER, c, FontId::proportional(r.height() * 0.7), Color32::from_rgb(couleur[0], couleur[1], couleur[2]));
            }
        }
    }
}

// ===================================================================================================
// 69 · Échecs
// ===================================================================================================

/// Pièces : 1 pion, 2 cavalier, 3 fou, 4 tour, 5 dame, 6 roi ; positif = blancs. Case 0 = a8, 63 = h1.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Position {
    pub p: [i8; 64],
    pub trait_: i8,
    pub roques: [bool; 4], // blanc petit, blanc grand, noir petit, noir grand
    pub ep: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Coup {
    pub de: usize,
    pub vers: usize,
    pub promo: i8,
}

impl Default for Position {
    fn default() -> Self {
        let mut p = [0i8; 64];
        let fond = [4, 2, 3, 5, 6, 3, 2, 4];
        for c in 0..8 {
            p[c] = -fond[c];
            p[8 + c] = -1;
            p[48 + c] = 1;
            p[56 + c] = fond[c];
        }
        Position { p, trait_: 1, roques: [true; 4], ep: None }
    }
}

const CAVALIER: [(i32, i32); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const ROI: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const DIAG: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const DROIT: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn case(l: i32, c: i32) -> Option<usize> {
    (0..8).contains(&l).then_some(())?;
    (0..8).contains(&c).then_some((l * 8 + c) as usize)
}

impl Position {
    /// La case est-elle attaquée par le camp `par` ?
    pub fn attaquee(&self, s: usize, par: i8) -> bool {
        let (l, c) = ((s / 8) as i32, (s % 8) as i32);
        // pions : un pion blanc attaque vers le haut (ligne - 1)
        let dl = if par == 1 { 1 } else { -1 };
        for dc in [-1, 1] {
            if case(l + dl, c + dc).is_some_and(|k| self.p[k] == par) {
                return true;
            }
        }
        for (a, b) in CAVALIER {
            if case(l + a, c + b).is_some_and(|k| self.p[k] == 2 * par) {
                return true;
            }
        }
        for (a, b) in ROI {
            if case(l + a, c + b).is_some_and(|k| self.p[k] == 6 * par) {
                return true;
            }
        }
        for (dirs, pieces) in [(DIAG, [3, 5]), (DROIT, [4, 5])] {
            for (a, b) in dirs {
                let (mut x, mut y) = (l + a, c + b);
                while let Some(k) = case(x, y) {
                    let v = self.p[k];
                    if v != 0 {
                        if v.signum() == par && pieces.contains(&v.abs()) {
                            return true;
                        }
                        break;
                    }
                    x += a;
                    y += b;
                }
            }
        }
        false
    }

    pub fn roi(&self, camp: i8) -> usize {
        self.p.iter().position(|v| *v == 6 * camp).unwrap_or(0)
    }

    pub fn en_echec(&self, camp: i8) -> bool {
        self.attaquee(self.roi(camp), -camp)
    }

    fn pseudo(&self) -> Vec<Coup> {
        let mut v = vec![];
        let t = self.trait_;
        for s in 0..64 {
            let x = self.p[s];
            if x == 0 || x.signum() != t {
                continue;
            }
            let (l, c) = ((s / 8) as i32, (s % 8) as i32);
            let ajouter = |vers: usize, v: &mut Vec<Coup>| {
                let promo_ligne = if t == 1 { 0 } else { 7 };
                if x.abs() == 1 && vers / 8 == promo_ligne {
                    for p in [5, 4, 3, 2] {
                        v.push(Coup { de: s, vers, promo: p * t });
                    }
                } else {
                    v.push(Coup { de: s, vers, promo: 0 });
                }
            };
            match x.abs() {
                1 => {
                    let d = -(t as i32);
                    if let Some(k) = case(l + d, c).filter(|k| self.p[*k] == 0) {
                        ajouter(k, &mut v);
                        let depart = if t == 1 { 6 } else { 1 };
                        if l == depart {
                            if let Some(k2) = case(l + 2 * d, c).filter(|k| self.p[*k] == 0) {
                                v.push(Coup { de: s, vers: k2, promo: 0 });
                            }
                        }
                    }
                    for dc in [-1, 1] {
                        if let Some(k) = case(l + d, c + dc) {
                            if self.p[k] != 0 && self.p[k].signum() == -t || self.ep == Some(k) {
                                ajouter(k, &mut v);
                            }
                        }
                    }
                }
                2 | 6 => {
                    let sauts = if x.abs() == 2 { CAVALIER } else { ROI };
                    for (a, b) in sauts {
                        if let Some(k) = case(l + a, c + b).filter(|k| self.p[*k] == 0 || self.p[*k].signum() == -t) {
                            v.push(Coup { de: s, vers: k, promo: 0 });
                        }
                    }
                    if x.abs() == 6 {
                        let (i, base) = if t == 1 { (0, 60) } else { (2, 4) };
                        if s == base && !self.en_echec(t) {
                            if self.roques[i] && self.p[base + 1] == 0 && self.p[base + 2] == 0 && self.p[base + 3] == 4 * t && !self.attaquee(base + 1, -t) && !self.attaquee(base + 2, -t) {
                                v.push(Coup { de: s, vers: base + 2, promo: 0 });
                            }
                            if self.roques[i + 1] && self.p[base - 1] == 0 && self.p[base - 2] == 0 && self.p[base - 3] == 0 && self.p[base - 4] == 4 * t && !self.attaquee(base - 1, -t) && !self.attaquee(base - 2, -t) {
                                v.push(Coup { de: s, vers: base - 2, promo: 0 });
                            }
                        }
                    }
                }
                n => {
                    let dirs: &[(i32, i32)] = match n {
                        3 => &DIAG,
                        4 => &DROIT,
                        _ => &[(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)],
                    };
                    for (a, b) in dirs {
                        let (mut y, mut z) = (l + a, c + b);
                        while let Some(k) = case(y, z) {
                            if self.p[k] == 0 {
                                v.push(Coup { de: s, vers: k, promo: 0 });
                            } else {
                                if self.p[k].signum() == -t {
                                    v.push(Coup { de: s, vers: k, promo: 0 });
                                }
                                break;
                            }
                            y += a;
                            z += b;
                        }
                    }
                }
            }
        }
        v
    }

    pub fn jouer(&self, m: Coup) -> Position {
        let mut n = *self;
        let x = n.p[m.de];
        let t = self.trait_;
        n.p[m.vers] = if m.promo != 0 { m.promo } else { x };
        n.p[m.de] = 0;
        // prise en passant
        if x.abs() == 1 && Some(m.vers) == self.ep {
            let k = if t == 1 { m.vers + 8 } else { m.vers - 8 };
            n.p[k] = 0;
        }
        n.ep = (x.abs() == 1 && m.de.abs_diff(m.vers) == 16).then(|| (m.de + m.vers) / 2);
        // roque : la tour suit
        if x.abs() == 6 && m.de.abs_diff(m.vers) == 2 {
            let (de, vers) = if m.vers > m.de { (m.de + 3, m.de + 1) } else { (m.de - 4, m.de - 1) };
            n.p[vers] = n.p[de];
            n.p[de] = 0;
        }
        for (k, s) in [(0usize, 63usize), (1, 56), (2, 7), (3, 0)] {
            if m.de == s || m.vers == s {
                n.roques[k] = false;
            }
        }
        if x == 6 {
            n.roques[0] = false;
            n.roques[1] = false;
        }
        if x == -6 {
            n.roques[2] = false;
            n.roques[3] = false;
        }
        n.trait_ = -t;
        n
    }

    pub fn coups(&self) -> Vec<Coup> {
        self.pseudo().into_iter().filter(|m| !self.jouer(*m).en_echec(self.trait_)).collect()
    }
}

const VALEUR: [i32; 7] = [0, 100, 320, 330, 500, 900, 0];

fn evaluer(p: &Position) -> i32 {
    // matériel + présence au centre + avance des pions, du point de vue du camp qui a le trait
    let mut s = 0;
    for k in 0..64 {
        let x = p.p[k];
        if x == 0 {
            continue;
        }
        let (l, c) = ((k / 8) as i32, (k % 8) as i32);
        let centre = 6 - ((l * 2 - 7).abs() + (c * 2 - 7).abs()) / 2;
        let mut v = VALEUR[x.unsigned_abs() as usize];
        match x.abs() {
            2 | 3 => v += centre * 4,
            1 => v += (if x > 0 { 6 - l } else { l - 1 }) * 6 + if (2..6).contains(&c) { 4 } else { 0 },
            5 => v += centre * 2,
            _ => {}
        }
        s += v * x.signum() as i32;
    }
    s * p.trait_ as i32
}

fn negamax(p: &Position, prof: u32, mut a: i32, b: i32, noeuds: &mut u32) -> i32 {
    *noeuds += 1;
    let mut coups = p.coups();
    if coups.is_empty() {
        return if p.en_echec(p.trait_) { -100_000 - prof as i32 } else { 0 };
    }
    if prof == 0 {
        return evaluer(p);
    }
    // prises d'abord (la plus grosse victime avec le plus petit attaquant)
    coups.sort_by_key(|m| -(VALEUR[p.p[m.vers].unsigned_abs() as usize] * 10 - VALEUR[p.p[m.de].unsigned_abs() as usize] / 10));
    let mut meilleur = i32::MIN + 1;
    for m in coups {
        let v = -negamax(&p.jouer(m), prof - 1, -b, -a, noeuds);
        meilleur = meilleur.max(v);
        a = a.max(v);
        if a >= b {
            break;
        }
    }
    meilleur
}

/// Coup de l'ordinateur ; un peu de hasard entre coups de même valeur.
pub fn coup_ordi(p: &Position, prof: u32) -> Option<Coup> {
    let mut coups = p.coups();
    melanger(&mut coups);
    let mut meilleur = None;
    let mut bv = i32::MIN + 1;
    let mut n = 0;
    for m in coups {
        let v = -negamax(&p.jouer(m), prof.saturating_sub(1), i32::MIN + 1, -bv + 1, &mut n);
        if v > bv || meilleur.is_none() {
            bv = v;
            meilleur = Some(m);
        }
    }
    meilleur
}

fn nom_case(s: usize) -> String {
    format!("{}{}", (b'a' + (s % 8) as u8) as char, 8 - s / 8)
}

fn glyphe(x: i8) -> char {
    ['♟', '♞', '♝', '♜', '♛', '♚'][x.unsigned_abs() as usize - 1]
}

pub struct Echecs {
    pos: Position,
    historique: Vec<Position>,
    choisie: Option<usize>,
    niveau: u32,
    blancs: bool,
    reflexion: Arc<Mutex<Option<Option<Coup>>>>,
    dernier: Option<Coup>,
    glyphes: Glyphes,
    victoires: u32,
    compte: bool,
}

impl Echecs {
    pub fn new() -> Self {
        Echecs { pos: Position::default(), historique: vec![], choisie: None, niveau: 2, blancs: true, reflexion: Arc::default(), dernier: None, glyphes: Glyphes::default(), victoires: charger::<Option<u32>>("echecs_victoires").unwrap_or(0), compte: false }
    }

    fn ordi(&mut self) {
        let (p, prof, r) = (self.pos, self.niveau + 1, self.reflexion.clone());
        *r.lock().unwrap() = Some(None);
        std::thread::spawn(move || {
            let m = coup_ordi(&p, prof);
            *r.lock().unwrap() = Some(Some(m.unwrap_or(Coup { de: 64, vers: 64, promo: 0 })));
        });
    }
}

impl Outil for Echecs {
    fn nom(&self) -> &'static str {
        "♟ Échecs"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Échecs contre l'ordinateur");
        let moi: i8 = if self.blancs { 1 } else { -1 };
        let reflechit = self.reflexion.lock().unwrap().is_some();
        // coup de l'ordi arrivé ?
        let fini = self.reflexion.lock().unwrap().as_ref().and_then(|x| *x);
        if let Some(m) = fini {
            *self.reflexion.lock().unwrap() = None;
            if m.de < 64 {
                self.historique.push(self.pos);
                self.pos = self.pos.jouer(m);
                self.dernier = Some(m);
            }
        }
        let coups = self.pos.coups();
        let fin = if coups.is_empty() { Some(if self.pos.en_echec(self.pos.trait_) { if self.pos.trait_ == moi { "Échec et mat… l'ordi gagne 🤖" } else { "Échec et mat ! Tu as gagné 🎉" } } else { "Pat : match nul" }) } else { None };
        if fin.is_some() && self.pos.trait_ != moi && !self.compte && self.pos.en_echec(self.pos.trait_) {
            self.compte = true;
            self.victoires += 1;
            enregistrer("echecs_victoires", &Some(self.victoires));
        }
        if fin.is_none() && self.pos.trait_ != moi && !reflechit && fini.is_none() {
            self.ordi();
        }
        ui.horizontal(|ui| {
            for (n, k) in [("Facile", 1), ("Moyen", 2), ("Difficile", 4)] {
                ui.selectable_value(&mut self.niveau, k, n);
            }
            ui.separator();
            for (n, b) in [("Blancs", true), ("Noirs", false)] {
                if ui.selectable_label(self.blancs == b, n).clicked() && self.blancs != b {
                    self.blancs = b;
                    *self = Echecs { blancs: b, niveau: self.niveau, victoires: self.victoires, ..Echecs::new() };
                }
            }
            if ui.button("Nouvelle partie").clicked() {
                *self = Echecs { blancs: self.blancs, niveau: self.niveau, victoires: self.victoires, ..Echecs::new() };
            }
            if ui.add_enabled(!reflechit && self.historique.len() >= 2, egui::Button::new("↩ Annuler mon coup")).clicked() {
                self.historique.pop();
                self.pos = self.historique.pop().unwrap();
                self.dernier = None;
                self.compte = false;
            }
            ui.weak(format!("🏆 {} victoire(s)", self.victoires));
        });
        ui.label(match fin {
            Some(f) => RichText::new(f).size(20.0).strong(),
            None if reflechit => RichText::new("🤖 L'ordi réfléchit…"),
            None if self.pos.en_echec(self.pos.trait_) => RichText::new("⚠ Échec !").color(Color32::LIGHT_RED),
            None => RichText::new("À toi de jouer."),
        });
        if reflechit {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let c = ((ui.available_height() - 20.0).min(ui.available_width()) / 8.0).clamp(36.0, 72.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 8.0), Sense::hover());
        let cibles: Vec<usize> = self.choisie.map(|s| coups.iter().filter(|m| m.de == s).map(|m| m.vers).collect()).unwrap_or_default();
        let mut clic = None;
        for v in 0..64 {
            // vue retournée si on joue les noirs
            let s = if self.blancs { v } else { 63 - v };
            let r = Rect::from_min_size(zone.min + Vec2::new((v % 8) as f32 * c, (v / 8) as f32 * c), Vec2::splat(c));
            let clair = (s / 8 + s % 8) % 2 == 0;
            let mut fond = if clair { Color32::from_rgb(240, 217, 230) } else { Color32::from_rgb(160, 100, 140) };
            if self.dernier.is_some_and(|m| m.de == s || m.vers == s) {
                fond = fond.lerp_to_gamma(OR, 0.45);
            }
            if self.choisie == Some(s) {
                fond = fond.lerp_to_gamma(ROSE, 0.6);
            }
            if self.pos.p[s].abs() == 6 && self.pos.p[s].signum() == self.pos.trait_ && self.pos.en_echec(self.pos.trait_) {
                fond = Color32::from_rgb(220, 60, 60);
            }
            ui.painter().rect_filled(r, 0.0, fond);
            let x = self.pos.p[s];
            if x != 0 {
                let (col, cont) = if x > 0 { ([255, 255, 255], [40, 30, 40]) } else { ([30, 25, 35], [230, 220, 230]) };
                self.glyphes.dessiner(ui, r.shrink(c * 0.08), glyphe(x), col, Some(cont));
            }
            if cibles.contains(&s) {
                ui.painter().circle_filled(r.center(), c * 0.14, Color32::from_black_alpha(90));
            }
            if v % 8 == 0 {
                ui.painter().text(r.left_top() + Vec2::new(2.0, 1.0), Align2::LEFT_TOP, (8 - s / 8).to_string(), FontId::proportional(10.0), Color32::from_gray(90));
            }
            if v >= 56 {
                ui.painter().text(r.right_bottom() - Vec2::new(2.0, 1.0), Align2::RIGHT_BOTTOM, ((b'a' + (s % 8) as u8) as char).to_string(), FontId::proportional(10.0), Color32::from_gray(90));
            }
            if ui.interact(r, ui.id().with(("echiquier", v)), Sense::click()).clicked() {
                clic = Some(s);
            }
        }
        if let (Some(s), None, false) = (clic, fin, reflechit) {
            if self.pos.trait_ == moi {
                if cibles.contains(&s) {
                    let de = self.choisie.unwrap();
                    // promotion : dame par défaut
                    let m = coups.iter().find(|m| m.de == de && m.vers == s && (m.promo == 0 || m.promo.abs() == 5)).copied().unwrap();
                    self.historique.push(self.pos);
                    self.pos = self.pos.jouer(m);
                    self.dernier = Some(m);
                    self.choisie = None;
                } else if self.pos.p[s] != 0 && self.pos.p[s].signum() == moi {
                    self.choisie = Some(s);
                } else {
                    self.choisie = None;
                }
            }
        }
        if let Some(m) = self.dernier {
            ui.weak(format!("Dernier coup : {} → {}", nom_case(m.de), nom_case(m.vers)));
        }
    }
}

// ===================================================================================================
// 70 · Solitaire (Klondike)
// ===================================================================================================

/// Carte : (valeur 1..=13, couleur 0 pique 1 cœur 2 carreau 3 trèfle), face visible.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Carte {
    pub v: u8,
    pub c: u8,
    pub visible: bool,
}

impl Carte {
    pub fn rouge(&self) -> bool {
        self.c == 1 || self.c == 2
    }
}

/// Peut-on poser `a` sur `b` dans le tableau ? (couleurs alternées, valeur -1)
pub fn sur_tableau(a: &Carte, b: Option<&Carte>) -> bool {
    match b {
        None => a.v == 13,
        Some(b) => b.visible && b.rouge() != a.rouge() && a.v + 1 == b.v,
    }
}

/// Peut-on poser `a` sur la fondation (même couleur, valeur +1) ?
pub fn sur_fondation(a: &Carte, f: &[Carte]) -> bool {
    match f.last() {
        None => a.v == 1,
        Some(b) => b.c == a.c && a.v == b.v + 1,
    }
}

#[derive(Clone)]
struct Jeu {
    pioche: Vec<Carte>,
    defausse: Vec<Carte>,
    fondations: [Vec<Carte>; 4],
    colonnes: [Vec<Carte>; 7],
    coups: u32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Lieu {
    Defausse,
    Fondation(usize),
    Colonne(usize, usize), // colonne, index de la carte
}

impl Jeu {
    fn nouveau() -> Self {
        let mut paquet: Vec<Carte> = (0..4).flat_map(|c| (1..=13).map(move |v| Carte { v, c, visible: false })).collect();
        melanger(&mut paquet);
        let mut colonnes: [Vec<Carte>; 7] = Default::default();
        for (i, col) in colonnes.iter_mut().enumerate() {
            for k in 0..=i {
                let mut c = paquet.pop().unwrap();
                c.visible = k == i;
                col.push(c);
            }
        }
        Jeu { pioche: paquet, defausse: vec![], fondations: Default::default(), colonnes, coups: 0 }
    }

    fn cartes(&self, l: Lieu) -> Vec<Carte> {
        match l {
            Lieu::Defausse => self.defausse.last().copied().into_iter().collect(),
            Lieu::Fondation(f) => self.fondations[f].last().copied().into_iter().collect(),
            Lieu::Colonne(c, i) => self.colonnes[c].get(i..).map(|s| s.to_vec()).unwrap_or_default(),
        }
    }

    fn retirer(&mut self, l: Lieu) {
        match l {
            Lieu::Defausse => {
                self.defausse.pop();
            }
            Lieu::Fondation(f) => {
                self.fondations[f].pop();
            }
            Lieu::Colonne(c, i) => {
                self.colonnes[c].truncate(i);
                if let Some(d) = self.colonnes[c].last_mut() {
                    d.visible = true;
                }
            }
        }
    }

    /// Déplace vers une colonne ou une fondation (vrai si le coup est valide).
    fn deplacer(&mut self, de: Lieu, vers: Lieu) -> bool {
        let c = self.cartes(de);
        let Some(premiere) = c.first().copied() else { return false };
        if !premiere.visible {
            return false;
        }
        let ok = match vers {
            Lieu::Fondation(f) => c.len() == 1 && sur_fondation(&premiere, &self.fondations[f]),
            Lieu::Colonne(k, _) => sur_tableau(&premiere, self.colonnes[k].last()),
            Lieu::Defausse => false,
        };
        if !ok {
            return false;
        }
        self.retirer(de);
        match vers {
            Lieu::Fondation(f) => self.fondations[f].extend(c),
            Lieu::Colonne(k, _) => self.colonnes[k].extend(c),
            Lieu::Defausse => {}
        }
        self.coups += 1;
        true
    }

    /// Envoie automatiquement la carte en fondation si possible.
    fn auto(&mut self, de: Lieu) -> bool {
        (0..4).any(|f| self.deplacer(de, Lieu::Fondation(f)))
    }

    fn gagne(&self) -> bool {
        self.fondations.iter().all(|f| f.len() == 13)
    }
}

pub struct Solitaire {
    j: Jeu,
    choix: Option<Lieu>,
    annuler: Vec<Jeu>,
    debut: Instant,
    glyphes: Glyphes,
    victoires: u32,
    compte: bool,
}

impl Solitaire {
    pub fn new() -> Self {
        Solitaire { j: Jeu::nouveau(), choix: None, annuler: vec![], debut: Instant::now(), glyphes: Glyphes::default(), victoires: charger::<Option<u32>>("solitaire_victoires").unwrap_or(0), compte: false }
    }

    fn dessiner_carte(&mut self, ui: &egui::Ui, r: Rect, c: Option<&Carte>, choisie: bool) {
        let p = ui.painter();
        match c {
            None => {
                p.rect_stroke(r, 6.0, egui::Stroke::new(1.5_f32, Color32::from_white_alpha(60)), egui::StrokeKind::Inside);
            }
            Some(c) if !c.visible => {
                p.rect_filled(r, 6.0, Color32::from_rgb(150, 60, 120));
                p.rect_stroke(r.shrink(4.0), 4.0, egui::Stroke::new(1.0_f32, Color32::from_rgb(255, 160, 210)), egui::StrokeKind::Inside);
            }
            Some(c) => {
                p.rect_filled(r, 6.0, Color32::from_rgb(250, 248, 245));
                if choisie {
                    p.rect_stroke(r, 6.0, egui::Stroke::new(3.0_f32, ROSE), egui::StrokeKind::Outside);
                }
                let couleur = if c.rouge() { [210, 30, 50] } else { [25, 25, 30] };
                let v = ["A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "V", "D", "R"][c.v as usize - 1];
                p.text(r.left_top() + Vec2::new(5.0, 2.0), Align2::LEFT_TOP, v, FontId::proportional(r.width() * 0.26), Color32::from_rgb(couleur[0], couleur[1], couleur[2]));
                let s = ['♠', '♥', '♦', '♣'][c.c as usize];
                let tl = r.width() * 0.24;
                self.glyphes.dessiner(ui, Rect::from_min_size(r.right_top() + Vec2::new(-tl - 4.0, 4.0), Vec2::splat(tl)), s, couleur, None);
                let tg = r.width() * 0.5;
                self.glyphes.dessiner(ui, Rect::from_center_size(r.center() + Vec2::new(0.0, r.height() * 0.1), Vec2::splat(tg)), s, couleur, None);
            }
        }
    }
}

impl Outil for Solitaire {
    fn nom(&self) -> &'static str {
        "🃏 Solitaire"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Solitaire");
            if ui.button("Nouvelle partie").clicked() {
                *self = Solitaire { victoires: self.victoires, ..Solitaire::new() };
            }
            if ui.add_enabled(!self.annuler.is_empty(), egui::Button::new("↩ Annuler")).clicked() {
                self.j = self.annuler.pop().unwrap();
                self.choix = None;
            }
            ui.weak(format!("{} coups · {} s · 🏆 {}", self.j.coups, self.debut.elapsed().as_secs(), self.victoires));
        });
        ui.weak("Clic sur une carte puis sur sa destination · double-clic : vers les piles du haut");
        if self.j.gagne() {
            ui.label(RichText::new("Bravo, gagné ! 🎉").size(24.0).color(OR));
            if !self.compte {
                self.compte = true;
                self.victoires += 1;
                enregistrer("solitaire_victoires", &Some(self.victoires));
            }
        } else {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        let lc = ((ui.available_width() - 20.0) / 7.0).clamp(60.0, 100.0);
        let (cl, ch, ecart) = (lc - 8.0, (lc - 8.0) * 1.4, (lc - 8.0) * 0.28);
        let hauteur = ch + 20.0 + ch + 18.0 * ecart;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(lc * 7.0, hauteur), Sense::hover());
        ui.painter().rect_filled(zone, 8.0, Color32::from_rgb(30, 70, 50));
        let rect = |col: usize, y: f32| Rect::from_min_size(zone.min + Vec2::new(col as f32 * lc + 4.0, y + 6.0), Vec2::new(cl, ch));
        let mut action: Option<(Lieu, bool)> = None; // lieu cliqué, double-clic
        let mut piocher = false;
        // pioche et défausse
        let rp = rect(0, 0.0);
        let dessus = self.j.pioche.last().copied();
        self.dessiner_carte(ui, rp, dessus.as_ref(), false);
        if ui.interact(rp, ui.id().with("pioche"), Sense::click()).clicked() {
            piocher = true;
        }
        let rd = rect(1, 0.0);
        let d = self.j.defausse.last().copied();
        self.dessiner_carte(ui, rd, d.as_ref(), self.choix == Some(Lieu::Defausse));
        let rep = ui.interact(rd, ui.id().with("defausse"), Sense::click());
        if rep.clicked() || rep.double_clicked() {
            action = Some((Lieu::Defausse, rep.double_clicked()));
        }
        for f in 0..4 {
            let r = rect(3 + f, 0.0);
            let c = self.j.fondations[f].last().copied();
            self.dessiner_carte(ui, r, c.as_ref(), self.choix == Some(Lieu::Fondation(f)));
            if c.is_none() {
                self.glyphes.dessiner(ui, r.shrink(cl * 0.25), ['♠', '♥', '♦', '♣'][f], [120, 160, 130], None);
            }
            if ui.interact(r, ui.id().with(("fond", f)), Sense::click()).clicked() {
                action = Some((Lieu::Fondation(f), false));
            }
        }
        for col in 0..7 {
            let cartes = self.j.colonnes[col].clone();
            let y0 = ch + 20.0;
            if cartes.is_empty() {
                let r = rect(col, y0);
                self.dessiner_carte(ui, r, None, false);
                if ui.interact(r, ui.id().with(("vide", col)), Sense::click()).clicked() {
                    action = Some((Lieu::Colonne(col, 0), false));
                }
            }
            let mut y = y0;
            for (i, c) in cartes.iter().enumerate() {
                let r = rect(col, y);
                y += if c.visible { ecart } else { ecart * 0.5 };
                let choisie = matches!(self.choix, Some(Lieu::Colonne(k, j)) if k == col && i >= j);
                self.dessiner_carte(ui, r, Some(c), choisie);
                // seule la partie visible de la carte reçoit le clic
                let zone_clic = if i + 1 < cartes.len() { Rect::from_min_size(r.min, Vec2::new(r.width(), if c.visible { ecart } else { ecart * 0.5 })) } else { r };
                let rep = ui.interact(zone_clic, ui.id().with(("carte", col, i)), Sense::click());
                if rep.clicked() || rep.double_clicked() {
                    action = Some((Lieu::Colonne(col, i), rep.double_clicked()));
                }
            }
        }
        if piocher {
            self.annuler.push(self.j.clone());
            if let Some(mut c) = self.j.pioche.pop() {
                c.visible = true;
                self.j.defausse.push(c);
            } else {
                self.j.pioche = self.j.defausse.drain(..).rev().map(|mut c| {
                    c.visible = false;
                    c
                }).collect();
            }
            self.choix = None;
        }
        if let Some((l, double)) = action {
            let avant = self.j.clone();
            let mut joue = false;
            if double {
                joue = self.j.auto(l);
                self.choix = None;
            } else if let Some(c) = self.choix {
                let cible = match l {
                    Lieu::Colonne(k, _) => Lieu::Colonne(k, 0),
                    x => x,
                };
                joue = c != l && self.j.deplacer(c, cible);
                self.choix = if joue { None } else { (!self.j.cartes(l).is_empty() && self.j.cartes(l)[0].visible && !matches!(l, Lieu::Fondation(_))).then_some(l) };
            } else if !self.j.cartes(l).is_empty() && self.j.cartes(l)[0].visible {
                self.choix = Some(l);
            }
            if joue {
                self.annuler.push(avant);
            }
        }
    }
}

// ===================================================================================================
// 71 · Mots mêlés
// ===================================================================================================

const THEMES_MOTS: [(&str, &[&str]); 4] = [
    ("Jeux vidéo", &["MANETTE", "CONSOLE", "NIVEAU", "BOSS", "PIXEL", "QUETE", "ARCADE", "AVATAR", "SERVEUR", "CREEPER", "DIAMANT", "PORTAIL", "DRAGON", "EPEE", "BOUCLIER"]),
    ("Japon", &["SAKURA", "TOKYO", "SUSHI", "RAMEN", "MANGA", "KIMONO", "NINJA", "SAMOURAI", "TEMPLE", "BAMBOU", "LANTERNE", "MATCHA", "ORIGAMI", "KARATE", "FUJI"]),
    ("Animaux", &["GIRAFE", "TIGRE", "RENARD", "PANDA", "DAUPHIN", "HIBOU", "LAPIN", "REQUIN", "ZEBRE", "KOALA", "LOUP", "CHAT", "CHEVAL", "TORTUE", "AIGLE"]),
    ("Espace", &["PLANETE", "ETOILE", "COMETE", "GALAXIE", "FUSEE", "LUNE", "SOLEIL", "ORBITE", "MARS", "SATURNE", "ECLIPSE", "NEBULEUSE", "ASTRONAUTE", "METEORE", "ATOME"]),
];

const DIRS: [(i32, i32); 8] = [(0, 1), (1, 0), (1, 1), (-1, 1), (0, -1), (-1, 0), (-1, -1), (1, -1)];

/// Grille n×n avec les mots placés : (grille, mots placés avec (départ, direction)).
pub fn generer_grille(n: usize, mots: &[&str], r: &mut impl FnMut(usize) -> usize) -> (Vec<char>, Vec<(String, usize, (i32, i32))>) {
    let mut g = vec!['.'; n * n];
    let mut places = vec![];
    for m in mots {
        let lettres: Vec<char> = m.chars().collect();
        for _ in 0..200 {
            let d = DIRS[r(8)];
            let (x, y) = (r(n) as i32, r(n) as i32);
            let ok = (0..lettres.len()).all(|k| {
                let (xx, yy) = (x + d.0 * k as i32, y + d.1 * k as i32);
                (0..n as i32).contains(&xx) && (0..n as i32).contains(&yy) && {
                    let c = g[yy as usize * n + xx as usize];
                    c == '.' || c == lettres[k]
                }
            });
            if ok {
                for (k, l) in lettres.iter().enumerate() {
                    g[(y + d.1 * k as i32) as usize * n + (x + d.0 * k as i32) as usize] = *l;
                }
                places.push((m.to_string(), y as usize * n + x as usize, d));
                break;
            }
        }
    }
    for c in g.iter_mut().filter(|c| **c == '.') {
        *c = (b'A' + r(26) as u8) as char;
    }
    (g, places)
}

/// Cases d'une ligne droite de a à b (vide si pas en ligne droite).
pub fn ligne(n: usize, a: usize, b: usize) -> Vec<usize> {
    let (ax, ay, bx, by) = ((a % n) as i32, (a / n) as i32, (b % n) as i32, (b / n) as i32);
    let (dx, dy) = (bx - ax, by - ay);
    if !(dx == 0 || dy == 0 || dx.abs() == dy.abs()) {
        return vec![];
    }
    let k = dx.abs().max(dy.abs());
    (0..=k).map(|i| ((ay + dy.signum() * i) * n as i32 + ax + dx.signum() * i) as usize).collect()
}

pub struct MotsMeles {
    n: usize,
    theme: usize,
    g: Vec<char>,
    mots: Vec<(String, usize, (i32, i32))>,
    trouves: Vec<Vec<usize>>,
    debut_sel: Option<usize>,
    debut: Instant,
    fini: Option<u64>,
}

impl MotsMeles {
    pub fn new() -> Self {
        let mut m = MotsMeles { n: 12, theme: 0, g: vec![], mots: vec![], trouves: vec![], debut_sel: None, debut: Instant::now(), fini: None };
        m.nouvelle();
        m
    }

    fn nouvelle(&mut self) {
        let mut liste: Vec<&str> = THEMES_MOTS[self.theme].1.iter().copied().filter(|m| m.len() <= self.n).collect();
        melanger(&mut liste);
        liste.truncate(if self.n <= 10 { 8 } else { 12 });
        liste.sort_by_key(|m| std::cmp::Reverse(m.len()));
        let mut r = |k: usize| h(k);
        (self.g, self.mots) = generer_grille(self.n, &liste, &mut r);
        self.trouves.clear();
        self.debut = Instant::now();
        self.fini = None;
    }
}

impl Outil for MotsMeles {
    fn nom(&self) -> &'static str {
        "🔤 Mots mêlés"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mots mêlés");
        ui.horizontal_wrapped(|ui| {
            for (k, (t, _)) in THEMES_MOTS.iter().enumerate() {
                if ui.selectable_label(self.theme == k, *t).clicked() {
                    self.theme = k;
                    self.nouvelle();
                }
            }
            ui.separator();
            for (n, t) in [(10, "Petite"), (12, "Moyenne"), (15, "Grande")] {
                if ui.selectable_label(self.n == n, t).clicked() {
                    self.n = n;
                    self.nouvelle();
                }
            }
            if ui.button("🔄 Nouvelle grille").clicked() {
                self.nouvelle();
            }
        });
        let n = self.n;
        let c = ((ui.available_height() - 60.0).min(ui.available_width() - 200.0) / n as f32).clamp(22.0, 40.0);
        ui.horizontal_top(|ui| {
            let (zone, rep) = ui.allocate_exact_size(Vec2::splat(c * n as f32), Sense::click_and_drag());
            let p = ui.painter_at(zone);
            p.rect_filled(zone, 6.0, Color32::from_rgb(30, 22, 40));
            let case_sous = |pos: egui::Pos2| -> Option<usize> {
                let (x, y) = (((pos.x - zone.left()) / c) as i32, ((pos.y - zone.top()) / c) as i32);
                ((0..n as i32).contains(&x) && (0..n as i32).contains(&y)).then(|| y as usize * n + x as usize)
            };
            if rep.drag_started() {
                self.debut_sel = rep.interact_pointer_pos().and_then(case_sous);
            }
            let sel: Vec<usize> = match (self.debut_sel, ui.input(|i| i.pointer.interact_pos()).and_then(case_sous)) {
                (Some(a), Some(b)) if rep.dragged() || rep.drag_stopped() => ligne(n, a, b),
                _ => vec![],
            };
            let couleurs = [ROSE, Color32::from_rgb(120, 170, 255), Color32::from_rgb(110, 220, 130), OR, Color32::from_rgb(190, 120, 255), Color32::from_rgb(255, 160, 80)];
            for (k, t) in self.trouves.iter().enumerate() {
                let (a, b) = (t[0], *t.last().unwrap());
                let ctr = |i: usize| zone.min + Vec2::new((i % n) as f32 * c + c / 2.0, (i / n) as f32 * c + c / 2.0);
                p.line_segment([ctr(a), ctr(b)], egui::Stroke::new(c * 0.75, couleurs[k % couleurs.len()].gamma_multiply(0.55)));
            }
            for i in 0..n * n {
                let r = Rect::from_min_size(zone.min + Vec2::new((i % n) as f32 * c, (i / n) as f32 * c), Vec2::splat(c));
                if sel.contains(&i) {
                    p.rect_filled(r.shrink(2.0), c / 2.0, Color32::from_white_alpha(70));
                }
                p.text(r.center(), Align2::CENTER_CENTER, self.g[i], FontId::monospace(c * 0.6), Color32::WHITE);
            }
            if rep.drag_stopped() && sel.len() > 1 {
                let mot: String = sel.iter().map(|i| self.g[*i]).collect();
                let inverse: String = mot.chars().rev().collect();
                if let Some(m) = self.mots.iter().find(|m| (m.0 == mot || m.0 == inverse) && !self.trouves.iter().any(|t| t.len() == m.0.len() && (t == &sel || t.iter().rev().eq(sel.iter())))) {
                    let _ = m;
                    self.trouves.push(sel.clone());
                    if self.trouves.len() == self.mots.len() {
                        self.fini = Some(self.debut.elapsed().as_secs());
                    }
                }
                self.debut_sel = None;
            }
            ui.vertical(|ui| {
                for (m, ..) in &self.mots {
                    let trouve = self.trouves.iter().any(|t| {
                        let s: String = t.iter().map(|i| self.g[*i]).collect();
                        s == *m || s.chars().rev().collect::<String>() == *m
                    });
                    ui.label(if trouve { RichText::new(m).strikethrough().weak() } else { RichText::new(m).strong() });
                }
                ui.add_space(8.0);
                match self.fini {
                    Some(t) => {
                        ui.label(RichText::new(format!("🎉 Fini en {} min {:02} s", t / 60, t % 60)).color(OR));
                    }
                    None => {
                        ui.weak(format!("{} / {} · {} s", self.trouves.len(), self.mots.len(), self.debut.elapsed().as_secs()));
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                    }
                }
            });
        });
        ui.weak("Fais glisser la souris de la première à la dernière lettre d'un mot (dans tous les sens, même en diagonale).");
    }
}

// ===================================================================================================
// 73 · Vitesse de frappe
// ===================================================================================================

const TEXTES: [&str; 12] = [
    "Le soleil se couchait sur la ville et les lanternes s'allumaient une à une le long de la rivière.",
    "Pour construire une base solide dans Minecraft, il faut d'abord trouver du fer et beaucoup de pierre.",
    "Les cerisiers en fleurs attirent chaque printemps des milliers de visiteurs dans les parcs de Tokyo.",
    "Un bon clavier mécanique rend la frappe plus agréable, mais c'est surtout l'entraînement qui compte.",
    "La fusée décolla dans un grondement immense et disparut en quelques secondes au-dessus des nuages.",
    "Il faut sauvegarder souvent son travail, car un ordinateur peut toujours planter au pire moment.",
    "Le renard roux traversa la forêt enneigée sans faire le moindre bruit, à la recherche de nourriture.",
    "Dans ce jeu, chaque niveau cache un trésor secret que seuls les joueurs les plus curieux découvrent.",
    "Le chat dormait sur le radiateur pendant que la pluie tapait doucement contre les vitres du salon.",
    "Apprendre à coder demande de la patience, mais voir son premier programme fonctionner est magique.",
    "Les vagues venaient s'écraser sur les rochers et l'odeur du sel flottait dans l'air chaud de l'été.",
    "Elle prit son vélo, mit son casque et partit faire le tour du lac avant que le soleil ne se lève.",
];

/// (mots par minute, précision %) : un « mot » = 5 caractères tapés correctement.
pub fn score_frappe(modele: &str, tape: &str, secondes: f64) -> (f64, f64) {
    let (m, t): (Vec<char>, Vec<char>) = (modele.chars().collect(), tape.chars().collect());
    let justes = t.iter().zip(&m).filter(|(a, b)| a == b).count();
    let ppm = if secondes > 0.0 { justes as f64 / 5.0 / (secondes / 60.0) } else { 0.0 };
    let precision = if t.is_empty() { 100.0 } else { justes as f64 * 100.0 / t.len() as f64 };
    (ppm, precision)
}

pub struct Frappe {
    texte: String,
    tape: String,
    debut: Option<Instant>,
    fini: Option<(f64, f64)>,
    record: f64,
    historique: Vec<f64>,
}

impl Frappe {
    pub fn new() -> Self {
        let (record, historique): (f64, Vec<f64>) = charger::<Option<(f64, Vec<f64>)>>("frappe").unwrap_or((0.0, vec![]));
        Frappe { texte: TEXTES[h(TEXTES.len())].into(), tape: String::new(), debut: None, fini: None, record, historique }
    }
}

impl Outil for Frappe {
    fn nom(&self) -> &'static str {
        "⌨ Vitesse de frappe"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Vitesse de frappe");
        ui.horizontal(|ui| {
            if ui.button("🔄 Autre texte").clicked() {
                *self = Frappe { record: self.record, historique: self.historique.clone(), ..Frappe::new() };
            }
            if self.record > 0.0 {
                ui.label(format!("🏆 Record : {:.0} mots/min", self.record));
            }
            if !self.historique.is_empty() {
                let m = self.historique.iter().sum::<f64>() / self.historique.len() as f64;
                ui.weak(format!("moyenne {m:.0} sur {} essais", self.historique.len()));
            }
        });
        // texte à recopier, coloré selon la frappe
        let modele: Vec<char> = self.texte.chars().collect();
        let tape: Vec<char> = self.tape.chars().collect();
        let mut job = egui::text::LayoutJob::default();
        for (k, c) in modele.iter().enumerate() {
            let couleur = match tape.get(k) {
                Some(t) if t == c => Color32::LIGHT_GREEN,
                Some(_) => Color32::LIGHT_RED,
                None if k == tape.len() => ROSE,
                None => Color32::GRAY,
            };
            let mut f = egui::TextFormat { font_id: FontId::proportional(22.0), color: couleur, ..Default::default() };
            if k == tape.len() {
                f.underline = egui::Stroke::new(2.0_f32, ROSE);
            }
            job.append(&c.to_string(), 0.0, f);
        }
        job.wrap.max_width = ui.available_width();
        ui.label(job);
        ui.add_space(8.0);
        if let Some((ppm, prec)) = self.fini {
            ui.label(RichText::new(format!("{ppm:.0} mots par minute · {prec:.0} % de précision")).size(26.0).color(OR));
            ui.label(match ppm as u32 {
                0..=20 => "Bon début ! L'important est de taper sans regarder le clavier.",
                21..=35 => "Bien ! C'est la vitesse moyenne.",
                36..=55 => "Très bien, plus rapide que la plupart des gens !",
                56..=80 => "Excellent, vitesse de pro !",
                _ => "Incroyable 🔥",
            });
            if ui.button("Recommencer").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *self = Frappe { record: self.record, historique: self.historique.clone(), ..Frappe::new() };
            }
            return;
        }
        let r = ui.add(egui::TextEdit::singleline(&mut self.tape).desired_width(f32::INFINITY).font(FontId::proportional(20.0)).hint_text("tape le texte ici…"));
        r.request_focus();
        if r.changed() && self.debut.is_none() && !self.tape.is_empty() {
            self.debut = Some(Instant::now());
        }
        let s = self.debut.map(|d| d.elapsed().as_secs_f64()).unwrap_or(0.0);
        let (ppm, prec) = score_frappe(&self.texte, &self.tape, s);
        ui.horizontal(|ui| {
            ui.label(format!("⏱ {s:.0} s"));
            ui.label(format!("{ppm:.0} mots/min"));
            ui.label(format!("{prec:.0} % juste"));
        });
        if self.debut.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if self.tape.chars().count() >= modele.len() {
            self.fini = Some((ppm, prec));
            if prec >= 85.0 {
                self.historique.push(ppm);
                if self.historique.len() > 50 {
                    self.historique.remove(0);
                }
                if ppm > self.record {
                    self.record = ppm;
                }
                enregistrer("frappe", &Some((self.record, self.historique.clone())));
            }
        }
    }
}

// ===================================================================================================
// 68 · Clicker
// ===================================================================================================

/// (nom, emoji, coût de base, dangos par seconde)
const AMELIORATIONS: [(&str, &str, f64, f64); 8] = [
    ("Apprenti", "🧑‍🍳", 15.0, 0.1),
    ("Étal de rue", "🏮", 100.0, 1.0),
    ("Cuisine", "🍳", 1_100.0, 8.0),
    ("Boutique", "🏪", 12_000.0, 47.0),
    ("Usine", "🏭", 130_000.0, 260.0),
    ("Temple du dango", "⛩", 1_400_000.0, 1_400.0),
    ("Fusée à dangos", "🚀", 20_000_000.0, 7_800.0),
    ("Dimension sucrée", "🌌", 330_000_000.0, 44_000.0),
];

pub fn cout(base: f64, possedes: u32) -> f64 {
    (base * 1.15f64.powi(possedes as i32)).ceil()
}

pub fn grand_nombre(x: f64) -> String {
    let unites = ["", " mille", " million", " milliard", " billion", " billiard", " trillion"];
    if x < 10_000.0 {
        return format!("{}", x.floor());
    }
    let k = ((x.log10() / 3.0).floor() as usize).min(unites.len() - 1);
    format!("{:.2}{}", x / 1000f64.powi(k as i32), unites[k]).replace('.', ",")
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct EtatClicker {
    dangos: f64,
    total: f64,
    clics: u64,
    possedes: [u32; 8],
    puissance_clic: u32,
    sauve: u64,
}

pub struct Clicker {
    e: EtatClicker,
    tic: Instant,
    sauve: Instant,
    bulles: Vec<(egui::Pos2, Instant, String)>,
    hors_ligne: f64,
}

fn secondes() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Clicker {
    pub fn new() -> Self {
        let mut e: EtatClicker = charger("clicker");
        // production pendant l'absence (moitié, 8 h max)
        let absent = secondes().saturating_sub(e.sauve).min(8 * 3600) as f64;
        let hors_ligne = if e.sauve > 0 { Self::par_seconde_de(&e) * absent * 0.5 } else { 0.0 };
        e.dangos += hors_ligne;
        e.total += hors_ligne;
        Clicker { e, tic: Instant::now(), sauve: Instant::now(), bulles: vec![], hors_ligne }
    }

    fn par_seconde_de(e: &EtatClicker) -> f64 {
        AMELIORATIONS.iter().zip(e.possedes.iter()).map(|(a, n)| a.3 * *n as f64).sum()
    }

    fn cout_clic(&self) -> f64 {
        cout(50.0, self.e.puissance_clic) * 2.0
    }
}

impl Outil for Clicker {
    fn nom(&self) -> &'static str {
        "🍡 Dango clicker"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn tic(&mut self, _: &egui::Context) {
        let dt = self.tic.elapsed().as_secs_f64();
        self.tic = Instant::now();
        let p = Self::par_seconde_de(&self.e) * dt;
        self.e.dangos += p;
        self.e.total += p;
        if self.sauve.elapsed() > Duration::from_secs(20) {
            self.sauve = Instant::now();
            self.e.sauve = secondes();
            enregistrer("clicker", &self.e);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dango clicker");
        if self.hors_ligne >= 1.0 {
            ui.label(format!("🌙 Pendant ton absence, tes boutiques ont fait {} dangos.", grand_nombre(self.hors_ligne)));
        }
        let ps = Self::par_seconde_de(&self.e);
        ui.label(RichText::new(format!("🍡 {}", grand_nombre(self.e.dangos))).size(34.0).strong());
        ui.weak(format!("{} par seconde · {} par clic · {} clics", grand_nombre(ps), 1 + self.e.puissance_clic, self.e.clics));
        ui.horizontal_top(|ui| {
            let (r, rep) = ui.allocate_exact_size(Vec2::splat(220.0), Sense::click());
            let presse = rep.is_pointer_button_down_on();
            let taille = if presse { 150.0 } else if rep.hovered() { 170.0 } else { 160.0 };
            ui.painter().circle_filled(r.center(), taille / 2.0 + 8.0, Color32::from_rgb(60, 30, 50));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, "🍡", FontId::proportional(taille * 0.75), Color32::WHITE);
            if rep.clicked() {
                let n = 1.0 + self.e.puissance_clic as f64;
                self.e.dangos += n;
                self.e.total += n;
                self.e.clics += 1;
                let pos = rep.interact_pointer_pos().unwrap_or(r.center());
                self.bulles.push((pos, Instant::now(), format!("+{}", grand_nombre(n))));
            }
            self.bulles.retain(|b| b.1.elapsed() < Duration::from_millis(900));
            for (p, t, s) in &self.bulles {
                let a = t.elapsed().as_secs_f32();
                ui.painter().text(*p - Vec2::new(0.0, a * 60.0), Align2::CENTER_CENTER, s, FontId::proportional(18.0), Color32::from_white_alpha((255.0 * (1.0 - a)) as u8));
            }
            ui.vertical(|ui| {
                let c = self.cout_clic();
                if ui.add_enabled(self.e.dangos >= c, egui::Button::new(format!("👆 Clic plus fort ({})", grand_nombre(c)))).clicked() {
                    self.e.dangos -= c;
                    self.e.puissance_clic += 1;
                }
                for (k, (n, emoji, base, prod)) in AMELIORATIONS.iter().enumerate() {
                    let deja = self.e.possedes[k];
                    let prix = cout(*base, deja);
                    // une amélioration n'apparaît qu'une fois la précédente achetée (ou presque abordable)
                    if k > 0 && self.e.possedes[k - 1] == 0 && self.e.total < base * 0.5 {
                        ui.weak("❓ ???");
                        break;
                    }
                    let b = egui::Button::new(format!("{emoji} {n} ×{deja} — {} 🍡 (+{}/s)", grand_nombre(prix), grand_nombre(*prod))).min_size(Vec2::new(340.0, 28.0));
                    if ui.add_enabled(self.e.dangos >= prix, b).clicked() {
                        self.e.dangos -= prix;
                        self.e.possedes[k] += 1;
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.weak(format!("Total fabriqué : {}", grand_nombre(self.e.total)));
            if ui.small_button("Recommencer à zéro").clicked() && rfd::MessageDialog::new().set_description("Tout recommencer ?").set_buttons(rfd::MessageButtons::YesNo).show() == rfd::MessageDialogResult::Yes {
                self.e = EtatClicker::default();
                self.hors_ligne = 0.0;
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    }
}

// ===================================================================================================
// 74 · Jeux à deux sur le même PC
// ===================================================================================================

/// 0 pierre, 1 feuille, 2 ciseaux → 0 égalité, 1 joueur 1 gagne, 2 joueur 2 gagne.
pub fn pfc(a: u8, b: u8) -> u8 {
    if a == b {
        0
    } else if (a + 1) % 3 == b {
        2
    } else {
        1
    }
}

#[derive(Default)]
struct Duel {
    depart: Option<Instant>,
    signal: Option<Instant>,
    resultat: String,
}

pub struct ADeux {
    jeu: u8,
    noms: (String, String),
    score: (u32, u32),
    choix: (Option<u8>, Option<u8>),
    resultat: String,
    duel: Duel,
    m3: [u8; 9],
    p4: Plateau4,
    tour: u8,
}

impl ADeux {
    pub fn new() -> Self {
        ADeux { jeu: 0, noms: ("Joueur 1".into(), "Joueur 2".into()), score: (0, 0), choix: (None, None), resultat: String::new(), duel: Duel::default(), m3: [0; 9], p4: [[0; C4L]; C4H], tour: 1 }
    }
}

const PFC: [&str; 3] = ["✊ Pierre", "✋ Feuille", "✌ Ciseaux"];

impl Outil for ADeux {
    fn nom(&self) -> &'static str {
        "👥 Jeux à deux (même PC)"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jeux à deux sur le même PC");
        ui.horizontal(|ui| {
            for (k, n) in ["✊ Pierre-feuille-ciseaux", "⚡ Duel de réflexes", "❌ Morpion", "🔴 4 en ligne"].iter().enumerate() {
                if ui.selectable_label(self.jeu == k as u8, *n).clicked() {
                    self.jeu = k as u8;
                    self.score = (0, 0);
                    self.resultat.clear();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.noms.0).desired_width(110.0));
            ui.label(RichText::new(format!("{}  —  {}", self.score.0, self.score.1)).size(24.0).strong());
            ui.add(egui::TextEdit::singleline(&mut self.noms.1).desired_width(110.0));
            if ui.small_button("Remettre à zéro").clicked() {
                self.score = (0, 0);
            }
        });
        ui.separator();
        let (n1, n2) = self.noms.clone();
        match self.jeu {
            0 => {
                ui.label(format!("{n1} : touches A (pierre), Z (feuille), E (ciseaux) · {n2} : touches I, O, P. Les choix restent cachés jusqu'à ce que les deux aient joué."));
                ui.input(|i| {
                    for (k, t) in [Key::A, Key::Z, Key::E].iter().enumerate() {
                        if i.key_pressed(*t) && self.choix.0.is_none() {
                            self.choix.0 = Some(k as u8);
                        }
                    }
                    for (k, t) in [Key::I, Key::O, Key::P].iter().enumerate() {
                        if i.key_pressed(*t) && self.choix.1.is_none() {
                            self.choix.1 = Some(k as u8);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if self.choix.0.is_some() { format!("{n1} : ✔ prêt") } else { format!("{n1} : …") }).size(20.0));
                    ui.add_space(40.0);
                    ui.label(RichText::new(if self.choix.1.is_some() { format!("{n2} : ✔ prêt") } else { format!("{n2} : …") }).size(20.0));
                });
                if let (Some(a), Some(b)) = self.choix {
                    let r = pfc(a, b);
                    match r {
                        1 => self.score.0 += 1,
                        2 => self.score.1 += 1,
                        _ => {}
                    }
                    self.resultat = format!("{} contre {} → {}", PFC[a as usize], PFC[b as usize], match r {
                        0 => "égalité".to_string(),
                        1 => format!("{n1} gagne !"),
                        _ => format!("{n2} gagne !"),
                    });
                    self.choix = (None, None);
                }
                ui.label(RichText::new(&self.resultat).size(22.0));
            }
            1 => {
                ui.label(format!("{n1} appuie sur Q, {n2} sur M, dès que l'écran devient vert. Trop tôt = point perdu !"));
                let d = &mut self.duel;
                match (d.depart, d.signal) {
                    (None, _) => {
                        if ui.button(RichText::new("▶ Prêts ?").size(20.0)).clicked() {
                            d.depart = Some(Instant::now() + Duration::from_millis(1500 + h(3500) as u64));
                            d.signal = None;
                            d.resultat.clear();
                        }
                    }
                    (Some(t), None) => {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 200.0), Sense::hover());
                        if Instant::now() >= t {
                            d.signal = Some(Instant::now());
                        }
                        ui.painter().rect_filled(r, 10.0, Color32::from_rgb(160, 40, 60));
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, "Attendez…", FontId::proportional(36.0), Color32::WHITE);
                        let (q, m) = ui.input(|i| (i.key_pressed(Key::Q), i.key_pressed(Key::M)));
                        if q || m {
                            let (perdant, gagnant) = if q { (&n1, &n2) } else { (&n2, &n1) };
                            if q {
                                self.score.1 += 1;
                            } else {
                                self.score.0 += 1;
                            }
                            d.resultat = format!("{perdant} a appuyé trop tôt ! Point pour {gagnant}.");
                            d.depart = None;
                        }
                        ui.ctx().request_repaint();
                    }
                    (Some(_), Some(s)) => {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 200.0), Sense::hover());
                        ui.painter().rect_filled(r, 10.0, Color32::from_rgb(40, 180, 90));
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, "MAINTENANT !", FontId::proportional(44.0), Color32::WHITE);
                        let (q, m) = ui.input(|i| (i.key_pressed(Key::Q), i.key_pressed(Key::M)));
                        if q || m {
                            let ms = s.elapsed().as_millis();
                            let g = if q { &n1 } else { &n2 };
                            if q {
                                self.score.0 += 1;
                            } else {
                                self.score.1 += 1;
                            }
                            d.resultat = format!("{g} gagne en {ms} ms !");
                            d.depart = None;
                        }
                        ui.ctx().request_repaint();
                    }
                }
                ui.label(RichText::new(&self.duel.resultat).size(22.0));
            }
            2 => {
                let fin = gagnant3(&self.m3).or(self.m3.iter().all(|c| *c != 0).then_some(0));
                ui.label(match fin {
                    Some(1) => format!("{n1} gagne ! 🎉"),
                    Some(2) => format!("{n2} gagne ! 🎉"),
                    Some(_) => "Match nul".into(),
                    None => format!("À {} ({})", if self.tour == 1 { &n1 } else { &n2 }, if self.tour == 1 { "✖" } else { "◯" }),
                });
                let c = 100.0;
                let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 3.0), Sense::hover());
                for i in 0..9 {
                    let r = Rect::from_min_size(zone.min + Vec2::new((i % 3) as f32 * c, (i / 3) as f32 * c), Vec2::splat(c)).shrink(4.0);
                    let rep = ui.interact(r, ui.id().with(("m3d", i)), Sense::click());
                    ui.painter().rect_filled(r, 8.0, Color32::from_rgb(45, 35, 55));
                    let t = match self.m3[i] {
                        1 => ("✖", ROSE),
                        2 => ("◯", OR),
                        _ => ("", Color32::WHITE),
                    };
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, t.0, FontId::proportional(60.0), t.1);
                    if rep.clicked() && self.m3[i] == 0 && fin.is_none() {
                        self.m3[i] = self.tour;
                        if let Some(g) = gagnant3(&self.m3) {
                            if g == 1 {
                                self.score.0 += 1;
                            } else {
                                self.score.1 += 1;
                            }
                        }
                        self.tour = 3 - self.tour;
                    }
                }
                if fin.is_some() && ui.button("Rejouer").clicked() {
                    self.m3 = [0; 9];
                }
            }
            _ => {
                let fin = gagnant4(&self.p4).or(self.p4[0].iter().all(|x| *x != 0).then_some(0));
                ui.label(match fin {
                    Some(1) => format!("{n1} gagne ! 🎉"),
                    Some(2) => format!("{n2} gagne ! 🎉"),
                    Some(_) => "Match nul".into(),
                    None => format!("À {} ({})", if self.tour == 1 { &n1 } else { &n2 }, if self.tour == 1 { "rose" } else { "jaune" }),
                });
                let c = 58.0;
                let (zone, rep) = ui.allocate_exact_size(Vec2::new(c * C4L as f32, c * C4H as f32), Sense::click());
                ui.painter().rect_filled(zone, 10.0, Color32::from_rgb(50, 40, 110));
                for l in 0..C4H {
                    for col in 0..C4L {
                        let couleur = match self.p4[l][col] {
                            1 => ROSE,
                            2 => OR,
                            _ => Color32::from_rgb(20, 14, 28),
                        };
                        ui.painter().circle_filled(zone.min + Vec2::new((col as f32 + 0.5) * c, (l as f32 + 0.5) * c), c * 0.4, couleur);
                    }
                }
                if rep.clicked() && fin.is_none() {
                    if let Some(col) = rep.interact_pointer_pos().map(|p| ((p.x - zone.left()) / c) as usize).filter(|c| *c < C4L) {
                        if jouer4(&mut self.p4, col, self.tour).is_some() {
                            if let Some(g) = gagnant4(&self.p4) {
                                if g == 1 {
                                    self.score.0 += 1;
                                } else {
                                    self.score.1 += 1;
                                }
                            }
                            self.tour = 3 - self.tour;
                        }
                    }
                }
                if fin.is_some() && ui.button("Rejouer").clicked() {
                    self.p4 = [[0; C4L]; C4H];
                }
            }
        }
    }
}

// ===================================================================================================
// 65 · Jeux à deux en réseau local
// ===================================================================================================

const PORT_JEU: u16 = 47_320;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum JeuReseau {
    Morpion,
    Quatre,
}

/// Messages échangés (une ligne de texte chacun).
pub fn message_coup(jeu: JeuReseau, case: usize) -> String {
    format!("{} {case}\n", if jeu == JeuReseau::Morpion { "M" } else { "Q" })
}

pub fn lire_message(l: &str) -> Option<(char, usize)> {
    let mut p = l.split_whitespace();
    let c = p.next()?.chars().next()?;
    Some((c, p.next()?.parse().ok()?))
}

#[derive(Default)]
struct Partie {
    connecte: bool,
    nom_autre: String,
    recus: Vec<String>,
    erreur: String,
}

pub struct Reseau {
    jeu: JeuReseau,
    hote: bool,
    ip: String,
    partie: Arc<Mutex<Partie>>,
    sortie: Option<Arc<Mutex<std::net::TcpStream>>>,
    attente: Arc<AtomicBool>,
    m3: [u8; 9],
    p4: Plateau4,
    mon_tour: bool,
    score: (u32, u32),
    en_attente: Option<Arc<Mutex<Option<std::net::TcpStream>>>>,
    compte_perdu: bool,
}

impl Reseau {
    pub fn new() -> Self {
        Reseau { jeu: JeuReseau::Quatre, hote: true, ip: String::new(), partie: Arc::default(), sortie: None, attente: Arc::default(), m3: [0; 9], p4: [[0; C4L]; C4H], mon_tour: true, score: (0, 0), en_attente: None, compte_perdu: false }
    }

    fn brancher(&mut self, s: std::net::TcpStream) {
        let lecture = s.try_clone();
        self.sortie = Some(Arc::new(Mutex::new(s)));
        let p = self.partie.clone();
        if let Ok(l) = lecture {
            std::thread::spawn(move || {
                for ligne in std::io::BufReader::new(l).lines() {
                    match ligne {
                        Ok(x) => p.lock().unwrap().recus.push(x),
                        Err(_) => break,
                    }
                }
                let mut x = p.lock().unwrap();
                x.connecte = false;
                x.erreur = "L'autre joueur est parti.".into();
            });
        }
    }

    fn envoyer(&self, m: &str) {
        if let Some(s) = &self.sortie {
            let _ = s.lock().unwrap().write_all(m.as_bytes());
        }
    }

    fn reinitialiser(&mut self) {
        self.m3 = [0; 9];
        self.p4 = [[0; C4L]; C4H];
        self.mon_tour = self.hote;
    }
}

impl Outil for Reseau {
    fn nom(&self) -> &'static str {
        "🌐 Jeux en réseau local"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Jouer à deux sur deux PC de la maison");
        let connecte = self.partie.lock().unwrap().connecte;
        if !connecte {
            ui.add_enabled_ui(!self.attente.load(Relaxed), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.jeu, JeuReseau::Quatre, "🔴 4 en ligne");
                    ui.selectable_value(&mut self.jeu, JeuReseau::Morpion, "❌ Morpion");
                });
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("🏠 Créer une partie").size(16.0)).clicked() {
                        self.hote = true;
                        self.attente.store(true, Relaxed);
                        let (p, a, jeu) = (self.partie.clone(), self.attente.clone(), self.jeu);
                        let flux: Arc<Mutex<Option<std::net::TcpStream>>> = Arc::default();
                        let f2 = flux.clone();
                        std::thread::spawn(move || {
                            let r = std::net::TcpListener::bind(("0.0.0.0", PORT_JEU)).and_then(|l| l.accept());
                            match r {
                                Ok((mut s, ad)) => {
                                    let _ = s.write_all(format!("JEU {}\n", if jeu == JeuReseau::Morpion { 3 } else { 4 }).as_bytes());
                                    let mut x = p.lock().unwrap();
                                    x.connecte = true;
                                    x.nom_autre = ad.ip().to_string();
                                    *f2.lock().unwrap() = Some(s);
                                }
                                Err(e) => p.lock().unwrap().erreur = format!("impossible d'attendre un joueur : {e}"),
                            }
                            a.store(false, Relaxed);
                        });
                        self.en_attente = Some(flux);
                    }
                    ui.weak(format!("Ton adresse : {}", crate::telephone::ip_locale().unwrap_or_default()));
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.ip).hint_text("adresse de l'autre PC (ex. 192.168.1.20)").desired_width(220.0));
                    if ui.button("🔗 Rejoindre").clicked() && !self.ip.trim().is_empty() {
                        self.hote = false;
                        match format!("{}:{PORT_JEU}", self.ip.trim()).parse().map_err(|_| "adresse invalide".to_string()).and_then(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(5)).map_err(|e| e.to_string())) {
                            Ok(s) => {
                                {
                                    let mut x = self.partie.lock().unwrap();
                                    x.connecte = true;
                                    x.nom_autre = self.ip.trim().to_string();
                                    x.erreur.clear();
                                }
                                self.brancher(s);
                                self.reinitialiser();
                            }
                            Err(e) => self.partie.lock().unwrap().erreur = format!("connexion impossible : {e}"),
                        }
                    }
                });
            });
            if self.attente.load(Relaxed) {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("En attente de l'autre joueur… (sur l'autre PC : Rejoindre avec ton adresse)");
                });
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            // l'hôte vient d'accepter une connexion
            if let Some(f) = self.en_attente.clone() {
                if let Some(s) = f.lock().unwrap().take() {
                    self.brancher(s);
                    self.en_attente = None;
                    self.reinitialiser();
                }
            }
            let e = self.partie.lock().unwrap().erreur.clone();
            if !e.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            ui.weak("Les deux PC doivent avoir Outils ouvert, sur le même réseau. Windows peut demander d'autoriser Outils dans le pare-feu (réseaux privés).");
            return;
        }
        // messages reçus
        let recus: Vec<String> = std::mem::take(&mut self.partie.lock().unwrap().recus);
        for m in recus {
            if let Some(j) = m.strip_prefix("JEU ") {
                self.jeu = if j.trim() == "3" { JeuReseau::Morpion } else { JeuReseau::Quatre };
                self.reinitialiser();
            } else if m.trim() == "NOUVEAU" {
                self.reinitialiser();
            } else if let Some((t, k)) = lire_message(&m) {
                let autre = if self.hote { 2 } else { 1 };
                match t {
                    'M' if k < 9 && self.m3[k] == 0 => {
                        self.m3[k] = autre;
                        self.mon_tour = true;
                    }
                    'Q' if k < C4L => {
                        if jouer4(&mut self.p4, k, autre).is_some() {
                            self.mon_tour = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        let moi = if self.hote { 1u8 } else { 2 };
        let nom = self.partie.lock().unwrap().nom_autre.clone();
        let fin = match self.jeu {
            JeuReseau::Morpion => gagnant3(&self.m3).or(self.m3.iter().all(|c| *c != 0).then_some(0)),
            JeuReseau::Quatre => gagnant4(&self.p4).or(self.p4[0].iter().all(|x| *x != 0).then_some(0)),
        };
        ui.horizontal(|ui| {
            ui.label(format!("🔗 Connecté à {nom}"));
            ui.label(RichText::new(format!("Moi {} — {} Lui", self.score.0, self.score.1)).strong());
            if ui.small_button("Quitter").clicked() {
                if let Some(s) = self.sortie.take() {
                    let _ = s.lock().unwrap().shutdown(std::net::Shutdown::Both);
                }
                self.partie.lock().unwrap().connecte = false;
            }
        });
        ui.label(RichText::new(match fin {
            Some(g) if g == moi => "Tu as gagné ! 🎉".to_string(),
            Some(0) => "Match nul".to_string(),
            Some(_) => "L'autre a gagné…".to_string(),
            None if self.mon_tour => format!("À toi ({})", if moi == 1 { "rose" } else { "jaune" }),
            None => "Au tour de l'autre…".to_string(),
        }).size(18.0));
        let mut joue: Option<usize> = None;
        match self.jeu {
            JeuReseau::Morpion => {
                let c = 100.0;
                let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * 3.0), Sense::hover());
                for i in 0..9 {
                    let r = Rect::from_min_size(zone.min + Vec2::new((i % 3) as f32 * c, (i / 3) as f32 * c), Vec2::splat(c)).shrink(4.0);
                    let rep = ui.interact(r, ui.id().with(("m3r", i)), Sense::click());
                    ui.painter().rect_filled(r, 8.0, Color32::from_rgb(45, 35, 55));
                    let t = match self.m3[i] {
                        1 => ("✖", ROSE),
                        2 => ("◯", OR),
                        _ => ("", Color32::WHITE),
                    };
                    ui.painter().text(r.center(), Align2::CENTER_CENTER, t.0, FontId::proportional(60.0), t.1);
                    if rep.clicked() && self.mon_tour && fin.is_none() && self.m3[i] == 0 {
                        self.m3[i] = moi;
                        joue = Some(i);
                    }
                }
            }
            JeuReseau::Quatre => {
                let c = 58.0;
                let (zone, rep) = ui.allocate_exact_size(Vec2::new(c * C4L as f32, c * C4H as f32), Sense::click());
                ui.painter().rect_filled(zone, 10.0, Color32::from_rgb(50, 40, 110));
                for l in 0..C4H {
                    for col in 0..C4L {
                        let couleur = match self.p4[l][col] {
                            1 => ROSE,
                            2 => OR,
                            _ => Color32::from_rgb(20, 14, 28),
                        };
                        ui.painter().circle_filled(zone.min + Vec2::new((col as f32 + 0.5) * c, (l as f32 + 0.5) * c), c * 0.4, couleur);
                    }
                }
                if rep.clicked() && self.mon_tour && fin.is_none() {
                    if let Some(col) = rep.interact_pointer_pos().map(|p| ((p.x - zone.left()) / c) as usize).filter(|c| *c < C4L) {
                        if jouer4(&mut self.p4, col, moi).is_some() {
                            joue = Some(col);
                        }
                    }
                }
            }
        }
        if let Some(k) = joue {
            self.envoyer(&message_coup(self.jeu, k));
            self.mon_tour = false;
            let fin2 = match self.jeu {
                JeuReseau::Morpion => gagnant3(&self.m3),
                JeuReseau::Quatre => gagnant4(&self.p4),
            };
            if fin2 == Some(moi) {
                self.score.0 += 1;
            }
        }
        if let Some(g) = fin {
            if g != 0 && g != moi && !self.compte_perdu {
                self.score.1 += 1;
                self.compte_perdu = true;
            }
            if ui.button("🔄 Nouvelle manche").clicked() {
                self.envoyer("NOUVEAU\n");
                self.reinitialiser();
            }
        } else {
            self.compte_perdu = false;
        }
        ui.ctx().request_repaint_after(Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn perft(p: &Position, d: u32) -> u64 {
        if d == 0 {
            return 1;
        }
        p.coups().iter().map(|m| perft(&p.jouer(*m), d - 1)).sum()
    }

    #[test]
    fn echecs_regles() {
        let p = Position::default();
        assert_eq!(perft(&p, 1), 20);
        assert_eq!(perft(&p, 2), 400);
        assert_eq!(perft(&p, 3), 8902);
        // mat du berger : e4 e5 Fc4 Cc6 Dh5 Cf6 Dxf7#
        let s = |n: &str| (8 - (n.as_bytes()[1] - b'0') as usize) * 8 + (n.as_bytes()[0] - b'a') as usize;
        let mut q = p;
        for (a, b) in [("e2", "e4"), ("e7", "e5"), ("f1", "c4"), ("b8", "c6"), ("d1", "h5"), ("g8", "f6"), ("h5", "f7")] {
            let m = q.coups().into_iter().find(|m| m.de == s(a) && m.vers == s(b)).expect(a);
            q = q.jouer(m);
        }
        assert!(q.coups().is_empty() && q.en_echec(-1));
        // l'ordi prend une dame offerte
        let mut r = Position { p: [0; 64], trait_: -1, roques: [false; 4], ep: None };
        r.p[s("e8")] = -6;
        r.p[s("e1")] = 6;
        r.p[s("d8")] = -4;
        r.p[s("d4")] = 5;
        let m = coup_ordi(&r, 2).unwrap();
        assert_eq!((m.de, m.vers), (s("d8"), s("d4")));
    }

    #[test]
    fn cartes_et_mots() {
        let as_pique = Carte { v: 1, c: 0, visible: true };
        assert!(sur_fondation(&as_pique, &[]));
        assert!(!sur_fondation(&Carte { v: 2, c: 0, visible: true }, &[]));
        let roi = Carte { v: 13, c: 1, visible: true };
        assert!(sur_tableau(&roi, None));
        assert!(sur_tableau(&Carte { v: 12, c: 0, visible: true }, Some(&roi)));
        assert!(!sur_tableau(&Carte { v: 12, c: 2, visible: true }, Some(&roi)));
        let mut j = Jeu::nouveau();
        assert_eq!(j.pioche.len(), 24);
        assert_eq!(j.colonnes.iter().map(|c| c.len()).sum::<usize>(), 28);
        assert!(j.colonnes.iter().all(|c| c.last().unwrap().visible));
        j.auto(Lieu::Defausse);
        let mut k = 0usize;
        let mut r = |n: usize| {
            k = k.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (k >> 8) % n
        };
        let (g, places) = generer_grille(12, &["SAKURA", "TOKYO", "NINJA"], &mut r);
        assert_eq!(places.len(), 3);
        for (m, d, dir) in &places {
            let s: String = (0..m.len()).map(|i| g[((d / 12) as i32 + dir.1 * i as i32) as usize * 12 + ((d % 12) as i32 + dir.0 * i as i32) as usize]).collect();
            assert_eq!(&s, m);
        }
        assert_eq!(ligne(5, 0, 24), vec![0, 6, 12, 18, 24]);
        assert_eq!(ligne(5, 4, 0), vec![4, 3, 2, 1, 0]);
        assert!(ligne(5, 0, 7).is_empty());
    }

    #[test]
    fn divers() {
        let (ppm, p) = score_frappe("bonjour le monde", "bonjour le monde", 6.0);
        assert!((ppm - 32.0).abs() < 1e-9 && p == 100.0);
        assert_eq!(score_frappe("abc", "axc", 60.0).1.round(), 67.0);
        assert_eq!(cout(15.0, 0), 15.0);
        assert_eq!(cout(15.0, 1), 18.0);
        assert_eq!(grand_nombre(1234.0), "1234");
        assert_eq!(grand_nombre(1_500_000.0), "1,50 million");
        assert_eq!(pfc(0, 2), 1);
        assert_eq!(pfc(0, 1), 2);
        assert_eq!(pfc(2, 2), 0);
        assert_eq!(lire_message(&message_coup(JeuReseau::Quatre, 3)), Some(('Q', 3)));
    }
}
