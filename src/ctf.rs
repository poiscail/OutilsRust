//! Apprendre la sécurité en jouant (CTF) : décodeur universel, défis à résoudre, stéganographie,
//! calculatrice réseau, labo de hachage, et les plateformes d'entraînement légales.
use crate::outil::{charger, enregistrer, Cat, Outil};
use base64::Engine;
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ===================================================================================================
// 16 · Décodeur
// ===================================================================================================

pub fn rot(s: &str, n: u8) -> String {
    s.chars().map(|c| match c {
        'a'..='z' => (((c as u8 - b'a' + n) % 26) + b'a') as char,
        'A'..='Z' => (((c as u8 - b'A' + n) % 26) + b'A') as char,
        x => x,
    }).collect()
}

pub fn atbash(s: &str) -> String {
    s.chars().map(|c| match c {
        'a'..='z' => (b'z' - (c as u8 - b'a')) as char,
        'A'..='Z' => (b'Z' - (c as u8 - b'A')) as char,
        x => x,
    }).collect()
}

pub fn vigenere(s: &str, cle: &str, dechiffrer: bool) -> String {
    let k: Vec<u8> = cle.to_lowercase().bytes().filter(|b| b.is_ascii_lowercase()).map(|b| b - b'a').collect();
    if k.is_empty() {
        return s.to_string();
    }
    let mut j = 0;
    s.chars().map(|c| {
        if !c.is_ascii_alphabetic() {
            return c;
        }
        let base = if c.is_ascii_lowercase() { b'a' } else { b'A' };
        let d = k[j % k.len()];
        j += 1;
        let x = c as u8 - base;
        (base + if dechiffrer { (x + 26 - d) % 26 } else { (x + d) % 26 }) as char
    }).collect()
}

const MORSE: &[(char, &str)] = &[
    ('A', ".-"), ('B', "-..."), ('C', "-.-."), ('D', "-.."), ('E', "."), ('F', "..-."), ('G', "--."), ('H', "...."), ('I', ".."), ('J', ".---"), ('K', "-.-"), ('L', ".-.."), ('M', "--"),
    ('N', "-."), ('O', "---"), ('P', ".--."), ('Q', "--.-"), ('R', ".-."), ('S', "..."), ('T', "-"), ('U', "..-"), ('V', "...-"), ('W', ".--"), ('X', "-..-"), ('Y', "-.--"), ('Z', "--.."),
    ('0', "-----"), ('1', ".----"), ('2', "..---"), ('3', "...--"), ('4', "....-"), ('5', "....."), ('6', "-...."), ('7', "--..."), ('8', "---.."), ('9', "----."),
];

pub fn vers_morse(s: &str) -> String {
    s.split_whitespace().map(|m| m.chars().filter_map(|c| MORSE.iter().find(|x| x.0 == c.to_ascii_uppercase()).map(|x| x.1)).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" / ")
}

pub fn depuis_morse(s: &str) -> Option<String> {
    if !s.chars().all(|c| ".-/ |\n".contains(c)) || !s.contains(['.', '-']) {
        return None;
    }
    Some(s.split(['/', '|']).map(|m| m.split_whitespace().map(|l| MORSE.iter().find(|x| x.1 == l).map(|x| x.0).unwrap_or('?')).collect::<String>()).collect::<Vec<_>>().join(" ").to_lowercase())
}

pub fn depuis_binaire(s: &str) -> Option<Vec<u8>> {
    let b: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if b.is_empty() || b.len() % 8 != 0 || !b.chars().all(|c| c == '0' || c == '1') {
        return None;
    }
    Some(b.as_bytes().chunks(8).map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap_or("0"), 2).unwrap_or(0)).collect())
}

pub fn depuis_hex(s: &str) -> Option<Vec<u8>> {
    let h: String = s.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    let h = h.trim_start_matches("0x");
    if h.is_empty() || h.len() % 2 != 0 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok()).collect()
}

pub fn depuis_base64(s: &str) -> Option<Vec<u8>> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if t.len() < 4 || !t.chars().all(|c| c.is_ascii_alphanumeric() || "+/=-_".contains(c)) {
        return None;
    }
    let e = &base64::engine::general_purpose::STANDARD;
    e.decode(&t).or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&t)).or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(t.trim_end_matches('='))).ok()
}

pub fn depuis_decimal(s: &str) -> Option<Vec<u8>> {
    let v: Vec<&str> = s.split([' ', ',']).filter(|x| !x.is_empty()).collect();
    if v.len() < 2 {
        return None;
    }
    v.iter().map(|x| x.parse::<u8>().ok()).collect()
}

pub fn xor(d: &[u8], cle: &[u8]) -> Vec<u8> {
    if cle.is_empty() {
        return d.to_vec();
    }
    d.iter().enumerate().map(|(i, b)| b ^ cle[i % cle.len()]).collect()
}

/// Note de « lisibilité » d'un texte (plus c'est haut, plus ça ressemble à du français ou de l'anglais).
pub fn lisibilite(t: &str) -> f32 {
    if t.is_empty() {
        return 0.0;
    }
    let n = t.chars().count() as f32;
    let imprimables = t.chars().filter(|c| c.is_alphanumeric() || " .,;:!?'-_{}()\"".contains(*c)).count() as f32;
    let lettres = t.chars().filter(|c| c.is_alphabetic() || *c == ' ').count() as f32;
    let bas = format!(" {} ", t.to_lowercase());
    let mots = [" le ", " la ", " les ", " de ", " et ", " est ", " un ", " the ", " and ", "flag", " is ", " des ", " pour ", " bravo", " mot "];
    let bonus = mots.iter().filter(|m| bas.contains(*m)).count() as f32 * 0.15;
    // paires de lettres fréquentes en français et en anglais
    const PAIRES: &[&str] = &["es", "le", "de", "en", "re", "nt", "on", "er", "te", "ou", "ch", "an", "qu", "th", "he", "in", "ai", "it", "ra", "la", "ar", "st", "ur", "ne", "is", "se", "et", "el", "co", "ce", "me", "ma", "pa", "ti", "io"];
    let c: Vec<char> = bas.chars().collect();
    let paires = c.windows(2).filter(|w| PAIRES.iter().any(|p| { let mut q = p.chars(); q.next() == Some(w[0]) && q.next() == Some(w[1]) })).count() as f32;
    let rares = c.iter().filter(|x| "kwxzqj".contains(**x)).count() as f32;
    imprimables / n * 0.6 + lettres / n * 0.4 + bonus + paires / n * 1.5 - rares / n
}

