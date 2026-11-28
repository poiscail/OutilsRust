//! « Mon PC » : le téléphone pilote Outils sur PC (même Wi-Fi).
//! Télécommande (1), état du PC (2), fichiers du PC (3), envoi vers le PC (4), presse-papiers (5),
//! Minecraft (6), notifications du PC (7), synchronisation agenda / devoirs / comptes à rebours (8).
use crate::commun::*;
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const PORT_DECOUVERTE: u16 = 8643;
pub const SYNCHRO: [(&str, &str); 4] = [("agenda", "📅 Agenda"), ("ecole", "🎒 Emploi du temps et devoirs"), ("rebours", "⏳ Comptes à rebours"), ("sante", "💧 Eau et calories")];

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub struct Pc {
    pub ip: String,
    pub port: u16,
    pub cle: String,
    pub nom: String,
    pub macs: Vec<String>,
    /// accès de l'extérieur (relais ntfy.sh, messages chiffrés)
    pub distance_actif: bool,
    pub distance_sujet: String,
    pub distance_cle: String,
}

// ---------------------------------------------------------------------------------------------------
// Messages chiffrés pour l'accès de l'extérieur (mêmes réglages que le PC)
// ---------------------------------------------------------------------------------------------------

pub fn chiffrer(cle_b64: &str, texte: &str) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let k = B64.decode(cle_b64).ok()?;
    let c = aes_gcm::Aes256Gcm::new_from_slice(&k).ok()?;
    let mut n = [0u8; 12];
    let _ = getrandom::getrandom(&mut n);
    let mut v = n.to_vec();
    v.extend(c.encrypt(aes_gcm::Nonce::from_slice(&n), texte.as_bytes()).ok()?);
    Some(B64.encode(v))
}

pub fn dechiffrer(cle_b64: &str, b64: &str) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let k = B64.decode(cle_b64).ok()?;
    let b = B64.decode(b64.trim()).ok()?;
    if b.len() < 28 {
        return None;
    }
    let c = aes_gcm::Aes256Gcm::new_from_slice(&k).ok()?;
    String::from_utf8(c.decrypt(aes_gcm::Nonce::from_slice(&b[..12]), &b[12..]).ok()?).ok()
}

pub fn decoder(s: &str) -> String {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hexa = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hexa(b[i + 1]), hexa(b[i + 2])) {
                o.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        o.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

/// « /cmd/mc_dire?texte=Salut%20!» → « mc_dire Salut ! » ; « /api/lancer?id=730 » → « lancer 730 ».
pub fn commande_relais(chemin: &str) -> String {
    let (c, q) = chemin.split_once('?').unwrap_or((chemin, ""));
    let nom = c.trim_start_matches("/cmd/").trim_start_matches("/api/");
    let valeur = q.split('&').find_map(|x| x.split_once('=').map(|y| y.1)).map(decoder).unwrap_or_default();
    if valeur.is_empty() { nom.to_string() } else { format!("{nom} {valeur}") }
}

/// Envoie une commande au PC par le relais (quand on n'est pas à la maison).
pub fn commande_distance(p: &Pc, cmd: &str) -> Result<(), String> {
    if !p.distance_actif || p.distance_sujet.is_empty() {
        return Err("accès à distance non activé sur le PC (Outils › Téléphone)".into());
    }
    let c = chiffrer(&p.distance_cle, cmd).ok_or("clé invalide")?;
    ureq::post(&format!("https://ntfy.sh/{}-pc", p.distance_sujet)).timeout(Duration::from_secs(15)).send_string(&c).map_err(|e| e.to_string())?;
    Ok(())
}

/// Dernières réponses et notifications reçues par le relais.
pub static DISTANCE_RECU: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

pub fn encoder(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

impl Pc {
    pub fn url(&self, chemin: &str) -> String {
        let sep = if chemin.contains('?') { '&' } else { '?' };
        format!("http://{}:{}{}{}k={}", self.ip, self.port, chemin, sep, self.cle)
    }

    fn agent(&self, secondes: u64) -> ureq::Agent {
        ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(4)).timeout(Duration::from_secs(secondes)).build()
    }

    pub fn get_json(&self, chemin: &str) -> Result<Value, String> {
        self.agent(20).get(&self.url(chemin)).call().map_err(erreur)?.into_json().map_err(|e| e.to_string())
    }

    pub fn get_texte(&self, chemin: &str) -> Result<String, String> {
        self.agent(20).get(&self.url(chemin)).call().map_err(erreur)?.into_string().map_err(|e| e.to_string())
    }

    pub fn post(&self, chemin: &str, corps: &[u8]) -> Result<String, String> {
        self.agent(30).post(&self.url(chemin)).send_bytes(corps).map_err(erreur)?.into_string().map_err(|e| e.to_string())
    }
}

fn erreur(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(403, _) => "clé refusée : associe à nouveau le téléphone".into(),
        ureq::Error::Status(c, r) => format!("erreur {c} : {}", r.into_string().unwrap_or_default()),
        ureq::Error::Transport(t) => format!("PC injoignable ({}) : Outils est-il ouvert, sur le même Wi-Fi ?", t.kind()),
    }
}

