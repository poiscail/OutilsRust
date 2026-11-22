//! Jeux, suite : classement entre amis (21), bataille de boules de neige à deux et jeu de rythme (26).
use crate::android;
use crate::commun::*;
use crate::jeux_tel::appuis;
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use crate::son::{Son, Synthe};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ===================================================================================================
// 21 · Classement entre amis (groupe partagé sur jsonblob.com)
// ===================================================================================================

const SERVICE: &str = "https://jsonblob.com/api/jsonBlob";

/// Jour (numéro) du sudoku du jour, comme dans le jeu.
fn jour_local() -> i64 {
    let t = crate::minecraft::horodatage();
    let n = |a: usize, b: usize| t.get(a..b).and_then(|x| x.parse::<u32>().ok()).unwrap_or(1);
    crate::quotidien2::jours_depuis_1970(n(0, 4) as i32, n(4, 6), n(6, 8))
}

/// Mes scores, lus dans les records des jeux.
pub fn mes_scores() -> Value {
    let sud: Value = charger("sudoku_jour");
    let jour = jour_local();
    let sudoku = (sud["jour"].as_i64() == Some(jour) && sud["fini"].as_bool() == Some(true)).then(|| json!({ "jour": jour, "temps": sud["secondes"] }));
    let n = |f: &str, k: &str| -> Value {
        let j: Value = charger(f);
        if k.is_empty() { j } else { j[k].clone() }
    };
    let badges: Value = charger("badges");
    json!({
        "maj": maintenant(),
        "sudoku": sudoku,
        "serie": sud["serie"],
        "laby": n("labyrinthe", "niveau_max"),
        "reflexes": n("reflexes", ""),
        "rythme": n("rythme", "record"),
        "badges": badges["l"].as_object().map_or(0, |o| o.len()),
    })
}

/// Ajoute (ou remplace) mon entrée dans les données du groupe.
pub fn fusionner(mut groupe: Value, pseudo: &str, moi: Value) -> Value {
    if !groupe["joueurs"].is_object() {
        groupe = json!({ "joueurs": {} });
    }
    groupe["joueurs"][pseudo] = moi;
    groupe
}

