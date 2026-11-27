//! Simulations de la vie : écosystème herbe-lapins-renards (7) et évolution de créatures qui
//! apprennent à marcher toutes seules (14).
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

/// Générateur pseudo-aléatoire rapide (xorshift), reproductible.
#[derive(Clone)]
pub struct Alea(pub u64);
impl Alea {
    pub fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn entre(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

// ---------------------------------------------------------------------------------------------------
// 7 · Écosystème
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Espece {
    Lapin,
    Renard,
}

#[derive(Clone, Debug)]
pub struct Animal {
    pub espece: Espece,
    pub p: Vec2,
    pub dir: Vec2,
    pub energie: f32,
    pub age: f32,
    /// gènes : vitesse et portée de la vue (ils changent un peu à chaque naissance)
    pub vitesse: f32,
    pub vue: f32,
}

pub struct Ecosysteme {
    pub taille: Vec2,
    pub herbe: Vec<f32>, // grille 40×28, 0..1
    pub animaux: Vec<Animal>,
    pub temps: f32,
    pub histoire: Vec<(f32, usize, usize, f32)>, // temps, lapins, renards, herbe moyenne
    pub pousse: f32,
    a: Alea,
}

const GX: usize = 40;
const GY: usize = 28;

impl Ecosysteme {
    pub fn nouveau(lapins: usize, renards: usize, graine: u64) -> Self {
        let mut a = Alea(graine | 1);
        let taille = Vec2::new(800.0, 560.0);
        let mut animaux = Vec::new();
        for k in 0..lapins + renards {
            let espece = if k < lapins { Espece::Lapin } else { Espece::Renard };
            let ang = a.entre(0.0, std::f32::consts::TAU);
            animaux.push(Animal {
                espece,
                p: Vec2::new(a.entre(0.0, taille.x), a.entre(0.0, taille.y)),
                dir: Vec2::new(ang.cos(), ang.sin()),
                energie: if espece == Espece::Lapin { 60.0 } else { 90.0 },
                age: a.entre(0.0, 20.0),
                vitesse: if espece == Espece::Lapin { 55.0 } else { 62.0 },
                vue: if espece == Espece::Lapin { 70.0 } else { 110.0 },
            });
        }
        Ecosysteme { taille, herbe: (0..GX * GY).map(|_| a.entre(0.3, 1.0)).collect(), animaux, temps: 0.0, histoire: Vec::new(), pousse: 0.08, a }
    }

    pub fn compter(&self) -> (usize, usize) {
        let l = self.animaux.iter().filter(|x| x.espece == Espece::Lapin).count();
        (l, self.animaux.len() - l)
    }

    fn case(&self, p: Vec2) -> usize {
        let x = ((p.x / self.taille.x * GX as f32) as usize).min(GX - 1);
        let y = ((p.y / self.taille.y * GY as f32) as usize).min(GY - 1);
        y * GX + x
    }

    pub fn avancer(&mut self, dt: f32) {
        self.temps += dt;
        for h in &mut self.herbe {
            *h = (*h + self.pousse * dt).min(1.0);
        }
        let n = self.animaux.len();
        let mut morts = vec![false; n];
        let mut naissances = Vec::new();
        for i in 0..n {
            let a = self.animaux[i].clone();
            let mut cible: Option<(Vec2, bool)> = None; // (direction, fuir ?)
            match a.espece {
                Espece::Lapin => {
                    // fuir le renard le plus proche visible
                    let r = self.animaux.iter().enumerate().filter(|(j, x)| x.espece == Espece::Renard && !morts[*j]).map(|(_, x)| x.p).filter(|p| (*p - a.p).length() < a.vue).min_by(|p, q| (*p - a.p).length().total_cmp(&(*q - a.p).length()));
                    if let Some(r) = r {
                        cible = Some((a.p - r, true));
                    }
                }
                Espece::Renard => {
                    let l = self.animaux.iter().enumerate().filter(|(j, x)| x.espece == Espece::Lapin && !morts[*j]).filter(|(_, x)| (x.p - a.p).length() < a.vue).min_by(|p, q| (p.1.p - a.p).length().total_cmp(&(q.1.p - a.p).length())).map(|(j, x)| (j, x.p));
                    if let Some((j, p)) = l {
                        if (p - a.p).length() < 9.0 {
                            morts[j] = true;
                            self.animaux[i].energie = (self.animaux[i].energie + 55.0).min(160.0);
                        } else {
                            cible = Some((p - a.p, false));
                        }
                    }
                }
            }
            let x = &mut self.animaux[i];
            match cible {
                Some((d, _)) if d.length() > 0.01 => x.dir = (x.dir * 0.6 + d.normalized() * 0.4).normalized(),
                _ => {
                    let t = self.a.entre(-1.2, 1.2) * dt * 3.0;
                    let (s, c) = t.sin_cos();
                    x.dir = Vec2::new(x.dir.x * c - x.dir.y * s, x.dir.x * s + x.dir.y * c);
                }
            }
            x.p += x.dir * x.vitesse * dt;
            // rebond sur les bords
            if x.p.x < 0.0 || x.p.x > self.taille.x {
                x.dir.x = -x.dir.x;
                x.p.x = x.p.x.clamp(0.0, self.taille.x);
            }
            if x.p.y < 0.0 || x.p.y > self.taille.y {
                x.dir.y = -x.dir.y;
                x.p.y = x.p.y.clamp(0.0, self.taille.y);
            }
            // aller vite coûte plus d'énergie
            x.energie -= dt * (1.5 + x.vitesse * x.vitesse / 2500.0 + x.vue / 120.0);
            x.age += dt;
            let x = x.clone();
            if x.espece == Espece::Lapin {
                let c = self.case(x.p);
                let mange = self.herbe[c].min(dt * 1.2);
                self.herbe[c] -= mange;
                self.animaux[i].energie += mange * 18.0;
            }
            let seuil = if x.espece == Espece::Lapin { 75.0 } else { 120.0 };
            let vieux = if x.espece == Espece::Lapin { 45.0 } else { 70.0 };
            if x.energie <= 0.0 || x.age > vieux {
                morts[i] = true;
            } else if x.energie > seuil {
                self.animaux[i].energie /= 2.0;
                let mut b = self.animaux[i].clone();
                b.age = 0.0;
                b.vitesse = (b.vitesse * self.a.entre(0.9, 1.1)).clamp(20.0, 140.0);
                b.vue = (b.vue * self.a.entre(0.9, 1.1)).clamp(20.0, 220.0);
                b.p += Vec2::new(self.a.entre(-8.0, 8.0), self.a.entre(-8.0, 8.0));
                b.p = Vec2::new(b.p.x.clamp(0.0, self.taille.x), b.p.y.clamp(0.0, self.taille.y));
                naissances.push(b);
            }
        }
        let mut k = 0;
        self.animaux.retain(|_| {
            k += 1;
            !morts[k - 1]
        });
        self.animaux.extend(naissances);
        // pas plus de 1500 animaux (sinon le PC rame)
        self.animaux.truncate(1500);
        if self.histoire.last().is_none_or(|h| self.temps - h.0 >= 0.5) {
            let (l, r) = self.compter();
            let moy = self.herbe.iter().sum::<f32>() / self.herbe.len() as f32;
            self.histoire.push((self.temps, l, r, moy));
            if self.histoire.len() > 600 {
                self.histoire.remove(0);
            }
        }
    }
}

pub struct EcosystemeOutil {
    e: Ecosysteme,
    lapins: usize,
    renards: usize,
    vitesse: f32,
    pause: bool,
}

impl EcosystemeOutil {
    pub fn new() -> Self {
        EcosystemeOutil { e: Ecosysteme::nouveau(60, 8, crate::outil::hasard(u64::MAX)), lapins: 60, renards: 8, vitesse: 1.0, pause: false }
    }
}

impl Outil for EcosystemeOutil {
    fn nom(&self) -> &'static str {
        "🐇 Simulateur d'écosystème"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Écosystème");
            ui.label("Lapins");
            ui.add(egui::DragValue::new(&mut self.lapins).range(0..=400));
            ui.label("Renards");
            ui.add(egui::DragValue::new(&mut self.renards).range(0..=100));
            if ui.button("🔄 Recommencer").clicked() {
                let p = self.e.pousse;
                self.e = Ecosysteme::nouveau(self.lapins, self.renards, crate::outil::hasard(u64::MAX));
                self.e.pousse = p;
            }
            ui.label("Herbe qui repousse");
            ui.add(egui::Slider::new(&mut self.e.pousse, 0.01..=0.3).show_value(false));
            ui.label("Vitesse");
            ui.add(egui::Slider::new(&mut self.vitesse, 0.25..=8.0).logarithmic(true).show_value(false));
            let t = if self.pause { "▶" } else { "⏸" };
            ui.toggle_value(&mut self.pause, t);
        });
        if !self.pause {
            let dt = ui.input(|i| i.stable_dt).min(0.05) * self.vitesse;
            let pas = (dt / 0.03).ceil().max(1.0) as usize;
            for _ in 0..pas {
                self.e.avancer(dt / pas as f32);
            }
        }
        let haut_graphe = 120.0;
        let dispo = ui.available_size() - Vec2::new(0.0, haut_graphe + 30.0);
        let ech = (dispo.x / self.e.taille.x).min(dispo.y / self.e.taille.y).max(0.3);
        let (zone, _) = ui.allocate_exact_size(self.e.taille * ech, Sense::hover());
        let p = ui.painter_at(zone);
        let (cw, ch) = (zone.width() / GX as f32, zone.height() / GY as f32);
        for y in 0..GY {
            for x in 0..GX {
                let h = self.e.herbe[y * GX + x];
                let c = Color32::from_rgb((70.0 - 40.0 * h) as u8, (60.0 + 120.0 * h) as u8, (40.0 - 10.0 * h) as u8);
                p.rect_filled(Rect::from_min_size(zone.min + Vec2::new(x as f32 * cw, y as f32 * ch), Vec2::new(cw + 0.5, ch + 0.5)), 0.0, c);
            }
        }
        for a in &self.e.animaux {
            let q = zone.min + a.p * ech;
            match a.espece {
                Espece::Lapin => p.circle_filled(q, 3.5 * ech.max(0.8), Color32::from_rgb(240, 240, 235)),
                Espece::Renard => p.circle_filled(q, 5.0 * ech.max(0.8), Color32::from_rgb(235, 120, 40)),
            };
        }
        let (l, r) = self.e.compter();
        let moy_l = |e: Espece| {
            let v: Vec<&Animal> = self.e.animaux.iter().filter(|a| a.espece == e).collect();
            if v.is_empty() { (0.0, 0.0) } else { (v.iter().map(|a| a.vitesse).sum::<f32>() / v.len() as f32, v.iter().map(|a| a.vue).sum::<f32>() / v.len() as f32) }
        };
        let (vl, ol) = moy_l(Espece::Lapin);
        let (vr, or) = moy_l(Espece::Renard);
        ui.label(format!("⚪ {l} lapins (vitesse {vl:.0}, vue {ol:.0}) · 🟠 {r} renards (vitesse {vr:.0}, vue {or:.0}) · {:.0} s", self.e.temps));
        // courbes de population
        let (g, _) = ui.allocate_exact_size(Vec2::new(zone.width(), haut_graphe), Sense::hover());
        let p = ui.painter_at(g);
        p.rect_filled(g, 4.0, Color32::from_rgb(20, 22, 28));
        let h = &self.e.histoire;
        if h.len() > 1 {
            let max = h.iter().map(|x| x.1.max(x.2)).max().unwrap_or(1).max(10) as f32;
            for (k, couleur) in [(0usize, Color32::from_rgb(240, 240, 235)), (1, Color32::from_rgb(235, 120, 40)), (2, Color32::from_rgb(90, 180, 70))] {
                let pts: Vec<Pos2> = h.iter().enumerate().map(|(i, x)| {
                    let v = match k {
                        0 => x.1 as f32 / max,
                        1 => x.2 as f32 / max,
                        _ => x.3,
                    };
                    Pos2::new(g.left() + i as f32 / (h.len() - 1) as f32 * g.width(), g.bottom() - v * g.height())
                }).collect();
                p.add(egui::Shape::line(pts, Stroke::new(1.5_f32, couleur)));
            }
            p.text(g.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, "lapins · renards · herbe", FontId::proportional(11.0), Color32::from_gray(150));
        }
        if l == 0 && r == 0 {
            ui.weak("Tout le monde a disparu… Recommence avec d'autres réglages !");
        }
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------------
// 14 · Évolution : des créatures apprennent à marcher
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Muscle {
    pub a: usize,
    pub b: usize,
    pub court: f32,
    pub long: f32,
    pub phase: f32,
}

#[derive(Clone, Debug)]
pub struct Creature {
    pub noeuds: Vec<(Vec2, f32)>, // position de départ, adhérence au sol (0..1)
    pub muscles: Vec<Muscle>,
    pub periode: f32,
    pub score: f32,
}

pub fn creature_hasard(a: &mut Alea) -> Creature {
    let n = 3 + (a.f() * 3.0) as usize;
    let noeuds: Vec<(Vec2, f32)> = (0..n).map(|_| (Vec2::new(a.entre(-40.0, 40.0), a.entre(-60.0, 0.0)), a.f())).collect();
    let mut muscles = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            if a.f() < 0.75 || j == i + 1 {
                let l = (noeuds[i].0 - noeuds[j].0).length().max(15.0);
                muscles.push(Muscle { a: i, b: j, court: l * a.entre(0.6, 1.0), long: l * a.entre(1.0, 1.5), phase: a.f() });
            }
        }
    }
    Creature { noeuds, muscles, periode: a.entre(0.5, 2.0), score: 0.0 }
}

