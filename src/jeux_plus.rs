//! Jeux : entraînement à la visée (1), testeur de manette (2), tier lists (3), jeu de rythme (4),
//! puzzle photo (5).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn hasard_f() -> f32 {
    crate::outil::hasard(1_000_000) as f32 / 1_000_000.0
}

// ---------------------------------------------------------------------------------------------------
// 1 · Entraînement à la visée
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum ModeVisee {
    Flick,
    Grille,
    Suivi,
    Precision,
}

pub const MODES_VISEE: [(ModeVisee, &str, &str); 4] = [
    (ModeVisee::Flick, "⚡ Flick", "Une cible à la fois : vise vite et clique."),
    (ModeVisee::Grille, "🔳 Grille", "Trois cibles en même temps, enchaîne les clics."),
    (ModeVisee::Suivi, "🎯 Suivi", "Garde la souris sur la cible qui bouge."),
    (ModeVisee::Precision, "🔬 Précision", "Petites cibles qui rétrécissent : touche-les avant qu'elles disparaissent."),
];

struct Cible {
    p: Vec2, // position relative à la zone (0..1)
    r: f32,
    nee: Instant,
    v: Vec2,
}

pub struct Visee {
    mode: ModeVisee,
    cibles: Vec<Cible>,
    debut: Option<Instant>,
    compte: Option<Instant>,
    touches: u32,
    clics: u32,
    reactions: Vec<f32>,
    suivi_dans: f32,
    duree: f32,
    records: Vec<(String, u32)>,
    fini: Option<u32>,
}

impl Visee {
    pub fn new() -> Self {
        Visee { mode: ModeVisee::Flick, cibles: Vec::new(), debut: None, compte: None, touches: 0, clics: 0, reactions: Vec::new(), suivi_dans: 0.0, duree: 30.0, records: charger("visee_records"), fini: None }
    }

    fn nouvelle_cible(&self) -> Cible {
        let r = match self.mode {
            ModeVisee::Precision => 0.035,
            ModeVisee::Suivi => 0.05,
            _ => 0.045,
        };
        let a = hasard_f() * std::f32::consts::TAU;
        Cible { p: Vec2::new(0.08 + hasard_f() * 0.84, 0.1 + hasard_f() * 0.8), r, nee: Instant::now(), v: Vec2::new(a.cos(), a.sin()) * 0.25 }
    }

    fn score(&self) -> u32 {
        match self.mode {
            ModeVisee::Suivi => (self.suivi_dans * 100.0) as u32,
            _ => self.touches * 100 + if self.clics > 0 { self.touches * 50 * self.touches / self.clics } else { 0 },
        }
    }
}

