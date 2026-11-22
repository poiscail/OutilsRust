//! Optimiser le Wi-Fi et le réseau mobile.
//! « Ma box » : diagnostic noté (5), canaux des voisins et meilleur canal (1), carte du signal par pièce (3),
//! accès et guide de la box (2), test des DNS (4, 11), surveillance des intrus (6), partage par QR code (7).
//! « Réseau mobile » : signal 4G/5G en direct (8), consommation par appli et forfait (9),
//! comparateur Wi-Fi / données mobiles (10), économie de données (12), historique des coupures (13).
use crate::android::{self, ReseauVoisin};
use crate::commun::*;
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::wifi_tel::{deviner, ping_tcp, qualite, sonder};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ===================================================================================================
// Calculs (testés)
// ===================================================================================================

/// Numéro de canal d'une fréquence Wi-Fi (MHz).
pub fn canal(f: i32) -> i32 {
    match f {
        2484 => 14,
        2400..=2483 => (f - 2407) / 5,
        5000..=5899 => (f - 5000) / 5,
        5925..=7125 => (f - 5950) / 5,
        _ => 0,
    }
}

pub const CANAUX_24: [i32; 3] = [1, 6, 11];
pub const CANAUX_5: [i32; 13] = [36, 40, 44, 48, 52, 56, 60, 64, 100, 116, 132, 149, 157];

/// Canaux « radar » (DFS) : la box doit parfois en changer, ce qui coupe le Wi-Fi quelques secondes.
pub fn dfs(c: i32) -> bool {
    (52..=144).contains(&c)
}

/// Gêne causée par les voisins sur chaque canal possible (plus c'est bas, mieux c'est).
/// Les réseaux de `mon_ssid` ne comptent pas.
pub fn encombrement(voisins: &[ReseauVoisin], mon_ssid: &str, cinq: bool) -> Vec<(i32, f32)> {
    let candidats: Vec<i32> = if cinq { CANAUX_5.to_vec() } else { CANAUX_24.to_vec() };
    candidats.into_iter().map(|c| {
        let mut g = 0.0;
        for v in voisins.iter().filter(|v| v.ssid != mon_ssid && (v.frequence >= 5000) == cinq && v.frequence > 0) {
            let cv = canal(v.frequence);
            // en 2,4 GHz un réseau déborde sur ±4 canaux ; en 5 GHz il occupe tout son bloc de 40/80/160 MHz
            let gene = if cinq { meme_bloc(cv, c, v.largeur) } else { (cv - c).abs() < 5 };
            if gene {
                g += ((v.rssi + 100).max(0) as f32 / 10.0).powi(2);
            }
        }
        if cinq && dfs(c) {
            g += 1.5;
        }
        (c, g)
    }).collect()
}

/// Deux canaux 5 GHz dans le même bloc de `largeur` MHz ?
pub fn meme_bloc(a: i32, b: i32, largeur: i32) -> bool {
    let base = |c: i32| if c >= 149 { 149 } else if c >= 100 { 100 } else { 36 };
    if base(a) != base(b) {
        return false;
    }
    let taille = (largeur / 5).max(4); // 20 MHz = 4 numéros de canal
    (a - base(a)) / taille == (b - base(b)) / taille
}

pub fn meilleur_canal(voisins: &[ReseauVoisin], mon_ssid: &str, cinq: bool) -> i32 {
    encombrement(voisins, mon_ssid, cinq).into_iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).map(|x| x.0).unwrap_or(if cinq { 36 } else { 1 })
}

/// Requête DNS (type A) pour `nom`.
pub fn requete_dns(id: u16, nom: &str) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    b.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for l in nom.trim_end_matches('.').split('.') {
        b.push(l.len() as u8);
        b.extend_from_slice(l.as_bytes());
    }
    b.extend_from_slice(&[0, 0, 1, 0, 1]);
    b
}

/// Temps de réponse d'un serveur DNS (ms).
pub fn temps_dns(serveur: &str, nom: &str) -> Option<u128> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let id = crate::outil::hasard(65_000) as u16;
    let t = Instant::now();
    s.send_to(&requete_dns(id, nom), (serveur, 53)).ok()?;
    let mut b = [0u8; 512];
    let (n, _) = s.recv_from(&mut b).ok()?;
    (n >= 4 && b[..2] == id.to_be_bytes() && b[2] & 0x80 != 0).then(|| t.elapsed().as_millis())
}

/// (nom, adresse, DNS privé pour Android, bloque les pubs ?)
pub const DNS: [(&str, &str, &str, bool); 5] = [
    ("Cloudflare", "1.1.1.1", "one.one.one.one", false),
    ("Google", "8.8.8.8", "dns.google", false),
    ("Quad9 (bloque les sites dangereux)", "9.9.9.9", "dns.quad9.net", false),
    ("AdGuard (bloque les pubs)", "94.140.14.14", "dns.adguard-dns.com", true),
    ("Ta box", "", "", false),
];

