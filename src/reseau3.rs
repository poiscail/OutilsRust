//! Réseau, suite : appareils connectés à la box (42), point d'accès Wi-Fi (43), ping des serveurs de jeux (44),
//! consommation internet par appli (45), mot de passe fuité (46).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Etat = Arc<Mutex<(bool, String)>>;

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
    Some(ctx.load_texture(format!("qr_{texte}"), egui::ColorImage::from_gray([n, n], &px), egui::TextureOptions::NEAREST))
}

// ---------------------------------------------------------------------------------------------------
// 42 · Qui est connecté à ma box
// ---------------------------------------------------------------------------------------------------

/// Toutes les adresses d'un /24 sauf réseau et diffusion.
pub fn adresses_du_reseau(ip: Ipv4Addr) -> Vec<Ipv4Addr> {
    let o = ip.octets();
    (1..255).map(|d| Ipv4Addr::new(o[0], o[1], o[2], d)).collect()
}

pub fn mac_texte(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(":")
}

/// Adresse « privée » (changée au hasard par les téléphones récents) : 2e bit du 1er octet.
pub fn mac_aleatoire(m: &str) -> bool {
    u8::from_str_radix(m.get(..2).unwrap_or("00"), 16).is_ok_and(|b| b & 2 != 0)
}

pub(crate) fn ping(ip: Ipv4Addr) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    unsafe {
        let h = IcmpCreateFile();
        if h.is_null() || h as isize == -1 {
            return None;
        }
        let donnees = *b"outils";
        let mut rep = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + 64];
        let n = IcmpSendEcho(h, u32::from_ne_bytes(ip.octets()), donnees.as_ptr() as *const _, donnees.len() as u16, std::ptr::null(), rep.as_mut_ptr() as *mut _, rep.len() as u32, 700);
        IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(rep.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime)
    }
}

/// Table ARP : ip → mac.
pub(crate) fn table_arp() -> BTreeMap<Ipv4Addr, String> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    let mut m = BTreeMap::new();
    unsafe {
        let mut t: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
        if GetIpNetTable2(2 /* AF_INET */, &mut t) != 0 || t.is_null() {
            return m;
        }
        let lignes = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        for l in lignes {
            if l.PhysicalAddressLength == 6 && l.State >= 2 {
                let a = l.Address.Ipv4.sin_addr.S_un.S_addr;
                let ip = Ipv4Addr::from(u32::from_be(a));
                let mac = mac_texte(&l.PhysicalAddress[..6]);
                if mac != "00:00:00:00:00:00" && mac != "FF:FF:FF:FF:FF:FF" {
                    m.insert(ip, mac);
                }
            }
        }
        FreeMibTable(t as *const _);
    }
    m
}

pub(crate) fn nom_hote(ip: Ipv4Addr) -> Option<String> {
    use windows_sys::Win32::Networking::WinSock::*;
    unsafe {
        let mut wsa: WSADATA = std::mem::zeroed();
        WSAStartup(0x202, &mut wsa);
        let mut sa: SOCKADDR_IN = std::mem::zeroed();
        sa.sin_family = AF_INET;
        sa.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
        let mut nom = [0u16; 256];
        let r = GetNameInfoW(&sa as *const _ as *const SOCKADDR, std::mem::size_of::<SOCKADDR_IN>() as i32, nom.as_mut_ptr(), 256, std::ptr::null_mut(), 0, NI_NAMEREQD as i32);
        (r == 0).then(|| {
            let n = nom.iter().position(|c| *c == 0).unwrap_or(0);
            String::from_utf16_lossy(&nom[..n]).trim_end_matches(".home").trim_end_matches(".lan").trim_end_matches(".local").to_string()
        })
    }
}