fn texte(b: &[u8]) -> Option<String> {
    let s = String::from_utf8(b.to_vec()).ok()?;
    (!s.chars().any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')).then_some(s)
}

/// Essaie tous les décodages (et deux niveaux de suite) ; renvoie les plus lisibles : (chemin, résultat).
pub fn detecter(entree: &str) -> Vec<(String, String)> {
    fn etape(s: &str) -> Vec<(String, String)> {
        let mut v = vec![];
        if let Some(t) = depuis_base64(s).and_then(|b| texte(&b)) {
            v.push(("Base64".into(), t));
        }
        if let Some(t) = depuis_hex(s).and_then(|b| texte(&b)) {
            v.push(("hexadécimal".into(), t));
        }
        if let Some(t) = depuis_binaire(s).and_then(|b| texte(&b)) {
            v.push(("binaire".into(), t));
        }
        if let Some(t) = depuis_decimal(s).and_then(|b| texte(&b)) {
            v.push(("codes ASCII".into(), t));
        }
        if let Some(t) = crate::secu_mdp::base32(s).and_then(|b| texte(&b)) {
            if s.len() >= 8 && s.chars().all(|c| c.is_ascii_uppercase() || "234567=".contains(c)) {
                v.push(("Base32".into(), t));
            }
        }
        if let Some(t) = depuis_morse(s) {
            v.push(("morse".into(), t));
        }
        if s.contains('%') {
            v.push(("URL".into(), crate::secu_mdp::decoder_url(s)));
        }
        if s.chars().any(|c| c.is_ascii_alphabetic()) {
            for n in 1..26u8 {
                v.push((format!("César {n} (ROT{})", 26 - n), rot(s, n)));
            }
            v.push(("Atbash".into(), atbash(s)));
            v.push(("à l'envers".into(), s.chars().rev().collect()));
        }
        // XOR avec un seul octet, sur des données en hexadécimal
        if let Some(b) = depuis_hex(s) {
            for k in 1..=255u8 {
                if let Some(t) = texte(&xor(&b, &[k])) {
                    v.push((format!("hexadécimal puis XOR 0x{k:02X}"), t));
                }
            }
        }
        v
    }
    let mut tous = etape(entree.trim());
    let premiers: Vec<(String, String)> = tous.iter().filter(|x| !x.0.starts_with("César") && x.0 != "à l'envers" && x.0 != "Atbash").cloned().collect();
    for (c, t) in premiers {
        for (c2, t2) in etape(&t) {
            tous.push((format!("{c} → {c2}"), t2));
        }
    }
    tous.retain(|x| !x.1.trim().is_empty() && x.1 != entree);
    tous.sort_by(|a, b| lisibilite(&b.1).total_cmp(&lisibilite(&a.1)));
    tous.dedup_by(|a, b| a.1 == b.1);
    tous.truncate(15);
    tous
}

pub struct Decodeur {
    entree: String,
    cle: String,
    sortie: String,
}

impl Decodeur {
    pub fn new() -> Self {
        Decodeur { entree: String::new(), cle: String::new(), sortie: String::new() }
    }
}

impl Outil for Decodeur {
    fn nom(&self) -> &'static str {
        "🧩 Décodeur CTF"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Décodeur universel");
        ui.weak("Base64, hexadécimal, binaire, morse, César, Vigenère, XOR, Atbash… Colle un message codé : la détection automatique essaie tout et classe les résultats les plus lisibles.");
        ui.add(egui::TextEdit::multiline(&mut self.entree).desired_rows(3).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        ui.horizontal(|ui| {
            ui.label("Clé (Vigenère / XOR) :");
            ui.add(egui::TextEdit::singleline(&mut self.cle).desired_width(140.0));
        });
        let e = self.entree.clone();
        let k = self.cle.clone();
        let mut res: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            let b64 = &base64::engine::general_purpose::STANDARD;
            let mut b = |n: &str, f: &dyn Fn() -> String| {
                if ui.button(n).clicked() {
                    res = Some(f());
                }
            };
            b("→ Base64", &|| b64.encode(e.as_bytes()));
            b("Base64 →", &|| depuis_base64(&e).map(|x| String::from_utf8_lossy(&x).to_string()).unwrap_or_else(|| "(pas du Base64)".into()));
            b("→ hexa", &|| e.bytes().map(|x| format!("{x:02x}")).collect());
            b("hexa →", &|| depuis_hex(&e).map(|x| String::from_utf8_lossy(&x).to_string()).unwrap_or_else(|| "(pas de l'hexa)".into()));
            b("→ binaire", &|| e.bytes().map(|x| format!("{x:08b}")).collect::<Vec<_>>().join(" "));
            b("binaire →", &|| depuis_binaire(&e).map(|x| String::from_utf8_lossy(&x).to_string()).unwrap_or_else(|| "(pas du binaire)".into()));
            b("→ morse", &|| vers_morse(&e));
            b("morse →", &|| depuis_morse(&e).unwrap_or_else(|| "(pas du morse)".into()));
            b("ROT13", &|| rot(&e, 13));
            b("Atbash", &|| atbash(&e));
            b("Vigenère chiffrer", &|| vigenere(&e, &k, false));
            b("Vigenère déchiffrer", &|| vigenere(&e, &k, true));
            b("XOR (clé)", &|| {
                let d = depuis_hex(&e).unwrap_or_else(|| e.as_bytes().to_vec());
                let r = xor(&d, k.as_bytes());
                texte(&r).unwrap_or_else(|| r.iter().map(|x| format!("{x:02x}")).collect())
            });
            b("URL →", &|| crate::secu_mdp::decoder_url(&e));
            b("à l'envers", &|| e.chars().rev().collect());
        });
        if let Some(r) = res {
            self.sortie = r;
        }
        if !self.sortie.is_empty() {
            ui.horizontal(|ui| {
                ui.monospace(&self.sortie);
                if ui.small_button("📋").clicked() {
                    ui.ctx().copy_text(self.sortie.clone());
                }
                if ui.small_button("↑ remettre en entrée").clicked() {
                    self.entree = std::mem::take(&mut self.sortie);
                }
            });
        }
        ui.separator();
        ui.strong("🔮 Détection automatique");
        if self.entree.trim().len() >= 3 {
            for (c, t) in detecter(&self.entree) {
                ui.horizontal(|ui| {
                    ui.weak(format!("{c} :"));
                    ui.monospace(t.chars().take(160).collect::<String>());
                });
            }
        }
    }
}

