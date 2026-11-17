//! Renommage par lot : remplacer du texte (ou une regex) ou appliquer un modèle {n} / {nom}, avec aperçu et annulation.
use eframe::egui;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(PartialEq, Clone, Copy)]
pub enum Mode {
    Remplacer,
    Modele,
}

pub struct Regle {
    pub mode: Mode,
    pub cherche: String,
    pub remplace: String,
    pub regex: bool,
    pub modele: String,
    pub debut: u32,
}

fn decouper(nom: &str) -> (&str, &str) {
    // l'extension (avec son point) est gardée telle quelle ; ".gitignore" n'a pas d'extension
    match nom.rfind('.') {
        Some(i) if i > 0 => (&nom[..i], &nom[i..]),
        _ => (nom, ""),
    }
}

pub fn nouveaux_noms(noms: &[String], r: &Regle) -> Result<Vec<String>, String> {
    let re = if r.mode == Mode::Remplacer && r.regex && !r.cherche.is_empty() {
        Some(regex::Regex::new(&r.cherche).map_err(|e| format!("Regex invalide : {e}"))?)
    } else {
        None
    };
    let largeur = (r.debut as usize + noms.len().saturating_sub(1)).to_string().len();
    Ok(noms.iter().enumerate().map(|(i, n)| {
        let (tige, ext) = decouper(n);
        let t = match r.mode {
            Mode::Remplacer => match &re {
                Some(re) => re.replace_all(tige, r.remplace.as_str()).into_owned(),
                None if r.cherche.is_empty() => tige.to_string(),
                None => tige.replace(&r.cherche, &r.remplace),
            },
            Mode::Modele => r.modele.replace("{n}", &format!("{:0largeur$}", r.debut as usize + i)).replace("{nom}", tige),
        };
        format!("{t}{ext}")
    }).collect())
}

/// Problème de chaque nouveau nom (None = OK).
pub fn problemes(dossier: &Path, anciens: &[String], nouveaux: &[String]) -> Vec<Option<&'static str>> {
    let bas = |s: &str| s.to_lowercase(); // Windows ignore la casse
    let renommes: HashSet<String> = anciens.iter().map(|a| bas(a)).collect();
    let mut vus = HashSet::new();
    let doublons: HashSet<String> = nouveaux.iter().map(|n| bas(n)).filter(|n| !vus.insert(n.clone())).collect();
    nouveaux.iter().zip(anciens).map(|(n, a)| {
        if n.trim().is_empty() || n.trim_end_matches('.').is_empty() {
            Some("nom vide")
        } else if n.chars().any(|c| "<>:\"/\\|?*".contains(c) || c.is_control()) {
            Some("caractère interdit")
        } else if doublons.contains(&bas(n)) {
            Some("doublon")
        } else if n != a && !renommes.contains(&bas(n)) && dossier.join(n).exists() {
            Some("existe déjà")
        } else {
            None
        }
    }).collect()
}

/// Renomme en deux temps (via des noms temporaires) pour permettre les échanges a↔b. Rend les paires (nouveau, ancien).
fn renommer(dossier: &Path, paires: &[(String, String)]) -> Result<Vec<(String, String)>, String> {
    let paires: Vec<_> = paires.iter().filter(|(a, n)| a != n).collect();
    let tmp = |i: usize| dossier.join(format!(".__renommer_{i}.tmp"));
    for (i, (a, _)) in paires.iter().enumerate() {
        if let Err(e) = std::fs::rename(dossier.join(a), tmp(i)) {
            for (j, (a, _)) in paires.iter().enumerate().take(i) {
                let _ = std::fs::rename(tmp(j), dossier.join(a)); // on remet ceux déjà déplacés
            }
            return Err(format!("{a} : {e}"));
        }
    }
    let mut fait = Vec::new();
    let mut erreur = None;
    for (i, (a, n)) in paires.iter().enumerate() {
        match std::fs::rename(tmp(i), dossier.join(n)) {
            Ok(()) => fait.push((n.clone(), a.clone())),
            Err(e) => {
                let _ = std::fs::rename(tmp(i), dossier.join(a)); // on remet l'original
                erreur.get_or_insert(format!("{n} : {e}"));
            }
        }
    }
    erreur.map_or(Ok(fait), Err)
}

pub struct Renommer {
    dossier: Option<PathBuf>,
    filtre: String,
    fichiers: Vec<String>,
    regle: Regle,
    historique: Vec<(String, String)>,
    message: String,
}

impl Renommer {
    pub fn new() -> Self {
        Renommer {
            dossier: None,
            filtre: String::new(),
            fichiers: Vec::new(),
            regle: Regle { mode: Mode::Remplacer, cherche: String::new(), remplace: String::new(), regex: false, modele: "{nom}_{n}".into(), debut: 1 },
            historique: Vec::new(),
            message: String::new(),
        }
    }