/// Classement d'une épreuve : (pseudo, valeur), du meilleur au moins bon.
pub fn classement(groupe: &Value, epreuve: &str, jour: i64) -> Vec<(String, f64)> {
    let mut v: Vec<(String, f64)> = groupe["joueurs"].as_object().into_iter().flatten().filter_map(|(p, j)| {
        let x = match epreuve {
            "sudoku" => (j["sudoku"]["jour"].as_i64() == Some(jour)).then(|| j["sudoku"]["temps"].as_f64()).flatten()?,
            k => j[k].as_f64()?,
        };
        (x > 0.0).then(|| (p.clone(), x))
    }).collect();
    let plus_petit_mieux = matches!(epreuve, "sudoku" | "reflexes");
    v.sort_by(|a, b| if plus_petit_mieux { a.1.partial_cmp(&b.1).unwrap() } else { b.1.partial_cmp(&a.1).unwrap() });
    v
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Groupe {
    code: String,
    pseudo: String,
}

pub struct Classement {
    g: Groupe,
    donnees: Arc<Mutex<Option<Value>>>,
    e: Etat,
    rejoindre: String,
    synchro: Instant,
}

impl Classement {
    pub fn new() -> Self {
        Classement { g: charger("classement"), donnees: Arc::default(), e: Arc::default(), rejoindre: String::new(), synchro: Instant::now() - Duration::from_secs(600) }
    }

    fn synchroniser(&mut self) {
        let (g, d) = (self.g.clone(), self.donnees.clone());
        self.synchro = Instant::now();
        en_fond(&self.e, "Mise à jour du classement…", move || {
            let url = format!("{SERVICE}/{}", g.code);
            let actuel: Value = ureq::get(&url).set("Accept", "application/json").timeout(Duration::from_secs(15)).call().map_err(|e| format!("groupe introuvable ({e})"))?.into_json().map_err(|e| e.to_string())?;
            let nouveau = fusionner(actuel, &g.pseudo, mes_scores());
            ureq::put(&url).set("Content-Type", "application/json").timeout(Duration::from_secs(15)).send_json(&nouveau).map_err(|e| e.to_string())?;
            *d.lock().unwrap() = Some(nouveau);
            Ok(String::new())
        });
    }
}

impl Outil for Classement {
    fn nom(&self) -> &'static str {
        "🏆 Classement entre amis"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let occupe = etat_ui(ui, &self.e);
        if self.g.pseudo.is_empty() || self.g.code.is_empty() {
            ui.label(RichText::new("Compare tes scores avec tes amis").size(20.0).strong());
            ui.weak("Sudoku du jour, labyrinthe, réflexes, rythme et badges. Les scores et ton pseudo sont stockés en ligne (jsonblob.com), visibles par ceux qui ont le code du groupe : n'utilise pas ton vrai nom.");
            ui.add(egui::TextEdit::singleline(&mut self.g.pseudo).hint_text("ton pseudo").desired_width(f32::INFINITY));
            if !self.g.pseudo.trim().is_empty() {
                if gros_bouton(ui, "➕ Créer un groupe") && !occupe {
                    let g2 = Arc::new(Mutex::new(self.g.clone()));
                    en_fond(&self.e, "Création…", move || {
                        let r = ureq::post(SERVICE).set("Content-Type", "application/json").set("Accept", "application/json").timeout(Duration::from_secs(15)).send_json(json!({ "joueurs": {} })).map_err(|e| e.to_string())?;
                        let loc = r.header("Location").or(r.header("location")).ok_or("le service n'a pas donné de code")?.to_string();
                        let code = loc.rsplit('/').next().unwrap_or("").to_string();
                        let mut x = g2.lock().unwrap();
                        x.code = code;
                        enregistrer("classement", &*x);
                        Ok("✔ Groupe créé : partage le code à tes amis".into())
                    });
                }
                ui.label("ou rejoins le groupe d'un ami :");
                ui.add(egui::TextEdit::singleline(&mut self.rejoindre).hint_text("code du groupe").desired_width(f32::INFINITY));
                if ui.button("Rejoindre").clicked() && !self.rejoindre.trim().is_empty() {
                    self.g.code = self.rejoindre.trim().rsplit('/').next().unwrap_or("").to_string();
                    enregistrer("classement", &self.g);
                    self.synchroniser();
                }
            }
            // création terminée : on relit
            let relu: Groupe = charger("classement");
            if !relu.code.is_empty() && self.g.code.is_empty() {
                self.g.code = relu.code;
                self.synchroniser();
            }
            ui.ctx().request_repaint_after(Duration::from_millis(500));
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("👤 {}", self.g.pseudo)).strong());
            if ui.button("🔄").clicked() && !occupe {
                self.synchroniser();
            }
            if ui.button("📋 Copier le code").clicked() {
                android::presse_ecrire(&format!("Rejoins mon classement dans l'appli Outils (🏆 Classement entre amis), code : {}", self.g.code));
                *self.e.lock().unwrap() = (false, "Code copié : envoie-le à tes amis.".into());
            }
        });
        if self.synchro.elapsed() > Duration::from_secs(300) && !occupe {
            self.synchroniser();
        }
        let Some(d) = self.donnees.lock().unwrap().clone() else { return };
        let jour = jour_local();
        for (ep, titre, unite) in [("sudoku", "📅 Sudoku du jour", "s"), ("laby", "🎱 Labyrinthe (niveau)", ""), ("reflexes", "⚡ Réflexes", "ms"), ("rythme", "🥁 Rythme", "pts"), ("badges", "🏅 Badges", ""), ("serie", "🔥 Série de sudokus", "j")] {
            let l = classement(&d, ep, jour);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(titre).size(17.0).strong());
                if l.is_empty() {
                    ui.weak("Personne pour l'instant.");
                }
                for (k, (p, v)) in l.iter().enumerate() {
                    let med = ["🥇", "🥈", "🥉"].get(k).copied().unwrap_or("  ");
                    let t = if ep == "sudoku" { format!("{}:{:02}", *v as u64 / 60, *v as u64 % 60) } else { format!("{v:.0} {unite}") };
                    ui.label(RichText::new(format!("{med} {p} — {t}")).strong().color(if *p == self.g.pseudo { ROSE } else { ui.visuals().text_color() }));
                }
            });
        }
        if ui.small_button("Quitter le groupe").clicked() {
            self.g.code.clear();
            enregistrer("classement", &self.g);
        }
    }
}

