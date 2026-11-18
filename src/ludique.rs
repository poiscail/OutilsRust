//! Petits outils : générateur de mots de passe (131), dés et tirage au sort (132), « à quoi on joue ? » (70),
//! test de réflexes (64), calculateur de sensibilité entre jeux (68).
use crate::outil::{charger, enregistrer, hasard, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------------
// 131 · Générateur de mots de passe
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct OptionsMdp {
    pub longueur: usize,
    pub minuscules: bool,
    pub majuscules: bool,
    pub chiffres: bool,
    pub symboles: bool,
    pub sans_ambigus: bool,
    pub phrase: bool, // phrase de passe (mots)
    pub mots: usize,
}

impl Default for OptionsMdp {
    fn default() -> Self {
        OptionsMdp { longueur: 20, minuscules: true, majuscules: true, chiffres: true, symboles: true, sans_ambigus: true, phrase: false, mots: 5 }
    }
}

const MOTS: &[&str] = &[
    "sakura", "lune", "torii", "neige", "renard", "pagode", "lanterne", "riviere", "montagne", "nuage", "orage", "jardin", "pont", "temple", "etoile",
    "brume", "cerisier", "chat", "dragon", "tigre", "vague", "foret", "sabre", "theiere", "pluie", "matin", "soir", "minuit", "ruelle", "neon", "tokyo",
    "kyoto", "fuji", "bambou", "koi", "carpe", "lotus", "papillon", "luciole", "hiver", "automne", "printemps", "ete", "phare", "ocean", "sable",
    "pierre", "flamme", "braise", "cristal", "plume", "corbeau", "loup", "ours", "cerf", "hibou", "comete", "aurore", "crepuscule", "eclair",
];

pub fn alphabet(o: &OptionsMdp) -> Vec<char> {
    let mut a = String::new();
    if o.minuscules {
        a += "abcdefghijklmnopqrstuvwxyz";
    }
    if o.majuscules {
        a += "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    }
    if o.chiffres {
        a += "0123456789";
    }
    if o.symboles {
        a += "!#$%&*+-=?@_~.,;:";
    }
    let mut v: Vec<char> = a.chars().collect();
    if o.sans_ambigus {
        v.retain(|c| !"lI1O0o".contains(*c));
    }
    v
}

pub fn generer(o: &OptionsMdp) -> String {
    if o.phrase {
        let mut m: Vec<String> = (0..o.mots.max(2)).map(|_| MOTS[hasard(MOTS.len() as u64) as usize].to_string()).collect();
        if o.majuscules {
            let i = hasard(m.len() as u64) as usize;
            m[i] = m[i].to_uppercase();
        }
        let mut s = m.join("-");
        if o.chiffres {
            s += &format!("-{}", hasard(100));
        }
        return s;
    }
    let a = alphabet(o);
    if a.is_empty() {
        return String::new();
    }
    (0..o.longueur.clamp(4, 128)).map(|_| a[hasard(a.len() as u64) as usize]).collect()
}

/// Entropie en bits (estimation) et appréciation.
pub fn solidite(o: &OptionsMdp) -> (f64, &'static str) {
    let bits = if o.phrase {
        o.mots.max(2) as f64 * (MOTS.len() as f64).log2() + if o.chiffres { 6.6 } else { 0.0 }
    } else {
        o.longueur as f64 * (alphabet(o).len().max(1) as f64).log2()
    };
    let note = match bits as u32 {
        0..=39 => "faible",
        40..=59 => "moyen",
        60..=79 => "solide",
        _ => "très solide",
    };
    (bits, note)
}

pub struct MotsDePasse {
    o: OptionsMdp,
    liste: Vec<String>,
}

impl MotsDePasse {
    pub fn new() -> Self {
        let o: OptionsMdp = charger("motsdepasse");
        let liste = (0..8).map(|_| generer(&o)).collect();
        MotsDePasse { o, liste }
    }
}

impl Outil for MotsDePasse {
    fn nom(&self) -> &'static str {
        "🔑 Mots de passe"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur de mots de passe");
        let avant = self.o.clone();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.o.phrase, false, "Caractères");
            ui.selectable_value(&mut self.o.phrase, true, "Phrase de passe (mots)");
        });
        if self.o.phrase {
            ui.add(egui::Slider::new(&mut self.o.mots, 3..=10).text("mots"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.o.majuscules, "un mot en MAJUSCULES");
                ui.checkbox(&mut self.o.chiffres, "un nombre");
            });
        } else {
            ui.add(egui::Slider::new(&mut self.o.longueur, 8..=64).text("caractères"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.o.minuscules, "a-z");
                ui.checkbox(&mut self.o.majuscules, "A-Z");
                ui.checkbox(&mut self.o.chiffres, "0-9");
                ui.checkbox(&mut self.o.symboles, "!#$…");
                ui.checkbox(&mut self.o.sans_ambigus, "sans l/I/1/O/0");
            });
        }
        let (bits, note) = solidite(&self.o);
        let c = match note {
            "faible" => Color32::LIGHT_RED,
            "moyen" => Color32::YELLOW,
            _ => Color32::LIGHT_GREEN,
        };
        ui.colored_label(c, format!("Solidité : {note} ({bits:.0} bits)"));
        if ui.button("🎲 Générer").clicked() || self.o != avant {
            self.liste = (0..8).map(|_| generer(&self.o)).collect();
        }
        ui.separator();
        for m in &self.liste {
            ui.horizontal(|ui| {
                if ui.small_button("📋").on_hover_text("Copier").clicked() {
                    ui.ctx().copy_text(m.clone());
                }
                ui.label(RichText::new(m).monospace().size(16.0));
            });
        }
        if self.o != avant {
            enregistrer("motsdepasse", &self.o);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 132 · Dés et tirage au sort
// ---------------------------------------------------------------------------------------------------

/// Lit « 2d6+3 », « d20 », « 4d8-1 » : (nombre de dés, faces, bonus).
pub fn lire_des(t: &str) -> Option<(u32, u32, i32)> {
    let t = t.trim().to_lowercase().replace(' ', "");
    let (gauche, bonus) = match t.find(['+', '-']) {
        Some(i) => (&t[..i], t[i..].parse::<i32>().ok()?),
        None => (t.as_str(), 0),
    };
    let (n, f) = gauche.split_once('d')?;
    let n = if n.is_empty() { 1 } else { n.parse().ok()? };
    let f = f.parse().ok()?;
    ((1..=100).contains(&n) && (2..=1000).contains(&f)).then_some((n, f, bonus))
}

pub struct Des {
    formule: String,
    resultats: Vec<String>,
    choix: String,
}

impl Des {
    pub fn new() -> Self {
        Des { formule: "2d6".into(), resultats: vec![], choix: "Pizza\nSushi\nBurger".into() }
    }
}

impl Outil for Des {
    fn nom(&self) -> &'static str {
        "🎲 Dés et tirage"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dés et tirage au sort");
        ui.horizontal_wrapped(|ui| {
            for f in ["d4", "d6", "2d6", "d8", "d10", "d12", "d20", "d100"] {
                if ui.button(f).clicked() {
                    self.formule = f.into();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.formule).desired_width(100.0));
            let lance = ui.button("🎲 Lancer").clicked();
            if ui.button("🪙 Pile ou face").clicked() {
                self.resultats.insert(0, if hasard(2) == 0 { "🪙 Pile".into() } else { "🪙 Face".into() });
            }
            if lance {
                match lire_des(&self.formule) {
                    Some((n, f, b)) => {
                        let jets: Vec<u64> = (0..n).map(|_| hasard(f as u64) + 1).collect();
                        let total = jets.iter().sum::<u64>() as i64 + b as i64;
                        self.resultats.insert(0, format!("{} → {total}   ({})", self.formule, jets.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ")));
                    }
                    None => self.resultats.insert(0, "Formule invalide (ex. 3d6+2)".into()),
                }
            }
        });
        if let Some(r) = self.resultats.first() {
            ui.label(RichText::new(r).size(28.0).strong());
        }
        ui.separator();
        ui.strong("Tirer au sort dans une liste (une ligne par choix)");
        ui.add(egui::TextEdit::multiline(&mut self.choix).desired_rows(5).desired_width(300.0));
        ui.horizontal(|ui| {
            let items: Vec<&str> = self.choix.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            if ui.add_enabled(!items.is_empty(), egui::Button::new("🎯 Tirer")).clicked() {
                let x = items[hasard(items.len() as u64) as usize].to_string();
                self.resultats.insert(0, format!("🎯 {x}"));
            }
            if ui.add_enabled(items.len() > 1, egui::Button::new("🔀 Mélanger l'ordre")).clicked() {
                let mut v: Vec<String> = items.iter().map(|s| s.to_string()).collect();
                for i in (1..v.len()).rev() {
                    v.swap(i, hasard(i as u64 + 1) as usize);
                }
                self.resultats.insert(0, format!("🔀 {}", v.join(" › ")));
            }
        });
        self.resultats.truncate(20);
        for r in self.resultats.iter().skip(1) {
            ui.weak(r);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 70 · À quoi on joue ce soir ?
// ---------------------------------------------------------------------------------------------------

pub struct Tirage {
    jeux: Vec<String>,
    exclus: std::collections::BTreeSet<String>,
    roue: Option<(Instant, usize)>, // animation : début, gagnant
}

impl Tirage {
    pub fn new() -> Self {
        Tirage { jeux: vec![], exclus: charger("tirage_exclus"), roue: None }
    }

    fn charger_jeux(&mut self) {
        use crate::lanceur::*;
        let var = |v: &str| std::path::PathBuf::from(std::env::var(v).unwrap_or_default());
        let mut j = scan_steam(steam_racine().as_deref());
        j.extend(scan_epic(&var("ProgramData").join(r"Epic\EpicGamesLauncher\Data\Manifests")));
        let mut noms: Vec<String> = j.into_iter().map(|g| g.nom).filter(|n| !n.contains("Redistributable") && !n.contains("Steamworks")).collect();
        noms.sort_by_key(|n| n.to_lowercase());
        noms.dedup();
        self.jeux = noms;
    }
}

impl Outil for Tirage {
    fn nom(&self) -> &'static str {
        "🎡 À quoi on joue ?"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("À quoi on joue ce soir ?");
        if self.jeux.is_empty() {
            self.charger_jeux();
        }
        let candidats: Vec<String> = self.jeux.iter().filter(|j| !self.exclus.contains(*j)).cloned().collect();
        if ui.add_enabled(!candidats.is_empty() && self.roue.is_none(), egui::Button::new(RichText::new("🎡 Tourner la roue").size(20.0))).clicked() {
            self.roue = Some((Instant::now(), hasard(candidats.len() as u64) as usize));
        }
        if let Some((t, gagnant)) = self.roue {
            let d = t.elapsed().as_secs_f32();
            let duree = 3.0;
            let nom = if d < duree {
                // défile de plus en plus lentement
                let pas = ((d / duree).powf(0.4) * 40.0) as usize;
                candidats.get((gagnant + pas) % candidats.len().max(1)).map(|s| s.as_str()).unwrap_or("")
            } else {
                candidats.get(gagnant).map(|s| s.as_str()).unwrap_or("")
            };
            ui.add_space(10.0);
            ui.label(RichText::new(nom).size(34.0).strong().color(if d < duree { Color32::GRAY } else { Color32::from_rgb(255, 120, 180) }));
            if d < duree {
                ui.ctx().request_repaint();
            } else if ui.button("Encore !").clicked() {
                self.roue = None;
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("{} jeu(x) dans la roue", candidats.len()));
            if ui.small_button("⟳ Relire les jeux").clicked() {
                self.charger_jeux();
            }
        });
        let avant = self.exclus.clone();
        egui::ScrollArea::vertical().id_salt("roue").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for j in &self.jeux {
                    let mut dedans = !self.exclus.contains(j);
                    if ui.checkbox(&mut dedans, j).changed() {
                        if dedans {
                            self.exclus.remove(j);
                        } else {
                            self.exclus.insert(j.clone());
                        }
                    }
                }
            });
        });
        if self.exclus != avant {
            enregistrer("tirage_exclus", &self.exclus);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 64 · Test de réflexes
// ---------------------------------------------------------------------------------------------------

enum EtatReflexe {
    Attente,
    Pret(Instant), // vert à cet instant
    Patience(Instant),
    TropTot,
    Resultat(u128),
}

pub struct Reflexes {
    etat: EtatReflexe,
    scores: Vec<u128>,
    meilleur: u128,
}

impl Reflexes {
    pub fn new() -> Self {
        let meilleur: u128 = charger::<Option<u128>>("reflexes").unwrap_or(0);
        Reflexes { etat: EtatReflexe::Attente, scores: vec![], meilleur }
    }
}

impl Outil for Reflexes {
    fn nom(&self) -> &'static str {
        "⚡ Test de réflexes"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test de réflexes");
        ui.weak("Clique (ou appuie sur Espace) dès que la zone devient verte.");
        let (zone, rep) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 260.0), egui::Sense::click());
        let espace = ui.input(|i| i.key_pressed(egui::Key::Space));
        let clic = rep.clicked() || espace;
        let (fond, texte) = match self.etat {
            EtatReflexe::Attente => (Color32::from_rgb(60, 70, 110), "Clique pour commencer".to_string()),
            EtatReflexe::Patience(_) => (Color32::from_rgb(170, 40, 60), "Attends le vert…".to_string()),
            EtatReflexe::Pret(_) => (Color32::from_rgb(40, 170, 90), "CLIQUE !".to_string()),
            EtatReflexe::TropTot => (Color32::from_rgb(120, 60, 60), "Trop tôt ! Clique pour réessayer".to_string()),
            EtatReflexe::Resultat(ms) => (Color32::from_rgb(60, 70, 110), format!("{ms} ms · clique pour recommencer")),
        };
        ui.painter().rect_filled(zone, 12.0, fond);
        ui.painter().text(zone.center(), egui::Align2::CENTER_CENTER, texte, egui::FontId::proportional(30.0), Color32::WHITE);
        match self.etat {
            EtatReflexe::Patience(t) => {
                if clic {
                    self.etat = EtatReflexe::TropTot;
                } else if Instant::now() >= t {
                    self.etat = EtatReflexe::Pret(Instant::now());
                }
                ui.ctx().request_repaint();
            }
            EtatReflexe::Pret(t) => {
                if clic {
                    let ms = t.elapsed().as_millis();
                    self.scores.push(ms);
                    if self.meilleur == 0 || ms < self.meilleur {
                        self.meilleur = ms;
                        enregistrer("reflexes", &Some(ms));
                    }
                    self.etat = EtatReflexe::Resultat(ms);
                }
                ui.ctx().request_repaint();
            }
            _ => {
                if clic {
                    self.etat = EtatReflexe::Patience(Instant::now() + Duration::from_millis(1500 + hasard(3000)));
                }
            }
        }
        if !self.scores.is_empty() {
            let moy = self.scores.iter().sum::<u128>() / self.scores.len() as u128;
            ui.label(format!("Essais : {} · moyenne {moy} ms · record {} ms", self.scores.len(), self.meilleur));
            ui.weak("Repère : 200-250 ms est la moyenne ; sous 180 ms, c'est très rapide.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 68 · Calculateur de sensibilité
// ---------------------------------------------------------------------------------------------------

/// (jeu, degrés tournés par « count » de souris à sensibilité 1)
pub const JEUX_SENS: &[(&str, f64)] = &[
    ("Counter-Strike 2 / CS:GO", 0.022),
    ("Apex Legends", 0.022),
    ("Team Fortress 2", 0.022),
    ("Valorant", 0.07),
    ("Overwatch 2", 0.0066),
    ("Fortnite (sens. 1 = 100 %)", 0.005555),
    ("Rainbow Six Siege", 0.00572958),
    ("Call of Duty (MW / Warzone)", 0.0066),
    ("Minecraft (sens. 100 % ≈ 1)", 0.15),
    ("Rust", 0.1125),
];

/// Sensibilité équivalente : même distance de souris pour un tour complet.
pub fn convertir(sens: f64, de: f64, vers: f64) -> f64 {
    sens * de / vers
}

/// Centimètres de tapis pour 360°.
pub fn cm_360(sens: f64, yaw: f64, dpi: f64) -> f64 {
    360.0 / (sens * yaw * dpi) * 2.54
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default)]
struct EtatSens {
    de: usize,
    vers: usize,
    sens: f64,
    dpi: f64,
    dpi2: f64,
}

pub struct Sensibilite {
    e: EtatSens,
}

impl Sensibilite {
    pub fn new() -> Self {
        let mut e: EtatSens = charger("sensibilite");
        if e.sens == 0.0 {
            e = EtatSens { de: 0, vers: 1, sens: 2.0, dpi: 800.0, dpi2: 800.0 };
        }
        Sensibilite { e }
    }
}

impl Outil for Sensibilite {
    fn nom(&self) -> &'static str {
        "🖱 Sensibilité entre jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Calculateur de sensibilité");
        let avant = self.e.clone();
        let choix = |ui: &mut egui::Ui, id: &str, v: &mut usize| {
            egui::ComboBox::from_id_salt(id).selected_text(JEUX_SENS[*v].0).width(260.0).show_ui(ui, |ui| {
                for (i, (n, _)) in JEUX_SENS.iter().enumerate() {
                    ui.selectable_value(v, i, *n);
                }
            });
        };
        egui::Grid::new("sens").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Jeu d'origine");
            choix(ui, "de", &mut self.e.de);
            ui.end_row();
            ui.label("Sensibilité");
            ui.add(egui::DragValue::new(&mut self.e.sens).speed(0.01).range(0.001..=1000.0));
            ui.end_row();
            ui.label("DPI de la souris");
            ui.add(egui::DragValue::new(&mut self.e.dpi).speed(10.0).range(100.0..=32000.0));
            ui.end_row();
            ui.label("Jeu cible");
            choix(ui, "vers", &mut self.e.vers);
            ui.end_row();
            ui.label("DPI dans le jeu cible");
            ui.add(egui::DragValue::new(&mut self.e.dpi2).speed(10.0).range(100.0..=32000.0));
            ui.end_row();
        });
        let (de, vers) = (JEUX_SENS[self.e.de].1, JEUX_SENS[self.e.vers].1);
        let s = convertir(self.e.sens, de, vers) * self.e.dpi / self.e.dpi2;
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Sensibilité à mettre :");
            ui.label(RichText::new(format!("{s:.4}")).size(26.0).strong().color(Color32::from_rgb(255, 120, 180)));
            if ui.small_button("📋").clicked() {
                ui.ctx().copy_text(format!("{s:.4}"));
            }
        });
        ui.label(format!("Distance pour un tour complet : {:.1} cm", cm_360(self.e.sens, de, self.e.dpi)));
        ui.weak("Formules de rotation (« yaw ») publiques de chaque jeu ; la valeur peut dépendre du champ de vision pour certains.");
        if self.e != avant {
            enregistrer("sensibilite", &self.e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mots_de_passe() {
        let o = OptionsMdp { longueur: 30, symboles: false, ..Default::default() };
        let m = generer(&o);
        assert_eq!(m.chars().count(), 30);
        assert!(m.chars().all(|c| c.is_ascii_alphanumeric() && !"lI1O0o".contains(c)));
        let p = generer(&OptionsMdp { phrase: true, mots: 4, chiffres: false, majuscules: false, ..Default::default() });
        assert_eq!(p.split('-').count(), 4);
        assert!(solidite(&OptionsMdp::default()).0 > 100.0);
    }

    #[test]
    fn des() {
        assert_eq!(lire_des("2d6+3"), Some((2, 6, 3)));
        assert_eq!(lire_des("d20"), Some((1, 20, 0)));
        assert_eq!(lire_des("4d8-1"), Some((4, 8, -1)));
        assert_eq!(lire_des("abc"), None);
        assert_eq!(lire_des("0d6"), None);
    }

    #[test]
    fn sensibilite() {
        // CS 2 → Valorant : diviser par 3,18
        assert!((convertir(2.0, 0.022, 0.07) - 0.628571).abs() < 1e-4);
        assert!((cm_360(2.0, 0.022, 800.0) - 25.98).abs() < 0.1);
    }
}
