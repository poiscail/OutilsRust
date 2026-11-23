//! Profils (33) : plusieurs personnes sur le même PC, chacune avec ses réglages, notes, thème, favoris…
//! (un code facultatif protège un profil des petits curieux). Sauvegarde dans le cloud (34) : copie
//! datée de tous les réglages, chaque jour, dans OneDrive / Google Drive / Dropbox.
use crate::outil::{charger, enregistrer, racine, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Profil {
    pub nom: String,
    pub emoji: String,
    /// empreinte du code (sel + SHA-256), vide si pas de code
    pub code: String,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Liste {
    pub profils: Vec<Profil>,
    pub dernier: Option<String>,
}

fn fichier_liste() -> PathBuf {
    racine().join("profils.json")
}

pub fn liste() -> Liste {
    std::fs::read_to_string(fichier_liste()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn garder(l: &Liste) {
    let _ = std::fs::create_dir_all(racine());
    if let Ok(s) = serde_json::to_string_pretty(l) {
        let _ = std::fs::write(fichier_liste(), s);
    }
}

/// Nom utilisable comme nom de dossier.
pub fn nom_sur(n: &str) -> String {
    let s: String = n.trim().chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).take(30).collect();
    s.trim().to_string()
}

pub fn empreinte(code: &str, sel: &str) -> String {
    let h = Sha256::digest(format!("outils|{sel}|{code}").as_bytes());
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn verifier_code(p: &Profil, code: &str) -> bool {
    p.code.is_empty() || empreinte(code, &p.nom) == p.code
}

/// Profil verrouillé par un code tant qu'il n'est pas saisi.
pub static VERROU: AtomicBool = AtomicBool::new(false);

/// À appeler tout au début : choisit le profil (`--profil Nom` ou le dernier utilisé).
pub fn choisir_au_demarrage() {
    let l = liste();
    let mut args = std::env::args();
    let demande = std::iter::from_fn(|| args.next()).skip_while(|a| a != "--profil").nth(1);
    let nom = demande.or(l.dernier.clone()).map(|n| nom_sur(&n)).filter(|n| !n.is_empty());
    let p = nom.and_then(|n| l.profils.iter().find(|p| p.nom == n).cloned());
    if let Some(p) = &p {
        let _ = std::fs::create_dir_all(racine().join("profils").join(&p.nom));
        VERROU.store(!p.code.is_empty(), Relaxed);
    }
    let _ = crate::outil::PROFIL.set(p.map(|p| p.nom));
}

pub fn actif() -> Option<String> {
    crate::outil::PROFIL.get().cloned().flatten()
}

/// Relance Outils sur un autre profil (None : principal).
pub fn changer(nom: Option<&str>) {
    let mut l = liste();
    l.dernier = nom.map(str::to_string);
    garder(&l);
    if let Ok(exe) = std::env::current_exe() {
        let mut c = std::process::Command::new(exe);
        c.arg("--profil").arg(nom.unwrap_or("-"));
        if c.spawn().is_ok() {
            crate::systeme::QUITTER.store(true, Relaxed);
        }
    }
}

/// Écran de code du profil ; renvoie vrai tant qu'il est verrouillé.
pub fn ecran_code(ctx: &egui::Context, saisie: &mut String) -> bool {
    if !VERROU.load(Relaxed) {
        return false;
    }
    let l = liste();
    let p = actif().and_then(|n| l.profils.into_iter().find(|p| p.nom == n)).unwrap_or_default();
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(120.0);
            ui.label(RichText::new(if p.emoji.is_empty() { "👤" } else { &p.emoji }).size(64.0));
            ui.heading(&p.nom);
            ui.add_space(10.0);
            let r = ui.add(egui::TextEdit::singleline(saisie).password(true).hint_text("code du profil").desired_width(180.0));
            r.request_focus();
            let ok = ui.button("Entrer").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if ok {
                if verifier_code(&p, saisie) {
                    VERROU.store(false, Relaxed);
                } else {
                    saisie.clear();
                }
            }
            ui.add_space(20.0);
            if ui.small_button("Changer de profil (principal)").clicked() {
                changer(None);
            }
        });
    });
    true
}