pub fn muter(c: &Creature, a: &mut Alea, force: f32) -> Creature {
    let mut m = c.clone();
    for n in &mut m.noeuds {
        if a.f() < 0.3 {
            n.0 += Vec2::new(a.entre(-8.0, 8.0), a.entre(-8.0, 8.0)) * force;
        }
        if a.f() < 0.3 {
            n.1 = (n.1 + a.entre(-0.3, 0.3) * force).clamp(0.0, 1.0);
        }
    }
    for mu in &mut m.muscles {
        if a.f() < 0.3 {
            mu.phase = (mu.phase + a.entre(-0.2, 0.2) * force).rem_euclid(1.0);
        }
        if a.f() < 0.3 {
            mu.court = (mu.court * (1.0 + a.entre(-0.15, 0.15) * force)).max(8.0);
            mu.long = (mu.long * (1.0 + a.entre(-0.15, 0.15) * force)).max(mu.court + 2.0);
        }
    }
    if a.f() < 0.2 {
        m.periode = (m.periode * (1.0 + a.entre(-0.2, 0.2) * force)).clamp(0.3, 3.0);
    }
    m.score = 0.0;
    m
}

/// État d'une créature en train de bouger.
pub struct Corps {
    pub p: Vec<Vec2>,
    pub avant: Vec<Vec2>,
    pub t: f32,
}