/// Médiane sur quelques sites connus.
pub fn mesurer_dns(serveur: &str) -> Option<u128> {
    let _ = temps_dns(serveur, "example.com"); // premier passage (mise en cache)
    let mut v: Vec<u128> = ["www.google.com", "www.youtube.com", "discord.com", "www.wikipedia.org", "store.steampowered.com"].iter().filter_map(|n| temps_dns(serveur, n)).collect();
    v.sort();
    v.get(v.len() / 2).copied()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesures {
    pub rssi: i32,
    pub frequence: i32,
    pub voisins_meme_canal: usize,
    pub debit: f64,  // Mb/s
    pub ping: f64,   // ms
    pub gigue: f64,  // ms
    pub perte: f64,  // %
    pub dns: f64,    // ms
}

/// Note sur 100 et conseils, du plus important au moins important.
pub fn diagnostic(m: &Mesures) -> (u32, Vec<String>) {
    let mut note: i32 = 100;
    let mut c: Vec<(i32, String)> = vec![];
    let mut retirer = |pts: i32, conseil: String| {
        note -= pts;
        c.push((pts, conseil));
    };
    match m.rssi {
        r if r >= -60 => {}
        r if r >= -70 => retirer(10, "Signal moyen : rapproche-toi de la box ou dégage-la (pas dans un meuble, pas derrière la TV).".into()),
        r if r >= -80 => retirer(25, "Signal faible : un répéteur, un système « mesh » ou un CPL Wi-Fi aiderait beaucoup ici.".into()),
        _ => retirer(40, "Signal très faible : cette pièce est une zone morte. Place un répéteur à mi-chemin de la box.".into()),
    }
    if m.frequence > 0 && m.frequence < 5000 && m.rssi > -65 {
        retirer(8, "Tu es en 2,4 GHz alors que le signal est bon : passe sur le 5 GHz de ta box (souvent 2 à 5 fois plus rapide).".into());
    }
    if m.voisins_meme_canal >= 3 {
        retirer(12, format!("{} réseaux voisins sur ton canal : change de canal dans les réglages de la box (onglet « Canaux »).", m.voisins_meme_canal));
    } else if m.voisins_meme_canal >= 1 {
        retirer(4, "Quelques voisins partagent ton canal : regarde l'onglet « Canaux » pour en choisir un plus calme.".into());
    }
    match m.debit {
        d if d <= 0.0 => {}
        d if d < 5.0 => retirer(20, "Débit très bas : redémarre la box ; si ça dure, appelle ton opérateur (garde l'historique des tests).".into()),
        d if d < 25.0 => retirer(10, "Débit modeste : évite les gros téléchargements pendant que tu joues ou regardes une vidéo.".into()),
        _ => {}
    }
    if m.ping > 80.0 {
        retirer(10, "Ping élevé : pour jouer, branche le PC en câble ou en CPL, et ferme les téléchargements.".into());
    }
    if m.gigue > 20.0 {
        retirer(8, "Ping instable (gigue) : souvent des interférences Wi-Fi ou un appareil qui télécharge en même temps.".into());
    }
    if m.perte > 1.0 {
        retirer(15, format!("{:.0} % de paquets perdus : connexion instable, redémarre la box et vérifie les câbles.", m.perte));
    }
    if m.dns > 60.0 {
        retirer(5, "Le DNS est lent : essaie Cloudflare ou AdGuard (onglet « DNS »).".into());
    }
    c.sort_by_key(|x| std::cmp::Reverse(x.0));
    if c.is_empty() {
        c.push((0, "Rien à redire : ton Wi-Fi est en pleine forme ✔".into()));
    }
    (note.clamp(0, 100) as u32, c.into_iter().map(|x| x.1).collect())
}

/// Texte d'un QR code Wi-Fi (format reconnu par les appareils photo des téléphones).
pub fn qr_wifi(ssid: &str, mdp: &str) -> String {
    let e = |s: &str| s.chars().flat_map(|c| if "\\;,:\"".contains(c) { vec!['\\', c] } else { vec![c] }).collect::<String>();
    if mdp.is_empty() { format!("WIFI:T:nopass;S:{};;", e(ssid)) } else { format!("WIFI:T:WPA;S:{};P:{};;", e(ssid), e(mdp)) }
}

/// Qualité du signal mobile d'après sa puissance (dBm) et la génération.
pub fn qualite_mobile(dbm: i32, generation: &str) -> (&'static str, Color32) {
    let (bon, moyen, faible) = if generation == "2G" || generation == "3G" { (-75, -85, -100) } else { (-85, -95, -105) };
    match dbm {
        0 => ("pas de signal", ROUGE),
        d if d >= bon => ("excellent", VERT),
        d if d >= moyen => ("bon", VERT),
        d if d >= faible => ("moyen", ORANGE),
        _ => ("faible", ROUGE),
    }
}

/// Box d'après le nom du Wi-Fi : (opérateur, adresse de la page de réglages, chemin du réglage des canaux).
pub fn guide_box(ssid: &str, passerelle: &str) -> (&'static str, String, &'static str) {
    let s = ssid.to_lowercase();
    let adresse = |a: &str| if passerelle.is_empty() { a.to_string() } else { format!("http://{passerelle}") };
    if s.starts_with("freebox") {
        ("Free (Freebox)", "http://mafreebox.freebox.fr".into(), "Paramètres de la Freebox › Wi-Fi › Configuration › onglet de chaque bande : choisis le canal (ou laisse « auto » et redémarre).")
    } else if s.starts_with("livebox") || s.starts_with("orange") {
        ("Orange (Livebox)", adresse("http://192.168.1.1"), "Wi-Fi › Avancé (ou « Paramètres avancés ») : décoche le canal automatique et choisis-le. Mot de passe : les 8 premiers caractères de la clé Wi-Fi au dos.")
    } else if s.starts_with("sfr") || s.starts_with("red_") {
        ("SFR / RED", adresse("http://192.168.1.1"), "Wi-Fi › Configuration › Canal. Identifiant « admin », mot de passe sur l'étiquette de la box.")
    } else if s.starts_with("bbox") || s.starts_with("bouygues") {
        ("Bouygues (Bbox)", "http://mabbox.bytel.fr".into(), "Wi-Fi › Réglages avancés › Canal. Mot de passe d'administration sur l'étiquette.")
    } else {
        ("ta box", adresse("http://192.168.1.1"), "Cherche « Wi-Fi », puis « Canal » ou « Réglages avancés ». Le mot de passe d'administration est souvent sur l'étiquette sous la box.")
    }
}

// ===================================================================================================
// Mesures réseau (ping, gigue, pertes, débit)
// ===================================================================================================

/// (ping médian, gigue, % de pertes) sur 20 essais.
pub fn ping_detaille() -> (f64, f64, f64) {
    let Ok(a) = "1.1.1.1:443".parse::<SocketAddr>() else { return (0.0, 0.0, 100.0) };
    let mut v = vec![];
    let mut perdus = 0;
    for _ in 0..20 {
        let t = Instant::now();
        match TcpStream::connect_timeout(&a, Duration::from_secs(2)) {
            Ok(_) => v.push(t.elapsed().as_secs_f64() * 1000.0),
            Err(_) => perdus += 1,
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    if v.is_empty() {
        return (0.0, 0.0, 100.0);
    }
    let mut s = v.clone();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let gigue = v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (v.len().max(2) - 1) as f64;
    (s[s.len() / 2], gigue, perdus as f64 * 5.0)
}

/// Débit descendant en Mb/s (téléchargement de `octets`).
pub fn debit(octets: u64) -> Result<f64, String> {
    let t = Instant::now();
    let r = ureq::get(&format!("https://speed.cloudflare.com/__down?bytes={octets}")).timeout(Duration::from_secs(60)).call().map_err(|e| e.to_string())?;
    let n = std::io::copy(&mut r.into_reader(), &mut std::io::sink()).map_err(|e| e.to_string())?;
    Ok(n as f64 * 8.0 / t.elapsed().as_secs_f64() / 1e6)
}

fn courbe(ui: &mut egui::Ui, v: &[f32], max: f32, h: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
    ui.painter().rect_filled(r, 8.0, ui.visuals().extreme_bg_color);
    let n = v.len().max(2);
    let pts: Vec<egui::Pos2> = v.iter().enumerate().map(|(k, x)| egui::pos2(r.left() + k as f32 / (n - 1) as f32 * r.width(), r.bottom() - (x / max).clamp(0.0, 1.0) * r.height())).collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(2.5_f32, ROSE)));
}

fn texture_qr(ctx: &egui::Context, texte: &str) -> Option<egui::TextureHandle> {
    let code = qrcode::QrCode::new(texte.as_bytes()).ok()?;
    let w = code.width();
    let marge = 4;
    let n = w + 2 * marge;
    let c = code.to_colors();
    let mut px = vec![255u8; n * n];
    for y in 0..w {
        for x in 0..w {
            if c[y * w + x] == qrcode::Color::Dark {
                px[(y + marge) * n + x + marge] = 0;
            }
        }
    }
    Some(ctx.load_texture("qr_wifi", egui::ColorImage::from_gray([n, n], &px), egui::TextureOptions::NEAREST))
}

// ===================================================================================================
// « Ma box »
// ===================================================================================================

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct ReglagesBox {
    pieces: Vec<(String, i32, f64, f64)>, // pièce, signal (dBm), débit, ping
    connus: BTreeMap<String, String>,      // ip → nom donné
    surveiller: bool,
    ssid_qr: String,
}

pub struct MaBox {
    onglet: usize,
    e: Etat,
    r: Arc<Mutex<ReglagesBox>>,
    diag: Arc<Mutex<Option<(u32, Vec<String>, Mesures)>>>,
    voisins: Arc<Mutex<Vec<ReseauVoisin>>>,
    piece: String,
    dns: Arc<Mutex<Vec<(usize, Option<u128>)>>>,
    appareils: Arc<Mutex<Vec<(String, Vec<u16>)>>>,
    mdp: String,
    qr: Option<(String, egui::TextureHandle)>,
}

impl MaBox {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglagesBox>> = Arc::new(Mutex::new(charger("ma_box")));
        surveiller_intrus(r.clone());
        MaBox { onglet: 0, e: Arc::default(), r, diag: Arc::default(), voisins: Arc::default(), piece: String::new(), dns: Arc::default(), appareils: Arc::default(), mdp: String::new(), qr: None }
    }

    fn sauver(&self) {
        enregistrer("ma_box", &*self.r.lock().unwrap());
    }
}

