//! VPN maison (WireGuard simplifié) : depuis l'extérieur, le téléphone ou un portable rejoint la maison
//! comme s'il était sur le Wi-Fi (Outils, fichiers, serveur Minecraft, et sous Linux toute la maison).
//! Outils crée les clés, la configuration du PC et un QR code par appareil, et ouvre le port sur la box.
use crate::outil::{charger, enregistrer, Cat, Outil};
use base64::Engine;
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const PORT: u16 = 51820;
const RESEAU: &str = "10.66.66";

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Appareil {
    pub nom: String,
    pub prive: String,
    pub public: String,
    pub numero: u8,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Reglages {
    pub prive: String,
    pub public: String,
    pub adresse: String,
    pub appareils: Vec<Appareil>,
    /// trafic internet de l'appareil aussi par la maison (Wi-Fi public)
    pub tout_internet: bool,
}

/// Paire de clés WireGuard (privée, publique) en Base64.
pub fn cles() -> (String, String) {
    let mut b = [0u8; 32];
    let _ = getrandom::getrandom(&mut b);
    let p = x25519_dalek::StaticSecret::from(b);
    let q = x25519_dalek::PublicKey::from(&p);
    let e = &base64::engine::general_purpose::STANDARD;
    (e.encode(p.to_bytes()), e.encode(q.as_bytes()))
}

/// Clé publique d'une clé privée en Base64.
pub fn publique_de(prive: &str) -> Option<String> {
    let e = &base64::engine::general_purpose::STANDARD;
    let b: [u8; 32] = e.decode(prive.trim()).ok()?.try_into().ok()?;
    Some(e.encode(x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(b)).as_bytes()))
}

/// Sous-réseau de la maison (ex. « 192.168.1.0/24 ») d'après l'adresse locale.
pub fn reseau_maison(ip: &str) -> Option<String> {
    let o: Vec<&str> = ip.split('.').collect();
    (o.len() == 4).then(|| format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
}

/// Configuration du PC (serveur).
pub fn conf_serveur(r: &Reglages, linux: bool) -> String {
    let mut t = format!("[Interface]\nAddress = {RESEAU}.1/24\nListenPort = {PORT}\nPrivateKey = {}\n", r.prive);
    if linux {
        // routage vers la maison et internet pour les appareils connectés
        t += "PostUp = sysctl -w net.ipv4.ip_forward=1; iptables -t nat -A POSTROUTING -s 10.66.66.0/24 -j MASQUERADE; iptables -A FORWARD -i %i -j ACCEPT; iptables -A FORWARD -o %i -j ACCEPT\n";
        t += "PostDown = iptables -t nat -D POSTROUTING -s 10.66.66.0/24 -j MASQUERADE; iptables -D FORWARD -i %i -j ACCEPT; iptables -D FORWARD -o %i -j ACCEPT\n";
    }
    for a in &r.appareils {
        t += &format!("\n# {}\n[Peer]\nPublicKey = {}\nAllowedIPs = {RESEAU}.{}/32\n", a.nom, a.public, a.numero);
    }
    t
}

/// Configuration d'un appareil (à scanner dans l'appli WireGuard).
pub fn conf_appareil(r: &Reglages, a: &Appareil, maison: Option<&str>, linux: bool) -> String {
    let autorises = if r.tout_internet && linux {
        "0.0.0.0/0".to_string()
    } else if linux {
        match maison {
            Some(m) => format!("{RESEAU}.0/24, {m}"),
            None => format!("{RESEAU}.0/24"),
        }
    } else {
        format!("{RESEAU}.1/32")
    };
    format!(
        "[Interface]\nPrivateKey = {}\nAddress = {RESEAU}.{}/32\nDNS = 1.1.1.1\n\n[Peer]\nPublicKey = {}\nEndpoint = {}:{PORT}\nAllowedIPs = {autorises}\nPersistentKeepalive = 25\n",
        a.prive, a.numero, r.public, r.adresse
    )
}

fn dossier() -> PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.join("vpn")).unwrap_or_default()
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
    Some(ctx.load_texture(format!("qr_vpn_{}", texte.len()), egui::ColorImage::from_gray([n, n], &px), egui::TextureOptions::NEAREST))
}