impl Corps {
    pub fn nouveau(c: &Creature) -> Self {
        // posée sur le sol (y = 0 en bas, vers le haut négatif)
        let bas = c.noeuds.iter().map(|n| n.0.y).fold(f32::MIN, f32::max);
        let p: Vec<Vec2> = c.noeuds.iter().map(|n| n.0 - Vec2::new(0.0, bas)).collect();
        Corps { avant: p.clone(), p, t: 0.0 }
    }

    pub fn pas(&mut self, c: &Creature, dt: f32) {
        self.t += dt;
        for i in 0..self.p.len() {
            let v = (self.p[i] - self.avant[i]) * 0.99;
            self.avant[i] = self.p[i];
            self.p[i] += v + Vec2::new(0.0, 500.0) * dt * dt;
        }
        for _ in 0..6 {
            for m in &c.muscles {
                let s = (std::f32::consts::TAU * (self.t / c.periode + m.phase)).sin() * 0.5 + 0.5;
                let voulu = m.court + (m.long - m.court) * s;
                let d = self.p[m.b] - self.p[m.a];
                let l = d.length().max(1e-3);
                let corr = d * ((l - voulu) / l) * 0.5 * 0.5;
                self.p[m.a] += corr;
                self.p[m.b] -= corr;
            }
            for i in 0..self.p.len() {
                if self.p[i].y > 0.0 {
                    self.p[i].y = 0.0;
                    // au sol, les noeuds qui adhèrent glissent peu : c'est ce qui fait avancer
                    let f = c.noeuds[i].1;
                    let vx = self.p[i].x - self.avant[i].x;
                    self.avant[i].x = self.p[i].x - vx * (1.0 - f * 0.95);
                    self.avant[i].y = 0.0;
                }
            }
        }
    }

