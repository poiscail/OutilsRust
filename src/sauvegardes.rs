//! Sauvegardes de jeux : zippe les dossiers de sauvegarde, garde un historique, restaure en un clic,
//! et sauvegarde tout seul quand un dossier a changé. Réglages dans %APPDATA%\Outils\sauvegardes.json.
use crate::minecraft::{horodatage, zipper};
use crate::systeme;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Entree {
    pub nom: String,
    pub dossier: PathBuf,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub jeux: Vec<Entree>,
    pub destination: PathBuf,
    pub garder: usize,
    pub auto: bool,
    pub auto_min: u32,
}

impl Default for Reglages {
    fn default() -> Self {
        let docs = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Documents");
        Reglages { jeux: vec![], destination: docs.join("Sauvegardes jeux"), garder: 10, auto: false, auto_min: 30 }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("sauvegardes.json")
}

/// Nom sans caractères interdits dans un nom de fichier Windows.
pub fn nom_sur(n: &str) -> String {
    let s: String = n.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "jeu".into() } else { s }
}

/// Date de modification la plus récente dans le dossier (récursif).
pub fn derniere_modif(d: &Path) -> Option<SystemTime> {
    let mut max = None;
    let mut pile = vec![d.to_path_buf()];
    while let Some(x) = pile.pop() {
        for e in fs::read_dir(&x).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                pile.push(e.path());
            } else if let Ok(t) = m.modified() {
                max = max.max(Some(t));
            }
        }
    }
    max
}

/// Archives d'un jeu, de la plus récente à la plus ancienne.
pub fn archives(dest: &Path, nom: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dest.join(nom_sur(nom))).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
        .collect();
    v.sort();
    v.reverse();
    v
}

/// Zippe le dossier du jeu dans destination\<nom>\<nom>_<date>.zip et taille l'historique.
pub fn sauvegarder(e: &Entree, dest: &Path, garder: usize) -> Result<PathBuf, String> {
    if !e.dossier.is_dir() {
        return Err(format!("Dossier introuvable : {}", e.dossier.display()));
    }
    let nom = nom_sur(&e.nom);
    let d = dest.join(&nom);
    fs::create_dir_all(&d).map_err(|x| x.to_string())?;
    let mut chemin = d.join(format!("{nom}_{}.zip", horodatage()));
    let mut i = 2;
    while chemin.exists() {
        chemin = d.join(format!("{nom}_{}_{i}.zip", horodatage()));
        i += 1;
    }
    zipper(&e.dossier, &e.dossier, &chemin)?;
    for vieux in archives(dest, &e.nom).into_iter().skip(garder.max(1)) {
        let _ = fs::remove_file(vieux);
    }
    Ok(chemin)
}

/// Remplace le contenu du dossier par l'archive (après une sauvegarde de sécurité de l'état actuel).
pub fn restaurer(e: &Entree, archive: &Path, dest: &Path, garder: usize) -> Result<(), String> {
    if e.dossier.is_dir() && fs::read_dir(&e.dossier).map(|mut r| r.next().is_some()).unwrap_or(false) {
        sauvegarder(&Entree { nom: format!("{} (avant restauration)", e.nom), dossier: e.dossier.clone() }, dest, garder)?;
    }
    let mut z = zip::ZipArchive::new(fs::File::open(archive).map_err(|x| x.to_string())?).map_err(|x| x.to_string())?;
    let tmp = e.dossier.with_extension("restauration_tmp");
    let _ = fs::remove_dir_all(&tmp);
    z.extract(&tmp).map_err(|x| x.to_string())?; // extract refuse les chemins qui sortent du dossier
    if e.dossier.exists() {
        fs::remove_dir_all(&e.dossier).map_err(|x| format!("Impossible de vider le dossier (jeu ouvert ?) : {x}"))?;
    }
    fs::rename(&tmp, &e.dossier).map_err(|x| x.to_string())
}