// ===================================================================================================
// 18 · Stéganographie
// ===================================================================================================

const MAGIE: &[u8; 4] = b"OUTL";

fn flux(mdp: &str, n: usize) -> Vec<u8> {
    use sha2::Digest;
    let mut v = Vec::with_capacity(n + 32);
    let mut i = 0u32;
    while v.len() < n {
        v.extend_from_slice(&sha2::Sha256::digest([mdp.as_bytes(), &i.to_le_bytes()].concat()));
        i += 1;
    }
    v.truncate(n);
    v
}

/// Cache un message dans les bits de poids faible (rouge, vert, bleu) d'une image.
pub fn cacher(img: &mut image::RgbaImage, message: &[u8], mdp: &str) -> Result<(), String> {
    let donnees = if mdp.is_empty() { message.to_vec() } else { xor(message, &flux(mdp, message.len())) };
    let mut tout = MAGIE.to_vec();
    tout.extend_from_slice(&(donnees.len() as u32).to_be_bytes());
    tout.extend_from_slice(&donnees);
    let place = img.width() as usize * img.height() as usize * 3 / 8;
    if tout.len() > place {
        return Err(format!("image trop petite : {place} octets de place, il en faut {}", tout.len()));
    }
    let bits = tout.iter().flat_map(|o| (0..8).rev().map(move |k| (o >> k) & 1));
    let mut px = img.pixels_mut().flat_map(|p| p.0.iter_mut().take(3));
    for b in bits {
        if let Some(c) = px.next() {
            *c = (*c & 0xFE) | b;
        }
    }
    Ok(())
}

fn lire_bits(img: &image::RgbaImage, n_octets: usize, saut: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n_octets);
    let mut cur = 0u8;
    let mut k = 0;
    for c in img.pixels().flat_map(|p| p.0.into_iter().take(3)).skip(saut * 8) {
        cur = (cur << 1) | (c & 1);
        k += 1;
        if k == 8 {
            v.push(cur);
            cur = 0;
            k = 0;
            if v.len() == n_octets {
                break;
            }
        }
    }
    v
}

/// Retrouve un message caché par Outils (None s'il n'y en a pas).
pub fn reveler(img: &image::RgbaImage, mdp: &str) -> Option<Vec<u8>> {
    let tete = lire_bits(img, 8, 0);
    if tete.len() < 8 || &tete[..4] != MAGIE {
        return None;
    }
    let n = u32::from_be_bytes([tete[4], tete[5], tete[6], tete[7]]) as usize;
    if n > img.width() as usize * img.height() as usize {
        return None;
    }
    let d = lire_bits(img, n, 8);
    Some(if mdp.is_empty() { d } else { xor(&d, &flux(mdp, d.len())) })
}

/// Image du plan de poids faible : les données cachées y forment souvent du « bruit » visible.
pub fn plan_faible(img: &image::RgbaImage) -> egui::ColorImage {
    let px: Vec<u8> = img.pixels().flat_map(|p| {
        let v = if (p[0] ^ p[1] ^ p[2]) & 1 == 1 { 255 } else { 0 };
        [v, v, v, 255]
    }).collect();
    egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], &px)
}

pub struct Stegano {
    chemin: Option<PathBuf>,
    img: Option<image::RgbaImage>,
    message: String,
    mdp: String,
    trouve: String,
    vue: Option<egui::TextureHandle>,
    info: String,
}

impl Stegano {
    pub fn new() -> Self {
        Stegano { chemin: None, img: None, message: String::new(), mdp: String::new(), trouve: String::new(), vue: None, info: String::new() }
    }
}

impl Outil for Stegano {
    fn nom(&self) -> &'static str {
        "🖼 Stéganographie"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stéganographie : messages cachés dans les images");
        ui.weak("Cache un texte dans une image sans que ça se voie (bits de poids faible des pixels), ou cherche s'il y en a un. Avec un mot de passe, seul celui qui le connaît peut le lire.");
        if ui.button("📂 Ouvrir une image…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "webp"]).pick_file() {
                match image::open(&p) {
                    Ok(i) => {
                        self.img = Some(i.to_rgba8());
                        self.chemin = Some(p);
                        self.vue = None;
                        self.trouve.clear();
                        self.info.clear();
                    }
                    Err(e) => self.info = format!("✖ {e}"),
                }
            }
        }
        let Some(img) = self.img.clone() else { return };
        ui.label(format!("{} × {} · place pour {} caractères environ", img.width(), img.height(), img.width() as usize * img.height() as usize * 3 / 8 - 8));
        ui.horizontal(|ui| {
            ui.label("Mot de passe (facultatif) :");
            ui.add(egui::TextEdit::singleline(&mut self.mdp).password(true).desired_width(140.0));
        });
        ui.separator();
        ui.strong("🔍 Chercher un message");
        ui.horizontal(|ui| {
            if ui.button("Révéler").clicked() {
                self.trouve = match reveler(&img, &self.mdp) {
                    Some(d) => match String::from_utf8(d.clone()) {
                        Ok(t) => format!("✔ Message trouvé : {t}"),
                        Err(_) => "Message trouvé, mais illisible : mauvais mot de passe ?".into(),
                    },
                    None => {
                        // pas caché par Outils : texte brut dans les bits faibles ?
                        let brut = lire_bits(&img, 400, 0);
                        let t: String = brut.iter().take_while(|b| (32..127).contains(*b)).map(|b| *b as char).collect();
                        if t.len() >= 4 { format!("Pas de message d'Outils, mais du texte dans les bits faibles : {t}") } else { "Aucun message caché trouvé.".into() }
                    }
                };
            }
            if ui.button("Voir le plan des bits faibles").clicked() {
                self.vue = Some(ui.ctx().load_texture("lsb", plan_faible(&img), egui::TextureOptions::NEAREST));
            }
        });
        if !self.trouve.is_empty() {
            ui.label(RichText::new(&self.trouve).strong());
        }
        if let Some(t) = &self.vue {
            let l = ui.available_width().min(500.0);
            ui.image((t.id(), egui::vec2(l, l * t.size()[1] as f32 / t.size()[0] as f32)));
            ui.weak("Une image normale donne du bruit uniforme ; une zone différente (souvent en haut) trahit des données cachées.");
        }
        ui.separator();
        ui.strong("✍ Cacher un message");
        ui.add(egui::TextEdit::multiline(&mut self.message).desired_rows(2).desired_width(f32::INFINITY));
        if ui.add_enabled(!self.message.is_empty(), egui::Button::new("Cacher et enregistrer (PNG)")).clicked() {
            let mut i = img.clone();
            match cacher(&mut i, self.message.as_bytes(), &self.mdp) {
                Ok(()) => {
                    let p = self.chemin.clone().unwrap_or_default();
                    let dest = p.with_file_name(format!("{} (message).png", p.file_stem().unwrap_or_default().to_string_lossy()));
                    self.info = match i.save(&dest) {
                        Ok(()) => format!("✔ Enregistrée : {} (garde-la en PNG : le JPEG efface le message)", dest.display()),
                        Err(e) => format!("✖ {e}"),
                    };
                }
                Err(e) => self.info = format!("✖ {e}"),
            }
        }
        ui.label(&self.info);
    }
}

