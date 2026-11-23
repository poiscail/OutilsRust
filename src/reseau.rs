//! Réseau : ping en continu (graphe, pertes, gigue), débit de la carte réseau, test de vitesse,
//! et connexions ouvertes par application. Hôtes dans %APPDATA%\Outils\reseau.json.
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Read;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::NetworkManagement::IpHelper::*;
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

const HISTO: usize = 120;

fn fichier() -> PathBuf {
    crate::outil::base().join("reseau.json")
}

pub fn resoudre(hote: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = hote.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    (hote, 0).to_socket_addrs().ok()?.find_map(|a| match a {
        std::net::SocketAddr::V4(v) => Some(*v.ip()),
        _ => None,
    })
}

/// Un ping ICMP ; Some(ms) ou None (perdu).
fn ping(h: HANDLE, ip: Ipv4Addr) -> Option<u32> {
    let donnees = *b"OutilsPing012345678901234567890!";
    let mut rep = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + donnees.len() + 16];
    let n = unsafe {
        IcmpSendEcho(h, u32::from_ne_bytes(ip.octets()), donnees.as_ptr().cast(), donnees.len() as u16, std::ptr::null(), rep.as_mut_ptr().cast(), rep.len() as u32, 1000)
    };
    if n == 0 {
        return None;
    }
    let r = unsafe { &*(rep.as_ptr() as *const ICMP_ECHO_REPLY) };
    (r.Status == 0).then_some(r.RoundTripTime)
}

/// Statistiques d'une série de pings : (moyenne, min, max, gigue, % de pertes).
pub fn stats(v: &VecDeque<Option<u32>>) -> Option<(f32, u32, u32, f32, f32)> {
    if v.is_empty() {
        return None;
    }
    let ok: Vec<u32> = v.iter().flatten().copied().collect();
    let pertes = 100.0 * (v.len() - ok.len()) as f32 / v.len() as f32;
    if ok.is_empty() {
        return Some((0.0, 0, 0, 0.0, pertes));
    }
    let moy = ok.iter().sum::<u32>() as f32 / ok.len() as f32;
    let gigue = if ok.len() > 1 { ok.windows(2).map(|w| (w[1] as f32 - w[0] as f32).abs()).sum::<f32>() / (ok.len() - 1) as f32 } else { 0.0 };
    Some((moy, *ok.iter().min().unwrap(), *ok.iter().max().unwrap(), gigue, pertes))
}

/// Octets reçus / envoyés sur les cartes physiques actives.
fn compteurs() -> (u64, u64) {
    let mut t: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if unsafe { GetIfTable2(&mut t) } != 0 || t.is_null() {
        return (0, 0);
    }
    let (mut rx, mut tx) = (0, 0);
    unsafe {
        let n = (*t).NumEntries as usize;
        let lignes = std::slice::from_raw_parts((*t).Table.as_ptr(), n);
        for l in lignes {
            let materiel = l.InterfaceAndOperStatusFlags._bitfield & 1 != 0;
            let filtre = l.InterfaceAndOperStatusFlags._bitfield & 2 != 0;
            if materiel && !filtre && l.OperStatus == 1 && l.Type != 24 {
                rx += l.InOctets;
                tx += l.OutOctets;
            }
        }
        FreeMibTable(t.cast());
    }
    (rx, tx)
}

pub(crate) fn nom_processus(pid: u32) -> String {
    if pid == 0 {
        return "Système (inactif)".into();
    }
    if pid == 4 {
        return "Système".into();
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return format!("PID {pid}");
        }
        let mut buf = [0u16; 512];
        let mut n = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut n);
        CloseHandle(h);
        if ok == 0 {
            return format!("PID {pid}");
        }
        let chemin = String::from_utf16_lossy(&buf[..n as usize]);
        chemin.rsplit('\\').next().unwrap_or(&chemin).to_string()
    }
}

