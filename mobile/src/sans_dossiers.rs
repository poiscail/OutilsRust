//! Version téléphone des quelques fonctions de fichiers dont le donjon du ménage a besoin
//! (sur PC, elles sont dans « Fichiers sans dossiers »).
use crate::outil::{Cat, Outil};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

fn stockage() -> PathBuf {
    PathBuf::from(crate::android::STOCKAGE)
}

/// Dossiers du téléphone ; le premier est celui des téléchargements.
pub fn dossiers_perso() -> Vec<(&'static str, &'static [&'static str], PathBuf)> {
    let s = stockage();
    vec![
        ("Téléchargements", &["telechargements", "download"][..], s.join("Download")),
        ("Photos", &["photos", "dcim"][..], s.join("DCIM")),
        ("Images", &["images"][..], s.join("Pictures")),
        ("Vidéos", &["videos"][..], s.join("Movies")),
        ("Musique", &["musique"][..], s.join("Music")),
        ("Documents", &["documents"][..], s.join("Documents")),
        ("WhatsApp", &["whatsapp"][..], s.join("Android/media/com.whatsapp/WhatsApp/Media")),
    ]
}

/// Corbeille d'Outils (restaurable 30 jours).
pub fn corbeille(p: &Path) -> bool {
    crate::commun::mettre_a_la_corbeille(p).is_ok()
}

pub fn maintenant() -> i64 {
    crate::commun::maintenant() as i64
}

pub fn taille_lisible(o: u64) -> String {
    crate::commun::taille(o)
}

const IGNORES: [&str; 4] = ["cache", ".thumbnails", "thumbnails", ".trash"];

/// Tous les fichiers sous `racines` : (chemin, taille, modification).
pub fn parcourir(racines: &[PathBuf], stop: &AtomicBool, max: usize) -> Vec<(PathBuf, u64, i64)> {
    let mut v = Vec::new();
    let mut pile: Vec<PathBuf> = racines.to_vec();
    while let Some(d) = pile.pop() {
        if stop.load(Relaxed) || v.len() >= max {
            break;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let nom = e.file_name().to_string_lossy().to_lowercase();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            if t.is_dir() {
                if !nom.starts_with('.') && !IGNORES.contains(&nom.as_str()) {
                    pile.push(e.path());
                }
            } else if !nom.starts_with(".nomedia") {
                let m = e.metadata().ok();
                let taille = m.as_ref().map(|m| m.len()).unwrap_or(0);
                let modif = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                v.push((e.path(), taille, modif));
            }
        }
    }
    v
}

#[derive(Default)]
pub struct Index {
    pub racines: Vec<PathBuf>,
}

pub fn index() -> Arc<Mutex<Index>> {
    Arc::new(Mutex::new(Index { racines: dossiers_perso().into_iter().map(|d| d.2).filter(|p| p.is_dir()).collect() }))
}

/// Le donjon, après avoir demandé l'accès aux fichiers.
pub struct DonjonTel(pub crate::donjon::Donjon);

impl Outil for DonjonTel {
    fn nom(&self) -> &'static str {
        self.0.nom()
    }
    fn cat(&self) -> Cat {
        self.0.cat()
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if crate::commun::acces_fichiers_ui(ui) {
            self.0.ui(ui);
        }
    }
}
