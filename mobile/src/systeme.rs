//! Notifications, version téléphone : un bandeau en haut de l'appli.
use std::sync::Mutex;

pub static NOTIFICATIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Bandeau dans l'appli et vraie notification Android (agenda, devoirs, eau…).
pub fn notifier_toujours(titre: &str, texte: &str) {
    if !titre.starts_with("🖥") && !titre.starts_with("🏅") {
        let id = titre.bytes().chain(texte.bytes()).fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 1000;
        crate::android::notifier(3000 + id as i32, titre, texte);
    }
    if let Ok(mut n) = NOTIFICATIONS.lock() {
        n.push((titre.to_string(), texte.to_string()));
    }
}