/// Scan du réseau toutes les 15 minutes : prévient si un appareil inconnu apparaît.
fn surveiller_intrus(r: Arc<Mutex<ReglagesBox>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(15 * 60));
        if !r.lock().unwrap().surveiller || android::reseau_actif() != "Wi-Fi" {
            continue;
        }
        let Some(i) = android::wifi() else { continue };
        let Some((base, _)) = i.ip.rsplit_once('.') else { continue };
        let trouves = scanner(base);
        let mut x = r.lock().unwrap();
        for (ip, ports) in trouves {
            if ip != i.ip && !x.connus.contains_key(&ip) {
                let t = deviner(&ports, ip == i.passerelle);
                android::notifier(4000 + ip.rsplit('.').next().and_then(|n| n.parse::<i32>().ok()).unwrap_or(0), "🛡 Nouvel appareil sur ton Wi-Fi", &format!("{t} · {ip}"));
                x.connus.insert(ip, format!("nouveau : {t}"));
            }
        }
        enregistrer("ma_box", &*x);
    });
}

fn scanner(base: &str) -> Vec<(String, Vec<u16>)> {
    let l: Arc<Mutex<Vec<(String, Vec<u16>)>>> = Arc::default();
    let mut fils = vec![];
    for lot in (1..=254u32).collect::<Vec<_>>().chunks(32) {
        let (lot, l, base) = (lot.to_vec(), l.clone(), base.to_string());
        fils.push(std::thread::spawn(move || {
            for h in lot {
                let ip = format!("{base}.{h}");
                if let Some(p) = sonder(&ip) {
                    l.lock().unwrap().push((ip, p));
                }
            }
        }));
    }
    for f in fils {
        let _ = f.join();
    }
    let mut v = std::mem::take(&mut *l.lock().unwrap());
    v.sort_by_key(|x| x.0.rsplit('.').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0));
    v
}

fn voisins_ou_permission(ui: &mut egui::Ui) -> bool {
    if android::a_permission("android.permission.ACCESS_FINE_LOCATION") {
        return true;
    }
    ui.label("Pour voir les Wi-Fi des voisins, Android exige l'autorisation « localisation » (Outils ne s'en sert pas pour te localiser).");
    if gros_bouton(ui, "📍 Autoriser") {
        android::demander(&["android.permission.ACCESS_FINE_LOCATION", "android.permission.ACCESS_COARSE_LOCATION"]);
    }
    ui.ctx().request_repaint_after(Duration::from_secs(1));
    false
}

