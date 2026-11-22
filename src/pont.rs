//! Pont avancé PC ↔ appli Android.
//! - Souris, clavier et manette (13, 14) : paquets UDP « clé|commande » sur le port 8644, transformés en
//!   vraies touches (codes de position : ZQSD sur AZERTY = WASD sur QWERTY) et mouvements de souris.
//! - Micro du téléphone (15) : son reçu en UDP sur le port 8645 et joué dans le câble virtuel (VB-CABLE),
//!   que Discord, OBS ou un jeu prennent comme micro. Caméra : images JPEG reçues par l'API.
//! - Commandes pour le téléphone (18, 19) : fichiers à lui envoyer, faire sonner.
//! - Accès de l'extérieur (20) : relais ntfy.sh, messages chiffrés (AES-256-GCM) avec une clé partagée.
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT_ENTREES: u16 = 8644;
pub const PORT_SON: u16 = 8645;

// ===================================================================================================
// Commandes de souris et de clavier
// ===================================================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Entree {
    Deplacer(i32, i32),
    Absolu(f32, f32),
    Bouton(u8, bool), // 0 gauche, 1 droit, 2 milieu ; appuyé ?
    Clic(u8),
    Molette(i32),
    Texte(String),
    Touche(u16, bool), // code de position (scan code), appuyée ?
    ToucheTap(u16),
}

