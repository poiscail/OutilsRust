//! Fun : roue de la fortune personnalisable (38) et escape game (37).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::time::Instant;

// ---------------------------------------------------------------------------------------------------
// 38 · Roue de la fortune
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Roues {
    pub actuelle: String,
    pub listes: Vec<(String, Vec<String>)>,
    pub retirer_gagnant: bool,
}

impl Default for Roues {
    fn default() -> Self {
        let l = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        Roues {
            actuelle: "Qui fait la vaisselle ?".into(),
            listes: vec![
                ("Qui fait la vaisselle ?".into(), l(&["Papa", "Maman", "Moi", "Ma sœur", "Mon frère"])),
                ("À quoi on joue ?".into(), l(&["Minecraft", "Fortnite", "Rocket League", "Mario Kart", "Among Us", "Jeu de société"])),
                ("Gages".into(), l(&["Chanter une chanson", "10 pompes", "Imiter un animal", "Parler en rimes 2 min", "Danser 30 s", "Raconter une blague"])),
                ("Film du soir".into(), l(&["Action", "Comédie", "Animation", "Horreur", "Science-fiction", "Au hasard sur Netflix"])),
            ],
            retirer_gagnant: false,
        }
    }
}

/// Indice de la case sous le pointeur (en haut) pour un angle de roue donné.
pub fn case_gagnante(angle: f32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    // la case k occupe [k·s, (k+1)·s) à partir du haut, dans le sens des aiguilles ; la roue a tourné de `angle`
    let s = TAU / n as f32;
    let a = (-angle).rem_euclid(TAU);
    ((a / s) as usize).min(n - 1)
}

pub struct Roue {
    r: Roues,
    angle: f32,
    vitesse: f32,
    dernier: Instant,
    gagnant: Option<String>,
    historique: Vec<String>,
    nouvelle: String,
    nom_liste: String,
}

impl Roue {
    pub fn new() -> Self {
        Roue { r: charger("roue"), angle: 0.0, vitesse: 0.0, dernier: Instant::now(), gagnant: None, historique: Vec::new(), nouvelle: String::new(), nom_liste: String::new() }
    }

    fn cases(&mut self) -> &mut Vec<String> {
        let nom = self.r.actuelle.clone();
        if !self.r.listes.iter().any(|l| l.0 == nom) {
            self.r.listes.push((nom.clone(), Vec::new()));
        }
        &mut self.r.listes.iter_mut().find(|l| l.0 == nom).unwrap().1
    }
}

fn couleur_case(k: usize, n: usize) -> Color32 {
    let h = k as f32 / n.max(1) as f32;
    Color32::from(egui::ecolor::Hsva::new(h, 0.65, 0.9, 1.0))
}