impl Outil for MaBox {
    fn nom(&self) -> &'static str {
        "🏠 Ma box"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let infos = android::wifi().unwrap_or_default();
        onglets(ui, &mut self.onglet, &["🩺 Diagnostic", "📡 Canaux", "🗺 Pièces", "🏠 Box", "⚡ DNS", "🛡 Intrus", "🔐 Partager"]);
        let occupe = etat_ui(ui, &self.e);
        match self.onglet {
            0 => {
                ui.weak("Un bouton, une minute : signal, bande, canal, débit, ping, stabilité et DNS, avec une note et quoi faire.");
                if gros_bouton(ui, "🩺 Lancer le diagnostic") && !occupe {
                    let d = self.diag.clone();
                    en_fond(&self.e, "Mesures en cours (environ 30 s)…", move || {
                        let i = android::wifi().ok_or("pas d'informations Wi-Fi")?;
                        let voisins = android::scan_wifi();
                        let mon = canal(i.frequence);
                        let meme = voisins.iter().filter(|v| v.ssid != i.ssid && canal(v.frequence) == mon && v.rssi > -85).count();
                        let (ping, gigue, perte) = ping_detaille();
                        let debit = debit(15_000_000).unwrap_or(0.0);
                        let dns = mesurer_dns(if i.dns.is_empty() || i.dns == "0.0.0.0" { "1.1.1.1" } else { &i.dns }).unwrap_or(0) as f64;
                        let m = Mesures { rssi: i.rssi, frequence: i.frequence, voisins_meme_canal: meme, debit, ping, gigue, perte, dns };
                        let (n, c) = diagnostic(&m);
                        *d.lock().unwrap() = Some((n, c, m));
                        Ok(String::new())
                    });
                }
                if let Some((note, conseils, m)) = self.diag.lock().unwrap().clone() {
                    let c = if note >= 80 { VERT } else if note >= 55 { ORANGE } else { ROUGE };
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(format!("{note} / 100")).size(56.0).strong().color(c));
                    });
                    egui::Grid::new("diag").striped(true).show(ui, |ui| {
                        for (a, b) in [
                            ("Signal", format!("{} dBm ({})", m.rssi, qualite(m.rssi).1)),
                            ("Bande", if m.frequence >= 5000 { "5 GHz".into() } else { "2,4 GHz".into() }),
                            ("Voisins sur ton canal", m.voisins_meme_canal.to_string()),
                            ("Débit", format!("{:.0} Mb/s", m.debit)),
                            ("Ping", format!("{:.0} ms (gigue {:.0} ms)", m.ping, m.gigue)),
                            ("Pertes", format!("{:.0} %", m.perte)),
                            ("DNS", format!("{:.0} ms", m.dns)),
                        ] {
                            ui.label(a);
                            ui.strong(b);
                            ui.end_row();
                        }
                    });
                    ui.label(RichText::new("À faire").size(18.0).strong());
                    for x in conseils {
                        ui.label(format!("• {x}"));
                    }
                }
            }
            1 => {
                if !voisins_ou_permission(ui) {
                    return;
                }
                if gros_bouton(ui, "📡 Scanner les Wi-Fi alentour") && !occupe {
                    let v = self.voisins.clone();
                    en_fond(&self.e, "Scan…", move || {
                        // le premier scan peut être vide : on réessaie une fois
                        let mut l = android::scan_wifi();
                        if l.is_empty() {
                            std::thread::sleep(Duration::from_secs(3));
                            l = android::scan_wifi();
                        }
                        let n = l.len();
                        *v.lock().unwrap() = l;
                        Ok(if n == 0 { "Aucun réseau vu : active la localisation d'Android et réessaie dans 30 s.".into() } else { format!("{n} réseaux") })
                    });
                }
                let v = self.voisins.lock().unwrap().clone();
                if !v.is_empty() {
                    let mien = v.iter().filter(|x| x.ssid == infos.ssid).map(|x| (canal(x.frequence), x.frequence >= 5000)).collect::<Vec<_>>();
                    for cinq in [false, true] {
                        let enc = encombrement(&v, &infos.ssid, cinq);
                        let max = enc.iter().map(|x| x.1).fold(1.0f32, f32::max);
                        let best = meilleur_canal(&v, &infos.ssid, cinq);
                        ui.label(RichText::new(if cinq { "5 GHz" } else { "2,4 GHz" }).size(18.0).strong());
                        for (c, g) in &enc {
                            ui.horizontal(|ui| {
                                let ici = mien.iter().any(|m| m.0 == *c && m.1 == cinq);
                                ui.add_sized([70.0, 20.0], egui::Label::new(RichText::new(format!("canal {c}")).strong().color(if *c == best { VERT } else if ici { ROSE } else { ui.visuals().text_color() })));
                                ui.add(egui::ProgressBar::new(g / max).desired_width(ui.available_width() - 90.0).fill(if *g / max > 0.6 { ROUGE } else if *g / max > 0.3 { ORANGE } else { VERT }));
                                ui.label(if ici { "← toi" } else if *c == best { "✔" } else { "" });
                            });
                        }
                        let actuel = mien.iter().find(|m| m.1 == cinq).map(|m| m.0);
                        match actuel {
                            Some(a) if a == best => {
                                ui.colored_label(VERT, "✔ Ta box est déjà sur le canal le plus calme.");
                            }
                            Some(a) => {
                                ui.label(RichText::new(format!("Conseil : passe ta box du canal {a} au canal {best}{}.", if cinq && dfs(best) { " (canal radar : rares micro-coupures possibles)" } else { "" })).strong());
                            }
                            None => {
                                ui.weak(format!("Canal conseillé : {best}"));
                            }
                        }
                    }
                    ui.collapsing(format!("Les {} réseaux vus", v.len()), |ui| {
                        let mut l = v.clone();
                        l.sort_by_key(|x| -x.rssi);
                        for x in l {
                            ui.label(format!("{} · canal {} · {} dBm · {} MHz", if x.ssid.is_empty() { "(masqué)" } else { &x.ssid }, canal(x.frequence), x.rssi, x.largeur));
                        }
                    });
                    ui.weak("Le canal se change dans les réglages de la box (onglet « Box »).");
                }
            }
            2 => {
                ui.weak("Va dans chaque pièce et mesure : tu verras où le Wi-Fi est bon, et où mettre un répéteur.");
                ui.horizontal_wrapped(|ui| {
                    for p in ["Salon", "Chambre", "Cuisine", "Bureau", "Salle de bain", "Couloir", "Jardin"] {
                        if ui.selectable_label(self.piece == p, p).clicked() {
                            self.piece = p.into();
                        }
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.piece).hint_text("ou tape le nom de la pièce").desired_width(f32::INFINITY));
                if gros_bouton(ui, "📏 Mesurer ici") && !occupe && !self.piece.trim().is_empty() {
                    let (r, piece) = (self.r.clone(), self.piece.trim().to_string());
                    en_fond(&self.e, "Mesure (10 s)…", move || {
                        let mut s = vec![];
                        for _ in 0..8 {
                            if let Some(i) = android::wifi() {
                                s.push(i.rssi);
                            }
                            std::thread::sleep(Duration::from_millis(400));
                        }
                        let rssi = if s.is_empty() { -100 } else { s.iter().sum::<i32>() / s.len() as i32 };
                        let d = debit(5_000_000).unwrap_or(0.0);
                        let p = ping_tcp("1.1.1.1").unwrap_or(0) as f64;
                        let mut x = r.lock().unwrap();
                        x.pieces.retain(|y| y.0 != piece);
                        x.pieces.push((piece.clone(), rssi, d, p));
                        enregistrer("ma_box", &*x);
                        Ok(format!("✔ {piece} mesuré"))
                    });
                }
                let mut l = self.r.lock().unwrap().pieces.clone();
                l.sort_by_key(|x| -x.1);
                for (p, rssi, d, ping) in &l {
                    let (q, mot) = qualite(*rssi);
                    ui.horizontal(|ui| {
                        ui.add_sized([110.0, 22.0], egui::Label::new(RichText::new(p).strong()));
                        ui.add(egui::ProgressBar::new(q as f32 / 100.0).desired_width(ui.available_width() - 150.0).fill(if q >= 70 { VERT } else if q >= 40 { ORANGE } else { ROUGE }).text(mot));
                        ui.weak(format!("{d:.0} Mb/s · {ping:.0} ms"));
                    });
                }
                if l.len() >= 2 {
                    let pire = l.last().unwrap();
                    if pire.1 < -72 {
                        ui.label(RichText::new(format!("Zone faible : {}. Un répéteur placé entre la box et cette pièce (là où le signal est encore « bon ») réglerait le problème.", pire.0)).strong());
                    } else {
                        ui.colored_label(VERT, "✔ Toutes les pièces mesurées ont un signal correct.");
                    }
                }
                if !l.is_empty() && ui.small_button("Effacer les mesures").clicked() {
                    self.r.lock().unwrap().pieces.clear();
                    self.sauver();
                }
            }
            3 => {
                let (op, adresse, guide) = guide_box(&infos.ssid, &infos.passerelle);
                ui.label(RichText::new(format!("Box détectée : {op}")).size(18.0).strong());
                if gros_bouton(ui, "🌐 Ouvrir la page de la box") {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&adresse));
                }
                ui.weak(&adresse);
                ui.label(RichText::new("Changer le canal").strong());
                ui.label(guide);
                ui.label(RichText::new("Les réglages qui comptent").strong());
                for t in [
                    "📡 Canal : prends celui conseillé dans l'onglet « Canaux » (ou « auto » puis redémarre la box).",
                    "⚡ 5 GHz activé, avec le même nom que le 2,4 GHz (« réseau unifié » / « smart Wi-Fi ») : les appareils choisissent seuls la meilleure bande.",
                    "📏 Largeur de canal : 80 MHz en 5 GHz, 20 MHz en 2,4 GHz (plus stable en appartement).",
                    "🔐 Sécurité WPA2 ou WPA3, jamais WEP ; change le mot de passe d'usine.",
                    "🔄 Redémarre la box une fois par mois, et vérifie ses mises à jour.",
                    "📍 Box en hauteur, à découvert, loin du micro-ondes et des murs épais.",
                ] {
                    ui.label(t);
                }
            }
            4 => {
                ui.weak("Le DNS traduit les noms de sites en adresses. Un DNS rapide accélère l'ouverture des pages et des jeux ; certains bloquent aussi les pubs.");
                if gros_bouton(ui, "⚡ Tester les DNS") && !occupe {
                    let (d, box_) = (self.dns.clone(), infos.passerelle.clone());
                    en_fond(&self.e, "Test des DNS…", move || {
                        let mut v = vec![];
                        for (k, (_, ip, _, _)) in DNS.iter().enumerate() {
                            let ip = if ip.is_empty() { box_.as_str() } else { ip };
                            v.push((k, if ip.is_empty() { None } else { mesurer_dns(ip) }));
                        }
                        v.sort_by_key(|x| x.1.unwrap_or(u128::MAX));
                        *d.lock().unwrap() = v;
                        Ok(String::new())
                    });
                }
                let v = self.dns.lock().unwrap().clone();
                for (k, ms) in &v {
                    let (nom, _, prive, pubs) = DNS[*k];
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(nom).size(17.0).strong());
                            match ms {
                                Some(m) => ui.colored_label(if *m < 20 { VERT } else if *m < 50 { ORANGE } else { ROUGE }, format!("{m} ms")),
                                None => ui.colored_label(Color32::GRAY, "pas de réponse"),
                            };
                            if pubs {
                                ui.label("🚫 pubs");
                            }
                        });
                        if !prive.is_empty() && ui.button("📱 Utiliser sur ce téléphone (DNS privé)").clicked() {
                            android::presse_ecrire(prive);
                            android::ouvrir_reglage("android.settings.WIRELESS_SETTINGS", false);
                            *self.e.lock().unwrap() = (false, format!("« {prive} » copié : Réseau et Internet › DNS privé › Nom d'hôte, colle-le."));
                        }
                    });
                }
                if !v.is_empty() {
                    ui.label(RichText::new("Sur le PC").strong());
                    ui.label("Paramètres › Réseau et Internet › Wi-Fi (ou Ethernet) › Propriétés du matériel › Attribution du serveur DNS › Modifier › Manuel : mets l'adresse du plus rapide (ex. 1.1.1.1). Outils PC a aussi l'outil « DNS le plus rapide ».");
                }
            }
            5 => {
                let mut s = self.r.lock().unwrap().surveiller;
                if ui.checkbox(&mut s, "Me prévenir si un appareil inconnu se connecte (vérifié toutes les 15 min quand l'appli tourne)").changed() {
                    self.r.lock().unwrap().surveiller = s;
                    self.sauver();
                }
                if gros_bouton(ui, "🔎 Scanner maintenant") && !occupe {
                    let (a, ip) = (self.appareils.clone(), infos.ip.clone());
                    en_fond(&self.e, "Scan…", move || {
                        let base = ip.rsplit_once('.').map(|x| x.0.to_string()).ok_or("pas d'adresse Wi-Fi")?;
                        let v = scanner(&base);
                        let n = v.len();
                        *a.lock().unwrap() = v;
                        Ok(format!("{n} appareil(s)"))
                    });
                }
                let liste = self.appareils.lock().unwrap().clone();
                if !liste.is_empty() && ui.button("✔ Ce sont tous mes appareils").clicked() {
                    let mut x = self.r.lock().unwrap();
                    for (ip, p) in &liste {
                        x.connus.entry(ip.clone()).or_insert_with(|| deviner(p, *ip == infos.passerelle).to_string());
                    }
                    drop(x);
                    self.sauver();
                }
                let connus = self.r.lock().unwrap().connus.clone();
                for (ip, p) in &liste {
                    ui.horizontal(|ui| {
                        let t = if *ip == infos.ip { "📱 Ce téléphone".to_string() } else { connus.get(ip).cloned().unwrap_or_else(|| deviner(p, *ip == infos.passerelle).to_string()) };
                        let nouveau = !connus.contains_key(ip) && *ip != infos.ip;
                        ui.label(RichText::new(t).strong().color(if nouveau { ORANGE } else { ui.visuals().text_color() }));
                        ui.monospace(ip);
                        if nouveau {
                            ui.colored_label(ORANGE, "inconnu");
                        }
                    });
                }
                ui.weak("Un appareil que tu ne connais pas ? Change le mot de passe Wi-Fi de la box : tout le monde sera déconnecté.");
            }
            _ => {
                ui.weak("Tes invités scannent ce QR code avec leur appareil photo et sont connectés, sans taper le mot de passe.");
                let mut x = self.r.lock().unwrap();
                if x.ssid_qr.is_empty() && !infos.ssid.contains("unknown") {
                    x.ssid_qr = infos.ssid.clone();
                }
                ui.add(egui::TextEdit::singleline(&mut x.ssid_qr).hint_text("nom du Wi-Fi").desired_width(f32::INFINITY));
                let ssid = x.ssid_qr.clone();
                drop(x);
                ui.add(egui::TextEdit::singleline(&mut self.mdp).hint_text("mot de passe du Wi-Fi (non enregistré)").password(true).desired_width(f32::INFINITY));
                let texte = qr_wifi(&ssid, &self.mdp);
                if !ssid.is_empty() {
                    if self.qr.as_ref().is_none_or(|q| q.0 != texte) {
                        self.qr = texture_qr(ui.ctx(), &texte).map(|t| (texte.clone(), t));
                        self.sauver();
                    }
                    if let Some((_, t)) = &self.qr {
                        let w = ui.available_width().min(320.0);
                        ui.vertical_centered(|ui| {
                            ui.add(egui::Image::new(t).fit_to_exact_size(Vec2::splat(w)));
                        });
                    }
                }
            }
        }
    }
}