/// Code de position (scan code) d'une touche nommée.
pub fn scan(nom: &str) -> Option<u16> {
    Some(match nom {
        "haut" => 0xE048,
        "bas" => 0xE050,
        "gauche" => 0xE04B,
        "droite" => 0xE04D,
        "espace" => 0x39,
        "entree" => 0x1C,
        "echap" => 0x01,
        "tab" => 0x0F,
        "retour" => 0x0E,
        "maj" => 0x2A,
        "ctrl" => 0x1D,
        "alt" => 0x38,
        "win" => 0xE05B,
        "suppr" => 0xE053,
        // lettres et chiffres par position (QWERTY) : « w » est la touche Z d'un clavier AZERTY
        n if n.len() == 1 => {
            let c = n.chars().next()?;
            let ligne1 = "qwertyuiop";
            let ligne2 = "asdfghjkl";
            let ligne3 = "zxcvbnm";
            if let Some(k) = ligne1.find(c) {
                0x10 + k as u16
            } else if let Some(k) = ligne2.find(c) {
                0x1E + k as u16
            } else if let Some(k) = ligne3.find(c) {
                0x2C + k as u16
            } else if c.is_ascii_digit() {
                if c == '0' { 0x0B } else { 0x02 + (c as u16 - '1' as u16) }
            } else {
                return None;
            }
        }
        n if n.starts_with('f') => {
            let k: u16 = n[1..].parse().ok()?;
            match k {
                1..=10 => 0x3A + k,
                11 => 0x57,
                12 => 0x58,
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Lit « m 5 -3 », « k espace down », « t bonjour »…
pub fn lire_entree(c: &str) -> Option<Entree> {
    let (cmd, reste) = c.split_once(' ').unwrap_or((c, ""));
    let mut p = reste.split_whitespace();
    let bouton = |s: Option<&str>| match s {
        Some("d") => 1,
        Some("m") => 2,
        _ => 0,
    };
    Some(match cmd {
        "m" => Entree::Deplacer(p.next()?.parse().ok()?, p.next()?.parse().ok()?),
        "a" => Entree::Absolu(p.next()?.parse().ok()?, p.next()?.parse().ok()?),
        "c" => Entree::Clic(bouton(p.next())),
        "b" => Entree::Bouton(bouton(p.next()), p.next()? == "down"),
        "w" => Entree::Molette(p.next()?.parse().ok()?),
        "t" => Entree::Texte(reste.to_string()),
        "k" => {
            let sc = scan(p.next()?)?;
            match p.next() {
                Some("down") => Entree::Touche(sc, true),
                Some("up") => Entree::Touche(sc, false),
                _ => Entree::ToucheTap(sc),
            }
        }
        _ => return None,
    })
}

#[cfg(windows)]
pub fn executer(e: &Entree) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let souris = |dx: i32, dy: i32, data: i32, f: u32| {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_MOUSE;
        i.Anonymous.mi = MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: f, time: 0, dwExtraInfo: 0 };
        i
    };
    let clavier = |sc: u16, unicode: u16, haut: bool| {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_KEYBOARD;
        let mut f = if haut { KEYEVENTF_KEYUP } else { 0 };
        let (vk, scan) = if unicode != 0 {
            f |= KEYEVENTF_UNICODE;
            (0, unicode)
        } else {
            f |= KEYEVENTF_SCANCODE;
            if sc & 0xE000 == 0xE000 {
                f |= KEYEVENTF_EXTENDEDKEY;
            }
            (0, sc & 0xFF)
        };
        i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: f, time: 0, dwExtraInfo: 0 };
        i
    };
    let envoyer = |v: &[INPUT]| unsafe {
        SendInput(v.len() as u32, v.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    };
    let boutons = |b: u8| -> (u32, u32) {
        match b {
            1 => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            2 => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        }
    };
    match e {
        Entree::Deplacer(dx, dy) => envoyer(&[souris(*dx, *dy, 0, MOUSEEVENTF_MOVE)]),
        Entree::Absolu(x, y) => {
            // écran principal : 0..65535
            let (ax, ay) = ((x.clamp(0.0, 1.0) * 65535.0) as i32, (y.clamp(0.0, 1.0) * 65535.0) as i32);
            envoyer(&[souris(ax, ay, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE)]);
        }
        Entree::Bouton(b, appuye) => {
            let (d, u) = boutons(*b);
            envoyer(&[souris(0, 0, 0, if *appuye { d } else { u })]);
        }
        Entree::Clic(b) => {
            let (d, u) = boutons(*b);
            envoyer(&[souris(0, 0, 0, d), souris(0, 0, 0, u)]);
        }
        Entree::Molette(n) => envoyer(&[souris(0, 0, n * 120, MOUSEEVENTF_WHEEL)]),
        Entree::Texte(t) => {
            let v: Vec<INPUT> = t.encode_utf16().flat_map(|u| [clavier(0, u, false), clavier(0, u, true)]).collect();
            envoyer(&v);
        }
        Entree::Touche(sc, appuyee) => envoyer(&[clavier(*sc, 0, !appuyee)]),
        Entree::ToucheTap(sc) => envoyer(&[clavier(*sc, 0, false), clavier(*sc, 0, true)]),
    }
}

/// Reçoit les commandes de souris et de clavier du téléphone.
pub fn ecouter_entrees(cle: Arc<Mutex<String>>, marche: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_ENTREES)) else { return };
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut b = [0u8; 2048];
        // touches restées appuyées si le téléphone se déconnecte : relâchées après 3 s sans nouvelles
        let mut appuyees: Vec<u16> = vec![];
        let mut dernier = Instant::now();
        while marche.load(Relaxed) {
            match s.recv_from(&mut b) {
                Ok((n, _)) => {
                    let t = String::from_utf8_lossy(&b[..n]);
                    let Some((k, cmds)) = t.split_once('|') else { continue };
                    if k != cle.lock().unwrap().as_str() {
                        continue;
                    }
                    dernier = Instant::now();
                    for c in cmds.split('\n') {
                        if let Some(e) = lire_entree(c) {
                            match &e {
                                Entree::Touche(sc, true) if !appuyees.contains(sc) => appuyees.push(*sc),
                                Entree::Touche(sc, false) => appuyees.retain(|x| x != sc),
                                _ => {}
                            }
                            #[cfg(windows)]
                            executer(&e);
                        }
                    }
                }
                Err(_) => {
                    if !appuyees.is_empty() && dernier.elapsed() > Duration::from_secs(3) {
                        for sc in appuyees.drain(..) {
                            #[cfg(windows)]
                            executer(&Entree::Touche(sc, false));
                            let _ = sc;
                        }
                    }
                }
            }
        }
    });
}

// ===================================================================================================
// Micro et caméra du téléphone
// ===================================================================================================

/// Dernière image de la caméra du téléphone (JPEG) et son heure d'arrivée.
pub static WEBCAM: Mutex<Option<(Vec<u8>, Instant)>> = Mutex::new(None);
/// Le son du micro du téléphone arrive-t-il ?
pub static MICRO_ACTIF: Mutex<Option<Instant>> = Mutex::new(None);
/// Sortie choisie pour le son du téléphone (vide = câble virtuel s'il existe).
pub static SORTIE_MICRO: Mutex<String> = Mutex::new(String::new());