    pub fn centre(&self) -> f32 {
        self.p.iter().map(|q| q.x).sum::<f32>() / self.p.len() as f32
    }
}

/// Distance parcourue en `secondes` (vers la droite).
pub fn evaluer(c: &Creature, secondes: f32) -> f32 {
    let mut b = Corps::nouveau(c);
    let depart = b.centre();
    let dt = 1.0 / 60.0;
    for _ in 0..(secondes / dt) as usize {
        b.pas(c, dt);
    }
    let d = b.centre() - depart;
    if d.is_finite() { d } else { 0.0 }
}

pub struct Population {
    pub creatures: Vec<Creature>,
    pub generation: usize,
    pub meilleurs: Vec<f32>,
    a: Alea,
}

impl Population {
    pub fn nouvelle(n: usize, graine: u64) -> Self {
        let mut a = Alea(graine | 1);
        Population { creatures: (0..n).map(|_| creature_hasard(&mut a)).collect(), generation: 0, meilleurs: Vec::new(), a }
    }

    /// Évalue, garde les meilleures, les fait « avoir des petits » (copies un peu changées).
    pub fn generation_suivante(&mut self) {
        for c in &mut self.creatures {
            c.score = evaluer(c, 8.0);
        }
        self.creatures.sort_by(|a, b| b.score.total_cmp(&a.score));
        self.meilleurs.push(self.creatures[0].score);
        let n = self.creatures.len();
        let gardes = n / 3;
        let mut v: Vec<Creature> = self.creatures[..gardes].to_vec();
        let mut k = 0;
        while v.len() < n - 2 {
            let m = muter(&self.creatures[k % gardes], &mut self.a, 1.0);
            v.push(m);
            k += 1;
        }
        // un peu de sang neuf
        while v.len() < n {
            v.push(creature_hasard(&mut self.a));
        }
        self.creatures = v;
        self.generation += 1;
    }
}

