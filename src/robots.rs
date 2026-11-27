//! Combat de robots codés (5) : chaque robot suit ses règles « Si … alors … » (la première règle vraie
//! gagne). On les fait s'affronter dans une arène ; les robots se partagent par un code.
use crate::outil::{charger, enregistrer, Cat, Outil};
use base64::Engine;
use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Condition {
    Toujours,
    EnnemiDevant,
    EnnemiAGauche,
    EnnemiADroite,
    EnnemiProche,
    MurDevant,
    Touche,
    EnergieBasse,
    BalleProche,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Avancer,
    Reculer,
    TournerGauche,
    TournerDroite,
    Tirer,
    Esquiver,
    Attendre,
}

pub const CONDITIONS: [(Condition, &str); 9] = [
    (Condition::Toujours, "toujours"),
    (Condition::EnnemiDevant, "un ennemi est devant"),
    (Condition::EnnemiAGauche, "un ennemi est à gauche"),
    (Condition::EnnemiADroite, "un ennemi est à droite"),
    (Condition::EnnemiProche, "un ennemi est tout près"),
    (Condition::MurDevant, "un mur est devant"),
    (Condition::Touche, "je viens d'être touché"),
    (Condition::EnergieBasse, "mon énergie est basse"),
    (Condition::BalleProche, "une balle arrive"),
];

