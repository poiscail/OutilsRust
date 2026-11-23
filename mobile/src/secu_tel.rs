//! Sécurité du téléphone : quelles applis ont accès à la caméra, au micro, à la position, aux SMS…
//! et audit du Wi-Fi depuis le téléphone (sécurité des réseaux, faux Wi-Fi, appareils à risque).
use crate::commun::*;
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// (permission, nom, gravité 1-3)
pub const SENSIBLES: &[(&str, &str, u8)] = &[
    ("android.permission.CAMERA", "📷 Caméra", 2),
    ("android.permission.RECORD_AUDIO", "🎙 Micro", 3),
    ("android.permission.ACCESS_FINE_LOCATION", "📍 Position précise", 2),
    ("android.permission.ACCESS_COARSE_LOCATION", "📍 Position approximative", 1),
    ("android.permission.ACCESS_BACKGROUND_LOCATION", "📍 Position même appli fermée", 3),
    ("android.permission.READ_CONTACTS", "👥 Contacts", 2),
    ("android.permission.READ_SMS", "💬 Lire les SMS", 3),
    ("android.permission.RECEIVE_SMS", "💬 Recevoir les SMS", 3),
    ("android.permission.SEND_SMS", "💬 Envoyer des SMS (payants !)", 3),
    ("android.permission.READ_CALL_LOG", "📞 Journal d'appels", 3),
    ("android.permission.CALL_PHONE", "📞 Passer des appels", 2),
    ("android.permission.READ_PHONE_STATE", "📞 Infos du téléphone", 1),
    ("android.permission.BODY_SENSORS", "❤ Capteurs du corps", 2),
    ("android.permission.READ_MEDIA_IMAGES", "🖼 Photos", 1),
    ("android.permission.READ_MEDIA_VIDEO", "🎞 Vidéos", 1),
    ("android.permission.READ_EXTERNAL_STORAGE", "🗂 Fichiers", 1),
    ("android.permission.READ_CALENDAR", "📅 Agenda", 1),
    ("android.permission.ACTIVITY_RECOGNITION", "🏃 Activité physique", 1),
    ("android.permission.NEARBY_WIFI_DEVICES", "📶 Appareils Wi-Fi proches", 1),
    ("android.permission.BLUETOOTH_SCAN", "🔵 Appareils Bluetooth proches", 1),
];

pub fn infos_permission(p: &str) -> Option<(&'static str, u8)> {
    SENSIBLES.iter().find(|x| x.0 == p).map(|x| (x.1, x.2))
}

/// Score de « curiosité » d'une appli (somme des gravités des permissions accordées).
pub fn curiosite(accordees: &[String]) -> u32 {
    accordees.iter().filter_map(|p| infos_permission(p)).map(|x| x.1 as u32).sum()
}

pub struct Permissions {
    applis: Arc<Mutex<Option<Vec<crate::android::Appli>>>>,
    systeme: bool,
    vue: usize,
}

impl Permissions {
    pub fn new() -> Self {
        Permissions { applis: Arc::default(), systeme: false, vue: 0 }
    }
}

impl Outil for Permissions {
    fn nom(&self) -> &'static str {
        "🛡 Permissions des applis"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.weak("Quelles applis peuvent utiliser ta caméra, ton micro, ta position, tes SMS ou tes contacts. Retire ce qui ne sert à rien : une lampe torche n'a pas besoin de tes SMS.");
        if self.applis.lock().unwrap().is_none() {
            let a = self.applis.clone();
            *a.lock().unwrap() = Some(vec![]);
            std::thread::spawn(move || *a.lock().unwrap() = Some(crate::android::applis()));
        }
        let v = self.applis.lock().unwrap().clone().unwrap_or_default();
        if v.is_empty() {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.vue, 0, "Par appli");
            ui.selectable_value(&mut self.vue, 1, "Par permission");
            ui.checkbox(&mut self.systeme, "applis du système");
        });
        let liste: Vec<&crate::android::Appli> = v.iter().filter(|a| (self.systeme || !a.systeme) && curiosite(&a.accordees) > 0).collect();
        if self.vue == 0 {
            let mut tri = liste.clone();
            tri.sort_by_key(|a| std::cmp::Reverse(curiosite(&a.accordees)));
            for a in tri {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(&a.nom).strong());
                        if ui.small_button("⚙ Réglages").clicked() {
                            crate::android::fiche_appli(&a.paquet);
                        }
                    });
                    let p: Vec<String> = a.accordees.iter().filter_map(|p| infos_permission(p)).map(|x| x.0.to_string()).collect();
                    let c = curiosite(&a.accordees);
                    ui.colored_label(if c >= 8 { Color32::from_rgb(255, 150, 100) } else { ui.visuals().text_color() }, p.join(" · "));
                });
            }
        } else {
            for (perm, nom, g) in SENSIBLES {
                let qui: Vec<&&crate::android::Appli> = liste.iter().filter(|a| a.accordees.iter().any(|x| x == perm)).collect();
                if qui.is_empty() {
                    continue;
                }
                egui::CollapsingHeader::new(RichText::new(format!("{nom} · {} appli(s)", qui.len())).color(if *g == 3 { Color32::from_rgb(255, 170, 80) } else { ui.visuals().text_color() })).id_salt(perm).show(ui, |ui| {
                    for a in qui {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(&a.nom);
                            if ui.small_button("⚙").clicked() {
                                crate::android::fiche_appli(&a.paquet);
                            }
                        });
                    }
                });
            }
        }
        ui.weak("Android › Paramètres › Sécurité et confidentialité › Gestionnaire d'autorisations permet aussi de tout revoir. Les permissions « Seulement si l'appli est en cours d'utilisation » sont les plus sûres.");
    }
}