impl Outil for Roue {
    fn nom(&self) -> &'static str {
        "🎡 Roue de la fortune"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.r.clone();
        ui.heading("Roue de la fortune");
        ui.horizontal_wrapped(|ui| {
            for (n, _) in self.r.listes.clone() {
                ui.selectable_value(&mut self.r.actuelle, n.clone(), n);
            }
            ui.add(egui::TextEdit::singleline(&mut self.nom_liste).hint_text("nouvelle roue").desired_width(130.0));
            if ui.small_button("➕").clicked() && !self.nom_liste.trim().is_empty() {
                self.r.actuelle = std::mem::take(&mut self.nom_liste).trim().to_string();
                self.cases();
            }
        });
        // animation
        let dt = self.dernier.elapsed().as_secs_f32().min(0.05);
        self.dernier = Instant::now();
        if self.vitesse > 0.0 {
            self.angle += self.vitesse * dt;
            self.vitesse = (self.vitesse - (1.2 + self.vitesse * 0.6) * dt).max(0.0);
            if self.vitesse == 0.0 {
                let n = self.cases().len();
                let k = case_gagnante(self.angle, n);
                let g = self.cases().get(k).cloned();
                if let Some(g) = g {
                    self.historique.insert(0, g.clone());
                    self.historique.truncate(10);
                    if self.r.retirer_gagnant && n > 1 {
                        self.cases().remove(k);
                    }
                    self.gagnant = Some(g);
                }
            }
            ui.ctx().request_repaint();
        }
        let cases = self.cases().clone();
        ui.horizontal_top(|ui| {
            let cote = ui.available_width().min(420.0).max(240.0);
            let (r, rep) = ui.allocate_exact_size(egui::vec2(cote, cote), Sense::click());
            let p = ui.painter_at(r);
            let c = r.center();
            let rayon = cote / 2.0 - 12.0;
            let n = cases.len().max(1);
            let s = TAU / n as f32;
            if cases.is_empty() {
                p.circle_filled(c, rayon, Color32::from_gray(60));
            }
            for (k, t) in cases.iter().enumerate() {
                // angles mesurés depuis le haut, sens des aiguilles
                let a0 = self.angle + k as f32 * s - TAU / 4.0;
                let pas = ((s / 0.05) as usize).max(2);
                let mut pts = vec![c];
                for i in 0..=pas {
                    let a = a0 + s * i as f32 / pas as f32;
                    pts.push(c + egui::vec2(a.cos(), a.sin()) * rayon);
                }
                p.add(egui::Shape::convex_polygon(pts, couleur_case(k, n), Stroke::new(1.0_f32, Color32::from_black_alpha(80))));
                let am = a0 + s / 2.0;
                let pos = c + egui::vec2(am.cos(), am.sin()) * rayon * 0.62;
                let court: String = t.chars().take(16).collect();
                p.text(pos, egui::Align2::CENTER_CENTER, court, FontId::proportional((rayon / 11.0).clamp(10.0, 18.0)), Color32::BLACK);
            }
            p.circle_filled(c, rayon * 0.1, Color32::WHITE);
            // pointeur
            let haut = Pos2::new(c.x, c.y - rayon - 2.0);
            p.add(egui::Shape::convex_polygon(vec![haut + egui::vec2(-12.0, -12.0), haut + egui::vec2(12.0, -12.0), haut + egui::vec2(0.0, 14.0)], Color32::WHITE, Stroke::new(2.0_f32, Color32::BLACK)));
            if rep.clicked() && self.vitesse == 0.0 && !cases.is_empty() {
                self.vitesse = 9.0 + crate::outil::hasard(1000) as f32 / 1000.0 * 8.0;
                self.gagnant = None;
            }
            ui.vertical(|ui| {
                if ui.add_enabled(self.vitesse == 0.0 && !cases.is_empty(), egui::Button::new(RichText::new("🎡 Lancer !").size(20.0))).clicked() {
                    self.vitesse = 9.0 + crate::outil::hasard(1000) as f32 / 1000.0 * 8.0;
                    self.gagnant = None;
                }
                if let Some(g) = &self.gagnant {
                    ui.label(RichText::new(format!("🎉 {g}")).size(26.0).strong().color(Color32::GOLD));
                }
                ui.checkbox(&mut self.r.retirer_gagnant, "Retirer la case gagnante");
                ui.separator();
                let mut retirer = None;
                for (k, t) in cases.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.colored_label(couleur_case(k, n), "⬤");
                        ui.label(t);
                        if ui.small_button("✖").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
                if let Some(k) = retirer {
                    self.cases().remove(k);
                }
                ui.horizontal(|ui| {
                    let r = ui.add(egui::TextEdit::singleline(&mut self.nouvelle).hint_text("ajouter une case").desired_width(150.0));
                    if (ui.small_button("➕").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.nouvelle.trim().is_empty() {
                        let t = std::mem::take(&mut self.nouvelle).trim().to_string();
                        self.cases().push(t);
                    }
                });
                if !self.historique.is_empty() {
                    ui.weak(format!("Derniers : {}", self.historique.join(", ")));
                }
            });
        });
        if self.r != avant {
            enregistrer("roue", &self.r);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 37 · Escape game
// ---------------------------------------------------------------------------------------------------

pub struct Salle {
    pub nom: &'static str,
    pub objets: &'static [(&'static str, &'static str, &'static str)], // émoji, nom, ce qu'on découvre
    pub question: &'static str,
    pub reponses: &'static [&'static str],
    pub indices: &'static [&'static str],
}

pub struct Aventure {
    pub nom: &'static str,
    pub intro: &'static str,
    pub salles: &'static [Salle],
}

