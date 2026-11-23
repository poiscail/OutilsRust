//! Sécurité du réseau de la maison (à n'utiliser que sur TON réseau) : ports ouverts des appareils,
//! audit de la box, intrus, objets connectés à risque, attaques en cours, et à qui parlent les applis du PC.
use crate::outil::{charger, enregistrer, Cat, Outil};
pub use crate::ports_risques::*;
use crate::reseau3::{nom_hote, passerelle, scanner, table_arp, Appareil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};









fn couleur(r: u8) -> Color32 {
    match r {
        0 => Color32::LIGHT_GREEN,
        1 => Color32::from_rgb(200, 200, 120),
        2 => Color32::from_rgb(255, 170, 80),
        _ => Color32::LIGHT_RED,
    }
}

// ---------- fabricants (adresse MAC) ----------

static FABRICANTS: Mutex<Option<BTreeMap<String, String>>> = Mutex::new(None);

/// Fabricant d'après le début de l'adresse MAC (macvendors.com, gardé en cache).
pub fn fabricant(mac: &str) -> String {
    if mac.len() < 8 {
        return String::new();
    }
    if crate::reseau3::mac_aleatoire(mac) {
        return "adresse privée (téléphone ou tablette)".into();
    }
    let oui = mac[..8].to_uppercase();
    {
        let mut g = FABRICANTS.lock().unwrap();
        let m = g.get_or_insert_with(|| charger("fabricants_mac"));
        if let Some(f) = m.get(&oui) {
            return f.clone();
        }
    }
    std::thread::sleep(Duration::from_millis(1100)); // limite du service : 1 demande par seconde
    let f = ureq::get(&format!("https://api.macvendors.com/{oui}")).timeout(Duration::from_secs(6)).call().ok().and_then(|r| r.into_string().ok()).unwrap_or_default();
    let f = if f.contains("errors") { String::new() } else { f.trim().to_string() };
    let mut g = FABRICANTS.lock().unwrap();
    let m = g.get_or_insert_with(BTreeMap::new);
    m.insert(oui, f.clone());
    enregistrer("fabricants_mac", &*m);
    f
}



// ===================================================================================================
// 1 · Scanner de ports  —  4 · Objets connectés à risque
// ===================================================================================================

#[derive(Clone, Default)]
struct Analyse {
    a: Appareil,
    fabricant: String,
    ports: Vec<u16>,
    genre: &'static str,
    rtsp_libre: bool,
}

/// Une caméra qui envoie son flux sans demander de mot de passe ? (requête RTSP OPTIONS/DESCRIBE)
fn rtsp_sans_mdp(ip: Ipv4Addr) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = TcpStream::connect_timeout(&SocketAddr::from((ip, 554)), Duration::from_millis(800)) else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!("DESCRIBE rtsp://{ip}:554/ RTSP/1.0\r\nCSeq: 1\r\nAccept: application/sdp\r\n\r\n");
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut b = [0u8; 512];
    let n = s.read(&mut b).unwrap_or(0);
    String::from_utf8_lossy(&b[..n]).starts_with("RTSP/1.0 200")
}

fn analyser_reseau(prog: &Mutex<(usize, usize)>, etat: &Mutex<String>) -> Vec<Analyse> {
    *etat.lock().unwrap() = "Recherche des appareils…".into();
    let appareils = scanner(prog);
    let ports = liste_ports();
    let mut v = vec![];
    for (k, a) in appareils.into_iter().enumerate() {
        *etat.lock().unwrap() = format!("Appareil {} : {}", k + 1, a.ip);
        let Ok(ip) = a.ip.parse::<Ipv4Addr>() else { continue };
        let o = ports_ouverts(ip, &ports, Duration::from_millis(350));
        let f = fabricant(&a.mac);
        let g = genre(&f, &a.nom, &o);
        let rtsp_libre = o.contains(&554) && rtsp_sans_mdp(ip);
        v.push(Analyse { a, fabricant: f, ports: o, genre: g, rtsp_libre });
    }
    v
}

