//! Socle des outils, version Linux : mêmes catégories et même trait que sous Windows, réglages dans
//! ~/.config/outils.
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
            Cat::Fun => "✨ Jeux et défis",
        }
    }
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

static DOSSIER: OnceLock<PathBuf> = OnceLock::new();

/// ~/.config/outils (ou $XDG_CONFIG_HOME/outils).
pub fn dossier() -> PathBuf {
    DOSSIER
        .get_or_init(|| {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
                .unwrap_or_else(|| PathBuf::from("."))
                .join("outils")
        })
        .clone()
}

/// Dossier commun (programmes téléchargés).
pub fn racine() -> PathBuf {
    dossier()
}

pub fn chemin(nom: &str) -> PathBuf {
    dossier().join(format!("{nom}.json"))
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

pub static JEU_ACTIF: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn jeu_actif() -> Option<String> {
    None
}

pub static DISCRET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn hasard(n: u64) -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::getrandom(&mut b);
    if n == 0 { 0 } else { u64::from_le_bytes(b) % n }
}

/// Ouvre un fichier, un dossier ou une adresse avec l'appli par défaut.
pub fn ouvrir(p: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(p).spawn();
}

static A_RECHARGER: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn recharger_synchro(nom: &str) {
    if let Ok(mut v) = A_RECHARGER.lock() {
        v.push(nom.to_string());
    }
}

pub fn a_recharger(nom: &str) -> bool {
    let Ok(mut v) = A_RECHARGER.lock() else { return false };
    let avant = v.len();
    v.retain(|x| x != nom);
    v.len() != avant
}

/// Outil à ouvrir (par son nom) : un outil peut passer la main à un autre.
pub static OUVRIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
