//! Jeux du téléphone : labyrinthe à bille qu'on incline (16), face à face à deux sur le même écran (18),
//! succès et badges de tous les jeux (20).
use crate::android::{self, Capteurs};
use crate::commun::*;
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------------
// 16 · Labyrinthe à bille
// ---------------------------------------------------------------------------------------------------

/// Murs de chaque case : [haut, droite, bas, gauche].
pub type Murs = Vec<[bool; 4]>;

/// Labyrinthe parfait (un seul chemin entre deux cases), par exploration en profondeur.
pub fn generer(l: usize, h: usize, r: &mut impl FnMut(usize) -> usize) -> Murs {
    let mut m = vec![[true; 4]; l * h];
    let mut vu = vec![false; l * h];
    let mut pile = vec![0usize];
    vu[0] = true;
    while let Some(&c) = pile.last() {
        let (x, y) = (c % l, c / l);
        let mut v = vec![];
        if y > 0 && !vu[c - l] {
            v.push((0, c - l));
        }
        if x + 1 < l && !vu[c + 1] {
            v.push((1, c + 1));
        }
        if y + 1 < h && !vu[c + l] {
            v.push((2, c + l));
        }
        if x > 0 && !vu[c - 1] {
            v.push((3, c - 1));
        }
        if v.is_empty() {
            pile.pop();
            continue;
        }
        let (d, n) = v[r(v.len())];
        m[c][d] = false;
        m[n][(d + 2) % 4] = false;
        vu[n] = true;
        pile.push(n);
    }
    m
}