fn note_appareil(x: &Analyse) -> u8 {
    let mut r = x.ports.iter().map(|p| infos_port(*p).1).max().unwrap_or(0);
    if x.rtsp_libre {
        r = 3;
    }
    r
}

pub struct ScannerPorts {
    liste: Arc<Mutex<Option<Vec<Analyse>>>>,
    prog: Arc<Mutex<(usize, usize)>>,
    etat: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
    objets: bool,
}

impl ScannerPorts {
    pub fn new(objets: bool) -> Self {
        ScannerPorts { liste: Arc::default(), prog: Arc::default(), etat: Arc::default(), en_cours: Arc::default(), objets }
    }

    fn lancer(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (l, p, e, c) = (self.liste.clone(), self.prog.clone(), self.etat.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let v = analyser_reseau(&p, &e);
            *l.lock().unwrap() = Some(v);
            c.store(false, Relaxed);
        });
    }
}

impl Outil for ScannerPorts {
    fn nom(&self) -> &'static str {
        if self.objets {
            "📹 Objets connectés à risque"
        } else {
            "🔓 Scanner de ports de la maison"
        }
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.objets {
            ui.heading("Objets connectés à risque");
            ui.weak("Caméras, ampoules, prises, TV, imprimantes : Outils repère ceux qui laissent une porte ouverte (Telnet, flux vidéo sans mot de passe, page non chiffrée) et explique comment la fermer.");
        } else {
            ui.heading("Scanner de ports de la maison");
            ui.weak("Pour chaque appareil de TON réseau : quels services répondent (ports ouverts), et ce que ça risque. À n'utiliser que chez toi : scanner le réseau des autres est interdit.");
        }
        let en_cours = self.en_cours.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!en_cours, egui::Button::new("🔎 Analyser le réseau")).clicked() {
                self.lancer();
            }
            if en_cours {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        });
        let Some(v) = self.liste.lock().unwrap().clone() else {
            ui.weak("Lance l'analyse (une à deux minutes).");
            return;
        };
        let mut v = v;
        v.sort_by_key(|x| std::cmp::Reverse(note_appareil(x)));
        let risques = v.iter().filter(|x| note_appareil(x) >= 2).count();
        ui.label(RichText::new(if risques == 0 { "✔ Aucun appareil à risque".to_string() } else { format!("⚠ {risques} appareil(s) à vérifier") }).strong().color(if risques == 0 { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 170, 80) }));
        egui::ScrollArea::vertical().id_salt("scan_ports").show(ui, |ui| {
            for x in &v {
                if self.objets && (x.genre.contains("ordinateur") || x.genre.contains("téléphone")) {
                    continue;
                }
                let r = note_appareil(x);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(x.genre).strong());
                        ui.strong(&x.a.ip);
                        if !x.a.nom.is_empty() {
                            ui.label(&x.a.nom);
                        }
                        ui.weak(format!("{} {}", x.a.mac, x.fabricant));
                        ui.colored_label(couleur(r), ["sûr", "à surveiller", "risqué", "dangereux"][r as usize]);
                    });
                    if x.ports.is_empty() {
                        ui.weak("Aucun port ouvert parmi les plus courants.");
                    }
                    for p in &x.ports {
                        let (n, risque, t) = infos_port(*p);
                        ui.colored_label(couleur(risque), format!("  {p} · {n} — {t}"));
                    }
                    if x.rtsp_libre {
                        ui.colored_label(Color32::LIGHT_RED, "  ❗ Le flux vidéo de cette caméra s'ouvre SANS mot de passe : n'importe qui sur le réseau peut la regarder. Mets un mot de passe dans son appli.");
                    }
                    if x.ports.contains(&23) {
                        ui.label("  → Coupe Telnet dans les réglages de l'appareil, change son mot de passe d'usine et fais sa mise à jour.");
                    }
                    if x.ports.contains(&80) && !x.ports.contains(&443) && r >= 1 && self.objets {
                        ui.label("  → Sa page de réglages n'est pas chiffrée : change le mot de passe d'usine et n'y va pas depuis un Wi-Fi public.");
                    }
                });
            }
        });
    }
}