pub(crate) fn passerelle() -> Option<Ipv4Addr> {
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    unsafe {
        let mut t: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(2, &mut t) != 0 || t.is_null() {
            return None;
        }
        let lignes = std::slice::from_raw_parts((*t).Table.as_ptr(), (*t).NumEntries as usize);
        let g = lignes.iter().filter(|l| l.DestinationPrefix.PrefixLength == 0).min_by_key(|l| l.Metric).map(|l| Ipv4Addr::from(u32::from_be(l.NextHop.Ipv4.sin_addr.S_un.S_addr)));
        FreeMibTable(t as *const _);
        g
    }
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Appareil {
    pub ip: String,
    pub mac: String,
    pub nom: String,
    pub ms: Option<u32>,
    pub surnom: String,
    pub vu: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Connus {
    surnoms: BTreeMap<String, String>, // mac → surnom
    vus: BTreeMap<String, String>,     // mac → première fois
}

pub(crate) fn scanner(prog: &Mutex<(usize, usize)>) -> Vec<Appareil> {
    let Some(moi) = crate::telephone::ip_locale().and_then(|s| s.parse::<Ipv4Addr>().ok()) else { return vec![] };
    let ips = adresses_du_reseau(moi);
    *prog.lock().unwrap() = (0, ips.len());
    let trouves: Arc<Mutex<BTreeMap<Ipv4Addr, Option<u32>>>> = Arc::default();
    let file = Arc::new(Mutex::new(ips));
    std::thread::scope(|s| {
        for _ in 0..48 {
            let (f, t) = (file.clone(), trouves.clone());
            s.spawn(move || loop {
                let Some(ip) = f.lock().unwrap().pop() else { break };
                let r = ping(ip);
                if r.is_some() {
                    t.lock().unwrap().insert(ip, r);
                }
                prog.lock().unwrap().0 += 1;
            });
        }
    });
    // les appareils qui ne répondent pas au ping (téléphones en veille) restent souvent dans la table ARP
    let arp = table_arp();
    let mut t = trouves.lock().unwrap().clone();
    for ip in arp.keys() {
        if ip.octets()[..3] == moi.octets()[..3] {
            t.entry(*ip).or_insert(None);
        }
    }
    t.insert(moi, Some(0));
    let mut v: Vec<Appareil> = t.into_iter().map(|(ip, ms)| Appareil { ip: ip.to_string(), mac: arp.get(&ip).cloned().unwrap_or_default(), nom: nom_hote(ip).unwrap_or_default(), ms, ..Default::default() }).collect();
    v.sort_by_key(|a| a.ip.parse::<Ipv4Addr>().map(u32::from).unwrap_or(0));
    v
}

pub struct Appareils {
    liste: Arc<Mutex<Option<Vec<Appareil>>>>,
    prog: Arc<Mutex<(usize, usize)>>,
    en_cours: Arc<AtomicBool>,
    connus: Connus,
    edition: Option<(String, String)>,
}

impl Appareils {
    pub fn new() -> Self {
        Appareils { liste: Arc::default(), prog: Arc::default(), en_cours: Arc::default(), connus: charger("appareils_reseau"), edition: None }
    }

    fn lancer(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (l, p, e) = (self.liste.clone(), self.prog.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let v = scanner(&p);
            *l.lock().unwrap() = Some(v);
            e.store(false, Relaxed);
        });
    }
}

impl Outil for Appareils {
    fn nom(&self) -> &'static str {
        "📶 Qui est sur mon Wi-Fi ?"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Qui est connecté à ma box ?");
        ui.weak("Tous les appareils du réseau de la maison : PC, téléphones, consoles, TV, objets connectés. Donne-leur un surnom pour repérer un intrus.");
        let en_cours = self.en_cours.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!en_cours, egui::Button::new(RichText::new("🔎 Scanner le réseau").size(16.0))).clicked() {
                self.lancer();
            }
            if en_cours {
                let (a, b) = *self.prog.lock().unwrap();
                ui.add(egui::ProgressBar::new(a as f32 / b.max(1) as f32).desired_width(200.0));
                ui.ctx().request_repaint_after(Duration::from_millis(200));
            }
        });
        let Some(v) = self.liste.lock().unwrap().clone() else {
            if !en_cours {
                self.lancer();
            }
            return;
        };
        let moi = crate::telephone::ip_locale().unwrap_or_default();
        let box_ = passerelle().map(|i| i.to_string()).unwrap_or_default();
        let mut change = false;
        for a in &v {
            if !a.mac.is_empty() && !self.connus.vus.contains_key(&a.mac) {
                self.connus.vus.insert(a.mac.clone(), crate::minecraft::horodatage().get(..10).unwrap_or("").to_string());
                change = true;
            }
        }
        let nouveaux = v.iter().filter(|a| self.connus.vus.get(&a.mac).is_some_and(|d| crate::minecraft::horodatage().starts_with(d.as_str())) && self.connus.surnoms.get(&a.mac).is_none()).count();
        ui.label(format!("{} appareil(s) trouvé(s){}", v.len(), if nouveaux > 0 { format!(" · {nouveaux} vu(s) pour la première fois aujourd'hui") } else { String::new() }));
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("appareils").striped(true).spacing([14.0, 5.0]).show(ui, |ui| {
                for t in ["", "Adresse", "Nom", "Surnom", "MAC", "Ping"] {
                    ui.strong(t);
                }
                ui.end_row();
                for a in &v {
                    let icone = if a.ip == moi { "💻" } else if a.ip == box_ { "📡" } else if mac_aleatoire(&a.mac) { "📱" } else { "🔹" };
                    ui.label(icone);
                    ui.label(&a.ip);
                    ui.label(if a.ip == moi { "ce PC".to_string() } else if a.ip == box_ { "la box".to_string() } else { a.nom.clone() });
                    let surnom = self.connus.surnoms.get(&a.mac).cloned().unwrap_or_default();
                    match &mut self.edition {
                        Some((m, s)) if *m == a.mac => {
                            let r = ui.add(egui::TextEdit::singleline(s).desired_width(140.0));
                            if r.lost_focus() {
                                let (m, s) = self.edition.take().unwrap();
                                if s.trim().is_empty() {
                                    self.connus.surnoms.remove(&m);
                                } else {
                                    self.connus.surnoms.insert(m, s.trim().to_string());
                                }
                                change = true;
                            } else {
                                r.request_focus();
                            }
                        }
                        _ => {
                            if ui.add_enabled(!a.mac.is_empty(), egui::Button::new(if surnom.is_empty() { "✏".to_string() } else { surnom.clone() }).small()).on_hover_text("Donner un surnom").clicked() {
                                self.edition = Some((a.mac.clone(), surnom));
                            }
                        }
                    }
                    ui.weak(if mac_aleatoire(&a.mac) { format!("{} (privée)", a.mac) } else { a.mac.clone() });
                    ui.weak(a.ms.map(|m| format!("{m} ms")).unwrap_or("—".into()));
                    ui.end_row();
                }
            });
        });
        ui.weak("📱 = adresse « privée » : souvent un téléphone ou une tablette récents.");
        if change {
            enregistrer("appareils_reseau", &self.connus);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 43 · Point d'accès Wi-Fi
// ---------------------------------------------------------------------------------------------------

mod partage {
    use windows::core::HSTRING;
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::*;

    fn e(x: windows::core::Error) -> String {
        x.message()
    }

    fn gestionnaire() -> Result<NetworkOperatorTetheringManager, String> {
        let p = NetworkInformation::GetInternetConnectionProfile().map_err(|_| "pas de connexion internet à partager".to_string())?;
        NetworkOperatorTetheringManager::CreateFromConnectionProfile(&p).map_err(e)
    }

    /// (actif, nom, mot de passe, appareils connectés, max)
    pub fn etat() -> Result<(bool, String, String, u32, u32), String> {
        let m = gestionnaire()?;
        let c = m.GetCurrentAccessPointConfiguration().map_err(e)?;
        Ok((m.TetheringOperationalState().map_err(e)? == TetheringOperationalState::On, c.Ssid().map_err(e)?.to_string(), c.Passphrase().map_err(e)?.to_string(), m.ClientCount().map_err(e)?, m.MaxClientCount().map_err(e)?))
    }

    pub fn configurer(nom: &str, mdp: &str) -> Result<(), String> {
        let m = gestionnaire()?;
        let c = m.GetCurrentAccessPointConfiguration().map_err(e)?;
        c.SetSsid(&HSTRING::from(nom)).map_err(e)?;
        c.SetPassphrase(&HSTRING::from(mdp)).map_err(e)?;
        m.ConfigureAccessPointAsync(&c).map_err(e)?.get().map_err(e)
    }

    pub fn basculer(oui: bool) -> Result<(), String> {
        let m = gestionnaire()?;
        let r = if oui { m.StartTetheringAsync() } else { m.StopTetheringAsync() }.map_err(e)?.get().map_err(e)?;
        let s = r.Status().map_err(e)?;
        if s == TetheringOperationStatus::Success {
            Ok(())
        } else {
            let msg = r.AdditionalErrorMessage().map(|x| x.to_string()).unwrap_or_default();
            Err(format!("refusé par Windows ({}){}", s.0, if msg.is_empty() { String::new() } else { format!(" : {msg}") }))
        }
    }
}

/// Texte du QR code de connexion Wi-Fi (format compris par les téléphones).
pub fn qr_wifi(ssid: &str, mdp: &str) -> String {
    let ech = |s: &str| s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace(':', "\\:").replace('"', "\\\"");
    format!("WIFI:T:WPA;S:{};P:{};;", ech(ssid), ech(mdp))
}

pub struct PointAcces {
    etat: Arc<Mutex<Option<Result<(bool, String, String, u32, u32), String>>>>,
    nom: String,
    mdp: String,
    voir: bool,
    e: Etat,
    qr: Option<(String, egui::TextureHandle)>,
}

impl PointAcces {
    pub fn new() -> Self {
        PointAcces { etat: Arc::default(), nom: String::new(), mdp: String::new(), voir: false, e: Arc::default(), qr: None }
    }

    fn rafraichir(&self) {
        let s = self.etat.clone();
        std::thread::spawn(move || *s.lock().unwrap() = Some(partage::etat()));
    }
}

impl Outil for PointAcces {
    fn nom(&self) -> &'static str {
        "📡 Point d'accès Wi-Fi"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Point d'accès Wi-Fi");
        ui.weak("Partage la connexion du PC en Wi-Fi (pour une console, une tablette, un téléphone sans forfait…).");
        if self.etat.lock().unwrap().is_none() {
            *self.etat.lock().unwrap() = Some(Err("lecture…".into()));
            self.rafraichir();
        }
        let occupe = etat_ui(ui, &self.e);
        let st = self.etat.lock().unwrap().clone();
        match st {
            Some(Ok((actif, ssid, mdp, n, max))) => {
                if self.nom.is_empty() {
                    (self.nom, self.mdp) = (ssid.clone(), mdp.clone());
                }
                ui.label(RichText::new(if actif { format!("🟢 Actif · {n}/{max} appareil(s) connecté(s)") } else { "⚪ Arrêté".into() }).size(18.0));
                ui.add_enabled_ui(!occupe, |ui| {
                    egui::Grid::new("ap").num_columns(2).show(ui, |ui| {
                        ui.label("Nom du réseau");
                        ui.add(egui::TextEdit::singleline(&mut self.nom).desired_width(220.0));
                        ui.end_row();
                        ui.label("Mot de passe");
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.mdp).password(!self.voir).desired_width(220.0));
                            ui.checkbox(&mut self.voir, "voir");
                        });
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        let change = self.nom != ssid || self.mdp != mdp;
                        if ui.button(if actif { "⏹ Arrêter" } else { "▶ Démarrer" }).clicked() {
                            let (nom, m, oui, s, e) = (self.nom.clone(), self.mdp.clone(), !actif, self.etat.clone(), self.e.clone());
                            *e.lock().unwrap() = (true, if oui { "Démarrage…".into() } else { "Arrêt…".into() });
                            std::thread::spawn(move || {
                                let r = (|| {
                                    if change && oui {
                                        if m.chars().count() < 8 {
                                            return Err("le mot de passe doit faire au moins 8 caractères".to_string());
                                        }
                                        partage::configurer(&nom, &m)?;
                                    }
                                    partage::basculer(oui)
                                })();
                                *e.lock().unwrap() = (false, match r {
                                    Ok(()) => if oui { "✔ Point d'accès démarré.".into() } else { "✔ Arrêté.".into() },
                                    Err(x) => format!("✖ {x}"),
                                });
                                *s.lock().unwrap() = Some(partage::etat());
                            });
                        }
                        if change && !actif {
                            ui.weak("(le nouveau nom / mot de passe sera appliqué au démarrage)");
                        }
                        if ui.small_button("⟳").clicked() {
                            self.rafraichir();
                        }
                    });
                });
                if actif {
                    let t = qr_wifi(&ssid, &mdp);
                    if self.qr.as_ref().is_none_or(|(x, _)| *x != t) {
                        if let Some(tex) = texture_qr(ui.ctx(), &t) {
                            self.qr = Some((t, tex));
                        }
                    }
                    if let Some((_, tex)) = &self.qr {
                        ui.label("Scanne pour te connecter :");
                        ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(200.0, 200.0)));
                    }
                }
                ui.ctx().request_repaint_after(Duration::from_secs(3));
            }
            Some(Err(e)) => {
                ui.weak(e);
                if ui.small_button("⟳").clicked() {
                    self.rafraichir();
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 44 · Ping des serveurs de jeux
// ---------------------------------------------------------------------------------------------------

/// (région, hôte) : points d'entrée des grands hébergeurs de jeux (la plupart des jeux en ligne y sont).
pub const REGIONS: [(&str, &str); 14] = [
    ("🇫🇷 Paris", "dynamodb.eu-west-3.amazonaws.com"),
    ("🇬🇧 Londres", "dynamodb.eu-west-2.amazonaws.com"),
    ("🇩🇪 Francfort", "dynamodb.eu-central-1.amazonaws.com"),
    ("🇮🇪 Irlande", "dynamodb.eu-west-1.amazonaws.com"),
    ("🇮🇹 Milan", "dynamodb.eu-south-1.amazonaws.com"),
    ("🇪🇸 Espagne", "dynamodb.eu-south-2.amazonaws.com"),
    ("🇸🇪 Stockholm", "dynamodb.eu-north-1.amazonaws.com"),
    ("🇺🇸 USA Est", "dynamodb.us-east-1.amazonaws.com"),
    ("🇺🇸 USA Ouest", "dynamodb.us-west-1.amazonaws.com"),
    ("🇧🇷 Brésil", "dynamodb.sa-east-1.amazonaws.com"),
    ("🇯🇵 Japon", "dynamodb.ap-northeast-1.amazonaws.com"),
    ("🇸🇬 Singapour", "dynamodb.ap-southeast-1.amazonaws.com"),
    ("🇦🇺 Australie", "dynamodb.ap-southeast-2.amazonaws.com"),
    ("🇿🇦 Afrique du Sud", "dynamodb.af-south-1.amazonaws.com"),
];

fn mesurer_tcp(hote: &str) -> Option<u32> {
    let a = (hote, 443).to_socket_addrs().ok()?.next()?;
    let t = Instant::now();
    std::net::TcpStream::connect_timeout(&a, Duration::from_secs(2)).ok()?;
    Some(t.elapsed().as_millis() as u32)
}

/// (médiane, gigue) de mesures.
pub fn stats(v: &[u32]) -> Option<(u32, u32)> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort();
    let med = s[s.len() / 2];
    let gigue = if v.len() > 1 { v.windows(2).map(|w| w[0].abs_diff(w[1])).sum::<u32>() / (v.len() as u32 - 1) } else { 0 };
    Some((med, gigue))
}

pub struct PingJeux {
    res: Arc<Mutex<Vec<Option<(u32, u32)>>>>,
    en_cours: Arc<AtomicBool>,
}

impl PingJeux {
    pub fn new() -> Self {
        PingJeux { res: Arc::new(Mutex::new(vec![None; REGIONS.len()])), en_cours: Arc::default() }
    }

    fn lancer(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        *self.res.lock().unwrap() = vec![None; REGIONS.len()];
        let (r, e) = (self.res.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            std::thread::scope(|s| {
                for (k, (_, h)) in REGIONS.iter().enumerate() {
                    let r = r.clone();
                    s.spawn(move || {
                        let _ = mesurer_tcp(h); // la première connexion inclut la recherche DNS
                        let v: Vec<u32> = (0..5).filter_map(|_| {
                            std::thread::sleep(Duration::from_millis(150));
                            mesurer_tcp(h)
                        }).collect();
                        r.lock().unwrap()[k] = Some(stats(&v).unwrap_or((u32::MAX, 0)));
                    });
                }
            });
            e.store(false, Relaxed);
        });
    }
}