/// Dossiers de sauvegarde courants trouvés sur le PC (nom, chemin).
pub fn suggestions() -> Vec<(String, PathBuf)> {
    let profil = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
    let mut v = vec![];
    let sous = |d: &Path| -> Vec<PathBuf> {
        let mut l: Vec<PathBuf> = fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        l.sort();
        l
    };
    for racine in [profil.join("Saved Games"), profil.join("Documents").join("My Games")] {
        for p in sous(&racine) {
            v.push((p.file_name().unwrap_or_default().to_string_lossy().into_owned(), p));
        }
    }
    // Unity : LocalLow\Éditeur\Jeu
    for editeur in sous(&local.with_file_name("LocalLow")) {
        let n = editeur.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if ["Microsoft", "Unity", "Temp", "Sun", "Intel", "NVIDIA", "Adobe"].contains(&n.as_str()) {
            continue;
        }
        for jeu in sous(&editeur) {
            v.push((jeu.file_name().unwrap_or_default().to_string_lossy().into_owned(), jeu));
        }
    }
    let mc = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft").join("saves");
    if mc.is_dir() {
        v.push(("Minecraft (mondes solo)".into(), mc));
    }
    v
}

fn date_archive(p: &Path) -> String {
    // ..._AAAAMMJJ_HHMMSS.zip -> JJ/MM/AAAA HH:MM
    let n = p.file_stem().unwrap_or_default().to_string_lossy();
    let parties: Vec<&str> = n.rsplitn(3, '_').collect();
    let (d, h) = if parties.len() >= 2 && parties[0].len() == 6 && parties[1].len() == 8 {
        (parties[1], parties[0])
    } else if parties.len() == 3 && parties[1].len() == 6 && parties[2].ends_with(|c: char| c.is_ascii_digit()) {
        (&parties[2][parties[2].len().saturating_sub(8)..], parties[1])
    } else {
        return n.into_owned();
    };
    format!("{}/{}/{} {}:{}", &d[6..8], &d[4..6], &d[0..4], &h[0..2], &h[2..4])
}

#[derive(Default)]
struct Etat {
    occupe: bool,
    message: String,
}

pub struct Sauvegardes {
    r: Arc<Mutex<Reglages>>,
    etat: Arc<Mutex<Etat>>,
    suggestions: Option<Vec<(String, PathBuf)>>,
    ouvert: Option<usize>,
    confirmer: Option<(usize, PathBuf)>,
}

impl Sauvegardes {
    pub fn new() -> Self {
        let r: Reglages = fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let r = Arc::new(Mutex::new(r));
        let etat: Arc<Mutex<Etat>> = Arc::default();
        // fil automatique : toutes les minutes, sauvegarde les jeux modifiés depuis leur dernière archive
        let (r2, e2) = (r.clone(), etat.clone());
        std::thread::spawn(move || {
            let mut derniers: std::collections::HashMap<String, SystemTime> = Default::default();
            loop {
                std::thread::sleep(Duration::from_secs(60));
                let reg = r2.lock().unwrap().clone();
                if !reg.auto {
                    continue;
                }
                for j in &reg.jeux {
                    let derniere_archive = derniers.get(&j.nom).copied().or_else(|| {
                        archives(&reg.destination, &j.nom).first().and_then(|a| a.metadata().ok()?.modified().ok())
                    });
                    let attendu = Duration::from_secs(60 * reg.auto_min.max(5) as u64);
                    let assez_vieux = derniere_archive.is_none_or(|t| t.elapsed().unwrap_or_default() >= attendu);
                    let change = match (derniere_modif(&j.dossier), derniere_archive) {
                        (Some(m), Some(a)) => m > a,
                        (Some(_), None) => true,
                        _ => false,
                    };
                    if assez_vieux && change {
                        let res = sauvegarder(j, &reg.destination, reg.garder);
                        derniers.insert(j.nom.clone(), SystemTime::now());
                        if let Err(x) = res {
                            e2.lock().unwrap().message = format!("Sauvegarde auto de {} : {x}", j.nom);
                        }
                    }
                }
            }
        });
        Sauvegardes { r, etat, suggestions: None, ouvert: None, confirmer: None }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = fs::create_dir_all(f.parent().unwrap());
        if let Ok(s) = serde_json::to_string_pretty(&*self.r.lock().unwrap()) {
            let _ = fs::write(f, s);
        }
    }

