//! Wi-Fi : connexion et signal en direct (trouver le meilleur endroit), appareils connectés,
//! test de débit, ping des serveurs de jeux, réveil du PC.
use crate::commun::*;
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const REGIONS: [(&str, &str); 10] = [
    ("Paris", "dynamodb.eu-west-3.amazonaws.com"),
    ("Londres", "dynamodb.eu-west-2.amazonaws.com"),
    ("Francfort", "dynamodb.eu-central-1.amazonaws.com"),
    ("Irlande", "dynamodb.eu-west-1.amazonaws.com"),
    ("Stockholm", "dynamodb.eu-north-1.amazonaws.com"),
    ("USA Est", "dynamodb.us-east-1.amazonaws.com"),
    ("USA Ouest", "dynamodb.us-west-1.amazonaws.com"),
    ("Brésil", "dynamodb.sa-east-1.amazonaws.com"),
    ("Japon", "dynamodb.ap-northeast-1.amazonaws.com"),
    ("Singapour", "dynamodb.ap-southeast-1.amazonaws.com"),
];

/// Qualité du signal en %, et en mots.
pub fn qualite(rssi: i32) -> (u32, &'static str) {
    let q = ((rssi + 95).clamp(0, 50) * 2) as u32;
    let t = match rssi {
        r if r >= -55 => "excellent",
        r if r >= -67 => "bon",
        r if r >= -75 => "moyen",
        r if r >= -85 => "faible",
        _ => "très faible",
    };
    (q, t)
}

/// Ce qu'est probablement un appareil, d'après ses ports ouverts.
pub fn deviner(ports: &[u16], passerelle: bool) -> &'static str {
    if passerelle {
        return "📡 Box / routeur";
    }
    let a = |p: u16| ports.contains(&p);
    if a(62078) {
        "📱 iPhone / iPad"
    } else if a(8008) || a(8009) {
        "📺 Chromecast / TV"
    } else if a(9100) || a(631) {
        "🖨 Imprimante"
    } else if a(554) {
        "📷 Caméra"
    } else if a(445) || a(139) || a(3389) {
        "💻 PC Windows"
    } else if a(5000) || a(7000) {
        "🔊 Appareil Apple / enceinte"
    } else if a(22) {
        "🖥 Serveur / NAS"
    } else if a(80) || a(443) {
        "🌐 Appareil connecté"
    } else {
        "❔ Appareil"
    }
}

const PORTS: [u16; 15] = [80, 443, 22, 445, 139, 3389, 62078, 8008, 8009, 9100, 631, 554, 5000, 7000, 8080];

/// Teste un appareil : présent ? et quels ports répondent.
fn sonder(ip: &str) -> Option<Vec<u16>> {
    let mut vivant = false;
    let mut ouverts = vec![];
    for p in PORTS {
        let Ok(a) = format!("{ip}:{p}").parse::<SocketAddr>() else { continue };
        match TcpStream::connect_timeout(&a, Duration::from_millis(250)) {
            Ok(_) => {
                vivant = true;
                ouverts.push(p);
            }
            // « refusé » veut dire que l'appareil existe
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => vivant = true,
            Err(_) => {}
        }
    }
    vivant.then_some(ouverts)
}

fn ping_tcp(hote: &str) -> Option<u128> {
    use std::net::ToSocketAddrs;
    let a = (hote, 443).to_socket_addrs().ok()?.next()?;
    let mut v = vec![];
    for _ in 0..3 {
        let t = Instant::now();
        if TcpStream::connect_timeout(&a, Duration::from_secs(2)).is_ok() {
            v.push(t.elapsed().as_millis());
        }
    }
    v.sort();
    v.first().copied()
}

pub struct Wifi {
    onglet: usize,
    e: Etat,
    infos: Option<crate::android::InfosWifi>,
    historique: Vec<i32>,
    dernier: Instant,
    appareils: Arc<Mutex<Vec<(String, Vec<u16>)>>>,
    debit: Arc<Mutex<Option<(f64, f64, u128)>>>,
    pings: Arc<Mutex<Vec<(String, Option<u128>)>>>,
}

impl Wifi {
    pub fn new() -> Self {
        Wifi { onglet: 0, e: Arc::default(), infos: None, historique: vec![], dernier: Instant::now() - Duration::from_secs(5), appareils: Arc::default(), debit: Arc::default(), pings: Arc::default() }
    }
}

