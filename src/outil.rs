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
    /// Ligne pour l'Accueil (None : rien à montrer).
    fn resume(&self) -> Option<String> {
        None
    }
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

/// Jeu en cours (mis à jour par la fenêtre principale à chaque image), pour les outils qui en dépendent.
pub static JEU_ACTIF: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn jeu_actif() -> Option<String> {
    JEU_ACTIF.lock().ok().and_then(|j| j.clone())
}

/// Mode stream actif : les onglets sensibles sont masqués.
pub static STREAM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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

/// Fichiers de réglages remplacés par une synchronisation (PC ↔ téléphone) : l'outil les relit.
static A_RECHARGER: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn recharger_synchro(nom: &str) {
    if let Ok(mut v) = A_RECHARGER.lock() {
        v.push(nom.to_string());
    }
}

/// Vrai (une fois) si le fichier `nom` vient d'être remplacé.
pub fn a_recharger(nom: &str) -> bool {
    let Ok(mut v) = A_RECHARGER.lock() else { return false };
    let avant = v.len();
    v.retain(|x| x != nom);
    v.len() != avant
}

/// Mode sourdine : un jeu tourne et Outils n'est pas au premier plan → les tâches de fond lourdes attendent.
pub static DISCRET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Raccourcis clavier qu'une autre appli a déjà pris (modificateurs, touche).
pub static RACCOURCIS_PRIS: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

/// RegisterHotKey qui note les raccourcis refusés (pour « Santé d'Outils »).
///
/// # Safety
/// Mêmes conditions que RegisterHotKey.
#[cfg(windows)]
pub unsafe fn raccourci(h: windows_sys::Win32::Foundation::HWND, id: i32, mods: u32, vk: u32) -> i32 {
    let ok = windows_sys::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey(h, id, mods, vk);
    let cle = (mods & 0x0F, vk);
    if let Ok(mut v) = RACCOURCIS_PRIS.lock() {
        v.retain(|x| *x != cle);
        if ok == 0 && windows_sys::Win32::Foundation::GetLastError() == 1409 {
            v.push(cle); // ERROR_HOTKEY_ALREADY_REGISTERED
        }
    }
    ok
}