// ===================================================================================================
// 28 · Audit du Wi-Fi depuis le téléphone
// ===================================================================================================

/// Note de sécurité d'un réseau d'après ses « capabilities » Android : (0 à 3, texte).
pub fn securite_reseau(caps: &str) -> (u8, &'static str) {
    let c = caps.to_uppercase();
    if c.contains("WPA3") || c.contains("SAE") {
        (3, "WPA3 : très bien")
    } else if c.contains("WPA2") || c.contains("RSN") {
        if c.contains("TKIP") && !c.contains("CCMP") { (1, "WPA2 avec TKIP : ancien") } else { (2, "WPA2 : bien") }
    } else if c.contains("WPA") {
        (1, "WPA : ancien, cassable")
    } else if c.contains("WEP") {
        (0, "WEP : cassable en quelques minutes")
    } else {
        (0, "ouvert : sans mot de passe")
    }
}

/// Réseaux qui portent le même nom avec des sécurités différentes (faux Wi-Fi possible).
pub fn jumeaux(v: &[crate::android::ReseauVoisin]) -> Vec<String> {
    let mut m: std::collections::BTreeMap<&str, Vec<u8>> = std::collections::BTreeMap::new();
    for r in v.iter().filter(|r| !r.ssid.is_empty()) {
        let n = securite_reseau(&r.securite).0;
        let e = m.entry(&r.ssid).or_default();
        if !e.contains(&n) {
            e.push(n);
        }
    }
    m.into_iter().filter(|(_, s)| s.len() > 1).map(|(n, _)| n.to_string()).collect()
}

const RISQUES: &[(u16, &str)] = &[
    (23, "Telnet ouvert : accès à distance non chiffré, cible des virus d'objets connectés"),
    (21, "FTP : mots de passe en clair"),
    (554, "flux vidéo de caméra : vérifie qu'il demande un mot de passe"),
    (1883, "MQTT : objets connectés souvent sans mot de passe"),
    (5900, "VNC : prise en main à distance"),
    (3389, "Bureau à distance Windows"),
    (7547, "TR-069 : gestion de la box par l'opérateur"),
    (445, "partage de fichiers Windows"),
];

#[derive(Clone)]
struct Trouve {
    ip: String,
    genre: &'static str,
    risques: Vec<&'static str>,
}

fn scanner_reseau(ip_tel: &str, passerelle: &str, prog: &Mutex<(usize, String)>) -> Vec<Trouve> {
    let base: Vec<&str> = ip_tel.split('.').collect();
    if base.len() != 4 {
        return vec![];
    }
    let prefixe = format!("{}.{}.{}", base[0], base[1], base[2]);
    let file = Arc::new(Mutex::new((1..255u32).collect::<Vec<_>>()));
    let res = Arc::new(Mutex::new(vec![]));
    std::thread::scope(|s| {
        for _ in 0..32 {
            let (f, r, prefixe) = (file.clone(), res.clone(), prefixe.clone());
            s.spawn(move || loop {
                let Some(d) = f.lock().unwrap().pop() else { break };
                let ip = format!("{prefixe}.{d}");
                prog.lock().unwrap().0 += 1;
                if ip == ip_tel {
                    continue;
                }
                let Some(mut ports) = crate::wifi_tel::sonder(&ip) else { continue };
                for (p, _) in RISQUES {
                    if !ports.contains(p) {
                        if let Ok(a) = format!("{ip}:{p}").parse::<SocketAddr>() {
                            if TcpStream::connect_timeout(&a, Duration::from_millis(250)).is_ok() {
                                ports.push(*p);
                            }
                        }
                    }
                }
                let risques: Vec<&'static str> = RISQUES.iter().filter(|(p, _)| ports.contains(p)).map(|x| x.1).collect();
                r.lock().unwrap().push(Trouve { genre: crate::wifi_tel::deviner(&ports, ip == passerelle), ip, risques });
            });
        }
    });
    let mut v = res.lock().unwrap().clone();
    v.sort_by_key(|t| std::cmp::Reverse(t.risques.len()));
    v
}

pub struct AuditWifi {
    voisins: Vec<crate::android::ReseauVoisin>,
    appareils: Arc<Mutex<Option<Vec<Trouve>>>>,
    prog: Arc<Mutex<(usize, String)>>,
    e: Etat,
    charge: bool,
}

