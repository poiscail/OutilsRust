//! Mini-jeux pour les pauses (150) : Serpent, Casse-briques, Démineur, et ceux de `minijeux2`. Records enregistrés.
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

// ---------------- Serpent ----------------

pub const GRILLE: i32 = 24;

pub struct Serpent {
    pub corps: VecDeque<(i32, i32)>,
    pub dir: (i32, i32),
    pub prochaine: (i32, i32),
    pub pomme: (i32, i32),
    pub fini: bool,
    pub score: u32,
}

impl Serpent {
    pub fn new() -> Self {
        let mut s = Serpent { corps: VecDeque::from([(10, 12), (9, 12), (8, 12)]), dir: (1, 0), prochaine: (1, 0), pomme: (0, 0), fini: false, score: 0 };
        s.placer_pomme();
        s
    }

    fn placer_pomme(&mut self) {
        loop {
            let p = (hasard(GRILLE as u64) as i32, hasard(GRILLE as u64) as i32);
            if !self.corps.contains(&p) {
                self.pomme = p;
                return;
            }
        }
    }

    pub fn tourner(&mut self, d: (i32, i32)) {
        if d != (-self.dir.0, -self.dir.1) {
            self.prochaine = d;
        }
    }

    /// Un pas : avance, mange, ou perd (mur ou soi-même).
    pub fn pas(&mut self) {
        if self.fini {
            return;
        }
        self.dir = self.prochaine;
        let t = self.corps[0];
        let n = (t.0 + self.dir.0, t.1 + self.dir.1);
        let mange = n == self.pomme;
        if !mange {
            self.corps.pop_back();
        }
        if n.0 < 0 || n.1 < 0 || n.0 >= GRILLE || n.1 >= GRILLE || self.corps.contains(&n) {
            self.fini = true;
            return;
        }
        self.corps.push_front(n);
        if mange {
            self.score += 1;
            self.placer_pomme();
        }
    }
}

// ---------------- Démineur ----------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Case {
    pub mine: bool,
    pub ouverte: bool,
    pub drapeau: bool,
    pub voisines: u8,
}

pub struct Demineur {
    pub l: usize,
    pub h: usize,
    pub mines: usize,
    pub cases: Vec<Case>,
    pub pret: bool, // mines posées (après le premier clic)
    pub perdu: bool,
    pub debut: Option<Instant>,
    pub fin: Option<u64>,
}

impl Demineur {
    pub fn new(l: usize, h: usize, mines: usize) -> Self {
        Demineur { l, h, mines, cases: vec![Case { mine: false, ouverte: false, drapeau: false, voisines: 0 }; l * h], pret: false, perdu: false, debut: None, fin: None }
    }

    fn voisins(&self, i: usize) -> Vec<usize> {
        let (x, y) = ((i % self.l) as i64, (i / self.l) as i64);
        let mut v = vec![];
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if (dx, dy) != (0, 0) && nx >= 0 && ny >= 0 && nx < self.l as i64 && ny < self.h as i64 {
                    v.push(ny as usize * self.l + nx as usize);
                }
            }
        }
        v
    }

    /// Pose les mines en évitant la première case cliquée et ses voisines.
    pub fn poser(&mut self, premier: usize, mines: &[usize]) {
        let interdit: Vec<usize> = std::iter::once(premier).chain(self.voisins(premier)).collect();
        for &m in mines.iter().filter(|m| !interdit.contains(m)).take(self.mines) {
            self.cases[m].mine = true;
        }
        for i in 0..self.cases.len() {
            self.cases[i].voisines = self.voisins(i).iter().filter(|&&v| self.cases[v].mine).count() as u8;
        }
        self.pret = true;
    }

    pub fn ouvrir(&mut self, i: usize) {
        if !self.pret {
            let mut tirage: Vec<usize> = (0..self.cases.len()).collect();
            for k in (1..tirage.len()).rev() {
                tirage.swap(k, hasard(k as u64 + 1) as usize);
            }
            self.poser(i, &tirage);
            self.debut = Some(Instant::now());
        }
        if self.perdu || self.fin.is_some() || self.cases[i].drapeau || self.cases[i].ouverte {
            return;
        }
        if self.cases[i].mine {
            self.perdu = true;
            for c in &mut self.cases {
                if c.mine {
                    c.ouverte = true;
                }
            }
            return;
        }
        // ouverture en cascade des zones sans mine autour
        let mut pile = vec![i];
        while let Some(k) = pile.pop() {
            if self.cases[k].ouverte || self.cases[k].drapeau {
                continue;
            }
            self.cases[k].ouverte = true;
            if self.cases[k].voisines == 0 {
                pile.extend(self.voisins(k));
            }
        }
        if self.cases.iter().all(|c| c.mine || c.ouverte) {
            self.fin = self.debut.map(|d| d.elapsed().as_secs());
        }
    }
}

