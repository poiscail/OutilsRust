//! Réseau : bloqueur de pubs et de sites (111), DNS le plus rapide (112), mots de passe Wi-Fi (113), ports ouverts (114),
//! historique des coupures (117), réveil à distance Wake-on-LAN (118), téléchargement de vidéos (119), liens courts (121),
//! surveillance de pages web (122).
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::pressepapiers::{decalage, maintenant_utc};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SANS_FENETRE: u32 = 0x0800_0000;

fn sortie(prog: &str, args: &[&str]) -> String {
    Command::new("cmd").args(["/C", &format!("chcp 65001>nul & {prog} {}", args.join(" "))]).creation_flags(SANS_FENETRE).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Outils").build()
}

type Etat = Arc<Mutex<(bool, String)>>;

fn en_fond(e: &Etat, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, "En cours…".into());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    o
}

// ---------------------------------------------------------------------------------------------------
// 111 · Bloqueur de pubs et de sites
// ---------------------------------------------------------------------------------------------------

/// Domaines d'une liste au format hosts (« 0.0.0.0 domaine »).
pub fn lire_liste_hosts(t: &str) -> Vec<String> {
    t.lines().filter_map(|l| {
        let l = l.split('#').next()?.trim();
        let mut p = l.split_whitespace();
        let ip = p.next()?;
        let d = p.next()?;
        (["0.0.0.0", "127.0.0.1"].contains(&ip) && d != "localhost" && d.contains('.')).then(|| d.to_lowercase())
    }).collect()
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default)]
struct Bloqueur {
    sites: String,
}

pub struct BloqueurUi {
    r: Bloqueur,
    e: Etat,
}

impl BloqueurUi {
    pub fn new() -> Self {
        BloqueurUi { r: charger("bloqueur"), e: Arc::default() }
    }
}

impl Outil for BloqueurUi {
    fn nom(&self) -> &'static str {
        "🚫 Bloqueur de pubs et de sites"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bloqueur de pubs et de sites");
        ui.weak("Bloque des domaines pour tout le PC (navigateurs, applis, jeux) via le fichier hosts. Demande les droits administrateur.");
        let occupe = etat_ui(ui, &self.e);
        let (pubs, sites) = (crate::hosts::bloques("pubs").len(), crate::hosts::bloques("sites").len());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Pubs et pisteurs : {}", if pubs > 0 { format!("{pubs} domaines bloqués") } else { "inactif".into() })).strong());
            if ui.add_enabled(!occupe, egui::Button::new("Activer / mettre à jour")).on_hover_text("Liste AdAway (quelques milliers de domaines publicitaires)").clicked() {
                en_fond(&self.e, || {
                    let t = agent().get("https://adaway.org/hosts.txt").call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
                    let d = lire_liste_hosts(&t);
                    crate::hosts::appliquer("pubs", &d)?;
                    Ok(format!("✔ {} domaines publicitaires bloqués.", d.len()))
                });
            }
            if pubs > 0 && ui.add_enabled(!occupe, egui::Button::new("Désactiver")).clicked() {
                en_fond(&self.e, || crate::hosts::appliquer("pubs", &[]).map(|_| "Bloqueur de pubs désactivé.".into()));
            }
        });
        ui.separator();
        ui.strong(format!("Mes sites bloqués ({sites})"));
        ui.add(egui::TextEdit::multiline(&mut self.r.sites).hint_text("un site par ligne, ex. tiktok.com").desired_rows(6).desired_width(360.0));
        if ui.add_enabled(!occupe, egui::Button::new("Appliquer")).clicked() {
            enregistrer("bloqueur", &self.r);
            let l: Vec<String> = self.r.sites.lines().map(str::to_string).collect();
            en_fond(&self.e, move || crate::hosts::appliquer("sites", &l).map(|_| "✔ Liste appliquée.".into()));
        }
        ui.weak("Un site débloqué peut mettre une minute à revenir (cache du navigateur).");
    }
}

// ---------------------------------------------------------------------------------------------------
// 112 · DNS le plus rapide
// ---------------------------------------------------------------------------------------------------