impl Outil for Visee {
    fn nom(&self) -> &'static str {
        "🎯 Entraînement à la visée"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Entraînement à la visée");
            for (m, n, d) in MODES_VISEE {
                if ui.selectable_label(self.mode == m, n).on_hover_text(d).clicked() && self.debut.is_none() {
                    self.mode = m;
                    self.fini = None;
                }
            }
            if self.debut.is_none() && self.compte.is_none() && ui.button(RichText::new("▶ Jouer (30 s)").strong()).clicked() {
                self.compte = Some(Instant::now());
                self.fini = None;
            }
        });
        let (zone, rep) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 8.0, Color32::from_rgb(18, 22, 30));
        let maintenant = Instant::now();
        // compte à rebours 3-2-1
        if let Some(c) = self.compte {
            let reste = 3.0 - c.elapsed().as_secs_f32();
            if reste <= 0.0 {
                self.compte = None;
                self.debut = Some(maintenant);
                self.touches = 0;
                self.clics = 0;
                self.reactions.clear();
                self.suivi_dans = 0.0;
                let n = if self.mode == ModeVisee::Grille { 3 } else { 1 };
                self.cibles = (0..n).map(|_| self.nouvelle_cible()).collect();
            } else {
                p.text(zone.center(), egui::Align2::CENTER_CENTER, format!("{}", reste.ceil()), FontId::proportional(90.0), Color32::WHITE);
            }
        }
        let vers = |c: &Cible| zone.min + Vec2::new(c.p.x * zone.width(), c.p.y * zone.height());
        let unite = zone.width().min(zone.height());
        if let Some(d) = self.debut {
            let t = d.elapsed().as_secs_f32();
            let dt = ui.input(|i| i.stable_dt).min(0.05);
            if t >= self.duree {
                self.debut = None;
                let s = self.score();
                self.fini = Some(s);
                let cle = format!("{:?}", self.mode);
                match self.records.iter_mut().find(|r| r.0 == cle) {
                    Some(r) if r.1 >= s => {}
                    Some(r) => r.1 = s,
                    None => self.records.push((cle, s)),
                }
                enregistrer("visee_records", &self.records);
            } else {
                let souris = ui.input(|i| i.pointer.hover_pos());
                // mouvements
                if self.mode == ModeVisee::Suivi {
                    for c in &mut self.cibles {
                        c.p += c.v * dt;
                        if c.p.x < 0.06 || c.p.x > 0.94 {
                            c.v.x = -c.v.x;
                        }
                        if c.p.y < 0.08 || c.p.y > 0.92 {
                            c.v.y = -c.v.y;
                        }
                        if hasard_f() < dt * 0.8 {
                            let a = hasard_f() * std::f32::consts::TAU;
                            c.v = Vec2::new(a.cos(), a.sin()) * (0.15 + hasard_f() * 0.3);
                        }
                    }
                    if let (Some(s), Some(c)) = (souris, self.cibles.first()) {
                        if s.distance(vers(c)) < c.r * unite {
                            self.suivi_dans += dt;
                        }
                    }
                }
                if self.mode == ModeVisee::Precision {
                    // la cible rétrécit et disparaît en 1,5 s
                    let mut neuves = Vec::new();
                    self.cibles.retain(|c| c.nee.elapsed() < Duration::from_millis(1500));
                    if self.cibles.is_empty() {
                        neuves.push(self.nouvelle_cible());
                    }
                    self.cibles.extend(neuves);
                }
                // clics
                if rep.clicked() && self.mode != ModeVisee::Suivi {
                    self.clics += 1;
                    if let Some(s) = rep.interact_pointer_pos() {
                        let r_eff = |c: &Cible| if self.mode == ModeVisee::Precision { c.r * (1.0 - c.nee.elapsed().as_secs_f32() / 1.5).max(0.2) } else { c.r };
                        if let Some(k) = self.cibles.iter().position(|c| s.distance(vers(c)) < r_eff(c) * unite) {
                            self.touches += 1;
                            self.reactions.push(self.cibles[k].nee.elapsed().as_secs_f32() * 1000.0);
                            let n = self.nouvelle_cible();
                            self.cibles[k] = n;
                        }
                    }
                }
                for c in &self.cibles {
                    let r = if self.mode == ModeVisee::Precision { c.r * (1.0 - c.nee.elapsed().as_secs_f32() / 1.5).max(0.2) } else { c.r } * unite;
                    let q = vers(c);
                    p.circle_filled(q, r, Color32::from_rgb(230, 70, 70));
                    p.circle_filled(q, r * 0.66, Color32::WHITE);
                    p.circle_filled(q, r * 0.33, Color32::from_rgb(230, 70, 70));
                }
                p.text(zone.left_top() + Vec2::new(12.0, 10.0), egui::Align2::LEFT_TOP, format!("⏱ {:.0} s   🎯 {}   score {}", self.duree - t, self.touches, self.score()), FontId::proportional(18.0), Color32::WHITE);
            }
            ui.ctx().request_repaint();
        }
        if self.debut.is_none() && self.compte.is_none() {
            let mut lignes = vec![];
            if let Some(s) = self.fini {
                lignes.push(format!("Score : {s}"));
                if self.mode != ModeVisee::Suivi {
                    let prec = if self.clics > 0 { self.touches * 100 / self.clics } else { 0 };
                    let moy = if self.reactions.is_empty() { 0.0 } else { self.reactions.iter().sum::<f32>() / self.reactions.len() as f32 };
                    lignes.push(format!("{} cibles · précision {prec} % · temps moyen {:.0} ms", self.touches, moy));
                } else {
                    lignes.push(format!("Sur la cible {:.1} s sur {:.0}", self.suivi_dans, self.duree));
                }
            }
            if let Some((_, r)) = self.records.iter().find(|r| r.0 == format!("{:?}", self.mode)) {
                lignes.push(format!("🏆 Record : {r}"));
            }
            lignes.push(MODES_VISEE.iter().find(|m| m.0 == self.mode).map(|m| m.2).unwrap_or("").to_string());
            for (k, l) in lignes.iter().enumerate() {
                p.text(zone.center() + Vec2::new(0.0, k as f32 * 30.0 - 40.0), egui::Align2::CENTER_CENTER, l, FontId::proportional(if k == 0 && self.fini.is_some() { 32.0 } else { 18.0 }), Color32::WHITE);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 2 · Testeur de manette
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct EtatManette {
    pub boutons: u16, // bits XInput
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
}

pub const BOUTONS_XINPUT: [(u16, &str); 14] = [
    (0x1000, "A"), (0x2000, "B"), (0x4000, "X"), (0x8000, "Y"), (0x0100, "LB"), (0x0200, "RB"), (0x0010, "Start"), (0x0020, "Back"),
    (0x0040, "L3"), (0x0080, "R3"), (0x0001, "▲"), (0x0002, "▼"), (0x0004, "◀"), (0x0008, "▶"),
];

/// Dérive : le stick ne revient pas au centre quand on le lâche.
pub fn derive(x: f32, y: f32) -> bool {
    (x * x + y * y).sqrt() > 0.08
}

#[cfg(windows)]
fn lire(i: u32) -> Option<EtatManette> {
    use windows_sys::Win32::UI::Input::XboxController::*;
    let mut s: XINPUT_STATE = unsafe { std::mem::zeroed() };
    if unsafe { XInputGetState(i, &mut s) } != 0 {
        return None;
    }
    let g = s.Gamepad;
    let a = |v: i16| (v as f32 / 32767.0).clamp(-1.0, 1.0);
    Some(EtatManette { boutons: g.wButtons, lx: a(g.sThumbLX), ly: a(g.sThumbLY), rx: a(g.sThumbRX), ry: a(g.sThumbRY), lt: g.bLeftTrigger as f32 / 255.0, rt: g.bRightTrigger as f32 / 255.0 })
}

#[cfg(windows)]
fn vibrer(i: u32, gauche: f32, droite: f32) {
    use windows_sys::Win32::UI::Input::XboxController::*;
    let mut v = XINPUT_VIBRATION { wLeftMotorSpeed: (gauche * 65535.0) as u16, wRightMotorSpeed: (droite * 65535.0) as u16 };
    unsafe { XInputSetState(i, &mut v) };
}

/// Linux : interface « joystick » du noyau (/dev/input/jsN), lue dans un fil.
#[cfg(not(windows))]
fn ecouter_js(n: u32, etat: Arc<Mutex<Option<EtatManette>>>, vivant: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        use std::io::Read;
        let Ok(mut f) = std::fs::File::open(format!("/dev/input/js{n}")) else { return };
        let mut e = EtatManette::default();
        *etat.lock().unwrap() = Some(e);
        let mut b = [0u8; 8];
        // boutons Linux (manette Xbox) → bits XInput
        const BITS: [u16; 11] = [0x1000, 0x2000, 0x4000, 0x8000, 0x0100, 0x0200, 0x0020, 0x0010, 0x0400, 0x0040, 0x0080];
        while vivant.load(Relaxed) && f.read_exact(&mut b).is_ok() {
            let v = i16::from_le_bytes([b[4], b[5]]) as f32 / 32767.0;
            let (genre, num) = (b[6] & 0x7F, b[7]);
            if genre == 1 {
                if let Some(bit) = BITS.get(num as usize) {
                    if v > 0.0 { e.boutons |= bit } else { e.boutons &= !bit }
                }
            } else if genre == 2 {
                match num {
                    0 => e.lx = v,
                    1 => e.ly = -v,
                    2 => e.lt = (v + 1.0) / 2.0,
                    3 => e.rx = v,
                    4 => e.ry = -v,
                    5 => e.rt = (v + 1.0) / 2.0,
                    6 => {
                        e.boutons &= !(0x0004 | 0x0008);
                        e.boutons |= if v < -0.5 { 0x0004 } else if v > 0.5 { 0x0008 } else { 0 };
                    }
                    7 => {
                        e.boutons &= !(0x0001 | 0x0002);
                        e.boutons |= if v < -0.5 { 0x0001 } else if v > 0.5 { 0x0002 } else { 0 };
                    }
                    _ => {}
                }
            }
            *etat.lock().unwrap() = Some(e);
        }
        *etat.lock().unwrap() = None;
    });
}