pub struct Profils {
    l: Liste,
    nom: String,
    emoji: String,
    code: String,
    message: String,
}

impl Profils {
    pub fn new() -> Self {
        Profils { l: liste(), nom: String::new(), emoji: "🙂".into(), code: String::new(), message: String::new() }
    }
}

const EMOJIS: [&str; 12] = ["🙂", "😎", "🦊", "🐱", "🐼", "🦄", "🎮", "⚽", "🎨", "🎧", "👾", "🌟"];

impl Outil for Profils {
    fn nom(&self) -> &'static str {
        "👥 Profils"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Profils");
        ui.weak("Chaque personne de la maison a son propre Outils : thème, notes, tâches, favoris, réglages. Les programmes téléchargés (ffmpeg…) sont partagés.");
        let courant = actif();
        ui.label(RichText::new(format!("Profil actuel : {}", courant.clone().unwrap_or_else(|| "Principal".into()))).strong());
        ui.separator();
        ui.horizontal(|ui| {
            let ici = courant.is_none();
            if ui.add_enabled(!ici, egui::Button::new(RichText::new("🏠 Principal").size(18.0)).min_size(egui::vec2(150.0, 44.0))).clicked() {
                changer(None);
            }
        });
        let mut retirer = None;
        for (k, p) in self.l.profils.clone().into_iter().enumerate() {
            ui.horizontal(|ui| {
                let ici = courant.as_deref() == Some(p.nom.as_str());
                let t = format!("{} {}{}", p.emoji, p.nom, if p.code.is_empty() { "" } else { " 🔒" });
                if ui.add_enabled(!ici, egui::Button::new(RichText::new(t).size(18.0)).min_size(egui::vec2(150.0, 44.0))).clicked() {
                    changer(Some(&p.nom));
                }
                if ici {
                    ui.colored_label(Color32::LIGHT_GREEN, "← actif");
                }
                if ui.small_button("🖥 Raccourci bureau").on_hover_text("Icône sur le bureau qui ouvre directement ce profil").clicked() {
                    self.message = raccourci_bureau(&p.nom).map(|_| "✔ Raccourci créé sur le bureau".into()).unwrap_or_else(|e| format!("✖ {e}"));
                }
                if !ici && ui.small_button("🗑").on_hover_text("Retirer de la liste (ses fichiers restent dans le dossier profils)").clicked() {
                    retirer = Some(k);
                }
            });
        }
        if let Some(k) = retirer {
            self.l.profils.remove(k);
            garder(&self.l);
        }
        ui.separator();
        ui.strong("Nouveau profil");
        ui.horizontal_wrapped(|ui| {
            for e in EMOJIS {
                ui.selectable_value(&mut self.emoji, e.to_string(), RichText::new(e).size(18.0));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("prénom").desired_width(140.0));
            ui.add(egui::TextEdit::singleline(&mut self.code).password(true).hint_text("code (facultatif)").desired_width(120.0));
            let n = nom_sur(&self.nom);
            let libre = !n.is_empty() && !self.l.profils.iter().any(|p| p.nom.eq_ignore_ascii_case(&n));
            if ui.add_enabled(libre, egui::Button::new("➕ Créer")).clicked() {
                let code = if self.code.is_empty() { String::new() } else { empreinte(&self.code, &n) };
                self.l.profils.push(Profil { nom: n.clone(), emoji: self.emoji.clone(), code });
                garder(&self.l);
                self.message = format!("✔ Profil « {n} » créé : clique dessus pour l'ouvrir.");
                self.nom.clear();
                self.code.clear();
            }
        });
        ui.weak("Le code évite que les petits frères changent tes réglages ; ce n'est pas un coffre-fort (le Coffre de mots de passe a son propre mot de passe maître).");
        ui.label(&self.message);
    }
}