/// Requête DNS minimale (type A, récursion demandée).
pub fn requete_dns(id: u16, domaine: &str) -> Vec<u8> {
    let mut q = vec![];
    q.extend(id.to_be_bytes());
    q.extend([0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for p in domaine.split('.') {
        q.push(p.len() as u8);
        q.extend(p.as_bytes());
    }
    q.extend([0, 0, 1, 0, 1]);
    q
}

/// Temps de réponse médian (ms) d'un serveur DNS, ou None s'il ne répond pas.
fn mesurer_dns(serveur: &str) -> Option<f64> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.set_read_timeout(Some(Duration::from_millis(1500))).ok()?;
    let cible: SocketAddr = format!("{serveur}:53").parse().ok()?;
    let mut temps = vec![];
    for (i, d) in ["google.com", "youtube.com", "discord.com", "steampowered.com", "wikipedia.org", "twitch.tv", "amazon.fr", "github.com"].iter().enumerate() {
        // sous-domaine aléatoire : oblige le serveur à vraiment résoudre (pas de cache)
        let nom = if i % 2 == 0 { d.to_string() } else { format!("x{}.{d}", crate::outil::hasard(1_000_000)) };
        let t = Instant::now();
        s.send_to(&requete_dns(i as u16 + 1, &nom), cible).ok()?;
        let mut b = [0u8; 512];
        if s.recv_from(&mut b).is_ok() {
            temps.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if temps.len() < 4 {
        return None;
    }
    temps.sort_by(f64::total_cmp);
    Some(temps[temps.len() / 2])
}

const DNS: [(&str, &str, &str); 6] = [
    ("Cloudflare", "1.1.1.1", "1.0.0.1"),
    ("Google", "8.8.8.8", "8.8.4.4"),
    ("Quad9 (bloque les sites malveillants)", "9.9.9.9", "149.112.112.112"),
    ("OpenDNS", "208.67.222.222", "208.67.220.220"),
    ("AdGuard (bloque les pubs)", "94.140.14.14", "94.140.15.15"),
    ("Cloudflare Famille (filtre adulte)", "1.1.1.3", "1.0.0.3"),
];

pub struct DnsRapide {
    res: Arc<Mutex<Vec<(usize, Option<f64>)>>>,
    actuels: Arc<Mutex<String>>,
    e: Etat,
}

impl DnsRapide {
    pub fn new() -> Self {
        DnsRapide { res: Arc::default(), actuels: Arc::default(), e: Arc::default() }
    }
}

const SCRIPT_IFACE: &str = "(Get-NetRoute -DestinationPrefix 0.0.0.0/0 | Sort-Object RouteMetric | Select-Object -First 1).InterfaceAlias";

impl Outil for DnsRapide {
    fn nom(&self) -> &'static str {
        "🧭 DNS le plus rapide"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("DNS le plus rapide");
        ui.weak("Le DNS traduit les noms de sites en adresses : un DNS rapide accélère le chargement des pages et des lanceurs de jeux.");
        let occupe = etat_ui(ui, &self.e);
        if self.actuels.lock().unwrap().is_empty() {
            *self.actuels.lock().unwrap() = "…".into();
            let a = self.actuels.clone();
            std::thread::spawn(move || {
                let s = crate::optimiseur::ps("Get-DnsClientServerAddress -AddressFamily IPv4 | Where-Object ServerAddresses | ForEach-Object { $_.InterfaceAlias + ' : ' + ($_.ServerAddresses -join ', ') }").unwrap_or_default();
                *a.lock().unwrap() = s.trim().to_string();
            });
        }
        ui.label(format!("DNS actuels : {}", self.actuels.lock().unwrap()));
        if ui.add_enabled(!occupe, egui::Button::new("⏱ Mesurer")).clicked() {
            let r = self.res.clone();
            en_fond(&self.e, move || {
                let mut v: Vec<(usize, Option<f64>)> = std::thread::scope(|s| {
                    let h: Vec<_> = DNS.iter().enumerate().map(|(i, d)| s.spawn(move || (i, mesurer_dns(d.1)))).collect();
                    h.into_iter().filter_map(|x| x.join().ok()).collect()
                });
                v.sort_by(|a, b| a.1.unwrap_or(9e9).total_cmp(&b.1.unwrap_or(9e9)));
                *r.lock().unwrap() = v;
                Ok("Mesure terminée.".into())
            });
        }
        let res = self.res.lock().unwrap().clone();
        let mut choisir = None;
        for (k, (i, ms)) in res.iter().enumerate() {
            let (nom, a, b) = DNS[*i];
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}{nom}", if k == 0 { "🏆 " } else { "" })).strong());
                ui.weak(format!("{a}, {b}"));
                match ms {
                    Some(m) => ui.label(format!("{m:.0} ms")),
                    None => ui.colored_label(Color32::LIGHT_RED, "ne répond pas"),
                };
                if ms.is_some() && ui.small_button("Utiliser").clicked() {
                    choisir = Some((a, b));
                }
            });
        }
        if let Some((a, b)) = choisir {
            let act = self.actuels.clone();
            en_fond(&self.e, move || {
                let i = crate::optimiseur::ps(SCRIPT_IFACE)?.trim().to_string();
                crate::optimiseur::ps(&format!("Set-DnsClientServerAddress -InterfaceAlias '{}' -ServerAddresses ('{a}','{b}'); Clear-DnsClientCache", i.replace('\'', "''")))
                    .map_err(|e| format!("{e} (droits administrateur nécessaires)"))?;
                act.lock().unwrap().clear();
                Ok(format!("✔ DNS {a} / {b} appliqués sur « {i} »."))
            });
        }
        if ui.add_enabled(!occupe, egui::Button::new("Revenir aux DNS de la box")).clicked() {
            let act = self.actuels.clone();
            en_fond(&self.e, move || {
                let i = crate::optimiseur::ps(SCRIPT_IFACE)?.trim().to_string();
                crate::optimiseur::ps(&format!("Set-DnsClientServerAddress -InterfaceAlias '{}' -ResetServerAddresses", i.replace('\'', "''")))?;
                act.lock().unwrap().clear();
                Ok("DNS automatiques (box) rétablis.".into())
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 113 · Mots de passe Wi-Fi enregistrés
// ---------------------------------------------------------------------------------------------------

/// Valeur après « : » des lignes dont le libellé contient l'un des mots (français ou anglais).
pub fn valeurs(t: &str, libelles: &[&str]) -> Vec<String> {
    t.lines().filter(|l| libelles.iter().any(|x| l.contains(x))).filter_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string())).filter(|v| !v.is_empty()).collect()
}

pub struct WifiMdp {
    liste: Option<Vec<(String, Option<String>)>>,
    montrer: bool,
}

impl WifiMdp {
    pub fn new() -> Self {
        WifiMdp { liste: None, montrer: false }
    }
}

impl Outil for WifiMdp {
    fn nom(&self) -> &'static str {
        "📶 Mots de passe Wi-Fi"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mots de passe Wi-Fi enregistrés");
        if self.liste.is_none() {
            let profils = valeurs(&sortie("netsh", &["wlan", "show", "profiles"]), &["Profil Tous les utilisateurs", "All User Profile"]);
            self.liste = Some(profils.into_iter().map(|p| {
                let d = sortie("netsh", &["wlan", "show", "profile", &format!("name=\"{p}\""), "key=clear"]);
                let k = valeurs(&d, &["Contenu de la clé", "Contenu de la cl", "Key Content"]).into_iter().next();
                (p, k)
            }).collect());
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.montrer, "Afficher les mots de passe");
            if ui.button("⟳").clicked() {
                self.liste = None;
            }
        });
        let l = self.liste.clone().unwrap_or_default();
        if l.is_empty() {
            ui.weak("Aucun réseau Wi-Fi enregistré (PC en câble ?).");
        }
        egui::Grid::new("wifi").num_columns(3).striped(true).spacing([20.0, 6.0]).show(ui, |ui| {
            for (n, k) in &l {
                ui.strong(n);
                match k {
                    Some(k) => {
                        ui.monospace(if self.montrer { k.clone() } else { "•".repeat(k.chars().count().min(16)) });
                        if ui.small_button("📋").clicked() {
                            ui.ctx().copy_text(k.clone());
                        }
                    }
                    None => {
                        ui.weak("ouvert ou entreprise");
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 114 · Ports ouverts
// ---------------------------------------------------------------------------------------------------

fn ports_ecoute() -> Vec<(String, u16, String)> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    let mut v = vec![];
    unsafe {
        let mut n = 0u32;
        GetExtendedTcpTable(std::ptr::null_mut(), &mut n, 0, 2, TCP_TABLE_OWNER_PID_LISTENER, 0);
        let mut b = vec![0u8; n as usize + 4096];
        let mut n = b.len() as u32;
        if GetExtendedTcpTable(b.as_mut_ptr().cast(), &mut n, 1, 2, TCP_TABLE_OWNER_PID_LISTENER, 0) == 0 {
            let t = b.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
            for r in std::slice::from_raw_parts((*t).table.as_ptr(), (*t).dwNumEntries as usize) {
                let ip = std::net::Ipv4Addr::from(u32::from_be(r.dwLocalAddr));
                v.push((format!("TCP {}", if ip.is_loopback() { "local" } else { "réseau" }), u16::from_be(r.dwLocalPort as u16), crate::reseau::nom_processus(r.dwOwningPid)));
            }
        }
        let mut n = 0u32;
        GetExtendedUdpTable(std::ptr::null_mut(), &mut n, 0, 2, UDP_TABLE_OWNER_PID, 0);
        let mut b = vec![0u8; n as usize + 4096];
        let mut n = b.len() as u32;
        if GetExtendedUdpTable(b.as_mut_ptr().cast(), &mut n, 1, 2, UDP_TABLE_OWNER_PID, 0) == 0 {
            let t = b.as_ptr() as *const MIB_UDPTABLE_OWNER_PID;
            for r in std::slice::from_raw_parts((*t).table.as_ptr(), (*t).dwNumEntries as usize) {
                let ip = std::net::Ipv4Addr::from(u32::from_be(r.dwLocalAddr));
                v.push((format!("UDP {}", if ip.is_loopback() { "local" } else { "réseau" }), u16::from_be(r.dwLocalPort as u16), crate::reseau::nom_processus(r.dwOwningPid)));
            }
        }
    }
    v.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    v.dedup();
    v
}

pub fn port_connu(p: u16) -> &'static str {
    match p {
        25565 => "Minecraft Java",
        19132 => "Minecraft Bedrock",
        27015..=27030 => "Steam / jeux Source",
        3074 => "Xbox Live / Call of Duty",
        80 | 443 | 8080 => "serveur web",
        8642 => "Outils · Téléphone",
        445 | 139 => "partage de fichiers Windows",
        3389 => "Bureau à distance",
        5353 => "découverte locale (mDNS)",
        1900 => "UPnP",
        _ => "",
    }
}

pub struct Ports {
    liste: (Option<Instant>, Vec<(String, u16, String)>),
    reseau_seul: bool,
}

impl Ports {
    pub fn new() -> Self {
        Ports { liste: (None, vec![]), reseau_seul: true }
    }
}

impl Outil for Ports {
    fn nom(&self) -> &'static str {
        "🔌 Ports ouverts"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ports ouverts");
        if self.liste.0.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.liste = (Some(Instant::now()), ports_ecoute());
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        ui.checkbox(&mut self.reseau_seul, "Seulement ceux visibles depuis le réseau (pas « local »)");
        egui::ScrollArea::vertical().id_salt("ports").show(ui, |ui| {
            egui::Grid::new("g_ports").num_columns(4).striped(true).spacing([18.0, 4.0]).show(ui, |ui| {
                for (t, p, a) in self.liste.1.iter().filter(|x| !self.reseau_seul || x.0.ends_with("réseau")) {
                    ui.monospace(p.to_string());
                    ui.weak(t);
                    ui.label(a);
                    ui.weak(port_connu(*p));
                    ui.end_row();
                }
            });
        });
        ui.weak("Un port « réseau » peut être joint par les autres appareils de ta maison ; depuis Internet, seulement s'il est ouvert sur la box.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 117 · Historique des coupures
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Coupures {
    liste: Vec<(i64, i64)>, // début, fin (UTC)
}

fn internet_ok() -> bool {
    ["1.1.1.1:443", "8.8.8.8:443", "9.9.9.9:443"].iter().any(|a| TcpStream::connect_timeout(&a.parse().unwrap(), Duration::from_secs(3)).is_ok())
}

pub struct HistoCoupures {
    c: Arc<Mutex<Coupures>>,
    en_cours: Arc<Mutex<Option<i64>>>,
}

impl HistoCoupures {
    pub fn new() -> Self {
        let c = Arc::new(Mutex::new(charger::<Coupures>("coupures")));
        let en_cours: Arc<Mutex<Option<i64>>> = Arc::default();
        let (c2, e2) = (c.clone(), en_cours.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            let ok = internet_ok();
            let mut e = e2.lock().unwrap();
            match (ok, *e) {
                (false, None) => *e = Some(maintenant_utc() - 10),
                (true, Some(d)) => {
                    *e = None;
                    let mut x = c2.lock().unwrap();
                    x.liste.push((d, maintenant_utc()));
                    let n = x.liste.len();
                    if n > 500 {
                        x.liste.drain(..n - 500);
                    }
                    enregistrer("coupures", &*x);
                    crate::systeme::notifier("Internet", &format!("Connexion revenue après {} s de coupure.", maintenant_utc() - d));
                }
                _ => {}
            }
        });
        HistoCoupures { c, en_cours }
    }
}

impl Outil for HistoCoupures {
    fn nom(&self) -> &'static str {
        "📉 Coupures internet"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Historique des coupures internet");
        ui.weak("Outils vérifie la connexion toutes les 20 s (3 serveurs différents). Pratique comme preuve pour le service client de ta box.");
        let off = decalage();
        match *self.en_cours.lock().unwrap() {
            Some(d) => ui.colored_label(Color32::LIGHT_RED, format!("🔴 Coupure en cours depuis {} s", maintenant_utc() - d)),
            None => ui.colored_label(Color32::LIGHT_GREEN, "🟢 Connexion OK"),
        };
        ui.ctx().request_repaint_after(Duration::from_secs(5));
        let c = self.c.lock().unwrap().clone();
        let semaine: i64 = c.liste.iter().filter(|x| x.1 > maintenant_utc() - 7 * 86400).map(|x| x.1 - x.0).sum();
        ui.label(format!("{} coupure(s) enregistrée(s) · {} de coupure ces 7 derniers jours", c.liste.len(), crate::quotidien::duree(semaine as u64).replace(" min", " min")));
        if ui.button("📋 Copier le rapport").clicked() {
            let t: String = c.liste.iter().map(|(d, f)| format!("{} → {} ({} s)\n", crate::notes::format_local(*d, off), crate::notes::format_local(*f, off), f - d)).collect();
            ui.ctx().copy_text(t);
        }
        egui::ScrollArea::vertical().id_salt("coupures").show(ui, |ui| {
            for (d, f) in c.liste.iter().rev() {
                ui.label(format!("{} · {} s", crate::notes::format_local(*d, off), f - d));
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 118 · Wake-on-LAN
// ---------------------------------------------------------------------------------------------------

pub fn lire_mac(s: &str) -> Option<[u8; 6]> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if h.len() != 12 {
        return None;
    }
    let mut m = [0u8; 6];
    for i in 0..6 {
        m[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(m)
}

/// 6 × FF puis 16 fois l'adresse MAC.
pub fn paquet_magique(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFF; 6];
    for _ in 0..16 {
        p.extend(mac);
    }
    p
}

pub struct Reveil {
    machines: Vec<(String, String)>,
    nom: String,
    mac: String,
    message: String,
    moi: Option<String>,
}

impl Reveil {
    pub fn new() -> Self {
        Reveil { machines: charger("wol"), nom: String::new(), mac: String::new(), message: String::new(), moi: None }
    }
}

impl Outil for Reveil {
    fn nom(&self) -> &'static str {
        "⏰ Allumer un PC à distance"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Allumer un PC à distance (Wake-on-LAN)");
        if self.moi.is_none() {
            self.moi = Some(sortie("getmac", &["/fo", "csv", "/nh", "/v"]).lines().filter(|l| !l.contains("N/A") && !l.contains("Media disconnected") && !l.contains("Média déconnecté"))
                .filter_map(|l| {
                    let c: Vec<&str> = l.split("\",\"").collect();
                    Some(format!("{} : {}", c.first()?.trim_matches('"'), c.get(2)?.trim_matches('"')))
                }).collect::<Vec<_>>().join("\n"));
        }
        ui.label("Adresse MAC de ce PC (à noter pour l'allumer depuis un autre appareil) :");
        ui.monospace(self.moi.clone().unwrap_or_default());
        ui.weak("Pour qu'un PC s'allume ainsi : activer « Wake on LAN » dans le BIOS et dans les propriétés de la carte réseau, et le brancher en câble.");
        ui.separator();
        let mut envoyer = None;
        let mut retirer = None;
        for (i, (n, m)) in self.machines.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(format!("⏻ Allumer {n}")).clicked() {
                    envoyer = Some(m.clone());
                }
                ui.weak(m);
                if ui.small_button("🗑").clicked() {
                    retirer = Some(i);
                }
            });
        }
        if let Some(i) = retirer {
            self.machines.remove(i);
            enregistrer("wol", &self.machines);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("nom").desired_width(120.0));
            ui.add(egui::TextEdit::singleline(&mut self.mac).hint_text("AA:BB:CC:DD:EE:FF").desired_width(160.0));
            if ui.button("➕").clicked() && lire_mac(&self.mac).is_some() {
                self.machines.push((std::mem::take(&mut self.nom), std::mem::take(&mut self.mac)));
                enregistrer("wol", &self.machines);
            }
        });
        if let Some(m) = envoyer.and_then(|m| lire_mac(&m)) {
            let r = UdpSocket::bind("0.0.0.0:0").and_then(|s| {
                s.set_broadcast(true)?;
                for port in [9, 7] {
                    s.send_to(&paquet_magique(m), ("255.255.255.255", port))?;
                }
                Ok(())
            });
            self.message = match r {
                Ok(()) => "✔ Signal envoyé : le PC devrait démarrer d'ici quelques secondes.".into(),
                Err(e) => format!("✖ {e}"),
            };
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 119 · Téléchargement de vidéos (yt-dlp)
// ---------------------------------------------------------------------------------------------------

pub fn progression_ytdlp(l: &str) -> Option<f32> {
    let i = l.find("[download]")?;
    let p = l[i + 10..].trim_start().split('%').next()?.trim();
    p.parse::<f32>().ok().map(|x| x / 100.0)
}

pub struct Telechargeur {
    url: String,
    audio: bool,
    qualite: usize,
    e: Arc<Mutex<(bool, f32, String)>>,
}

impl Telechargeur {
    pub fn new() -> Self {
        Telechargeur { url: String::new(), audio: false, qualite: 0, e: Arc::new(Mutex::new((false, 0.0, String::new()))) }
    }
}

impl Outil for Telechargeur {
    fn nom(&self) -> &'static str {
        "⬇ Télécharger une vidéo"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Télécharger une vidéo");
        ui.weak("YouTube, Twitch (clips, rediffusions), et des centaines d'autres sites, via yt-dlp (winget install yt-dlp.yt-dlp ; ffmpeg conseillé). Uniquement pour ce que tu as le droit de garder.");
        let (occupe, p, m) = self.e.lock().unwrap().clone();
        ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("adresse de la vidéo").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.audio, false, "🎞 Vidéo (MP4)");
            ui.selectable_value(&mut self.audio, true, "🎵 Son seulement (MP3)");
            if !self.audio {
                for (i, q) in ["meilleure", "1080p", "720p"].iter().enumerate() {
                    ui.selectable_value(&mut self.qualite, i, *q);
                }
            }
        });
        if ui.add_enabled(!occupe && self.url.starts_with("http"), egui::Button::new("⬇ Télécharger")).clicked() {
            let dest = std::path::PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads");
            let mut a: Vec<String> = vec!["--newline".into(), "-P".into(), dest.display().to_string(), "-o".into(), "%(title)s.%(ext)s".into()];
            if self.audio {
                a.extend(["-x", "--audio-format", "mp3"].map(String::from));
            } else {
                let h = ["", "[height<=1080]", "[height<=720]"][self.qualite];
                a.extend(["-f".into(), format!("bv*{h}[ext=mp4]+ba[ext=m4a]/b{h}[ext=mp4]/bv*{h}+ba/b"), "--merge-output-format".into(), "mp4".into()]);
            }
            a.push(self.url.clone());
            let e = self.e.clone();
            *e.lock().unwrap() = (true, 0.0, "Démarrage…".into());
            std::thread::spawn(move || {
                use std::io::BufRead;
                let r = Command::new("yt-dlp").args(&a).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).creation_flags(SANS_FENETRE).spawn();
                let mut c = match r {
                    Ok(c) => c,
                    Err(_) => {
                        *e.lock().unwrap() = (false, 0.0, "yt-dlp introuvable : winget install yt-dlp.yt-dlp".into());
                        return;
                    }
                };
                let mut fichier = String::new();
                if let Some(o) = c.stdout.take() {
                    for l in std::io::BufReader::new(o).lines().map_while(Result::ok) {
                        if let Some(x) = progression_ytdlp(&l) {
                            e.lock().unwrap().1 = x;
                        }
                        if let Some(d) = l.split("Destination: ").nth(1) {
                            fichier = d.trim().to_string();
                            e.lock().unwrap().2 = format!("→ {fichier}");
                        }
                    }
                }
                let ok = c.wait().is_ok_and(|s| s.success());
                let mut err = String::new();
                if !ok {
                    if let Some(mut s) = c.stderr.take() {
                        use std::io::Read;
                        let _ = s.read_to_string(&mut err);
                    }
                }
                *e.lock().unwrap() = (false, 1.0, if ok { format!("✔ Téléchargé dans Téléchargements {fichier}") } else { format!("✖ {}", err.lines().last().unwrap_or("échec")) });
            });
        }
        if occupe {
            ui.add(egui::ProgressBar::new(p).show_percentage());
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        if !m.is_empty() {
            ui.label(m);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 121 · Liens courts
// ---------------------------------------------------------------------------------------------------

pub struct LiensCourts {
    url: String,
    historique: Vec<(String, String)>,
    e: Etat,
    dernier: Arc<Mutex<Option<(String, String)>>>,
}

impl LiensCourts {
    pub fn new() -> Self {
        LiensCourts { url: String::new(), historique: charger("liens_courts"), e: Arc::default(), dernier: Arc::default() }
    }
}

impl Outil for LiensCourts {
    fn nom(&self) -> &'static str {
        "🔗 Liens courts"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Raccourcir un lien");
        if let Some(x) = self.dernier.lock().unwrap().take() {
            ui.ctx().copy_text(x.1.clone());
            self.historique.insert(0, x);
            self.historique.truncate(50);
            enregistrer("liens_courts", &self.historique);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("https://…").desired_width(400.0));
            let occupe = self.e.lock().unwrap().0;
            if ui.add_enabled(!occupe && self.url.starts_with("http"), egui::Button::new("✂ Raccourcir")).clicked() {
                let (u, d) = (self.url.clone(), self.dernier.clone());
                en_fond(&self.e, move || {
                    let court = agent().get("https://is.gd/create.php").query("format", "simple").query("url", &u).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
                    let court = court.trim().to_string();
                    if !court.starts_with("https://is.gd/") {
                        return Err(court);
                    }
                    *d.lock().unwrap() = Some((u, court.clone()));
                    Ok(format!("✔ {court} (copié)"))
                });
            }
        });
        etat_ui(ui, &self.e);
        ui.weak("Service gratuit is.gd ; les liens raccourcis sont publics.");
        for (l, c) in &self.historique {
            ui.horizontal(|ui| {
                if ui.small_button("📋").clicked() {
                    ui.ctx().copy_text(c.clone());
                }
                ui.monospace(c);
                ui.weak(l.chars().take(70).collect::<String>());
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 122 · Surveillance de pages web
// ---------------------------------------------------------------------------------------------------

/// Texte visible d'une page HTML (balises, scripts et styles retirés, espaces resserrés).
pub fn texte_html(h: &str) -> String {
    let re_bloc = regex::Regex::new(r"(?is)<(script|style|noscript)[^>]*>.*?</(script|style|noscript)>").unwrap();
    let re_balise = regex::Regex::new(r"(?s)<[^>]+>").unwrap();
    let s = re_bloc.replace_all(h, " ");
    let s = re_balise.replace_all(&s, "\n");
    let s = s.replace("&nbsp;", " ").replace("&amp;", "&").replace("&#39;", "'").replace("&quot;", "\"").replace("&euro;", "€");
    s.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Première ligne qui a changé (pour l'alerte).
pub fn difference(avant: &str, apres: &str) -> Option<String> {
    let a: std::collections::HashSet<&str> = avant.lines().collect();
    apres.lines().find(|l| !a.contains(l)).map(|l| l.chars().take(140).collect())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Page {
    pub url: String,
    pub nom: String,
    pub minutes: u32,
    pub mot: String, // si rempli : alerte quand le mot apparaît / disparaît
    pub texte: String,
    pub derniere: i64,
    pub changement: String,
}

pub struct Surveillance {
    pages: Arc<Mutex<Vec<Page>>>,
    url: String,
}

impl Surveillance {
    pub fn new() -> Self {
        let pages: Arc<Mutex<Vec<Page>>> = Arc::new(Mutex::new(charger("surveillance")));
        let p2 = pages.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            let a_faire: Vec<(usize, Page)> = p2.lock().unwrap().iter().cloned().enumerate().filter(|(_, p)| maintenant_utc() - p.derniere >= p.minutes.max(5) as i64 * 60).collect();
            for (i, p) in a_faire {
                let Ok(h) = agent().get(&p.url).call().and_then(|r| Ok(r.into_string()?)) else { continue };
                let t = texte_html(&h);
                let mut x = p2.lock().unwrap();
                let Some(q) = x.get_mut(i).filter(|q| q.url == p.url) else { continue };
                let premiere = q.texte.is_empty();
                let alerte = if !q.mot.trim().is_empty() {
                    let (avant, apres) = (q.texte.to_lowercase().contains(&q.mot.to_lowercase()), t.to_lowercase().contains(&q.mot.to_lowercase()));
                    (avant != apres && !premiere).then(|| format!("« {} » {}", q.mot, if apres { "apparaît" } else { "a disparu" }))
                } else if !premiere && q.texte != t {
                    Some(difference(&q.texte, &t).unwrap_or_else(|| "du texte a été retiré".into()))
                } else {
                    None
                };
                if let Some(a) = alerte {
                    crate::systeme::notifier_toujours(&format!("Page modifiée : {}", q.nom), &a);
                    q.changement = format!("{} · {a}", crate::notes::format_local(maintenant_utc(), decalage()));
                }
                q.texte = t;
                q.derniere = maintenant_utc();
                enregistrer("surveillance", &*x);
            }
        });
        Surveillance { pages, url: String::new() }
    }
}

impl Outil for Surveillance {
    fn nom(&self) -> &'static str {
        "👀 Surveiller une page web"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Surveiller une page web");
        ui.weak("Notification quand une page change : retour en stock, nouvelle annonce, résultats… (pages publiques seulement, sans connexion).");
        let avant = self.pages.lock().unwrap().clone();
        let mut pages = avant.clone();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("https://…").desired_width(380.0));
            if ui.button("➕ Surveiller").clicked() && self.url.starts_with("http") {
                let nom = self.url.split('/').nth(2).unwrap_or("page").to_string();
                pages.push(Page { url: std::mem::take(&mut self.url), nom, minutes: 30, ..Default::default() });
            }
        });
        let mut retirer = None;
        for (i, p) in pages.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.nom).desired_width(160.0));
                    if ui.link(p.url.chars().take(60).collect::<String>()).clicked() {
                        let _ = Command::new("explorer").arg(&p.url).spawn();
                    }
                    if ui.small_button("🗑").clicked() {
                        retirer = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("toutes les");
                    ui.add(egui::DragValue::new(&mut p.minutes).range(5..=1440).suffix(" min"));
                    ui.label("mot à guetter (facultatif)");
                    ui.add(egui::TextEdit::singleline(&mut p.mot).hint_text("ex. En stock").desired_width(140.0));
                    if ui.small_button("Vérifier maintenant").clicked() {
                        p.derniere = 0;
                    }
                });
                if p.derniere > 0 {
                    ui.weak(format!("Vérifiée le {}", crate::notes::format_local(p.derniere, decalage())));
                }
                if !p.changement.is_empty() {
                    ui.colored_label(Color32::from_rgb(255, 170, 200), format!("Dernier changement : {}", p.changement));
                }
            });
        }
        if let Some(i) = retirer {
            pages.remove(i);
        }
        if pages != avant {
            let mut x = self.pages.lock().unwrap();
            if *x == avant {
                *x = pages;
                enregistrer("surveillance", &*x);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listes_et_paquets() {
        assert_eq!(lire_liste_hosts("# pub\n0.0.0.0 ads.example.com\n127.0.0.1 localhost\n127.0.0.1 track.io # x\n"), vec!["ads.example.com", "track.io"]);
        let q = requete_dns(7, "a.bc");
        assert_eq!(q, vec![0, 7, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, b'a', 2, b'b', b'c', 0, 0, 1, 0, 1]);
        assert_eq!(lire_mac("aa-bb-cc-dd-ee-ff"), Some([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]));
        assert_eq!(lire_mac("zz"), None);
        let p = paquet_magique([1, 2, 3, 4, 5, 6]);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[6..12], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn textes() {
        assert_eq!(valeurs("    Profil Tous les utilisateurs : Maison\n    All User Profile : Box\n", &["Profil Tous les utilisateurs", "All User Profile"]), vec!["Maison", "Box"]);
        assert_eq!(progression_ytdlp("[download]  42.3% of 10.00MiB at 1.00MiB/s"), Some(0.423));
        assert_eq!(texte_html("<html><style>x{}</style><p>Prix&nbsp;: 10 &euro;</p><script>a()</script><p>En stock</p></html>"), "Prix : 10 €\nEn stock");
        assert_eq!(difference("a\nb", "a\nc"), Some("c".into()));
        assert_eq!(port_connu(25565), "Minecraft Java");
    }
}
