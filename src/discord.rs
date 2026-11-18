//! Discord : statut « Rich Presence » (jeu en cours, serveur Minecraft) via le canal local de l'appli Discord,
//! et messages automatiques dans un salon via un webhook (serveur allumé / éteint, joueurs).
//! Réglages dans %APPDATA%\Outils\discord.json.
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub presence: bool,
    pub client_id: String,
    pub montrer_jeu: bool,
    pub montrer_serveur: bool,
    pub texte_libre: String,
    pub webhook: String,
    pub webhook_serveur: bool,
    pub webhook_joueurs: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            presence: false, client_id: String::new(), montrer_jeu: true, montrer_serveur: true, texte_libre: String::new(),
            webhook: String::new(), webhook_serveur: true, webhook_joueurs: true,
        }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("discord.json")
}

/// Trame IPC Discord : opcode (u32 LE), longueur (u32 LE), JSON.
pub fn trame(op: u32, v: &Value) -> Vec<u8> {
    let corps = v.to_string().into_bytes();
    let mut t = Vec::with_capacity(8 + corps.len());
    t.extend(op.to_le_bytes());
    t.extend((corps.len() as u32).to_le_bytes());
    t.extend(corps);
    t
}

fn lire_trame(f: &mut File) -> std::io::Result<(u32, Value)> {
    let mut entete = [0u8; 8];
    f.read_exact(&mut entete)?;
    let op = u32::from_le_bytes(entete[..4].try_into().unwrap());
    let n = u32::from_le_bytes(entete[4..].try_into().unwrap()) as usize;
    if n > 1 << 20 {
        return Err(std::io::Error::other("trame trop grande"));
    }
    let mut corps = vec![0u8; n];
    f.read_exact(&mut corps)?;
    Ok((op, serde_json::from_slice(&corps).unwrap_or(Value::Null)))
}

/// Ce qu'on affiche : (détails, état). None = rien.
pub fn activite(r: &Reglages, jeu: Option<&str>, serveur: Option<&(bool, Vec<String>)>) -> Option<(String, String)> {
    let mut details = None;
    let mut etat = None;
    if r.montrer_jeu {
        details = jeu.map(|j| format!("Joue à {j}"));
    }
    if r.montrer_serveur {
        if let Some((pret, joueurs)) = serveur {
            let s = if *pret { format!("Serveur Minecraft : {} joueur(s)", joueurs.len()) } else { "Serveur Minecraft : démarrage…".into() };
            if details.is_none() { details = Some(s) } else { etat = Some(s) }
        }
    }
    if !r.texte_libre.trim().is_empty() {
        let t = r.texte_libre.trim().to_string();
        if details.is_none() { details = Some(t) } else if etat.is_none() { etat = Some(t) }
    }
    details.map(|d| (d, etat.unwrap_or_default()))
}

/// Messages webhook pour le passage d'un état du serveur à l'autre.
pub fn evenements(r: &Reglages, avant: Option<&(bool, Vec<String>)>, apres: Option<&(bool, Vec<String>)>) -> Vec<String> {
    let mut m = vec![];
    let pret = |s: Option<&(bool, Vec<String>)>| s.is_some_and(|x| x.0);
    if r.webhook_serveur {
        if !pret(avant) && pret(apres) {
            m.push("🟢 Le serveur Minecraft est en ligne.".to_string());
        }
        if pret(avant) && apres.is_none() {
            m.push("🔴 Le serveur Minecraft est arrêté.".to_string());
        }
    }
    if r.webhook_joueurs {
        if let (Some(a), Some(b)) = (avant, apres) {
            for j in b.1.iter().filter(|j| !a.1.contains(j)) {
                m.push(format!("➡ {j} a rejoint le serveur ({} en ligne).", b.1.len()));
            }
            for j in a.1.iter().filter(|j| !b.1.contains(j)) {
                m.push(format!("⬅ {j} a quitté le serveur ({} en ligne).", b.1.len()));
            }
        }
    }
    m
}

pub fn envoyer_webhook(url: &str, texte: &str) -> Result<(), String> {
    if !url.starts_with("https://discord.com/api/webhooks/") && !url.starts_with("https://discordapp.com/api/webhooks/") {
        return Err("adresse de webhook Discord invalide".into());
    }
    ureq::post(url).set("User-Agent", "Outils").send_json(json!({ "content": texte, "username": "Outils" })).map(|_| ()).map_err(|e| e.to_string())
}