pub struct TestManette {
    numero: u32,
    traces: [Vec<Pos2>; 2],
    moteurs: (f32, f32),
    vus: u16,
    #[cfg(not(windows))]
    etat: Arc<Mutex<Option<EtatManette>>>,
    #[cfg(not(windows))]
    vivant: Arc<AtomicBool>,
}

impl TestManette {
    pub fn new() -> Self {
        TestManette {
            numero: 0,
            traces: [Vec::new(), Vec::new()],
            moteurs: (0.0, 0.0),
            vus: 0,
            #[cfg(not(windows))]
            etat: Arc::default(),
            #[cfg(not(windows))]
            vivant: Arc::new(AtomicBool::new(false)),
        }
    }

    fn etat(&mut self) -> Option<EtatManette> {
        #[cfg(windows)]
        return lire(self.numero);
        #[cfg(not(windows))]
        {
            if !self.vivant.swap(true, Relaxed) {
                ecouter_js(self.numero, self.etat.clone(), self.vivant.clone());
            }
            let e = *self.etat.lock().unwrap();
            if e.is_none() {
                self.vivant.store(false, Relaxed);
            }
            e
        }
    }
}

impl Outil for TestManette {
    fn nom(&self) -> &'static str {
        "🕹 Testeur de manette"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Testeur de manette");
            for n in 0..4 {
                if ui.selectable_label(self.numero == n, format!("Manette {}", n + 1)).clicked() {
                    self.numero = n;
                    self.vus = 0;
                    #[cfg(not(windows))]
                    self.vivant.store(false, Relaxed);
                }
            }
        });
        ui.ctx().request_repaint_after(Duration::from_millis(16));
        let Some(e) = self.etat() else {
            ui.label(RichText::new("Aucune manette branchée à cet emplacement.").size(18.0));
            ui.weak("Branche une manette Xbox (ou compatible) en USB ou en Bluetooth.");
            return;
        };
        self.vus |= e.boutons;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(760.0), 330.0), Sense::hover());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 20.0, Color32::from_rgb(35, 38, 48));
        let o = zone.min;
        // sticks
        for (k, (x, y, c, bit)) in [(e.lx, e.ly, Pos2::new(150.0, 150.0), 0x0040u16), (e.rx, e.ry, Pos2::new(470.0, 230.0), 0x0080)].into_iter().enumerate() {
            let centre = o + c.to_vec2();
            let r = 60.0;
            p.circle_filled(centre, r, Color32::from_rgb(25, 27, 35));
            p.circle_stroke(centre, r * 0.08 / 1.0 * 1.0 + r * 0.08, Stroke::new(1.0_f32, Color32::from_gray(80)));
            let point = centre + Vec2::new(x, -y) * r;
            self.traces[k].push(point);
            if self.traces[k].len() > 120 {
                self.traces[k].remove(0);
            }
            p.add(egui::Shape::line(self.traces[k].clone(), Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(90, 170, 250, 120))));
            let appuye = e.boutons & bit != 0;
            p.circle_filled(point, 14.0, if appuye { Color32::from_rgb(90, 200, 120) } else { Color32::from_rgb(90, 170, 250) });
            p.text(centre + Vec2::new(0.0, r + 14.0), egui::Align2::CENTER_CENTER, format!("x {x:+.2}  y {y:+.2}"), FontId::monospace(12.0), Color32::from_gray(200));
        }
        // gâchettes
        for (k, v) in [e.lt, e.rt].into_iter().enumerate() {
            let x = if k == 0 { 60.0 } else { 640.0 };
            let r = Rect::from_min_size(o + Vec2::new(x, 20.0), Vec2::new(60.0, 14.0));
            p.rect_filled(r, 4.0, Color32::from_gray(25));
            p.rect_filled(Rect::from_min_size(r.min, Vec2::new(60.0 * v, 14.0)), 4.0, Color32::from_rgb(250, 180, 60));
            p.text(r.center_bottom() + Vec2::new(0.0, 10.0), egui::Align2::CENTER_CENTER, format!("{} {:.0} %", if k == 0 { "LT" } else { "RT" }, v * 100.0), FontId::proportional(12.0), Color32::WHITE);
        }
        // boutons
        let places: [(u16, Pos2, Color32); 14] = [
            (0x1000, Pos2::new(610.0, 170.0), Color32::from_rgb(90, 200, 120)),
            (0x2000, Pos2::new(650.0, 130.0), Color32::from_rgb(230, 80, 90)),
            (0x4000, Pos2::new(570.0, 130.0), Color32::from_rgb(80, 150, 240)),
            (0x8000, Pos2::new(610.0, 90.0), Color32::from_rgb(240, 200, 70)),
            (0x0100, Pos2::new(90.0, 60.0), Color32::GRAY),
            (0x0200, Pos2::new(670.0, 60.0), Color32::GRAY),
            (0x0010, Pos2::new(420.0, 120.0), Color32::GRAY),
            (0x0020, Pos2::new(330.0, 120.0), Color32::GRAY),
            (0x0040, Pos2::new(-100.0, -100.0), Color32::GRAY),
            (0x0080, Pos2::new(-100.0, -100.0), Color32::GRAY),
            (0x0001, Pos2::new(290.0, 200.0), Color32::GRAY),
            (0x0002, Pos2::new(290.0, 260.0), Color32::GRAY),
            (0x0004, Pos2::new(260.0, 230.0), Color32::GRAY),
            (0x0008, Pos2::new(320.0, 230.0), Color32::GRAY),
        ];
        for (bit, pos, c) in places {
            if pos.x < 0.0 {
                continue;
            }
            let nom = BOUTONS_XINPUT.iter().find(|b| b.0 == bit).map(|b| b.1).unwrap_or("");
            let on = e.boutons & bit != 0;
            p.circle_filled(o + pos.to_vec2(), 17.0, if on { c } else { c.gamma_multiply(0.3) });
            p.text(o + pos.to_vec2(), egui::Align2::CENTER_CENTER, nom, FontId::proportional(13.0), Color32::WHITE);
        }
        // bilan
        let manquants: Vec<&str> = BOUTONS_XINPUT.iter().filter(|b| self.vus & b.0 == 0).map(|b| b.1).collect();
        if manquants.is_empty() {
            ui.colored_label(Color32::LIGHT_GREEN, "✔ Tous les boutons ont répondu.");
        } else {
            ui.label(format!("Appuie sur chaque bouton pour le tester. Encore : {}", manquants.join(" ")));
        }
        for (n, x, y, bit) in [("gauche", e.lx, e.ly, 0x0040u16), ("droit", e.rx, e.ry, 0x0080)] {
            if e.boutons & bit == 0 && derive(x, y) && (x * x + y * y).sqrt() < 0.35 {
                ui.colored_label(Color32::from_rgb(255, 180, 80), format!("⚠ Stick {n} : il ne revient pas tout à fait au centre ({:.0} %) — si tu ne le touches pas, c'est une dérive (« drift »).", (x * x + y * y).sqrt() * 100.0));
            }
        }
        #[cfg(windows)]
        ui.horizontal(|ui| {
            ui.label("📳 Vibrations :");
            let avant = self.moteurs;
            ui.add(egui::Slider::new(&mut self.moteurs.0, 0.0..=1.0).text("gauche (forte)"));
            ui.add(egui::Slider::new(&mut self.moteurs.1, 0.0..=1.0).text("droite (fine)"));
            if self.moteurs != avant {
                vibrer(self.numero, self.moteurs.0, self.moteurs.1);
            }
            if ui.button("Stop").clicked() {
                self.moteurs = (0.0, 0.0);
                vibrer(self.numero, 0.0, 0.0);
            }
        });
        #[cfg(not(windows))]
        let _ = self.moteurs;
    }
}