// ---------------- Casse-briques ----------------

pub struct Briques {
    pub balle: Pos2,
    pub vitesse: Vec2,
    pub raquette: f32,
    pub briques: Vec<(Rect, u8)>,
    pub vies: u32,
    pub score: u32,
    pub lancee: bool,
}

const LB: f32 = 600.0;
const HB: f32 = 420.0;

impl Briques {
    pub fn new() -> Self {
        let mut briques = vec![];
        for r in 0..5 {
            for c in 0..10 {
                briques.push((Rect::from_min_size(Pos2::new(10.0 + c as f32 * 58.0, 30.0 + r as f32 * 22.0), Vec2::new(54.0, 18.0)), r as u8));
            }
        }
        Briques { balle: Pos2::new(LB / 2.0, HB - 40.0), vitesse: Vec2::new(180.0, -260.0), raquette: LB / 2.0, briques, vies: 3, score: 0, lancee: false }
    }

    pub fn avancer(&mut self, dt: f32) {
        if !self.lancee || self.vies == 0 {
            self.balle = Pos2::new(self.raquette, HB - 40.0);
            return;
        }
        let mut b = self.balle + self.vitesse * dt;
        if b.x < 6.0 || b.x > LB - 6.0 {
            self.vitesse.x = -self.vitesse.x;
            b.x = b.x.clamp(6.0, LB - 6.0);
        }
        if b.y < 6.0 {
            self.vitesse.y = self.vitesse.y.abs();
        }
        // raquette
        if b.y > HB - 34.0 && b.y < HB - 20.0 && (b.x - self.raquette).abs() < 50.0 && self.vitesse.y > 0.0 {
            self.vitesse.y = -self.vitesse.y.abs();
            self.vitesse.x = (b.x - self.raquette) * 6.0;
        }
        if let Some(i) = self.briques.iter().position(|(r, _)| r.expand(5.0).contains(b)) {
            self.briques.remove(i);
            self.vitesse.y = -self.vitesse.y;
            self.vitesse *= 1.01;
            self.score += 10;
        }
        if b.y > HB {
            self.vies = self.vies.saturating_sub(1);
            self.lancee = false;
            self.vitesse = Vec2::new(180.0, -260.0);
        }
        self.balle = b;
    }
}

// ---------------- Outil ----------------

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Records {
    serpent: u32,
    briques: u32,
    demineur: Option<u64>,
}

pub struct MiniJeux {
    onglet: u8,
    serpent: Serpent,
    dernier_pas: Instant,
    demineur: Demineur,
    briques: Briques,
    dernier: Instant,
    records: Records,
    salle: crate::minijeux2::Salle,
}

impl MiniJeux {
    pub fn new() -> Self {
        MiniJeux { onglet: 0, serpent: Serpent::new(), dernier_pas: Instant::now(), demineur: Demineur::new(16, 12, 30), briques: Briques::new(), dernier: Instant::now(), records: charger("minijeux"), salle: crate::minijeux2::Salle::new() }
    }
}