// ===================================================================================================
// « Réseau mobile »
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Forfait {
    limite_go: f64,
    jour_debut: u32,
    alerte: String, // « cycle:80 » déjà envoyée
    economie: Vec<bool>,
    comparaison: BTreeMap<String, (f64, f64)>, // réseau → (débit, ping)
}

impl Default for Forfait {
    fn default() -> Self {
        Forfait { limite_go: 0.0, jour_debut: 1, alerte: String::new(), economie: vec![], comparaison: BTreeMap::new() }
    }
}

/// Début du cycle de forfait en cours (secondes Unix), d'après le jour de renouvellement.
pub fn debut_cycle(maintenant: u64, jour: u32) -> u64 {
    let j = (maintenant / 86_400) as i64;
    let (a, m, d) = crate::quotidien2::date_de_jours(j);
    let jour = jour.clamp(1, 28);
    let (a2, m2) = if d >= jour { (a, m) } else if m == 1 { (a - 1, 12) } else { (a, m - 1) };
    crate::quotidien2::jours_depuis_1970(a2, m2, jour) as u64 * 86_400
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Coupures {
    l: Vec<(u64, u64, String)>, // début, fin, réseau
}

/// Vérifie la connexion toutes les 20 s et note les coupures ; prévient aussi quand le forfait s'épuise.
fn veiller(f: Arc<Mutex<Forfait>>) {
    static LANCE: AtomicBool = AtomicBool::new(false);
    if LANCE.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let mut debut: Option<(u64, String)> = None;
        let mut tour = 0u32;
        loop {
            std::thread::sleep(Duration::from_secs(20));
            tour += 1;
            let reseau = android::reseau_actif();
            let ok = "1.1.1.1:443".parse::<SocketAddr>().ok().is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_secs(4)).is_ok()) || "8.8.8.8:53".parse::<SocketAddr>().ok().is_some_and(|a| TcpStream::connect_timeout(&a, Duration::from_secs(4)).is_ok());
            match (ok, &debut) {
                (false, None) => debut = Some((maintenant(), reseau.to_string())),
                (true, Some((d, r))) => {
                    let mut c: Coupures = charger("coupures");
                    c.l.push((*d, maintenant(), r.clone()));
                    let n = c.l.len();
                    if n > 300 {
                        c.l.drain(..n - 300);
                    }
                    enregistrer("coupures", &c);
                    debut = None;
                }
                _ => {}
            }
            // forfait : toutes les 15 minutes
            if tour % 45 == 1 && android::acces_statistiques() {
                let mut x = f.lock().unwrap();
                if x.limite_go > 0.0 {
                    let d = debut_cycle(maintenant(), x.jour_debut);
                    let total: u64 = android::conso_par_appli(true, d, maintenant()).iter().map(|a| a.1 + a.2).sum();
                    let pct = total as f64 / (x.limite_go * 1e9) * 100.0;
                    for seuil in [80, 100] {
                        let cle = format!("{d}:{seuil}");
                        if pct >= seuil as f64 && x.alerte < cle {
                            android::notifier(4500 + seuil, "📱 Forfait mobile", &format!("{pct:.0} % de ton forfait utilisé ({})", taille(total)));
                            x.alerte = cle;
                            enregistrer("forfait", &*x);
                        }
                    }
                }
            }
        }
    });
}

