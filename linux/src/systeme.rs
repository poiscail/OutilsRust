//! Notifications du bureau (notify-send) et bandeau dans la fenêtre.
use std::sync::Mutex;

pub static NOTIFICATIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn notifier_toujours(titre: &str, texte: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "Outils", titre, texte]).spawn();
    if let Ok(mut n) = NOTIFICATIONS.lock() {
        n.push((titre.to_string(), texte.to_string()));
    }
}

pub fn notifier(titre: &str, texte: &str) {
    notifier_toujours(titre, texte);
}