/// Installe et démarre le tunnel du PC. Renvoie un message.
fn activer(r: &Reglages) -> Result<String, String> {
    let d = dossier();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let conf = d.join("outils-vpn.conf");
        std::fs::write(&conf, conf_serveur(r, false)).map_err(|e| e.to_string())?;
        let wg = PathBuf::from(std::env::var("ProgramFiles").unwrap_or(r"C:\Program Files".into())).join(r"WireGuard\wireguard.exe");
        if !wg.is_file() {
            return Err("WireGuard n'est pas installé : clique sur « Installer WireGuard ».".into());
        }
        let script = format!(
            "Start-Process -FilePath '{}' -ArgumentList '/installtunnelservice','\"{}\"' -Verb RunAs -Wait; New-NetFirewallRule -DisplayName 'Outils VPN' -Direction Inbound -Protocol UDP -LocalPort {PORT} -Action Allow -ErrorAction SilentlyContinue | Out-Null",
            wg.display(),
            conf.display()
        );
        crate::optimiseur::ps(&script)?;
        return Ok("✔ VPN actif sur le PC (service WireGuard « outils-vpn »).".into());
    }
    #[allow(unreachable_code)]
    {
        let conf = d.join("outils.conf");
        std::fs::write(&conf, conf_serveur(r, true)).map_err(|e| e.to_string())?;
        let script = format!("install -m 600 '{}' /etc/wireguard/outils.conf && systemctl enable wg-quick@outils && systemctl restart wg-quick@outils", conf.display());
        let o = std::process::Command::new("pkexec").args(["sh", "-c", &script]).output().map_err(|e| e.to_string())?;
        if o.status.success() {
            Ok("✔ VPN actif (wg-quick@outils), il redémarre avec le PC.".into())
        } else {
            Err(format!("{} (WireGuard est-il installé ? paquet « wireguard-tools »)", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("annulé")))
        }
    }
}

fn actif() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let o = std::process::Command::new("sc").args(["query", "WireGuardTunnel$outils-vpn"]).creation_flags(0x0800_0000).output();
        return o.map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING")).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    std::process::Command::new("systemctl").args(["is-active", "wg-quick@outils"]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active").unwrap_or(false)
}

pub struct Vpn {
    r: Reglages,
    nom: String,
    qr: Option<(String, egui::TextureHandle)>,
    message: Arc<Mutex<String>>,
    etat: Option<bool>,
}

impl Vpn {
    pub fn new() -> Self {
        let mut r: Reglages = charger("vpn");
        if r.prive.is_empty() {
            let (p, q) = cles();
            r.prive = p;
            r.public = q;
            enregistrer("vpn", &r);
        }
        Vpn { r, nom: String::new(), qr: None, message: Arc::default(), etat: None }
    }
}