pub struct Evolution {
    pop: Arc<Mutex<Population>>,
    tourne: Arc<AtomicBool>,
    meilleure: Option<Creature>,
    corps: Option<Corps>,
    camera: f32,
}

impl Evolution {
    pub fn new() -> Self {
        Evolution { pop: Arc::new(Mutex::new(Population::nouvelle(40, crate::outil::hasard(u64::MAX)))), tourne: Arc::default(), meilleure: None, corps: None, camera: 0.0 }
    }

    fn lancer(&self) {
        if self.tourne.swap(true, Relaxed) {
            return;
        }
        let (p, t) = (self.pop.clone(), self.tourne.clone());
        std::thread::spawn(move || {
            while t.load(Relaxed) {
                // on travaille sur une copie pour ne pas bloquer l'affichage
                let mut copie = {
                    let x = p.lock().unwrap();
                    Population { creatures: x.creatures.clone(), generation: x.generation, meilleurs: x.meilleurs.clone(), a: x.a.clone() }
                };
                copie.generation_suivante();
                *p.lock().unwrap() = copie;
            }
        });
    }
}

impl Drop for Evolution {
    fn drop(&mut self) {
        self.tourne.store(false, Relaxed);
    }
}

impl Outil for Evolution {
    fn nom(&self) -> &'static str {
        "🧬 Évolution en direct"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let (gen, meilleurs, champion) = {
            let p = self.pop.lock().unwrap();
            (p.generation, p.meilleurs.clone(), p.creatures.first().cloned())
        };
        ui.horizontal_wrapped(|ui| {
            ui.heading("Évolution en direct");
            if self.tourne.load(Relaxed) {
                if ui.button("⏸ Pause").clicked() {
                    self.tourne.store(false, Relaxed);
                }
            } else if ui.button(RichText::new("▶ Faire évoluer").strong()).clicked() {
                self.lancer();
            }
            if ui.button("🔄 Nouvelles créatures").clicked() {
                self.tourne.store(false, Relaxed);
                *self.pop.lock().unwrap() = Population::nouvelle(40, crate::outil::hasard(u64::MAX));
                self.meilleure = None;
                self.corps = None;
            }
            ui.label(format!("Génération {gen}"));
            if let Some(m) = meilleurs.last() {
                ui.label(format!("· record : {:.0} cm en 8 s", m.max(0.0) / 2.0));
            }
        });
        ui.weak("40 créatures faites de points et de muscles. À chaque génération, les meilleures marcheuses ont des « petits » un peu différents : elles apprennent à avancer toutes seules.");
        // on rejoue la meilleure créature de la dernière génération
        if gen > 0 && (self.meilleure.is_none() || self.corps.as_ref().is_some_and(|c| c.t > 8.0)) {
            self.meilleure = champion.clone();
            self.corps = self.meilleure.as_ref().map(Corps::nouveau);
            self.camera = 0.0;
        }
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), (ui.available_height() - 130.0).max(200.0)), Sense::hover());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 6.0, Color32::from_rgb(170, 210, 240));
        let sol = zone.bottom() - 40.0;
        p.rect_filled(Rect::from_min_max(Pos2::new(zone.left(), sol), zone.max), 0.0, Color32::from_rgb(110, 170, 80));
        if let (Some(c), Some(b)) = (&self.meilleure, &mut self.corps) {
            let dt = ui.input(|i| i.stable_dt).min(0.05);
            let n = (dt * 60.0).round().max(1.0) as usize;
            for _ in 0..n {
                b.pas(c, 1.0 / 60.0);
            }
            self.camera += (b.centre() - self.camera) * 0.1;
            let ech = 2.0;
            let vers = |q: Vec2| Pos2::new(zone.center().x + (q.x - self.camera) * ech, sol + q.y * ech);
            // repères de distance
            let debut = ((self.camera - zone.width() / ech) / 50.0).floor() as i32;
            for k in debut..debut + (zone.width() / ech / 50.0) as i32 * 2 + 4 {
                let x = vers(Vec2::new(k as f32 * 50.0, 0.0)).x;
                p.line_segment([Pos2::new(x, sol), Pos2::new(x, sol + 10.0)], Stroke::new(1.0_f32, Color32::from_gray(60)));
                p.text(Pos2::new(x, sol + 20.0), egui::Align2::CENTER_CENTER, format!("{}", k * 25), FontId::proportional(10.0), Color32::from_gray(50));
            }
            for m in &c.muscles {
                let s = (std::f32::consts::TAU * (b.t / c.periode + m.phase)).sin() * 0.5 + 0.5;
                p.line_segment([vers(b.p[m.a]), vers(b.p[m.b])], Stroke::new(4.0_f32, Color32::from_rgb((150.0 + 100.0 * s) as u8, 60, 60)));
            }
            for (k, q) in b.p.iter().enumerate() {
                let f = c.noeuds[k].1;
                p.circle_filled(vers(*q), 7.0, Color32::from_rgb((255.0 * (1.0 - f)) as u8, (255.0 * (1.0 - f)) as u8, (255.0 * (1.0 - f)) as u8));
                p.circle_stroke(vers(*q), 7.0, Stroke::new(1.5_f32, Color32::BLACK));
            }
            p.text(zone.left_top() + Vec2::new(10.0, 8.0), egui::Align2::LEFT_TOP, format!("Meilleure de la génération {} · {:.0} cm", gen, (b.centre() - Corps::nouveau(c).centre()) / 2.0), FontId::proportional(14.0), Color32::BLACK);
        } else {
            p.text(zone.center(), egui::Align2::CENTER_CENTER, "Clique « Faire évoluer » !", FontId::proportional(22.0), Color32::BLACK);
        }
        // progrès au fil des générations
        let (g, _) = ui.allocate_exact_size(Vec2::new(zone.width(), 100.0), Sense::hover());
        let p = ui.painter_at(g);
        p.rect_filled(g, 4.0, Color32::from_rgb(20, 22, 28));
        if meilleurs.len() > 1 {
            let max = meilleurs.iter().fold(1.0f32, |a, b| a.max(*b));
            let min = meilleurs.iter().fold(0.0f32, |a, b| a.min(*b));
            let pts: Vec<Pos2> = meilleurs.iter().enumerate().map(|(i, v)| Pos2::new(g.left() + i as f32 / (meilleurs.len() - 1) as f32 * g.width(), g.bottom() - (v - min) / (max - min).max(1.0) * (g.height() - 10.0))).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0_f32, Color32::from_rgb(120, 220, 120))));
            p.text(g.left_top() + Vec2::new(6.0, 4.0), egui::Align2::LEFT_TOP, "distance de la meilleure, génération après génération", FontId::proportional(11.0), Color32::from_gray(150));
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ecosysteme() {
        let mut e = Ecosysteme::nouveau(40, 5, 7);
        for _ in 0..600 {
            e.avancer(0.05);
        }
        assert!(e.histoire.len() > 50);
        assert!(e.animaux.iter().all(|a| a.p.x >= 0.0 && a.p.x <= e.taille.x && a.p.y >= 0.0 && a.p.y <= e.taille.y));
        assert!(e.herbe.iter().all(|h| (0.0..=1.0).contains(h)));
        // sans renards, les lapins prospèrent
        let mut e = Ecosysteme::nouveau(20, 0, 3);
        for _ in 0..400 {
            e.avancer(0.05);
        }
        assert!(e.compter().0 > 20, "{}", e.compter().0);
    }

    #[test]
    fn evolution() {
        let mut a = Alea(5);
        let c = creature_hasard(&mut a);
        assert!(c.noeuds.len() >= 3 && !c.muscles.is_empty());
        let d = evaluer(&c, 4.0);
        assert!(d.is_finite());
        // la même créature avance toujours pareil (simulation reproductible)
        assert_eq!(d, evaluer(&c, 4.0));
        let mut p = Population::nouvelle(20, 11);
        for _ in 0..12 {
            p.generation_suivante();
        }
        // la sélection garde les meilleures : le record ne baisse jamais beaucoup, et progresse
        assert!(p.meilleurs.last().unwrap() >= p.meilleurs.first().unwrap(), "{:?}", p.meilleurs);
        let b = Corps::nouveau(&p.creatures[0]);
        assert!(b.p.iter().all(|q| q.y <= 0.0 + 1e-3));
    }
}