// ===================================================================================================
// 2 · Audit de la box
// ===================================================================================================

/// (authentification, chiffrement) de la connexion Wi-Fi actuelle (sortie de « netsh wlan show interfaces »).
pub fn securite_wifi(sortie: &str) -> Option<(String, String)> {
    let val = |cles: &[&str]| {
        sortie.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            cles.iter().any(|c| k.trim().eq_ignore_ascii_case(c)).then(|| v.trim().to_string())
        })
    };
    Some((val(&["Authentication", "Authentification"])?, val(&["Cipher", "Chiffrement"]).unwrap_or_default()))
}



/// La box répond-elle en UPnP (les applis peuvent ouvrir des ports toutes seules) ?
fn upnp_actif(box_ip: Ipv4Addr) -> bool {
    let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return false };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let m = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
    let _ = s.send_to(m.as_bytes(), "239.255.255.250:1900");
    let mut b = [0u8; 1500];
    let fin = Instant::now() + Duration::from_secs(2);
    while Instant::now() < fin {
        match s.recv_from(&mut b) {
            Ok((_, SocketAddr::V4(a))) if *a.ip() == box_ip => return true,
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    false
}

/// Serveurs DNS utilisés par Windows.
pub fn serveurs_dns() -> Vec<String> {
    let o = std::process::Command::new("powershell").args(["-NoProfile", "-Command", "(Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object { $_.ServerAddresses } | Select-Object -ExpandProperty ServerAddresses) -join ','"]).creation_flags(0x0800_0000).output();
    let mut v: Vec<String> = o.map(|o| String::from_utf8_lossy(&o.stdout).trim().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    v.dedup();
    v
}



#[derive(Clone)]
struct Point {
    ok: bool,
    texte: String,
}

fn auditer_box(etat: &Mutex<String>) -> (u32, Vec<Point>) {
    let mut v = vec![];
    let mut note = 0u32;
    *etat.lock().unwrap() = "Wi-Fi…".into();
    let wlan = std::process::Command::new("netsh").args(["wlan", "show", "interfaces"]).creation_flags(0x0800_0000).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    match securite_wifi(&wlan) {
        Some((a, c)) => {
            let (n, t) = note_wifi(&a, &c);
            note += n;
            v.push(Point { ok: n >= 24, texte: t });
        }
        None => {
            note += 20;
            v.push(Point { ok: true, texte: "PC branché par câble (pas de Wi-Fi à vérifier depuis ce PC).".into() });
        }
    }
    let Some(b) = passerelle() else {
        v.push(Point { ok: false, texte: "Box introuvable (pas de connexion ?)".into() });
        return (note, v);
    };
    *etat.lock().unwrap() = format!("Ports de la box ({b})…");
    let ouverts = ports_ouverts(b, &[21, 22, 23, 80, 443, 445, 7547, 8080, 8443, 5900], Duration::from_millis(600));
    let mut pn = 25u32;
    for p in &ouverts {
        let (n, r, t) = infos_port(*p);
        if r >= 2 {
            pn = pn.saturating_sub(if r == 3 { 15 } else { 7 });
            v.push(Point { ok: false, texte: format!("Port {p} ({n}) ouvert sur la box : {t}.") });
        }
    }
    if ouverts.contains(&80) && !ouverts.contains(&443) {
        pn = pn.saturating_sub(5);
        v.push(Point { ok: false, texte: "La page d'administration de la box n'est pas chiffrée (HTTP) : ne t'y connecte que depuis chez toi, et change son mot de passe d'usine.".into() });
    }
    if pn == 25 {
        v.push(Point { ok: true, texte: "Aucun service dangereux ouvert sur la box.".into() });
    }
    note += pn;
    *etat.lock().unwrap() = "UPnP…".into();
    if upnp_actif(b) {
        note += 5;
        v.push(Point { ok: false, texte: "UPnP est actif : les applis (et les virus) peuvent ouvrir des ports vers internet tout seuls. Pratique pour les jeux en ligne, mais coupe-le si tu n'en as pas besoin.".into() });
    } else {
        note += 15;
        v.push(Point { ok: true, texte: "UPnP ne répond pas (aucune appli ne peut ouvrir de port toute seule).".into() });
    }
    *etat.lock().unwrap() = "DNS…".into();
    let dns = serveurs_dns();
    let bizarres: Vec<&String> = dns.iter().filter(|d| !dns_connu(d, &b.to_string())).collect();
    if bizarres.is_empty() {
        note += 20;
        v.push(Point { ok: true, texte: format!("DNS utilisés : {} (normaux).", dns.join(", ")) });
    } else {
        v.push(Point { ok: false, texte: format!("DNS inhabituels : {}. Si tu ne les as pas choisis, un programme ou la box a peut-être été piraté (redirection vers de faux sites).", bizarres.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) });
    }
    note += 10;
    v.push(Point { ok: true, texte: "À vérifier toi-même dans la page de la box : WPS désactivé, mot de passe admin changé, mises à jour automatiques actives.".into() });
    (note.min(100), v)
}

pub struct AuditBox {
    r: Arc<Mutex<Option<(u32, Vec<Point>)>>>,
    etat: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
}

impl AuditBox {
    pub fn new() -> Self {
        AuditBox { r: Arc::default(), etat: Arc::default(), en_cours: Arc::default() }
    }
}

impl Outil for AuditBox {
    fn nom(&self) -> &'static str {
        "🛡 Audit de la box"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Audit de la box");
        ui.weak("Vérifie la sécurité de ta box et de ton Wi-Fi : chiffrement, services ouverts, UPnP, DNS. Avec une note sur 100 et quoi corriger.");
        let en_cours = self.en_cours.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!en_cours, egui::Button::new("🔎 Lancer l'audit")).clicked() {
                self.en_cours.store(true, Relaxed);
                let (r, e, c) = (self.r.clone(), self.etat.clone(), self.en_cours.clone());
                std::thread::spawn(move || {
                    *r.lock().unwrap() = Some(auditer_box(&e));
                    c.store(false, Relaxed);
                });
            }
            if en_cours {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        });
        let Some((note, points)) = self.r.lock().unwrap().clone() else { return };
        let c = if note >= 80 { Color32::LIGHT_GREEN } else if note >= 55 { Color32::from_rgb(240, 200, 70) } else { Color32::LIGHT_RED };
        ui.label(RichText::new(format!("{note} / 100")).size(30.0).strong().color(c));
        for p in points {
            ui.colored_label(if p.ok { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 170, 80) }, format!("{} {}", if p.ok { "✔" } else { "⚠" }, p.texte));
        }
        if let Some(b) = passerelle() {
            if ui.button("🌐 Ouvrir la page de la box").clicked() {
                let _ = std::process::Command::new("explorer").arg(format!("http://{b}")).spawn();
            }
        }
    }
}