// ===================================================================================================
// 19 · Calculatrice réseau
// ===================================================================================================

#[derive(Debug, PartialEq)]
pub struct Sousreseau {
    pub reseau: std::net::Ipv4Addr,
    pub diffusion: std::net::Ipv4Addr,
    pub masque: std::net::Ipv4Addr,
    pub premier: std::net::Ipv4Addr,
    pub dernier: std::net::Ipv4Addr,
    pub hotes: u64,
}

pub fn calculer(cidr: &str) -> Option<Sousreseau> {
    let (ip, p) = cidr.trim().split_once('/').unwrap_or((cidr.trim(), "24"));
    let ip: std::net::Ipv4Addr = ip.trim().parse().ok()?;
    let p: u32 = if p.contains('.') { u32::from(p.parse::<std::net::Ipv4Addr>().ok()?).count_ones() } else { p.trim().parse().ok()? };
    if p > 32 {
        return None;
    }
    let m: u32 = if p == 0 { 0 } else { u32::MAX << (32 - p) };
    let r = u32::from(ip) & m;
    let d = r | !m;
    let (premier, dernier, hotes) = match p {
        32 => (r, r, 1),
        31 => (r, d, 2),
        _ => (r + 1, d - 1, (1u64 << (32 - p)) - 2),
    };
    Some(Sousreseau { reseau: r.into(), diffusion: d.into(), masque: m.into(), premier: premier.into(), dernier: dernier.into(), hotes })
}

pub fn binaire_ip(ip: std::net::Ipv4Addr) -> String {
    ip.octets().iter().map(|o| format!("{o:08b}")).collect::<Vec<_>>().join(".")
}

pub struct CalculReseau {
    cidr: String,
    nombre: String,
    v6: String,
}

impl CalculReseau {
    pub fn new() -> Self {
        CalculReseau { cidr: "192.168.1.42/24".into(), nombre: "255".into(), v6: "2001:0db8:0000:0000:0000:ff00:0042:8329".into() }
    }
}

impl Outil for CalculReseau {
    fn nom(&self) -> &'static str {
        "🧮 Calculatrice réseau"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Calculatrice réseau");
        ui.horizontal(|ui| {
            ui.label("Adresse / masque :");
            ui.add(egui::TextEdit::singleline(&mut self.cidr).desired_width(200.0).font(egui::TextStyle::Monospace));
        });
        match calculer(&self.cidr) {
            Some(s) => {
                egui::Grid::new("cidr").num_columns(3).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (n, ip) in [("Réseau", s.reseau), ("Masque", s.masque), ("Première adresse", s.premier), ("Dernière adresse", s.dernier), ("Diffusion (broadcast)", s.diffusion)] {
                        ui.label(n);
                        ui.monospace(ip.to_string());
                        ui.weak(RichText::new(binaire_ip(ip)).monospace());
                        ui.end_row();
                    }
                });
                let ip: Option<std::net::Ipv4Addr> = self.cidr.split('/').next().and_then(|x| x.trim().parse().ok());
                ui.label(format!("{} appareil(s) possible(s){}", s.hotes, ip.map(|i| if i.is_private() { " · adresse privée (réseau local)" } else if i.is_loopback() { " · cette machine" } else { " · adresse publique (internet)" }).unwrap_or("")));
            }
            None => {
                ui.weak("Écris une adresse comme 192.168.1.42/24 ou 10.0.0.5/255.255.255.0");
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Nombre :");
            ui.add(egui::TextEdit::singleline(&mut self.nombre).desired_width(160.0).font(egui::TextStyle::Monospace));
        });
        let t = self.nombre.trim().to_lowercase();
        let n = if let Some(h) = t.strip_prefix("0x") { u64::from_str_radix(h, 16).ok() } else if let Some(b) = t.strip_prefix("0b") { u64::from_str_radix(b, 2).ok() } else if let Some(o) = t.strip_prefix("0o") { u64::from_str_radix(o, 8).ok() } else { t.parse().ok() };
        match n {
            Some(n) => {
                ui.monospace(format!("décimal {n} · hexa 0x{n:X} · binaire 0b{n:b} · octal 0o{n:o}"));
            }
            None => {
                ui.weak("décimal, ou 0x… (hexa), 0b… (binaire), 0o… (octal)");
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("IPv6 :");
            ui.add(egui::TextEdit::singleline(&mut self.v6).desired_width(330.0).font(egui::TextStyle::Monospace));
        });
        if let Ok(a) = self.v6.trim().parse::<std::net::Ipv6Addr>() {
            ui.monospace(format!("courte : {a}"));
            ui.monospace(format!("complète : {}", a.segments().iter().map(|s| format!("{s:04x}")).collect::<Vec<_>>().join(":")));
        }
    }
}

// ===================================================================================================
// 20 · Labo de hachage
// ===================================================================================================