impl Outil for PingJeux {
    fn nom(&self) -> &'static str {
        "🏓 Ping des serveurs de jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ping des serveurs de jeux par région");
        ui.weak("Fortnite, Valorant, Apex, Rocket League et beaucoup d'autres hébergent leurs serveurs dans ces centres de données : choisis la région la plus rapide dans les options du jeu.");
        let en_cours = self.en_cours.load(Relaxed);
        if ui.add_enabled(!en_cours, egui::Button::new(RichText::new("📡 Mesurer").size(16.0))).clicked() {
            self.lancer();
        }
        if en_cours {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        let r = self.res.lock().unwrap().clone();
        let max = r.iter().flatten().filter(|x| x.0 != u32::MAX).map(|x| x.0).max().unwrap_or(100).max(50);
        let meilleur = r.iter().flatten().filter(|x| x.0 != u32::MAX).map(|x| x.0).min();
        egui::Grid::new("ping_jeux").spacing([14.0, 6.0]).show(ui, |ui| {
            for (k, (nom, _)) in REGIONS.iter().enumerate() {
                ui.label(*nom);
                match r[k] {
                    None => {
                        if en_cours {
                            ui.spinner();
                        } else {
                            ui.label("—");
                        }
                        ui.label("");
                    }
                    Some((u32::MAX, _)) => {
                        ui.colored_label(Color32::GRAY, "injoignable");
                        ui.label("");
                    }
                    Some((ms, gigue)) => {
                        let c = match ms {
                            0..=40 => Color32::LIGHT_GREEN,
                            41..=80 => Color32::from_rgb(180, 230, 120),
                            81..=130 => Color32::from_rgb(250, 210, 90),
                            _ => Color32::LIGHT_RED,
                        };
                        let (rr, _) = ui.allocate_exact_size(egui::vec2(220.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(egui::Rect::from_min_size(rr.min, egui::vec2(rr.width() * ms as f32 / max as f32, 14.0)), 3.0, c);
                        ui.label(RichText::new(format!("{ms} ms{}{}", if gigue > 5 { format!(" (± {gigue})") } else { String::new() }, if Some(ms) == meilleur { "  ⭐" } else { "" })).color(c));
                    }
                }
                ui.end_row();
            }
        });
        ui.weak("Mesure du temps de connexion (proche du ping en jeu, parfois quelques ms de plus).");
    }
}

// ---------------------------------------------------------------------------------------------------
// 45 · Consommation internet par appli
// ---------------------------------------------------------------------------------------------------

/// Nom lisible d'une appli : chemin d'exe → nom du fichier ; nom de paquet → partie lisible.
pub fn nom_appli(n: &str) -> String {
    if n.contains('\\') {
        return n.rsplit('\\').next().unwrap_or(n).trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
    }
    if let Some(p) = n.split('_').next().filter(|_| n.contains('_')) {
        return p.rsplit('.').next().unwrap_or(p).to_string();
    }
    n.to_string()
}

fn consommation(jours: i64) -> Result<Vec<(String, u64, u64)>, String> {
    use windows::Foundation::DateTime;
    use windows::Networking::Connectivity::*;
    let e = |x: windows::core::Error| x.message();
    // DateTime : centaines de nanosecondes depuis 1601
    let maintenant = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|x| x.to_string())?.as_secs() as i64 + 11_644_473_600) * 10_000_000;
    let debut = DateTime { UniversalTime: maintenant - jours * 86_400 * 10_000_000 };
    let fin = DateTime { UniversalTime: maintenant };
    let etats = NetworkUsageStates { Roaming: TriStates::DoNotCare, Shared: TriStates::DoNotCare };
    let mut total: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for p in NetworkInformation::GetConnectionProfiles().map_err(e)? {
        let Ok(op) = p.GetAttributedNetworkUsageAsync(debut, fin, etats) else { continue };
        let Ok(l) = op.get() else { continue };
        for u in l {
            let n = nom_appli(&u.AttributionName().map(|x| x.to_string()).unwrap_or_default());
            let x = total.entry(if n.is_empty() { "Système Windows".into() } else { n }).or_default();
            x.0 += u.BytesReceived().unwrap_or(0);
            x.1 += u.BytesSent().unwrap_or(0);
        }
    }
    let mut v: Vec<(String, u64, u64)> = total.into_iter().map(|(n, (r, s))| (n, r, s)).collect();
    v.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)));
    Ok(v)
}