/// Connexions TCP établies, regroupées par application : nom -> liste "ip:port".
pub fn connexions() -> BTreeMap<String, Vec<String>> {
    let mut taille = 0u32;
    unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut taille, 0, 2, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) };
    let mut buf = vec![0u8; taille as usize + 4096];
    let mut taille = buf.len() as u32;
    let mut par: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if unsafe { GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut taille, 1, 2, TCP_TABLE_OWNER_PID_CONNECTIONS, 0) } != 0 {
        return par;
    }
    let t = buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
    let lignes = unsafe { std::slice::from_raw_parts((*t).table.as_ptr(), (*t).dwNumEntries as usize) };
    let mut noms: HashMap<u32, String> = HashMap::new();
    for l in lignes {
        if l.dwState != MIB_TCP_STATE_ESTAB as u32 {
            continue;
        }
        let ip = Ipv4Addr::from(u32::from_be(l.dwRemoteAddr));
        if ip.is_loopback() {
            continue;
        }
        let port = u16::from_be(l.dwRemotePort as u16);
        let nom = noms.entry(l.dwOwningPid).or_insert_with(|| nom_processus(l.dwOwningPid)).clone();
        par.entry(nom).or_default().push(format!("{ip}:{port}"));
    }
    par
}

pub fn debit_lisible(octets_s: f64) -> String {
    let bits = octets_s * 8.0;
    if bits >= 1e9 {
        format!("{:.2} Gbit/s", bits / 1e9)
    } else if bits >= 1e6 {
        format!("{:.1} Mbit/s", bits / 1e6)
    } else {
        format!("{:.0} kbit/s", bits / 1e3)
    }
}

struct Sonde {
    hote: String,
    pings: VecDeque<Option<u32>>,
    ip: Option<Ipv4Addr>,
}

#[derive(Default)]
struct Partage {
    sondes: Vec<Sonde>,
    debit: VecDeque<(f64, f64)>, // octets/s reçus, envoyés
    test: Option<String>,        // résultat ou état du test de vitesse
    test_en_cours: bool,
}

pub struct Reseau {
    p: Arc<Mutex<Partage>>,
    actif: Arc<AtomicBool>, // les fils ne mesurent que si l'onglet a été ouvert récemment
    vu: Instant,
    nouvel_hote: String,
    conn: (Option<Instant>, BTreeMap<String, Vec<String>>),
}