// ===================================================================================================
// 26 · Bataille de boules de neige (à deux sur le même écran, ou contre le téléphone)
// ===================================================================================================

struct Boule {
    pos: Vec2,
    v: f32, // vers le haut (−) ou le bas (+), en fraction de hauteur par seconde
}

pub struct BoulesDeNeige {
    joueurs: [f32; 2], // position x (0..1) : 0 = bas, 1 = haut
    boules: Vec<Boule>,
    score: [u32; 2],
    recharge: [Instant; 2],
    touche: [Option<Instant>; 2],
    doigts: HashMap<u64, Pos2>,
    ordi: bool,
    dernier: Instant,
    flocons: Vec<Vec2>,
}

impl BoulesDeNeige {
    pub fn new() -> Self {
        let flocons = (0..60).map(|_| Vec2::new(hasard(1000) as f32 / 1000.0, hasard(1000) as f32 / 1000.0)).collect();
        BoulesDeNeige { joueurs: [0.5; 2], boules: vec![], score: [0; 2], recharge: [Instant::now(); 2], touche: [None; 2], doigts: HashMap::new(), ordi: true, dernier: Instant::now(), flocons }
    }

    fn lancer(&mut self, j: usize) {
        if self.recharge[j].elapsed() < Duration::from_millis(450) {
            return;
        }
        self.recharge[j] = Instant::now();
        let y = if j == 0 { 0.86 } else { 0.14 };
        self.boules.push(Boule { pos: Vec2::new(self.joueurs[j], y), v: if j == 0 { -0.9 } else { 0.9 } });
    }
}

