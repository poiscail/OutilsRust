//! Détecteur de programmes indésirables : logiciels publicitaires ou « optimiseurs » douteux,
//! mineurs de cryptomonnaie cachés, extensions de navigateur trop curieuses, démarrages suspects.
//! Rien n'est supprimé sans toi : Outils montre, explique, et propose de désinstaller.
use crate::optimiseur::{reg_noms, reg_sous_cles, reg_texte};
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

/// Programmes connus pour afficher des pubs, changer le moteur de recherche, ou faire peur pour vendre.
pub const PUBLICITAIRES: &[&str] = &[
    "ask toolbar", "babylon", "conduit", "mywebsearch", "search protect", "webcompanion", "web companion", "wajam", "segurazo", "bytefence", "reimage", "restoro",
    "pc accelerate", "onesafe", "advanced pc cleaner", "mypc backup", "relevantknowledge", "driver support", "driversupport", "systweak", "pc cleaner pro",
    "speedupmypc", "regclean pro", "pcfixer", "shopping assistant", "yahoo toolbar", "delta toolbar", "sweetpacks", "crossrider", "mindspark",
    "searchbar", "coupon", "savemaster", "pc optimizer pro", "slimcleaner", "winzip driver updater", "driver updater", "auslogics driver", "iobit driver booster",
    "outbyte", "pc helpsoft", "smart pc fixer", "pc app store", "hola vpn", "browsefox", "bonzi", "opencandy", "installcore", "superfish",
];

/// Mineurs de cryptomonnaie (souvent installés en cachette avec un jeu piraté).
pub const MINEURS: &[&str] = &["xmrig", "nicehash", "minergate", "phoenixminer", "t-rex", "trex", "lolminer", "nbminer", "gminer", "ethminer", "cgminer", "bfgminer", "nanominer", "teamredminer", "claymore", "cpuminer", "srbminer"];

/// Permissions d'extension qui permettent de tout voir ou de tout modifier.
const PERMISSIONS: &[(&str, &str)] = &[
    ("<all_urls>", "lit et modifie tous les sites"),
    ("*://*/*", "lit et modifie tous les sites"),
    ("http://*/*", "lit et modifie tous les sites"),
    ("https://*/*", "lit et modifie tous les sites"),
    ("webRequest", "voit tout ce que tu télécharges"),
    ("proxy", "peut faire passer ta connexion ailleurs"),
    ("nativeMessaging", "parle à un programme installé sur le PC"),
    ("history", "lit ton historique"),
    ("cookies", "lit tes cookies (sessions)"),
    ("management", "gère tes autres extensions"),
    ("debugger", "contrôle complet des onglets"),
];

#[derive(Clone)]
pub enum Action {
    Desinstaller(String),
    Dossier(PathBuf),
    Terminer(u32),
    RetirerDemarrage(String, String),
}

#[derive(Clone)]
pub struct Trouvaille {
    pub grave: u8, // 2 = à retirer, 1 = à vérifier
    pub genre: &'static str,
    pub nom: String,
    pub pourquoi: String,
    pub action: Option<Action>,
}

pub fn est_publicitaire(nom: &str) -> bool {
    let n = nom.to_lowercase();
    PUBLICITAIRES.iter().any(|p| n.contains(p))
}

pub fn est_mineur(nom: &str) -> bool {
    let n = nom.to_lowercase();
    let base = n.trim_end_matches(".exe");
    MINEURS.iter().any(|m| base == *m || base.starts_with(&format!("{m}-")) || base.starts_with(&format!("{m}_")) || (m.len() > 5 && base.contains(m)))
}

/// Risques d'une extension d'après son manifest.json.
pub fn risques_extension(manifest: &serde_json::Value) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = vec![];
    let mut perms: Vec<String> = vec![];
    for k in ["permissions", "host_permissions", "optional_permissions"] {
        if let Some(a) = manifest[k].as_array() {
            perms.extend(a.iter().filter_map(|x| x.as_str().map(String::from)));
        }
    }
    if let Some(cs) = manifest["content_scripts"].as_array() {
        for c in cs {
            if let Some(m) = c["matches"].as_array() {
                perms.extend(m.iter().filter_map(|x| x.as_str().map(String::from)));
            }
        }
    }
    for (p, t) in PERMISSIONS {
        if perms.iter().any(|x| x == p) && !v.contains(t) {
            v.push(t);
        }
    }
    v
}