pub struct DataApplis {
    jours: i64,
    res: Arc<Mutex<Option<Result<Vec<(String, u64, u64)>, String>>>>,
}

impl DataApplis {
    pub fn new() -> Self {
        DataApplis { jours: 30, res: Arc::default() }
    }

    fn lancer(&self) {
        let (r, j) = (self.res.clone(), self.jours);
        *r.lock().unwrap() = Some(Err("calcul…".into()));
        std::thread::spawn(move || *r.lock().unwrap() = Some(consommation(j)));
    }
}

impl Outil for DataApplis {
    fn nom(&self) -> &'static str {
        "📊 Consommation internet par appli"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Consommation internet par appli");
        ui.weak("Ce que chaque programme a téléchargé et envoyé (compté par Windows). Pratique avec un forfait limité ou une connexion partagée.");
        ui.horizontal(|ui| {
            for (j, n) in [(1, "Aujourd'hui"), (7, "7 jours"), (30, "30 jours")] {
                if ui.selectable_value(&mut self.jours, j, n).clicked() {
                    self.lancer();
                }
            }
            if ui.small_button("⟳").clicked() {
                self.lancer();
            }
        });
        let r = self.res.lock().unwrap().clone();
        match r {
            None => self.lancer(),
            Some(Err(e)) => {
                ui.horizontal(|ui| {
                    if e == "calcul…" {
                        ui.spinner();
                    }
                    ui.weak(e);
                });
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            Some(Ok(v)) => {
                let total: u64 = v.iter().map(|x| x.1 + x.2).sum();
                ui.label(RichText::new(format!("Total : {}", crate::disque::taille_lisible(total))).size(18.0));
                let max = v.first().map(|x| x.1 + x.2).unwrap_or(1).max(1);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    egui::Grid::new("data_applis").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                        ui.strong("Appli");
                        ui.strong("");
                        ui.strong("⬇ Reçu");
                        ui.strong("⬆ Envoyé");
                        ui.end_row();
                        for (n, rec, env) in v.iter().take(60) {
                            ui.label(n);
                            ui.add(egui::ProgressBar::new((rec + env) as f32 / max as f32).desired_width(150.0));
                            ui.label(crate::disque::taille_lisible(*rec));
                            ui.weak(crate::disque::taille_lisible(*env));
                            ui.end_row();
                        }
                    });
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 46 · Mot de passe fuité ?
// ---------------------------------------------------------------------------------------------------

/// (préfixe envoyé, suffixe gardé) du SHA-1 en majuscules.
pub fn decouper_sha1(mdp: &str) -> (String, String) {
    use sha1::Digest;
    let h: String = sha1::Sha1::digest(mdp.as_bytes()).iter().map(|b| format!("{b:02X}")).collect();
    (h[..5].to_string(), h[5..].to_string())
}

/// Nombre de fuites dans la réponse de l'API (lignes « SUFFIXE:N »).
pub fn compter_fuites(reponse: &str, suffixe: &str) -> u64 {
    reponse.lines().find_map(|l| {
        let (s, n) = l.trim().split_once(':')?;
        s.eq_ignore_ascii_case(suffixe).then(|| n.trim().parse().unwrap_or(0))
    }).unwrap_or(0)
}

/// Estimation grossière : (bits d'entropie, appréciation).
pub fn solidite(m: &str) -> (u32, &'static str) {
    let mut taille_alphabet = 0;
    if m.chars().any(|c| c.is_ascii_lowercase()) {
        taille_alphabet += 26;
    }
    if m.chars().any(|c| c.is_ascii_uppercase()) {
        taille_alphabet += 26;
    }
    if m.chars().any(|c| c.is_ascii_digit()) {
        taille_alphabet += 10;
    }
    if m.chars().any(|c| !c.is_ascii_alphanumeric()) {
        taille_alphabet += 33;
    }
    let bits = (m.chars().count() as f64 * (taille_alphabet.max(1) as f64).log2()) as u32;
    (bits, match bits {
        0..=35 => "très faible",
        36..=59 => "faible",
        60..=79 => "correct",
        80..=99 => "solide",
        _ => "très solide",
    })
}

pub struct Fuite {
    mdp: String,
    res: Arc<Mutex<Option<Result<u64, String>>>>,
}

impl Fuite {
    pub fn new() -> Self {
        Fuite { mdp: String::new(), res: Arc::default() }
    }
}

impl Outil for Fuite {
    fn nom(&self) -> &'static str {
        "🕵 Mot de passe fuité ?"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mon mot de passe a-t-il fuité ?");
        ui.weak("Vérifie s'il apparaît dans les fuites de données connues (base Have I Been Pwned). Le mot de passe ne quitte jamais le PC : seuls les 5 premiers caractères de son empreinte sont envoyés.");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.mdp).password(true).hint_text("mot de passe à tester").desired_width(260.0));
            if r.changed() {
                *self.res.lock().unwrap() = None;
            }
            if (ui.button("🔎 Vérifier").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.mdp.is_empty() {
                let (pre, suf) = decouper_sha1(&self.mdp);
                let res = self.res.clone();
                *res.lock().unwrap() = Some(Err("vérification…".into()));
                std::thread::spawn(move || {
                    let r = ureq::get(&format!("https://api.pwnedpasswords.com/range/{pre}")).set("Add-Padding", "true").timeout(Duration::from_secs(15)).call()
                        .map_err(|e| e.to_string()).and_then(|r| r.into_string().map_err(|e| e.to_string())).map(|t| compter_fuites(&t, &suf));
                    *res.lock().unwrap() = Some(r);
                });
            }
        });
        if !self.mdp.is_empty() {
            let (b, a) = solidite(&self.mdp);
            ui.label(format!("Solidité estimée : {a} ({b} bits)"));
        }
        match self.res.lock().unwrap().clone() {
            Some(Ok(0)) => {
                ui.colored_label(Color32::LIGHT_GREEN, RichText::new("✔ Jamais vu dans une fuite connue.").size(20.0));
            }
            Some(Ok(n)) => {
                ui.colored_label(Color32::LIGHT_RED, RichText::new(format!("⚠ Vu {n} fois dans des fuites : change-le partout où tu l'utilises !")).size(20.0));
                ui.label("Astuce : le générateur de l'onglet « Mots de passe » en crée de solides, et le Coffre les retient pour toi.");
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        let v = adresses_du_reseau(Ipv4Addr::new(192, 168, 1, 37));
        assert_eq!(v.len(), 254);
        assert_eq!(v[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(mac_texte(&[0, 0x1a, 0xff]), "00:1A:FF");
        assert!(mac_aleatoire("DA:A1:19:00:00:01"));
        assert!(!mac_aleatoire("00:1A:2B:00:00:01"));
        assert_eq!(qr_wifi("Mon;Wifi", "a:b"), "WIFI:T:WPA;S:Mon\\;Wifi;P:a\\:b;;");
        assert_eq!(stats(&[30, 10, 20]), Some((20, 15)));
        assert_eq!(stats(&[]), None);
        assert_eq!(nom_appli(r"\device\harddiskvolume3\program files\discord\discord.exe"), "discord");
        assert_eq!(nom_appli("Microsoft.WindowsStore_8wekyb3d8bbwe"), "WindowsStore");
        assert_eq!(nom_appli("svchost"), "svchost");
        // « password » : préfixe connu 5BAA6
        let (p, s) = decouper_sha1("password");
        assert_eq!(p, "5BAA6");
        assert_eq!(compter_fuites("0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n1E4C9B93F3F0682250B6CF8331B7EE68FD8:9545824\r\n", &s), 9_545_824);
        assert_eq!(compter_fuites("AAA:3", &s), 0);
        assert_eq!(solidite("abc").1, "très faible");
        assert!(solidite("Tr0ub4dour&3-Cheval-Agrafe!").0 >= 100);
    }
}