impl Outil for BoulesDeNeige {
    fn nom(&self) -> &'static str {
        "❄ Boules de neige"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.ordi, "contre le téléphone");
            if ui.button("Nouvelle partie").clicked() {
                self.score = [0; 2];
                self.boules.clear();
            }
        });
        ui.weak("Glisse le doigt de ton côté pour bouger, touche pour lancer. Premier à 5 !");
        let h = (ui.ctx().screen_rect().height() * 0.72).max(380.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click_and_drag());
        let milieu = zone.center().y;
        let nouveaux = appuis(ui, &mut self.doigts);
        let dt = self.dernier.elapsed().as_secs_f32().min(0.04);
        self.dernier = Instant::now();
        let fini = self.score.iter().any(|s| *s >= 5);
        if !fini {
            for p in self.doigts.values() {
                if zone.contains(*p) {
                    let x = ((p.x - zone.left()) / zone.width()).clamp(0.05, 0.95);
                    let j = if p.y > milieu { 0 } else { 1 };
                    if j == 0 || !self.ordi {
                        self.joueurs[j] = x;
                    }
                }
            }
            for p in nouveaux.into_iter().filter(|p| zone.contains(*p)) {
                let j = if p.y > milieu { 0 } else { 1 };
                if j == 0 || !self.ordi {
                    self.lancer(j);
                }
            }
            if self.ordi {
                // l'ordi esquive les boules et vise le joueur
                let menace = self.boules.iter().filter(|b| b.v < 0.0 && b.pos.y < 0.5).map(|b| b.pos.x).find(|x| (x - self.joueurs[1]).abs() < 0.12);
                let cible = match menace {
                    Some(x) => if x > 0.5 { x - 0.3 } else { x + 0.3 },
                    None => self.joueurs[0],
                };
                self.joueurs[1] += (cible - self.joueurs[1]).clamp(-0.6 * dt, 0.6 * dt);
                if (self.joueurs[1] - self.joueurs[0]).abs() < 0.1 && hasard(100) < 4 {
                    self.lancer(1);
                }
            }
            for b in self.boules.iter_mut() {
                b.pos.y += b.v * dt;
            }
            let mut touches = vec![];
            self.boules.retain(|b| {
                for (j, y) in [(0usize, 0.86f32), (1, 0.14)] {
                    let vers_lui = (j == 0) == (b.v > 0.0);
                    if vers_lui && (b.pos.y - y).abs() < 0.04 && (b.pos.x - self.joueurs[j]).abs() < 0.07 {
                        touches.push(j);
                        return false;
                    }
                }
                (0.0..1.0).contains(&b.pos.y)
            });
            for j in touches {
                self.score[1 - j] += 1;
                self.touche[j] = Some(Instant::now());
                android::vibrer(80);
            }
        }
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 12.0, Color32::from_rgb(200, 225, 245));
        let temps = ui.input(|i| i.time) as f32;
        for f in &self.flocons {
            let y = (f.y + temps * (0.03 + f.x * 0.04)) % 1.0;
            p.circle_filled(zone.min + Vec2::new(f.x * zone.width(), y * zone.height()), 2.0, Color32::WHITE);
        }
        p.line_segment([Pos2::new(zone.left(), milieu), Pos2::new(zone.right(), milieu)], Stroke::new(2.0_f32, Color32::from_rgb(150, 180, 210)));
        for (j, y, c) in [(0usize, 0.86f32, ROSE), (1, 0.14, Color32::from_rgb(70, 120, 220))] {
            let centre = zone.min + Vec2::new(self.joueurs[j] * zone.width(), y * zone.height());
            let rouge = self.touche[j].is_some_and(|t| t.elapsed() < Duration::from_millis(300));
            p.circle_filled(centre, zone.width() * 0.055, if rouge { Color32::WHITE } else { c });
            p.circle_filled(centre + Vec2::new(0.0, if j == 0 { -1.0 } else { 1.0 } * zone.width() * 0.06), zone.width() * 0.03, Color32::from_rgb(255, 220, 190));
            p.text(Pos2::new(zone.left() + 30.0, if j == 0 { milieu + 30.0 } else { milieu - 30.0 }), Align2::CENTER_CENTER, self.score[j].to_string(), FontId::proportional(32.0), c);
        }
        for b in &self.boules {
            p.circle_filled(zone.min + Vec2::new(b.pos.x * zone.width(), b.pos.y * zone.height()), 7.0, Color32::WHITE);
            p.circle_stroke(zone.min + Vec2::new(b.pos.x * zone.width(), b.pos.y * zone.height()), 7.0, Stroke::new(1.0_f32, Color32::from_rgb(150, 170, 200)));
        }
        if fini {
            let t = if self.score[0] >= 5 { "Le joueur du bas gagne ! 🎉" } else if self.ordi { "Le téléphone gagne…" } else { "Le joueur du haut gagne ! 🎉" };
            p.text(zone.center(), Align2::CENTER_CENTER, t, FontId::proportional(26.0), Color32::from_rgb(40, 40, 80));
        } else {
            ui.ctx().request_repaint();
        }
    }
}

// ===================================================================================================
// 26 · Jeu de rythme (4 couloirs, musique générée)
// ===================================================================================================