// ---------------------------------------------------------------------------------------------------
// 3 · Tier lists
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Element {
    pub texte: String,
    pub image: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct TierList {
    pub titre: String,
    pub rangs: Vec<(String, [u8; 3], Vec<Element>)>,
    pub reserve: Vec<Element>,
}

impl Default for TierList {
    fn default() -> Self {
        let r = |n: &str, c: [u8; 3]| (n.to_string(), c, Vec::new());
        TierList {
            titre: "Mes jeux préférés".into(),
            rangs: vec![r("S", [255, 127, 127]), r("A", [255, 191, 127]), r("B", [255, 223, 127]), r("C", [255, 255, 127]), r("D", [191, 255, 127]), r("F", [127, 191, 255])],
            reserve: ["Minecraft", "Fortnite", "Mario Kart", "Zelda", "Among Us", "Rocket League"].iter().map(|t| Element { texte: t.to_string(), image: None }).collect(),
        }
    }
}

/// Déplace un élément (rang source, indice) vers un rang (usize::MAX = réserve) à une position.
pub fn deplacer(t: &mut TierList, de: (usize, usize), vers: usize, pos: usize) {
    let e = if de.0 == usize::MAX {
        if de.1 >= t.reserve.len() {
            return;
        }
        t.reserve.remove(de.1)
    } else {
        let Some(r) = t.rangs.get_mut(de.0) else { return };
        if de.1 >= r.2.len() {
            return;
        }
        r.2.remove(de.1)
    };
    let dernier = t.rangs.len() - 1;
    let liste = if vers == usize::MAX { &mut t.reserve } else { &mut t.rangs[vers.min(dernier)].2 };
    liste.insert(pos.min(liste.len()), e);
}

pub struct TierLists {
    listes: Vec<TierList>,
    actuelle: usize,
    saisie: String,
    vignettes: Vec<(PathBuf, egui::TextureHandle)>,
    message: String,
}

impl TierLists {
    pub fn new() -> Self {
        let mut l: Vec<TierList> = charger("tier_lists");
        if l.is_empty() {
            l.push(TierList::default());
        }
        TierLists { listes: l, actuelle: 0, saisie: String::new(), vignettes: Vec::new(), message: String::new() }
    }

    fn vignette(&mut self, ctx: &egui::Context, p: &PathBuf) -> Option<egui::TextureHandle> {
        if let Some((_, t)) = self.vignettes.iter().find(|(q, _)| q == p) {
            return Some(t.clone());
        }
        let i = image::open(p).ok()?.thumbnail(128, 128).to_rgba8();
        let c = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
        let t = ctx.load_texture(p.to_string_lossy(), c, Default::default());
        self.vignettes.push((p.clone(), t.clone()));
        Some(t)
    }

    fn element_ui(&mut self, ui: &mut egui::Ui, e: &Element) {
        match e.image.as_ref().and_then(|p| self.vignette(ui.ctx(), p)) {
            Some(t) => {
                ui.add(egui::Image::new(&t).fit_to_exact_size(Vec2::splat(64.0))).on_hover_text(&e.texte);
            }
            None => {
                egui::Frame::new().fill(Color32::from_gray(60)).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                    ui.label(RichText::new(&e.texte).strong());
                });
            }
        }
    }
}

/// Image de la tier list (pour la partager).
pub fn image_tier(t: &TierList, police: Option<&ab_glyph::FontVec>) -> image::RgbaImage {
    let (l, h_rang, case) = (1100u32, 110u32, 96u32);
    let haut = 70 + h_rang * t.rangs.len() as u32;
    let mut img = image::RgbaImage::from_pixel(l, haut, image::Rgba([26, 26, 30, 255]));
    if let Some(f) = police {
        crate::creatif::ecrire(&mut img, f, 40.0, 20.0, 14.0, &t.titre, [255, 255, 255], None);
    }
    for (k, (nom, c, elems)) in t.rangs.iter().enumerate() {
        let y = 70 + k as u32 * h_rang;
        crate::atelier::rectangle(&mut img, 0.0, y as f32, 110.0, (h_rang - 4) as f32, image::Rgba([c[0], c[1], c[2], 255]));
        if let Some(f) = police {
            let w = crate::creatif::largeur(f, 44.0, nom);
            crate::creatif::ecrire(&mut img, f, 44.0, 55.0 - w / 2.0, y as f32 + 28.0, nom, [20, 20, 20], None);
        }
        for (i, e) in elems.iter().enumerate() {
            let x = 120 + i as u32 * (case + 6);
            if x + case > l {
                break;
            }
            match e.image.as_ref().and_then(|p| image::open(p).ok()) {
                Some(im) => {
                    let v = crate::createur::couvrir(&im.to_rgba8(), case, case);
                    image::imageops::overlay(&mut img, &v, x as i64, y as i64 + 5);
                }
                None => {
                    crate::atelier::rectangle(&mut img, x as f32, y as f32 + 5.0, case as f32, case as f32, image::Rgba([60, 60, 66, 255]));
                    if let Some(f) = police {
                        for (j, ligne) in crate::atelier::couper(f, 18.0, &e.texte, case as f32 - 8.0).iter().take(4).enumerate() {
                            crate::creatif::ecrire(&mut img, f, 18.0, x as f32 + 5.0, y as f32 + 12.0 + j as f32 * 20.0, ligne, [240, 240, 240], None);
                        }
                    }
                }
            }
        }
    }
    img
}

