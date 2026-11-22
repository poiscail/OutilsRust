//! Bloqueur de pubs pour toute la maison : le PC devient le serveur DNS du réseau. Les adresses de pub
//! et de pistage reçoivent « 0.0.0.0 », le reste est demandé au vrai DNS (Cloudflare, Google…).
//! Il suffit de mettre l'adresse du PC comme DNS dans la box (ou sur un appareil) : téléphones, TV,
//! consoles sont protégés sans rien installer.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const LISTE: &str = "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts";

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub amont: String,
    pub autorises: Vec<String>,
    pub bloques_en_plus: Vec<String>,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: false, amont: "1.1.1.1".into(), autorises: vec![], bloques_en_plus: vec![] }
    }
}

/// Noms de domaine d'un fichier hosts (« 0.0.0.0 pub.exemple.com »).
pub fn lire_hosts(t: &str) -> HashSet<String> {
    let mut v = HashSet::new();
    for l in t.lines() {
        let l = l.split('#').next().unwrap_or("").trim();
        let mut c = l.split_whitespace();
        let (Some(ip), Some(nom)) = (c.next(), c.next()) else { continue };
        if (ip == "0.0.0.0" || ip == "127.0.0.1") && nom.contains('.') && nom != "0.0.0.0" && !nom.ends_with("localhost") {
            v.insert(nom.to_lowercase());
        }
    }
    v
}

/// Nom demandé et type (A = 1, AAAA = 28…) d'une requête DNS ; et la position de fin de la question.
pub fn question(p: &[u8]) -> Option<(String, u16, usize)> {
    if p.len() < 17 || u16::from_be_bytes([p[4], p[5]]) == 0 {
        return None;
    }
    let mut i = 12;
    let mut parties = vec![];
    loop {
        let n = *p.get(i)? as usize;
        if n == 0 {
            i += 1;
            break;
        }
        if n & 0xC0 != 0 || parties.len() > 60 {
            return None;
        }
        parties.push(String::from_utf8_lossy(p.get(i + 1..i + 1 + n)?).to_lowercase());
        i += 1 + n;
    }
    let t = u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]);
    Some((parties.join("."), t, i + 4))
}

/// Le domaine ou l'un de ses parents est-il bloqué ? (« a.pub.com » est bloqué si « pub.com » l'est)
pub fn bloque(nom: &str, liste: &HashSet<String>, autorises: &[String]) -> bool {
    let est = |d: &str| autorises.iter().any(|a| a == d || d.ends_with(&format!(".{a}")));
    if est(nom) {
        return false;
    }
    let mut d = nom;
    loop {
        if liste.contains(d) {
            return true;
        }
        match d.find('.') {
            Some(k) if d[k + 1..].contains('.') => d = &d[k + 1..],
            _ => return false,
        }
    }
}

/// Réponse « 0.0.0.0 » (ou « :: ») à une requête bloquée.
pub fn reponse_bloquee(req: &[u8], fin_question: usize, genre: u16) -> Vec<u8> {
    let mut r = req[..fin_question].to_vec();
    r[2] = 0x81 | (req[2] & 0x01); // réponse, récursion demandée recopiée
    r[3] = 0x80;
    let donnees: &[u8] = match genre {
        1 => &[0, 0, 0, 0],
        28 => &[0; 16],
        _ => &[],
    };
    let n = if donnees.is_empty() { 0u16 } else { 1 };
    r[6..8].copy_from_slice(&n.to_be_bytes()); // réponses
    r[8..12].copy_from_slice(&[0, 0, 0, 0]);
    if n == 1 {
        r.extend_from_slice(&[0xC0, 0x0C]);
        r.extend_from_slice(&genre.to_be_bytes());
        r.extend_from_slice(&[0, 1, 0, 0, 0, 60]); // classe IN, durée 60 s
        r.extend_from_slice(&(donnees.len() as u16).to_be_bytes());
        r.extend_from_slice(donnees);
    }
    r
}

#[derive(Default)]
struct Stats {
    requetes: AtomicU64,
    bloquees: AtomicU64,
    top: Mutex<HashMap<String, u64>>,
    appareils: Mutex<HashMap<String, (u64, u64)>>,
}