/// MD5 (cassé, à ne plus utiliser — mais encore partout dans les CTF).
pub fn md5(m: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = m.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((m.len() as u64).wrapping_mul(8)).to_le_bytes());
    for bloc in msg.chunks(64) {
        let w: Vec<u32> = bloc.chunks(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(w[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut r = [0u8; 16];
    for (i, x) in [a0, b0, c0, d0].iter().enumerate() {
        r[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    r
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn empreintes(t: &[u8]) -> Vec<(&'static str, String)> {
    use sha2::Digest;
    vec![
        ("MD5 (cassé)", hex(&md5(t))),
        ("SHA-1 (cassé)", hex(&sha1::Sha1::digest(t))),
        ("SHA-256", hex(&sha2::Sha256::digest(t))),
        ("SHA-512", hex(&sha2::Sha512::digest(t))),
    ]
}

/// Bits différents entre deux empreintes (effet avalanche).
pub fn bits_differents(a: &[u8], b: &[u8]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (x ^ y).count_ones()).sum()
}

/// Retrouve un code ou un petit mot à partir de son empreinte (pour montrer pourquoi il faut du sel et des mots de passe longs).
pub fn casser(cible: &str, limite: Duration) -> Option<(String, u64)> {
    use sha2::Digest;
    let cible = cible.trim().to_lowercase();
    let debut = Instant::now();
    let mut essais = 0u64;
    let ok = |s: &str| -> bool {
        match cible.len() {
            32 => hex(&md5(s.as_bytes())) == cible,
            40 => hex(&sha1::Sha1::digest(s.as_bytes())) == cible,
            64 => hex(&sha2::Sha256::digest(s.as_bytes())) == cible,
            _ => false,
        }
    };
    for n in 1..=6u32 {
        for i in 0..10u64.pow(n) {
            let s = format!("{i:0width$}", width = n as usize);
            essais += 1;
            if ok(&s) {
                return Some((s, essais));
            }
        }
    }
    let alpha = b"abcdefghijklmnopqrstuvwxyz";
    for n in 1..=5usize {
        let total = 26u64.pow(n as u32);
        for mut i in 0..total {
            let mut s = vec![0u8; n];
            for c in s.iter_mut().rev() {
                *c = alpha[(i % 26) as usize];
                i /= 26;
            }
            essais += 1;
            if ok(std::str::from_utf8(&s).unwrap_or("")) {
                return Some((String::from_utf8_lossy(&s).to_string(), essais));
            }
            if essais % 100_000 == 0 && debut.elapsed() > limite {
                return None;
            }
        }
    }
    None
}

/// Deux textes différents dont les `bits` premiers bits du SHA-256 sont identiques (paradoxe des anniversaires).
pub fn collision(bits: u32) -> Option<(String, String, u64)> {
    use sha2::Digest;
    let mut vus: std::collections::HashMap<u64, String> = std::collections::HashMap::new();
    for i in 0..(1u64 << (bits / 2 + 6)) {
        let s = format!("outils-{i}");
        let h = sha2::Sha256::digest(s.as_bytes());
        let v = u64::from_be_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]]) >> (64 - bits);
        if let Some(a) = vus.insert(v, s.clone()) {
            return Some((a, s, i + 1));
        }
    }
    None
}

pub struct Hachage {
    texte: String,
    sel: String,
    cible: String,
    resultat: Arc<Mutex<String>>,
    collision: Option<(String, String, u64)>,
}

impl Hachage {
    pub fn new() -> Self {
        Hachage { texte: "bonjour".into(), sel: String::new(), cible: String::new(), resultat: Arc::default(), collision: None }
    }
}

impl Outil for Hachage {
    fn nom(&self) -> &'static str {
        "#️⃣ Labo de hachage"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Labo de hachage");
        ui.weak("Une empreinte (hash) transforme n'importe quel texte en une suite fixe. Les sites gardent l'empreinte de ton mot de passe, pas le mot de passe. Expériences :");
        ui.horizontal(|ui| {
            ui.label("Texte :");
            ui.add(egui::TextEdit::singleline(&mut self.texte).desired_width(240.0));
            ui.label("Sel :");
            ui.add(egui::TextEdit::singleline(&mut self.sel).desired_width(100.0)).on_hover_text("Ajouté avant le texte : le même mot de passe donne une empreinte différente pour chaque compte");
        });
        let entree = format!("{}{}", self.sel, self.texte);
        for (n, h) in empreintes(entree.as_bytes()) {
            ui.horizontal(|ui| {
                ui.add_sized([110.0, 18.0], egui::Label::new(n));
                ui.monospace(h);
            });
        }
        // avalanche : une seule lettre change
        use sha2::Digest;
        let mut autre = entree.clone().into_bytes();
        if let Some(d) = autre.last_mut() {
            *d ^= 1;
        }
        let (a, b) = (sha2::Sha256::digest(entree.as_bytes()), sha2::Sha256::digest(&autre));
        ui.weak(format!("Effet avalanche : en changeant un seul bit du texte, {} bits sur 256 changent dans le SHA-256 (environ la moitié, c'est voulu).", bits_differents(&a, &b)));
        ui.separator();
        ui.strong("🔓 Retrouver un texte depuis son empreinte");
        ui.weak("Colle une empreinte MD5, SHA-1 ou SHA-256 : Outils essaie tous les codes jusqu'à 6 chiffres et tous les mots jusqu'à 5 lettres. C'est pour ça qu'un code PIN ou un mot court ne protège rien.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.cible).desired_width(420.0).font(egui::TextStyle::Monospace));
            if ui.button("Casser").clicked() {
                let (c, r) = (self.cible.clone(), self.resultat.clone());
                *r.lock().unwrap() = "Recherche…".into();
                std::thread::spawn(move || {
                    let t = Instant::now();
                    *r.lock().unwrap() = match casser(&c, Duration::from_secs(60)) {
                        Some((s, n)) => format!("✔ Trouvé : « {s} » en {n} essais et {:.1} s", t.elapsed().as_secs_f32()),
                        None => "✖ Pas trouvé (texte plus long, ou avec du sel) : c'est ce qu'on veut pour un vrai mot de passe !".into(),
                    };
                });
            }
        });
        ui.label(self.resultat.lock().unwrap().clone());
        ui.separator();
        ui.strong("🎂 Collision (paradoxe des anniversaires)");
        ui.horizontal(|ui| {
            if ui.button("Trouver deux textes avec le même début d'empreinte (32 bits)").clicked() {
                self.collision = collision(32);
            }
        });
        if let Some((a, b, n)) = &self.collision {
            ui.monospace(format!("« {a} » et « {b} » : mêmes 32 premiers bits, trouvés en {n} essais (au lieu des 4 milliards qu'on imagine)."));
            ui.weak("Pour 256 bits, il faudrait plus d'essais qu'il n'y a d'atomes sur Terre : c'est ce qui rend SHA-256 sûr. MD5 et SHA-1, eux, ont des collisions connues.");
        }
    }
}

