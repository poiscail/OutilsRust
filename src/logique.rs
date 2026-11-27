//! Générateur de labyrinthes (à jouer ou à imprimer, 5) et « coder avec des blocs » : une tortue qui
//! dessine (10).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Instant;

// ---------------------------------------------------------------------------------------------------
// 5 · Labyrinthes
// ---------------------------------------------------------------------------------------------------

/// Murs de chaque case : haut, droite, bas, gauche.
#[derive(Clone)]
pub struct Labyrinthe {
    pub l: usize,
    pub h: usize,
    pub murs: Vec<[bool; 4]>,
}

const DIRS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

struct Alea(u64);
impl Alea {
    fn suivant(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

impl Labyrinthe {
    /// Labyrinthe parfait (un seul chemin entre deux cases), par exploration en profondeur.
    /// `sinueux` (0..1) : plus il est grand, plus les couloirs sont longs et tordus.
    pub fn generer(l: usize, h: usize, graine: u64, sinueux: f32) -> Self {
        let mut m = Labyrinthe { l, h, murs: vec![[true; 4]; l * h] };
        let mut vu = vec![false; l * h];
        let mut r = Alea(graine.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut pile = vec![0usize];
        vu[0] = true;
        while !pile.is_empty() {
            // « sinueux » : on continue depuis la dernière case ; sinon parfois depuis une case au hasard
            let k = if (r.suivant(1000) as f32) < sinueux * 1000.0 { pile.len() - 1 } else { r.suivant(pile.len()) };
            let c = pile[k];
            let (x, y) = ((c % l) as i32, (c / l) as i32);
            let voisins: Vec<usize> = (0..4).filter(|&d| {
                let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                nx >= 0 && ny >= 0 && (nx as usize) < l && (ny as usize) < h && !vu[ny as usize * l + nx as usize]
            }).collect();
            if voisins.is_empty() {
                pile.remove(k);
                continue;
            }
            let d = voisins[r.suivant(voisins.len())];
            let n = ((y + DIRS[d].1) as usize) * l + (x + DIRS[d].0) as usize;
            m.murs[c][d] = false;
            m.murs[n][(d + 2) % 4] = false;
            vu[n] = true;
            pile.push(n);
        }
        m
    }

    pub fn ouvert(&self, c: usize, d: usize) -> bool {
        !self.murs[c][d]
    }

    /// Chemin le plus court de l'entrée (haut-gauche) à la sortie (bas-droite).
    pub fn solution(&self) -> Vec<usize> {
        let fin = self.l * self.h - 1;
        let mut avant = vec![usize::MAX; self.l * self.h];
        let mut file = VecDeque::from([0usize]);
        avant[0] = 0;
        while let Some(c) = file.pop_front() {
            if c == fin {
                break;
            }
            let (x, y) = ((c % self.l) as i32, (c / self.l) as i32);
            for d in 0..4 {
                if self.ouvert(c, d) {
                    let n = ((y + DIRS[d].1) as usize) * self.l + (x + DIRS[d].0) as usize;
                    if avant[n] == usize::MAX {
                        avant[n] = c;
                        file.push_back(n);
                    }
                }
            }
        }
        let mut v = vec![fin];
        while *v.last().unwrap() != 0 {
            let p = avant[*v.last().unwrap()];
            if p == usize::MAX {
                return Vec::new();
            }
            v.push(p);
        }
        v.reverse();
        v
    }

    /// Image (pour imprimer) : `case` pixels par case.
    pub fn image(&self, case: u32, avec_solution: bool) -> RgbaImage {
        let marge = case;
        let (w, hh) = (self.l as u32 * case + 2 * marge, self.h as u32 * case + 2 * marge);
        let mut img = RgbaImage::from_pixel(w, hh, Rgba([255, 255, 255, 255]));
        let ep = (case / 8).max(2) as f32;
        let noir = Rgba([0, 0, 0, 255]);
        if avec_solution {
            for c in self.solution() {
                let (x, y) = ((c % self.l) as f32, (c / self.l) as f32);
                crate::atelier::rectangle(&mut img, marge as f32 + x * case as f32 + case as f32 * 0.3, marge as f32 + y * case as f32 + case as f32 * 0.3, case as f32 * 0.4, case as f32 * 0.4, Rgba([230, 60, 60, 255]));
            }
        }
        for y in 0..self.h {
            for x in 0..self.l {
                let c = y * self.l + x;
                let (px, py) = ((marge + x as u32 * case) as f32, (marge + y as u32 * case) as f32);
                let m = self.murs[c];
                // entrée (en haut à gauche) et sortie (en bas à droite) ouvertes
                if m[0] && !(x == 0 && y == 0) {
                    crate::atelier::rectangle(&mut img, px, py, case as f32 + ep, ep, noir);
                }
                if m[3] {
                    crate::atelier::rectangle(&mut img, px, py, ep, case as f32 + ep, noir);
                }
                if y == self.h - 1 && m[2] && x != self.l - 1 {
                    crate::atelier::rectangle(&mut img, px, py + case as f32, case as f32 + ep, ep, noir);
                }
                if x == self.l - 1 && m[1] {
                    crate::atelier::rectangle(&mut img, px + case as f32, py, ep, case as f32 + ep, noir);
                }
            }
        }
        img
    }
}

pub struct Labyrinthes {
    taille: usize,
    sinueux: f32,
    graine: u64,
    lab: Labyrinthe,
    joueur: usize,
    debut: Option<Instant>,
    fini: Option<f32>,
    solution: bool,
    traces: Vec<usize>,
    message: String,
}

impl Labyrinthes {
    pub fn new() -> Self {
        let graine = crate::outil::hasard(u64::MAX);
        let lab = Labyrinthe::generer(20, 14, graine, 0.85);
        Labyrinthes { taille: 20, sinueux: 0.85, graine, lab, joueur: 0, debut: None, fini: None, solution: false, traces: vec![0], message: String::new() }
    }

    fn nouveau(&mut self) {
        self.graine = crate::outil::hasard(u64::MAX);
        let h = (self.taille as f32 * 0.7).round() as usize;
        self.lab = Labyrinthe::generer(self.taille, h.max(3), self.graine, self.sinueux);
        self.joueur = 0;
        self.debut = None;
        self.fini = None;
        self.traces = vec![0];
    }
}

impl Outil for Labyrinthes {
    fn nom(&self) -> &'static str {
        "🌀 Labyrinthes"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Labyrinthes");
            ui.label("Taille");
            if ui.add(egui::Slider::new(&mut self.taille, 5..=80)).changed() {
                self.nouveau();
            }
            ui.label("Couloirs");
            if ui.add(egui::Slider::new(&mut self.sinueux, 0.0..=1.0).custom_formatter(|v, _| if v > 0.7 { "longs".into() } else if v > 0.35 { "moyens".into() } else { "courts".into() })).drag_stopped() {
                self.nouveau();
            }
            if ui.button("🎲 Nouveau").clicked() {
                self.nouveau();
            }
            ui.checkbox(&mut self.solution, "Solution");
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("🖨 PDF à imprimer (+ solution)…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("labyrinthe.pdf").save_file() {
                    let case = (2000 / self.lab.l.max(self.lab.h) as u32).max(10);
                    let pages = [self.lab.image(case, false), self.lab.image(case, true)];
                    self.message = std::fs::write(&p, crate::atelier::pdf_images(&pages)).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("💾 Image…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("labyrinthe.png").save_file() {
                    self.message = self.lab.image(30, self.solution).save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            ui.weak("Joue avec les flèches (ou ZQSD) : va de la case verte à la sortie.");
            ui.label(&self.message);
        });
        // déplacements
        if self.fini.is_none() {
            for (touches, d) in [([egui::Key::ArrowUp, egui::Key::Z], 0usize), ([egui::Key::ArrowRight, egui::Key::D], 1), ([egui::Key::ArrowDown, egui::Key::S], 2), ([egui::Key::ArrowLeft, egui::Key::Q], 3)] {
                if touches.iter().any(|k| ui.input(|i| i.key_pressed(*k))) && self.lab.ouvert(self.joueur, d) {
                    let (x, y) = ((self.joueur % self.lab.l) as i32, (self.joueur / self.lab.l) as i32);
                    let (nx, ny) = (x + DIRS[d].0, y + DIRS[d].1);
                    if nx >= 0 && ny >= 0 && (nx as usize) < self.lab.l && (ny as usize) < self.lab.h {
                        self.joueur = ny as usize * self.lab.l + nx as usize;
                        self.debut.get_or_insert_with(Instant::now);
                        if self.traces.last() != Some(&self.joueur) {
                            self.traces.push(self.joueur);
                        }
                        if self.joueur == self.lab.l * self.lab.h - 1 {
                            self.fini = self.debut.map(|d| d.elapsed().as_secs_f32());
                        }
                    }
                }
            }
        }
        let (l, h) = (self.lab.l as f32, self.lab.h as f32);
        let dispo = ui.available_size() - Vec2::new(0.0, 30.0);
        let case = (dispo.x / l).min(dispo.y / h).max(4.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(case * l, case * h), Sense::hover());
        let p = ui.painter_at(zone.expand(3.0));
        p.rect_filled(zone, 0.0, Color32::from_rgb(245, 242, 232));
        let centre = |c: usize| zone.min + Vec2::new(((c % self.lab.l) as f32 + 0.5) * case, ((c / self.lab.l) as f32 + 0.5) * case);
        if self.solution {
            let s: Vec<Pos2> = self.lab.solution().into_iter().map(centre).collect();
            p.add(egui::Shape::line(s, Stroke::new((case * 0.25).max(1.5), Color32::from_rgba_unmultiplied(230, 60, 60, 170))));
        }
        let t: Vec<Pos2> = self.traces.iter().map(|c| centre(*c)).collect();
        p.add(egui::Shape::line(t, Stroke::new((case * 0.18).max(1.0), Color32::from_rgba_unmultiplied(60, 140, 230, 140))));
        let mur = Stroke::new((case / 7.0).max(1.5), Color32::from_rgb(30, 30, 40));
        for y in 0..self.lab.h {
            for x in 0..self.lab.l {
                let m = self.lab.murs[y * self.lab.l + x];
                let o = zone.min + Vec2::new(x as f32 * case, y as f32 * case);
                if m[0] && !(x == 0 && y == 0) {
                    p.line_segment([o, o + Vec2::new(case, 0.0)], mur);
                }
                if m[3] {
                    p.line_segment([o, o + Vec2::new(0.0, case)], mur);
                }
                if y == self.lab.h - 1 && m[2] && x != self.lab.l - 1 {
                    p.line_segment([o + Vec2::new(0.0, case), o + Vec2::new(case, case)], mur);
                }
                if x == self.lab.l - 1 && m[1] {
                    p.line_segment([o + Vec2::new(case, 0.0), o + Vec2::new(case, case)], mur);
                }
            }
        }
        p.circle_filled(centre(self.lab.l * self.lab.h - 1), case * 0.3, Color32::from_rgb(250, 190, 40));
        p.circle_filled(centre(self.joueur), case * 0.32, Color32::from_rgb(40, 170, 80));
        match self.fini {
            Some(s) => {
                ui.label(RichText::new(format!("🎉 Sorti en {s:.1} s, en {} pas (le plus court : {}).", self.traces.len() - 1, self.lab.solution().len() - 1)).size(18.0).color(Color32::GOLD));
            }
            None => {
                if let Some(d) = self.debut {
                    ui.label(format!("⏱ {:.0} s", d.elapsed().as_secs_f32()));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 10 · Coder avec des blocs : la tortue qui dessine
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Bloc {
    Avancer(f32),
    Reculer(f32),
    Droite(f32),
    Gauche(f32),
    Repeter(u32, Vec<Bloc>),
    Couleur([u8; 3]),
    ArcEnCiel,
    Epaisseur(f32),
    Lever,
    Baisser,
    Centre,
}

impl Bloc {
    fn nom(&self) -> String {
        match self {
            Bloc::Avancer(n) => format!("Avancer de {n}"),
            Bloc::Reculer(n) => format!("Reculer de {n}"),
            Bloc::Droite(a) => format!("Tourner à droite de {a}°"),
            Bloc::Gauche(a) => format!("Tourner à gauche de {a}°"),
            Bloc::Repeter(n, _) => format!("Répéter {n} fois"),
            Bloc::Couleur(_) => "Couleur du crayon".into(),
            Bloc::ArcEnCiel => "Couleur suivante de l'arc-en-ciel".into(),
            Bloc::Epaisseur(e) => format!("Épaisseur {e}"),
            Bloc::Lever => "Lever le crayon".into(),
            Bloc::Baisser => "Baisser le crayon".into(),
            Bloc::Centre => "Revenir au centre".into(),
        }
    }
    fn couleur(&self) -> Color32 {
        match self {
            Bloc::Avancer(_) | Bloc::Reculer(_) => Color32::from_rgb(70, 130, 220),
            Bloc::Droite(_) | Bloc::Gauche(_) => Color32::from_rgb(140, 90, 210),
            Bloc::Repeter(..) => Color32::from_rgb(230, 160, 40),
            Bloc::Centre => Color32::from_rgb(90, 170, 120),
            _ => Color32::from_rgb(200, 80, 130),
        }
    }
    /// Équivalent en Python (module turtle), pour apprendre à écrire du vrai code.
    fn python(&self, retrait: usize, s: &mut String) {
        let r = "    ".repeat(retrait);
        match self {
            Bloc::Avancer(n) => s.push_str(&format!("{r}forward({n})\n")),
            Bloc::Reculer(n) => s.push_str(&format!("{r}backward({n})\n")),
            Bloc::Droite(a) => s.push_str(&format!("{r}right({a})\n")),
            Bloc::Gauche(a) => s.push_str(&format!("{r}left({a})\n")),
            Bloc::Repeter(n, v) => {
                s.push_str(&format!("{r}for i in range({n}):\n"));
                if v.is_empty() {
                    s.push_str(&format!("{r}    pass\n"));
                }
                for b in v {
                    b.python(retrait + 1, s);
                }
            }
            Bloc::Couleur(c) => s.push_str(&format!("{r}pencolor(\"#{:02x}{:02x}{:02x}\")\n", c[0], c[1], c[2])),
            Bloc::ArcEnCiel => s.push_str(&format!("{r}pencolor(couleur_suivante())\n")),
            Bloc::Epaisseur(e) => s.push_str(&format!("{r}pensize({e})\n")),
            Bloc::Lever => s.push_str(&format!("{r}penup()\n")),
            Bloc::Baisser => s.push_str(&format!("{r}pendown()\n")),
            Bloc::Centre => s.push_str(&format!("{r}home()\n")),
        }
    }
}

/// Un trait dessiné par la tortue.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub a: Vec2,
    pub b: Vec2,
    pub couleur: Color32,
    pub epaisseur: f32,
}

/// Exécute le programme : liste des traits, et position/cap finaux. `max` limite les étapes.
pub fn executer(prog: &[Bloc], max: usize) -> (Vec<Segment>, Vec2, f32) {
    struct T {
        p: Vec2,
        cap: f32,
        crayon: bool,
        couleur: Color32,
        ep: f32,
        teinte: f32,
        traits: Vec<Segment>,
        etapes: usize,
    }
    fn tourner(t: &mut T, v: &[Bloc], max: usize) {
        for b in v {
            if t.etapes >= max {
                return;
            }
            t.etapes += 1;
            match b {
                Bloc::Avancer(n) | Bloc::Reculer(n) => {
                    let s = if matches!(b, Bloc::Reculer(_)) { -n } else { *n };
                    let r = t.cap.to_radians();
                    let q = t.p + Vec2::new(r.sin(), -r.cos()) * s;
                    if t.crayon {
                        t.traits.push(Segment { a: t.p, b: q, couleur: t.couleur, epaisseur: t.ep });
                    }
                    t.p = q;
                }
                Bloc::Droite(a) => t.cap += a,
                Bloc::Gauche(a) => t.cap -= a,
                Bloc::Repeter(n, v) => {
                    for _ in 0..(*n).min(1000) {
                        tourner(t, v, max);
                    }
                }
                Bloc::Couleur(c) => t.couleur = Color32::from_rgb(c[0], c[1], c[2]),
                Bloc::ArcEnCiel => {
                    t.teinte = (t.teinte + 0.07) % 1.0;
                    t.couleur = Color32::from(egui::ecolor::Hsva::new(t.teinte, 0.85, 1.0, 1.0));
                }
                Bloc::Epaisseur(e) => t.ep = *e,
                Bloc::Lever => t.crayon = false,
                Bloc::Baisser => t.crayon = true,
                Bloc::Centre => {
                    t.p = Vec2::ZERO;
                    t.cap = 0.0;
                }
            }
        }
    }
    let mut t = T { p: Vec2::ZERO, cap: 0.0, crayon: true, couleur: Color32::from_rgb(40, 200, 120), ep: 2.0, teinte: 0.0, traits: Vec::new(), etapes: 0 };
    tourner(&mut t, prog, max);
    (t.traits, t.p, t.cap)
}

/// Programmes d'exemple.
pub fn exemples() -> Vec<(&'static str, Vec<Bloc>)> {
    use Bloc::*;
    vec![
        ("⬜ Carré", vec![Repeter(4, vec![Avancer(150.0), Droite(90.0)])]),
        ("⭐ Étoile", vec![Couleur([250, 200, 40]), Repeter(5, vec![Avancer(200.0), Droite(144.0)])]),
        ("🌀 Spirale", vec![Repeter(120, vec![ArcEnCiel, Avancer(0.0), Droite(59.0)]), Centre, Repeter(90, vec![ArcEnCiel, Avancer(4.0), Droite(91.0), Avancer(4.0), Droite(1.0)])]),
        ("🌸 Fleur", vec![Repeter(12, vec![ArcEnCiel, Repeter(36, vec![Avancer(8.0), Droite(10.0)]), Droite(30.0)])]),
        ("🏠 Maison", vec![Couleur([230, 120, 60]), Repeter(4, vec![Avancer(120.0), Droite(90.0)]), Avancer(120.0), Droite(30.0), Couleur([200, 60, 60]), Repeter(3, vec![Avancer(120.0), Droite(120.0)])]),
    ]
}

pub struct Tortue {
    prog: Vec<Bloc>,
    pas: usize,
    anime: Option<Instant>,
    vitesse: f32,
    python: bool,
    choisi: Option<Vec<usize>>,
}

impl Tortue {
    pub fn new() -> Self {
        let mut prog: Vec<Bloc> = charger("tortue");
        if prog.is_empty() {
            prog = exemples()[0].1.clone();
        }
        Tortue { prog, pas: usize::MAX, anime: None, vitesse: 30.0, python: false, choisi: None }
    }
}

/// Liste de blocs modifiable (récursive pour « Répéter »). Renvoie vrai si quelque chose a changé.
fn liste_blocs(ui: &mut egui::Ui, v: &mut Vec<Bloc>, chemin: Vec<usize>, choisi: &mut Option<Vec<usize>>) -> bool {
    let mut change = false;
    let mut action: Option<(usize, i32)> = None; // (indice, -1 haut / 1 bas / 0 supprimer)
    for k in 0..v.len() {
        let mut c = chemin.clone();
        c.push(k);
        let col = v[k].couleur();
        egui::Frame::new().fill(col.gamma_multiply(0.35)).stroke(Stroke::new(1.0_f32, col)).corner_radius(6.0).inner_margin(5.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                let est_choisi = choisi.as_ref() == Some(&c);
                if ui.selectable_label(est_choisi, RichText::new(v[k].nom()).strong()).on_hover_text("Clic : les nouveaux blocs iront dedans (pour « Répéter ») ou juste après").clicked() {
                    *choisi = if est_choisi { None } else { Some(c.clone()) };
                }
                match &mut v[k] {
                    Bloc::Avancer(n) | Bloc::Reculer(n) => change |= ui.add(egui::DragValue::new(n).range(0.0..=800.0)).changed(),
                    Bloc::Droite(a) | Bloc::Gauche(a) => change |= ui.add(egui::DragValue::new(a).range(-360.0..=360.0).suffix("°")).changed(),
                    Bloc::Repeter(n, _) => change |= ui.add(egui::DragValue::new(n).range(1..=500)).changed(),
                    Bloc::Couleur(c) => change |= ui.color_edit_button_srgb(c).changed(),
                    Bloc::Epaisseur(e) => change |= ui.add(egui::DragValue::new(e).range(0.5..=30.0)).changed(),
                    _ => {}
                }
                if ui.small_button("⬆").clicked() {
                    action = Some((k, -1));
                }
                if ui.small_button("⬇").clicked() {
                    action = Some((k, 1));
                }
                if ui.small_button("✖").clicked() {
                    action = Some((k, 0));
                }
            });
            if let Bloc::Repeter(_, dedans) = &mut v[k] {
                ui.indent(("rep", c.clone()), |ui| {
                    change |= liste_blocs(ui, dedans, c.clone(), choisi);
                    if dedans.is_empty() {
                        ui.weak("(vide : choisis ce bloc puis ajoute des blocs dedans)");
                    }
                });
            }
        });
    }
    if let Some((k, a)) = action {
        change = true;
        match a {
            0 => {
                v.remove(k);
                *choisi = None;
            }
            -1 if k > 0 => v.swap(k, k - 1),
            1 if k + 1 < v.len() => v.swap(k, k + 1),
            _ => {}
        }
    }
    change
}

/// Ajoute un bloc à l'endroit choisi : dans un « Répéter » choisi, sinon juste après le bloc choisi, sinon à la fin.
fn inserer(prog: &mut Vec<Bloc>, choisi: &Option<Vec<usize>>, b: Bloc) {
    let Some(c) = choisi.clone().filter(|c| !c.is_empty()) else {
        prog.push(b);
        return;
    };
    let mut liste = prog;
    for &i in &c[..c.len() - 1] {
        match liste.get_mut(i) {
            Some(Bloc::Repeter(_, v)) => liste = v,
            _ => return,
        }
    }
    let i = *c.last().unwrap();
    match liste.get_mut(i) {
        Some(Bloc::Repeter(_, v)) => v.push(b),
        Some(_) => liste.insert(i + 1, b),
        None => liste.push(b),
    }
}

impl Outil for Tortue {
    fn nom(&self) -> &'static str {
        "🐢 Coder avec des blocs"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.prog.clone();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Coder avec des blocs");
            ui.label("Exemples :");
            for (n, p) in exemples() {
                if ui.small_button(n).clicked() {
                    self.prog = p;
                    self.choisi = None;
                    self.anime = Some(Instant::now());
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Ajouter :");
            let mut ajout = None;
            for (n, b) in [("⬆ Avancer", Bloc::Avancer(50.0)), ("⬇ Reculer", Bloc::Reculer(50.0)), ("↻ Droite", Bloc::Droite(90.0)), ("↺ Gauche", Bloc::Gauche(90.0)), ("🔁 Répéter", Bloc::Repeter(4, vec![])), ("🎨 Couleur", Bloc::Couleur([60, 160, 255])), ("🌈 Arc-en-ciel", Bloc::ArcEnCiel), ("✏ Épaisseur", Bloc::Epaisseur(4.0)), ("☝ Lever", Bloc::Lever), ("👇 Baisser", Bloc::Baisser), ("🎯 Centre", Bloc::Centre)] {
                if ui.small_button(n).clicked() {
                    ajout = Some(b);
                }
            }
            if let Some(b) = ajout {
                inserer(&mut self.prog, &self.choisi, b);
            }
        });
        ui.horizontal(|ui| {
            if ui.button(RichText::new("▶ Lancer").strong()).clicked() {
                self.anime = Some(Instant::now());
            }
            ui.label("Vitesse");
            ui.add(egui::Slider::new(&mut self.vitesse, 2.0..=600.0).logarithmic(true).show_value(false));
            ui.toggle_value(&mut self.python, "🐍 Voir en Python");
            if ui.small_button("🧹 Tout effacer").clicked() {
                self.prog.clear();
                self.choisi = None;
            }
        });
        let total = executer(&self.prog, usize::MAX).0.len().max(1);
        self.pas = match self.anime {
            Some(t) => {
                let n = (t.elapsed().as_secs_f32() * self.vitesse) as usize;
                if n >= 100_000 {
                    self.anime = None;
                }
                ui.ctx().request_repaint();
                n
            }
            None => usize::MAX,
        };
        ui.columns(2, |c| {
            egui::ScrollArea::vertical().id_salt("blocs").max_height(c[0].available_height()).show(&mut c[0], |ui| {
                let mut choisi = self.choisi.clone();
                if liste_blocs(ui, &mut self.prog, Vec::new(), &mut choisi) {
                    self.anime = None;
                }
                self.choisi = choisi;
                if self.prog.is_empty() {
                    ui.weak("Ajoute des blocs avec les boutons du haut, ou ouvre un exemple.");
                }
                if self.python {
                    let mut s = String::from("from turtle import *\n\n");
                    for b in &self.prog {
                        b.python(0, &mut s);
                    }
                    s.push_str("\ndone()\n");
                    ui.separator();
                    ui.label(RichText::new(s).monospace());
                }
            });
            let ui = &mut c[1];
            let cote = ui.available_width().min(ui.available_height()).max(200.0);
            let (zone, _) = ui.allocate_exact_size(Vec2::splat(cote), Sense::hover());
            let p = ui.painter_at(zone);
            p.rect_filled(zone, 6.0, Color32::from_rgb(16, 20, 28));
            // échelle : le dessin entier tient dans la zone
            let (tous, ..) = executer(&self.prog, usize::MAX);
            let etendue = tous.iter().flat_map(|s| [s.a, s.b]).fold(200.0f32, |m, q| m.max(q.x.abs()).max(q.y.abs()));
            let e = cote / 2.0 / (etendue * 1.1);
            let vers = |v: Vec2| zone.center() + v * e;
            let (traits, pos, cap) = executer(&self.prog, self.pas);
            for s in &traits {
                p.line_segment([vers(s.a), vers(s.b)], Stroke::new((s.epaisseur * e).max(1.0), s.couleur));
            }
            // la tortue
            let r = cap.to_radians();
            let dir = Vec2::new(r.sin(), -r.cos());
            let cote_t = Vec2::new(-dir.y, dir.x);
            let q = vers(pos);
            p.add(egui::Shape::convex_polygon(vec![q + dir * 12.0, q - dir * 7.0 + cote_t * 8.0, q - dir * 7.0 - cote_t * 8.0], Color32::from_rgb(90, 200, 120), Stroke::new(1.5_f32, Color32::WHITE)));
            p.text(zone.left_top() + Vec2::new(8.0, 6.0), egui::Align2::LEFT_TOP, format!("{} traits", total), FontId::proportional(12.0), Color32::from_gray(140));
        });
        if self.prog != avant {
            enregistrer("tortue", &self.prog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labyrinthe() {
        let m = Labyrinthe::generer(15, 10, 42, 0.8);
        // labyrinthe parfait : nombre de passages = cases − 1
        let ouvertures: usize = m.murs.iter().map(|c| c.iter().filter(|x| !**x).count()).sum::<usize>() / 2;
        assert_eq!(ouvertures, 15 * 10 - 1);
        let s = m.solution();
        assert_eq!(s[0], 0);
        assert_eq!(*s.last().unwrap(), 149);
        for w in s.windows(2) {
            let (a, b) = (w[0] as i32, w[1] as i32);
            assert!((a - b).abs() == 1 || (a - b).abs() == 15);
        }
        let img = m.image(10, true);
        assert_eq!(img.width(), 15 * 10 + 20);
        assert_ne!(Labyrinthe::generer(15, 10, 43, 0.8).murs, m.murs);
    }

    #[test]
    fn tortue() {
        use Bloc::*;
        let (t, p, cap) = executer(&[Repeter(4, vec![Avancer(100.0), Droite(90.0)])], usize::MAX);
        assert_eq!(t.len(), 4);
        assert!(p.length() < 1e-3 && (cap - 360.0).abs() < 1e-3);
        assert!((t[0].b - Vec2::new(0.0, -100.0)).length() < 1e-3); // la tortue part vers le haut
        let (t, ..) = executer(&[Lever, Avancer(10.0), Baisser, Avancer(10.0)], usize::MAX);
        assert_eq!(t.len(), 1);
        assert_eq!(executer(&[Repeter(10, vec![Avancer(1.0)])], 4).0.len(), 3); // limité en étapes
        let mut prog = vec![Repeter(3, vec![])];
        inserer(&mut prog, &Some(vec![0]), Avancer(5.0));
        inserer(&mut prog, &None, Droite(5.0));
        assert_eq!(prog, vec![Repeter(3, vec![Avancer(5.0)]), Droite(5.0)]);
        let mut s = String::new();
        prog[0].python(0, &mut s);
        assert_eq!(s, "for i in range(3):\n    forward(5)\n");
        for (_, p) in exemples() {
            assert!(!executer(&p, usize::MAX).0.is_empty());
        }
    }
}