fn raccourci_bureau(nom: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bureau = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Desktop").join(format!("Outils - {nom}.lnk"));
    let e = |s: &str| s.replace('\'', "''");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Arguments='--profil \"{}\"';$s.IconLocation='{},0';$s.Save()",
        e(&bureau.display().to_string()),
        e(&exe.display().to_string()),
        e(nom),
        e(&exe.display().to_string())
    );
    crate::optimiseur::ps(&script).map(|_| ())
}

// ---------------------------------------------------------------------------------------------------
// 34 · Sauvegarde dans le cloud
// ---------------------------------------------------------------------------------------------------

/// Dossiers synchronisés trouvés sur le PC : (nom, chemin).
pub fn dossiers_cloud() -> Vec<(String, PathBuf)> {
    let mut v = Vec::new();
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(p) = std::env::var(var) {
            let p = PathBuf::from(p);
            if p.is_dir() && !v.iter().any(|(_, x)| *x == p) {
                v.push(("OneDrive".into(), p));
            }
        }
    }
    let u = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    for (n, p) in [("Dropbox", u.join("Dropbox")), ("Google Drive", u.join("Google Drive")), ("Google Drive", u.join("Mon Drive")), ("iCloud Drive", u.join("iCloudDrive")), ("pCloud", u.join("pCloudDrive"))] {
        if p.is_dir() {
            v.push((n.into(), p));
        }
    }
    // Google Drive pour ordinateur : un lecteur virtuel (G:\Mon Drive)
    for l in b'D'..=b'Z' {
        for d in ["Mon Drive", "My Drive"] {
            let p = PathBuf::from(format!("{}:\\{d}", l as char));
            if p.is_dir() {
                v.push(("Google Drive".into(), p));
            }
        }
    }
    v
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct ReglagesCloud {
    dossier: Option<PathBuf>,
    auto: bool,
    garder: usize,
    derniere: String, // date AAAA-MM-JJ de la dernière copie automatique
}

impl Default for ReglagesCloud {
    fn default() -> Self {
        ReglagesCloud { dossier: None, auto: true, garder: 10, derniere: String::new() }
    }
}

fn nom_pc() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

fn destination(d: &Path) -> PathBuf {
    d.join("Outils-sauvegardes").join(nom_pc())
}

/// Fait une copie datée et ne garde que les `garder` plus récentes.
fn sauvegarder(d: &Path, garder: usize) -> Result<String, String> {
    let dest = destination(d);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let f = dest.join(format!("Outils-{}.zip", crate::minecraft::horodatage_iso()));
    let base = PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
    let n = crate::coeur::exporter(&base, &f)?;
    let mut v = copies(d);
    while v.len() > garder.max(1) {
        let _ = std::fs::remove_file(v.remove(0));
    }
    Ok(format!("✔ {n} fichiers copiés dans {}", f.display()))
}

/// Copies existantes, de la plus ancienne à la plus récente.
fn copies(d: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(destination(d)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "zip")).collect();
    v.sort();
    v
}

pub struct SauvegardeCloud {
    r: ReglagesCloud,
    trouves: Vec<(String, PathBuf)>,
    message: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
    prochain: std::time::Instant,
}

impl SauvegardeCloud {
    pub fn new() -> Self {
        SauvegardeCloud { r: charger("sauvegarde_cloud"), trouves: Vec::new(), message: Arc::default(), en_cours: Arc::default(), prochain: std::time::Instant::now() + Duration::from_secs(120) }
    }

    fn lancer(&mut self) {
        let Some(d) = self.r.dossier.clone() else { return };
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (m, e, g) = (self.message.clone(), self.en_cours.clone(), self.r.garder);
        *m.lock().unwrap() = "Sauvegarde en cours…".into();
        std::thread::spawn(move || {
            *m.lock().unwrap() = sauvegarder(&d, g).unwrap_or_else(|x| format!("✖ {x}"));
            e.store(false, Relaxed);
        });
    }
}