impl AuditWifi {
    pub fn new() -> Self {
        AuditWifi { voisins: vec![], appareils: Arc::default(), prog: Arc::default(), e: Arc::default(), charge: false }
    }
}

impl Outil for AuditWifi {
    fn nom(&self) -> &'static str {
        "🔐 Audit du Wi-Fi"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.weak("Vérifie la sécurité de ton Wi-Fi et des réseaux autour, repère un éventuel faux Wi-Fi, et cherche les appareils à risque chez toi. Seulement sur TON réseau.");
        if !self.charge || ui.button("⟳ Réanalyser les réseaux").clicked() {
            self.charge = true;
            self.voisins = crate::android::scan_wifi();
        }
        let w = crate::android::wifi();
        if let Some(w) = &w {
            let moi = self.voisins.iter().find(|v| v.ssid == w.ssid);
            let (n, t) = moi.map(|m| securite_reseau(&m.securite)).unwrap_or((2, "inconnue"));
            ui.label(RichText::new(format!("Ton réseau « {} » : {t}", w.ssid)).size(17.0).strong().color([Color32::LIGHT_RED, Color32::from_rgb(255, 170, 80), Color32::LIGHT_GREEN, Color32::LIGHT_GREEN][n as usize]));
            if moi.is_some_and(|m| m.securite.to_uppercase().contains("WPS")) {
                ui.colored_label(Color32::from_rgb(255, 170, 80), "⚠ WPS est activé sur ta box : ce bouton/code permet parfois de deviner le mot de passe. Désactive-le dans les réglages de la box.");
            }
            if n < 2 {
                ui.label("→ Dans la page de ta box, choisis WPA2-AES ou WPA3 et un long mot de passe.");
            }
        }
        let j = jumeaux(&self.voisins);
        for s in &j {
            ui.colored_label(Color32::LIGHT_RED, format!("❗ « {s} » existe avec plusieurs sécurités différentes : un faux réseau imite peut-être le vrai. Ne te connecte qu'à celui qui demande le mot de passe habituel."));
        }
        let ouverts = self.voisins.iter().filter(|v| securite_reseau(&v.securite).0 == 0 && !v.ssid.is_empty()).count();
        if ouverts > 0 {
            ui.weak(format!("{ouverts} réseau(x) ouvert(s) ou en WEP autour : ne t'y connecte pas pour des choses importantes (banque, mots de passe)."));
        }
        egui::CollapsingHeader::new(format!("Réseaux autour ({})", self.voisins.len())).show(ui, |ui| {
            for v in &self.voisins {
                let (n, t) = securite_reseau(&v.securite);
                ui.colored_label([Color32::LIGHT_RED, Color32::from_rgb(255, 170, 80), Color32::LIGHT_GREEN, Color32::LIGHT_GREEN][n as usize], format!("{} · {t} · {} dBm", if v.ssid.is_empty() { "(caché)" } else { &v.ssid }, v.rssi));
            }
        });
        if self.voisins.is_empty() {
            ui.weak("Aucun réseau visible : active la localisation (Android l'exige pour lister les Wi-Fi).");
        }
        ui.separator();
        ui.strong("Appareils de la maison");
        let occupe = etat_ui(ui, &self.e);
        if !occupe && gros_bouton(ui, "🔎 Chercher les appareils à risque") {
            if let Some(w) = w.clone() {
                let (a, p, e) = (self.appareils.clone(), self.prog.clone(), self.e.clone());
                *p.lock().unwrap() = (0, String::new());
                en_fond(&e, "Analyse du réseau…", move || {
                    let v = scanner_reseau(&w.ip, &w.passerelle, &p);
                    let n = v.len();
                    *a.lock().unwrap() = Some(v);
                    Ok(format!("{n} appareil(s) trouvé(s)"))
                });
            }
        }
        if occupe {
            let (n, _) = self.prog.lock().unwrap().clone();
            ui.add(egui::ProgressBar::new(n as f32 / 254.0));
        }
        if let Some(v) = self.appareils.lock().unwrap().clone() {
            for t in v {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} {}", t.genre, t.ip));
                    if t.risques.is_empty() {
                        ui.colored_label(Color32::LIGHT_GREEN, "✔");
                    }
                });
                for r in t.risques {
                    ui.colored_label(Color32::from_rgb(255, 170, 80), format!("   ⚠ {r}"));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn securite() {
        assert_eq!(securite_reseau("[WPA2-PSK-CCMP][RSN-PSK-CCMP][ESS][WPS]").0, 2);
        assert_eq!(securite_reseau("[WPA3-SAE-CCMP][ESS]").0, 3);
        assert_eq!(securite_reseau("[WEP][ESS]").0, 0);
        assert_eq!(securite_reseau("[ESS]").0, 0);
        let v = crate::android::scan_wifi();
        assert_eq!(jumeaux(&v), vec!["Box-Maison".to_string()]);
        assert_eq!(curiosite(&["android.permission.READ_SMS".into(), "android.permission.CAMERA".into(), "x".into()]), 5);
    }
}