impl Outil for MiniJeux {
    fn nom(&self) -> &'static str {
        "🕹 Mini-jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.onglet, 0, "🐍 Serpent");
            ui.selectable_value(&mut self.onglet, 1, "🧱 Casse-briques");
            ui.selectable_value(&mut self.onglet, 2, "💣 Démineur");
            for (k, n) in crate::minijeux2::NOMS.iter().enumerate() {
                ui.selectable_value(&mut self.onglet, 3 + k as u8, *n);
            }
        });
        ui.separator();
        let rose = Color32::from_rgb(255, 95, 162);
        match self.onglet {
            0 => {
                let s = &mut self.serpent;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Score {} · record {}", s.score, self.records.serpent)).strong());
                    if ui.button("Nouvelle partie").clicked() {
                        *s = Serpent::new();
                    }
                    ui.weak("Flèches ou ZQSD");
                });
                ui.input(|i| {
                    for (k, d) in [(egui::Key::ArrowUp, (0, -1)), (egui::Key::Z, (0, -1)), (egui::Key::ArrowDown, (0, 1)), (egui::Key::S, (0, 1)),
                        (egui::Key::ArrowLeft, (-1, 0)), (egui::Key::Q, (-1, 0)), (egui::Key::ArrowRight, (1, 0)), (egui::Key::D, (1, 0))] {
                        if i.key_pressed(k) {
                            s.tourner(d);
                        }
                    }
                });
                let attente = (140 - s.score.min(20) * 4) as u128;
                if self.dernier_pas.elapsed().as_millis() >= attente {
                    s.pas();
                    self.dernier_pas = Instant::now();
                    if s.score > self.records.serpent {
                        self.records.serpent = s.score;
                        enregistrer("minijeux", &self.records);
                    }
                }
                let c = 18.0;
                let (zone, _) = ui.allocate_exact_size(Vec2::splat(c * GRILLE as f32), Sense::hover());
                let p = ui.painter_at(zone);
                p.rect_filled(zone, 6.0, Color32::from_rgb(20, 14, 28));
                let case = |x: i32, y: i32| Rect::from_min_size(zone.min + Vec2::new(x as f32 * c, y as f32 * c), Vec2::splat(c)).shrink(1.5);
                p.circle_filled(case(s.pomme.0, s.pomme.1).center(), c / 2.5, Color32::from_rgb(255, 80, 80));
                for (k, (x, y)) in s.corps.iter().enumerate() {
                    p.rect_filled(case(*x, *y), 4.0, if k == 0 { rose } else { Color32::from_rgb(200, 90, 160) });
                }
                if s.fini {
                    p.text(zone.center(), egui::Align2::CENTER_CENTER, "Perdu ! « Nouvelle partie »", egui::FontId::proportional(26.0), Color32::WHITE);
                } else {
                    ui.ctx().request_repaint();
                }
            }
            1 => {
                let b = &mut self.briques;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Score {} · vies {} · record {}", b.score, b.vies, self.records.briques)).strong());
                    if ui.button("Nouvelle partie").clicked() {
                        *b = Briques::new();
                    }
                    ui.weak("Souris pour la raquette, clic pour lancer");
                });
                let (zone, rep) = ui.allocate_exact_size(Vec2::new(LB, HB), Sense::click());
                if let Some(pos) = rep.hover_pos() {
                    b.raquette = (pos.x - zone.left()).clamp(50.0, LB - 50.0);
                }
                if rep.clicked() && b.vies > 0 {
                    b.lancee = true;
                }
                let dt = self.dernier.elapsed().as_secs_f32().min(0.05);
                self.dernier = Instant::now();
                for _ in 0..4 {
                    b.avancer(dt / 4.0);
                }
                if b.score > self.records.briques {
                    self.records.briques = b.score;
                    enregistrer("minijeux", &self.records);
                }
                let p = ui.painter_at(zone);
                p.rect_filled(zone, 6.0, Color32::from_rgb(20, 14, 28));
                for (r, rang) in &b.briques {
                    let teinte = [Color32::from_rgb(255, 95, 162), Color32::from_rgb(230, 110, 220), Color32::from_rgb(170, 120, 255), Color32::from_rgb(110, 150, 255), Color32::from_rgb(90, 200, 230)];
                    p.rect_filled(r.translate(zone.min.to_vec2()), 3.0, teinte[*rang as usize % 5]);
                }
                p.rect_filled(Rect::from_center_size(zone.min + Vec2::new(b.raquette, HB - 26.0), Vec2::new(100.0, 10.0)), 5.0, Color32::WHITE);
                p.circle_filled(zone.min + b.balle.to_vec2(), 6.0, Color32::from_rgb(255, 230, 140));
                if b.briques.is_empty() {
                    p.text(zone.center(), egui::Align2::CENTER_CENTER, "Gagné ! 🎉", egui::FontId::proportional(32.0), Color32::WHITE);
                } else if b.vies == 0 {
                    p.text(zone.center(), egui::Align2::CENTER_CENTER, "Perdu !", egui::FontId::proportional(32.0), Color32::WHITE);
                } else {
                    ui.ctx().request_repaint();
                }
            }
            n if n >= 3 => self.salle.ui(ui, n as usize - 3),
            _ => {
                let d = &mut self.demineur;
                ui.horizontal(|ui| {
                    for (n, l, h, m) in [("Facile", 9, 9, 10), ("Moyen", 16, 12, 30), ("Difficile", 24, 14, 70)] {
                        if ui.button(n).clicked() {
                            *d = Demineur::new(l, h, m);
                        }
                    }
                    let drapeaux = d.cases.iter().filter(|c| c.drapeau).count();
                    ui.label(format!("💣 {} · ⏱ {} s", d.mines as i64 - drapeaux as i64, d.fin.unwrap_or(d.debut.map(|t| t.elapsed().as_secs()).unwrap_or(0))));
                    if let Some(r) = self.records.demineur {
                        ui.weak(format!("record {r} s"));
                    }
                    ui.weak("clic droit : drapeau");
                });
                let c = 28.0;
                let (zone, _) = ui.allocate_exact_size(Vec2::new(c * d.l as f32, c * d.h as f32), Sense::hover());
                for i in 0..d.cases.len() {
                    let r = Rect::from_min_size(zone.min + Vec2::new((i % d.l) as f32 * c, (i / d.l) as f32 * c), Vec2::splat(c)).shrink(1.0);
                    let rep = ui.interact(r, ui.id().with(("case", i)), Sense::click());
                    let k = d.cases[i];
                    let fond = if k.ouverte { if k.mine { Color32::from_rgb(200, 50, 60) } else { Color32::from_rgb(45, 35, 55) } } else { Color32::from_rgb(90, 70, 120) };
                    ui.painter().rect_filled(r, 3.0, fond);
                    let texte = if k.ouverte && k.mine { "💣".to_string() } else if k.ouverte && k.voisines > 0 { k.voisines.to_string() } else if k.drapeau { "🚩".to_string() } else { String::new() };
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, texte, egui::FontId::proportional(15.0), Color32::WHITE);
                    if rep.clicked() {
                        d.ouvrir(i);
                    }
                    if rep.secondary_clicked() && !k.ouverte {
                        d.cases[i].drapeau = !k.drapeau;
                    }
                }
                if let Some(t) = d.fin {
                    ui.label(RichText::new(format!("Gagné en {t} s ! 🎉")).size(20.0));
                    if self.records.demineur.is_none_or(|r| t < r) && d.mines >= 30 {
                        self.records.demineur = Some(t);
                        enregistrer("minijeux", &self.records);
                    }
                } else if d.perdu {
                    ui.label(RichText::new("Boum ! Choisis une difficulté pour rejouer.").size(20.0));
                } else if d.debut.is_some() {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serpent() {
        let mut s = Serpent::new();
        s.pomme = (11, 12);
        s.pas();
        assert_eq!((s.score, s.corps.len()), (1, 4));
        s.tourner((-1, 0)); // demi-tour interdit
        s.pas();
        assert_eq!(s.corps[0], (12, 12));
        for _ in 0..20 {
            s.pas();
        }
        assert!(s.fini); // mur
    }

    #[test]
    fn demineur() {
        let mut d = Demineur::new(5, 5, 3);
        d.poser(0, &[24, 23, 22, 1, 6]); // 1 et 6 sont voisines de la 1re case : ignorées
        assert_eq!(d.cases.iter().filter(|c| c.mine).count(), 3);
        assert!(!d.cases[1].mine);
        d.debut = Some(Instant::now());
        d.ouvrir(0);
        assert!(d.cases[0].ouverte && d.cases[12].ouverte); // cascade
        assert!(!d.perdu);
        assert!(d.fin.is_some()); // tout ce qui n'est pas miné est ouvert
    }
}