// ===================================================================================================
// 3 · Alerte d'intrus  —  5 · Détecteur d'attaques
// ===================================================================================================

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Surveillance {
    actif: bool,
    connus: BTreeMap<String, String>, // mac → description
    passerelle_mac: String,
    dns: Vec<String>,
    journal: Vec<(String, String)>,
}



/// Réseaux Wi-Fi visibles : SSID → liste des sécurités annoncées (netsh wlan show networks mode=bssid).
pub fn reseaux_visibles(sortie: &str) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut ssid = String::new();
    for l in sortie.lines() {
        let t = l.trim();
        if t.to_uppercase().starts_with("SSID") && t.contains(':') {
            ssid = t.split_once(':').map(|x| x.1.trim().to_string()).unwrap_or_default();
        } else if let Some((k, v)) = t.split_once(':') {
            let k = k.trim().to_lowercase();
            if (k == "authentication" || k == "authentification") && !ssid.is_empty() {
                let e = m.entry(ssid.clone()).or_default();
                if !e.contains(&v.trim().to_string()) {
                    e.push(v.trim().to_string());
                }
            }
        }
    }
    m
}

pub struct Gardien {
    s: Arc<Mutex<Surveillance>>,
    lance: bool,
    attaques: bool,
    en_cours: Arc<AtomicBool>,
    derniers: Arc<Mutex<Vec<Appareil>>>,
}

