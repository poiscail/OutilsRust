//! Simulateur de physique (bac à sable) : balles, caisses, dominos, cordes, ressorts, murs et rampes.
//! Moteur « Verlet » : des points reliés par des liens rigides ou souples, et des collisions entre
//! les points, les bords des objets et les murs.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub p: Vec2,
    pub avant: Vec2,
    pub r: f32,
    /// 0 = fixé
    pub inv_masse: f32,
    pub corps: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Lien {
    pub a: usize,
    pub b: usize,
    pub longueur: f32,
    /// 1 = rigide, plus petit = ressort
    pub raideur: f32,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Genre {
    Balle,
    Caisse,
    Domino,
    Corde,
    Ressort,
}

#[derive(Clone, Debug)]
pub struct Corps {
    pub genre: Genre,
    pub points: Vec<usize>,
    /// bords (paires de points) contre lesquels les autres objets butent
    pub bords: Vec<(usize, usize)>,
    pub couleur: Color32,
}

#[derive(Clone, Default)]
pub struct Monde {
    pub points: Vec<Point>,
    pub liens: Vec<Lien>,
    pub corps: Vec<Corps>,
    pub murs: Vec<(Vec2, Vec2)>,
    pub taille: Vec2,
    pub gravite: f32,
    pub rebond: f32,
}

fn proche_segment(p: Vec2, a: Vec2, b: Vec2) -> (Vec2, f32) {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t, t)
}

impl Monde {
    pub fn nouveau(taille: Vec2) -> Self {
        Monde { taille, gravite: 900.0, rebond: 0.3, ..Default::default() }
    }

    fn point(&mut self, p: Vec2, r: f32, masse: f32, corps: usize) -> usize {
        self.points.push(Point { p, avant: p, r, inv_masse: if masse > 0.0 { 1.0 / masse } else { 0.0 }, corps });
        self.points.len() - 1
    }

    fn lier(&mut self, a: usize, b: usize, raideur: f32, visible: bool) {
        let longueur = (self.points[a].p - self.points[b].p).length();
        self.liens.push(Lien { a, b, longueur, raideur, visible });
    }

    pub fn balle(&mut self, c: Vec2, r: f32, couleur: Color32) -> usize {
        let k = self.corps.len();
        let p = self.point(c, r, r * r, k);
        self.corps.push(Corps { genre: Genre::Balle, points: vec![p], bords: vec![], couleur });
        k
    }

    /// Rectangle rigide (4 coins + 2 diagonales).
    pub fn rectangle(&mut self, c: Vec2, l: f32, h: f32, genre: Genre, couleur: Color32) -> usize {
        let k = self.corps.len();
        let m = l * h / 4.0;
        let coins = [Vec2::new(-l, -h), Vec2::new(l, -h), Vec2::new(l, h), Vec2::new(-l, h)].map(|d| self.point(c + d / 2.0, 2.0, m, k));
        for i in 0..4 {
            self.lier(coins[i], coins[(i + 1) % 4], 1.0, false);
        }
        self.lier(coins[0], coins[2], 1.0, false);
        self.lier(coins[1], coins[3], 1.0, false);
        let bords = (0..4).map(|i| (coins[i], coins[(i + 1) % 4])).collect();
        self.corps.push(Corps { genre, points: coins.to_vec(), bords, couleur });
        k
    }

    /// Corde accrochée en `a` (fixe) qui pend jusqu'à `b`.
    pub fn corde(&mut self, a: Vec2, b: Vec2, couleur: Color32) -> usize {
        let k = self.corps.len();
        let n = (((b - a).length() / 14.0) as usize).clamp(2, 60);
        let mut pts = Vec::new();
        for i in 0..=n {
            let p = a + (b - a) * (i as f32 / n as f32);
            pts.push(self.point(p, 3.0, if i == 0 { 0.0 } else { 1.0 }, k));
        }
        for w in pts.windows(2) {
            self.lier(w[0], w[1], 1.0, true);
        }
        self.corps.push(Corps { genre: Genre::Corde, points: pts, bords: vec![], couleur });
        k
    }

    /// Ressort entre deux points existants.
    pub fn ressort(&mut self, a: usize, b: usize) {
        let k = self.corps.len();
        self.lier(a, b, 0.04, true);
        self.corps.push(Corps { genre: Genre::Ressort, points: vec![a, b], bords: vec![], couleur: Color32::from_rgb(200, 200, 90) });
        let _ = k;
    }