impl Outil for TierLists {
    fn nom(&self) -> &'static str {
        "📊 Tier lists"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.listes.clone();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Tier lists");
            for k in 0..self.listes.len() {
                let t = self.listes[k].titre.clone();
                ui.selectable_value(&mut self.actuelle, k, t);
            }
            if ui.small_button("➕ Nouvelle").clicked() {
                self.listes.push(TierList { titre: format!("Tier list {}", self.listes.len() + 1), reserve: Vec::new(), ..TierList::default() });
                self.actuelle = self.listes.len() - 1;
            }
        });
        let a = self.actuelle.min(self.listes.len() - 1);
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.listes[a].titre).desired_width(240.0));
            ui.add(egui::TextEdit::singleline(&mut self.saisie).hint_text("nouvel élément").desired_width(160.0));
            if ui.button("➕").clicked() && !self.saisie.trim().is_empty() {
                let t = std::mem::take(&mut self.saisie).trim().to_string();
                self.listes[a].reserve.push(Element { texte: t, image: None });
            }
            if ui.button("🖼 Images…").clicked() {
                for p in rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"]).pick_files().unwrap_or_default() {
                    let texte = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                    self.listes[a].reserve.push(Element { texte, image: Some(p) });
                }
            }
            if ui.button("💾 Image à partager…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("tier_list.png").save_file() {
                    let f = crate::creatif::police(&["arialbd.ttf", "segoeuib.ttf", "DejaVuSans-Bold.ttf"]);
                    self.message = image_tier(&self.listes[a], f.as_ref()).save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if self.listes.len() > 1 && ui.small_button("🗑 Supprimer la liste").clicked() {
                self.listes.remove(a);
                self.actuelle = 0;
                return;
            }
        });
        ui.label(&self.message);
        ui.weak("Glisse les éléments d'une ligne à l'autre. Clic droit pour retirer un élément.");
        let a = self.actuelle.min(self.listes.len() - 1);
        let mut mouvement: Option<((usize, usize), usize, usize)> = None;
        let mut retirer: Option<(usize, usize)> = None;
        let n_rangs = self.listes[a].rangs.len();
        for r in (0..n_rangs).chain([usize::MAX]) {
            let (nom, couleur, elems) = if r == usize::MAX {
                ("Réserve".to_string(), [70u8, 70, 76], self.listes[a].reserve.clone())
            } else {
                let x = &self.listes[a].rangs[r];
                (x.0.clone(), x.1, x.2.clone())
            };
            let cadre = egui::Frame::new().fill(Color32::from_gray(32)).inner_margin(4.0);
            let (_, depot) = ui.dnd_drop_zone::<(usize, usize), ()>(cadre, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    let (bloc, _) = ui.allocate_exact_size(Vec2::new(70.0, 64.0), Sense::hover());
                    ui.painter().rect_filled(bloc, 4.0, Color32::from_rgb(couleur[0], couleur[1], couleur[2]));
                    ui.painter().text(bloc.center(), egui::Align2::CENTER_CENTER, &nom, FontId::proportional(if r == usize::MAX { 14.0 } else { 28.0 }), Color32::BLACK);
                    for (i, e) in elems.iter().enumerate() {
                        let id = egui::Id::new(("tier", a, r, i));
                        let rep = ui.dnd_drag_source(id, (r, i), |ui| self.element_ui(ui, e)).response;
                        rep.context_menu(|ui| {
                            if ui.button("🗑 Retirer").clicked() {
                                retirer = Some((r, i));
                                ui.close_menu();
                            }
                        });
                    }
                });
            });
            if let Some(de) = depot {
                mouvement = Some((*de, r, usize::MAX));
            }
        }
        if let Some((de, vers, pos)) = mouvement {
            deplacer(&mut self.listes[a], de, vers, pos);
        }
        if let Some((r, i)) = retirer {
            if r == usize::MAX {
                self.listes[a].reserve.remove(i);
            } else {
                self.listes[a].rangs[r].2.remove(i);
            }
        }
        if self.listes != avant {
            enregistrer("tier_lists", &self.listes);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 4 · Jeu de rythme
// ---------------------------------------------------------------------------------------------------

/// Notes (temps en secondes, piste 0..4) détectées dans le son : débuts de sons forts dans 4 bandes.
pub fn detecter_notes(e: &[f32], freq: u32, difficulte: u8) -> Vec<(f32, u8)> {
    let fen = 1024usize;
    let pas = 512usize;
    if e.len() < fen * 4 {
        return Vec::new();
    }
    let mut energies: Vec<[f32; 4]> = Vec::new();
    let mut k = 0;
    while k + fen <= e.len() {
        let b = crate::scene::bandes(&e[k..k + fen], freq, 4);
        energies.push([b[0], b[1], b[2], b[3]]);
        k += pas;
    }
    let seuil = [1.5, 1.35, 1.2][difficulte.min(2) as usize];
    let ecart = [0.45, 0.3, 0.18][difficulte.min(2) as usize];
    let mut notes = Vec::new();
    let mut dernier = [-10.0f32; 4];
    let mut global = -10.0f32;
    let t_pas = pas as f32 / freq as f32;
    let fenetre = (1.0 / t_pas) as usize; // ~1 s de moyenne
    for i in 1..energies.len() {
        let t = i as f32 * t_pas;
        for b in 0..4 {
            let debut = i.saturating_sub(fenetre);
            let moy = energies[debut..i].iter().map(|x| x[b]).sum::<f32>() / (i - debut).max(1) as f32;
            let v = energies[i][b];
            let monte = v - energies[i - 1][b];
            if v > moy * seuil && monte > 0.04 && v > 0.25 && t - dernier[b] > ecart && t - global > ecart / 2.5 {
                notes.push((t, b as u8));
                dernier[b] = t;
                global = t;
            }
        }
    }
    notes
}

#[derive(Default)]
struct Chanson {
    wav: Option<PathBuf>,
    notes: Vec<(f32, u8)>,
    duree: f32,
    message: String,
    en_cours: bool,
}

pub struct Rythme {
    fichier: Option<PathBuf>,
    chanson: Arc<Mutex<Chanson>>,
    difficulte: u8,
    debut: Option<Instant>,
    stop: Arc<AtomicBool>,
    touchees: Vec<Option<bool>>, // par note : None = pas encore, Some(true) = touchée
    score: (u32, u32, u32, u32), // parfait, bien, raté, combo
    meilleur_combo: u32,
    eclats: Vec<(Instant, u8, &'static str)>,
    records: Vec<(String, u32)>,
}

const TOUCHES_RYTHME: [(egui::Key, &str); 4] = [(egui::Key::D, "D"), (egui::Key::F, "F"), (egui::Key::J, "J"), (egui::Key::K, "K")];
const AVANCE: f32 = 1.6; // secondes pour qu'une note descende

impl Rythme {
    pub fn new() -> Self {
        Rythme { fichier: None, chanson: Arc::default(), difficulte: 1, debut: None, stop: Arc::new(AtomicBool::new(false)), touchees: Vec::new(), score: (0, 0, 0, 0), meilleur_combo: 0, eclats: Vec::new(), records: charger("rythme_records") }
    }

    fn preparer(&self) {
        let Some(f) = self.fichier.clone() else { return };
        let (c, d) = (self.chanson.clone(), self.difficulte);
        c.lock().unwrap().en_cours = true;
        c.lock().unwrap().message = "Analyse de la musique…".into();
        std::thread::spawn(move || {
            let r = (|| -> Result<(PathBuf, Vec<(f32, u8)>, f32), String> {
                let wav = std::env::temp_dir().join("outils_rythme.wav");
                let o = crate::createur::ffmpeg()?.args(["-y", "-v", "error", "-i"]).arg(&f).args(["-vn", "-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"]).arg(&wav).output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec").to_string());
                }
                let o = crate::createur::ffmpeg()?.args(["-v", "error", "-i"]).arg(&f).args(["-vn", "-ac", "1", "-ar", "22050", "-f", "f32le", "-"]).output().map_err(|e| e.to_string())?;
                let e: Vec<f32> = o.stdout.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                Ok((wav, detecter_notes(&e, 22050, d), e.len() as f32 / 22050.0))
            })();
            let mut x = c.lock().unwrap();
            x.en_cours = false;
            match r {
                Ok((w, n, d)) => {
                    x.message = format!("✔ {} notes. Prêt !", n.len());
                    x.wav = Some(w);
                    x.notes = n;
                    x.duree = d;
                }
                Err(e) => x.message = format!("✖ {e}"),
            }
        });
    }

    fn lancer(&mut self) {
        let Some(w) = self.chanson.lock().unwrap().wav.clone() else { return };
        self.stop.store(true, Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        crate::scene::jouer_wav(w, self.stop.clone());
        self.debut = Some(Instant::now());
        self.touchees = vec![None; self.chanson.lock().unwrap().notes.len()];
        self.score = (0, 0, 0, 0);
        self.meilleur_combo = 0;
    }

    fn points(&self) -> u32 {
        self.score.0 * 300 + self.score.1 * 100
    }
}

impl Drop for Rythme {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

impl Outil for Rythme {
    fn nom(&self) -> &'static str {
        "🥁 Jeu de rythme"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let (pret, msg, en_cours, duree) = {
            let c = self.chanson.lock().unwrap();
            (c.wav.is_some(), c.message.clone(), c.en_cours, c.duree)
        };
        ui.horizontal_wrapped(|ui| {
            ui.heading("Jeu de rythme");
            if self.debut.is_none() {
                if ui.button("🎵 Choisir une musique…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Musique", &["mp3", "m4a", "flac", "wav", "ogg", "opus"]).pick_file() {
                        self.fichier = Some(p);
                        self.preparer();
                    }
                }
                for (d, n) in [(0u8, "Facile"), (1, "Normal"), (2, "Difficile")] {
                    if ui.selectable_label(self.difficulte == d, n).clicked() && self.difficulte != d {
                        self.difficulte = d;
                        self.preparer();
                    }
                }
                if ui.add_enabled(pret && !en_cours, egui::Button::new(RichText::new("▶ Jouer").strong())).clicked() {
                    self.lancer();
                }
            } else if ui.button("⏹ Arrêter").clicked() {
                self.stop.store(true, Relaxed);
                self.debut = None;
            }
            if en_cours {
                ui.spinner();
            }
            ui.label(msg);
        });
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(520.0), ui.available_height().max(300.0)), Sense::hover());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 8.0, Color32::from_rgb(16, 14, 26));
        let lw = zone.width() / 4.0;
        let ligne = zone.bottom() - 70.0;
        let couleurs = [Color32::from_rgb(240, 90, 120), Color32::from_rgb(90, 200, 250), Color32::from_rgb(250, 210, 80), Color32::from_rgb(120, 230, 120)];
        let appuis: Vec<bool> = TOUCHES_RYTHME.iter().map(|(k, _)| ui.input(|i| i.key_pressed(*k))).collect();
        let tenues: Vec<bool> = TOUCHES_RYTHME.iter().map(|(k, _)| ui.input(|i| i.key_down(*k))).collect();
        for k in 0..4 {
            let x = zone.left() + lw * k as f32;
            p.line_segment([Pos2::new(x, zone.top()), Pos2::new(x, zone.bottom())], Stroke::new(1.0_f32, Color32::from_gray(40)));
            let c = Pos2::new(x + lw / 2.0, ligne);
            p.circle_stroke(c, lw * 0.32, Stroke::new(3.0_f32, couleurs[k]));
            if tenues[k] {
                p.circle_filled(c, lw * 0.32, couleurs[k].gamma_multiply(0.4));
            }
            p.text(Pos2::new(c.x, zone.bottom() - 25.0), egui::Align2::CENTER_CENTER, TOUCHES_RYTHME[k].1, FontId::proportional(20.0), Color32::WHITE);
        }
        let Some(debut) = self.debut else {
            let t = if pret { "Touches D F J K. Appuie quand la note arrive sur le cercle." } else { "Choisis une musique (MP3…) : les notes sont créées à partir du rythme." };
            p.text(zone.center(), egui::Align2::CENTER_CENTER, t, FontId::proportional(15.0), Color32::WHITE);
            if let Some(f) = &self.fichier {
                let nom = f.file_stem().unwrap_or_default().to_string_lossy().to_string();
                if let Some((_, r)) = self.records.iter().find(|r| r.0 == nom) {
                    p.text(zone.center() + Vec2::new(0.0, 30.0), egui::Align2::CENTER_CENTER, format!("🏆 Record : {r}"), FontId::proportional(15.0), Color32::GOLD);
                }
            }
            return;
        };
        let t = debut.elapsed().as_secs_f32();
        let notes = self.chanson.lock().unwrap().notes.clone();
        // jugement des appuis
        for (k, appui) in appuis.iter().enumerate() {
            if !appui {
                continue;
            }
            let proche = notes.iter().enumerate().filter(|(i, n)| n.1 as usize == k && self.touchees[*i].is_none() && (n.0 - t).abs() < 0.2).min_by(|a, b| (a.1 .0 - t).abs().total_cmp(&(b.1 .0 - t).abs()));
            if let Some((i, n)) = proche {
                let ecart = (n.0 - t).abs();
                self.touchees[i] = Some(true);
                self.score.3 += 1;
                if ecart < 0.06 {
                    self.score.0 += 1;
                    self.eclats.push((Instant::now(), k as u8, "Parfait !"));
                } else {
                    self.score.1 += 1;
                    self.eclats.push((Instant::now(), k as u8, "Bien"));
                }
                self.meilleur_combo = self.meilleur_combo.max(self.score.3);
            }
        }
        // notes ratées
        for (i, n) in notes.iter().enumerate() {
            if self.touchees[i].is_none() && t - n.0 > 0.2 {
                self.touchees[i] = Some(false);
                self.score.2 += 1;
                self.score.3 = 0;
                self.eclats.push((Instant::now(), n.1, "Raté"));
            }
        }
        // dessin des notes qui descendent
        for (i, n) in notes.iter().enumerate() {
            if self.touchees[i].is_some() || n.0 < t - 0.2 || n.0 > t + AVANCE {
                continue;
            }
            let y = ligne - (n.0 - t) / AVANCE * (ligne - zone.top());
            let x = zone.left() + lw * n.1 as f32 + lw / 2.0;
            p.circle_filled(Pos2::new(x, y), lw * 0.27, couleurs[n.1 as usize]);
        }
        self.eclats.retain(|e| e.0.elapsed() < Duration::from_millis(500));
        for (q, k, texte) in &self.eclats {
            let a = 1.0 - q.elapsed().as_secs_f32() * 2.0;
            p.text(Pos2::new(zone.left() + lw * *k as f32 + lw / 2.0, ligne - 50.0 - q.elapsed().as_secs_f32() * 60.0), egui::Align2::CENTER_CENTER, *texte, FontId::proportional(18.0), Color32::from_white_alpha((a * 255.0) as u8));
        }
        p.text(zone.left_top() + Vec2::new(10.0, 10.0), egui::Align2::LEFT_TOP, format!("Score {}   Combo {}", self.points(), self.score.3), FontId::proportional(18.0), Color32::WHITE);
        if t > duree + 1.0 {
            self.debut = None;
            let total = (self.score.0 + self.score.1 + self.score.2).max(1);
            let nom = self.fichier.as_ref().map(|f| f.file_stem().unwrap_or_default().to_string_lossy().to_string()).unwrap_or_default();
            let s = self.points();
            match self.records.iter_mut().find(|r| r.0 == nom) {
                Some(r) if r.1 >= s => {}
                Some(r) => r.1 = s,
                None => self.records.push((nom, s)),
            }
            enregistrer("rythme_records", &self.records);
            self.chanson.lock().unwrap().message = format!("🎉 Fini ! {s} points · {} parfaits, {} bien, {} ratés · précision {} % · meilleur combo {}", self.score.0, self.score.1, self.score.2, (self.score.0 + self.score.1) * 100 / total, self.meilleur_combo);
        }
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------------
// 5 · Puzzle photo
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub col: usize,
    pub lig: usize,
    pub pos: Vec2, // coin haut-gauche, en fraction de la zone de jeu
    pub posee: bool,
}