fn noter(s: &mut Surveillance, titre: &str, texte: String) {
    crate::systeme::notifier_toujours(titre, &texte);
    s.journal.insert(0, (crate::minecraft::date_lisible(), format!("{titre} : {texte}")));
    s.journal.truncate(100);
}

fn verifier_attaques(s: &Arc<Mutex<Surveillance>>) {
    let Some(b) = passerelle() else { return };
    let arp = table_arp();
    let mut g = s.lock().unwrap();
    if let Some(mac) = arp.get(&b) {
        if g.passerelle_mac.is_empty() {
            g.passerelle_mac = mac.clone();
        } else if g.passerelle_mac != *mac {
            let ancien = g.passerelle_mac.clone();
            noter(&mut g, "⚠ La box a changé d'adresse MAC", format!("{ancien} → {mac}. Si tu n'as pas changé de box, un appareil se fait peut-être passer pour elle (usurpation ARP)."));
            g.passerelle_mac = mac.clone();
        }
    }
    if let Some(autres) = usurpation_arp(&arp, b) {
        let l = autres.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", ");
        if !g.journal.first().is_some_and(|j| j.1.contains(&l)) {
            noter(&mut g, "⚠ Usurpation de la box possible", format!("{l} utilise(nt) la même adresse MAC que la box : un appareil intercepte peut-être le trafic."));
        }
    }
    drop(g);
    let dns = serveurs_dns();
    let wlan = std::process::Command::new("netsh").args(["wlan", "show", "networks", "mode=bssid"]).creation_flags(0x0800_0000).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let actuel = std::process::Command::new("netsh").args(["wlan", "show", "interfaces"]).creation_flags(0x0800_0000).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let mon_ssid = actuel.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == "SSID").then(|| v.trim().to_string())
    });
    let mut g = s.lock().unwrap();
    if !g.dns.is_empty() && dns != g.dns && !dns.is_empty() {
        let (a, n) = (g.dns.join(", "), dns.join(", "));
        noter(&mut g, "DNS modifiés", format!("{a} → {n}. Si ce n'est pas toi, un programme redirige peut-être ta navigation."));
    }
    if !dns.is_empty() {
        g.dns = dns;
    }
    if let Some(ss) = mon_ssid {
        if let Some(sec) = reseaux_visibles(&wlan).get(&ss) {
            if sec.len() > 1 && !g.journal.first().is_some_and(|j| j.1.contains("jumeau")) {
                noter(&mut g, "⚠ Faux Wi-Fi possible", format!("« {ss} » est visible avec plusieurs sécurités ({}) : un réseau jumeau imite peut-être le tien.", sec.join(" / ")));
            }
        }
    }
    enregistrer("gardien_reseau", &*g);
}

fn verifier_intrus(s: &Arc<Mutex<Surveillance>>, derniers: &Arc<Mutex<Vec<Appareil>>>) {
    let p = Mutex::new((0, 0));
    let v = scanner(&p);
    *derniers.lock().unwrap() = v.clone();
    let premiere = s.lock().unwrap().connus.is_empty();
    for a in v.iter().filter(|a| !a.mac.is_empty()) {
        let connu = s.lock().unwrap().connus.contains_key(&a.mac);
        if connu {
            continue;
        }
        let f = fabricant(&a.mac);
        let d = format!("{} {} {}", a.ip, a.nom, f).trim().to_string();
        let mut g = s.lock().unwrap();
        g.connus.insert(a.mac.clone(), d.clone());
        if !premiere {
            noter(&mut g, "Nouvel appareil sur le Wi-Fi", format!("{d} ({}). Si tu ne le connais pas, change le mot de passe du Wi-Fi.", a.mac));
        }
    }
    enregistrer("gardien_reseau", &*s.lock().unwrap());
}

