//! Notifications, version téléphone : un bandeau en haut de l'appli.
use std::sync::Mutex;

pub static NOTIFICATIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn notifier_toujours(titre: &str, texte: &str) {
    if let Ok(mut n) = NOTIFICATIONS.lock() {
        n.push((titre.to_string(), texte.to_string()));
    }
}