/// Notes d'un morceau : (temps en secondes, couloir 0..3).
pub fn partition(bpm: f32, duree: f32, graine: u64) -> Vec<(f32, usize)> {
    let mut s = graine | 1;
    let mut r = |n: u64| {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s % n
    };
    let temps = 60.0 / bpm;
    let mut v = vec![];
    let mut t = 2.0;
    let mut dernier = 0;
    while t < duree {
        let mesure = ((t - 2.0) / (temps * 4.0)) as u64;
        // de plus en plus dense
        let densite = (35 + mesure * 4).min(85);
        if r(100) < densite {
            let mut c = r(4) as usize;
            if c == dernier && r(2) == 0 {
                c = (c + 1 + r(3) as usize) % 4;
            }
            v.push((t, c));
            dernier = c;
        }
        t += if mesure > 6 && r(4) == 0 { temps / 2.0 } else { temps };
    }
    v
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct RecordRythme {
    record: u32,
}

pub struct Rythme {
    synthe: Option<Synthe>,
    notes: Vec<(f32, usize, Option<bool>)>, // temps, couloir, réussie ?
    debut: Option<Instant>,
    bpm: f32,
    score: u32,
    combo: u32,
    jugement: (String, Instant),
    battements: u32,
    rec: RecordRythme,
    doigts: HashMap<u64, Pos2>,
    vu: Instant,
}

const NOTES: [f32; 4] = [261.6, 329.6, 392.0, 523.3];
const DUREE: f32 = 60.0;

impl Rythme {
    pub fn new() -> Self {
        Rythme { synthe: None, notes: vec![], debut: None, bpm: 110.0, score: 0, combo: 0, jugement: (String::new(), Instant::now()), battements: 0, rec: charger("rythme"), doigts: HashMap::new(), vu: Instant::now() }
    }

    fn jouer(&mut self, couloir: usize) {
        let Some(d) = self.debut else { return };
        let t = d.elapsed().as_secs_f32();
        if let Some(s) = &self.synthe {
            s.jouer(Son::Note(NOTES[couloir]));
        }
        let cible = self.notes.iter_mut().filter(|n| n.1 == couloir && n.2.is_none()).min_by(|a, b| (a.0 - t).abs().partial_cmp(&(b.0 - t).abs()).unwrap());
        match cible {
            Some(n) if (n.0 - t).abs() < 0.09 => {
                n.2 = Some(true);
                self.combo += 1;
                self.score += 100 + self.combo.min(50) * 4;
                self.jugement = ("Parfait !".into(), Instant::now());
            }
            Some(n) if (n.0 - t).abs() < 0.17 => {
                n.2 = Some(true);
                self.combo += 1;
                self.score += 50 + self.combo.min(50) * 2;
                self.jugement = ("Bien".into(), Instant::now());
            }
            _ => {
                self.combo = 0;
                self.jugement = ("Raté".into(), Instant::now());
            }
        }
    }
}

impl Outil for Rythme {
    fn nom(&self) -> &'static str {
        "🥁 Rythme"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn tic(&mut self, _: &egui::Context) {
        // l'appli ou l'outil est quitté : partie arrêtée et son coupé (sinon il tourne pour rien)
        if self.vu.elapsed() > Duration::from_secs(1) {
            self.debut = None;
            self.synthe = None;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        let Some(debut) = self.debut else {
            if !self.jugement.0.is_empty() {
                ui.label(RichText::new(&self.jugement.0).size(20.0).strong().color(ROSE));
            }
            ui.label(RichText::new("Touche les couloirs quand les notes arrivent sur la ligne").size(18.0).strong());
            ui.horizontal(|ui| {
                ui.label("Tempo");
                for (b, n) in [(90.0, "Tranquille"), (110.0, "Normal"), (135.0, "Rapide"), (160.0, "Expert")] {
                    ui.selectable_value(&mut self.bpm, b, n);
                }
            });
            ui.label(format!("Record : {}", self.rec.record));
            if gros_bouton(ui, "▶ Jouer") {
                self.notes = partition(self.bpm, DUREE, hasard(u64::MAX)).into_iter().map(|(t, c)| (t, c, None)).collect();
                self.synthe.get_or_insert_with(Synthe::demarrer);
                self.debut = Some(Instant::now());
                (self.score, self.combo, self.battements) = (0, 0, 0);
            }
            return;
        };
        let t = debut.elapsed().as_secs_f32();
        // la batterie suit le tempo
        let temps = 60.0 / self.bpm;
        let b = (t / (temps / 2.0)) as u32;
        if b >= self.battements {
            if let Some(s) = &self.synthe {
                s.jouer(if b % 2 == 0 { Son::Caisse } else { Son::Charley });
            }
            self.battements = b + 1;
        }
        // notes ratées
        for n in self.notes.iter_mut().filter(|n| n.2.is_none() && t - n.0 > 0.2) {
            n.2 = Some(false);
            self.combo = 0;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}", self.score)).size(26.0).strong().color(ROSE));
            ui.label(format!("combo ×{}", self.combo));
            ui.label(format!("{:.0} s", (DUREE - t).max(0.0)));
        });
        let h = (ui.ctx().screen_rect().height() * 0.7).max(380.0);
        let (zone, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::click());
        let nouveaux = appuis(ui, &mut self.doigts);
        let l = zone.width() / 4.0;
        for p in nouveaux.into_iter().filter(|p| zone.contains(*p)) {
            self.jouer(((p.x - zone.left()) / l) as usize % 4);
        }
        // clavier : D F J K (pour essayer sur PC)
        let touches = ui.input(|i| [egui::Key::D, egui::Key::F, egui::Key::J, egui::Key::K].map(|k| i.key_pressed(k)));
        for (c, &x) in touches.iter().enumerate() {
            if x {
                self.jouer(c);
            }
        }
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 10.0, Color32::from_rgb(20, 16, 34));
        let ligne = zone.bottom() - 70.0;
        let couleurs = [ROSE, Color32::from_rgb(250, 200, 70), Color32::from_rgb(90, 200, 255), Color32::from_rgb(130, 230, 140)];
        for c in 0..4 {
            let x = zone.left() + c as f32 * l;
            p.line_segment([Pos2::new(x, zone.top()), Pos2::new(x, zone.bottom())], Stroke::new(1.0_f32, Color32::from_gray(50)));
            p.circle_stroke(Pos2::new(x + l / 2.0, ligne), l * 0.32, Stroke::new(3.0_f32, couleurs[c]));
        }
        let vitesse = zone.height() * 0.55; // pixels par seconde
        for (tn, c, ok) in &self.notes {
            if ok.is_some() {
                continue;
            }
            let y = ligne - (tn - t) * vitesse;
            if y < zone.top() - 30.0 || y > zone.bottom() + 30.0 {
                continue;
            }
            p.circle_filled(Pos2::new(zone.left() + *c as f32 * l + l / 2.0, y), l * 0.28, couleurs[*c]);
        }
        if self.jugement.1.elapsed() < Duration::from_millis(400) {
            p.text(Pos2::new(zone.center().x, ligne - 120.0), Align2::CENTER_CENTER, &self.jugement.0, FontId::proportional(30.0), Color32::WHITE);
        }
        if t > DUREE + 1.0 {
            let reussies = self.notes.iter().filter(|n| n.2 == Some(true)).count();
            if self.score > self.rec.record {
                self.rec.record = self.score;
                enregistrer("rythme", &self.rec);
            }
            self.debut = None;
            self.synthe = None;
            self.jugement = (format!("Fini : {} / {} notes · {} points", reussies, self.notes.len(), self.score), Instant::now());
            android::vibrer(200);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classements_et_partitions() {
        let g = fusionner(Value::Null, "Lou", json!({ "sudoku": { "jour": 5, "temps": 300 }, "laby": 4, "reflexes": 240 }));
        let g = fusionner(g, "Max", json!({ "sudoku": { "jour": 5, "temps": 250 }, "laby": 7, "reflexes": 300 }));
        let g = fusionner(g, "Zoé", json!({ "sudoku": { "jour": 4, "temps": 100 }, "laby": 2 }));
        let s = classement(&g, "sudoku", 5);
        assert_eq!(s.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["Max", "Lou"]); // Zoé : autre jour
        assert_eq!(classement(&g, "laby", 5)[0].0, "Max");
        assert_eq!(classement(&g, "reflexes", 5)[0].0, "Lou"); // plus petit = mieux
        let p = partition(120.0, 60.0, 42);
        assert!(p.len() > 40 && p.iter().all(|(t, c)| *t >= 2.0 && *t < 60.0 && *c < 4));
        assert!(p.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(partition(120.0, 60.0, 42), p); // même graine, même morceau
    }
}