struct Serveur {
    marche: AtomicBool,
    liste: RwLock<HashSet<String>>,
    r: RwLock<Reglages>,
    stats: Stats,
    etat: Mutex<String>,
}

fn liste_locale() -> std::path::PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.join("liste_pubs_hosts.txt")).unwrap_or_default()
}

fn charger_liste(s: &Serveur, telecharger: bool) {
    let f = liste_locale();
    let vieux = f.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(7 * 86400)).unwrap_or(true);
    if telecharger || vieux {
        *s.etat.lock().unwrap() = "Téléchargement de la liste des pubs…".into();
        let r = ureq::get(LISTE).timeout(Duration::from_secs(60)).call().ok().and_then(|r| r.into_string().ok());
        if let Some(t) = r.filter(|t| t.len() > 10_000) {
            let _ = std::fs::write(&f, t);
        }
    }
    let mut l = lire_hosts(&std::fs::read_to_string(&f).unwrap_or_default());
    l.extend(s.r.read().unwrap().bloques_en_plus.iter().map(|x| x.to_lowercase()));
    *s.etat.lock().unwrap() = format!("{} adresses de pub et de pistage bloquées", l.len());
    *s.liste.write().unwrap() = l;
}

fn servir(s: Arc<Serveur>) {
    let sock = match UdpSocket::bind("0.0.0.0:53") {
        Ok(x) => x,
        Err(e) => {
            *s.etat.lock().unwrap() = format!("✖ Impossible d'ouvrir le port 53 ({e}) : un autre serveur DNS tourne peut-être déjà.");
            s.marche.store(false, Relaxed);
            return;
        }
    };
    let _ = sock.set_read_timeout(Some(Duration::from_millis(500)));
    let Ok(amont) = UdpSocket::bind("0.0.0.0:0") else { return };
    let _ = amont.set_read_timeout(Some(Duration::from_millis(80)));
    let sock = Arc::new(sock);
    // requêtes transmises au vrai DNS : (id transmis) → (client, id d'origine, moment)
    let attente: Arc<Mutex<HashMap<u16, (SocketAddr, u16, Instant)>>> = Arc::default();
    {
        let (s2, sock2, amont2, att2) = (s.clone(), sock.clone(), amont.try_clone(), attente.clone());
        if let Ok(amont2) = amont2 {
            std::thread::spawn(move || {
                let mut b = [0u8; 4096];
                while s2.marche.load(Relaxed) {
                    if let Ok((n, _)) = amont2.recv_from(&mut b) {
                        if n < 12 {
                            continue;
                        }
                        let id = u16::from_be_bytes([b[0], b[1]]);
                        if let Some((client, orig, _)) = att2.lock().unwrap().remove(&id) {
                            b[..2].copy_from_slice(&orig.to_be_bytes());
                            let _ = sock2.send_to(&b[..n], client);
                        }
                    }
                    att2.lock().unwrap().retain(|_, v| v.2.elapsed() < Duration::from_secs(5));
                }
            });
        }
    }
    let mut prochain: u16 = 1;
    let mut b = [0u8; 1500];
    while s.marche.load(Relaxed) {
        let Ok((n, client)) = sock.recv_from(&mut b) else { continue };
        let req = &b[..n];
        let Some((nom, genre, fin)) = question(req) else { continue };
        s.stats.requetes.fetch_add(1, Relaxed);
        let ip = client.ip().to_string();
        let r = s.r.read().unwrap().clone();
        let est_bloque = bloque(&nom, &s.liste.read().unwrap(), &r.autorises);
        {
            let mut a = s.stats.appareils.lock().unwrap();
            let e = a.entry(ip).or_default();
            e.0 += 1;
            if est_bloque {
                e.1 += 1;
            }
        }
        if est_bloque {
            s.stats.bloquees.fetch_add(1, Relaxed);
            *s.stats.top.lock().unwrap().entry(nom).or_default() += 1;
            let _ = sock.send_to(&reponse_bloquee(req, fin, genre), client);
            continue;
        }
        let orig = u16::from_be_bytes([req[0], req[1]]);
        prochain = prochain.wrapping_add(1).max(1);
        attente.lock().unwrap().insert(prochain, (client, orig, Instant::now()));
        let mut q = req.to_vec();
        q[..2].copy_from_slice(&prochain.to_be_bytes());
        let _ = amont.send_to(&q, format!("{}:53", r.amont.trim()));
    }
}