/// Son du micro du téléphone : paquets « clé|» puis échantillons 16 bits, 16 kHz, mono.
pub fn ecouter_son(cle: Arc<Mutex<String>>, marche: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_SON)) else { return };
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut b = [0u8; 4096];
        let mut lecteur: Option<(rodio::OutputStream, rodio::Sink)> = None;
        while marche.load(Relaxed) {
            let Ok((n, _)) = s.recv_from(&mut b) else {
                // plus de son : on libère la sortie
                if MICRO_ACTIF.lock().unwrap().is_some_and(|t| t.elapsed() > Duration::from_secs(3)) {
                    *MICRO_ACTIF.lock().unwrap() = None;
                    lecteur = None;
                }
                continue;
            };
            let k = cle.lock().unwrap().clone();
            if n <= k.len() + 1 || &b[..k.len()] != k.as_bytes() || b[k.len()] != b'|' {
                continue;
            }
            *MICRO_ACTIF.lock().unwrap() = Some(Instant::now());
            if lecteur.is_none() {
                lecteur = ouvrir_sortie();
            }
            if let Some((_, sink)) = &lecteur {
                let ech: Vec<i16> = b[k.len() + 1..n].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
                // pas plus de 300 ms de retard
                if sink.len() < 6 {
                    sink.append(rodio::buffer::SamplesBuffer::new(1, 16_000, ech));
                }
            }
        }
    });
}

fn ouvrir_sortie() -> Option<(rodio::OutputStream, rodio::Sink)> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};
    let voulu = SORTIE_MICRO.lock().unwrap().clone();
    let hote = rodio::cpal::default_host();
    let dev = hote.output_devices().ok()?.find(|d| {
        let n = d.name().unwrap_or_default();
        if voulu.is_empty() { n.to_lowercase().contains("cable input") } else { n == voulu }
    });
    let (flux, h) = match dev {
        Some(d) => rodio::OutputStream::try_from_device(&d).ok()?,
        None => rodio::OutputStream::try_default().ok()?,
    };
    let sink = rodio::Sink::try_new(&h).ok()?;
    Some((flux, sink))
}

// ===================================================================================================
// Commandes en attente pour le téléphone (fichiers à recevoir, sonner)
// ===================================================================================================

pub static POUR_TEL: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

pub fn envoyer_au_tel(chemin: &std::path::Path) {
    let t = chemin.metadata().map(|m| m.len()).unwrap_or(0);
    POUR_TEL.lock().unwrap().push(serde_json::json!({ "type": "fichier", "chemin": chemin.display().to_string(), "nom": chemin.file_name().unwrap_or_default().to_string_lossy(), "taille": t }));
}

pub fn faire_sonner() {
    POUR_TEL.lock().unwrap().push(serde_json::json!({ "type": "sonner" }));
}

// ===================================================================================================
// Accès de l'extérieur par ntfy.sh (messages chiffrés)
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Distance {
    pub actif: bool,
    pub sujet: String, // nom secret du canal
    pub cle: String,   // clé AES-256 en base64
}

impl Distance {
    pub fn nouvelle() -> Distance {
        let mut s = [0u8; 18];
        let mut k = [0u8; 32];
        let _ = getrandom::getrandom(&mut s);
        let _ = getrandom::getrandom(&mut k);
        let sujet: String = s.iter().map(|x| format!("{x:02x}")).collect();
        Distance { actif: false, sujet: format!("outils-{sujet}"), cle: B64.encode(k) }
    }
}

pub fn chiffrer(cle_b64: &str, texte: &str) -> Option<String> {
    let k = B64.decode(cle_b64).ok()?;
    let c = Aes256Gcm::new_from_slice(&k).ok()?;
    let mut n = [0u8; 12];
    let _ = getrandom::getrandom(&mut n);
    let mut v = n.to_vec();
    v.extend(c.encrypt(Nonce::from_slice(&n), texte.as_bytes()).ok()?);
    Some(B64.encode(v))
}

pub fn dechiffrer(cle_b64: &str, b64: &str) -> Option<String> {
    let k = B64.decode(cle_b64).ok()?;
    let b = B64.decode(b64.trim()).ok()?;
    if b.len() < 28 {
        return None;
    }
    let c = Aes256Gcm::new_from_slice(&k).ok()?;
    String::from_utf8(c.decrypt(Nonce::from_slice(&b[..12]), &b[12..]).ok()?).ok()
}