/// Un exe qui démarre depuis un dossier temporaire ou au nom aléatoire est suspect.
pub fn demarrage_suspect(commande: &str) -> Option<&'static str> {
    let c = commande.to_lowercase();
    if c.contains("\\temp\\") || c.contains("\\tmp\\") {
        return Some("se lance depuis un dossier temporaire");
    }
    if c.contains("\\appdata\\roaming\\") && c.contains(".exe") {
        let exe = c.split(".exe").next().unwrap_or("").rsplit('\\').next().unwrap_or("");
        let chiffres = exe.chars().filter(|x| x.is_ascii_digit()).count();
        if exe.len() >= 8 && (chiffres * 3 >= exe.len() || !exe.chars().any(|x| "aeiouy".contains(x))) {
            return Some("nom de programme aléatoire, caché dans AppData");
        }
    }
    if c.contains("powershell") && (c.contains("-enc") || c.contains("hidden")) {
        return Some("commande PowerShell cachée");
    }
    None
}

fn programmes() -> Vec<Trouvaille> {
    let mut v = vec![];
    for base in [r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall"] {
        for k in reg_sous_cles(base) {
            let p = format!(r"{base}\{k}");
            let Some(nom) = reg_texte(&p, "DisplayName") else { continue };
            let editeur = reg_texte(&p, "Publisher").unwrap_or_default();
            if est_publicitaire(&nom) || est_publicitaire(&editeur) {
                v.push(Trouvaille {
                    grave: 2,
                    genre: "Programme",
                    nom: nom.clone(),
                    pourquoi: "Connu pour afficher des pubs, changer ta page d'accueil, ou faire peur pour vendre un « nettoyage ». Il n'est pas utile.".into(),
                    action: reg_texte(&p, "UninstallString").map(Action::Desinstaller),
                });
            } else if est_mineur(&nom) {
                v.push(Trouvaille { grave: 2, genre: "Mineur", nom, pourquoi: "Programme de minage de cryptomonnaie : si tu ne l'as pas installé toi-même, il utilise ton PC pour quelqu'un d'autre.".into(), action: reg_texte(&p, "UninstallString").map(Action::Desinstaller) });
            }
        }
    }
    v
}

fn processus() -> Vec<Trouvaille> {
    let mut v = vec![];
    let Ok(o) = std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"]).creation_flags(0x0800_0000).output() else { return v };
    for l in String::from_utf8_lossy(&o.stdout).lines() {
        let c: Vec<&str> = l.split("\",\"").map(|x| x.trim_matches('"')).collect();
        if c.len() < 2 {
            continue;
        }
        if est_mineur(c[0]) {
            let pid = c[1].parse().unwrap_or(0);
            let chemin = crate::lanceur::chemin_processus(pid).unwrap_or_default();
            v.push(Trouvaille { grave: 2, genre: "Mineur en marche", nom: format!("{} ({chemin})", c[0]), pourquoi: "Un programme de minage tourne en ce moment : il fait chauffer le PC et ralentit les jeux.".into(), action: Some(Action::Terminer(pid)) });
        }
    }
    v
}