/// Adresse lue dans le QR code du PC : « http://192.168.1.20:8642/?k=abc… ».
pub fn lire_adresse(t: &str) -> Option<Pc> {
    let r = t.trim().strip_prefix("http://")?;
    let (hote, reste) = r.split_once('/').unwrap_or((r, ""));
    let (ip, port) = hote.split_once(':')?;
    let cle = reste.split(['?', '&']).find_map(|x| x.strip_prefix("k="))?;
    Some(Pc { ip: ip.into(), port: port.parse().ok()?, cle: cle.into(), ..Default::default() })
}

/// Cherche les PC avec Outils sur le Wi-Fi : (ip, port, nom).
pub fn chercher_pcs() -> Vec<(String, u16, String)> {
    let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return vec![] };
    let _ = s.set_broadcast(true);
    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
    let mut cibles = vec!["255.255.255.255".to_string()];
    if let Some(ip) = crate::telephone::ip_locale() {
        if let Some((a, _)) = ip.rsplit_once('.') {
            cibles.push(format!("{a}.255"));
        }
    }
    let mut v: Vec<(String, u16, String)> = vec![];
    let debut = Instant::now();
    let mut b = [0u8; 512];
    while debut.elapsed() < Duration::from_millis(1800) {
        if debut.elapsed().as_millis() % 600 < 320 {
            for c in &cibles {
                let _ = s.send_to(b"OUTILS?", (c.as_str(), PORT_DECOUVERTE));
            }
        }
        if let Ok((n, de)) = s.recv_from(&mut b) {
            if let Ok(j) = serde_json::from_slice::<Value>(&b[..n]) {
                let ip = de.ip().to_string();
                if j["outils"] == 1 && !v.iter().any(|x| x.0 == ip) {
                    v.push((ip, j["port"].as_u64().unwrap_or(8642) as u16, j["nom"].as_str().unwrap_or("PC").to_string()));
                }
            }
        }
    }
    v
}

/// Réveille le PC à distance (Wake-on-LAN) : paquet magique à chaque adresse MAC connue.
pub fn reveiller(macs: &[String]) -> Result<(), String> {
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_broadcast(true).map_err(|e| e.to_string())?;
    let mut fait = false;
    for m in macs {
        let o: Vec<u8> = m.split([':', '-']).filter_map(|x| u8::from_str_radix(x, 16).ok()).collect();
        if o.len() != 6 {
            continue;
        }
        let mut p = vec![0xFFu8; 6];
        for _ in 0..16 {
            p.extend_from_slice(&o);
        }
        for port in [9, 7] {
            let _ = s.send_to(&p, ("255.255.255.255", port));
        }
        fait = true;
    }
    if fait { Ok(()) } else { Err("adresse MAC du PC inconnue : connecte-toi une fois au PC allumé".into()) }
}

// ---------------------------------------------------------------------------------------------------
// Notifications du PC (fil en arrière-plan)
// ---------------------------------------------------------------------------------------------------