// ===================================================================================================
// 17 · Défis  —  21 · S'entraîner
// ===================================================================================================

pub struct Defi {
    pub titre: &'static str,
    pub enonce: &'static str,
    pub donnees: &'static str,
    pub indice: &'static str,
    pub reponse_sha256: &'static str,
    pub points: u32,
}

pub const DEFIS: &[Defi] = &[
    Defi { titre: "1. Le vieux code de César", enonce: "Jules César décalait chaque lettre de quelques rangs. Quel est le mot qui suit « est » ?", donnees: "oh gudshdx hvw iurpdjh", indice: "Décodeur › détection automatique, ou ROT avec un petit décalage.", reponse_sha256: "a21f0362ace36416398904e6871d84b2f4ad4fa0a0f1cbd1dc1692c48a7318dc", points: 10 },
    Defi { titre: "2. Base64", enonce: "Ce texte finit souvent par « = ». Trouve le drapeau (FLAG{…}).", donnees: "RkxBR3tiYXNlNjRfY2VzdF9mYWNpbGV9", indice: "Bouton « Base64 → » du décodeur.", reponse_sha256: "dc88d7d11afba2b2a48aafd598e045d77763a9175289ca80d56e83e4cc32b383", points: 10 },
    Defi { titre: "3. Hexadécimal", enonce: "Deux caractères (0-9, a-f) par lettre. Trouve le drapeau.", donnees: "464c41477b686578615f6761676e657d", indice: "Bouton « hexa → ».", reponse_sha256: "404387a050ecdd18e173b2ad397071f25047a60613beed3acb1c6e816feadb93", points: 10 },
    Defi { titre: "4. Points et traits", enonce: "Un message de marin. Que dit-il (deux mots) ?", donnees: ".--. .. .-. .- - . / -- .- .-.. .. -.", indice: "Morse : « / » sépare les mots.", reponse_sha256: "eca231b4788fd0e09fb06f147550afa608e5f6c9118763fc8467c16b2b990d8b", points: 10 },
    Defi { titre: "5. Uns et zéros", enonce: "Huit chiffres par lettre. Quel mot ?", donnees: "01101111 01110101 01110100 01101001 01101100 01110011", indice: "Binaire.", reponse_sha256: "13a3c6498bb6ce3a16a8e2b089016b7d91e79410de4ceec983e8c2042876f989", points: 15 },
    Defi { titre: "6. Vigenère", enonce: "Un chiffrement à clé. La clé est le mot français pour « key ». Quel est le message (en un mot) ?", donnees: "tprfpdxzyulqkyyke", indice: "Clé : « cle », bouton « Vigenère déchiffrer ».", reponse_sha256: "8e2991116a737982011fe7f4409d853cd0fb58ee1045648e5f75a3313d9b085c", points: 20 },
    Defi { titre: "7. XOR mystère", enonce: "Chaque octet a été mélangé (XOR) avec le même nombre secret. Que dit le message ?", donnees: "48584b5c450a49424b475a434544", indice: "La détection automatique essaie les 255 clés possibles.", reponse_sha256: "d3ff2e3185b2b272ee3872a980a6e73637075dd53ad7d354a6495b99ff7f5e2d", points: 25 },
    Defi { titre: "8. Code PIN", enonce: "Voici l'empreinte SHA-256 d'un code à 4 chiffres. Quel est le code ?", donnees: "a388f562e286fdf28986f9253579f4d096446e01dd0c771996a51ff11b390fa2", indice: "Labo de hachage › « Casser ».", reponse_sha256: "a388f562e286fdf28986f9253579f4d096446e01dd0c771996a51ff11b390fa2", points: 25 },
    Defi { titre: "9. Poupées russes", enonce: "Deux codages l'un dans l'autre. Quel est le message ?", donnees: "cG5wdXIgcG5wdXI=", indice: "Base64 d'abord… puis regarde les lettres.", reponse_sha256: "56266b80340255549939b6b605cd4c5711342f0ea5081bd3a5e0033ddc509479", points: 25 },
    Defi { titre: "10. Réseau", enonce: "Quelle est l'adresse de diffusion (broadcast) du réseau de 192.168.10.37/27 ?", donnees: "192.168.10.37/27", indice: "Réseau › Calculatrice réseau.", reponse_sha256: "34b69cf0216ba188acb14740477294de309b9b0ea6159476a96c9520f0f3f41c", points: 20 },
    Defi { titre: "11. Le miroir", enonce: "L'alphabet est retourné (a↔z, b↔y…). Où est le trésor ? (les 3 derniers mots)", donnees: "ov givhli vhg hlfh ov xsvmv", indice: "Atbash.", reponse_sha256: "533aa19ff1cb3dbdbe337cf905a3cece7008f8c6dc1033dfed0366b30608804f", points: 15 },
    Defi { titre: "12. L'image bavarde", enonce: "Enregistre l'image du défi : un drapeau FLAG{…} est caché dans ses pixels.", donnees: "(bouton « Enregistrer l'image »)", indice: "Fun › Stéganographie › Révéler.", reponse_sha256: "21f7a09495f6234b03bd8690db33e2e3a75b9b17641cee06de5903719958b6bf", points: 30 },
    Defi { titre: "13. Ce que l'image ne montre pas", enonce: "Enregistre l'image : le mot de passe est écrit dans ses métadonnées (pas dans les pixels).", donnees: "(bouton « Enregistrer l'image »)", indice: "Fichiers › Visionneuse hexadécimale › Texte trouvé dedans (ou Métadonnées cachées).", reponse_sha256: "4d0b2b722ee70934df8d98a66fb600b36374ecf32bbefc9c1f4ff47729b8d382", points: 30 },
];

fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in d {
        c ^= *b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// Ajoute un bloc texte (tEXt) juste après l'en-tête d'un PNG.
pub fn png_avec_texte(png: &[u8], cle: &str, valeur: &str) -> Vec<u8> {
    let mut chunk = b"tEXt".to_vec();
    chunk.extend_from_slice(cle.as_bytes());
    chunk.push(0);
    chunk.extend_from_slice(valeur.as_bytes());
    let mut o = png[..33].to_vec(); // signature + IHDR
    o.extend_from_slice(&((chunk.len() - 4) as u32).to_be_bytes());
    o.extend_from_slice(&chunk);
    o.extend_from_slice(&crc32(&chunk).to_be_bytes());
    o.extend_from_slice(&png[33..]);
    o
}

fn image_defi(n: u32) -> image::RgbaImage {
    image::RgbaImage::from_fn(320, 200, |x, y| {
        let v = ((x * 7 + y * 3 + n * 40) % 256) as u8;
        image::Rgba([v, (x % 256) as u8 / 2 + 60, (y % 256) as u8 + 30, 255])
    })
}

fn enregistrer_image_defi(k: usize) -> Result<PathBuf, String> {
    let d = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads");
    let mut img = image_defi(k as u32);
    let p = d.join(format!("defi_outils_{}.png", k + 1));
    if k == 11 {
        cacher(&mut img, b"FLAG{pixels_bavards}", "")?;
        img.save(&p).map_err(|e| e.to_string())?;
    } else {
        let mut b = std::io::Cursor::new(Vec::new());
        img.write_to(&mut b, image::ImageFormat::Png).map_err(|e| e.to_string())?;
        std::fs::write(&p, png_avec_texte(&b.into_inner(), "Comment", "le mot de passe est licorne")).map_err(|e| e.to_string())?;
    }
    Ok(p)
}

pub fn bonne_reponse(r: &str, attendu: &str) -> bool {
    use sha2::Digest;
    let n = r.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    hex(&sha2::Sha256::digest(n.as_bytes())) == attendu
}

pub struct Defis {
    resolus: Vec<usize>,
    reponses: Vec<String>,
    indices: Vec<bool>,
    message: String,
}

impl Defis {
    pub fn new() -> Self {
        Defis { resolus: charger("defis_ctf"), reponses: vec![String::new(); DEFIS.len()], indices: vec![false; DEFIS.len()], message: String::new() }
    }
}

pub fn points(resolus: &[usize]) -> u32 {
    resolus.iter().filter_map(|k| DEFIS.get(*k)).map(|d| d.points).sum()
}

impl Outil for Defis {
    fn nom(&self) -> &'static str {
        "🏴 Défis de sécurité"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Défis de sécurité (CTF)");
        let total: u32 = DEFIS.iter().map(|d| d.points).sum();
        let p = points(&self.resolus);
        ui.label(RichText::new(format!("🏆 {p} / {total} points · {} / {} défis", self.resolus.len(), DEFIS.len())).size(17.0).strong());
        ui.add(egui::ProgressBar::new(p as f32 / total as f32).desired_width(300.0));
        let rang = match p * 100 / total.max(1) {
            0..=19 => "🐣 Débutant",
            20..=49 => "🔍 Apprenti",
            50..=79 => "🕵 Enquêteur",
            80..=99 => "🥷 Expert",
            _ => "👑 Maître du CTF",
        };
        ui.label(format!("Rang : {rang}"));
        ui.weak("Utilise les outils d'Outils (Décodeur CTF, Labo de hachage, Stéganographie, Calculatrice réseau, Visionneuse hexadécimale) pour trouver les réponses.");
        ui.label(&self.message);
        egui::ScrollArea::vertical().id_salt("defis").show(ui, |ui| {
            for (k, d) in DEFIS.iter().enumerate() {
                let fait = self.resolus.contains(&k);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.strong(format!("{} {}", if fait { "✅" } else { "🔒" }, d.titre));
                        ui.weak(format!("{} pts", d.points));
                    });
                    ui.label(d.enonce);
                    ui.horizontal(|ui| {
                        ui.monospace(d.donnees);
                        if !d.donnees.starts_with('(') && ui.small_button("📋").clicked() {
                            ui.ctx().copy_text(d.donnees.to_string());
                        }
                        if (k == 11 || k == 12) && ui.small_button("💾 Enregistrer l'image").clicked() {
                            self.message = match enregistrer_image_defi(k) {
                                Ok(p) => format!("Image enregistrée : {}", p.display()),
                                Err(e) => format!("✖ {e}"),
                            };
                        }
                    });
                    if !fait {
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(&mut self.reponses[k]).hint_text("ta réponse").desired_width(220.0));
                            if ui.button("Valider").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                if bonne_reponse(&self.reponses[k], d.reponse_sha256) {
                                    self.resolus.push(k);
                                    enregistrer("defis_ctf", &self.resolus);
                                    self.message = format!("🎉 Bravo ! +{} points", d.points);
                                } else {
                                    self.message = "✖ Pas encore… regarde l'indice".into();
                                }
                            }
                            if ui.small_button("💡 Indice").clicked() {
                                self.indices[k] = !self.indices[k];
                            }
                        });
                        if self.indices[k] {
                            ui.weak(format!("💡 {}", d.indice));
                        }
                    }
                });
            }
        });
        if !self.resolus.is_empty() && ui.small_button("Recommencer à zéro").clicked() {
            self.resolus.clear();
            enregistrer("defis_ctf", &self.resolus);
        }
    }
}

const PLATEFORMES: &[(&str, &str, &str, &str)] = &[
    ("Root-Me", "https://www.root-me.org", "🇫🇷 en français, des centaines de défis classés par thème (web, crypto, réseau, stéganographie…)", "débutant → expert"),
    ("TryHackMe", "https://tryhackme.com", "cours guidés pas à pas avec des machines virtuelles dans le navigateur", "débutant"),
    ("PicoCTF", "https://picoctf.org", "CTF gratuit pensé pour les collégiens et lycéens", "débutant"),
    ("Hack The Box", "https://www.hackthebox.com", "machines réalistes à attaquer (légalement, elles sont faites pour ça)", "intermédiaire → expert"),
    ("OverTheWire", "https://overthewire.org/wargames/", "« Bandit » : apprendre la ligne de commande Linux en jouant", "débutant"),
    ("Passe ton hack d'abord", "https://www.passetonhackdabord.fr", "🇫🇷 concours national pour les lycéens", "débutant"),
    ("CTFtime", "https://ctftime.org", "calendrier de toutes les compétitions CTF du monde, à faire en équipe", "tous niveaux"),
    ("ANSSI · MOOC SecNumacadémie", "https://secnumacademie.gouv.fr", "🇫🇷 cours officiel et gratuit de l'État sur la cybersécurité", "débutant"),
];

