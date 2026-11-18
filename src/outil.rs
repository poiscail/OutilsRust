//! Socle commun des outils ajoutés en lot : catégorie du menu, interface, tic par image, réglages JSON.
use eframe::egui;
use serde::{de::DeserializeOwned, Serialize};
use std::path::PathBuf;

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

impl Cat {
    pub const TOUTES: [Cat; 8] = [Cat::Jeux, Cat::Minecraft, Cat::Fichiers, Cat::Systeme, Cat::Reseau, Cat::Quotidien, Cat::Creatif, Cat::Fun];

    pub fn nom(self) -> &'static str {
        match self {
            Cat::Jeux => "🎮 Jeux",
            Cat::Minecraft => "⛏ Minecraft",
            Cat::Fichiers => "🗂 Fichiers",
            Cat::Systeme => "🖥 Système",
            Cat::Reseau => "🌐 Réseau",
            Cat::Quotidien => "📅 Quotidien",
            Cat::Creatif => "🎨 Créatif et son",
            Cat::Fun => "✨ Apparence et fun",
        }
    }
}

pub trait Outil {
    fn nom(&self) -> &'static str;
    fn cat(&self) -> Cat;
    fn ui(&mut self, ui: &mut egui::Ui);
    /// Appelé à chaque image, même hors de l'onglet.
    fn tic(&mut self, _ctx: &egui::Context) {}
}

pub fn chemin(nom: &str) -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join(format!("{nom}.json"))
}

/// Réglages enregistrés (ou valeurs par défaut).
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

/// Nombre aléatoire 0..n (cryptographique, suffisant pour tout).
pub fn hasard(n: u64) -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    if n == 0 { 0 } else { u64::from_le_bytes(b) % n }
}

/// Ouvre un dossier ou un fichier dans l'explorateur.
pub fn ouvrir(p: &std::path::Path) {
    let _ = std::process::Command::new("explorer").arg(p).spawn();
}
