//! Listes du quotidien sur un même modèle : tâches (124), liens (126), films et séries (127), lecture (128),
//! et un tableau Kanban (125). Chaque liste dans %APPDATA%\Outils\<nom>.json.
use crate::notes::{format_local, lire_local};
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::pressepapiers::{decalage, maintenant_utc};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Element {
    pub id: u64,
    pub titre: String,
    pub statut: usize,
    pub note: u8,          // 0..=5 étoiles
    pub champ: String,     // URL, épisode, chapitre… selon la liste
    pub categorie: String, // dossier / genre / priorité
    pub commentaire: String,
    pub echeance: Option<i64>, // UTC, tâches seulement
    pub rappele: bool,
}

pub struct Modele {
    pub fichier: &'static str,
    pub nom: &'static str,
    pub titre: &'static str,
    pub statuts: &'static [&'static str],
    pub champ: Option<&'static str>,
    pub categorie: &'static str,
    pub etoiles: bool,
    pub echeance: bool,
    pub lien: bool,
}

pub const TACHES: Modele = Modele {
    fichier: "taches", nom: "✅ Tâches", titre: "Tâches", statuts: &["À faire", "Fait"], champ: None, categorie: "Priorité / projet", etoiles: false, echeance: true, lien: false,
};
pub const LIENS: Modele = Modele {
    fichier: "liens", nom: "🔖 Liens", titre: "Gestionnaire de liens", statuts: &["Lien"], champ: Some("Adresse"), categorie: "Dossier", etoiles: false, echeance: false, lien: true,
};
pub const FILMS: Modele = Modele {
    fichier: "films", nom: "🎬 Films et séries", titre: "Films et séries", statuts: &["À voir", "En cours", "Vu", "Abandonné"], champ: Some("Saison / épisode"),
    categorie: "Genre / plateforme", etoiles: true, echeance: false, lien: false,
};
pub const LECTURE: Modele = Modele {
    fichier: "lecture", nom: "📚 Livres et mangas", titre: "Suivi de lecture", statuts: &["À lire", "En cours", "Lu", "Abandonné"], champ: Some("Tome / chapitre"),
    categorie: "Auteur / type", etoiles: true, echeance: false, lien: false,
};

/// Éléments visibles : filtre texte + statut, tri (échéance, puis titre).
pub fn vue<'a>(v: &'a [Element], q: &str, statut: Option<usize>) -> Vec<&'a Element> {
    let q = q.to_lowercase();
    let mut r: Vec<&Element> = v.iter().filter(|e| statut.is_none_or(|s| e.statut == s))
        .filter(|e| q.is_empty() || [&e.titre, &e.categorie, &e.champ, &e.commentaire].iter().any(|t| t.to_lowercase().contains(&q))).collect();
    r.sort_by(|a, b| a.statut.cmp(&b.statut).then(a.echeance.unwrap_or(i64::MAX).cmp(&b.echeance.unwrap_or(i64::MAX))).then(a.titre.to_lowercase().cmp(&b.titre.to_lowercase())));
    r
}

pub struct Liste {
    m: &'static Modele,
    elements: Vec<Element>,
    recherche: String,
    filtre: Option<usize>,
    choisi: Option<u64>,
    saisie_date: String,
    nouveau: String,
}

impl Liste {
    pub fn new(m: &'static Modele) -> Self {
        Liste { m, elements: charger(m.fichier), recherche: String::new(), filtre: None, choisi: None, saisie_date: String::new(), nouveau: String::new() }
    }
}