pub const AVENTURES: &[Aventure] = &[
    Aventure {
        nom: "🧪 Le laboratoire abandonné",
        intro: "Tu te réveilles enfermé dans un laboratoire. Les savants sont partis en vitesse… Trouve la sortie !",
        salles: &[
            Salle {
                nom: "Le hall",
                objets: &[("📓", "Carnet", "« Le code de la porte : les pattes d'une araignée, puis d'un insecte, puis d'un chien, puis d'un oiseau. »"), ("🪴", "Plante", "Rien… juste de la poussière."), ("🖼", "Affiche", "Une affiche sur les petites bêtes : « L'araignée n'est PAS un insecte ! »")],
                question: "Code de la porte (4 chiffres)",
                reponses: &["8642"],
                indices: &["Une araignée a 8 pattes.", "Un insecte en a 6, un chien 4, un oiseau 2."],
            },
            Salle {
                nom: "Le bureau",
                objets: &[("💻", "Ordinateur", "Un message à l'écran : « FKLPLH »"), ("📜", "Post-it", "« César avance chaque lettre de 3 dans l'alphabet. Pour lire, recule de 3. »"), ("☕", "Tasse", "Froide. Quelqu'un est parti vite.")],
                question: "Mot de passe de l'ordinateur",
                reponses: &["chimie"],
                indices: &["F recule de 3 : F → E → D → C.", "Le mot commence par CHI…"],
            },
            Salle {
                nom: "La salle des fioles",
                objets: &[("🔴", "Fiole rouge", "Étiquette : « 2 × la fiole bleue »"), ("🔵", "Fiole bleue", "Étiquette : « 7 »"), ("🟢", "Fiole verte", "Étiquette : « rouge + bleue »"), ("📋", "Consigne", "« Le coffre s'ouvre avec la valeur de la fiole verte. »")],
                question: "Code du coffre",
                reponses: &["21"],
                indices: &["La fiole rouge vaut 2 × 7.", "Verte = 14 + 7."],
            },
            Salle {
                nom: "La sortie",
                objets: &[("📻", "Radio", "Des bips : ·−··  ··  −···  ·−·  ·"), ("📖", "Manuel", "Code morse : L = ·−··   I = ··   B = −···   R = ·−·   E = ·   S = ···   O = −−−")],
                question: "Mot qui ouvre la sortie",
                reponses: &["libre"],
                indices: &["Chaque groupe de signes est une lettre.", "La première lettre est L."],
            },
        ],
    },
    Aventure {
        nom: "🏴‍☠️ L'île au trésor",
        intro: "Ton bateau a fait naufrage sur une île. Un vieux pirate y aurait caché son trésor…",
        salles: &[
            Salle {
                nom: "La plage",
                objets: &[("🍾", "Bouteille", "Un message : « Je suis le seul mot qui s'écrit toujours mal dans le dictionnaire. »"), ("🦀", "Crabe", "Il te pince ! Aïe."), ("🌴", "Palmier", "Une inscription : « La réponse tient en trois lettres. »")],
                question: "Réponse à la devinette",
                reponses: &["mal"],
                indices: &["Relis bien la phrase : « toujours ___ ».", "C'est le mot « mal » lui-même !"],
            },
            Salle {
                nom: "La grotte",
                objets: &[("🔦", "Lampe", "Sur le mur : « 2, 4, 8, 16, ? »"), ("🦇", "Chauves-souris", "Elles s'envolent en criant."), ("🪨", "Rocher", "Gravé : « Le cadenas veut le nombre suivant. »")],
                question: "Code du cadenas",
                reponses: &["32"],
                indices: &["Chaque nombre est le double du précédent.", "16 × 2 = ?"],
            },
            Salle {
                nom: "Le vieux navire",
                objets: &[("🗺", "Carte", "« Le mot de passe se cache dans les premières lettres : Tous Récupèrent En Silence Or Rare. »"), ("🧭", "Boussole", "Elle pointe vers la cale."), ("🛢", "Tonneau", "Du vieux rhum… beurk.")],
                question: "Mot de passe de la cale",
                reponses: &["tresor"],
                indices: &["Prends la première lettre de chaque mot.", "T, R, E, S…"],
            },
            Salle {
                nom: "Le coffre",
                objets: &[("⚖", "Balance", "« 3 pièces d'or pèsent autant que 6 pièces d'argent. »"), ("📜", "Parchemin", "« Combien de pièces d'argent pèsent autant que 5 pièces d'or ? Ce nombre ouvre le coffre. »")],
                question: "Code du coffre",
                reponses: &["10"],
                indices: &["1 pièce d'or = 2 pièces d'argent.", "5 × 2 = ?"],
            },
        ],
    },
    Aventure {
        nom: "🚀 La station spatiale",
        intro: "Alerte ! La station est en panne et tu es seul à bord. Rejoins le module de retour vers la Terre.",
        salles: &[
            Salle {
                nom: "Le sas",
                objets: &[("🕐", "Horloge", "Il est 14 h 45. Une note : « Le code est l'heure qu'il sera dans 15 minutes, sans les deux points. »"), ("🧑‍🚀", "Combinaison", "Vide… et un peu froissée.")],
                question: "Code du sas (4 chiffres)",
                reponses: &["1500"],
                indices: &["14 h 45 + 15 minutes = ?", "15 h 00 s'écrit 1500."],
            },
            Salle {
                nom: "Le poste de pilotage",
                objets: &[("🖥", "Écran", "« Mot secret mélangé : NUTRASE »"), ("🪐", "Maquette", "Une planète avec de beaux anneaux."), ("🎧", "Casque", "Grésillements…")],
                question: "Mot secret",
                reponses: &["saturne"],
                indices: &["C'est une planète.", "La maquette t'aide : elle a des anneaux."],
            },
            Salle {
                nom: "Le labo",
                objets: &[("🔋", "Batteries", "« A : 30 %. B : le double de A. C : la moitié de B, plus 10. »"), ("📋", "Consigne", "« Entre la charge de la batterie C pour rallumer les moteurs. »")],
                question: "Charge de la batterie C (en %)",
                reponses: &["40"],
                indices: &["B = 60 %.", "La moitié de 60, plus 10."],
            },
            Salle {
                nom: "Le module de retour",
                objets: &[("📡", "Radio", "« 01001111 01001011 »"), ("📖", "Aide-mémoire", "« Chaque groupe de 8 chiffres est une lettre : 01000001 = A, 01001011 = K, 01001111 = O. »")],
                question: "Mot à envoyer à la Terre",
                reponses: &["ok"],
                indices: &["Compare chaque groupe avec l'aide-mémoire.", "Deux lettres seulement."],
            },
        ],
    },
];