impl Outil for SauvegardeCloud {
    fn nom(&self) -> &'static str {
        "☁ Sauvegarde dans le cloud"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if !self.r.auto || self.r.dossier.is_none() || std::time::Instant::now() < self.prochain {
            return;
        }
        self.prochain = std::time::Instant::now() + Duration::from_secs(1800);
        let jour = crate::minecraft::horodatage_iso().get(..10).unwrap_or_default().to_string();
        if self.r.derniere != jour {
            self.r.derniere = jour;
            enregistrer("sauvegarde_cloud", &self.r);
            self.lancer();
        }
    }
    fn resume(&self) -> Option<String> {
        self.r.dossier.as_ref().map(|_| format!("☁ Dernière sauvegarde cloud : {}", if self.r.derniere.is_empty() { "jamais" } else { &self.r.derniere }))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegarde dans le cloud");
        ui.weak("Chaque jour, Outils copie tous ses réglages (notes, tâches, coffre toujours chiffré, jeux, thèmes, profils…) dans ton dossier OneDrive, Google Drive ou Dropbox. Si le PC tombe en panne, tout est récupérable.");
        let avant = self.r.clone();
        if self.trouves.is_empty() {
            self.trouves = dossiers_cloud();
        }
        ui.horizontal_wrapped(|ui| {
            for (n, p) in self.trouves.clone() {
                let choisi = self.r.dossier.as_ref() == Some(&p);
                if ui.selectable_label(choisi, format!("☁ {n}")).on_hover_text(p.display().to_string()).clicked() {
                    self.r.dossier = Some(p);
                }
            }
            if ui.button("📁 Autre dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.r.dossier = Some(d);
                }
            }
        });
        if self.trouves.is_empty() {
            ui.weak("Aucun dossier cloud trouvé : installe OneDrive (déjà dans Windows), Google Drive pour ordinateur ou Dropbox, ou choisis un autre dossier (clé USB, disque externe).");
        }
        let Some(d) = self.r.dossier.clone() else { return };
        ui.monospace(destination(&d).display().to_string());
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.r.auto, "Tous les jours, tout seul");
            ui.label("Garder");
            ui.add(egui::DragValue::new(&mut self.r.garder).range(1..=60));
            ui.label("copies");
            if ui.add_enabled(!self.en_cours.load(Relaxed), egui::Button::new("☁ Sauvegarder maintenant")).clicked() {
                self.lancer();
            }
        });
        ui.label(self.message.lock().unwrap().clone());
        ui.separator();
        ui.strong("Restaurer");
        let v = copies(&d);
        if v.is_empty() {
            ui.weak("Pas encore de copie.");
        }
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for p in v.iter().rev() {
                ui.horizontal(|ui| {
                    ui.label(format!("📦 {}", p.file_stem().unwrap_or_default().to_string_lossy().trim_start_matches("Outils-")));
                    if ui.small_button("Restaurer").on_hover_text("L'état actuel est d'abord gardé à part, par sécurité").clicked() {
                        let base = PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
                        *self.message.lock().unwrap() = crate::coeur::importer(&base, p).map(|_| "✔ Restauré : redémarre Outils.".to_string()).unwrap_or_else(|e| format!("✖ {e}"));
                    }
                });
            }
        });
        // copies des autres PC (même compte cloud)
        let autres: Vec<PathBuf> = std::fs::read_dir(d.join("Outils-sauvegardes")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy() != nom_pc())).collect();
        if !autres.is_empty() {
            ui.weak(format!("Autres PC dans ce cloud : {}", autres.iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>().join(", ")));
        }
        if self.r != avant {
            enregistrer("sauvegarde_cloud", &self.r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profils() {
        assert_eq!(nom_sur("  Léo/../x  "), "Léox");
        assert_eq!(nom_sur("Marie-Ève 2"), "Marie-Ève 2");
        let p = Profil { nom: "Léo".into(), emoji: "🦊".into(), code: empreinte("1234", "Léo") };
        assert!(verifier_code(&p, "1234") && !verifier_code(&p, "0000"));
        assert_ne!(empreinte("1234", "Léo"), empreinte("1234", "Zoé"));
        assert!(verifier_code(&Profil::default(), "n'importe"));
    }
}