impl Outil for Liste {
    fn nom(&self) -> &'static str {
        self.m.nom
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }

    /// Tâches : notification à l'échéance.
    fn tic(&mut self, _: &egui::Context) {
        if !self.m.echeance {
            return;
        }
        let m = maintenant_utc();
        let mut change = false;
        for e in self.elements.iter_mut().filter(|e| e.statut == 0 && !e.rappele && e.echeance.is_some_and(|d| d <= m)) {
            crate::systeme::notifier_toujours("Tâche à faire", &e.titre);
            e.rappele = true;
            change = true;
        }
        if change {
            enregistrer(self.m.fichier, &self.elements);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let m = self.m;
        ui.heading(m.titre);
        let avant = self.elements.clone();
        let off = decalage();
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.nouveau).hint_text("Ajouter…").desired_width(260.0));
            if (ui.button("➕").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.nouveau.trim().is_empty() {
                let id = self.elements.iter().map(|e| e.id).max().unwrap_or(0) + 1;
                let (titre, champ) = if m.lien && self.nouveau.contains("://") { (self.nouveau.clone(), self.nouveau.clone()) } else { (self.nouveau.clone(), String::new()) };
                self.elements.push(Element { id, titre: titre.trim().into(), champ, ..Default::default() });
                self.choisi = Some(id);
                self.nouveau.clear();
            }
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍").desired_width(150.0));
            if m.statuts.len() > 1 {
                ui.selectable_value(&mut self.filtre, None, "Tout");
                for (i, s) in m.statuts.iter().enumerate() {
                    let n = self.elements.iter().filter(|e| e.statut == i).count();
                    ui.selectable_value(&mut self.filtre, Some(i), format!("{s} ({n})"));
                }
            }
        });
        ui.separator();
        let mut supprimer = None;
        ui.columns(2, |c| {
            egui::ScrollArea::vertical().id_salt(("liste", m.fichier)).show(&mut c[0], |ui| {
                let v: Vec<Element> = vue(&self.elements, &self.recherche, self.filtre).into_iter().cloned().collect();
                let mut categorie_prec: Option<String> = None;
                for e in &v {
                    if m.lien && categorie_prec.as_deref() != Some(&e.categorie) {
                        ui.label(RichText::new(if e.categorie.is_empty() { "Sans dossier" } else { &e.categorie }).strong());
                        categorie_prec = Some(e.categorie.clone());
                    }
                    ui.horizontal(|ui| {
                        if m.statuts.len() == 2 {
                            let mut fait = e.statut == 1;
                            if ui.checkbox(&mut fait, "").changed() {
                                if let Some(x) = self.elements.iter_mut().find(|x| x.id == e.id) {
                                    x.statut = fait as usize;
                                }
                            }
                        }
                        let mut t = RichText::new(&e.titre);
                        if m.statuts.len() == 2 && e.statut == 1 {
                            t = t.strikethrough().weak();
                        }
                        if ui.selectable_label(self.choisi == Some(e.id), t).clicked() {
                            self.choisi = Some(e.id);
                            self.saisie_date = e.echeance.map(|d| format_local(d, off)).unwrap_or_default();
                        }
                        if m.lien && ui.small_button("↗").on_hover_text(&e.champ).clicked() {
                            let _ = std::process::Command::new("explorer").arg(&e.champ).spawn();
                        }
                        if m.etoiles && e.note > 0 {
                            ui.weak("★".repeat(e.note as usize));
                        }
                        if let Some(d) = e.echeance {
                            let retard = e.statut == 0 && d < maintenant_utc();
                            ui.colored_label(if retard { Color32::LIGHT_RED } else { Color32::GRAY }, format_local(d, off));
                        }
                        if !m.champ.is_none() && !e.champ.is_empty() && !m.lien {
                            ui.weak(&e.champ);
                        }
                        if m.statuts.len() > 2 {
                            ui.weak(format!("· {}", m.statuts.get(e.statut).unwrap_or(&"")));
                        }
                    });
                }
                if v.is_empty() {
                    ui.weak("Rien ici.");
                }
            });
            let ui = &mut c[1];
            let Some(e) = self.choisi.and_then(|id| self.elements.iter_mut().find(|e| e.id == id)) else {
                ui.weak("Choisis un élément pour le modifier.");
                return;
            };
            ui.add(egui::TextEdit::singleline(&mut e.titre).font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
            if m.statuts.len() > 1 {
                ui.horizontal(|ui| {
                    for (i, s) in m.statuts.iter().enumerate() {
                        ui.selectable_value(&mut e.statut, i, *s);
                    }
                });
            }
            if let Some(ch) = m.champ {
                ui.horizontal(|ui| {
                    ui.label(ch);
                    ui.add(egui::TextEdit::singleline(&mut e.champ).desired_width(f32::INFINITY));
                });
            }
            ui.horizontal(|ui| {
                ui.label(m.categorie);
                ui.add(egui::TextEdit::singleline(&mut e.categorie).desired_width(200.0));
            });
            if m.etoiles {
                ui.horizontal(|ui| {
                    ui.label("Note");
                    for k in 1..=5u8 {
                        if ui.selectable_label(e.note >= k, "★").clicked() {
                            e.note = if e.note == k { 0 } else { k };
                        }
                    }
                });
            }
            if m.echeance {
                ui.horizontal(|ui| {
                    ui.label("Échéance");
                    if ui.add(egui::TextEdit::singleline(&mut self.saisie_date).hint_text("JJ/MM/AAAA HH:MM").desired_width(140.0)).changed() {
                        e.echeance = lire_local(&self.saisie_date, maintenant_utc(), off);
                        e.rappele = false;
                    }
                    for (n, j) in [("Aujourd'hui", 0), ("Demain", 1), ("Dans 7 j", 7)] {
                        if ui.small_button(n).clicked() {
                            let t = ((maintenant_utc() + off).div_euclid(86400) + j) * 86400 + 18 * 3600 - off;
                            e.echeance = Some(t);
                            e.rappele = false;
                            self.saisie_date = format_local(t, off);
                        }
                    }
                });
            }
            ui.add(egui::TextEdit::multiline(&mut e.commentaire).hint_text("Notes…").desired_rows(6).desired_width(f32::INFINITY));
            if ui.button("🗑 Supprimer").clicked() {
                supprimer = Some(e.id);
            }
        });
        if let Some(id) = supprimer {
            self.elements.retain(|e| e.id != id);
            self.choisi = None;
        }
        if self.elements != avant {
            enregistrer(m.fichier, &self.elements);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 125 · Kanban
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Carte {
    pub texte: String,
    pub colonne: usize,
    pub couleur: u8,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Tableau {
    pub nom: String,
    pub colonnes: Vec<String>,
    pub cartes: Vec<Carte>,
}

impl Default for Tableau {
    fn default() -> Self {
        Tableau { nom: "Mon projet".into(), colonnes: vec!["À faire".into(), "En cours".into(), "Fait".into()], cartes: vec![] }
    }
}

/// Déplace une carte d'une colonne (bornée).
pub fn deplacer(t: &mut Tableau, i: usize, sens: i32) {
    if let Some(c) = t.cartes.get_mut(i) {
        let n = (c.colonne as i32 + sens).clamp(0, t.colonnes.len() as i32 - 1);
        c.colonne = n as usize;
    }
}

pub struct Kanban {
    tableaux: Vec<Tableau>,
    choisi: usize,
    nouvelle: String,
}

impl Kanban {
    pub fn new() -> Self {
        let mut t: Vec<Tableau> = charger("kanban");
        if t.is_empty() {
            t.push(Tableau::default());
        }
        Kanban { tableaux: t, choisi: 0, nouvelle: String::new() }
    }
}

impl Outil for Kanban {
    fn nom(&self) -> &'static str {
        "🗂 Tableau Kanban"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tableau Kanban");
        let avant = self.tableaux.clone();
        ui.horizontal(|ui| {
            for (i, t) in self.tableaux.iter().enumerate() {
                if ui.selectable_label(self.choisi == i, &t.nom).clicked() {
                    self.choisi = i;
                }
            }
            if ui.button("➕ Tableau").clicked() {
                self.tableaux.push(Tableau { nom: format!("Projet {}", self.tableaux.len() + 1), ..Default::default() });
                self.choisi = self.tableaux.len() - 1;
            }
        });
        self.choisi = self.choisi.min(self.tableaux.len() - 1);
        let t = &mut self.tableaux[self.choisi];
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut t.nom).desired_width(200.0));
            let r = ui.add(egui::TextEdit::singleline(&mut self.nouvelle).hint_text("Nouvelle carte…").desired_width(260.0));
            if (ui.button("➕").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.nouvelle.trim().is_empty() {
                t.cartes.push(Carte { texte: std::mem::take(&mut self.nouvelle), ..Default::default() });
            }
            if ui.button("➕ Colonne").clicked() {
                t.colonnes.push("Nouvelle".into());
            }
        });
        ui.separator();
        let couleurs = [Color32::from_rgb(70, 50, 80), Color32::from_rgb(120, 50, 90), Color32::from_rgb(50, 80, 120), Color32::from_rgb(50, 110, 80), Color32::from_rgb(130, 100, 40)];
        let mut action: Option<(usize, i32)> = None;
        let mut retirer = None;
        let mut retirer_col = None;
        let n = t.colonnes.len();
        egui::ScrollArea::horizontal().id_salt("kanban").show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for col in 0..n {
                    ui.vertical(|ui| {
                        ui.set_width(230.0);
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut t.colonnes[col]).desired_width(170.0));
                            if n > 1 && t.cartes.iter().all(|c| c.colonne != col) && ui.small_button("✖").clicked() {
                                retirer_col = Some(col);
                            }
                        });
                        for (i, c) in t.cartes.iter_mut().enumerate().filter(|(_, c)| c.colonne == col) {
                            egui::Frame::new().fill(couleurs[c.couleur as usize % couleurs.len()]).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                                ui.set_width(214.0);
                                ui.add(egui::TextEdit::multiline(&mut c.texte).desired_rows(2).frame(false).desired_width(f32::INFINITY));
                                ui.horizontal(|ui| {
                                    if col > 0 && ui.small_button("◀").clicked() {
                                        action = Some((i, -1));
                                    }
                                    if col + 1 < n && ui.small_button("▶").clicked() {
                                        action = Some((i, 1));
                                    }
                                    if ui.small_button("🎨").clicked() {
                                        c.couleur = (c.couleur + 1) % couleurs.len() as u8;
                                    }
                                    if ui.small_button("🗑").clicked() {
                                        retirer = Some(i);
                                    }
                                });
                            });
                            ui.add_space(4.0);
                        }
                    });
                    ui.separator();
                }
            });
        });
        if let Some((i, s)) = action {
            deplacer(t, i, s);
        }
        if let Some(i) = retirer {
            t.cartes.remove(i);
        }
        if let Some(col) = retirer_col {
            t.colonnes.remove(col);
            for c in &mut t.cartes {
                if c.colonne > col {
                    c.colonne -= 1;
                }
            }
        }
        if self.tableaux != avant {
            enregistrer("kanban", &self.tableaux);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtre_et_tri() {
        let v = vec![
            Element { id: 1, titre: "Zelda".into(), statut: 0, ..Default::default() },
            Element { id: 2, titre: "appeler".into(), statut: 0, echeance: Some(10), ..Default::default() },
            Element { id: 3, titre: "Fini".into(), statut: 1, ..Default::default() },
        ];
        let r = vue(&v, "", None);
        assert_eq!(r.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2, 1, 3]);
        assert_eq!(vue(&v, "zel", None).len(), 1);
        assert_eq!(vue(&v, "", Some(1))[0].id, 3);
    }

    #[test]
    fn kanban() {
        let mut t = Tableau { cartes: vec![Carte::default()], ..Default::default() };
        deplacer(&mut t, 0, 1);
        deplacer(&mut t, 0, 5);
        assert_eq!(t.cartes[0].colonne, 2);
        deplacer(&mut t, 0, -9);
        assert_eq!(t.cartes[0].colonne, 0);
    }
}