    fn en_fond(&self, quoi: &'static str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let etat = self.etat.clone();
        {
            let mut e = etat.lock().unwrap();
            if e.occupe {
                return;
            }
            e.occupe = true;
            e.message = format!("{quoi}…");
        }
        std::thread::spawn(move || {
            let res = f();
            let mut e = etat.lock().unwrap();
            e.occupe = false;
            e.message = match res {
                Ok(m) => m,
                Err(x) => format!("Erreur : {x}"),
            };
        });
    }

    pub fn resume(&self) -> String {
        let r = self.r.lock().unwrap();
        format!("{} jeu(x) suivi(s){}", r.jeux.len(), if r.auto { " · auto" } else { "" })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegardes de jeux");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        let (occupe, message) = {
            let e = self.etat.lock().unwrap();
            (e.occupe, e.message.clone())
        };
        ui.horizontal(|ui| {
            ui.label("Rangées dans :");
            ui.monospace(r.destination.display().to_string());
            if ui.button("Changer…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    r.destination = d;
                }
            }
            if ui.button("📂").on_hover_text("Ouvrir").clicked() {
                let _ = fs::create_dir_all(&r.destination);
                let _ = std::process::Command::new("explorer").arg(&r.destination).spawn();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Garder");
            ui.add(egui::DragValue::new(&mut r.garder).range(1..=200));
            ui.label("archives par jeu");
            ui.separator();
            ui.checkbox(&mut r.auto, "Sauvegarde auto si modifié, au plus toutes les");
            ui.add_enabled(r.auto, egui::DragValue::new(&mut r.auto_min).range(5..=1440).suffix(" min"));
        });
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter un dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    let nom = d.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    r.jeux.push(Entree { nom, dossier: d });
                }
            }
            if ui.button("🔎 Trouver les sauvegardes sur le PC").clicked() {
                self.suggestions = Some(suggestions());
            }
            if ui.add_enabled(!occupe && !r.jeux.is_empty(), egui::Button::new("💾 Tout sauvegarder")).clicked() {
                let (jeux, dest, garder) = (r.jeux.clone(), r.destination.clone(), r.garder);
                self.en_fond("Sauvegarde", move || {
                    let mut erreurs = vec![];
                    for j in &jeux {
                        if let Err(x) = sauvegarder(j, &dest, garder) {
                            erreurs.push(format!("{} : {x}", j.nom));
                        }
                    }
                    if erreurs.is_empty() { Ok(format!("{} jeu(x) sauvegardé(s).", jeux.len())) } else { Err(erreurs.join(" · ")) }
                });
            }
        });
        if occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&message);
            });
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        } else if !message.is_empty() {
            ui.label(&message);
        }

        if let Some(s) = &self.suggestions {
            let mut fermer = false;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("{} dossier(s) trouvé(s)", s.len()));
                    if ui.small_button("Fermer").clicked() {
                        fermer = true;
                    }
                });
                egui::ScrollArea::vertical().id_salt("sugg").max_height(160.0).show(ui, |ui| {
                    for (n, p) in s {
                        let deja = r.jeux.iter().any(|j| &j.dossier == p);
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!deja, egui::Button::new(if deja { "✔" } else { "➕" }).small()).clicked() {
                                r.jeux.push(Entree { nom: n.clone(), dossier: p.clone() });
                            }
                            ui.label(n);
                            ui.weak(p.display().to_string());
                        });
                    }
                });
            });
            if fermer {
                self.suggestions = None;
            }
        }
        ui.separator();

        let mut retirer = None;
        egui::ScrollArea::vertical().id_salt("jeux_sauv").show(ui, |ui| {
            for (i, j) in r.jeux.iter_mut().enumerate() {
                let liste = archives(&avant.destination, &j.nom);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut j.nom).desired_width(180.0));
                        ui.weak(j.dossier.display().to_string());
                        if !j.dossier.is_dir() {
                            ui.colored_label(egui::Color32::LIGHT_RED, "introuvable");
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!occupe, egui::Button::new("💾 Sauvegarder")).clicked() {
                            let (e, d, g) = (j.clone(), avant.destination.clone(), avant.garder);
                            self.en_fond("Sauvegarde", move || sauvegarder(&e, &d, g).map(|p| format!("Sauvé : {}", p.display())));
                        }
                        let titre = match liste.first() {
                            Some(a) => format!("🕘 {} archive(s), dernière le {}", liste.len(), date_archive(a)),
                            None => "Aucune archive".into(),
                        };
                        if ui.add_enabled(!liste.is_empty(), egui::Button::new(titre)).clicked() {
                            self.ouvert = if self.ouvert == Some(i) { None } else { Some(i) };
                        }
                        if ui.button("📂").on_hover_text("Dossier du jeu").clicked() {
                            let _ = std::process::Command::new("explorer").arg(&j.dossier).spawn();
                        }
                        if ui.button("🗑").on_hover_text("Ne plus suivre (les archives restent)").clicked() {
                            retirer = Some(i);
                        }
                    });
                    if self.ouvert == Some(i) {
                        for a in &liste {
                            ui.horizontal(|ui| {
                                ui.label(date_archive(a));
                                ui.weak(crate::disque::taille_lisible(a.metadata().map(|m| m.len()).unwrap_or(0)));
                                if ui.add_enabled(!occupe, egui::Button::new("↩ Restaurer").small()).clicked() {
                                    self.confirmer = Some((i, a.clone()));
                                }
                            });
                        }
                    }
                });
            }
        });
        if let Some(i) = retirer {
            r.jeux.remove(i);
            self.ouvert = None;
        }

        if let Some((i, a)) = self.confirmer.clone() {
            let mut garder_ouvert = true;
            egui::Window::new("Restaurer ?").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
                if let Some(j) = r.jeux.get(i) {
                    ui.label(format!("Remplacer les sauvegardes de {} par celles du {} ?", j.nom, date_archive(&a)));
                    ui.weak("L'état actuel est d'abord archivé (« avant restauration »). Ferme le jeu avant.");
                    ui.horizontal(|ui| {
                        if ui.button("Restaurer").clicked() {
                            let (e, d, g) = (j.clone(), avant.destination.clone(), avant.garder);
                            self.en_fond("Restauration", move || {
                                restaurer(&e, &a, &d, g)?;
                                systeme::notifier("Sauvegardes", &format!("{} restauré.", e.nom));
                                Ok(format!("{} restauré.", e.nom))
                            });
                            garder_ouvert = false;
                        }
                        if ui.button("Annuler").clicked() {
                            garder_ouvert = false;
                        }
                    });
                } else {
                    garder_ouvert = false;
                }
            });
            if !garder_ouvert {
                self.confirmer = None;
            }
        }

        if r != avant {
            *self.r.lock().unwrap() = r;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noms() {
        assert_eq!(nom_sur("Elden Ring: DLC?"), "Elden Ring_ DLC_");
        assert_eq!(nom_sur("  ..  "), "jeu");
    }

    #[test]
    fn dates() {
        assert_eq!(date_archive(Path::new("x/Elden_Ring_20261118_193005.zip")), "18/11/2026 19:30");
        assert_eq!(date_archive(Path::new("x/Jeu_20261118_193005_2.zip")), "18/11/2026 19:30");
    }

    #[test]
    fn aller_retour() {
        let base = std::env::temp_dir().join(format!("outils_sauv_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let jeu = base.join("jeu");
        fs::create_dir_all(jeu.join("slot")).unwrap();
        fs::write(jeu.join("slot/save.dat"), "v1").unwrap();
        let e = Entree { nom: "Mon Jeu".into(), dossier: jeu.clone() };
        let dest = base.join("dest");
        let a = sauvegarder(&e, &dest, 3).unwrap();
        fs::write(jeu.join("slot/save.dat"), "v2").unwrap();
        fs::write(jeu.join("nouveau.dat"), "x").unwrap();
        restaurer(&e, &a, &dest, 3).unwrap();
        assert_eq!(fs::read_to_string(jeu.join("slot/save.dat")).unwrap(), "v1");
        assert!(!jeu.join("nouveau.dat").exists());
        assert_eq!(archives(&dest, "Mon Jeu (avant restauration)").len(), 1);
        for _ in 0..4 {
            sauvegarder(&e, &dest, 3).unwrap();
        }
        assert_eq!(archives(&dest, "Mon Jeu").len(), 3);
        let _ = fs::remove_dir_all(&base);
    }
}