    /// Point le plus proche (pour attraper ou accrocher).
    pub fn point_proche(&self, p: Vec2, max: f32) -> Option<usize> {
        self.points.iter().enumerate().map(|(i, q)| (i, (q.p - p).length() - q.r)).filter(|(_, d)| *d < max).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0)
    }

    /// Supprime le corps qui contient le point `i`.
    pub fn supprimer(&mut self, i: usize) {
        let k = self.points[i].corps;
        let garde: Vec<bool> = self.points.iter().map(|p| p.corps != k).collect();
        let mut nouvel = vec![usize::MAX; self.points.len()];
        let mut n = 0;
        for (j, g) in garde.iter().enumerate() {
            if *g {
                nouvel[j] = n;
                n += 1;
            }
        }
        self.points = self.points.iter().zip(&garde).filter(|(_, g)| **g).map(|(p, _)| *p).collect();
        self.liens = self.liens.iter().filter(|l| nouvel[l.a] != usize::MAX && nouvel[l.b] != usize::MAX).map(|l| Lien { a: nouvel[l.a], b: nouvel[l.b], ..*l }).collect();
        let mut corps = Vec::new();
        let mut renum = vec![usize::MAX; self.corps.len()];
        for (j, c) in self.corps.iter().enumerate() {
            if j == k || c.points.iter().any(|p| nouvel[*p] == usize::MAX) {
                continue;
            }
            renum[j] = corps.len();
            corps.push(Corps { points: c.points.iter().map(|p| nouvel[*p]).collect(), bords: c.bords.iter().map(|(a, b)| (nouvel[*a], nouvel[*b])).collect(), ..c.clone() });
        }
        for p in &mut self.points {
            p.corps = renum[p.corps];
        }
        self.corps = corps;
    }

    /// Avance de `dt` secondes (en plusieurs petits pas pour la stabilité).
    pub fn avancer(&mut self, dt: f32) {
        let pas = 8;
        let h = dt / pas as f32;
        for _ in 0..pas {
            for p in &mut self.points {
                if p.inv_masse == 0.0 {
                    continue;
                }
                let v = (p.p - p.avant) * 0.999;
                p.avant = p.p;
                p.p += v + Vec2::new(0.0, self.gravite) * h * h;
            }
            for _ in 0..3 {
                self.contraintes();
                self.collisions();
            }
        }
    }

    fn contraintes(&mut self) {
        for l in &self.liens {
            let (a, b) = (self.points[l.a], self.points[l.b]);
            let d = b.p - a.p;
            let len = d.length().max(1e-4);
            let w = a.inv_masse + b.inv_masse;
            if w == 0.0 {
                continue;
            }
            let corr = d * ((len - l.longueur) / len) * l.raideur / w;
            self.points[l.a].p += corr * a.inv_masse;
            self.points[l.b].p -= corr * b.inv_masse;
        }
    }

    fn collisions(&mut self) {
        let n = self.points.len();
        let (l, h) = (self.taille.x, self.taille.y);
        let rebond = self.rebond;
        // bords du monde
        for p in &mut self.points {
            if p.inv_masse == 0.0 {
                continue;
            }
            let v = p.p - p.avant;
            if p.p.y > h - p.r {
                p.p.y = h - p.r;
                p.avant.y = p.p.y + v.y * rebond;
                p.avant.x = p.p.x - v.x * 0.9; // frottement au sol
            }
            if p.p.x < p.r {
                p.p.x = p.r;
                p.avant.x = p.p.x + v.x * rebond;
            }
            if p.p.x > l - p.r {
                p.p.x = l - p.r;
                p.avant.x = p.p.x + v.x * rebond;
            }
        }
        // point contre point (corps différents)
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (self.points[i], self.points[j]);
                if a.corps == b.corps {
                    continue;
                }
                let d = b.p - a.p;
                let min = a.r + b.r;
                let dist2 = d.length_sq();
                if dist2 < min * min && dist2 > 1e-6 {
                    let dist = dist2.sqrt();
                    let w = a.inv_masse + b.inv_masse;
                    if w == 0.0 {
                        continue;
                    }
                    let corr = d / dist * (min - dist) / w;
                    self.points[i].p -= corr * a.inv_masse;
                    self.points[j].p += corr * b.inv_masse;
                }
            }
        }
        // point contre murs fixes
        for i in 0..n {
            let p = self.points[i];
            if p.inv_masse == 0.0 {
                continue;
            }
            for &(a, b) in &self.murs {
                let (q, _) = proche_segment(p.p, a, b);
                let d = p.p - q;
                let dist = d.length();
                let min = p.r + 4.0;
                if dist < min && dist > 1e-5 {
                    let normale = d / dist;
                    let v = p.p - p.avant;
                    self.points[i].p = q + normale * min;
                    // composante normale amortie, glissement le long du mur
                    let vn = normale * v.dot(normale);
                    let vt = v - vn;
                    self.points[i].avant = self.points[i].p - (vt * 0.98 - vn * rebond);
                }
            }
        }
        // point contre bords des autres objets (caisses, dominos)
        let bords: Vec<(usize, usize, usize)> = self.corps.iter().enumerate().flat_map(|(k, c)| c.bords.iter().map(move |(a, b)| (k, *a, *b))).collect();
        for i in 0..n {
            for &(k, a, b) in &bords {
                let p = self.points[i];
                if p.corps == k {
                    continue;
                }
                let (pa, pb) = (self.points[a], self.points[b]);
                let (q, t) = proche_segment(p.p, pa.p, pb.p);
                let d = p.p - q;
                let dist = d.length();
                let min = p.r + 2.0;
                if dist < min && dist > 1e-5 {
                    let normale = d / dist;
                    let enfonce = min - dist;
                    let (wa, wb) = ((1.0 - t) * pa.inv_masse, t * pb.inv_masse);
                    let w = p.inv_masse + wa + wb;
                    if w == 0.0 {
                        continue;
                    }
                    let corr = normale * enfonce / w;
                    self.points[i].p += corr * p.inv_masse;
                    self.points[a].p -= corr * wa;
                    self.points[b].p -= corr * wb;
                }
            }
        }
    }

    /// Énergie cinétique (approchée), pour les tests.
    pub fn agitation(&self) -> f32 {
        self.points.iter().filter(|p| p.inv_masse > 0.0).map(|p| (p.p - p.avant).length_sq()).sum()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Outil_ {
    Balle,
    Caisse,
    Domino,
    Mur,
    Corde,
    Ressort,
    Main,
    Gomme,
}

pub struct Physique {
    monde: Monde,
    outil: Outil_,
    taille_balle: f32,
    pause: bool,
    ralenti: bool,
    debut: Option<Vec2>,
    attrape: Option<usize>,
    ressort_de: Option<usize>,
    couleur: usize,
    pret: bool,
}

const COULEURS: [Color32; 8] = [
    Color32::from_rgb(240, 90, 80),
    Color32::from_rgb(90, 170, 250),
    Color32::from_rgb(120, 210, 110),
    Color32::from_rgb(250, 200, 60),
    Color32::from_rgb(190, 120, 240),
    Color32::from_rgb(250, 140, 60),
    Color32::from_rgb(80, 220, 210),
    Color32::from_rgb(240, 120, 190),
];

impl Physique {
    pub fn new() -> Self {
        let mut p = Physique { monde: Monde::nouveau(Vec2::new(900.0, 500.0)), outil: Outil_::Balle, taille_balle: 18.0, pause: false, ralenti: false, debut: None, attrape: None, ressort_de: None, couleur: 0, pret: false };
        p.scene(0);
        p
    }

    fn couleur(&mut self) -> Color32 {
        self.couleur = (self.couleur + 1) % COULEURS.len();
        COULEURS[self.couleur]
    }

    fn scene(&mut self, n: u8) {
        let t = self.monde.taille;
        let (g, r) = (self.monde.gravite, self.monde.rebond);
        self.monde = Monde { gravite: g, rebond: r, ..Monde::nouveau(t) };
        let sol = t.y;
        match n {
            // dominos et une balle sur une rampe
            0 => {
                self.monde.murs.push((Vec2::new(40.0, 120.0), Vec2::new(240.0, sol - 60.0)));
                for k in 0..10 {
                    let c = self.couleur();
                    self.monde.rectangle(Vec2::new(300.0 + k as f32 * 50.0, sol - 40.0), 12.0, 80.0, Genre::Domino, c);
                }
                self.monde.balle(Vec2::new(70.0, 90.0), 20.0, COULEURS[0]);
            }
            // pendule de Newton
            1 => {
                for k in 0..5 {
                    let x = t.x / 2.0 - 80.0 + k as f32 * 40.0;
                    let c = self.monde.corde(Vec2::new(x, 60.0), Vec2::new(x, 260.0), Color32::GRAY);
                    let bout = *self.monde.corps[c].points.last().unwrap();
                    self.monde.points[bout].r = 19.5;
                    self.monde.points[bout].inv_masse = 1.0 / 30.0;
                }
                // la première bille part de côté
                let c = self.monde.corps[0].points.clone();
                for (i, p) in c.iter().enumerate() {
                    let f = i as f32 / (c.len() - 1) as f32;
                    let q = Vec2::new(t.x / 2.0 - 80.0 - 150.0 * f, 60.0 + 120.0 * f);
                    self.monde.points[*p].p = q;
                    self.monde.points[*p].avant = q;
                }
            }
            // pyramide de caisses
            2 => {
                for rang in 0..5 {
                    for k in 0..(5 - rang) {
                        let c = self.couleur();
                        let x = t.x / 2.0 - (4 - rang) as f32 * 25.0 + k as f32 * 52.0;
                        self.monde.rectangle(Vec2::new(x, sol - 26.0 - rang as f32 * 52.0), 48.0, 48.0, Genre::Caisse, c);
                    }
                }
                self.monde.balle(Vec2::new(60.0, sol - 300.0), 30.0, COULEURS[0]);
                let b = self.monde.corps.last().unwrap().points[0];
                self.monde.points[b].avant = self.monde.points[b].p - Vec2::new(14.0, 0.0);
            }
            // pluie de balles et entonnoir
            _ => {
                self.monde.murs.push((Vec2::new(100.0, 150.0), Vec2::new(t.x / 2.0 - 30.0, 300.0)));
                self.monde.murs.push((Vec2::new(t.x - 100.0, 150.0), Vec2::new(t.x / 2.0 + 30.0, 300.0)));
                for k in 0..60 {
                    let c = COULEURS[k % COULEURS.len()];
                    self.monde.balle(Vec2::new(150.0 + (k % 15) as f32 * 40.0, 20.0 + (k / 15) as f32 * 30.0), 9.0 + (k % 3) as f32 * 3.0, c);
                }
            }
        }
    }
}

impl Outil for Physique {
    fn nom(&self) -> &'static str {
        "🧪 Simulateur de physique"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Simulateur de physique");
            for (o, n) in [(Outil_::Balle, "🔵 Balle"), (Outil_::Caisse, "📦 Caisse"), (Outil_::Domino, "🁢 Domino"), (Outil_::Mur, "🧱 Mur / rampe"), (Outil_::Corde, "🪢 Corde"), (Outil_::Ressort, "🌀 Ressort"), (Outil_::Main, "✋ Attraper"), (Outil_::Gomme, "🧽 Gomme")] {
                if ui.selectable_label(self.outil == o, n).clicked() {
                    self.outil = o;
                    self.ressort_de = None;
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let t = if self.pause { "▶ Reprendre" } else { "⏸ Pause" };
            ui.toggle_value(&mut self.pause, t);
            ui.toggle_value(&mut self.ralenti, "🐢 Ralenti");
            ui.label("Gravité");
            ui.add(egui::Slider::new(&mut self.monde.gravite, -600.0..=2500.0).show_value(false));
            ui.label("Rebond");
            ui.add(egui::Slider::new(&mut self.monde.rebond, 0.0..=0.95).show_value(false));
            if self.outil == Outil_::Balle {
                ui.label("Taille");
                ui.add(egui::Slider::new(&mut self.taille_balle, 6.0..=50.0).show_value(false));
            }
            ui.separator();
            for (k, n) in ["🁢 Dominos", "⚪ Pendule de Newton", "📦 Pyramide", "🌧 Pluie de balles"].iter().enumerate() {
                if ui.small_button(*n).clicked() {
                    self.scene(k as u8);
                }
            }
            if ui.small_button("🗑 Tout effacer").clicked() {
                let t = self.monde.taille;
                let (g, r) = (self.monde.gravite, self.monde.rebond);
                self.monde = Monde { gravite: g, rebond: r, ..Monde::nouveau(t) };
            }
        });
        let (zone, rep) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.monde.taille = zone.size();
        if !self.pret {
            // première ouverture : la scène se place à la vraie taille de la fenêtre
            self.pret = true;
            self.scene(0);
        }
        let vers_monde = |p: Pos2| p - zone.min;
        // souris
        if let Some(pos) = rep.interact_pointer_pos().map(vers_monde) {
            match self.outil {
                Outil_::Balle if rep.clicked() => {
                    let c = self.couleur();
                    self.monde.balle(pos, self.taille_balle, c);
                }
                Outil_::Caisse if rep.clicked() => {
                    let c = self.couleur();
                    self.monde.rectangle(pos, 50.0, 50.0, Genre::Caisse, c);
                }
                Outil_::Domino if rep.clicked() => {
                    let c = self.couleur();
                    self.monde.rectangle(pos, 12.0, 80.0, Genre::Domino, c);
                }
                Outil_::Mur | Outil_::Corde => {
                    if rep.drag_started() {
                        self.debut = Some(pos);
                    }
                    if rep.drag_stopped() {
                        if let Some(a) = self.debut.take() {
                            if (pos - a).length() > 10.0 {
                                if self.outil == Outil_::Mur {
                                    self.monde.murs.push((a, pos));
                                } else {
                                    self.monde.corde(a, pos, Color32::from_rgb(200, 170, 120));
                                }
                            }
                        }
                    }
                }
                Outil_::Ressort if rep.clicked() => match (self.ressort_de, self.monde.point_proche(pos, 25.0)) {
                    (None, Some(i)) => self.ressort_de = Some(i),
                    (Some(a), Some(b)) if a != b => {
                        self.monde.ressort(a, b);
                        self.ressort_de = None;
                    }
                    _ => self.ressort_de = None,
                },
                Outil_::Main => {
                    if rep.drag_started() {
                        self.attrape = self.monde.point_proche(pos, 30.0);
                    }
                    if let Some(i) = self.attrape {
                        if i < self.monde.points.len() && self.monde.points[i].inv_masse > 0.0 {
                            let p = &mut self.monde.points[i];
                            p.avant = p.p;
                            p.p = p.p + (pos - p.p) * 0.5;
                        }
                    }
                    if rep.drag_stopped() {
                        self.attrape = None;
                    }
                }
                Outil_::Gomme if rep.clicked() || rep.dragged() => {
                    if let Some(i) = self.monde.point_proche(pos, 15.0) {
                        self.monde.supprimer(i);
                    } else if let Some(k) = self.monde.murs.iter().position(|(a, b)| (proche_segment(pos, *a, *b).0 - pos).length() < 10.0) {
                        self.monde.murs.remove(k);
                    }
                }
                _ => {}
            }
        }
        if !self.pause {
            let dt = ui.input(|i| i.stable_dt).min(1.0 / 30.0);
            self.monde.avancer(if self.ralenti { dt / 4.0 } else { dt });
        }
        // dessin
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 6.0, Color32::from_rgb(22, 24, 34));
        let ecran = |v: Vec2| zone.min + v;
        for (a, b) in &self.monde.murs {
            p.line_segment([ecran(*a), ecran(*b)], Stroke::new(8.0_f32, Color32::from_rgb(140, 140, 160)));
        }
        if let (Some(a), Some(pos)) = (self.debut, rep.interact_pointer_pos()) {
            p.line_segment([ecran(a), pos], Stroke::new(3.0_f32, Color32::from_white_alpha(120)));
        }
        for c in &self.monde.corps {
            let pts: Vec<Pos2> = c.points.iter().map(|i| ecran(self.monde.points[*i].p)).collect();
            match c.genre {
                Genre::Balle => {
                    let q = self.monde.points[c.points[0]];
                    p.circle_filled(ecran(q.p), q.r, c.couleur);
                    // un reflet pour voir tourner
                    p.circle_filled(ecran(q.p) - Vec2::splat(q.r * 0.3), q.r * 0.25, Color32::from_white_alpha(70));
                }
                Genre::Caisse | Genre::Domino => {
                    p.add(egui::Shape::convex_polygon(pts.clone(), c.couleur, Stroke::new(1.5_f32, Color32::from_black_alpha(140))));
                }
                Genre::Corde => {
                    p.add(egui::Shape::line(pts.clone(), Stroke::new(3.0_f32, c.couleur)));
                    p.circle_filled(pts[0], 5.0, Color32::GRAY);
                    if let Some(i) = c.points.last() {
                        let q = self.monde.points[*i];
                        if q.r > 4.0 {
                            p.circle_filled(ecran(q.p), q.r, Color32::from_rgb(200, 200, 215));
                        }
                    }
                }
                Genre::Ressort => {
                    let (a, b) = (pts[0], pts[1]);
                    let d = b - a;
                    let n = Vec2::new(-d.y, d.x).normalized() * 7.0;
                    let zigzag: Vec<Pos2> = (0..=16).map(|k| a + d * (k as f32 / 16.0) + if k % 2 == 1 && k < 16 { n * if k % 4 == 1 { 1.0 } else { -1.0 } } else { Vec2::ZERO }).collect();
                    p.add(egui::Shape::line(zigzag, Stroke::new(2.0_f32, c.couleur)));
                }
            }
        }
        if let Some(i) = self.ressort_de {
            if let Some(q) = self.monde.points.get(i) {
                p.circle_stroke(ecran(q.p), q.r + 6.0, Stroke::new(2.0_f32, Color32::YELLOW));
            }
        }
        let aide = match self.outil {
            Outil_::Mur => "Glisse pour tracer un mur ou une rampe",
            Outil_::Corde => "Glisse depuis le point d'accroche",
            Outil_::Ressort => "Clique deux objets pour les relier par un ressort",
            Outil_::Main => "Attrape et lance les objets",
            Outil_::Gomme => "Clique ou glisse sur ce que tu veux enlever",
            _ => "Clique pour poser",
        };
        p.text(zone.left_top() + Vec2::new(10.0, 8.0), egui::Align2::LEFT_TOP, format!("{aide} · {} objets", self.monde.corps.len()), egui::FontId::proportional(13.0), Color32::from_white_alpha(140));
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physique() {
        let mut m = Monde::nouveau(Vec2::new(400.0, 300.0));
        m.rebond = 0.0;
        let b = m.balle(Vec2::new(100.0, 50.0), 10.0, Color32::RED);
        let c = m.rectangle(Vec2::new(250.0, 100.0), 40.0, 40.0, Genre::Caisse, Color32::BLUE);
        for _ in 0..400 {
            m.avancer(1.0 / 60.0);
        }
        // la balle et la caisse reposent sur le sol, sans le traverser
        let pb = m.points[m.corps[b].points[0]].p;
        assert!((pb.y - 290.0).abs() < 2.0, "{pb:?}");
        for &i in &m.corps[c].points {
            assert!(m.points[i].p.y <= 300.5);
        }
        // la caisse garde sa forme
        let pts: Vec<Vec2> = m.corps[c].points.iter().map(|i| m.points[*i].p).collect();
        assert!(((pts[0] - pts[1]).length() - 40.0).abs() < 1.0);
        assert!(((pts[0] - pts[2]).length() - 40.0 * 2f32.sqrt()).abs() < 1.5);
        assert!(m.agitation() < 0.05, "{}", m.agitation());
        // un mur incliné retient une balle
        let mut m = Monde::nouveau(Vec2::new(400.0, 300.0));
        m.murs.push((Vec2::new(0.0, 100.0), Vec2::new(400.0, 200.0)));
        let b = m.balle(Vec2::new(200.0, 50.0), 8.0, Color32::RED);
        for _ in 0..120 {
            m.avancer(1.0 / 60.0);
        }
        let p = m.points[m.corps[b].points[0]].p;
        let (q, _) = proche_segment(p, Vec2::new(0.0, 100.0), Vec2::new(400.0, 200.0));
        assert!(p.y < q.y, "la balle a traversé le mur");
        // la gomme enlève un objet et renumérote le reste
        let mut m = Monde::nouveau(Vec2::new(400.0, 300.0));
        m.balle(Vec2::new(50.0, 50.0), 10.0, Color32::RED);
        m.rectangle(Vec2::new(200.0, 50.0), 20.0, 20.0, Genre::Caisse, Color32::RED);
        m.corde(Vec2::new(300.0, 10.0), Vec2::new(300.0, 100.0), Color32::RED);
        m.supprimer(0);
        assert_eq!(m.corps.len(), 2);
        assert!(m.points.iter().all(|p| p.corps < 2));
        assert!(m.liens.iter().all(|l| l.a < m.points.len() && l.b < m.points.len()));
        m.avancer(0.1);
    }

    #[test]
    fn dominos_tombent() {
        let mut m = Monde::nouveau(Vec2::new(900.0, 500.0));
        let mut d = Vec::new();
        for k in 0..6 {
            d.push(m.rectangle(Vec2::new(300.0 + k as f32 * 50.0, 460.0), 12.0, 80.0, Genre::Domino, Color32::RED));
        }
        // on pousse le premier
        for &i in &m.corps[d[0]].points.clone() {
            if m.points[i].p.y < 450.0 {
                m.points[i].avant.x -= 6.0;
            }
        }
        for _ in 0..600 {
            m.avancer(1.0 / 60.0);
        }
        // le dernier domino est tombé : son haut est bas
        let haut = m.corps[d[5]].points.iter().map(|i| m.points[*i].p.y).fold(f32::MAX, f32::min);
        assert!(haut > 440.0, "le dernier domino est encore debout ({haut})");
    }
}