/// (texte, réglage d'Android à ouvrir, conseil)
pub const ECONOMIES: [(&str, &str, &str); 8] = [
    ("Économiseur de données d'Android", "android.settings.DATA_USAGE_SETTINGS", "Active-le : les applis en arrière-plan n'utilisent plus les données mobiles."),
    ("DNS privé qui bloque les pubs", "android.settings.WIRELESS_SETTINGS", "Réseau et Internet › DNS privé › dns.adguard-dns.com : moins de pubs, donc moins de données."),
    ("Bascule automatique Wi-Fi → données", "android.settings.WIFI_SETTINGS", "Dans les préférences Wi-Fi, désactive « passer aux données mobiles » si tu veux protéger ton forfait."),
    ("Mises à jour des applis en Wi-Fi seulement", "", "Play Store › photo de profil › Paramètres › Préférences réseau › « Via Wi-Fi uniquement »."),
    ("Lecture automatique des vidéos", "", "YouTube, Instagram, TikTok, Facebook : dans leurs réglages, « économie de données » / pas de lecture auto en 4G."),
    ("Sauvegarde des photos en Wi-Fi seulement", "", "Google Photos › Paramètres › Sauvegarde › Utilisation des données mobiles : désactivée."),
    ("Données en arrière-plan des applis gourmandes", "", "Onglet « Données » : touche une appli très gourmande › Données mobiles › désactive « Données en arrière-plan »."),
    ("5G ou 4G ?", "android.settings.DATA_ROAMING_SETTINGS", "Si la 5G capte mal chez toi, choisir « 4G » dans le type de réseau préféré économise la batterie et stabilise la connexion."),
];

pub struct Mobile {
    onglet: usize,
    e: Etat,
    f: Arc<Mutex<Forfait>>,
    historique: Vec<f32>,
    dernier: Instant,
    signal: Option<android::SignalMobile>,
    periode: usize,
    mobile: bool,
    conso: Arc<Mutex<Option<Vec<(String, u64, u64)>>>>,
}

impl Mobile {
    pub fn new() -> Self {
        let f = Arc::new(Mutex::new(charger::<Forfait>("forfait")));
        veiller(f.clone());
        Mobile { onglet: 0, e: Arc::default(), f, historique: vec![], dernier: Instant::now() - Duration::from_secs(5), signal: None, periode: 2, mobile: true, conso: Arc::default() }
    }

    fn sauver(&self) {
        enregistrer("forfait", &*self.f.lock().unwrap());
    }
}