fn demarrages() -> Vec<Trouvaille> {
    let mut v = vec![];
    for base in [r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run", r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"] {
        for n in reg_noms(base) {
            let Some(c) = reg_texte(base, &n) else { continue };
            if let Some(p) = demarrage_suspect(&c) {
                v.push(Trouvaille { grave: 1, genre: "Démarrage", nom: format!("{n} : {c}"), pourquoi: format!("Lancé à chaque démarrage, et {p}. À vérifier (et à analyser avec l'antivirus)."), action: Some(Action::RetirerDemarrage(base.to_string(), n.clone())) });
            } else if est_publicitaire(&c) || est_mineur(Path::new(c.trim_matches('"').split(".exe").next().unwrap_or("")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default().as_str()) {
                v.push(Trouvaille { grave: 2, genre: "Démarrage", nom: format!("{n} : {c}"), pourquoi: "Un programme indésirable se lance à chaque démarrage.".into(), action: Some(Action::RetirerDemarrage(base.to_string(), n.clone())) });
            }
        }
    }
    v
}

fn extensions() -> Vec<Trouvaille> {
    let mut v = vec![];
    let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
    let navigateurs = [("Chrome", local.join(r"Google\Chrome\User Data")), ("Edge", local.join(r"Microsoft\Edge\User Data")), ("Brave", local.join(r"BraveSoftware\Brave-Browser\User Data")), ("Opera", PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(r"Opera Software\Opera Stable"))];
    for (nav, racine) in navigateurs {
        let mut profils: Vec<PathBuf> = vec![racine.join("Default"), racine.clone()];
        for e in std::fs::read_dir(&racine).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("Profile ") {
                profils.push(e.path());
            }
        }
        for p in profils {
            for ext in std::fs::read_dir(p.join("Extensions")).into_iter().flatten().flatten() {
                let Some(version) = std::fs::read_dir(ext.path()).into_iter().flatten().flatten().map(|e| e.path()).find(|d| d.join("manifest.json").is_file()) else { continue };
                let Ok(m) = std::fs::read_to_string(version.join("manifest.json")).map(|t| serde_json::from_str::<serde_json::Value>(&t).unwrap_or_default()) else { continue };
                let mut nom = m["name"].as_str().unwrap_or("?").to_string();
                if nom.starts_with("__MSG_") {
                    // nom traduit : messages.json de la langue par défaut
                    let langue = m["default_locale"].as_str().unwrap_or("en");
                    let cle = nom.trim_start_matches("__MSG_").trim_end_matches("__").to_lowercase();
                    if let Ok(t) = std::fs::read_to_string(version.join("_locales").join(langue).join("messages.json")) {
                        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) {
                            if let Some(o) = j.as_object() {
                                if let Some((_, x)) = o.iter().find(|(k, _)| k.to_lowercase() == cle) {
                                    nom = x["message"].as_str().unwrap_or(&nom).to_string();
                                }
                            }
                        }
                    }
                }
                let r = risques_extension(&m);
                let pub_ = est_publicitaire(&nom);
                if r.len() >= 2 || pub_ {
                    v.push(Trouvaille {
                        grave: if pub_ { 2 } else { 1 },
                        genre: "Extension",
                        nom: format!("{nom} ({nav})"),
                        pourquoi: if pub_ { "Extension publicitaire connue.".into() } else { format!("Elle {}. Garde-la seulement si tu l'as installée exprès et que tu lui fais confiance.", r.join(", ")) },
                        action: Some(Action::Dossier(ext.path())),
                    });
                }
            }
        }
    }
    v
}

pub fn analyser() -> Vec<Trouvaille> {
    let mut v = programmes();
    v.extend(processus());
    v.extend(demarrages());
    v.extend(extensions());
    v.sort_by(|a, b| b.grave.cmp(&a.grave));
    v
}

pub struct Indesirables {
    l: Arc<Mutex<Option<Vec<Trouvaille>>>>,
    en_cours: Arc<AtomicBool>,
    message: String,
}

impl Indesirables {
    pub fn new() -> Self {
        Indesirables { l: Arc::default(), en_cours: Arc::default(), message: String::new() }
    }

    fn lancer(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (l, e) = (self.l.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            *l.lock().unwrap() = Some(analyser());
            e.store(false, Relaxed);
        });
    }
}