/// Réponse simplifiée : minuscules, sans accents, espaces ni ponctuation.
pub fn normaliser(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .filter(|c| c.is_alphanumeric())
        .collect()
}

pub struct EscapeGame {
    aventure: Option<usize>,
    salle: usize,
    vus: Vec<usize>,
    saisie: String,
    indices: usize,
    total_indices: usize,
    debut: Instant,
    erreur: bool,
    fini: Option<u64>,
    records: Vec<(String, u64)>,
}

impl EscapeGame {
    pub fn new() -> Self {
        EscapeGame { aventure: None, salle: 0, vus: Vec::new(), saisie: String::new(), indices: 0, total_indices: 0, debut: Instant::now(), erreur: false, fini: None, records: charger("escape_records") }
    }
}

impl Outil for EscapeGame {
    fn nom(&self) -> &'static str {
        "🔐 Escape game"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Escape game");
        let Some(a) = self.aventure else {
            ui.weak("Fouille chaque salle (clique sur les objets), résous l'énigme et sors avant tout le monde !");
            for (k, av) in AVENTURES.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new(av.nom).size(18.0)).min_size(egui::vec2(260.0, 40.0))).clicked() {
                        *self = EscapeGame { aventure: Some(k), debut: Instant::now(), records: std::mem::take(&mut self.records), ..EscapeGame::new() };
                    }
                    if let Some((_, s)) = self.records.iter().find(|r| r.0 == av.nom) {
                        ui.weak(format!("record : {} min {:02} s", s / 60, s % 60));
                    }
                });
            }
            return;
        };
        let av = &AVENTURES[a];
        if let Some(s) = self.fini {
            ui.label(RichText::new("🎉 Tu t'es échappé !").size(28.0).color(Color32::GOLD));
            ui.label(format!("{} — en {} min {:02} s, avec {} indice(s).", av.nom, s / 60, s % 60, self.total_indices));
            if ui.button("↩ Choisir une autre aventure").clicked() {
                self.aventure = None;
            }
            return;
        }
        let salle = &av.salles[self.salle];
        let t = self.debut.elapsed().as_secs();
        ui.horizontal(|ui| {
            ui.strong(av.nom);
            ui.label(format!("· Salle {}/{} : {}", self.salle + 1, av.salles.len(), salle.nom));
            ui.label(format!("· ⏱ {} min {:02} s", t / 60, t % 60));
            if ui.small_button("Abandonner").clicked() {
                self.aventure = None;
            }
        });
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        if self.salle == 0 && self.vus.is_empty() {
            ui.weak(av.intro);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (k, (e, n, _)) in salle.objets.iter().enumerate() {
                let vu = self.vus.contains(&k);
                let b = egui::Button::new(RichText::new(format!("{e}\n{n}")).size(20.0)).min_size(egui::vec2(110.0, 90.0)).selected(vu);
                if ui.add(b).clicked() && !vu {
                    self.vus.push(k);
                }
            }
        });
        for &k in &self.vus {
            let (e, n, d) = salle.objets[k];
            ui.label(format!("{e} {n} : {d}"));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(salle.question);
            let r = ui.add(egui::TextEdit::singleline(&mut self.saisie).desired_width(140.0));
            if ui.button("🔓 Essayer").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                if salle.reponses.iter().any(|x| normaliser(x) == normaliser(&self.saisie)) {
                    self.salle += 1;
                    self.vus.clear();
                    self.saisie.clear();
                    self.indices = 0;
                    self.erreur = false;
                    if self.salle >= av.salles.len() {
                        let s = self.debut.elapsed().as_secs() + 30 * self.total_indices as u64;
                        self.fini = Some(s);
                        match self.records.iter_mut().find(|r| r.0 == av.nom) {
                            Some(r) if r.1 <= s => {}
                            Some(r) => r.1 = s,
                            None => self.records.push((av.nom.to_string(), s)),
                        }
                        enregistrer("escape_records", &self.records);
                    }
                } else {
                    self.erreur = true;
                }
            }
        });
        if self.erreur {
            ui.colored_label(Color32::LIGHT_RED, "🔒 Ça ne s'ouvre pas…");
        }
        if self.salle < av.salles.len() {
            for i in &salle.indices[..self.indices.min(salle.indices.len())] {
                ui.label(format!("💡 {i}"));
            }
            if self.indices < salle.indices.len() && ui.small_button("💡 Un indice (+30 s)").clicked() {
                self.indices += 1;
                self.total_indices += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roue_et_escape() {
        assert_eq!(case_gagnante(0.0, 4), 0);
        // la roue a tourné d'un peu plus d'un quart de tour dans le sens des aiguilles : la case 2 arrive en haut
        assert_eq!(case_gagnante(TAU / 4.0 + 0.01, 4), 2);
        assert_eq!(case_gagnante(-TAU / 4.0 - 0.01, 4), 1);
        assert_eq!(normaliser("Trésor !"), "tresor");
        assert_eq!(normaliser(" 40 % "), "40");
        for a in AVENTURES {
            assert!(!a.salles.is_empty());
            for s in a.salles {
                assert!(!s.reponses.is_empty() && !s.indices.is_empty() && !s.objets.is_empty());
            }
        }
        // vérifie les énigmes calculables
        let cesar: String = "FKLPLH".chars().map(|c| (((c as u8 - b'A' + 23) % 26) + b'a') as char).collect();
        assert_eq!(cesar, AVENTURES[0].salles[1].reponses[0]);
        let ascii: String = ["01001111", "01001011"].iter().map(|b| u8::from_str_radix(b, 2).unwrap() as char).collect::<String>().to_lowercase();
        assert_eq!(ascii, AVENTURES[2].salles[3].reponses[0]);
        let mut a: Vec<char> = "nutrase".chars().collect();
        let mut b: Vec<char> = "saturne".chars().collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
}
