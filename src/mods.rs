//! Gestionnaire de mods : par jeu, active / désactive chaque mod (déplacé dans un dossier « _desactives »
//! à côté), profils de mods à appliquer en un clic, ajout par glisser-déposer.
//! Réglages dans %APPDATA%\Outils\mods.json.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Jeu {
    pub nom: String,
    pub dossier: PathBuf,
    pub profils: BTreeMap<String, BTreeSet<String>>, // nom -> mods actifs
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Reglages {
    pub jeux: Vec<Jeu>,
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("mods.json")
}

/// Dossier où vont les mods désactivés.
pub fn dossier_off(d: &Path) -> PathBuf {
    let nom = d.file_name().unwrap_or_default().to_string_lossy();
    d.with_file_name(format!("{nom}_desactives"))
}

/// (nom, actif) de chaque mod (fichiers et dossiers), trié.
pub fn lister(d: &Path) -> Vec<(String, bool)> {
    let lire = |x: &Path| -> Vec<String> {
        fs::read_dir(x).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.')).collect()
    };
    let mut v: Vec<(String, bool)> = lire(d).into_iter().map(|n| (n, true)).collect();
    v.extend(lire(&dossier_off(d)).into_iter().map(|n| (n, false)));
    v.sort_by_key(|(n, _)| n.to_lowercase());
    v
}

pub fn basculer(d: &Path, nom: &str, actif: bool) -> Result<(), String> {
    let (de, vers) = if actif { (dossier_off(d), d.to_path_buf()) } else { (d.to_path_buf(), dossier_off(d)) };
    let src = de.join(nom);
    if !src.exists() {
        return Ok(()); // déjà dans le bon état
    }
    fs::create_dir_all(&vers).map_err(|e| e.to_string())?;
    fs::rename(&src, vers.join(nom)).map_err(|e| format!("{nom} : {e} (jeu ouvert ?)"))
}

/// Active exactement les mods du profil (les mods inconnus du profil sont désactivés).
pub fn appliquer(d: &Path, actifs: &BTreeSet<String>) -> Vec<String> {
    lister(d).into_iter().filter_map(|(n, on)| {
        let voulu = actifs.contains(&n);
        (voulu != on).then(|| basculer(d, &n, voulu).err()).flatten()
    }).collect()
}

