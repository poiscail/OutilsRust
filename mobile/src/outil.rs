//! Socle des outils, version téléphone : mêmes catégories et même trait que sur PC, réglages dans
//! le dossier privé de l'appli.
use eframe::egui;
use serde::{de::DeserializeOwned, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Cat {
    Jeux,
    Minecraft,
    Fichiers,
    Systeme,
    Reseau,
    Quotidien,
    Creatif,
    Fun,
}

pub trait Outil {
    fn nom(&self) -> &'static str;
    fn cat(&self) -> Cat;
    fn ui(&mut self, ui: &mut egui::Ui);
    fn tic(&mut self, _ctx: &egui::Context) {}
    fn resume(&self) -> Option<String> {
        None
    }
}

/// Dossier privé de l'appli (donné par Android au démarrage).
pub static DOSSIER: OnceLock<PathBuf> = OnceLock::new();

pub fn chemin(nom: &str) -> PathBuf {
    DOSSIER.get().cloned().unwrap_or_else(|| PathBuf::from(".")).join(format!("{nom}.json"))
}

pub fn charger<T: DeserializeOwned + Default>(nom: &str) -> T {
    std::fs::read_to_string(chemin(nom)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn enregistrer<T: Serialize>(nom: &str, v: &T) {
    let f = chemin(nom);
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    if let Ok(s) = serde_json::to_string_pretty(v) {
        let _ = std::fs::write(f, s);
    }
}

pub fn jeu_actif() -> Option<String> {
    None
}

pub fn hasard(n: u64) -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    if n == 0 { 0 } else { u64::from_le_bytes(b) % n }
}