#[derive(Default)]
struct Ipc {
    voulu: Option<(String, String, String)>, // (client_id, détails, état) ; None = effacer / se déconnecter
    etat: String,
}

pub struct Discord {
    r: Reglages,
    ipc: Arc<Mutex<Ipc>>,
    dernier_jeu: Option<String>,
    serveur: Option<(bool, Vec<String>)>,
    message: Arc<Mutex<String>>,
}

impl Discord {
    pub fn new() -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let ipc: Arc<Mutex<Ipc>> = Arc::default();
        let i2 = ipc.clone();
        std::thread::spawn(move || {
            let mut conn: Option<(File, String)> = None; // (canal, client_id)
            let mut envoye: Option<(String, String)> = None;
            let debut = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let voulu = i2.lock().unwrap().voulu.clone();
                let Some((id, details, etat)) = voulu else {
                    if conn.take().is_some() {
                        i2.lock().unwrap().etat = "Déconnecté.".into();
                    }
                    envoye = None;
                    continue;
                };
                if conn.as_ref().is_some_and(|c| c.1 != id) {
                    conn = None; // autre application Discord
                    envoye = None;
                }
                if conn.is_none() {
                    let canal = (0..10).find_map(|n| std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{n}")).ok());
                    let Some(mut f) = canal else {
                        i2.lock().unwrap().etat = "Discord n'est pas ouvert.".into();
                        continue;
                    };
                    let ok = f.write_all(&trame(0, &json!({ "v": 1, "client_id": id }))).is_ok()
                        && matches!(lire_trame(&mut f), Ok((1, v)) if v["evt"] == "READY");
                    if !ok {
                        i2.lock().unwrap().etat = "Discord a refusé l'identifiant d'application.".into();
                        continue;
                    }
                    conn = Some((f, id.clone()));
                    envoye = None;
                }
                let cible = Some((details.clone(), etat.clone()));
                if envoye == cible {
                    continue;
                }
                let mut act = json!({ "details": details, "timestamps": { "start": debut } });
                if !etat.is_empty() {
                    act["state"] = json!(etat);
                }
                let cmd = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": act }, "nonce": format!("{}", SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)) });
                let f = &mut conn.as_mut().unwrap().0;
                match f.write_all(&trame(1, &cmd)).and_then(|_| lire_trame(f)) {
                    Ok((_, v)) if v["evt"] == "ERROR" => {
                        i2.lock().unwrap().etat = format!("Erreur Discord : {}", v["data"]["message"].as_str().unwrap_or("?"));
                        envoye = cible;
                    }
                    Ok(_) => {
                        i2.lock().unwrap().etat = "Statut affiché sur Discord.".into();
                        envoye = cible;
                    }
                    Err(_) => {
                        conn = None; // Discord fermé : on retentera
                        i2.lock().unwrap().etat = "Connexion à Discord perdue, nouvelle tentative…".into();
                    }
                }
            }
        });
        Discord { r, ipc, dernier_jeu: None, serveur: None, message: Arc::default() }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&self.r).unwrap_or_default());
    }

    fn webhook(&self, texte: String) {
        let (url, msg) = (self.r.webhook.clone(), self.message.clone());
        std::thread::spawn(move || {
            if let Err(e) = envoyer_webhook(&url, &texte) {
                *msg.lock().unwrap() = format!("Webhook : {e}");
            }
        });
    }

    /// À chaque image, avec le jeu en cours et l'état du serveur Minecraft.
    pub fn tic(&mut self, jeu: Option<String>, serveur: Option<(bool, Vec<String>)>) {
        if serveur != self.serveur {
            if !self.r.webhook.trim().is_empty() {
                for m in evenements(&self.r, self.serveur.as_ref(), serveur.as_ref()) {
                    self.webhook(m);
                }
            }
            self.serveur = serveur;
        }
        self.dernier_jeu = jeu;
        let id = self.r.client_id.trim();
        let voulu = if self.r.presence && !id.is_empty() {
            activite(&self.r, self.dernier_jeu.as_deref(), self.serveur.as_ref()).map(|(d, e)| (id.to_string(), d, e))
        } else {
            None
        };
        let mut ipc = self.ipc.lock().unwrap();
        if ipc.voulu != voulu {
            ipc.voulu = voulu;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Discord");
        let avant = self.r.clone();
        ui.strong("Statut sur ton profil (Rich Presence)");
        ui.checkbox(&mut self.r.presence, "Afficher mon activité sur Discord");
        ui.horizontal(|ui| {
            ui.label("ID d'application");
            ui.add(egui::TextEdit::singleline(&mut self.r.client_id).hint_text("ex. 1234567890123456789").desired_width(200.0));
            ui.hyperlink_to("Créer une application", "https://discord.com/developers/applications");
        });
        ui.weak("Crée une application (son nom s'affiche en titre : « Outils », « PC de … »), copie son « Application ID » ici. Pas besoin de bot ni de jeton.");
        ui.checkbox(&mut self.r.montrer_jeu, "Montrer le jeu en cours (détecté via le Lanceur)");
        ui.checkbox(&mut self.r.montrer_serveur, "Montrer le serveur Minecraft et le nombre de joueurs");
        ui.horizontal(|ui| {
            ui.label("Texte en plus");
            ui.add(egui::TextEdit::singleline(&mut self.r.texte_libre).hint_text("ex. Dispo pour jouer").desired_width(240.0));
        });
        let (details, etat) = activite(&self.r, self.dernier_jeu.as_deref(), self.serveur.as_ref()).unwrap_or_default();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.weak("Aperçu");
            if details.is_empty() {
                ui.label("(rien à afficher pour l'instant)");
            } else {
                ui.strong(details);
                if !etat.is_empty() {
                    ui.label(etat);
                }
            }
        });
        let e = self.ipc.lock().unwrap().etat.clone();
        if self.r.presence && !e.is_empty() {
            ui.label(e);
        }
        ui.separator();
        ui.strong("Messages dans un salon (webhook)");
        ui.horizontal(|ui| {
            ui.label("URL du webhook");
            ui.add(egui::TextEdit::singleline(&mut self.r.webhook).password(true).hint_text("https://discord.com/api/webhooks/…").desired_width(320.0));
            if ui.add_enabled(!self.r.webhook.trim().is_empty(), egui::Button::new("Tester")).clicked() {
                self.webhook("👋 Test depuis Outils.".into());
                *self.message.lock().unwrap() = "Message de test envoyé.".into();
            }
        });
        ui.weak("Salon Discord → Modifier le salon → Intégrations → Webhooks → Nouveau webhook → Copier l'URL.");
        ui.checkbox(&mut self.r.webhook_serveur, "Prévenir quand le serveur Minecraft s'allume ou s'arrête");
        ui.checkbox(&mut self.r.webhook_joueurs, "Prévenir quand un joueur rejoint ou quitte");
        let m = self.message.lock().unwrap().clone();
        if !m.is_empty() {
            ui.label(m);
        }
        if self.r != avant {
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trames() {
        let t = trame(1, &json!({"a": 1}));
        assert_eq!(&t[..4], &1u32.to_le_bytes());
        assert_eq!(&t[4..8], &7u32.to_le_bytes());
        assert_eq!(&t[8..], br#"{"a":1}"#);
    }

    #[test]
    fn affichage() {
        let r = Reglages::default();
        let srv = (true, vec!["Bob".to_string()]);
        assert_eq!(activite(&r, Some("Elden Ring"), Some(&srv)), Some(("Joue à Elden Ring".into(), "Serveur Minecraft : 1 joueur(s)".into())));
        assert_eq!(activite(&r, None, Some(&srv)), Some(("Serveur Minecraft : 1 joueur(s)".into(), String::new())));
        assert_eq!(activite(&r, None, None), None);
    }

    #[test]
    fn messages_serveur() {
        let r = Reglages::default();
        let a = (true, vec!["Bob".to_string()]);
        let b = (true, vec!["Bob".to_string(), "Alice".to_string()]);
        assert_eq!(evenements(&r, None, Some(&(false, vec![]))), Vec::<String>::new());
        assert_eq!(evenements(&r, Some(&(false, vec![])), Some(&a)).len(), 1);
        assert_eq!(evenements(&r, Some(&a), Some(&b)), vec!["➡ Alice a rejoint le serveur (2 en ligne).".to_string()]);
        assert_eq!(evenements(&r, Some(&b), None), vec!["🔴 Le serveur Minecraft est arrêté.".to_string()]);
    }
}