fn copier_dans(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)?;
        for e in fs::read_dir(src)?.flatten() {
            copier_dans(&e.path(), &dest.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}

pub struct Mods {
    r: Reglages,
    choisi: usize,
    nouveau_profil: String,
    message: String,
}

impl Mods {
    pub fn new() -> Self {
        let mut r: Reglages = fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if r.jeux.is_empty() {
            let mc = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft").join("mods");
            if mc.is_dir() {
                r.jeux.push(Jeu { nom: "Minecraft".into(), dossier: mc, profils: BTreeMap::new() });
            }
        }
        Mods { r, choisi: 0, nouveau_profil: String::new(), message: String::new() }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = fs::create_dir_all(f.parent().unwrap());
        let _ = fs::write(f, serde_json::to_string_pretty(&self.r).unwrap_or_default());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mods");
        let avant = self.r.clone();
        ui.horizontal(|ui| {
            for (i, j) in self.r.jeux.iter().enumerate() {
                if ui.selectable_label(self.choisi == i, &j.nom).clicked() {
                    self.choisi = i;
                }
            }
            if ui.button("➕ Jeu…").on_hover_text("Choisis le dossier des mods du jeu").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    let nom = d.parent().and_then(|p| p.file_name()).unwrap_or_default().to_string_lossy().into_owned();
                    self.r.jeux.push(Jeu { nom, dossier: d, profils: BTreeMap::new() });
                    self.choisi = self.r.jeux.len() - 1;
                }
            }
        });
        if self.r.jeux.is_empty() {
            ui.weak("Ajoute le dossier des mods d'un jeu (ex. %APPDATA%\\.minecraft\\mods).");
            return;
        }
        self.choisi = self.choisi.min(self.r.jeux.len() - 1);
        let mut supprimer_jeu = false;
        let jeu = &mut self.r.jeux[self.choisi];
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut jeu.nom).desired_width(160.0));
            ui.monospace(jeu.dossier.display().to_string());
            if ui.small_button("📂").clicked() {
                let _ = std::process::Command::new("explorer").arg(&jeu.dossier).spawn();
            }
            if ui.small_button("🗑").on_hover_text("Ne plus gérer ce jeu (les fichiers restent)").clicked() {
                supprimer_jeu = true;
            }
        });
        let mods = lister(&jeu.dossier);
        let actifs: BTreeSet<String> = mods.iter().filter(|(_, a)| *a).map(|(n, _)| n.clone()).collect();
        // profils
        ui.horizontal_wrapped(|ui| {
            ui.strong("Profils :");
            let mut appliquer_p = None;
            let mut retirer = None;
            for (nom, liste) in &jeu.profils {
                let courant = liste == &actifs;
                let rep = ui.selectable_label(courant, nom).on_hover_text(format!("{} mod(s) · clic droit pour supprimer", liste.len()));
                if rep.clicked() {
                    appliquer_p = Some(liste.clone());
                }
                if rep.secondary_clicked() {
                    retirer = Some(nom.clone());
                }
            }
            if let Some(l) = appliquer_p {
                let err = appliquer(&jeu.dossier, &l);
                self.message = if err.is_empty() { "Profil appliqué.".into() } else { err.join(" · ") };
            }
            if let Some(n) = retirer {
                jeu.profils.remove(&n);
            }
            ui.add(egui::TextEdit::singleline(&mut self.nouveau_profil).hint_text("nom du profil").desired_width(120.0));
            if ui.button("💾 Enregistrer").clicked() && !self.nouveau_profil.trim().is_empty() {
                jeu.profils.insert(self.nouveau_profil.trim().to_string(), actifs.clone());
                self.nouveau_profil.clear();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Tout activer").clicked() {
                let tous = mods.iter().map(|(n, _)| n.clone()).collect();
                appliquer(&jeu.dossier, &tous);
            }
            if ui.button("Tout désactiver").clicked() {
                appliquer(&jeu.dossier, &BTreeSet::new());
            }
            if ui.button("➕ Ajouter des mods…").clicked() {
                if let Some(v) = rfd::FileDialog::new().pick_files() {
                    for p in v {
                        let _ = copier_dans(&p, &jeu.dossier.join(p.file_name().unwrap_or_default()));
                    }
                }
            }
            ui.weak(format!("{} actif(s) sur {}", actifs.len(), mods.len()));
        });
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in deposes {
            let _ = fs::create_dir_all(&jeu.dossier);
            if let Err(e) = copier_dans(&p, &jeu.dossier.join(p.file_name().unwrap_or_default())) {
                self.message = e.to_string();
            }
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.separator();
        egui::ScrollArea::vertical().id_salt("liste_mods").show(ui, |ui| {
            for (n, actif) in &mods {
                let mut on = *actif;
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, "").changed() {
                        if let Err(e) = basculer(&jeu.dossier, n, on) {
                            self.message = e;
                        }
                    }
                    if *actif { ui.label(n) } else { ui.colored_label(Color32::GRAY, n) };
                });
            }
            if mods.is_empty() {
                ui.weak("Aucun mod dans ce dossier. Glisse des fichiers ici pour les ajouter.");
            }
        });
        if supprimer_jeu {
            self.r.jeux.remove(self.choisi);
            self.choisi = 0;
        }
        if self.r != avant {
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profils() {
        let base = std::env::temp_dir().join(format!("outils_mods_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let d = base.join("mods");
        fs::create_dir_all(&d).unwrap();
        for n in ["a.jar", "b.jar", "c.jar"] {
            fs::write(d.join(n), n).unwrap();
        }
        basculer(&d, "b.jar", false).unwrap();
        assert_eq!(lister(&d), vec![("a.jar".into(), true), ("b.jar".into(), false), ("c.jar".into(), true)]);
        assert!(dossier_off(&d).join("b.jar").exists());
        let p: BTreeSet<String> = ["b.jar".to_string()].into();
        assert!(appliquer(&d, &p).is_empty());
        assert_eq!(lister(&d), vec![("a.jar".into(), false), ("b.jar".into(), true), ("c.jar".into(), false)]);
        let _ = fs::remove_dir_all(&base);
    }
}