pub const ACTIONS: [(Action, &str); 7] = [
    (Action::Avancer, "avancer"),
    (Action::Reculer, "reculer"),
    (Action::TournerGauche, "tourner à gauche"),
    (Action::TournerDroite, "tourner à droite"),
    (Action::Tirer, "tirer"),
    (Action::Esquiver, "faire un pas de côté"),
    (Action::Attendre, "attendre"),
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Robot {
    pub nom: String,
    pub couleur: [u8; 3],
    pub regles: Vec<(Condition, Vec<Action>)>,
}

/// Robots tout prêts, à battre.
pub fn adversaires() -> Vec<Robot> {
    use Action::*;
    use Condition::*;
    vec![
        Robot { nom: "Tourelle".into(), couleur: [160, 160, 170], regles: vec![(EnnemiDevant, vec![Tirer]), (Toujours, vec![TournerDroite])] },
        Robot { nom: "Fonceur".into(), couleur: [230, 80, 60], regles: vec![(MurDevant, vec![TournerDroite]), (EnnemiDevant, vec![Avancer, Tirer]), (EnnemiAGauche, vec![TournerGauche]), (EnnemiADroite, vec![TournerDroite]), (Toujours, vec![Avancer, TournerGauche])] },
        Robot { nom: "Esquiveur".into(), couleur: [80, 200, 230], regles: vec![(BalleProche, vec![Esquiver]), (MurDevant, vec![TournerGauche]), (EnnemiDevant, vec![Tirer]), (EnnemiAGauche, vec![TournerGauche]), (EnnemiADroite, vec![TournerDroite]), (Toujours, vec![Avancer])] },
        Robot { nom: "Prudent".into(), couleur: [120, 200, 100], regles: vec![(EnnemiProche, vec![Reculer, Tirer]), (Touche, vec![Esquiver]), (EnnemiDevant, vec![Tirer]), (EnnemiAGauche, vec![TournerGauche]), (EnnemiADroite, vec![TournerDroite]), (MurDevant, vec![TournerDroite]), (Toujours, vec![TournerGauche])] },
    ]
}

/// Code à partager (« ROBOT: » + JSON en Base64).
pub fn code(r: &Robot) -> String {
    format!("ROBOT:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(r).unwrap_or_default()))
}

pub fn lire_code(s: &str) -> Option<Robot> {
    let b = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim().strip_prefix("ROBOT:")?).ok()?;
    let r: Robot = serde_json::from_slice(&b).ok()?;
    (r.regles.len() <= 30).then_some(r)
}

pub const ARENE: Vec2 = Vec2::new(640.0, 420.0);
const RAYON: f32 = 14.0;
const VITESSE: f32 = 90.0;
const ROTATION: f32 = 200.0; // degrés par seconde
const RECHARGE: f32 = 0.55;
const VITESSE_BALLE: f32 = 320.0;
const DEGATS: f32 = 10.0;

#[derive(Clone, Debug)]
pub struct EnJeu {
    pub robot: Robot,
    pub p: Vec2,
    pub cap: f32, // degrés, 0 = vers la droite
    pub energie: f32,
    pub recharge: f32,
    pub touche: f32,
    pub tirs: u32,
    pub reussis: u32,
}

#[derive(Clone, Debug)]
pub struct Balle {
    pub p: Vec2,
    pub v: Vec2,
    pub tireur: usize,
}

pub struct Combat {
    pub robots: Vec<EnJeu>,
    pub balles: Vec<Balle>,
    pub temps: f32,
    pub fini: Option<Option<usize>>,
}

fn dir(cap: f32) -> Vec2 {
    let r = cap.to_radians();
    Vec2::new(r.cos(), r.sin())
}

/// Angle signé (degrés, −180..180) de la cible vue depuis le robot.
fn angle_vers(e: &EnJeu, cible: Vec2) -> f32 {
    let d = cible - e.p;
    let a = d.y.atan2(d.x).to_degrees();
    (a - e.cap + 540.0).rem_euclid(360.0) - 180.0
}

impl Combat {
    pub fn nouveau(robots: Vec<Robot>) -> Self {
        let places = [Vec2::new(80.0, 80.0), Vec2::new(560.0, 340.0), Vec2::new(560.0, 80.0), Vec2::new(80.0, 340.0)];
        let robots = robots.into_iter().take(4).enumerate().map(|(k, r)| {
            let p = places[k];
            let c = ARENE / 2.0 - p;
            EnJeu { robot: r, p, cap: c.y.atan2(c.x).to_degrees(), energie: 100.0, recharge: 0.0, touche: 99.0, tirs: 0, reussis: 0 }
        }).collect();
        Combat { robots, balles: Vec::new(), temps: 0.0, fini: None }
    }

    fn vrai(&self, i: usize, c: Condition) -> bool {
        let e = &self.robots[i];
        let ennemis = self.robots.iter().enumerate().filter(|(j, x)| *j != i && x.energie > 0.0);
        match c {
            Condition::Toujours => true,
            Condition::EnnemiDevant => ennemis.clone().any(|(_, x)| angle_vers(e, x.p).abs() < 8.0),
            Condition::EnnemiAGauche => ennemis.clone().map(|(_, x)| (angle_vers(e, x.p), (x.p - e.p).length())).min_by(|a, b| a.1.total_cmp(&b.1)).is_some_and(|(a, _)| a < -8.0),
            Condition::EnnemiADroite => ennemis.clone().map(|(_, x)| (angle_vers(e, x.p), (x.p - e.p).length())).min_by(|a, b| a.1.total_cmp(&b.1)).is_some_and(|(a, _)| a > 8.0),
            Condition::EnnemiProche => ennemis.clone().any(|(_, x)| (x.p - e.p).length() < 110.0),
            Condition::MurDevant => {
                let q = e.p + dir(e.cap) * 45.0;
                q.x < RAYON || q.y < RAYON || q.x > ARENE.x - RAYON || q.y > ARENE.y - RAYON
            }
            Condition::Touche => e.touche < 0.6,
            Condition::EnergieBasse => e.energie < 35.0,
            Condition::BalleProche => self.balles.iter().any(|b| b.tireur != i && (b.p - e.p).length() < 90.0 && b.v.dot(e.p - b.p) > 0.0),
        }
    }

    pub fn avancer(&mut self, dt: f32) {
        if self.fini.is_some() {
            return;
        }
        self.temps += dt;
        for i in 0..self.robots.len() {
            if self.robots[i].energie <= 0.0 {
                continue;
            }
            let actions = self.robots[i].robot.regles.iter().find(|(c, _)| self.vrai(i, *c)).map(|(_, a)| a.clone()).unwrap_or_default();
            let e = &mut self.robots[i];
            e.recharge -= dt;
            e.touche += dt;
            for a in actions.iter().take(2) {
                match a {
                    Action::Avancer => e.p += dir(e.cap) * VITESSE * dt,
                    Action::Reculer => e.p -= dir(e.cap) * VITESSE * 0.7 * dt,
                    Action::TournerGauche => e.cap -= ROTATION * dt,
                    Action::TournerDroite => e.cap += ROTATION * dt,
                    Action::Esquiver => e.p += dir(e.cap + 90.0) * VITESSE * dt,
                    Action::Tirer if e.recharge <= 0.0 => {
                        e.recharge = RECHARGE;
                        e.tirs += 1;
                        self.balles.push(Balle { p: e.p + dir(e.cap) * (RAYON + 3.0), v: dir(e.cap) * VITESSE_BALLE, tireur: i });
                    }
                    _ => {}
                }
            }
            let e = &mut self.robots[i];
            e.p = Vec2::new(e.p.x.clamp(RAYON, ARENE.x - RAYON), e.p.y.clamp(RAYON, ARENE.y - RAYON));
        }
        // les robots ne se traversent pas
        for i in 0..self.robots.len() {
            for j in i + 1..self.robots.len() {
                let d = self.robots[j].p - self.robots[i].p;
                let l = d.length();
                if l < 2.0 * RAYON && l > 1e-3 {
                    let c = d / l * (2.0 * RAYON - l) / 2.0;
                    self.robots[i].p -= c;
                    self.robots[j].p += c;
                }
            }
        }
        // balles
        let mut gardees = Vec::new();
        for mut b in std::mem::take(&mut self.balles) {
            b.p += b.v * dt;
            if b.p.x < 0.0 || b.p.y < 0.0 || b.p.x > ARENE.x || b.p.y > ARENE.y {
                continue;
            }
            let cible = self.robots.iter().position(|r| r.energie > 0.0 && (r.p - b.p).length() < RAYON);
            match cible {
                Some(k) if k != b.tireur => {
                    self.robots[k].energie -= DEGATS;
                    self.robots[k].touche = 0.0;
                    self.robots[b.tireur].reussis += 1;
                }
                Some(_) => gardees.push(b),
                None => gardees.push(b),
            }
        }
        self.balles = gardees;
        let vivants: Vec<usize> = (0..self.robots.len()).filter(|&i| self.robots[i].energie > 0.0).collect();
        if vivants.len() <= 1 {
            self.fini = Some(vivants.first().copied());
        } else if self.temps > 90.0 {
            // temps écoulé : le plus d'énergie gagne
            self.fini = Some(vivants.into_iter().max_by(|a, b| self.robots[*a].energie.total_cmp(&self.robots[*b].energie)));
        }
    }
}

pub struct CombatRobots {
    mes: Vec<Robot>,
    choisi: usize,
    participants: Vec<bool>, // mes robots puis adversaires
    combat: Option<Combat>,
    vitesse: f32,
    code_colle: String,
    message: String,
}

impl CombatRobots {
    pub fn new() -> Self {
        let mut mes: Vec<Robot> = charger("robots");
        if mes.is_empty() {
            mes.push(Robot { nom: "Mon robot".into(), couleur: [250, 200, 60], regles: vec![(Condition::EnnemiDevant, vec![Action::Tirer]), (Condition::Toujours, vec![Action::TournerDroite])] });
        }
        CombatRobots { participants: vec![false; mes.len() + 4], mes, choisi: 0, combat: None, vitesse: 1.0, code_colle: String::new(), message: String::new() }
    }

    fn tous(&self) -> Vec<Robot> {
        self.mes.iter().cloned().chain(adversaires()).collect()
    }
}

impl Outil for CombatRobots {
    fn nom(&self) -> &'static str {
        "🤖 Combat de robots codés"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.mes.clone();
        ui.heading("Combat de robots codés");
        if self.combat.is_some() {
            // --- arène ---
            ui.horizontal(|ui| {
                if ui.button("⬅ Retour à l'atelier").clicked() {
                    self.combat = None;
                    return;
                }
                ui.label("Vitesse");
                ui.add(egui::Slider::new(&mut self.vitesse, 0.25..=4.0).show_value(false));
            });
            let Some(c) = &mut self.combat else { return };
            let dt = ui.input(|i| i.stable_dt).min(0.05) * self.vitesse;
            let n = (dt / (1.0 / 60.0)).ceil().max(1.0) as usize;
            for _ in 0..n {
                c.avancer(dt / n as f32);
            }
            let ech = (ui.available_width() / ARENE.x).min((ui.available_height() - 80.0) / ARENE.y).max(0.4);
            let (zone, _) = ui.allocate_exact_size(ARENE * ech, Sense::hover());
            let p = ui.painter_at(zone);
            p.rect_filled(zone, 6.0, Color32::from_rgb(28, 30, 38));
            p.rect_stroke(zone, 6.0, Stroke::new(3.0_f32, Color32::from_gray(90)), egui::StrokeKind::Inside);
            let vers = |v: Vec2| zone.min + v * ech;
            for b in &c.balles {
                p.circle_filled(vers(b.p), 3.0 * ech, Color32::YELLOW);
            }
            for r in &c.robots {
                let col = Color32::from_rgb(r.robot.couleur[0], r.robot.couleur[1], r.robot.couleur[2]);
                if r.energie <= 0.0 {
                    p.circle_filled(vers(r.p), RAYON * ech, Color32::from_gray(50));
                    continue;
                }
                let q = vers(r.p);
                p.circle_filled(q, RAYON * ech, if r.touche < 0.15 { Color32::WHITE } else { col });
                p.line_segment([q, q + dir(r.cap) * (RAYON + 8.0) * ech], Stroke::new(4.0 * ech, Color32::from_gray(220)));
                let barre = Rect::from_min_size(q + Vec2::new(-RAYON, -RAYON - 9.0) * ech, Vec2::new(2.0 * RAYON * ech, 4.0));
                p.rect_filled(barre, 1.0, Color32::from_gray(40));
                p.rect_filled(Rect::from_min_size(barre.min, Vec2::new(barre.width() * r.energie / 100.0, 4.0)), 1.0, Color32::from_rgb(90, 220, 100));
                p.text(q + Vec2::new(0.0, (RAYON + 10.0) * ech), egui::Align2::CENTER_CENTER, &r.robot.nom, FontId::proportional(11.0), Color32::WHITE);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("⏱ {:.0} s", c.temps));
                for r in &c.robots {
                    ui.label(format!("{} : {:.0} ❤ · {}/{} tirs réussis", r.robot.nom, r.energie.max(0.0), r.reussis, r.tirs));
                }
            });
            match c.fini {
                Some(Some(k)) => {
                    ui.label(RichText::new(format!("🏆 {} gagne !", c.robots[k].robot.nom)).size(22.0).color(Color32::GOLD));
                }
                Some(None) => {
                    ui.label(RichText::new("Égalité !").size(22.0));
                }
                None => ui.ctx().request_repaint(),
            }
            return;
        }
        // --- atelier ---
        ui.weak("Ton robot lit ses règles de haut en bas à chaque instant et fait ce que dit la première règle vraie (une ou deux actions).");
        ui.horizontal_wrapped(|ui| {
            for k in 0..self.mes.len() {
                let n = self.mes[k].nom.clone();
                ui.selectable_value(&mut self.choisi, k, format!("🤖 {n}"));
            }
            if ui.small_button("➕ Nouveau robot").clicked() {
                self.mes.push(Robot { nom: format!("Robot {}", self.mes.len() + 1), couleur: [100 + (self.mes.len() as u8 * 60) % 150, 150, 220], regles: vec![(Condition::Toujours, vec![Action::Avancer])] });
                self.choisi = self.mes.len() - 1;
                self.participants.insert(self.mes.len() - 1, false);
            }
        });
        let k = self.choisi.min(self.mes.len() - 1);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.mes[k].nom).desired_width(160.0));
            ui.color_edit_button_srgb(&mut self.mes[k].couleur);
            if ui.small_button("📋 Copier le code").on_hover_text("Pour l'envoyer à un ami").clicked() {
                ui.ctx().copy_text(code(&self.mes[k]));
                self.message = "Code copié : colle-le à un ami !".into();
            }
            if self.mes.len() > 1 && ui.small_button("🗑").clicked() {
                self.mes.remove(k);
                self.participants.remove(k);
                self.choisi = 0;
            }
        });
        let k = self.choisi.min(self.mes.len() - 1);
        let mut action: Option<(usize, i32)> = None;
        let regles = &mut self.mes[k].regles;
        for (i, (c, a)) in regles.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", i + 1)).weak());
                ui.label("Si");
                egui::ComboBox::from_id_salt(("cond", k, i)).selected_text(CONDITIONS.iter().find(|x| x.0 == *c).map(|x| x.1).unwrap_or("")).show_ui(ui, |ui| {
                    for (v, n) in CONDITIONS {
                        ui.selectable_value(c, v, n);
                    }
                });
                ui.label("alors");
                for j in 0..2 {
                    let texte = a.get(j).map(|x| ACTIONS.iter().find(|y| y.0 == *x).map(|y| y.1).unwrap_or("")).unwrap_or(if j == 0 { "…" } else { "(rien d'autre)" });
                    egui::ComboBox::from_id_salt(("act", k, i, j)).selected_text(texte).show_ui(ui, |ui| {
                        for (v, n) in ACTIONS {
                            if ui.selectable_label(a.get(j) == Some(&v), n).clicked() {
                                if j < a.len() {
                                    a[j] = v;
                                } else {
                                    a.push(v);
                                }
                            }
                        }
                        if j == 1 && ui.selectable_label(a.len() < 2, "(rien d'autre)").clicked() {
                            a.truncate(1);
                        }
                    });
                    if j == 0 {
                        ui.label("et");
                    }
                }
                if ui.small_button("⬆").clicked() {
                    action = Some((i, -1));
                }
                if ui.small_button("✖").clicked() {
                    action = Some((i, 0));
                }
            });
        }
        if let Some((i, m)) = action {
            if m == 0 {
                regles.remove(i);
            } else if i > 0 {
                regles.swap(i, i - 1);
            }
        }
        if regles.len() < 30 && ui.small_button("➕ Ajouter une règle").clicked() {
            regles.push((Condition::Toujours, vec![Action::Avancer]));
        }
        ui.separator();
        ui.strong("⚔ Combat");
        let tous = self.tous();
        self.participants.resize(tous.len(), false);
        ui.horizontal_wrapped(|ui| {
            for (i, r) in tous.iter().enumerate() {
                let t = if i < self.mes.len() { format!("🤖 {}", r.nom) } else { format!("🦾 {}", r.nom) };
                ui.checkbox(&mut self.participants[i], t);
            }
        });
        let choisis: Vec<Robot> = tous.iter().zip(&self.participants).filter(|(_, p)| **p).map(|(r, _)| r.clone()).collect();
        ui.horizontal(|ui| {
            if ui.add_enabled((2..=4).contains(&choisis.len()), egui::Button::new(RichText::new("⚔ Lancer le combat").strong())).on_disabled_hover_text("Choisis de 2 à 4 robots").clicked() {
                self.combat = Some(Combat::nouveau(choisis));
            }
            ui.add(egui::TextEdit::singleline(&mut self.code_colle).hint_text("colle le code ROBOT:… d'un ami").desired_width(220.0));
            if !self.code_colle.trim().is_empty() {
                match lire_code(&self.code_colle) {
                    Some(r) => {
                        if ui.button(format!("➕ Ajouter « {} »", r.nom)).clicked() {
                            self.mes.push(r);
                            self.participants.insert(self.mes.len() - 1, true);
                            self.code_colle.clear();
                        }
                    }
                    None => {
                        ui.colored_label(Color32::LIGHT_RED, "code invalide");
                    }
                }
            }
        });
        ui.label(&self.message);
        if self.mes != avant {
            enregistrer("robots", &self.mes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robots() {
        let a = adversaires();
        let r = lire_code(&code(&a[1])).unwrap();
        assert_eq!(r, a[1]);
        assert!(lire_code("ROBOT:@@").is_none());
        // un robot qui ne fait rien perd contre la tourelle
        let immobile = Robot { nom: "Immobile".into(), couleur: [0, 0, 0], regles: vec![(Condition::Toujours, vec![Action::Attendre])] };
        let mut c = Combat::nouveau(vec![a[0].clone(), immobile]);
        for _ in 0..60 * 95 {
            c.avancer(1.0 / 60.0);
        }
        assert_eq!(c.fini, Some(Some(0)), "{:?}", c.robots.iter().map(|r| r.energie).collect::<Vec<_>>());
        // tous les robots tout prêts s'affrontent sans bloquer et le combat se termine
        let mut c = Combat::nouveau(a);
        for _ in 0..60 * 95 {
            c.avancer(1.0 / 60.0);
        }
        assert!(c.fini.is_some());
        assert!(c.robots.iter().all(|r| r.p.x >= 0.0 && r.p.x <= ARENE.x));
        // angle : un ennemi juste devant
        let e = EnJeu { robot: immobile_robot(), p: Vec2::ZERO, cap: 0.0, energie: 100.0, recharge: 0.0, touche: 9.0, tirs: 0, reussis: 0 };
        assert!(angle_vers(&e, Vec2::new(100.0, 0.0)).abs() < 1e-3);
        assert!(angle_vers(&e, Vec2::new(0.0, 100.0)) > 80.0); // à droite (y vers le bas)
    }

    fn immobile_robot() -> Robot {
        Robot { nom: "x".into(), couleur: [0, 0, 0], regles: vec![] }
    }
}
