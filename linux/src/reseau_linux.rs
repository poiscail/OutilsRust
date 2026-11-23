//! Sécurité du réseau sous Linux (à n'utiliser que sur TON réseau) : appareils connectés et leurs ports
//! à risque, audit de la box et du Wi-Fi, ports ouverts sur ce PC. Même analyse que sous Windows.
use crate::outil::{Cat, Outil};
use crate::ports_risques::{dns_connu, genre, infos_port, liste_ports, note_wifi, ports_ouverts, usurpation_arp};
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn lire(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// Adresse de la box d'après /proc/net/route (passerelle de la route par défaut).
pub fn passerelle_de(route: &str) -> Option<Ipv4Addr> {
    route.lines().skip(1).find_map(|l| {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() > 2 && c[1] == "00000000" {
            let g = u32::from_str_radix(c[2], 16).ok()?;
            Some(Ipv4Addr::from(g.swap_bytes()))
        } else {
            None
        }
    })
}

/// Table ARP (/proc/net/arp) : ip → mac.
pub fn arp_de(t: &str) -> BTreeMap<Ipv4Addr, String> {
    t.lines()
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            let ip: Ipv4Addr = c.first()?.parse().ok()?;
            let mac = c.get(3)?.to_uppercase();
            (mac != "00:00:00:00:00:00").then_some((ip, mac))
        })
        .collect()
}

/// Serveurs DNS (resolv.conf ; si c'est systemd-resolved, on demande les vrais à resolvectl).
pub fn dns() -> Vec<String> {
    let mut v: Vec<String> = lire("/etc/resolv.conf").lines().filter_map(|l| l.trim().strip_prefix("nameserver").map(|x| x.trim().to_string())).collect();
    if v.iter().any(|d| d.starts_with("127.0.0.53")) {
        let o = std::process::Command::new("resolvectl").arg("dns").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let vrais: Vec<String> = o.split_whitespace().filter(|x| x.parse::<std::net::IpAddr>().is_ok()).map(String::from).collect();
        if !vrais.is_empty() {
            v = vrais;
        }
    }
    v.dedup();
    v
}

/// Sécurité du Wi-Fi actif d'après NetworkManager (« nmcli -t -f ACTIVE,SSID,SECURITY dev wifi »).
pub fn wifi_actif(sortie: &str) -> Option<(String, String)> {
    sortie.lines().find_map(|l| {
        let mut c = l.splitn(3, ':');
        let (a, s, sec) = (c.next()?, c.next()?, c.next().unwrap_or(""));
        (a == "yes" || a == "oui").then(|| (s.to_string(), sec.to_string()))
    })
}

fn ip_locale() -> Option<Ipv4Addr> {
    crate::telephone::ip_locale()?.parse().ok()
}

/// Appareil présent ? (une connexion acceptée ou refusée prouve qu'il existe)
fn present(ip: Ipv4Addr) -> bool {
    for p in [80, 443, 22, 445, 62078, 8008, 554, 9100, 7000, 5353] {
        match TcpStream::connect_timeout(&SocketAddr::from((ip, p)), Duration::from_millis(200)) {
            Ok(_) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => return true,
            Err(_) => {}
        }
    }
    false
}

#[derive(Clone)]
struct Appareil {
    ip: Ipv4Addr,
    mac: String,
    ports: Vec<u16>,
    genre: &'static str,
}

fn scanner(prog: &Mutex<usize>) -> Vec<Appareil> {
    let Some(moi) = ip_locale() else { return vec![] };
    let o = moi.octets();
    let file = Arc::new(Mutex::new((1..255u8).map(|d| Ipv4Addr::new(o[0], o[1], o[2], d)).collect::<Vec<_>>()));
    let vus = Arc::new(Mutex::new(vec![]));
    std::thread::scope(|s| {
        for _ in 0..48 {
            let (f, v) = (file.clone(), vus.clone());
            s.spawn(move || loop {
                let Some(ip) = f.lock().unwrap().pop() else { break };
                *prog.lock().unwrap() += 1;
                if ip != moi && present(ip) {
                    v.lock().unwrap().push(ip);
                }
            });
        }
    });
    let arp = arp_de(&lire("/proc/net/arp"));
    let mut ips = vus.lock().unwrap().clone();
    for ip in arp.keys() {
        if ip.octets()[..3] == o[..3] && !ips.contains(ip) {
            ips.push(*ip);
        }
    }
    let box_ip = passerelle_de(&lire("/proc/net/route"));
    let ports = liste_ports();
    let mut v: Vec<Appareil> = ips
        .into_iter()
        .map(|ip| {
            let p = ports_ouverts(ip, &ports, Duration::from_millis(300));
            let mac = arp.get(&ip).cloned().unwrap_or_default();
            let g = if Some(ip) == box_ip { "📡 box / routeur" } else { genre("", "", &p) };
            Appareil { ip, mac, ports: p, genre: g }
        })
        .collect();
    v.sort_by_key(|a| u32::from(a.ip));
    v
}

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