impl Outil for Wifi {
    fn nom(&self) -> &'static str {
        "📶 Wi-Fi"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.dernier.elapsed() > Duration::from_millis(900) {
            self.dernier = Instant::now();
            self.infos = crate::android::wifi();
            if let Some(i) = &self.infos {
                self.historique.push(i.rssi);
                if self.historique.len() > 120 {
                    self.historique.remove(0);
                }
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        onglets(ui, &mut self.onglet, &["📶 Connexion", "👥 Appareils", "🚀 Débit", "🎮 Ping jeux"]);
        let occupe = etat_ui(ui, &self.e);
        let infos = self.infos.clone().unwrap_or_default();
        match self.onglet {
            0 => {
                if infos.ip == "0.0.0.0" || infos.ip.is_empty() {
                    ui.colored_label(ORANGE, "Pas connecté au Wi-Fi.");
                    return;
                }
                let (q, mot) = qualite(infos.rssi);
                let nom = if infos.ssid.is_empty() || infos.ssid.contains("unknown") { "(nom masqué : autorise la localisation)".to_string() } else { infos.ssid.clone() };
                ui.label(RichText::new(format!("📶 {nom}")).size(22.0).strong());
                let c = if q >= 70 { VERT } else if q >= 40 { ORANGE } else { ROUGE };
                ui.add(egui::ProgressBar::new(q as f32 / 100.0).fill(c).text(format!("Signal {mot} ({} dBm)", infos.rssi)));
                // courbe : promène-toi dans la maison pour trouver le meilleur endroit
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 120.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 8.0, ui.visuals().extreme_bg_color);
                let pts: Vec<egui::Pos2> = self.historique.iter().enumerate().map(|(k, v)| {
                    let x = r.left() + k as f32 / 119.0 * r.width();
                    let y = r.bottom() - (qualite(*v).0 as f32 / 100.0) * r.height();
                    egui::pos2(x, y)
                }).collect();
                ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.5_f32, ROSE)));
                ui.weak("Promène-toi avec le téléphone : la courbe montre où le signal est le meilleur (pour placer un répéteur ou ton PC).");
                egui::Grid::new("infos_wifi").striped(true).show(ui, |ui| {
                    let bande = if infos.frequence > 5900 { "6 GHz" } else if infos.frequence > 4900 { "5 GHz (rapide, porte moins loin)" } else { "2,4 GHz (porte loin, plus lent)" };
                    for (a, b) in [("Bande", bande.to_string()), ("Vitesse de liaison", format!("{} Mb/s", infos.vitesse)), ("Adresse du téléphone", infos.ip.clone()), ("Box (passerelle)", infos.passerelle.clone()), ("DNS", infos.dns.clone())] {
                        ui.label(a);
                        ui.strong(b);
                        ui.end_row();
                    }
                });
                if infos.ssid.contains("unknown") && ui.button("Afficher le nom du Wi-Fi (autoriser la localisation)").clicked() {
                    crate::android::demander(&["android.permission.ACCESS_FINE_LOCATION"]);
                }
            }
            1 => {
                ui.weak("Liste les appareils branchés sur ton Wi-Fi (téléphones, PC, TV, caméras…). Un appareil inconnu ? Change le mot de passe de ta box.");
                if gros_bouton(ui, "🔎 Scanner le réseau") && !occupe {
                    let (l, i) = (self.appareils.clone(), infos.clone());
                    l.lock().unwrap().clear();
                    en_fond(&self.e, "Scan des 254 adresses…", move || {
                        let base = i.ip.rsplit_once('.').map(|x| x.0.to_string()).ok_or("pas d'adresse Wi-Fi")?;
                        let mut fils = vec![];
                        for lot in (1..=254u32).collect::<Vec<_>>().chunks(32) {
                            let lot = lot.to_vec();
                            let (l, base) = (l.clone(), base.clone());
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
                        let mut v = l.lock().unwrap();
                        if !v.iter().any(|x| x.0 == i.ip) {
                            v.push((i.ip.clone(), vec![]));
                        }
                        v.sort_by_key(|x| x.0.rsplit('.').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0));
                        Ok(format!("{} appareil(s) trouvé(s)", v.len()))
                    });
                }
                for (ip, ports) in self.appareils.lock().unwrap().iter() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let t = if *ip == infos.ip { "📱 Ce téléphone" } else { deviner(ports, *ip == infos.passerelle) };
                            ui.label(RichText::new(t).size(17.0).strong());
                            ui.monospace(ip);
                        });
                        if !ports.is_empty() {
                            ui.weak(format!("ports ouverts : {}", ports.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", ")));
                        }
                    });
                }
            }
            2 => {
                if gros_bouton(ui, "🚀 Tester le débit") && !occupe {
                    let d = self.debit.clone();
                    en_fond(&self.e, "Test en cours (une vingtaine de secondes)…", move || {
                        let ping = ping_tcp("speed.cloudflare.com").ok_or("pas d'accès à internet")?;
                        let t = Instant::now();
                        let r = ureq::get("https://speed.cloudflare.com/__down?bytes=25000000").timeout(Duration::from_secs(60)).call().map_err(|e| e.to_string())?;
                        let n = std::io::copy(&mut r.into_reader(), &mut std::io::sink()).map_err(|e| e.to_string())?;
                        let des = n as f64 * 8.0 / t.elapsed().as_secs_f64() / 1e6;
                        let corps = vec![0u8; 8_000_000];
                        let t = Instant::now();
                        ureq::post("https://speed.cloudflare.com/__up").timeout(Duration::from_secs(60)).send_bytes(&corps).map_err(|e| e.to_string())?;
                        let mon = corps.len() as f64 * 8.0 / t.elapsed().as_secs_f64() / 1e6;
                        *d.lock().unwrap() = Some((des, mon, ping));
                        Ok(String::new())
                    });
                }
                if let Some((d, m, p)) = *self.debit.lock().unwrap() {
                    for (t, v, u) in [("⬇ Téléchargement", d, "Mb/s"), ("⬆ Envoi", m, "Mb/s"), ("⏱ Ping", p as f64, "ms")] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t).size(18.0));
                            ui.label(RichText::new(format!("{v:.0} {u}")).size(28.0).strong().color(ROSE));
                        });
                    }
                    ui.weak(match d {
                        x if x > 300.0 => "Très rapide : parfait pour tout, même télécharger de gros jeux.",
                        x if x > 50.0 => "Rapide : streaming 4K et jeu en ligne sans souci.",
                        x if x > 10.0 => "Correct : vidéo HD et jeu en ligne.",
                        _ => "Lent : rapproche-toi de la box ou essaie le 5 GHz.",
                    });
                }
            }
            _ => {
                if gros_bouton(ui, "🎮 Mesurer le ping vers les serveurs") && !occupe {
                    let p = self.pings.clone();
                    en_fond(&self.e, "Mesure…", move || {
                        let mut v: Vec<(String, Option<u128>)> = REGIONS.iter().map(|(n, h)| (n.to_string(), ping_tcp(h))).collect();
                        v.sort_by_key(|x| x.1.unwrap_or(u128::MAX));
                        *p.lock().unwrap() = v;
                        Ok(String::new())
                    });
                }
                for (n, ms) in self.pings.lock().unwrap().iter() {
                    ui.horizontal(|ui| {
                        ui.add_sized([110.0, 24.0], egui::Label::new(RichText::new(n).size(17.0)));
                        match ms {
                            Some(m) => {
                                let c = if *m < 40 { VERT } else if *m < 100 { ORANGE } else { ROUGE };
                                ui.add(egui::ProgressBar::new((*m as f32 / 300.0).min(1.0)).fill(c).desired_width(ui.available_width() - 80.0));
                                ui.colored_label(c, format!("{m} ms"));
                            }
                            None => {
                                ui.colored_label(Color32::GRAY, "injoignable");
                            }
                        }
                    });
                }
                ui.weak("Choisis dans tes jeux la région au ping le plus bas.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_et_appareils() {
        assert_eq!(qualite(-50).1, "excellent");
        assert_eq!(qualite(-95).0, 0);
        assert_eq!(qualite(-45).0, 100);
        assert_eq!(deviner(&[62078], false), "📱 iPhone / iPad");
        assert_eq!(deviner(&[80], true), "📡 Box / routeur");
        assert_eq!(deviner(&[445, 80], false), "💻 PC Windows");
    }
}