impl Outil for Vpn {
    fn nom(&self) -> &'static str {
        "🔐 VPN maison"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let linux = cfg!(not(windows));
        ui.heading("VPN maison (WireGuard)");
        ui.weak(if linux {
            "Depuis l'extérieur (4G, Wi-Fi public), ton téléphone ou ton portable rejoint la maison comme s'il y était : Outils, fichiers, serveur Minecraft, imprimante… et même ta connexion internet, chiffrée."
        } else {
            "Depuis l'extérieur (4G, Wi-Fi public), ton téléphone ou ton portable rejoint ce PC comme s'il était à la maison : Outils, fichiers partagés, serveur Minecraft, bureau à distance. Tout est chiffré."
        });
        let etat = *self.etat.get_or_insert_with(actif);
        ui.label(RichText::new(if etat { "● VPN actif sur ce PC" } else { "● VPN pas encore activé" }).strong().color(if etat { Color32::LIGHT_GREEN } else { Color32::GRAY }));
        ui.separator();
        ui.strong("1. L'adresse de la maison");
        ui.horizontal_wrapped(|ui| {
            if ui.add(egui::TextEdit::singleline(&mut self.r.adresse).hint_text("adresse IP publique ou nom DynDNS").desired_width(240.0)).changed() {
                enregistrer("vpn", &self.r);
                self.qr = None;
            }
            if ui.button("🔍 Trouver").clicked() {
                if let Some(ip) = crate::upnp::ip_publique() {
                    self.r.adresse = ip;
                    enregistrer("vpn", &self.r);
                    self.qr = None;
                }
            }
        });
        ui.weak("Si ton adresse publique change souvent, crée un nom gratuit (DuckDNS, No-IP, ou le DynDNS de ta box) et écris-le ici.");
        ui.strong("2. Ouvrir le port sur la box");
        ui.horizontal_wrapped(|ui| {
            if ui.button(format!("🔓 Ouvrir le port {PORT} (UDP) automatiquement")).clicked() {
                let m = self.message.clone();
                *m.lock().unwrap() = "Ouverture du port…".into();
                std::thread::spawn(move || {
                    *m.lock().unwrap() = match crate::upnp::ouvrir_port(PORT, "UDP", "Outils VPN") {
                        Ok(()) => format!("✔ Port {PORT} ouvert sur la box"),
                        Err(e) => format!("✖ {e} — ouvre-le à la main dans la box (UDP {PORT} vers ce PC)"),
                    };
                });
            }
        });
        ui.strong("3. Activer le VPN sur ce PC");
        ui.horizontal_wrapped(|ui| {
            #[cfg(windows)]
            if ui.button("📥 Installer WireGuard").clicked() {
                let m = self.message.clone();
                std::thread::spawn(move || {
                    *m.lock().unwrap() = match crate::majlogiciels::winget(&["install", "--id", "WireGuard.WireGuard", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"]) {
                        Ok(_) => "✔ WireGuard installé".into(),
                        Err(e) => format!("✖ {e}"),
                    };
                });
            }
            if ui.add_enabled(!self.r.appareils.is_empty(), egui::Button::new("▶ Activer / mettre à jour le VPN")).on_disabled_hover_text("Ajoute d'abord un appareil").clicked() {
                let (m, r) = (self.message.clone(), self.r.clone());
                *m.lock().unwrap() = "Activation (une fenêtre peut demander l'autorisation)…".into();
                std::thread::spawn(move || {
                    *m.lock().unwrap() = activer(&r).unwrap_or_else(|e| format!("✖ {e}"));
                });
                self.etat = None;
            }
        });
        if linux && ui.checkbox(&mut self.r.tout_internet, "Faire passer aussi tout internet par la maison (protège sur les Wi-Fi publics)").changed() {
            enregistrer("vpn", &self.r);
            self.qr = None;
        }
        ui.label(self.message.lock().unwrap().clone());
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.separator();
        ui.strong("4. Mes appareils");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("nom (ex. Téléphone de Léo)").desired_width(220.0));
            if ui.add_enabled(!self.nom.trim().is_empty() && self.r.appareils.len() < 250, egui::Button::new("➕ Ajouter")).clicked() {
                let (p, q) = cles();
                let numero = (2..=254).find(|n| !self.r.appareils.iter().any(|a| a.numero == *n)).unwrap_or(2);
                self.r.appareils.push(Appareil { nom: std::mem::take(&mut self.nom), prive: p, public: q, numero });
                enregistrer("vpn", &self.r);
                *self.message.lock().unwrap() = "Appareil ajouté : clique « Activer / mettre à jour le VPN » pour que le PC l'accepte.".into();
            }
        });
        let maison = crate::telephone::ip_locale().and_then(|ip| reseau_maison(&ip));
        let mut retirer = None;
        for a in self.r.appareils.clone() {
            ui.horizontal(|ui| {
                ui.label(format!("📱 {} · {RESEAU}.{}", a.nom, a.numero));
                if ui.small_button("QR code").clicked() {
                    let t = conf_appareil(&self.r, &a, maison.as_deref(), linux);
                    self.qr = texture_qr(ui.ctx(), &t).map(|x| (a.nom.clone(), x));
                }
                if ui.small_button("💾 Fichier .conf").clicked() {
                    if let Some(p) = rfd::FileDialog::new().set_file_name(format!("{}.conf", a.nom.replace(' ', "_"))).save_file() {
                        let _ = std::fs::write(p, conf_appareil(&self.r, &a, maison.as_deref(), linux));
                    }
                }
                if ui.small_button("🗑").clicked() {
                    retirer = Some(a.numero);
                }
            });
        }
        if let Some(n) = retirer {
            self.r.appareils.retain(|a| a.numero != n);
            enregistrer("vpn", &self.r);
        }
        if let Some((n, t)) = &self.qr {
            if self.r.adresse.is_empty() {
                ui.colored_label(Color32::from_rgb(255, 200, 100), "Remplis d'abord l'adresse de la maison (étape 1).");
            }
            ui.label(format!("Dans l'appli WireGuard du téléphone : ➕ › « Scanner un code QR » ({n}) :"));
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(260.0, 260.0)));
            ui.weak("Ce QR code contient la clé secrète de l'appareil : ne le montre à personne.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vpn() {
        let (p, q) = cles();
        assert_eq!(publique_de(&p).as_deref(), Some(q.as_str()));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(&q).unwrap().len(), 32);
        // vecteur de la RFC 7748 (clé privée d'Alice → clé publique)
        let alice = base64::engine::general_purpose::STANDARD.encode(hex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(publique_de(&alice).unwrap()).unwrap(), hex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
        let mut r = Reglages { prive: p, public: q, adresse: "maison.duckdns.org".into(), ..Default::default() };
        let a = Appareil { nom: "Tel".into(), prive: "AAA".into(), public: "BBB".into(), numero: 2 };
        r.appareils.push(a.clone());
        let s = conf_serveur(&r, true);
        assert!(s.contains("ListenPort = 51820") && s.contains("AllowedIPs = 10.66.66.2/32") && s.contains("MASQUERADE"));
        let c = conf_appareil(&r, &a, Some("192.168.1.0/24"), true);
        assert!(c.contains("Endpoint = maison.duckdns.org:51820") && c.contains("AllowedIPs = 10.66.66.0/24, 192.168.1.0/24"));
        assert!(conf_appareil(&r, &a, None, false).contains("AllowedIPs = 10.66.66.1/32"));
        assert_eq!(reseau_maison("192.168.1.42").as_deref(), Some("192.168.1.0/24"));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }
}