impl Gardien {
    pub fn new(attaques: bool) -> Self {
        Gardien { s: Arc::new(Mutex::new(charger("gardien_reseau"))), lance: false, attaques, en_cours: Arc::default(), derniers: Arc::default() }
    }
}

static SURVEILLANCE_LANCEE: AtomicBool = AtomicBool::new(false);

impl Outil for Gardien {
    fn nom(&self) -> &'static str {
        if self.attaques {
            "🕵 Détecteur d'attaques réseau"
        } else {
            "🚨 Alerte d'intrus sur le Wi-Fi"
        }
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.lance || !self.s.lock().unwrap().actif {
            return;
        }
        self.lance = true;
        // une seule surveillance pour les deux outils (ils partagent le même fichier)
        if SURVEILLANCE_LANCEE.swap(true, Relaxed) {
            return;
        }
        let (s, d) = (self.s.clone(), self.derniers.clone());
        std::thread::spawn(move || {
            let mut tour = 0u64;
            loop {
                std::thread::sleep(Duration::from_secs(60));
                *s.lock().unwrap() = charger("gardien_reseau");
                if !s.lock().unwrap().actif {
                    continue;
                }
                if crate::outil::DISCRET.load(Relaxed) {
                    continue;
                }
                verifier_attaques(&s);
                if tour % 10 == 0 {
                    verifier_intrus(&s, &d);
                }
                tour += 1;
            }
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.attaques {
            ui.heading("Détecteur d'attaques réseau");
            ui.weak("Surveille en fond : un appareil qui se fait passer pour la box (usurpation ARP), un faux Wi-Fi qui imite le tien, des DNS changés en douce. Tu es prévenu tout de suite (et sur le téléphone).");
        } else {
            ui.heading("Alerte d'intrus sur le Wi-Fi");
            ui.weak("Outils regarde toutes les 10 minutes qui est connecté, et te prévient quand un appareil inconnu arrive, avec son fabricant.");
        }
        let mut g = self.s.lock().unwrap();
        let avant = g.actif;
        ui.checkbox(&mut g.actif, RichText::new("Surveiller en permanence (intrus et attaques)").strong());
        if g.actif != avant {
            enregistrer("gardien_reseau", &*g);
            self.lance = false;
        }
        drop(g);
        if !self.attaques {
            let en_cours = self.en_cours.load(Relaxed);
            if ui.add_enabled(!en_cours, egui::Button::new("🔎 Vérifier maintenant")).clicked() {
                self.en_cours.store(true, Relaxed);
                let (s, d, c) = (self.s.clone(), self.derniers.clone(), self.en_cours.clone());
                std::thread::spawn(move || {
                    verifier_intrus(&s, &d);
                    c.store(false, Relaxed);
                });
            }
            if en_cours {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            ui.strong("Appareils connus");
            let mut oublier = None;
            let connus = self.s.lock().unwrap().connus.clone();
            for (mac, d) in &connus {
                ui.horizontal(|ui| {
                    ui.label(format!("{d} · {mac}"));
                    if ui.small_button("✖").on_hover_text("Oublier (il sera signalé s'il revient)").clicked() {
                        oublier = Some(mac.clone());
                    }
                });
            }
            if let Some(m) = oublier {
                let mut g = self.s.lock().unwrap();
                g.connus.remove(&m);
                enregistrer("gardien_reseau", &*g);
            }
            ui.weak("Les téléphones récents changent d'adresse MAC par réseau (« adresse privée ») : un même téléphone peut apparaître une fois de plus.");
        } else if ui.button("🔎 Vérifier maintenant").clicked() {
            let s = self.s.clone();
            std::thread::spawn(move || verifier_attaques(&s));
        }
        ui.separator();
        ui.strong("Journal");
        let j = self.s.lock().unwrap().journal.clone();
        if j.is_empty() {
            ui.weak("Rien à signaler.");
        }
        for (h, t) in j.iter().take(40) {
            ui.label(format!("{h} · {t}"));
        }
    }
}

// ===================================================================================================
// 6 · Analyseur de trafic : à qui parlent les applis du PC
// ===================================================================================================



#[derive(Clone)]
struct Lien {
    appli: String,
    ip: Ipv4Addr,
    port: u16,
    nom: String,
}

fn connexions_detaillees() -> Vec<(String, Ipv4Addr, u16)> {
    let mut v = vec![];
    for (appli, l) in crate::reseau::connexions() {
        for x in l {
            if let Some((ip, p)) = x.split_once(':') {
                if let (Ok(ip), Ok(p)) = (ip.parse::<Ipv4Addr>(), p.parse::<u16>()) {
                    if !ip.is_private() {
                        v.push((appli.clone(), ip, p));
                    }
                }
            }
        }
    }
    v
}

pub struct Trafic {
    liens: Arc<Mutex<Vec<Lien>>>,
    noms: Arc<Mutex<HashMap<Ipv4Addr, String>>>,
    pays: Arc<Mutex<HashMap<Ipv4Addr, String>>>,
    vu: Instant,
    maj: Instant,
    occupe: Arc<AtomicBool>,
    filtre: String,
}

impl Trafic {
    pub fn new() -> Self {
        Trafic { liens: Arc::default(), noms: Arc::default(), pays: Arc::default(), vu: Instant::now(), maj: Instant::now() - Duration::from_secs(60), occupe: Arc::default(), filtre: String::new() }
    }

    fn actualiser(&self) {
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (l, n, o) = (self.liens.clone(), self.noms.clone(), self.occupe.clone());
        std::thread::spawn(move || {
            let c = connexions_detaillees();
            for (_, ip, _) in &c {
                if !n.lock().unwrap().contains_key(ip) {
                    let nom = nom_hote(*ip).unwrap_or_default();
                    n.lock().unwrap().insert(*ip, nom);
                }
            }
            let nn = n.lock().unwrap().clone();
            *l.lock().unwrap() = c.into_iter().map(|(appli, ip, port)| Lien { appli, ip, port, nom: nn.get(&ip).cloned().unwrap_or_default() }).collect();
            o.store(false, Relaxed);
        });
    }

    /// Pays des serveurs (ipwho.is, seulement quand on le demande : les adresses sont envoyées au service).
    fn localiser(&self) {
        let ips: Vec<Ipv4Addr> = self.liens.lock().unwrap().iter().map(|l| l.ip).filter(|ip| !self.pays.lock().unwrap().contains_key(ip)).collect();
        let p = self.pays.clone();
        std::thread::spawn(move || {
            let mut vus = std::collections::HashSet::new();
            for ip in ips.into_iter().filter(|i| vus.insert(*i)).take(60) {
                let r = ureq::get(&format!("https://ipwho.is/{ip}")).timeout(Duration::from_secs(6)).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok());
                if let Some(j) = r {
                    let t = format!("{} {}", j["flag"]["emoji"].as_str().unwrap_or(""), j["country"].as_str().unwrap_or("?"));
                    p.lock().unwrap().insert(ip, t.trim().to_string());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
    }
}

impl Outil for Trafic {
    fn nom(&self) -> &'static str {
        "📡 À qui parle mon PC ?"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        if self.maj.elapsed() > Duration::from_secs(4) {
            self.maj = Instant::now();
            self.actualiser();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        ui.heading("À qui parle mon PC ?");
        ui.weak("Chaque appli du PC et les serveurs internet auxquels elle est connectée en ce moment, avec les pisteurs, la pub et la télémétrie repérés.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("filtrer (appli, serveur…)").desired_width(220.0));
            if ui.button("🌍 Trouver les pays").on_hover_text("Les adresses des serveurs sont envoyées au service ipwho.is").clicked() {
                self.localiser();
            }
        });
        let l = self.liens.lock().unwrap().clone();
        let p = self.pays.lock().unwrap().clone();
        let f = self.filtre.to_lowercase();
        let mut par: BTreeMap<String, Vec<&Lien>> = BTreeMap::new();
        for x in l.iter().filter(|x| f.is_empty() || x.appli.to_lowercase().contains(&f) || x.nom.to_lowercase().contains(&f)) {
            par.entry(x.appli.clone()).or_default().push(x);
        }
        let pisteurs = l.iter().filter(|x| pisteur(&x.nom).is_some()).count();
        ui.label(format!("{} connexion(s) · {} appli(s) · {} vers des pisteurs ou de la télémétrie", l.len(), par.len(), pisteurs));
        egui::ScrollArea::vertical().id_salt("trafic").show(ui, |ui| {
            for (appli, v) in &par {
                egui::CollapsingHeader::new(format!("{appli} ({})", v.len())).id_salt(appli).default_open(v.len() <= 6).show(ui, |ui| {
                    for x in v {
                        let pays = p.get(&x.ip).cloned().unwrap_or_default();
                        let texte = format!("{}:{} {} {}", x.ip, x.port, if x.nom.is_empty() { String::new() } else { format!("({})", x.nom) }, pays);
                        match pisteur(&x.nom) {
                            Some(t) => ui.colored_label(Color32::from_rgb(255, 170, 80), format!("{texte} — 👁 {t}")),
                            None => ui.label(texte),
                        };
                    }
                });
            }
        });
        ui.weak("Pour couper une appli qui parle trop : Système › Couper internet à une appli. Pour bloquer pubs et pisteurs partout : Réseau › Bloqueur de pubs.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures_reseau() {
        let s = "    Nom                    : Wi-Fi\n    SSID                   : MaBox\n    Authentification       : WPA2 - Personnel\n    Chiffrement            : CCMP\n";
        let (a, c) = securite_wifi(s).unwrap();
        assert_eq!((a.as_str(), c.as_str()), ("WPA2 - Personnel", "CCMP"));
        assert_eq!(note_wifi(&a, &c).0, 24);
        assert_eq!(note_wifi("WPA3-Personal", "GCMP").0, 30);
        assert_eq!(note_wifi("Open", "None").0, 0);
        assert_eq!(note_wifi("WEP", "WEP").0, 0);
        assert!(dns_connu("1.1.1.1", "192.168.1.254") && dns_connu("192.168.1.254", "192.168.1.254"));
        assert!(!dns_connu("45.33.12.9", "192.168.1.254"));
        let mut arp = BTreeMap::new();
        let b: Ipv4Addr = "192.168.1.1".parse().unwrap();
        arp.insert(b, "AA:BB".to_string());
        arp.insert("192.168.1.20".parse().unwrap(), "CC:DD".to_string());
        assert!(usurpation_arp(&arp, b).is_none());
        arp.insert("192.168.1.66".parse().unwrap(), "AA:BB".to_string());
        assert_eq!(usurpation_arp(&arp, b).unwrap().len(), 1);
        let n = "SSID 1 : MaBox\n    Type de réseau            : Infrastructure\n    Authentification          : WPA2 - Personnel\n    BSSID 1                 : aa:bb\nSSID 2 : MaBox\n    Authentification          : Ouvrir\n";
        let r = reseaux_visibles(n);
        assert_eq!(r.get("MaBox").map(|v| v.len()), Some(2));
        assert_eq!(pisteur("ad.doubleclick.net"), Some("pub Google"));
        assert!(pisteur("store.steampowered.com").is_none());
        assert_eq!(genre("Hikvision", "", &[80]), "📹 caméra");
        assert_eq!(genre("", "", &[62078]), "📱 téléphone / tablette");
        assert_eq!(infos_port(23).1, 3);
    }
}