impl Outil for Indesirables {
    fn nom(&self) -> &'static str {
        "🦠 Programmes indésirables"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programmes indésirables");
        ui.weak("Cherche les logiciels publicitaires, les faux « nettoyeurs », les mineurs de cryptomonnaie cachés, les extensions de navigateur trop curieuses et les démarrages suspects. Rien n'est supprimé sans toi.");
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.en_cours.load(Relaxed), egui::Button::new("🔍 Analyser")).clicked() {
                self.lancer();
            }
            if self.en_cours.load(Relaxed) {
                ui.spinner();
                ui.label("Analyse…");
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
            }
            ui.label(&self.message);
        });
        let l = self.l.lock().unwrap().clone();
        let Some(l) = l else {
            ui.weak("Lance l'analyse (quelques secondes).");
            return;
        };
        ui.separator();
        if l.is_empty() {
            ui.label(RichText::new("✔ Rien d'indésirable trouvé.").color(Color32::LIGHT_GREEN).size(16.0));
            ui.weak("Pour une vérification complète, lance aussi une analyse antivirus (Système › Analyse antivirus).");
            return;
        }
        let mut refaire = false;
        egui::ScrollArea::vertical().id_salt("indesirables").show(ui, |ui| {
            for t in &l {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(if t.grave == 2 { "❗" } else { "⚠" }).color(if t.grave == 2 { Color32::LIGHT_RED } else { Color32::from_rgb(255, 200, 100) }));
                        ui.weak(t.genre);
                        ui.strong(&t.nom);
                    });
                    ui.label(&t.pourquoi);
                    match &t.action {
                        Some(Action::Desinstaller(c)) => {
                            if ui.button("🧹 Désinstaller").clicked() {
                                let _ = std::process::Command::new("cmd").args(["/C", c]).creation_flags(0x0800_0000).spawn();
                                self.message = "Le désinstalleur s'ouvre : suis ses étapes (refuse les « offres » qu'il propose).".into();
                            }
                        }
                        Some(Action::Dossier(d)) => {
                            if ui.button("📂 Voir le dossier").on_hover_text("Pour la retirer : menu du navigateur › Extensions").clicked() {
                                let _ = std::process::Command::new("explorer").arg(d).spawn();
                            }
                        }
                        Some(Action::Terminer(pid)) => {
                            if ui.button("⛔ Arrêter ce programme").clicked() {
                                let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).creation_flags(0x0800_0000).status();
                                self.message = "Arrêté. Pense à le désinstaller et à lancer une analyse antivirus.".into();
                                refaire = true;
                            }
                        }
                        Some(Action::RetirerDemarrage(cle, nom)) => {
                            if ui.button("🚫 Ne plus lancer au démarrage").clicked() {
                                let r = crate::optimiseur::ps(&format!("Remove-ItemProperty -Path 'Registry::{}' -Name '{}'", cle.replace("HKCU", "HKEY_CURRENT_USER").replace("HKLM", "HKEY_LOCAL_MACHINE"), nom.replace('\'', "''")));
                                self.message = match r {
                                    Ok(_) => "✔ Retiré du démarrage.".into(),
                                    Err(e) => format!("✖ {e} (Outils en administrateur ?)"),
                                };
                                refaire = true;
                            }
                        }
                        None => {}
                    }
                });
            }
        });
        if refaire {
            self.lancer();
        }
    }
    fn resume(&self) -> Option<String> {
        let g = self.l.lock().ok()?;
        let n = g.as_ref()?.iter().filter(|t| t.grave == 2).count();
        (n > 0).then(|| format!("{n} programme(s) indésirable(s)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reperes() {
        assert!(est_publicitaire("Web Companion 7.1"));
        assert!(est_publicitaire("Driver Support One"));
        assert!(!est_publicitaire("Steam"));
        assert!(!est_publicitaire("Discord"));
        assert!(est_mineur("xmrig.exe"));
        assert!(est_mineur("PhoenixMiner_6.2"));
        assert!(!est_mineur("explorer.exe"));
        assert!(!est_mineur("trexchat.exe"));
        let m: serde_json::Value = serde_json::from_str(r#"{"name":"X","permissions":["tabs","webRequest","cookies"],"host_permissions":["<all_urls>"]}"#).unwrap();
        assert_eq!(risques_extension(&m).len(), 3);
        let sage: serde_json::Value = serde_json::from_str(r#"{"name":"Y","permissions":["storage"]}"#).unwrap();
        assert!(risques_extension(&sage).is_empty());
        assert!(demarrage_suspect(r"C:\Users\a\AppData\Local\Temp\x.exe").is_some());
        assert!(demarrage_suspect(r#""C:\Users\a\AppData\Roaming\xq7z9k2m4\b8x7z9k2.exe""#).is_some());
        assert!(demarrage_suspect(r#""C:\Program Files\Steam\steam.exe" -silent"#).is_none());
        assert!(demarrage_suspect(r#""C:\Users\a\AppData\Roaming\Spotify\Spotify.exe""#).is_none());
        assert!(demarrage_suspect("powershell -w hidden -enc AAAA").is_some());
    }
}