/// Mélange les pièces autour du plateau.
pub fn melanger(cols: usize, ligs: usize) -> Vec<Piece> {
    let mut v: Vec<Piece> = (0..cols * ligs).map(|k| Piece { col: k % cols, lig: k / cols, pos: Vec2::ZERO, posee: false }).collect();
    for p in &mut v {
        // à droite du plateau (qui occupe la moitié gauche)
        p.pos = Vec2::new(0.55 + hasard_f() * 0.38, hasard_f() * (1.0 - 1.0 / ligs as f32));
    }
    let n = v.len();
    for i in (1..n).rev() {
        v.swap(i, crate::outil::hasard(i as u64 + 1) as usize);
    }
    v
}

pub struct Puzzle {
    image: Option<egui::TextureHandle>,
    ratio: f32,
    cols: usize,
    pieces: Vec<Piece>,
    tenue: Option<(usize, Vec2)>,
    debut: Option<Instant>,
    fini: Option<f32>,
    fantome: bool,
}

impl Puzzle {
    pub fn new() -> Self {
        Puzzle { image: None, ratio: 1.5, cols: 4, pieces: Vec::new(), tenue: None, debut: None, fini: None, fantome: true }
    }

    fn charger_image(&mut self, ctx: &egui::Context, p: &std::path::Path) {
        if let Ok(i) = image::open(p) {
            let i = i.thumbnail(1400, 1400).to_rgba8();
            self.ratio = i.width() as f32 / i.height() as f32;
            let c = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
            self.image = Some(ctx.load_texture("puzzle", c, Default::default()));
            self.nouvelle_partie();
        }
    }