impl Outil for Mobile {
    fn nom(&self) -> &'static str {
        "📱 Réseau mobile"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        onglets(ui, &mut self.onglet, &["📶 Signal", "📊 Données", "⚖ Wi-Fi ou 4G", "💸 Économie", "📉 Coupures"]);
        let occupe = etat_ui(ui, &self.e);
        match self.onglet {
            0 => {
                if self.dernier.elapsed() > Duration::from_millis(900) {
                    self.dernier = Instant::now();
                    self.signal = android::signal_mobile();
                    if let Some(s) = &self.signal {
                        self.historique.push((s.dbm + 130).max(0) as f32);
                        if self.historique.len() > 120 {
                            self.historique.remove(0);
                        }
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
                let Some(s) = self.signal.clone() else {
                    ui.weak("Pas de carte SIM ou réseau mobile indisponible.");
                    return;
                };
                let (mot, c) = qualite_mobile(s.dbm, &s.generation);
                ui.label(RichText::new(format!("{} · {}", s.operateur, s.generation)).size(22.0).strong());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} dBm", s.dbm)).size(40.0).strong().color(c));
                    ui.label(RichText::new(mot).size(22.0).color(c));
                });
                ui.add(egui::ProgressBar::new(s.niveau as f32 / 4.0).fill(c).text(format!("{} barre(s) sur 4", s.niveau)));
                courbe(ui, &self.historique, 70.0, 120.0);
                ui.label(format!("En ce moment, internet passe par : {}", android::reseau_actif()));
                ui.weak("Plus le nombre de dBm est proche de 0, meilleur est le signal (−80 excellent, −100 faible). Promène-toi pour trouver l'endroit qui capte le mieux ; près d'une fenêtre, c'est souvent mieux.");
            }
            1 => {
                if !android::acces_statistiques() {
                    ui.label("Pour voir quelles applis consomment tes données, Android demande l'accès « Utilisation des applis ».");
                    if gros_bouton(ui, "📊 Donner l'accès") {
                        android::ouvrir_reglage("android.settings.USAGE_ACCESS_SETTINGS", false);
                    }
                    ui.weak("Dans la liste, choisis Outils et active l'accès, puis reviens.");
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                    return;
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.mobile, true, "📶 Données mobiles");
                    ui.selectable_value(&mut self.mobile, false, "📡 Wi-Fi");
                });
                ui.horizontal(|ui| {
                    for (k, n) in ["Aujourd'hui", "7 jours", "Ce forfait"].iter().enumerate() {
                        ui.selectable_value(&mut self.periode, k, *n);
                    }
                });
                let mut f = self.f.lock().unwrap().clone();
                ui.horizontal(|ui| {
                    ui.label("Forfait :");
                    ui.add(egui::DragValue::new(&mut f.limite_go).range(0.0..=1000.0).speed(1.0).suffix(" Go"));
                    ui.label("renouvelé le");
                    ui.add(egui::DragValue::new(&mut f.jour_debut).range(1..=28));
                });
                if f.limite_go != self.f.lock().unwrap().limite_go || f.jour_debut != self.f.lock().unwrap().jour_debut {
                    *self.f.lock().unwrap() = f.clone();
                    self.sauver();
                }
                if gros_bouton(ui, "📊 Calculer") && !occupe {
                    let maintenant_ = maintenant();
                    let debut = match self.periode {
                        0 => maintenant_ - maintenant_ % 86_400,
                        1 => maintenant_ - 7 * 86_400,
                        _ => debut_cycle(maintenant_, f.jour_debut),
                    };
                    let (c, m) = (self.conso.clone(), self.mobile);
                    en_fond(&self.e, "Calcul…", move || {
                        let v = android::conso_par_appli(m, debut, maintenant());
                        let t: u64 = v.iter().map(|x| x.1 + x.2).sum();
                        *c.lock().unwrap() = Some(v);
                        Ok(format!("Total : {}", taille(t)))
                    });
                }
                if let Some(v) = self.conso.lock().unwrap().clone() {
                    let total: u64 = v.iter().map(|x| x.1 + x.2).sum();
                    if self.mobile && self.periode == 2 && f.limite_go > 0.0 {
                        let pct = total as f64 / (f.limite_go * 1e9);
                        ui.add(egui::ProgressBar::new(pct.min(1.0) as f32).fill(if pct > 0.9 { ROUGE } else if pct > 0.7 { ORANGE } else { VERT }).text(format!("{} sur {} Go", taille(total), f.limite_go)));
                    }
                    let max = v.first().map(|x| x.1 + x.2).unwrap_or(1).max(1);
                    for (nom, rx, tx) in v.iter().take(30) {
                        if rx + tx < 100_000 {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.add_sized([130.0, 22.0], egui::Label::new(RichText::new(nom).strong()).truncate());
                            ui.add(egui::ProgressBar::new((rx + tx) as f32 / max as f32).desired_width(ui.available_width() - 80.0).fill(ROSE));
                            ui.label(taille(rx + tx));
                        });
                    }
                    ui.weak("Une appli consomme trop ? Réglages › Applis › l'appli › Données mobiles › désactive « Données en arrière-plan ».");
                }
            }
            2 => {
                ui.weak("Fais le test en Wi-Fi, puis coupe le Wi-Fi et refais-le en 4G/5G : tu sauras lequel est le meilleur ici.");
                let r = android::reseau_actif();
                ui.label(RichText::new(format!("Réseau actuel : {r}")).size(18.0).strong());
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("⚖ Tester ce réseau").size(18.0)).clicked() && !occupe {
                        let (f, nom) = (self.f.clone(), if r == "mobile" { "Données mobiles".to_string() } else { r.to_string() });
                        en_fond(&self.e, "Test (une quinzaine de secondes)…", move || {
                            let (ping, _, _) = ping_detaille();
                            let d = debit(10_000_000)?;
                            let mut x = f.lock().unwrap();
                            x.comparaison.insert(nom.clone(), (d, ping));
                            enregistrer("forfait", &*x);
                            Ok(format!("✔ {nom} : {d:.0} Mb/s, {ping:.0} ms"))
                        });
                    }
                    if ui.button("📡 Réglages Wi-Fi").clicked() {
                        android::ouvrir_reglage("android.settings.WIFI_SETTINGS", false);
                    }
                });
                let c = self.f.lock().unwrap().comparaison.clone();
                for (nom, (d, p)) in &c {
                    ui.horizontal(|ui| {
                        ui.add_sized([150.0, 24.0], egui::Label::new(RichText::new(nom).strong()));
                        ui.label(RichText::new(format!("{d:.0} Mb/s")).size(20.0).color(ROSE));
                        ui.label(format!("· {p:.0} ms"));
                    });
                }
                if let (Some(w), Some(m)) = (c.get("Wi-Fi"), c.get("Données mobiles")) {
                    let gagnant = if w.0 >= m.0 * 0.8 && w.1 <= m.1 * 1.5 { "Wi-Fi" } else { "données mobiles" };
                    ui.label(RichText::new(format!("Ici, le mieux c'est : {gagnant}")).size(18.0).strong().color(VERT));
                    if gagnant == "données mobiles" {
                        ui.weak("Ton Wi-Fi est moins bon que ta 4G à cet endroit : regarde « Ma box » › Diagnostic.");
                    }
                }
            }
            3 => {
                ui.weak("Les réglages qui font vraiment économiser des données. Coche ce qui est fait.");
                let mut f = self.f.lock().unwrap();
                f.economie.resize(ECONOMIES.len(), false);
                let mut change = false;
                for (k, (titre, reglage, conseil)) in ECONOMIES.iter().enumerate() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        change |= ui.checkbox(&mut f.economie[k], RichText::new(*titre).size(17.0).strong()).changed();
                        ui.label(*conseil);
                        if !reglage.is_empty() && ui.button("⚙ Ouvrir le réglage").clicked() {
                            if k == 1 {
                                android::presse_ecrire("dns.adguard-dns.com");
                            }
                            android::ouvrir_reglage(reglage, false);
                        }
                    });
                }
                let n = f.economie.iter().filter(|x| **x).count();
                ui.add(egui::ProgressBar::new(n as f32 / ECONOMIES.len() as f32).fill(VERT).text(format!("{n} / {}", ECONOMIES.len())));
                if change {
                    enregistrer("forfait", &*f);
                }
            }
            _ => {
                ui.weak("L'appli vérifie la connexion toutes les 20 secondes tant qu'elle tourne, et note chaque coupure : utile comme preuve pour ton opérateur.");
                let c: Coupures = charger("coupures");
                let semaine = maintenant().saturating_sub(7 * 86_400);
                let recentes: Vec<&(u64, u64, String)> = c.l.iter().filter(|x| x.0 >= semaine).collect();
                let total: u64 = recentes.iter().map(|x| x.1 - x.0).sum();
                ui.label(RichText::new(format!("7 derniers jours : {} coupure(s), {} min au total", recentes.len(), total / 60)).size(18.0).strong());
                for (d, f, r) in c.l.iter().rev().take(50) {
                    let s = d % 86_400;
                    ui.label(format!("{} {:02}:{:02} · {} min {} s · {r}", date(*d), s / 3600, s / 60 % 60, (f - d) / 60, (f - d) % 60));
                }
                if !c.l.is_empty() && ui.button("📋 Copier le rapport (pour l'opérateur)").clicked() {
                    let t: Vec<String> = c.l.iter().map(|(d, f, r)| format!("{} {:02}:{:02} UTC — {} s — {r}", date(*d), d % 86_400 / 3600, d % 3600 / 60, f - d)).collect();
                    android::presse_ecrire(&format!("Coupures internet relevées par Outils :\n{}", t.join("\n")));
                    *self.e.lock().unwrap() = (false, "Rapport copié.".into());
                }
                if c.l.is_empty() {
                    ui.colored_label(VERT, "Aucune coupure enregistrée pour l'instant.");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(ssid: &str, rssi: i32, f: i32, l: i32) -> ReseauVoisin {
        ReseauVoisin { ssid: ssid.into(), bssid: String::new(), rssi, frequence: f, largeur: l }
    }

    #[test]
    fn canaux() {
        assert_eq!(canal(2412), 1);
        assert_eq!(canal(2437), 6);
        assert_eq!(canal(2462), 11);
        assert_eq!(canal(5180), 36);
        assert_eq!(canal(5745), 149);
        assert!(meme_bloc(36, 48, 80) && !meme_bloc(36, 52, 80) && meme_bloc(36, 40, 40) && !meme_bloc(36, 44, 40) && !meme_bloc(36, 40, 20));
        // deux voisins forts sur le 6 et un sur le 1 : le 11 est le plus calme
        let l = vec![v("A", -50, 2437, 20), v("B", -55, 2437, 20), v("C", -70, 2412, 20), v("Moi", -40, 2462, 20)];
        assert_eq!(meilleur_canal(&l, "Moi", false), 11);
        // en 5 GHz, un voisin en 80 MHz sur 36 gêne 36 à 48 ; le DFS est évité
        let l = vec![v("A", -50, 5180, 80)];
        let b = meilleur_canal(&l, "Moi", true);
        assert!(!(36..=48).contains(&b) && !dfs(b), "{b}");
    }

    #[test]
    fn dns_et_qr() {
        let q = requete_dns(0x1234, "discord.com");
        assert_eq!(&q[..2], &[0x12, 0x34]);
        assert_eq!(&q[12..20], b"\x07discord");
        assert_eq!(&q[q.len() - 5..], &[0, 0, 1, 0, 1]);
        assert_eq!(qr_wifi("Box;1", "a:b"), "WIFI:T:WPA;S:Box\\;1;P:a\\:b;;");
        assert_eq!(qr_wifi("Libre", ""), "WIFI:T:nopass;S:Libre;;");
    }

    #[test]
    fn diagnostics() {
        let bon = Mesures { rssi: -50, frequence: 5180, voisins_meme_canal: 0, debit: 300.0, ping: 12.0, gigue: 2.0, perte: 0.0, dns: 10.0 };
        let (n, c) = diagnostic(&bon);
        assert_eq!(n, 100);
        assert!(c[0].contains("pleine forme"));
        let mauvais = Mesures { rssi: -82, frequence: 2437, voisins_meme_canal: 4, debit: 3.0, ping: 120.0, gigue: 30.0, perte: 10.0, dns: 90.0 };
        let (n, c) = diagnostic(&mauvais);
        assert!(n < 20);
        assert!(c[0].contains("zone morte"));
        assert_eq!(qualite_mobile(-80, "4G").0, "excellent");
        assert_eq!(qualite_mobile(-110, "5G").0, "faible");
        assert_eq!(guide_box("Freebox-ABC", "192.168.1.254").0, "Free (Freebox)");
        assert!(guide_box("Livebox-12", "192.168.1.1").1.contains("192.168.1.1"));
        // forfait renouvelé le 15 : le 20 mai → 15 mai ; le 3 mai → 15 avril
        let j = |a, m, d| crate::quotidien2::jours_depuis_1970(a, m, d) as u64 * 86_400;
        assert_eq!(debut_cycle(j(2026, 5, 20) + 3600, 15), j(2026, 5, 15));
        assert_eq!(debut_cycle(j(2026, 5, 3), 15), j(2026, 4, 15));
        assert_eq!(debut_cycle(j(2026, 1, 3), 15), j(2025, 12, 15));
    }
}