const PARCOURS: &[&str] = &[
    "Finir les défis d'Outils",
    "Apprendre les bases de Linux (OverTheWire Bandit, niveaux 0 à 10)",
    "Comprendre le réseau : adresses IP, ports, DNS (Calculatrice réseau, Scanner de ports chez toi)",
    "Faire le parcours « Pre Security » de TryHackMe",
    "Résoudre 10 défis Root-Me (catégories Cryptanalyse et Stéganographie)",
    "Apprendre un langage : Python (très utilisé en sécurité)",
    "Comprendre le web : HTML, requêtes HTTP, cookies (défis « Web - Client » de Root-Me)",
    "Participer à PicoCTF ou à un CTF en équipe sur CTFtime",
    "Lire les guides de l'ANSSI (SecNumacadémie)",
];

pub struct Entrainement {
    fait: Vec<usize>,
}

impl Entrainement {
    pub fn new() -> Self {
        Entrainement { fait: charger("parcours_ctf") }
    }
}

impl Outil for Entrainement {
    fn nom(&self) -> &'static str {
        "🎓 S'entraîner au hacking (légal)"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("S'entraîner au hacking, légalement");
        ui.label("La règle d'or : on ne s'attaque qu'à ce qui est à soi, ou à ce qui est fait pour ça. S'introduire dans un système qui n'est pas à toi (même « pour voir ») est un délit (articles 323-1 et suivants du Code pénal). Les plateformes ci-dessous sont faites pour apprendre sans risque.");
        ui.separator();
        for (n, u, d, niveau) in PLATEFORMES {
            ui.horizontal_wrapped(|ui| {
                if ui.link(RichText::new(*n).strong()).clicked() {
                    let _ = std::process::Command::new("explorer").arg(*u).spawn();
                }
                ui.label(*d);
                ui.weak(format!("({niveau})"));
            });
        }
        ui.separator();
        let resolus: Vec<usize> = charger("defis_ctf");
        ui.strong(format!("🗺 Ton parcours · défis d'Outils : {} / {}", resolus.len(), DEFIS.len()));
        let mut change = false;
        for (k, e) in PARCOURS.iter().enumerate() {
            let mut c = self.fait.contains(&k) || (k == 0 && resolus.len() == DEFIS.len());
            if ui.checkbox(&mut c, *e).changed() {
                if c {
                    self.fait.push(k);
                } else {
                    self.fait.retain(|x| *x != k);
                }
                change = true;
            }
        }
        if change {
            enregistrer("parcours_ctf", &self.fait);
        }
        ui.add(egui::ProgressBar::new(self.fait.len() as f32 / PARCOURS.len() as f32).desired_width(300.0).show_percentage());
        ui.weak("Plus tard, les métiers : pentesteur (on te paie pour tester des systèmes, avec autorisation), analyste SOC, chasseur de primes de bugs (bug bounty sur YesWeHack, plateforme française).");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodages() {
        assert_eq!(rot("abc XYZ", 3), "def ABC");
        assert_eq!(atbash("bonjour"), "ylmqlfi");
        assert_eq!(vigenere(&vigenere("rendezvous", "cle", false), "cle", true), "rendezvous");
        assert_eq!(depuis_morse(".--. .. .-. .- - . / -- .- .-.. .. -.").unwrap(), "pirate malin");
        assert_eq!(vers_morse("sos"), "... --- ...");
        assert_eq!(depuis_binaire("01101111 01101011").unwrap(), b"ok");
        assert_eq!(depuis_hex("4f4b").unwrap(), b"OK");
        assert_eq!(depuis_base64("RkxBR3tiYXNlNjRfY2VzdF9mYWNpbGV9").unwrap(), b"FLAG{base64_cest_facile}");
        let d = detecter("oh gudshdx hvw iurpdjh");
        assert!(d.iter().take(3).any(|x| x.1 == "le drapeau est fromage"), "{:?}", &d[..3]);
        let x = detecter("48584b5c450a49424b475a434544");
        assert!(x.iter().any(|y| y.1 == "bravo champion"));
        let p = detecter("cG5wdXIgcG5wdXI=");
        assert!(p.iter().any(|y| y.1 == "cache cache"), "{p:?}");
    }

    #[test]
    fn stegano_reseau_hachage() {
        let mut img = image_defi(1);
        cacher(&mut img, b"FLAG{pixels_bavards}", "").unwrap();
        assert_eq!(reveler(&img, "").unwrap(), b"FLAG{pixels_bavards}");
        let mut i2 = image_defi(2);
        cacher(&mut i2, b"secret", "mdp").unwrap();
        assert_eq!(reveler(&i2, "mdp").unwrap(), b"secret");
        assert_ne!(reveler(&i2, "faux").unwrap(), b"secret");
        assert!(reveler(&image_defi(3), "").is_none());
        let s = calculer("192.168.10.37/27").unwrap();
        assert_eq!(s.diffusion.to_string(), "192.168.10.63");
        assert_eq!(s.reseau.to_string(), "192.168.10.32");
        assert_eq!(s.hotes, 30);
        assert_eq!(calculer("10.0.0.5/255.255.255.0").unwrap().masque.to_string(), "255.255.255.0");
        assert_eq!(hex(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
        assert_eq!(casser("a388f562e286fdf28986f9253579f4d096446e01dd0c771996a51ff11b390fa2", Duration::from_secs(20)).unwrap().0, "4821");
        let (a, b, _) = collision(20).unwrap();
        assert_ne!(a, b);
        for (k, d) in DEFIS.iter().enumerate() {
            assert_eq!(d.reponse_sha256.len(), 64, "défi {k}");
        }
        assert!(bonne_reponse("  Fromage ", DEFIS[0].reponse_sha256));
        assert!(bonne_reponse("FLAG{pixels_bavards}", DEFIS[11].reponse_sha256));
        assert!(bonne_reponse("192.168.10.63", DEFIS[9].reponse_sha256));
        let mut b = std::io::Cursor::new(Vec::new());
        image_defi(0).write_to(&mut b, image::ImageFormat::Png).unwrap();
        let png = png_avec_texte(&b.into_inner(), "Comment", "le mot de passe est licorne");
        let relue = image::load_from_memory(&png).unwrap();
        assert_eq!(relue.width(), 320);
        assert!(memchr::memmem::find(&png, b"licorne").is_some());
    }
}