    fn lignes(&self) -> usize {
        ((self.cols as f32 / self.ratio).round() as usize).max(2)
    }

    fn nouvelle_partie(&mut self) {
        self.pieces = melanger(self.cols, self.lignes());
        self.debut = Some(Instant::now());
        self.fini = None;
    }
}

fn photo_au_hasard() -> Option<PathBuf> {
    let m = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    let base = PathBuf::from(m);
    let d = [base.join("Pictures"), base.join("Images")].into_iter().find(|d| d.is_dir())?;
    let mut v = Vec::new();
    let mut pile = vec![d];
    while let Some(p) = pile.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let q = e.path();
            if q.is_dir() && v.len() < 2000 {
                pile.push(q);
            } else if q.extension().and_then(|e| e.to_str()).is_some_and(|e| ["jpg", "jpeg", "png"].contains(&e.to_lowercase().as_str())) {
                v.push(q);
            }
        }
    }
    (!v.is_empty()).then(|| v[crate::outil::hasard(v.len() as u64) as usize].clone())
}

impl Outil for Puzzle {
    fn nom(&self) -> &'static str {
        "🧩 Puzzle photo"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Puzzle photo");
            if ui.button("🖼 Choisir une photo…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file() {
                    self.charger_image(ui.ctx(), &p);
                }
            }
            if ui.button("🎲 Photo au hasard").on_hover_text("Une de tes photos (dossier Images)").clicked() {
                if let Some(p) = photo_au_hasard() {
                    self.charger_image(ui.ctx(), &p);
                }
            }
            ui.label("Pièces");
            for n in [3usize, 4, 6, 8, 10] {
                if ui.selectable_label(self.cols == n, format!("{}", n * ((n as f32 / self.ratio).round() as usize).max(2))).clicked() {
                    self.cols = n;
                    if self.image.is_some() {
                        self.nouvelle_partie();
                    }
                }
            }
            ui.checkbox(&mut self.fantome, "Modèle en transparence");
        });
        let Some(tex) = self.image.clone() else {
            ui.weak("Choisis une photo : elle devient un puzzle à reconstituer.");
            return;
        };
        let (cols, ligs) = (self.cols, self.lignes());
        let (zone, rep) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 6.0, Color32::from_rgb(28, 30, 36));
        // plateau : moitié gauche, au bon format
        let lp = (zone.width() * 0.5).min(zone.height() * self.ratio) - 10.0;
        let hp = lp / self.ratio;
        let plateau = Rect::from_min_size(zone.min + Vec2::new(8.0, (zone.height() - hp) / 2.0), Vec2::new(lp, hp));
        p.rect_filled(plateau, 2.0, Color32::from_gray(20));
        if self.fantome {
            p.image(tex.id(), plateau, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(45));
        }
        let (pw, ph) = (lp / cols as f32, hp / ligs as f32);
        let rect_piece = |pc: &Piece| -> Rect {
            if pc.posee {
                Rect::from_min_size(plateau.min + Vec2::new(pc.col as f32 * pw, pc.lig as f32 * ph), Vec2::new(pw, ph))
            } else {
                Rect::from_min_size(zone.min + Vec2::new(pc.pos.x * zone.width(), pc.pos.y * zone.height()), Vec2::new(pw, ph))
            }
        };
        // attraper / lâcher
        if rep.drag_started() {
            if let Some(h) = rep.interact_pointer_pos() {
                if let Some(i) = (0..self.pieces.len()).rev().find(|&i| !self.pieces[i].posee && rect_piece(&self.pieces[i]).contains(h)) {
                    let pc = self.pieces.remove(i);
                    let decal = h - rect_piece(&pc).min;
                    self.pieces.push(pc);
                    self.tenue = Some((self.pieces.len() - 1, decal));
                }
            }
        }
        if let (Some((i, decal)), Some(h)) = (self.tenue, rep.interact_pointer_pos()) {
            let coin = h - decal - zone.min;
            self.pieces[i].pos = Vec2::new(coin.x / zone.width(), coin.y / zone.height());
        }
        if rep.drag_stopped() {
            if let Some((i, _)) = self.tenue.take() {
                let pc = self.pieces[i];
                let juste = plateau.min + Vec2::new(pc.col as f32 * pw, pc.lig as f32 * ph);
                if rect_piece(&pc).min.distance(juste) < pw.min(ph) * 0.25 {
                    self.pieces[i].posee = true;
                    // les pièces posées passent dessous
                    let pc = self.pieces.remove(i);
                    self.pieces.insert(0, pc);
                }
            }
        }
        for pc in &self.pieces {
            let r = rect_piece(pc);
            let uv = Rect::from_min_size(Pos2::new(pc.col as f32 / cols as f32, pc.lig as f32 / ligs as f32), Vec2::new(1.0 / cols as f32, 1.0 / ligs as f32));
            p.image(tex.id(), r, uv, Color32::WHITE);
            if !pc.posee {
                p.rect_stroke(r, 0.0, Stroke::new(1.0_f32, Color32::from_black_alpha(160)), egui::StrokeKind::Inside);
            }
        }
        let posees = self.pieces.iter().filter(|p| p.posee).count();
        let total = self.pieces.len();
        if self.fini.is_none() && posees == total && total > 0 {
            self.fini = self.debut.map(|d| d.elapsed().as_secs_f32());
        }
        let texte = match self.fini {
            Some(s) => format!("🎉 Bravo ! Terminé en {} min {:02} s", s as u32 / 60, s as u32 % 60),
            None => format!("{posees} / {total} pièces · {} s", self.debut.map(|d| d.elapsed().as_secs()).unwrap_or(0)),
        };
        p.text(zone.right_top() + Vec2::new(-10.0, 8.0), egui::Align2::RIGHT_TOP, texte, FontId::proportional(16.0), Color32::WHITE);
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jeux() {
        assert!(derive(0.1, 0.05) && !derive(0.02, 0.01));
        let mut t = TierList::default();
        let n = t.reserve.len();
        deplacer(&mut t, (usize::MAX, 0), 0, 0);
        assert_eq!(t.rangs[0].2[0].texte, "Minecraft");
        assert_eq!(t.reserve.len(), n - 1);
        deplacer(&mut t, (0, 0), 2, 5);
        assert!(t.rangs[0].2.is_empty() && t.rangs[2].2.len() == 1);
        deplacer(&mut t, (9, 0), 0, 0); // rang inexistant : rien ne bouge
        assert_eq!(t.rangs[2].2.len(), 1);
        let img = image_tier(&t, None);
        assert_eq!(img.width(), 1100);
        let p = melanger(4, 3);
        assert_eq!(p.len(), 12);
        let mut vus: Vec<(usize, usize)> = p.iter().map(|x| (x.col, x.lig)).collect();
        vus.sort();
        vus.dedup();
        assert_eq!(vus.len(), 12);
        assert!(p.iter().all(|x| x.pos.x >= 0.55 && !x.posee));
        // détection de notes : des coups réguliers
        let freq = 22050;
        let mut e = vec![0.0f32; freq as usize * 6];
        for k in 0..12 {
            let d = (k as f32 * 0.5 * freq as f32) as usize + freq as usize;
            for i in 0..2000 {
                if d + i < e.len() {
                    e[d + i] = ((i as f32 * 0.3).sin()) * (1.0 - i as f32 / 2000.0);
                }
            }
        }
        let n = detecter_notes(&e, freq, 1);
        assert!(n.len() >= 6, "{} notes", n.len());
        assert!(n.windows(2).all(|w| w[1].0 >= w[0].0));
    }
}