static SERVEUR: std::sync::OnceLock<Arc<Serveur>> = std::sync::OnceLock::new();

fn serveur() -> Arc<Serveur> {
    SERVEUR
        .get_or_init(|| Arc::new(Serveur { marche: AtomicBool::new(false), liste: RwLock::default(), r: RwLock::new(charger("dns_maison")), stats: Stats::default(), etat: Mutex::new(String::new()) }))
        .clone()
}

fn demarrer() {
    let s = serveur();
    if s.marche.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        charger_liste(&s, false);
        servir(s);
    });
}

fn arreter() {
    serveur().marche.store(false, Relaxed);
}

fn ip_locale() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.1.1:80").ok()?;
    Some(s.local_addr().ok()?.ip().to_string())
}

pub struct DnsMaison {
    r: Reglages,
    ajout: String,
    autoriser: String,
    lance: bool,
}

impl DnsMaison {
    pub fn new() -> Self {
        let r: Reglages = charger("dns_maison");
        DnsMaison { r, ajout: String::new(), autoriser: String::new(), lance: false }
    }

    fn sauver(&self) {
        enregistrer("dns_maison", &self.r);
        *serveur().r.write().unwrap() = self.r.clone();
    }
}

impl Outil for DnsMaison {
    fn nom(&self) -> &'static str {
        "🏠 Bloqueur de pubs pour la maison"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if !self.lance {
            self.lance = true;
            if self.r.actif {
                demarrer();
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bloqueur de pubs pour toute la maison");
        ui.weak("Le PC devient le serveur DNS de la maison : les pubs et les pisteurs sont bloqués sur tous les appareils (téléphones, TV, consoles) sans rien installer dessus.");
        let s = serveur();
        let marche = s.marche.load(Relaxed);
        ui.horizontal(|ui| {
            if marche {
                ui.label(RichText::new("● Actif").color(Color32::LIGHT_GREEN).strong());
                if ui.button("⏹ Arrêter").clicked() {
                    arreter();
                    self.r.actif = false;
                    self.sauver();
                }
            } else {
                ui.label(RichText::new("● Arrêté").color(Color32::GRAY).strong());
                if ui.button("▶ Démarrer").clicked() {
                    self.r.actif = true;
                    self.sauver();
                    demarrer();
                }
            }
            if ui.button("⟳ Mettre la liste à jour").clicked() {
                let s2 = s.clone();
                std::thread::spawn(move || charger_liste(&s2, true));
            }
        });
        ui.label(s.etat.lock().unwrap().clone());
        ui.separator();
        ui.strong("Comment l'utiliser");
        let ip = ip_locale().unwrap_or_else(|| "l'adresse du PC".into());
        ui.label(format!("1. Dans les réglages de ta box (souvent http://192.168.1.1), section DHCP ou DNS, mets {ip} comme serveur DNS principal."));
        ui.label("2. Ou seulement sur un appareil : Wi-Fi › réglages du réseau › DNS manuel, avec la même adresse.");
        ui.weak("Le PC doit rester allumé avec Outils ouvert. Mets un DNS secondaire (1.1.1.1) dans la box : si le PC est éteint, internet marche quand même (sans le blocage). Windows peut demander d'autoriser Outils dans le pare-feu : accepte pour les réseaux privés.");
        ui.horizontal(|ui| {
            ui.label("Vrai DNS utilisé :");
            for (n, a) in [("Cloudflare", "1.1.1.1"), ("Google", "8.8.8.8"), ("Quad9", "9.9.9.9")] {
                if ui.selectable_label(self.r.amont == a, n).clicked() {
                    self.r.amont = a.into();
                    self.sauver();
                }
            }
        });
        ui.separator();
        let (q, b) = (s.stats.requetes.load(Relaxed), s.stats.bloquees.load(Relaxed));
        ui.label(RichText::new(format!("{q} demandes · {b} bloquées ({:.0} %)", if q > 0 { b as f64 * 100.0 / q as f64 } else { 0.0 })).size(16.0));
        ui.columns(2, |c| {
            c[0].strong("Les plus bloqués");
            let mut top: Vec<(String, u64)> = s.stats.top.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
            top.sort_by(|a, b| b.1.cmp(&a.1));
            let mut autoriser = None;
            for (n, k) in top.iter().take(12) {
                c[0].horizontal(|ui| {
                    ui.label(format!("{k} × {n}"));
                    if ui.small_button("✔").on_hover_text("Autoriser ce site (si quelque chose ne marche plus)").clicked() {
                        autoriser = Some(n.clone());
                    }
                });
            }
            if let Some(n) = autoriser {
                self.r.autorises.push(n);
                self.sauver();
            }
            c[1].strong("Par appareil");
            let mut ap: Vec<(String, (u64, u64))> = s.stats.appareils.lock().unwrap().iter().map(|(k, v)| (k.clone(), *v)).collect();
            ap.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            for (ip, (n, bl)) in ap.iter().take(12) {
                c[1].label(format!("{ip} : {n} demandes, {bl} bloquées"));
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.autoriser).hint_text("site à toujours autoriser").desired_width(200.0));
            if ui.button("Autoriser").clicked() && self.autoriser.contains('.') {
                self.r.autorises.push(self.autoriser.trim().to_lowercase());
                self.autoriser.clear();
                self.sauver();
            }
            ui.add(egui::TextEdit::singleline(&mut self.ajout).hint_text("site à bloquer en plus").desired_width(200.0));
            if ui.button("Bloquer").clicked() && self.ajout.contains('.') {
                let d = self.ajout.trim().to_lowercase();
                s.liste.write().unwrap().insert(d.clone());
                self.r.bloques_en_plus.push(d);
                self.ajout.clear();
                self.sauver();
            }
        });
        let mut retirer = None;
        ui.horizontal_wrapped(|ui| {
            for (k, a) in self.r.autorises.iter().enumerate() {
                if ui.small_button(format!("✔ {a} ✖")).clicked() {
                    retirer = Some(k);
                }
            }
        });
        if let Some(k) = retirer {
            self.r.autorises.remove(k);
            self.sauver();
        }
    }
    fn resume(&self) -> Option<String> {
        let s = serveur();
        s.marche.load(Relaxed).then(|| format!("Actif · {} pubs bloquées", s.stats.bloquees.load(Relaxed)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requete(nom: &str, genre: u16) -> Vec<u8> {
        let mut p = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for x in nom.split('.') {
            p.push(x.len() as u8);
            p.extend_from_slice(x.as_bytes());
        }
        p.push(0);
        p.extend_from_slice(&genre.to_be_bytes());
        p.extend_from_slice(&[0, 1]);
        p
    }

    #[test]
    fn dns() {
        let r = requete("ads.Exemple.com", 1);
        let (n, g, fin) = question(&r).unwrap();
        assert_eq!((n.as_str(), g, fin), ("ads.exemple.com", 1, r.len()));
        let l = lire_hosts("# titre\n0.0.0.0 exemple.com\n127.0.0.1 localhost\n0.0.0.0 pistage.net # commentaire\n");
        assert_eq!(l.len(), 2);
        assert!(bloque("ads.exemple.com", &l, &[]));
        assert!(bloque("exemple.com", &l, &[]));
        assert!(!bloque("exemple.org", &l, &[]));
        assert!(!bloque("ads.exemple.com", &l, &["exemple.com".into()]));
        let rep = reponse_bloquee(&r, fin, 1);
        assert_eq!(&rep[..2], &[0x12, 0x34]);
        assert_eq!(rep[3] & 0x0F, 0);
        assert_eq!(u16::from_be_bytes([rep[6], rep[7]]), 1);
        assert_eq!(&rep[rep.len() - 4..], &[0, 0, 0, 0]);
        let rep6 = reponse_bloquee(&requete("a.b", 28), requete("a.b", 28).len(), 28);
        assert_eq!(rep6.len(), requete("a.b", 28).len() + 12 + 16);
        assert!(question(&[0; 5]).is_none());
    }
}