fn suivre_notifs(pc: Arc<Mutex<Option<Pc>>>) {
    std::thread::spawn(move || {
        let mut depuis: Option<u64> = None;
        let mut tour = 0u32;
        let mut relais_depuis = "30s".to_string();
        loop {
            tour += 1;
            let p = pc.lock().unwrap().clone();
            if let Some(p) = p {
                // à la maison : fichiers envoyés par le PC, « faire sonner »
                match p.get_json("/api/pour_tel") {
                    Ok(v) => {
                        for c in v.as_array().into_iter().flatten() {
                            match c["type"].as_str() {
                                Some("sonner") => {
                                    crate::android::sonner(30);
                                    crate::android::notifier(1999, "🔔 Ton PC te cherche", "Le téléphone sonne depuis Outils sur PC");
                                }
                                Some("fichier") => {
                                    let (chemin, nom) = (c["chemin"].as_str().unwrap_or("").to_string(), c["nom"].as_str().unwrap_or("fichier").to_string());
                                    let p2 = p.clone();
                                    std::thread::spawn(move || {
                                        let r = ureq::get(&p2.url(&format!("/api/fichier?chemin={}", encoder(&chemin)))).timeout(Duration::from_secs(3600)).call();
                                        if let Ok(r) = r {
                                            let dest = libre(&dossier_outils(), &nom);
                                            if let Ok(mut f) = std::fs::File::create(&dest) {
                                                if std::io::copy(&mut r.into_reader(), &mut f).is_ok() {
                                                    crate::android::signaler_media(&[dest.display().to_string()]);
                                                    crate::android::notifier(2999, "📥 Reçu du PC", &format!("{nom} (Téléchargements › Outils)"));
                                                }
                                            }
                                        }
                                    });
                                }
                                _ => {}
                            }
                        }
                        if tour % 3 == 1 {
                            if let Ok(j) = p.get_json(&format!("/api/notifs?depuis={}", depuis.unwrap_or(u64::MAX))) {
                                let dernier = j["dernier"].as_u64().unwrap_or(0);
                                // premier passage : on ne renvoie pas les anciennes notifications
                                if depuis.is_some() {
                                    for n in j["liste"].as_array().into_iter().flatten() {
                                        let (t, m) = (n["titre"].as_str().unwrap_or(""), n["texte"].as_str().unwrap_or(""));
                                        crate::android::notifier(2000 + (n["n"].as_u64().unwrap_or(0) % 1000) as i32, &format!("🖥 {t}"), m);
                                        crate::systeme::notifier_toujours(&format!("🖥 {t}"), m);
                                    }
                                }
                                depuis = Some(dernier);
                            }
                        }
                    }
                    // pas à la maison : relais (réponses et notifications chiffrées)
                    Err(_) if p.distance_actif && !p.distance_sujet.is_empty() => {
                        let url = format!("https://ntfy.sh/{}-tel/json?poll=1&since={relais_depuis}", p.distance_sujet);
                        if let Ok(r) = ureq::get(&url).timeout(Duration::from_secs(20)).call() {
                            for l in r.into_string().unwrap_or_default().lines() {
                                let Ok(m) = serde_json::from_str::<Value>(l) else { continue };
                                if let Some(id) = m["id"].as_str() {
                                    relais_depuis = id.to_string();
                                }
                                let Some(j) = m["message"].as_str().and_then(|t| dechiffrer(&p.distance_cle, t)).and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
                                if let Some(t) = j["notif"].as_str() {
                                    crate::android::notifier(2500, &format!("🖥 {t}"), j["texte"].as_str().unwrap_or(""));
                                }
                                let mut v = DISTANCE_RECU.lock().unwrap();
                                v.push(j);
                                let n = v.len();
                                if n > 20 {
                                    v.drain(..n - 20);
                                }
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            std::thread::sleep(Duration::from_secs(8));
        }
    });
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

pub struct MonPc {
    pc: Arc<Mutex<Option<Pc>>>,
    onglet: usize,
    e: Etat,
    // appairage
    trouves: Arc<Mutex<Option<Vec<(String, u16, String)>>>>,
    choisi: Option<(String, u16, String)>,
    code: String,
    manuel: String,
    // état, jeux, fichiers
    infos: Arc<Mutex<Option<Value>>>,
    infos_quand: Instant,
    jeux: Arc<Mutex<Vec<(String, String)>>>,
    q: String,
    resultats: Arc<Mutex<Option<Value>>>,
    // envoi, presse-papiers, Minecraft
    nav: crate::fichiers_tel::Navigateur,
    texte: String,
    message_mc: String,
}

impl MonPc {
    pub fn new() -> Self {
        let p: Option<Pc> = charger("pc");
        let pc = Arc::new(Mutex::new(p.filter(|p| !p.ip.is_empty())));
        suivre_notifs(pc.clone());
        MonPc {
            pc, onglet: 0, e: Arc::default(), trouves: Arc::default(), choisi: None, code: String::new(), manuel: String::new(), infos: Arc::default(), infos_quand: Instant::now() - Duration::from_secs(60),
            jeux: Arc::default(), q: String::new(), resultats: Arc::default(), nav: crate::fichiers_tel::Navigateur::new(), texte: String::new(), message_mc: String::new(),
        }
    }

    /// Associe le PC donné par le QR code (appelé par le scanner).
    pub fn associer_adresse(t: &str) -> bool {
        match lire_adresse(t) {
            Some(mut p) => {
                if let Ok(j) = p.get_json("/api/etat") {
                    p.nom = j["nom"].as_str().unwrap_or("PC").into();
                }
                enregistrer("pc", &Some(p));
                *A_RELIRE.lock().unwrap() = true;
                true
            }
            None => false,
        }
    }

    fn pc(&self) -> Option<Pc> {
        self.pc.lock().unwrap().clone()
    }

    fn action(&self, chemin: &str) {
        let Some(p) = self.pc() else { return };
        let c = chemin.to_string();
        let injoignable = self.infos.lock().unwrap().as_ref().is_some_and(|j| j.is_null());
        en_fond(&self.e, "…", move || {
            crate::android::vibrer(25);
            match p.post(&c, b"") {
                Ok(r) => Ok(r),
                // pas à la maison : par le relais chiffré
                Err(_) if injoignable && p.distance_actif => {
                    let cmd = commande_relais(&c);
                    commande_distance(&p, &cmd)?;
                    Ok("🌍 Envoyé par l'accès à distance".into())
                }
                Err(e) => Err(e),
            }
        });
    }

    fn rafraichir(&mut self) {
        let Some(p) = self.pc() else { return };
        self.infos_quand = Instant::now();
        let (i, pc) = (self.infos.clone(), self.pc.clone());
        std::thread::spawn(move || {
            if let Ok(d) = p.get_json("/api/distance") {
                let mut q = pc.lock().unwrap().clone().unwrap_or_else(|| p.clone());
                let (a, s, k) = (d["actif"].as_bool().unwrap_or(false), d["sujet"].as_str().unwrap_or("").to_string(), d["cle"].as_str().unwrap_or("").to_string());
                if (a, &s, &k) != (q.distance_actif, &q.distance_sujet, &q.distance_cle) {
                    (q.distance_actif, q.distance_sujet, q.distance_cle) = (a, s, k);
                    enregistrer("pc", &Some(q.clone()));
                    *pc.lock().unwrap() = Some(q);
                }
            }
            if let Ok(j) = p.get_json("/api/etat") {
                // on garde les adresses MAC pour réveiller le PC plus tard
                let macs: Vec<String> = j["macs"].as_array().into_iter().flatten().filter_map(|m| m.as_str().map(String::from)).collect();
                if !macs.is_empty() && macs != p.macs {
                    let mut q = p.clone();
                    q.macs = macs;
                    enregistrer("pc", &Some(q.clone()));
                    *pc.lock().unwrap() = Some(q);
                }
                *i.lock().unwrap() = Some(j);
            } else {
                *i.lock().unwrap() = Some(Value::Null);
            }
        });
    }

    fn ui_associer(&mut self, ui: &mut egui::Ui) {
        ui.heading("Associer ton PC");
        ui.weak("Sur le PC : Outils › Téléphone › Démarrer, puis « Associer un téléphone ». Le téléphone et le PC doivent être sur le même Wi-Fi.");
        let occupe = etat_ui(ui, &self.e);
        if gros_bouton(ui, "🔎 Chercher mon PC sur le Wi-Fi") && !occupe {
            let t = self.trouves.clone();
            *t.lock().unwrap() = None;
            en_fond(&self.e, "Recherche…", move || {
                let v = chercher_pcs();
                let n = v.len();
                *t.lock().unwrap() = Some(v);
                Ok(if n == 0 { "Aucun PC trouvé : démarre « Téléphone » dans Outils sur le PC.".into() } else { String::new() })
            });
        }
        if let Some(v) = self.trouves.lock().unwrap().clone() {
            for x in v {
                if ui.selectable_label(self.choisi.as_ref() == Some(&x), RichText::new(format!("🖥 {}  ({})", x.2, x.0)).size(18.0)).clicked() {
                    self.choisi = Some(x);
                }
            }
        }
        if let Some((ip, port, nom)) = self.choisi.clone() {
            ui.label(format!("Code à 6 chiffres affiché sur {nom} :"));
            ui.add(egui::TextEdit::singleline(&mut self.code).hint_text("123456").font(egui::FontId::monospace(26.0)).desired_width(180.0));
            if gros_bouton(ui, "✔ Associer") && !occupe {
                let (code, pc) = (self.code.replace(' ', ""), self.pc.clone());
                en_fond(&self.e, "Association…", move || {
                    let url = format!("http://{ip}:{port}/api/appairer?code={}&appareil={}", encoder(&code), encoder(&crate::android::modele()));
                    let j: Value = ureq::get(&url).timeout(Duration::from_secs(8)).call().map_err(erreur)?.into_json().map_err(|e| e.to_string())?;
                    let p = Pc { ip, port, cle: j["cle"].as_str().unwrap_or("").into(), nom: j["nom"].as_str().unwrap_or(&nom).into(), ..Default::default() };
                    enregistrer("pc", &Some(p.clone()));
                    *pc.lock().unwrap() = Some(p);
                    Ok("✔ Téléphone associé !".into())
                });
            }
        }
        ui.add_space(10.0);
        ui.collapsing("Autres façons", |ui| {
            ui.label("📷 Scanner le QR code affiché sur le PC : outil « Scanner QR », il associe tout seul.");
            ui.label("Ou colle l'adresse affichée sous le QR code :");
            ui.add(egui::TextEdit::singleline(&mut self.manuel).hint_text("http://192.168.1.20:8642/?k=…").desired_width(f32::INFINITY));
            if ui.button("Associer avec cette adresse").clicked() {
                if MonPc::associer_adresse(&self.manuel) {
                    *self.pc.lock().unwrap() = charger("pc");
                } else {
                    *self.e.lock().unwrap() = (false, "✖ adresse incomplète".into());
                }
            }
        });
    }
}

/// Le scanner QR a associé un PC : relire la configuration.
pub static A_RELIRE: Mutex<bool> = Mutex::new(false);

impl Outil for MonPc {
    fn nom(&self) -> &'static str {
        "🖥 Mon PC"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if std::mem::take(&mut *A_RELIRE.lock().unwrap()) {
            *self.pc.lock().unwrap() = charger("pc");
        }
        let Some(pc) = self.pc() else {
            self.ui_associer(ui);
            return;
        };
        if self.infos_quand.elapsed() > Duration::from_secs(if self.onglet == 1 { 2 } else { 10 }) {
            self.rafraichir();
        }
        let infos = self.infos.lock().unwrap().clone();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("🖥 {}", if pc.nom.is_empty() { &pc.ip } else { &pc.nom })).size(18.0).strong());
            match &infos {
                Some(Value::Null) => {
                    ui.colored_label(ROUGE, "● injoignable");
                }
                Some(_) => {
                    ui.colored_label(VERT, "● connecté");
                }
                None => {
                    ui.spinner();
                }
            }
        });
        if infos.as_ref().is_some_and(|j| j.is_null()) && pc.distance_actif {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new("🌍 Hors de la maison : accès à distance").strong());
                ui.weak("La télécommande et Minecraft passent par le relais chiffré.");
                if ui.button("Demander l'état du PC").clicked() {
                    let p = pc.clone();
                    en_fond(&self.e, "Demande envoyée…", move || {
                        commande_distance(&p, "etat")?;
                        Ok("Demande envoyée : la réponse arrive en quelques secondes.".into())
                    });
                }
                let dernier = DISTANCE_RECU.lock().unwrap().iter().rev().find(|j| j.get("cpu").is_some()).cloned();
                if let Some(j) = dernier {
                    ui.label(format!("CPU {:.0} % · RAM {:.0} % · {}", j["cpu"].as_f64().unwrap_or(0.0), j["ram"].as_f64().unwrap_or(0.0), j["jeu"].as_str().map(|x| format!("🎮 {x}")).unwrap_or_else(|| "pas de jeu en cours".into())));
                    ui.label(format!("⛏ Minecraft : {}", j["mc"].as_str().unwrap_or("?")));
                }
                for j in DISTANCE_RECU.lock().unwrap().iter().rev().take(3) {
                    if let Some(m) = j["ok"].as_str() {
                        ui.weak(format!("✔ {m}"));
                    }
                }
            });
        }
        if infos.as_ref().is_some_and(|j| j.is_null()) {
            ui.weak("Le PC est éteint, ou Outils n'est pas ouvert dessus (Téléphone › Démarrer).");
            ui.horizontal_wrapped(|ui| {
                if ui.button("⏰ Réveiller le PC").on_hover_text("Wake-on-LAN : à activer dans le BIOS et la carte réseau du PC").clicked() {
                    *self.e.lock().unwrap() = (false, match reveiller(&pc.macs) {
                        Ok(()) => "Signal de réveil envoyé : patiente une trentaine de secondes.".into(),
                        Err(e) => format!("✖ {e}"),
                    });
                }
                if ui.button("Oublier ce PC").clicked() {
                    enregistrer("pc", &None::<Pc>);
                    *self.pc.lock().unwrap() = None;
                }
            });
        }
        onglets(ui, &mut self.onglet, &["🎛 Télécommande", "📊 État", "🎮 Jeux", "🔎 Fichiers", "📤 Envoyer", "📋 Presse-papiers", "⛏ Minecraft", "🔄 Synchro"]);
        let occupe = etat_ui(ui, &self.e);
        match self.onglet {
            0 => {
                let g = |ui: &mut egui::Ui, t: &str| ui.add_sized([ui.available_width() / 3.0 - 8.0, 64.0], egui::Button::new(RichText::new(t).size(20.0))).clicked();
                ui.horizontal_wrapped(|ui| {
                    if g(ui, "🔉 −") {
                        self.action("/cmd/vol_moins");
                    }
                    if g(ui, "🔇") {
                        self.action("/cmd/muet");
                    }
                    if g(ui, "🔊 +") {
                        self.action("/cmd/vol_plus");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if g(ui, "⏮") {
                        self.action("/cmd/precedent");
                    }
                    if g(ui, "⏯") {
                        self.action("/cmd/lecture");
                    }
                    if g(ui, "⏭") {
                        self.action("/cmd/suivant");
                    }
                });
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if g(ui, "🔒 Verrouiller") {
                        self.action("/cmd/verrouiller");
                    }
                    if g(ui, "🌙 Veille") {
                        self.action("/cmd/veille");
                    }
                    if g(ui, "⏰ Réveiller") {
                        *self.e.lock().unwrap() = (false, reveiller(&pc.macs).map(|_| "Signal de réveil envoyé.".to_string()).unwrap_or_else(|e| format!("✖ {e}")));
                    }
                });
            }
            1 => match infos.as_ref().filter(|j| !j.is_null()) {
                Some(j) => {
                    let jauge = |ui: &mut egui::Ui, nom: &str, v: f64, t: f64| {
                        if v < 0.0 {
                            return;
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.add_sized([70.0, 24.0], egui::Label::new(RichText::new(nom).strong()));
                            let c = if v > 85.0 { ROUGE } else if v > 60.0 { ORANGE } else { VERT };
                            ui.add(egui::ProgressBar::new((v / 100.0) as f32).fill(c).desired_width(ui.available_width() - 70.0).text(format!("{v:.0} %")));
                            if t > 0.0 {
                                ui.label(format!("{t:.0}°C"));
                            }
                        });
                    };
                    jauge(ui, "CPU", j["cpu"].as_f64().unwrap_or(-1.0), j["cpu_t"].as_f64().unwrap_or(-1.0));
                    jauge(ui, "GPU", j["gpu"].as_f64().unwrap_or(-1.0), j["gpu_t"].as_f64().unwrap_or(-1.0));
                    jauge(ui, "RAM", j["ram"].as_f64().unwrap_or(-1.0), -1.0);
                    let fps = j["fps"].as_f64().unwrap_or(-1.0);
                    if let Some(jeu) = j["jeu"].as_str() {
                        ui.label(RichText::new(format!("🎮 {jeu}{}", if fps >= 0.0 { format!(" · {fps:.0} FPS") } else { String::new() })).size(20.0).strong());
                    } else {
                        ui.weak("Aucun jeu en cours.");
                    }
                    ui.weak(format!("Outils {} sur le PC", j["version"].as_str().unwrap_or("?")));
                }
                None => {
                    ui.weak("En attente du PC…");
                }
            },
            2 => {
                if ui.button("🔄 Charger la liste des jeux du PC").clicked() && !occupe {
                    let (p, l) = (pc.clone(), self.jeux.clone());
                    en_fond(&self.e, "Liste des jeux…", move || {
                        let j = p.get_json("/api/jeux")?;
                        let mut v: Vec<(String, String)> = j.as_array().into_iter().flatten().map(|x| (x["id"].as_str().unwrap_or("").into(), x["nom"].as_str().unwrap_or("").into())).collect();
                        v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
                        let n = v.len();
                        *l.lock().unwrap() = v;
                        Ok(format!("{n} jeux"))
                    });
                }
                for (id, nom) in self.jeux.lock().unwrap().clone() {
                    if ui.add(egui::Button::new(RichText::new(format!("▶ {nom}")).size(17.0)).min_size(egui::vec2(ui.available_width(), 44.0))).clicked() {
                        self.action(&format!("/api/lancer?id={}", encoder(&id)));
                    }
                }
            }
            3 => {
                ui.weak("Dis ce que tu cherches, comme à une personne : « pdf reçu sur discord », « photos de la plage cet été », « musique téléchargée hier ». Puis télécharge-le sur le téléphone.");
                let r = ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("ex. : pdf reçu sur discord ce mois-ci").desired_width(f32::INFINITY));
                if (r.lost_focus() || ui.button("🔎 Chercher").clicked()) && !self.q.trim().is_empty() && !occupe {
                    let (p, q, res) = (pc.clone(), self.q.clone(), self.resultats.clone());
                    en_fond(&self.e, "Recherche sur le PC…", move || {
                        // la recherche « en français » (Fichiers sans dossiers), sinon celle par nom (PC plus ancien)
                        let j = match p.get_json(&format!("/api/trouver?q={}", encoder(&q))) {
                            Ok(j) if j["liste"].as_array().is_some_and(|l| !l.is_empty()) => j,
                            _ => p.get_json(&format!("/api/chercher?q={}", encoder(&q)))?,
                        };
                        let t = j["total"].as_u64().unwrap_or(0);
                        *res.lock().unwrap() = Some(j);
                        Ok(format!("{t} résultat(s)"))
                    });
                }
                if let Some(j) = self.resultats.lock().unwrap().clone() {
                    if let Some(c) = j["compris"].as_array().filter(|c| !c.is_empty()) {
                        ui.horizontal_wrapped(|ui| {
                            ui.weak("Compris :");
                            for x in c {
                                ui.label(RichText::new(x.as_str().unwrap_or("")).background_color(ui.visuals().faint_bg_color));
                            }
                        });
                    }
                    for x in j["liste"].as_array().into_iter().flatten() {
                        let (nom, chemin) = (x["nom"].as_str().unwrap_or(""), x["chemin"].as_str().unwrap_or(""));
                        let dossier = x["dossier"].as_bool().unwrap_or(false);
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("{} {nom}", icone(nom, dossier))).strong());
                                if !dossier {
                                    ui.weak(taille(x["taille"].as_u64().unwrap_or(0)));
                                }
                                if let Some(src) = x["source"].as_str().filter(|s| !s.is_empty()) {
                                    ui.weak(format!("🌐 {src}"));
                                }
                            });
                            ui.weak(chemin);
                            if !dossier && ui.button("⬇ Télécharger sur le téléphone").clicked() && !occupe {
                                let (p, c, n) = (pc.clone(), chemin.to_string(), nom.to_string());
                                en_fond(&self.e, &format!("Téléchargement de {n}…"), move || {
                                    let r = ureq::get(&p.url(&format!("/api/fichier?chemin={}", encoder(&c)))).timeout(Duration::from_secs(3600)).call().map_err(erreur)?;
                                    let dest = libre(&dossier_outils(), &n);
                                    let mut f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
                                    std::io::copy(&mut r.into_reader(), &mut f).map_err(|e| e.to_string())?;
                                    crate::android::signaler_media(&[dest.display().to_string()]);
                                    Ok(format!("✔ Enregistré dans {}", dest.display()))
                                });
                            }
                        });
                    }
                }
            }
            4 => {
                ui.weak("Choisis des fichiers du téléphone : ils arrivent sur le PC dans Téléchargements › Depuis le téléphone.");
                if !acces_fichiers_ui(ui) {
                    return;
                }
                self.nav.ui(ui);
                let choisis = self.nav.choisis.clone();
                if !choisis.is_empty() && gros_bouton(ui, &format!("📤 Envoyer {} fichier(s) au PC", choisis.len())) && !occupe {
                    let p = pc.clone();
                    self.nav.choisis.clear();
                    en_fond(&self.e, "Envoi…", move || {
                        let mut n = 0;
                        for f in &choisis {
                            let nom = f.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let fic = std::fs::File::open(f).map_err(|e| e.to_string())?;
                            let lon = fic.metadata().map(|m| m.len()).unwrap_or(0);
                            ureq::post(&p.url(&format!("/envoyer?nom={}", encoder(&nom)))).timeout(Duration::from_secs(3600)).set("Content-Length", &lon.to_string()).send(fic).map_err(erreur)?;
                            n += 1;
                        }
                        crate::android::vibrer(60);
                        Ok(format!("✔ {n} fichier(s) envoyé(s) au PC"))
                    });
                }
            }
            5 => {
                if gros_bouton(ui, "📤 Envoyer mon presse-papiers au PC") && !occupe {
                    let p = pc.clone();
                    en_fond(&self.e, "…", move || {
                        let t = crate::android::presse_lire().filter(|t| !t.is_empty()).ok_or("le presse-papiers du téléphone est vide")?;
                        p.post("/presse", t.as_bytes())
                    });
                }
                if gros_bouton(ui, "📥 Récupérer le presse-papiers du PC") && !occupe {
                    let p = pc.clone();
                    en_fond(&self.e, "…", move || {
                        let t = p.get_texte("/presse")?;
                        if crate::android::presse_ecrire(&t) {
                            Ok(format!("✔ Copié sur le téléphone : {}", t.chars().take(80).collect::<String>()))
                        } else {
                            Ok(format!("Texte du PC : {t}"))
                        }
                    });
                }
                ui.separator();
                ui.label("Ou tape un texte à coller sur le PC :");
                ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(3).desired_width(f32::INFINITY));
                if ui.button("Envoyer ce texte").clicked() && !self.texte.is_empty() {
                    let (p, t) = (pc.clone(), self.texte.clone());
                    en_fond(&self.e, "…", move || p.post("/presse", t.as_bytes()));
                }
            }
            6 => {
                if let Some(j) = infos.as_ref().filter(|j| !j.is_null()) {
                    let etat = j["mc"].as_str().unwrap_or("?");
                    ui.label(RichText::new(format!("⛏ Serveur : {etat}")).size(20.0).strong().color(if etat == "en ligne" { VERT } else { Color32::GRAY }));
                    let joueurs: Vec<&str> = j["joueurs"].as_array().into_iter().flatten().filter_map(|x| x.as_str()).collect();
                    if !joueurs.is_empty() {
                        ui.label(format!("👥 {} : {}", joueurs.len(), joueurs.join(", ")));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button(RichText::new("▶ Démarrer").size(18.0)).clicked() {
                        self.action("/cmd/mc_demarrer");
                    }
                    if ui.button(RichText::new("⏹ Arrêter").size(18.0)).clicked() {
                        self.action("/cmd/mc_arreter");
                    }
                    if ui.button(RichText::new("💾 Sauvegarder").size(18.0)).clicked() {
                        self.action("/cmd/mc_sauvegarder");
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.message_mc).hint_text("message dans le chat").desired_width(ui.available_width() - 90.0));
                    if ui.button("Envoyer").clicked() && !self.message_mc.trim().is_empty() {
                        let t = std::mem::take(&mut self.message_mc);
                        self.action(&format!("/cmd/mc_dire?texte={}", encoder(&t)));
                    }
                });
            }
            _ => {
                ui.weak("Garde les mêmes données sur le PC et le téléphone : la version la plus récente remplace l'autre.");
                for (nom, titre) in SYNCHRO {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(titre).size(17.0));
                        ui.weak(format!("modifié ici le {}", date(date_fichier(&crate::outil::chemin(nom)))));
                    });
                }
                if gros_bouton(ui, "🔄 Synchroniser maintenant") && !occupe {
                    let p = pc.clone();
                    en_fond(&self.e, "Synchronisation…", move || {
                        let mut bilan = vec![];
                        for (nom, titre) in SYNCHRO {
                            let local = crate::outil::chemin(nom);
                            let t_local = date_fichier(&local);
                            let j = p.get_json(&format!("/api/synchro?nom={nom}"))?;
                            let t_pc = j["t"].as_u64().unwrap_or(0);
                            if t_pc > t_local && !j["contenu"].is_null() {
                                let _ = std::fs::create_dir_all(local.parent().unwrap());
                                std::fs::write(&local, serde_json::to_string_pretty(&j["contenu"]).unwrap_or_default()).map_err(|e| e.to_string())?;
                                crate::outil::recharger_synchro(nom);
                                bilan.push(format!("{titre} ⬇"));
                            } else if t_local > t_pc && t_local > 0 {
                                let mut b = vec![];
                                std::fs::File::open(&local).and_then(|mut f| f.read_to_end(&mut b)).map_err(|e| e.to_string())?;
                                p.post(&format!("/api/synchro?nom={nom}"), &b)?;
                                bilan.push(format!("{titre} ⬆"));
                            }
                        }
                        Ok(if bilan.is_empty() { "✔ Déjà à jour".into() } else { format!("✔ {}", bilan.join(" · ")) })
                    });
                }
                ui.weak("⬇ = récupéré du PC, ⬆ = envoyé au PC.");
            }
        }
        ui.add_space(12.0);
        if ui.small_button("Associer un autre PC").clicked() {
            enregistrer("pc", &None::<Pc>);
            *self.pc.lock().unwrap() = None;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adresses() {
        let p = lire_adresse("http://192.168.1.20:8642/?k=abc123").unwrap();
        assert_eq!((p.ip.as_str(), p.port, p.cle.as_str()), ("192.168.1.20", 8642, "abc123"));
        assert!(lire_adresse("https://exemple.fr").is_none());
        assert_eq!(p.url("/api/fichier?chemin=a"), "http://192.168.1.20:8642/api/fichier?chemin=a&k=abc123");
        assert_eq!(encoder("C:\\été 1.txt"), "C%3A%5C%C3%A9t%C3%A9%201.txt");
        assert_eq!(decoder(&encoder("Salut à tous !")), "Salut à tous !");
        assert_eq!(commande_relais(&format!("/cmd/mc_dire?texte={}", encoder("Coucou !"))), "mc_dire Coucou !");
        assert_eq!(commande_relais("/api/lancer?id=730"), "lancer 730");
        assert_eq!(commande_relais("/cmd/vol_plus"), "vol_plus");
        assert_eq!(decoder("100%é%2"), "100%é%2");
    }
}