/// Toutes les cases sont-elles reliées à la première ?
pub fn connexe(m: &Murs, l: usize) -> bool {
    let mut vu = vec![false; m.len()];
    let mut pile = vec![0];
    vu[0] = true;
    while let Some(c) = pile.pop() {
        for (d, delta) in [(0, -(l as i64)), (1, 1), (2, l as i64), (3, -1)] {
            if !m[c][d] {
                let n = (c as i64 + delta) as usize;
                if !vu[n] {
                    vu[n] = true;
                    pile.push(n);
                }
            }
        }
    }
    vu.iter().all(|x| *x)
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct RecordsLaby {
    niveau_max: u32,
    temps: HashMap<u32, f32>,
}

pub struct Labyrinthe {
    c: Option<Capteurs>,
    vu: Instant,
    niveau: u32,
    murs: Murs,
    l: usize,
    h: usize,
    bille: Vec2, // en cases
    vitesse: Vec2,
    debut: Instant,
    gagne: Option<f32>,
    rec: RecordsLaby,
    dernier: Instant,
}

impl Labyrinthe {
    pub fn new() -> Self {
        let rec: RecordsLaby = charger("labyrinthe");
        let mut j = Labyrinthe { c: None, vu: Instant::now(), niveau: rec.niveau_max.max(1), murs: vec![], l: 0, h: 0, bille: Vec2::splat(0.5), vitesse: Vec2::ZERO, debut: Instant::now(), gagne: None, rec, dernier: Instant::now() };
        j.nouveau();
        j
    }

    fn nouveau(&mut self) {
        self.l = (4 + self.niveau as usize).min(16);
        self.h = (6 + self.niveau as usize * 3 / 2).min(26);
        self.murs = generer(self.l, self.h, &mut |n| hasard(n as u64) as usize);
        self.bille = Vec2::splat(0.5);
        self.vitesse = Vec2::ZERO;
        self.debut = Instant::now();
        self.gagne = None;
    }

    /// Avance la bille en glissant le long des murs.
    fn avancer(&mut self, acc: Vec2, dt: f32) {
        self.vitesse = (self.vitesse + acc * dt * 1.6) * 0.985;
        self.vitesse = self.vitesse.clamp(Vec2::splat(-6.0), Vec2::splat(6.0));
        let r = 0.28;
        let pas = (self.vitesse.length() * dt / 0.1).ceil().max(1.0) as usize;
        for _ in 0..pas {
            let d = self.vitesse * dt / pas as f32;
            for axe in 0..2 {
                let mut p = self.bille;
                if axe == 0 { p.x += d.x } else { p.y += d.y }
                let (cx, cy) = (self.bille.x.floor() as usize, self.bille.y.floor() as usize);
                let c = cy.min(self.h - 1) * self.l + cx.min(self.l - 1);
                let bloque = if axe == 0 {
                    (d.x > 0.0 && self.murs[c][1] && p.x + r > (cx + 1) as f32) || (d.x < 0.0 && self.murs[c][3] && p.x - r < cx as f32)
                } else {
                    (d.y > 0.0 && self.murs[c][2] && p.y + r > (cy + 1) as f32) || (d.y < 0.0 && self.murs[c][0] && p.y - r < cy as f32)
                };
                if bloque {
                    if axe == 0 { self.vitesse.x *= -0.3 } else { self.vitesse.y *= -0.3 }
                } else {
                    self.bille = p;
                }
            }
            self.bille.x = self.bille.x.clamp(r, self.l as f32 - r);
            self.bille.y = self.bille.y.clamp(r, self.h as f32 - r);
        }
    }
}

impl Outil for Labyrinthe {
    fn nom(&self) -> &'static str {
        "🎱 Labyrinthe à bille"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.vu.elapsed() > Duration::from_millis(800) {
            self.c = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let c = self.c.get_or_insert_with(|| Capteurs::demarrer(true, false, false));
        let m = *c.m.lock().unwrap();
        let dt = self.dernier.elapsed().as_secs_f32().min(0.05);
        self.dernier = Instant::now();
        // inclinaison (téléphone tenu en portrait) ; flèches du clavier pour essayer sur PC
        let mut acc = if cfg!(target_os = "android") { Vec2::new(-m.acc[0], m.acc[1]) } else { Vec2::ZERO };
        ui.input(|i| {
            for (k, v) in [(egui::Key::ArrowLeft, Vec2::new(-6.0, 0.0)), (egui::Key::ArrowRight, Vec2::new(6.0, 0.0)), (egui::Key::ArrowUp, Vec2::new(0.0, -6.0)), (egui::Key::ArrowDown, Vec2::new(0.0, 6.0))] {
                if i.key_down(k) {
                    acc += v;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Niveau {}", self.niveau)).size(20.0).strong());
            let t = self.gagne.unwrap_or(self.debut.elapsed().as_secs_f32());
            ui.label(RichText::new(format!("⏱ {t:.1} s")).size(18.0));
            if let Some(r) = self.rec.temps.get(&self.niveau) {
                ui.weak(format!("record {r:.1} s"));
            }
        });
        if self.gagne.is_none() {
            self.avancer(acc, dt);
            let sortie = Vec2::new(self.l as f32 - 0.5, self.h as f32 - 0.5);
            if (self.bille - sortie).length() < 0.4 {
                let t = self.debut.elapsed().as_secs_f32();
                self.gagne = Some(t);
                android::vibrer(150);
                let r = self.rec.temps.entry(self.niveau).or_insert(f32::MAX);
                *r = r.min(t);
                self.rec.niveau_max = self.rec.niveau_max.max(self.niveau + 1);
                enregistrer("labyrinthe", &self.rec);
            }
        }
        let w = ui.available_width();
        let haut = ui.ctx().screen_rect().height() * 0.68;
        let case = (w / self.l as f32).min(haut / self.h as f32).max(8.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(case * self.l as f32, case * self.h as f32), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(zone, 6.0, Color32::from_rgb(30, 24, 40));
        let sortie = Rect::from_min_size(zone.min + Vec2::new((self.l - 1) as f32 * case, (self.h - 1) as f32 * case), Vec2::splat(case));
        p.rect_filled(sortie.shrink(3.0), 4.0, VERT.gamma_multiply(0.6));
        let trait_ = Stroke::new((case / 8.0).max(2.0), Color32::from_rgb(230, 200, 255));
        for y in 0..self.h {
            for x in 0..self.l {
                let m = self.murs[y * self.l + x];
                let o = zone.min + Vec2::new(x as f32 * case, y as f32 * case);
                if m[0] {
                    p.line_segment([o, o + Vec2::new(case, 0.0)], trait_);
                }
                if m[3] {
                    p.line_segment([o, o + Vec2::new(0.0, case)], trait_);
                }
                if y == self.h - 1 && m[2] {
                    p.line_segment([o + Vec2::new(0.0, case), o + Vec2::new(case, case)], trait_);
                }
                if x == self.l - 1 && m[1] {
                    p.line_segment([o + Vec2::new(case, 0.0), o + Vec2::new(case, case)], trait_);
                }
            }
        }
        p.circle_filled(zone.min + self.bille * case, case * 0.28, ROSE);
        if let Some(t) = self.gagne {
            p.text(zone.center(), Align2::CENTER_CENTER, format!("Bravo ! {t:.1} s"), FontId::proportional(32.0), Color32::WHITE);
            ui.horizontal(|ui| {
                if ui.button(RichText::new("Niveau suivant ➜").size(18.0)).clicked() {
                    self.niveau += 1;
                    self.nouveau();
                }
                if ui.button("Rejouer").clicked() {
                    self.nouveau();
                }
            });
        } else {
            ui.ctx().request_repaint();
            ui.horizontal(|ui| {
                ui.weak("Incline le téléphone pour guider la bille jusqu'à la case verte.");
                if self.niveau > 1 && ui.small_button("◀ niveau").clicked() {
                    self.niveau -= 1;
                    self.nouveau();
                }
                if self.niveau < self.rec.niveau_max && ui.small_button("niveau ▶").clicked() {
                    self.niveau += 1;
                    self.nouveau();
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 18 · Face à face : réflexes en duel et air hockey (multi-touch)
// ---------------------------------------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
enum Duel {
    Reflexes,
    Hockey,
}

pub struct FaceAFace {
    jeu: Duel,
    score: [u32; 2],
    // réflexes
    signal: Option<Instant>,
    attente: Instant,
    message: String,
    // hockey
    doigts: HashMap<u64, Pos2>,
    raquettes: [Vec2; 2], // en fraction de la zone (0..1)
    palet: Vec2,
    v: Vec2,
    ordi: bool,
    dernier: Instant,
}

impl FaceAFace {
    pub fn new() -> Self {
        FaceAFace {
            jeu: Duel::Reflexes, score: [0; 2], signal: None, attente: Instant::now() + Duration::from_secs(2), message: "Chacun pose un doigt près de son côté…".into(),
            doigts: HashMap::new(), raquettes: [Vec2::new(0.5, 0.85), Vec2::new(0.5, 0.15)], palet: Vec2::new(0.5, 0.5), v: Vec2::ZERO, ordi: false, dernier: Instant::now(),
        }
    }

    fn manche(&mut self) {
        self.signal = None;
        self.attente = Instant::now() + Duration::from_millis(1500 + hasard(3500));
        self.message = "Attendez le signal…".into();
    }
}

/// Positions des doigts posés sur l'écran (et de la souris, pour essayer sur PC).
pub fn appuis(ui: &egui::Ui, doigts: &mut HashMap<u64, Pos2>) -> Vec<Pos2> {
    let mut nouveaux = vec![];
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Touch { id, phase, pos, .. } => match phase {
                    egui::TouchPhase::Start => {
                        doigts.insert(id.0, *pos);
                        nouveaux.push(*pos);
                    }
                    egui::TouchPhase::Move => {
                        doigts.insert(id.0, *pos);
                    }
                    _ => {
                        doigts.remove(&id.0);
                    }
                },
                egui::Event::PointerButton { pos, pressed: true, .. } => nouveaux.push(*pos),
                _ => {}
            }
        }
        if !i.any_touches() {
            doigts.clear();
            if let Some(p) = i.pointer.latest_pos().filter(|_| i.pointer.primary_down()) {
                doigts.insert(u64::MAX, p);
            }
        }
    });
    nouveaux
}

impl Outil for FaceAFace {
    fn nom(&self) -> &'static str {
        "🤜 Face à face"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.selectable_label(self.jeu == Duel::Reflexes, RichText::new("⚡ Réflexes").size(17.0)).clicked() {
                self.jeu = Duel::Reflexes;
                self.score = [0; 2];
                self.manche();
            }
            if ui.selectable_label(self.jeu == Duel::Hockey, RichText::new("🏒 Air hockey").size(17.0)).clicked() {
                self.jeu = Duel::Hockey;
                self.score = [0; 2];
                self.palet = Vec2::new(0.5, 0.5);
                self.v = Vec2::ZERO;
            }
            if self.jeu == Duel::Hockey {
                ui.checkbox(&mut self.ordi, "contre le téléphone");
            }
        });
        ui.weak("Posez le téléphone à plat entre vous deux : chacun joue de son côté de l'écran.");
        let h = (ui.available_height() - 10.0).max(420.0).min(700.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click_and_drag());
        let milieu = zone.center().y;
        let nouveaux = appuis(ui, &mut self.doigts);
        let p = ui.painter_at(zone);
        ui.ctx().request_repaint();
        match self.jeu {
            Duel::Reflexes => {
                let maintenant = Instant::now();
                if self.signal.is_none() && maintenant >= self.attente && self.message == "Attendez le signal…" {
                    self.signal = Some(maintenant);
                }
                let fond = if self.signal.is_some() { VERT } else { Color32::from_rgb(60, 30, 50) };
                p.rect_filled(zone, 12.0, fond);
                p.line_segment([Pos2::new(zone.left(), milieu), Pos2::new(zone.right(), milieu)], Stroke::new(3.0_f32, Color32::WHITE));
                for (k, centre) in [(0, Pos2::new(zone.center().x, (milieu + zone.bottom()) / 2.0)), (1, Pos2::new(zone.center().x, (zone.top() + milieu) / 2.0))] {
                    p.text(centre, Align2::CENTER_CENTER, self.score[k].to_string(), FontId::proportional(60.0), Color32::WHITE);
                }
                // le joueur du haut lit à l'envers
                p.text(Pos2::new(zone.center().x, milieu + 30.0), Align2::CENTER_CENTER, &self.message, FontId::proportional(20.0), Color32::WHITE);
                if self.signal.is_some() {
                    p.text(Pos2::new(zone.center().x, milieu - 30.0), Align2::CENTER_CENTER, "TOUCHE !", FontId::proportional(28.0), Color32::BLACK);
                }
                if let Some(a) = nouveaux.first().filter(|a| zone.contains(**a)) {
                    let joueur = if a.y > milieu { 0 } else { 1 };
                    let nom = if joueur == 0 { "Le joueur du bas" } else { "Le joueur du haut" };
                    let attend = self.message == "Attendez le signal…";
                    match self.signal {
                        Some(s) => {
                            self.score[joueur] += 1;
                            self.message = format!("{nom} gagne en {} ms ! Touchez pour rejouer", s.elapsed().as_millis());
                            android::vibrer(120);
                            self.signal = None;
                        }
                        None if attend => {
                            self.score[1 - joueur] += 1;
                            self.message = format!("{nom} est parti trop tôt ! Touchez pour rejouer");
                        }
                        None => self.manche(),
                    }
                }
            }
            Duel::Hockey => {
                let dt = self.dernier.elapsed().as_secs_f32().min(0.03);
                self.dernier = Instant::now();
                let taille = zone.size();
                let vers = |v: Vec2| zone.min + Vec2::new(v.x * taille.x, v.y * taille.y);
                // chaque doigt pilote la raquette de son côté
                for pos in self.doigts.values() {
                    if !zone.contains(*pos) {
                        continue;
                    }
                    let f = Vec2::new((pos.x - zone.left()) / taille.x, (pos.y - zone.top()) / taille.y);
                    if f.y > 0.5 {
                        self.raquettes[0] = Vec2::new(f.x, f.y.max(0.55));
                    } else if !self.ordi {
                        self.raquettes[1] = Vec2::new(f.x, f.y.min(0.45));
                    }
                }
                if self.ordi {
                    let cible = Vec2::new(self.palet.x, if self.palet.y < 0.5 { self.palet.y.min(0.42) } else { 0.12 });
                    self.raquettes[1] += (cible - self.raquettes[1]) * (dt * 4.0).min(1.0);
                }
                // palet
                self.palet += self.v * dt;
                self.v *= 0.995;
                if self.palet.x < 0.04 || self.palet.x > 0.96 {
                    self.v.x = -self.v.x;
                    self.palet.x = self.palet.x.clamp(0.04, 0.96);
                }
                let but = (0.32..0.68).contains(&self.palet.x);
                if self.palet.y < 0.02 || self.palet.y > 0.98 {
                    if but {
                        let marqueur = if self.palet.y < 0.5 { 0 } else { 1 };
                        self.score[marqueur] += 1;
                        android::vibrer(200);
                        self.palet = Vec2::new(0.5, 0.5);
                        self.v = Vec2::new(0.0, if marqueur == 0 { -0.2 } else { 0.2 });
                    } else {
                        self.v.y = -self.v.y;
                        self.palet.y = self.palet.y.clamp(0.02, 0.98);
                    }
                }
                let ratio = taille.y / taille.x;
                for r in self.raquettes {
                    let mut d = self.palet - r;
                    d.y *= ratio;
                    if d.length() < 0.11 {
                        let n = d.normalized();
                        let vitesse = self.v.length().max(0.8) * 1.05;
                        self.v = Vec2::new(n.x, n.y / ratio).normalized() * vitesse.min(2.5);
                        self.palet = r + Vec2::new(n.x * 0.11, n.y * 0.11 / ratio);
                        android::vibrer(15);
                    }
                }
                p.rect_filled(zone, 16.0, Color32::from_rgb(20, 60, 90));
                p.line_segment([Pos2::new(zone.left(), milieu), Pos2::new(zone.right(), milieu)], Stroke::new(2.0_f32, Color32::from_white_alpha(80)));
                p.circle_stroke(zone.center(), taille.x * 0.15, Stroke::new(2.0_f32, Color32::from_white_alpha(80)));
                for y in [zone.top(), zone.bottom()] {
                    p.line_segment([Pos2::new(zone.left() + 0.32 * taille.x, y), Pos2::new(zone.left() + 0.68 * taille.x, y)], Stroke::new(8.0_f32, OR));
                }
                for (k, c) in [(0, ROSE), (1, Color32::from_rgb(90, 170, 255))] {
                    p.circle_filled(vers(self.raquettes[k]), taille.x * 0.07, c);
                    p.text(Pos2::new(zone.right() - 30.0, if k == 0 { milieu + 40.0 } else { milieu - 40.0 }), Align2::CENTER_CENTER, self.score[k].to_string(), FontId::proportional(36.0), Color32::WHITE);
                }
                p.circle_filled(vers(self.palet), taille.x * 0.04, Color32::WHITE);
            }
        }
    }
}

const OR: Color32 = Color32::from_rgb(250, 210, 90);

// ---------------------------------------------------------------------------------------------------
// 20 · Succès et badges
// ---------------------------------------------------------------------------------------------------

/// Valeur numérique d'un fichier de records : `chemin` vide = la valeur elle-même (ou le 1er élément).
pub fn valeur(j: &Value, chemin: &[&str]) -> Option<f64> {
    let mut v = j;
    for c in chemin {
        v = v.get(c)?;
    }
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Array(a) => a.first().and_then(|x| x.as_f64()),
        Value::Bool(b) => Some(*b as u8 as f64),
        _ => None,
    }
}

/// (id, titre, description, fichier, chemin, seuil, plus petit = mieux)
pub const BADGES: [(&str, &str, &str, &str, &[&str], f64, bool); 22] = [
    ("serpent20", "🐍 Charmeur de serpent", "20 points au serpent", "minijeux", &["serpent"], 20.0, false),
    ("serpent50", "🐉 Dompteur de dragon", "50 points au serpent", "minijeux", &["serpent"], 50.0, false),
    ("briques", "🧱 Démolisseur", "300 points au casse-briques", "minijeux", &["briques"], 300.0, false),
    ("demineur", "💣 Démineur express", "démineur en moins de 2 minutes", "minijeux", &["demineur"], 120.0, true),
    ("fusion", "🔢 Fusionneur", "2048 au jeu de fusion", "minijeux2", &["fusion"], 2048.0, false),
    ("blocs", "🟦 Maître des blocs", "40 lignes aux blocs qui tombent", "minijeux2", &["blocs"], 40.0, false),
    ("oiseau", "🐦 Pilote", "15 tuyaux passés avec l'oiseau", "minijeux2", &["oiseau"], 15.0, false),
    ("p4", "🔴 Stratège", "10 victoires au 4 en ligne", "minijeux2", &["victoires4"], 10.0, false),
    ("sudoku1", "📅 Premier sudoku", "réussir un sudoku du jour", "sudoku_jour", &["total"], 1.0, false),
    ("sudoku7", "🔥 Une semaine de sudoku", "7 jours de suite au sudoku du jour", "sudoku_jour", &["serie"], 7.0, false),
    ("sudoku_vite", "⚡ Sudoku éclair", "un sudoku du jour en moins de 5 minutes", "sudoku_jour", &["meilleur"], 300.0, true),
    ("quiz", "🌍 Globe-trotter", "10/10 au quiz des drapeaux", "quiz_drapeaux", &["drapeaux"], 10.0, false),
    ("capitales", "🏛 Géographe", "10/10 au quiz des capitales", "quiz_drapeaux", &["capitales"], 10.0, false),
    ("echecs", "♟ Grand maître", "battre l'ordi aux échecs", "echecs_victoires", &[], 1.0, false),
    ("solitaire", "🃏 Patient", "réussir un solitaire", "solitaire_victoires", &[], 1.0, false),
    ("reflexes", "⚡ Réflexes de ninja", "réagir en moins de 250 ms", "reflexes", &[], 250.0, true),
    ("frappe", "⌨ Doigts de fée", "60 mots par minute", "frappe", &[], 60.0, false),
    ("dango", "🍡 Gourmand", "1 million de dangos au total", "clicker", &["total"], 1_000_000.0, false),
    ("laby5", "🎱 Explorateur", "atteindre le niveau 5 du labyrinthe", "labyrinthe", &["niveau_max"], 5.0, false),
    ("laby10", "🗺 Cartographe", "atteindre le niveau 10 du labyrinthe", "labyrinthe", &["niveau_max"], 10.0, false),
    ("pas", "👟 Marcheur", "10 000 pas en une journée", "podometre", &["aujourdhui"], 10_000.0, false),
    ("pc", "🖥 Connecté", "associer le téléphone à son PC", "pc", &["port"], 1.0, false),
];

pub fn obtenu(j: &Value, chemin: &[&str], seuil: f64, inverse: bool) -> bool {
    match valeur(j, chemin) {
        Some(v) if inverse => v > 0.0 && v <= seuil,
        Some(v) => v >= seuil,
        None => false,
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Gagnes {
    l: HashMap<String, u64>,
}

pub struct Badges {
    g: Gagnes,
    verif: Instant,
}

impl Badges {
    pub fn new() -> Self {
        Badges { g: charger("badges"), verif: Instant::now() }
    }

    fn verifier(&mut self) {
        let mut nouveaux = vec![];
        let mut cache: HashMap<&str, Value> = HashMap::new();
        for (id, titre, _, fichier, chemin, seuil, inverse) in BADGES {
            if self.g.l.contains_key(id) {
                continue;
            }
            let j = cache.entry(fichier).or_insert_with(|| charger::<Value>(fichier));
            if obtenu(j, chemin, seuil, inverse) {
                self.g.l.insert(id.to_string(), maintenant());
                nouveaux.push(titre);
            }
        }
        if !nouveaux.is_empty() {
            enregistrer("badges", &self.g);
            for t in nouveaux {
                android::notifier(5000 + (hasard(900) as i32), "🏅 Nouveau badge !", t);
                crate::systeme::notifier_toujours("🏅 Nouveau badge !", t);
            }
        }
    }
}

impl Outil for Badges {
    fn nom(&self) -> &'static str {
        "🏅 Succès et badges"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.verif.elapsed() > Duration::from_secs(5) {
            self.verif = Instant::now();
            self.verifier();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let n = self.g.l.len();
        ui.label(RichText::new(format!("{n} / {} badges", BADGES.len())).size(24.0).strong().color(ROSE));
        ui.add(egui::ProgressBar::new(n as f32 / BADGES.len() as f32).fill(ROSE));
        for (id, titre, desc, ..) in BADGES {
            let quand = self.g.l.get(id);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    let c = if quand.is_some() { ui.visuals().strong_text_color() } else { Color32::GRAY };
                    ui.vertical(|ui| {
                        ui.label(RichText::new(if quand.is_some() { titre.to_string() } else { format!("🔒 {}", titre.split_once(' ').map_or(titre, |x| x.1)) }).size(17.0).strong().color(c));
                        ui.weak(desc);
                    });
                    if let Some(t) = quand {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.weak(date(*t));
                        });
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labyrinthes_et_badges() {
        let mut s = 7u64;
        let mut r = |n: usize| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % n as u64) as usize
        };
        for (l, h) in [(5, 8), (12, 20), (16, 26)] {
            let m = generer(l, h, &mut r);
            assert!(connexe(&m, l));
            // labyrinthe parfait : exactement (cases − 1) passages
            let passages: usize = m.iter().map(|c| c.iter().filter(|x| !**x).count()).sum::<usize>() / 2;
            assert_eq!(passages, l * h - 1);
            // bords fermés
            assert!((0..l).all(|x| m[x][0] && m[(h - 1) * l + x][2]));
        }
        let j: Value = serde_json::json!({ "serpent": 25, "demineur": 90, "frappe": [72.0, []] });
        assert!(obtenu(&j, &["serpent"], 20.0, false));
        assert!(!obtenu(&j, &["serpent"], 50.0, false));
        assert!(obtenu(&j, &["demineur"], 120.0, true));
        assert!(obtenu(&j["frappe"], &[], 60.0, false));
        assert!(!obtenu(&Value::Null, &["x"], 1.0, false));
        assert!(obtenu(&serde_json::json!(3), &[], 1.0, false));
    }
}