static DISTANCE: std::sync::OnceLock<Mutex<Distance>> = std::sync::OnceLock::new();

/// Réglages de l'accès à distance (partagés par le serveur et l'interface).
pub fn distance() -> &'static Mutex<Distance> {
    DISTANCE.get_or_init(|| Mutex::new(Distance::default()))
}

/// Écoute les commandes envoyées de l'extérieur et y répond (état, actions).
/// `agir` exécute une action (« vol_plus », « mc_demarrer »…) et renvoie un texte.
pub fn ecouter_distance(agir: Arc<dyn Fn(&str) -> String + Send + Sync>) {
    static LANCE: AtomicBool = AtomicBool::new(false);
    if LANCE.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let mut depuis = "all".to_string();
        let mut premier = true;
        loop {
            let x = distance().lock().unwrap().clone();
            if !x.actif || x.sujet.is_empty() {
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
            let url = format!("https://ntfy.sh/{}-pc/json?poll=1&since={}", x.sujet, if premier { "10s".to_string() } else { depuis.clone() });
            premier = false;
            if let Ok(r) = ureq::get(&url).timeout(Duration::from_secs(30)).call() {
                let corps = r.into_string().unwrap_or_default();
                for l in corps.lines() {
                    let Ok(m) = serde_json::from_str::<serde_json::Value>(l) else { continue };
                    if let Some(id) = m["id"].as_str() {
                        depuis = id.to_string();
                    }
                    let Some(cmd) = m["message"].as_str().and_then(|t| dechiffrer(&x.cle, t)) else { continue };
                    let reponse = agir(&cmd);
                    if let Some(c) = chiffrer(&x.cle, &reponse) {
                        let _ = ureq::post(&format!("https://ntfy.sh/{}-tel", x.sujet)).timeout(Duration::from_secs(15)).send_string(&c);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(8));
        }
    });
}

/// Notification du PC vers le téléphone quand il n'est pas à la maison.
pub fn notifier_distance(titre: &str, texte: &str) {
    let d = distance().lock().unwrap().clone();
    if !d.actif {
        return;
    }
    let m = serde_json::json!({ "notif": titre, "texte": texte }).to_string();
    if let Some(c) = chiffrer(&d.cle, &m) {
        let sujet = d.sujet.clone();
        std::thread::spawn(move || {
            let _ = ureq::post(&format!("https://ntfy.sh/{sujet}-tel")).timeout(Duration::from_secs(15)).send_string(&c);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrees() {
        assert_eq!(lire_entree("m 5 -3"), Some(Entree::Deplacer(5, -3)));
        assert_eq!(lire_entree("c d"), Some(Entree::Clic(1)));
        assert_eq!(lire_entree("b g down"), Some(Entree::Bouton(0, true)));
        assert_eq!(lire_entree("w -2"), Some(Entree::Molette(-2)));
        assert_eq!(lire_entree("t salut à toi"), Some(Entree::Texte("salut à toi".into())));
        assert_eq!(lire_entree("k espace down"), Some(Entree::Touche(0x39, true)));
        assert_eq!(lire_entree("k w"), Some(Entree::ToucheTap(0x11))); // Z sur un clavier AZERTY
        assert_eq!(scan("a"), Some(0x1E));
        assert_eq!(scan("m"), Some(0x32));
        assert_eq!(scan("1"), Some(0x02));
        assert_eq!(scan("0"), Some(0x0B));
        assert_eq!(scan("f12"), Some(0x58));
        assert_eq!(lire_entree("k inconnue"), None);
        assert_eq!(lire_entree("a 0.5 0.25"), Some(Entree::Absolu(0.5, 0.25)));
    }

    #[test]
    fn chiffrement() {
        let d = Distance::nouvelle();
        let c = chiffrer(&d.cle, "vol_plus").unwrap();
        assert_eq!(dechiffrer(&d.cle, &c).as_deref(), Some("vol_plus"));
        let autre = Distance::nouvelle();
        assert!(dechiffrer(&autre.cle, &c).is_none());
        assert!(d.sujet.starts_with("outils-") && d.sujet.len() > 30);
    }
}