impl Reseau {
    pub fn new() -> Self {
        let hotes: Vec<String> = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| vec!["1.1.1.1".into(), "8.8.8.8".into(), "google.com".into()]);
        let p = Arc::new(Mutex::new(Partage { sondes: hotes.into_iter().map(|h| Sonde { hote: h, pings: VecDeque::new(), ip: None }).collect(), ..Default::default() }));
        let actif = Arc::new(AtomicBool::new(false));
        let (p2, a2) = (p.clone(), actif.clone());
        std::thread::spawn(move || {
            let icmp = unsafe { IcmpCreateFile() };
            let mut avant = (compteurs(), Instant::now());
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if !a2.load(Relaxed) {
                    avant = (compteurs(), Instant::now());
                    continue;
                }
                // débit
                let c = compteurs();
                let dt = avant.1.elapsed().as_secs_f64().max(0.001);
                let mesure = (c.0.saturating_sub(avant.0 .0) as f64 / dt, c.1.saturating_sub(avant.0 .1) as f64 / dt);
                avant = (c, Instant::now());
                // pings : on travaille sur une copie de la liste pour ne pas bloquer l'interface
                let cibles: Vec<(String, Option<Ipv4Addr>)> = p2.lock().unwrap().sondes.iter().map(|s| (s.hote.clone(), s.ip)).collect();
                let resultats: Vec<(String, Option<Ipv4Addr>, Option<u32>)> = cibles.into_iter().map(|(h, ip)| {
                    let ip = ip.or_else(|| resoudre(&h));
                    let r = ip.and_then(|ip| ping(icmp, ip));
                    (h, ip, r)
                }).collect();
                let mut p = p2.lock().unwrap();
                p.debit.push_back(mesure);
                while p.debit.len() > HISTO {
                    p.debit.pop_front();
                }
                for (h, ip, r) in resultats {
                    if let Some(s) = p.sondes.iter_mut().find(|s| s.hote == h) {
                        s.ip = ip;
                        s.pings.push_back(r);
                        while s.pings.len() > HISTO {
                            s.pings.pop_front();
                        }
                    }
                }
            }
        });
        Reseau { p, actif, vu: Instant::now() - Duration::from_secs(3600), nouvel_hote: String::new(), conn: (None, BTreeMap::new()) }
    }

    /// Coupe les mesures quand l'onglet n'a pas été regardé depuis une minute.
    pub fn tic(&mut self) {
        self.actif.store(self.vu.elapsed() < Duration::from_secs(60), Relaxed);
    }

    fn enregistrer(&self) {
        let hotes: Vec<String> = self.p.lock().unwrap().sondes.iter().map(|s| s.hote.clone()).collect();
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&hotes).unwrap_or_default());
    }

    fn test_vitesse(&self) {
        let p = self.p.clone();
        {
            let mut x = p.lock().unwrap();
            if x.test_en_cours {
                return;
            }
            x.test_en_cours = true;
            x.test = Some("Téléchargement…".into());
        }
        std::thread::spawn(move || {
            let res = (|| -> Result<String, String> {
                let t = Instant::now();
                let mut lu = 0u64;
                let mut r = ureq::get("https://speed.cloudflare.com/__down?bytes=50000000").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?.into_reader();
                let mut buf = vec![0u8; 1 << 16];
                while t.elapsed() < Duration::from_secs(12) {
                    let n = r.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    lu += n as u64;
                }
                let des = lu as f64 / t.elapsed().as_secs_f64();
                p.lock().unwrap().test = Some(format!("↓ {} · envoi…", debit_lisible(des)));
                let donnees = vec![0u8; 10_000_000];
                let t = Instant::now();
                ureq::post("https://speed.cloudflare.com/__up").set("User-Agent", "Outils").send_bytes(&donnees).map_err(|e| e.to_string())?;
                let mon = donnees.len() as f64 / t.elapsed().as_secs_f64();
                Ok(format!("↓ {}   ↑ {}", debit_lisible(des), debit_lisible(mon)))
            })();
            let mut x = p.lock().unwrap();
            x.test_en_cours = false;
            x.test = Some(res.unwrap_or_else(|e| format!("Échec : {e}")));
        });
    }

    fn graphe(ui: &mut egui::Ui, series: &[(Vec<Option<f32>>, Color32)], hauteur: f32, unite: &dyn Fn(f32) -> String) {
        let (zone, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), hauteur), Sense::hover());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 4.0, Color32::from_black_alpha(60));
        let max = series.iter().flat_map(|(v, _)| v.iter().flatten()).fold(1f32, |a, &b| a.max(b)) * 1.15;
        for k in 1..4 {
            let y = zone.bottom() - zone.height() * k as f32 / 4.0;
            p.line_segment([Pos2::new(zone.left(), y), Pos2::new(zone.right(), y)], Stroke::new(1.0f32, Color32::from_white_alpha(18)));
            p.text(Pos2::new(zone.left() + 4.0, y), egui::Align2::LEFT_BOTTOM, unite(max * k as f32 / 4.0), egui::FontId::proportional(10.0), Color32::from_white_alpha(120));
        }
        for (v, c) in series {
            let dx = zone.width() / (HISTO - 1) as f32;
            let depart = HISTO - v.len();
            let mut trait_ = vec![];
            for (i, x) in v.iter().enumerate() {
                let px = zone.left() + (depart + i) as f32 * dx;
                match x {
                    Some(y) => trait_.push(Pos2::new(px, zone.bottom() - zone.height() * y / max)),
                    None => {
                        p.line_segment([Pos2::new(px, zone.top()), Pos2::new(px, zone.bottom())], Stroke::new(1.0f32, Color32::from_rgba_unmultiplied(255, 60, 60, 90)));
                        if trait_.len() > 1 {
                            p.add(egui::Shape::line(std::mem::take(&mut trait_), Stroke::new(1.5f32, *c)));
                        }
                        trait_.clear();
                    }
                }
            }
            if trait_.len() > 1 {
                p.add(egui::Shape::line(trait_, Stroke::new(1.5f32, *c)));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.vu = Instant::now();
        self.actif.store(true, Relaxed);
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.heading("Réseau");
        egui::ScrollArea::vertical().id_salt("reseau").show(ui, |ui| {
            // ---- débit ----
            let (debit, test, test_en_cours) = {
                let p = self.p.lock().unwrap();
                (p.debit.clone(), p.test.clone(), p.test_en_cours)
            };
            ui.horizontal(|ui| {
                ui.strong("Débit de la carte réseau");
                if let Some((r, t)) = debit.back() {
                    ui.colored_label(Color32::from_rgb(110, 200, 255), format!("↓ {}", debit_lisible(*r)));
                    ui.colored_label(Color32::from_rgb(255, 140, 200), format!("↑ {}", debit_lisible(*t)));
                }
            });
            let rx: Vec<Option<f32>> = debit.iter().map(|d| Some(d.0 as f32)).collect();
            let tx: Vec<Option<f32>> = debit.iter().map(|d| Some(d.1 as f32)).collect();
            Self::graphe(ui, &[(rx, Color32::from_rgb(110, 200, 255)), (tx, Color32::from_rgb(255, 140, 200))], 110.0, &|v| debit_lisible(v as f64));
            ui.horizontal(|ui| {
                if ui.add_enabled(!test_en_cours, egui::Button::new("⚡ Test de vitesse")).clicked() {
                    self.test_vitesse();
                }
                if test_en_cours {
                    ui.spinner();
                }
                if let Some(t) = test {
                    ui.label(t);
                }
                ui.weak("(serveurs Cloudflare)");
            });
            ui.separator();

            // ---- pings ----
            ui.strong("Ping");
            let mut retirer = None;
            let couleurs = [Color32::from_rgb(120, 220, 140), Color32::from_rgb(255, 200, 90), Color32::from_rgb(150, 170, 255), Color32::from_rgb(255, 120, 170), Color32::from_rgb(90, 220, 220)];
            let mut series = vec![];
            {
                let p = self.p.lock().unwrap();
                for (i, s) in p.sondes.iter().enumerate() {
                    let c = couleurs[i % couleurs.len()];
                    ui.horizontal(|ui| {
                        ui.colored_label(c, "⏺");
                        ui.label(&s.hote);
                        if let Some(ip) = s.ip {
                            if ip.to_string() != s.hote {
                                ui.weak(ip.to_string());
                            }
                        }
                        match stats(&s.pings) {
                            Some((moy, mn, mx, gigue, pertes)) => {
                                let etat = match s.pings.back() {
                                    Some(Some(ms)) => format!("{ms} ms"),
                                    Some(None) => "perdu".into(),
                                    None => "…".into(),
                                };
                                ui.strong(etat);
                                ui.weak(format!("moy {moy:.0} · min {mn} · max {mx} · gigue {gigue:.1} ms · pertes {pertes:.0} %"));
                            }
                            None => {
                                ui.weak("en attente…");
                            }
                        }
                        if ui.small_button("✖").clicked() {
                            retirer = Some(i);
                        }
                    });
                    series.push((s.pings.iter().map(|x| x.map(|v| v as f32)).collect::<Vec<_>>(), c));
                }
            }
            Self::graphe(ui, &series, 130.0, &|v| format!("{v:.0} ms"));
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.nouvel_hote).hint_text("adresse ou IP (serveur de jeu…)").desired_width(220.0));
                let entree = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("➕ Ajouter").clicked() || entree) && !self.nouvel_hote.trim().is_empty() {
                    let h = self.nouvel_hote.trim().to_string();
                    self.p.lock().unwrap().sondes.push(Sonde { hote: h, pings: VecDeque::new(), ip: None });
                    self.nouvel_hote.clear();
                    self.enregistrer();
                }
            });
            if let Some(i) = retirer {
                self.p.lock().unwrap().sondes.remove(i);
                self.enregistrer();
            }
            ui.separator();

            // ---- connexions ----
            if self.conn.0.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
                self.conn = (Some(Instant::now()), connexions());
            }
            ui.strong(format!("Connexions ouvertes ({} applis)", self.conn.1.len()));
            ui.weak("Débit par appli impossible sans droits admin : on montre qui est connecté et à quoi.");
            let mut tri: Vec<(&String, &Vec<String>)> = self.conn.1.iter().collect();
            tri.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (nom, liste) in tri {
                egui::CollapsingHeader::new(format!("{nom} · {} connexion(s)", liste.len())).id_salt(("conn", nom)).show(ui, |ui| {
                    for a in liste.iter().take(40) {
                        ui.monospace(a);
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statistiques() {
        let v: VecDeque<Option<u32>> = [Some(10), Some(20), None, Some(30)].into_iter().collect();
        let (moy, mn, mx, gigue, pertes) = stats(&v).unwrap();
        assert_eq!((moy, mn, mx, gigue, pertes), (20.0, 10, 30, 10.0, 25.0));
        assert!(stats(&VecDeque::new()).is_none());
    }

    #[test]
    fn debits() {
        assert_eq!(debit_lisible(125_000.0), "1.0 Mbit/s");
        assert_eq!(debit_lisible(1000.0), "8 kbit/s");
        assert_eq!(debit_lisible(250_000_000.0), "2.00 Gbit/s");
    }

    #[test]
    fn ip_directe() {
        assert_eq!(resoudre("192.168.1.10"), Some(Ipv4Addr::new(192, 168, 1, 10)));
    }
}