fn audit_box() -> (u32, Vec<(bool, String)>) {
    let mut v = vec![];
    let mut note = 0;
    let nm = std::process::Command::new("nmcli").args(["-t", "-f", "ACTIVE,SSID,SECURITY", "dev", "wifi"]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    match wifi_actif(&nm) {
        Some((ssid, sec)) => {
            let (n, t) = note_wifi(&sec, if sec.contains("WPA") { "CCMP" } else { "" });
            note += n;
            v.push((n >= 24, format!("« {ssid} » : {t}")));
        }
        None => {
            note += 20;
            v.push((true, "Pas de Wi-Fi actif vu par NetworkManager (câble, ou nmcli absent).".into()));
        }
    }
    let Some(b) = passerelle_de(&lire("/proc/net/route")) else {
        v.push((false, "Box introuvable (pas de connexion ?)".into()));
        return (note, v);
    };
    let ouverts = ports_ouverts(b, &[21, 22, 23, 80, 443, 445, 7547, 8080, 8443, 5900], Duration::from_millis(600));
    let mut pn = 25u32;
    for p in &ouverts {
        let (n, r, t) = infos_port(*p);
        if r >= 2 {
            pn = pn.saturating_sub(if r == 3 { 15 } else { 7 });
            v.push((false, format!("Port {p} ({n}) ouvert sur la box : {t}.")));
        }
    }
    if ouverts.contains(&80) && !ouverts.contains(&443) {
        pn = pn.saturating_sub(5);
        v.push((false, "Page d'administration de la box non chiffrée (HTTP) : change son mot de passe d'usine.".into()));
    }
    if pn == 25 {
        v.push((true, "Aucun service dangereux ouvert sur la box.".into()));
    }
    note += pn;
    if upnp_actif(b) {
        note += 5;
        v.push((false, "UPnP actif : les applis peuvent ouvrir des ports vers internet toutes seules.".into()));
    } else {
        note += 15;
        v.push((true, "UPnP ne répond pas.".into()));
    }
    let d = dns();
    let bizarres: Vec<&String> = d.iter().filter(|x| !dns_connu(x, &b.to_string())).collect();
    if bizarres.is_empty() {
        note += 20;
        v.push((true, format!("DNS utilisés : {} (normaux).", d.join(", "))));
    } else {
        v.push((false, format!("DNS inhabituels : {}. Si tu ne les as pas choisis, ta navigation est peut-être détournée.", bizarres.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
    }
    if let Some(autres) = usurpation_arp(&arp_de(&lire("/proc/net/arp")), b) {
        v.push((false, format!("{} utilise(nt) la même adresse MAC que la box : quelqu'un intercepte peut-être le trafic.", autres.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(", "))));
    } else {
        v.push((true, "Personne ne se fait passer pour la box.".into()));
    }
    note += 10;
    (note.min(100), v)
}

/// Ports qui écoutent sur ce PC (/proc/net/tcp et tcp6, état 0A) : (port, ouvert au réseau ?).
pub fn ports_ecoute(tcp: &str) -> Vec<(u16, bool)> {
    let mut v = vec![];
    for l in tcp.lines().skip(1) {
        let c: Vec<&str> = l.split_whitespace().collect();
        if c.len() < 4 || c[3] != "0A" {
            continue;
        }
        let Some((adr, port)) = c[1].split_once(':') else { continue };
        let Ok(p) = u16::from_str_radix(port, 16) else { continue };
        let local = adr == "0100007F" || adr == "00000000000000000000000001000000";
        if !v.iter().any(|x: &(u16, bool)| x.0 == p) {
            v.push((p, !local));
        }
    }
    v.sort();
    v
}

pub struct SecuReseau {
    onglet: usize,
    appareils: Arc<Mutex<Option<Vec<Appareil>>>>,
    prog: Arc<Mutex<usize>>,
    en_cours: Arc<AtomicBool>,
    audit: Arc<Mutex<Option<(u32, Vec<(bool, String)>)>>>,
}

impl SecuReseau {
    pub fn new() -> Self {
        SecuReseau { onglet: 0, appareils: Arc::default(), prog: Arc::default(), en_cours: Arc::default(), audit: Arc::default() }
    }
}

fn couleur(r: u8) -> Color32 {
    [Color32::LIGHT_GREEN, Color32::from_rgb(200, 200, 120), Color32::from_rgb(255, 170, 80), Color32::LIGHT_RED][r.min(3) as usize]
}

impl Outil for SecuReseau {
    fn nom(&self) -> &'static str {
        "🛡 Sécurité du réseau"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sécurité du réseau");
        ui.weak("Appareils de la maison et leurs portes ouvertes, note de la box et du Wi-Fi, services ouverts sur ce PC. À n'utiliser que chez toi : scanner le réseau des autres est interdit.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.onglet, 0, "📶 Appareils");
            ui.selectable_value(&mut self.onglet, 1, "🛡 Box et Wi-Fi");
            ui.selectable_value(&mut self.onglet, 2, "💻 Ce PC");
        });
        ui.separator();
        let en_cours = self.en_cours.load(Relaxed);
        match self.onglet {
            0 => {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!en_cours, egui::Button::new("🔎 Analyser le réseau")).clicked() {
                        self.en_cours.store(true, Relaxed);
                        *self.prog.lock().unwrap() = 0;
                        let (a, p, e) = (self.appareils.clone(), self.prog.clone(), self.en_cours.clone());
                        std::thread::spawn(move || {
                            *a.lock().unwrap() = Some(scanner(&p));
                            e.store(false, Relaxed);
                        });
                    }
                    if en_cours {
                        ui.add(egui::ProgressBar::new(*self.prog.lock().unwrap() as f32 / 254.0).desired_width(200.0));
                        ui.ctx().request_repaint_after(Duration::from_millis(300));
                    }
                });
                let Some(v) = self.appareils.lock().unwrap().clone() else { return };
                ui.label(format!("{} appareil(s)", v.len()));
                for a in &v {
                    let r = a.ports.iter().map(|p| infos_port(*p).1).max().unwrap_or(0);
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(a.genre);
                            ui.strong(a.ip.to_string());
                            ui.weak(&a.mac);
                            ui.colored_label(couleur(r), ["sûr", "à surveiller", "risqué", "dangereux"][r as usize]);
                        });
                        for p in &a.ports {
                            let (n, rr, t) = infos_port(*p);
                            ui.colored_label(couleur(rr), format!("  {p} · {n} — {t}"));
                        }
                    });
                }
            }
            1 => {
                if ui.add_enabled(!en_cours, egui::Button::new("🔎 Lancer l'audit")).clicked() {
                    self.en_cours.store(true, Relaxed);
                    let (a, e) = (self.audit.clone(), self.en_cours.clone());
                    std::thread::spawn(move || {
                        *a.lock().unwrap() = Some(audit_box());
                        e.store(false, Relaxed);
                    });
                }
                if en_cours {
                    ui.spinner();
                    ui.ctx().request_repaint_after(Duration::from_millis(300));
                }
                if let Some((n, l)) = self.audit.lock().unwrap().clone() {
                    ui.label(RichText::new(format!("{n} / 100")).size(28.0).strong().color(if n >= 80 { Color32::LIGHT_GREEN } else if n >= 55 { Color32::from_rgb(240, 200, 70) } else { Color32::LIGHT_RED }));
                    for (ok, t) in l {
                        ui.colored_label(if ok { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 170, 80) }, format!("{} {t}", if ok { "✔" } else { "⚠" }));
                    }
                    ui.weak("À vérifier toi-même dans la page de la box : WPS désactivé, mot de passe admin changé, mises à jour automatiques.");
                }
            }
            _ => {
                ui.weak("Services de ce PC qu'on peut joindre depuis le réseau. Pour en fermer un : arrête le programme, ou bloque-le avec le pare-feu (sudo ufw enable).");
                let mut l = ports_ecoute(&lire("/proc/net/tcp"));
                for x in ports_ecoute(&lire("/proc/net/tcp6")) {
                    if !l.iter().any(|y| y.0 == x.0) {
                        l.push(x);
                    }
                }
                l.sort();
                for (p, reseau) in l {
                    let (n, r, t) = infos_port(p);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{p}")).monospace().strong());
                        ui.label(if reseau { "ouvert au réseau" } else { "seulement ce PC" });
                        if reseau {
                            ui.colored_label(couleur(r), format!("{n} — {t}"));
                        } else {
                            ui.weak(n);
                        }
                    });
                }
                let ufw = std::process::Command::new("ufw").arg("status").output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
                if ufw.contains("inactive") || ufw.contains("inactif") {
                    ui.colored_label(Color32::from_rgb(255, 170, 80), "⚠ Le pare-feu ufw est désactivé : sudo ufw enable");
                } else if ufw.contains("active") || ufw.contains("actif") {
                    ui.colored_label(Color32::LIGHT_GREEN, "✔ Pare-feu ufw actif");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures_linux() {
        let r = "Iface\tDestination\tGateway \tFlags\nwlp2s0\t00000000\t0101A8C0\t0003\nwlp2s0\t0001A8C0\t00000000\t0001\n";
        assert_eq!(passerelle_de(r), Some(Ipv4Addr::new(192, 168, 1, 1)));
        let a = "IP address       HW type     Flags       HW address            Mask     Device\n192.168.1.1      0x1         0x2         aa:bb:cc:dd:ee:ff     *        wlp2s0\n192.168.1.9      0x1         0x0         00:00:00:00:00:00     *        wlp2s0\n";
        let t = arp_de(a);
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(&Ipv4Addr::new(192, 168, 1, 1)).map(|s| s.as_str()), Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(wifi_actif("no:Voisin:WPA2\nyes:MaBox:WPA2 WPA3\n"), Some(("MaBox".into(), "WPA2 WPA3".into())));
        let tcp = "  sl  local_address rem_address   st\n   0: 0100007F:0277 00000000:0000 0A 00000000\n   1: 00000000:0016 00000000:0000 0A 00000000\n   2: 0100007F:9C40 0101A8C0:01BB 01 00000000\n";
        assert_eq!(ports_ecoute(tcp), vec![(22, true), (631, false)]);
    }
}