    fn lister(&mut self) {
        let exts: Vec<String> = self.filtre.split([',', ' ', ';']).map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect();
        self.fichiers = self.dossier.as_ref().and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| exts.is_empty() || exts.iter().any(|x| n.to_lowercase().ends_with(&format!(".{x}"))))
            .collect();
        self.fichiers.sort_by_key(|n| n.to_lowercase());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Renommer par lot");
        ui.horizontal(|ui| {
            if ui.button("📁 Choisir un dossier").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.dossier = Some(d);
                    self.historique.clear();
                    self.lister();
                }
            }
            if let Some(d) = &self.dossier {
                ui.label(d.display().to_string());
            }
        });
        let Some(dossier) = self.dossier.clone() else {
            ui.label("Choisis un dossier pour commencer.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Extensions (ex. jpg, png ; vide = toutes)");
            if ui.text_edit_singleline(&mut self.filtre).changed() {
                self.lister();
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.regle.mode, Mode::Remplacer, "Remplacer du texte");
            ui.radio_value(&mut self.regle.mode, Mode::Modele, "Modèle");
        });
        match self.regle.mode {
            Mode::Remplacer => {
                ui.horizontal(|ui| {
                    ui.label("Chercher");
                    ui.text_edit_singleline(&mut self.regle.cherche);
                    ui.label("Remplacer par");
                    ui.text_edit_singleline(&mut self.regle.remplace);
                    ui.checkbox(&mut self.regle.regex, "Regex");
                });
            }
            Mode::Modele => {
                ui.horizontal(|ui| {
                    ui.label("Modèle");
                    ui.text_edit_singleline(&mut self.regle.modele);
                    ui.label("Début");
                    ui.add(egui::DragValue::new(&mut self.regle.debut).range(0..=99999));
                });
                ui.small("{n} = numéro, {nom} = nom d'origine. L'extension est gardée.");
            }
        }
        let nouveaux = match nouveaux_noms(&self.fichiers, &self.regle) {
            Ok(n) => n,
            Err(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
                self.fichiers.clone()
            }
        };
        let probs = problemes(&dossier, &self.fichiers, &nouveaux);
        let a_faire = self.fichiers.iter().zip(&nouveaux).filter(|(a, n)| a != n).count();
        let bloque = probs.iter().any(Option::is_some);
        ui.horizontal(|ui| {
            if ui.add_enabled(a_faire > 0 && !bloque, egui::Button::new(format!("✏ Renommer ({a_faire})"))).clicked() {
                let paires: Vec<_> = self.fichiers.iter().cloned().zip(nouveaux.iter().cloned()).collect();
                match renommer(&dossier, &paires) {
                    Ok(h) => {
                        self.message = format!("{} fichier(s) renommé(s).", h.len());
                        self.historique = h;
                    }
                    Err(e) => self.message = format!("Erreur : {e}"),
                }
                self.lister();
            }
            if ui.add_enabled(!self.historique.is_empty(), egui::Button::new("↶ Annuler")).clicked() {
                match renommer(&dossier, &self.historique) {
                    Ok(_) => self.message = "Renommage annulé.".into(),
                    Err(e) => self.message = format!("Erreur : {e}"),
                }
                self.historique.clear();
                self.lister();
            }
            ui.label(&self.message);
        });
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("apercu").striped(true).num_columns(3).show(ui, |ui| {
                for ((a, n), p) in self.fichiers.iter().zip(&nouveaux).zip(&probs) {
                    ui.label(a);
                    if a == n {
                        ui.weak(n);
                    } else {
                        ui.strong(n);
                    }
                    if let Some(p) = p {
                        ui.colored_label(egui::Color32::LIGHT_RED, *p);
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regle(mode: Mode) -> Regle {
        Regle { mode, cherche: String::new(), remplace: String::new(), regex: false, modele: String::new(), debut: 1 }
    }

    #[test]
    fn noms() {
        let f: Vec<String> = ["IMG_001.jpg", "IMG_002.JPG", "notes", ".gitignore"].map(String::from).into();
        let mut r = regle(Mode::Remplacer);
        r.cherche = "IMG_".into();
        r.remplace = "Vacances ".into();
        assert_eq!(nouveaux_noms(&f, &r).unwrap(), ["Vacances 001.jpg", "Vacances 002.JPG", "notes", ".gitignore"]);
        r.regex = true;
        r.cherche = r"\d+".into();
        r.remplace = "#$0".into();
        assert_eq!(nouveaux_noms(&f, &r).unwrap()[0], "IMG_#001.jpg");
        r.cherche = "(".into();
        assert!(nouveaux_noms(&f, &r).is_err());
        let mut m = regle(Mode::Modele);
        m.modele = "photo_{n}_{nom}".into();
        m.debut = 9;
        let n = nouveaux_noms(&f, &m).unwrap();
        assert_eq!(n[0], "photo_09_IMG_001.jpg");
        assert_eq!(n[3], "photo_12_.gitignore");
    }

    #[test]
    fn conflits() {
        let d = std::env::temp_dir();
        let a: Vec<String> = ["a.txt", "b.txt", "c.txt"].map(String::from).into();
        let n: Vec<String> = ["b.txt", "A.TXT", "x?.txt"].map(String::from).into();
        assert_eq!(problemes(&d, &a, &n), [None, None, Some("caractère interdit")]);
        let n: Vec<String> = ["z.txt", "Z.txt", ""].map(String::from).into();
        assert_eq!(problemes(&d, &a, &n), [Some("doublon"), Some("doublon"), Some("nom vide")]);
    }

    #[test]
    fn echange() {
        let d = std::env::temp_dir().join(format!("renommer_test_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a"), "A").unwrap();
        std::fs::write(d.join("b"), "B").unwrap();
        let h = renommer(&d, &[("a".into(), "b".into()), ("b".into(), "a".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "B");
        renommer(&d, &h).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a")).unwrap(), "A");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
