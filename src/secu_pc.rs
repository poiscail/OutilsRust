//! Protéger le PC : pare-feu simplifié, surveillance des réglages sensibles (hosts, démarrage, proxy,
//! DNS, services), pot de miel qui repère les scans du réseau, et audit de durcissement de Windows.
use crate::optimiseur::{ps, reg_noms, reg_sous_cles, reg_texte};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(prog).args(args).creation_flags(0x0800_0000).output().map_err(|e| e.to_string())?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    if o.status.success() { Ok(s) } else { Err(if s.trim().is_empty() { "échec (droits administrateur ?)".into() } else { s.trim().lines().last().unwrap_or("").to_string() }) }
}

// ===================================================================================================
// 22 · Pare-feu simplifié
// ===================================================================================================

/// État des profils du pare-feu (sortie de « netsh advfirewall show allprofiles state »).
pub fn profils_parefeu(s: &str) -> Vec<(String, bool)> {
    let mut v = vec![];
    let mut nom = String::new();
    for l in s.lines() {
        let t = l.trim();
        if t.ends_with(':') && t.contains(' ') && !t.contains("---") {
            nom = t.trim_end_matches(':').to_string();
        } else if let Some(x) = t.strip_prefix("State").or_else(|| t.strip_prefix("État")).or_else(|| t.strip_prefix("Etat")) {
            let x = x.trim().to_uppercase();
            v.push((nom.clone(), x.starts_with("ON") || x.starts_with("ACTIF") || x.starts_with("ACTIV")));
        }
    }
    v
}

pub struct ParefeuSimple {
    ports: Vec<(String, u16, String)>,
    profils: Vec<(String, bool)>,
    message: String,
    charge: bool,
}

impl ParefeuSimple {
    pub fn new() -> Self {
        ParefeuSimple { ports: vec![], profils: vec![], message: String::new(), charge: false }
    }

    fn actualiser(&mut self) {
        self.ports = crate::reseau2::ports_ecoute().into_iter().filter(|p| p.0.contains("réseau")).collect();
        self.profils = profils_parefeu(&cmd("netsh", &["advfirewall", "show", "allprofiles", "state"]).unwrap_or_default());
        self.charge = true;
    }
}

impl Outil for ParefeuSimple {
    fn nom(&self) -> &'static str {
        "🧱 Pare-feu simplifié"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.charge {
            self.actualiser();
        }
        ui.heading("Pare-feu simplifié");
        ui.weak("Ce qui écoute sur ton PC et peut être joint par les autres appareils du réseau, et le bouton pour le bloquer. (Pour couper internet à une appli dans l'autre sens : Système › Couper internet à une appli.)");
        ui.horizontal_wrapped(|ui| {
            for (n, on) in &self.profils {
                ui.colored_label(if *on { Color32::LIGHT_GREEN } else { Color32::LIGHT_RED }, format!("{} {n}", if *on { "🛡" } else { "⚠ désactivé :" }));
            }
            if self.profils.iter().any(|p| !p.1) && ui.button("Réactiver le pare-feu").clicked() {
                self.message = match cmd("netsh", &["advfirewall", "set", "allprofiles", "state", "on"]) {
                    Ok(_) => "✔ Pare-feu réactivé".into(),
                    Err(e) => format!("✖ {e}"),
                };
                self.actualiser();
            }
            if ui.button("⟳").clicked() {
                self.actualiser();
            }
        });
        ui.label(&self.message);
        ui.separator();
        let mut bloquer: Option<String> = None;
        egui::Grid::new("parefeu").striped(true).num_columns(4).show(ui, |ui| {
            ui.strong("Port");
            ui.strong("Programme");
            ui.strong("À quoi ça sert");
            ui.label("");
            ui.end_row();
            for (p, port, prog) in &self.ports {
                ui.label(format!("{p} {port}"));
                ui.label(prog);
                let (n, risque, _) = crate::secu_reseau::infos_port(*port);
                let connu = crate::reseau2::port_connu(*port);
                ui.colored_label(if risque >= 2 { Color32::from_rgb(255, 170, 80) } else { ui.visuals().text_color() }, if connu.is_empty() { n } else { connu });
                if !prog.starts_with("PID") && prog != "System" && prog != "svchost.exe" && ui.small_button("🚫 Bloquer l'entrée").on_hover_text("Les autres appareils ne pourront plus le joindre").clicked() {
                    bloquer = Some(prog.clone());
                }
                ui.end_row();
            }
        });
        if let Some(prog) = bloquer {
            let script = format!("$p = (Get-Process -Name '{}' -ErrorAction Stop | Select-Object -First 1).Path; New-NetFirewallRule -DisplayName 'Outils - bloquer {}' -Direction Inbound -Action Block -Program $p | Out-Null", prog.trim_end_matches(".exe").replace('\'', "''"), prog.replace('\'', "''"));
            self.message = match ps(&script) {
                Ok(_) => format!("✔ {prog} ne peut plus être joint depuis le réseau"),
                Err(e) => format!("✖ {e} (lance Outils en administrateur)"),
            };
        }
        ui.separator();
        if ui.button("Retirer les blocages créés par Outils").clicked() {
            self.message = match ps("Get-NetFirewallRule -DisplayName 'Outils - bloquer*' | Remove-NetFirewallRule") {
                Ok(_) => "✔ Blocages retirés".into(),
                Err(e) => format!("✖ {e}"),
            };
        }
    }
}

// ===================================================================================================
// 23 · Surveillance des changements
// ===================================================================================================

/// Photo des réglages sensibles : nom → valeur.
pub fn photo_systeme() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let hosts = std::fs::read_to_string(r"C:\Windows\System32\drivers\etc\hosts").unwrap_or_default();
    let lignes: Vec<&str> = hosts.lines().map(|l| l.trim()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
    m.insert("Fichier hosts".into(), lignes.join(" | "));
    for base in [r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run", r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run", r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce", r"HKLM\Software\Microsoft\Windows\CurrentVersion\RunOnce"] {
        for n in reg_noms(base) {
            m.insert(format!("Démarrage : {n} ({})", if base.starts_with("HKCU") { "utilisateur" } else { "tous" }), reg_texte(base, &n).unwrap_or_default());
        }
    }
    for d in [std::env::var("APPDATA").map(|a| format!(r"{a}\Microsoft\Windows\Start Menu\Programs\Startup")).unwrap_or_default(), r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\StartUp".to_string()] {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n != "desktop.ini" {
                m.insert(format!("Dossier Démarrage : {n}"), d.clone());
            }
        }
    }
    let is = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let proxy = crate::optimiseur::reg_dword(is, "ProxyEnable").unwrap_or(0);
    m.insert("Proxy".into(), if proxy == 1 { reg_texte(is, "ProxyServer").unwrap_or_else(|| "activé".into()) } else { "aucun".into() });
    m.insert("Script de proxy (PAC)".into(), reg_texte(is, "AutoConfigURL").unwrap_or_default());
    let wl = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon";
    m.insert("Winlogon Shell".into(), reg_texte(wl, "Shell").unwrap_or_default());
    m.insert("Winlogon Userinit".into(), reg_texte(wl, "Userinit").unwrap_or_default());
    m.insert("DNS".into(), crate::secu_reseau::serveurs_dns().join(", "));
    let services = reg_sous_cles(r"HKLM\SYSTEM\CurrentControlSet\Services");
    for s in services {
        let p = format!(r"HKLM\SYSTEM\CurrentControlSet\Services\{s}");
        if let Some(img) = reg_texte(&p, "ImagePath") {
            m.insert(format!("Service : {s}"), img);
        }
    }
    m
}

/// Différences entre deux photos : (clé, avant, après).
pub fn differences(avant: &BTreeMap<String, String>, apres: &BTreeMap<String, String>) -> Vec<(String, Option<String>, Option<String>)> {
    let mut v = vec![];
    for (k, a) in apres {
        match avant.get(k) {
            Some(b) if b == a => {}
            b => v.push((k.clone(), b.cloned(), Some(a.clone()))),
        }
    }
    for (k, b) in avant {
        if !apres.contains_key(k) {
            v.push((k.clone(), Some(b.clone()), None));
        }
    }
    v
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Veille {
    actif: bool,
    reference: BTreeMap<String, String>,
    journal: Vec<(String, String, String, String)>, // heure, clé, avant, après
    hosts_sauve: String,
}

pub struct Surveillance {
    v: Arc<Mutex<Veille>>,
    lance: bool,
    message: String,
}

impl Surveillance {
    pub fn new() -> Self {
        Surveillance { v: Arc::new(Mutex::new(charger("surveillance_systeme"))), lance: false, message: String::new() }
    }
}

fn tour_surveillance(v: &Arc<Mutex<Veille>>) {
    let photo = photo_systeme();
    let mut g = v.lock().unwrap();
    if g.reference.is_empty() {
        g.reference = photo;
        g.hosts_sauve = std::fs::read_to_string(r"C:\Windows\System32\drivers\etc\hosts").unwrap_or_default();
        enregistrer("surveillance_systeme", &*g);
        return;
    }
    let d = differences(&g.reference, &photo);
    if d.is_empty() {
        return;
    }
    let h = crate::minecraft::date_lisible();
    for (k, a, b) in &d {
        let genre = if b.is_none() { "retiré" } else if a.is_none() { "ajouté" } else { "modifié" };
        crate::systeme::notifier_toujours(&format!("Réglage sensible {genre}"), &format!("{k} : {}", b.clone().or(a.clone()).unwrap_or_default().chars().take(120).collect::<String>()));
        g.journal.insert(0, (h.clone(), k.clone(), a.clone().unwrap_or_default(), b.clone().unwrap_or_default()));
    }
    g.journal.truncate(200);
    g.reference = photo;
    enregistrer("surveillance_systeme", &*g);
}

static SURV_LANCEE: AtomicBool = AtomicBool::new(false);

impl Outil for Surveillance {
    fn nom(&self) -> &'static str {
        "👁 Surveillance des changements"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.lance || !self.v.lock().unwrap().actif {
            return;
        }
        self.lance = true;
        if SURV_LANCEE.swap(true, Relaxed) {
            return;
        }
        let v = self.v.clone();
        std::thread::spawn(move || loop {
            if v.lock().unwrap().actif && !crate::outil::DISCRET.load(Relaxed) {
                tour_surveillance(&v);
            }
            std::thread::sleep(Duration::from_secs(120));
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Surveillance des changements");
        ui.weak("Les virus et logiciels douteux modifient presque toujours les mêmes endroits : fichier hosts (faux sites), programmes au démarrage, proxy (espionner la navigation), DNS, services. Outils les surveille toutes les 2 minutes et te prévient du moindre changement.");
        let mut g = self.v.lock().unwrap();
        if ui.checkbox(&mut g.actif, RichText::new("Surveiller en permanence").strong()).changed() {
            enregistrer("surveillance_systeme", &*g);
            self.lance = false;
        }
        ui.label(format!("{} réglages surveillés", g.reference.len()));
        drop(g);
        ui.horizontal(|ui| {
            if ui.button("🔎 Vérifier maintenant").clicked() {
                let v = self.v.clone();
                std::thread::spawn(move || tour_surveillance(&v));
            }
            if ui.button("Reprendre la photo de référence").on_hover_text("Après avoir installé un programme en connaissance de cause").clicked() {
                let mut g = self.v.lock().unwrap();
                g.reference.clear();
                drop(g);
                let v = self.v.clone();
                std::thread::spawn(move || tour_surveillance(&v));
            }
        });
        ui.label(&self.message);
        ui.separator();
        ui.strong("Changements repérés");
        let j = self.v.lock().unwrap().journal.clone();
        if j.is_empty() {
            ui.weak("Aucun changement depuis le début de la surveillance.");
        }
        let mut annuler: Option<(String, String)> = None;
        egui::ScrollArea::vertical().id_salt("surv").show(ui, |ui| {
            for (h, k, a, b) in j.iter().take(60) {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.weak(h);
                        ui.strong(k);
                    });
                    if !a.is_empty() {
                        ui.label(format!("avant : {}", a.chars().take(200).collect::<String>()));
                    }
                    if !b.is_empty() {
                        ui.label(format!("après : {}", b.chars().take(200).collect::<String>()));
                    }
                    if k.starts_with("Démarrage : ") && !b.is_empty() && a.is_empty() && ui.small_button("🚫 Retirer ce démarrage").clicked() {
                        annuler = Some((k.clone(), String::new()));
                    }
                    if k == "Fichier hosts" && ui.small_button("↩ Remettre le fichier hosts d'avant").clicked() {
                        annuler = Some((k.clone(), String::new()));
                    }
                });
            }
        });
        if let Some((k, _)) = annuler {
            if k == "Fichier hosts" {
                let s = self.v.lock().unwrap().hosts_sauve.clone();
                self.message = match std::fs::write(r"C:\Windows\System32\drivers\etc\hosts", s) {
                    Ok(()) => "✔ Fichier hosts remis".into(),
                    Err(e) => format!("✖ {e} (Outils en administrateur ?)"),
                };
            } else if let Some(nom) = k.strip_prefix("Démarrage : ").and_then(|x| x.rsplit_once(" (")).map(|x| x.0.to_string()) {
                let cle = if k.ends_with("(utilisateur)") { r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Run" } else { r"HKLM:\Software\Microsoft\Windows\CurrentVersion\Run" };
                self.message = match ps(&format!("Remove-ItemProperty -Path '{cle}' -Name '{}'", nom.replace('\'', "''"))) {
                    Ok(_) => "✔ Retiré du démarrage".into(),
                    Err(e) => format!("✖ {e}"),
                };
            }
        }
    }
}

// ===================================================================================================
// 24 · Pot de miel
// ===================================================================================================

/// (port, nom, message d'accueil imité)
const PIEGES: &[(u16, &str, &str)] = &[
    (21, "FTP", "220 FTP server ready\r\n"),
    (23, "Telnet", "\r\nlogin: "),
    (2323, "Telnet (objets)", "\r\nlogin: "),
    (8081, "Web d'administration", "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"admin\"\r\n\r\n"),
    (5900, "VNC", "RFB 003.008\n"),
    (1433, "Base de données SQL", ""),
];

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Miel {
    actif: bool,
    ports: Vec<u16>,
    journal: Vec<(String, String, u16, String)>, // heure, ip, port, ce qui a été envoyé
}

pub struct PotDeMiel {
    m: Arc<Mutex<Miel>>,
    etat: Arc<Mutex<Vec<String>>>,
    lance: bool,
}

static MIEL_LANCE: AtomicBool = AtomicBool::new(false);

fn lancer_pieges(m: Arc<Mutex<Miel>>, etat: Arc<Mutex<Vec<String>>>) {
    let ports = m.lock().unwrap().ports.clone();
    let moi = crate::telephone::ip_locale().unwrap_or_default();
    for p in ports {
        let Some((_, nom, accueil)) = PIEGES.iter().find(|x| x.0 == p).copied() else { continue };
        let ecoute = match TcpListener::bind(("0.0.0.0", p)) {
            Ok(l) => l,
            Err(e) => {
                etat.lock().unwrap().push(format!("Port {p} : déjà utilisé ({e})"));
                continue;
            }
        };
        etat.lock().unwrap().push(format!("Port {p} ({nom}) : piège en place"));
        let (m2, moi2) = (m.clone(), moi.clone());
        std::thread::spawn(move || {
            for c in ecoute.incoming().flatten() {
                if !m2.lock().unwrap().actif {
                    break;
                }
                let ip = c.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                let mut c = c;
                let _ = c.set_read_timeout(Some(Duration::from_secs(3)));
                let _ = c.write_all(accueil.as_bytes());
                let mut b = [0u8; 256];
                let n = c.read(&mut b).unwrap_or(0);
                let recu: String = String::from_utf8_lossy(&b[..n]).chars().map(|x| if x.is_control() { ' ' } else { x }).collect();
                let local = ip == "127.0.0.1" || ip == moi2;
                let mut g = m2.lock().unwrap();
                g.journal.insert(0, (crate::minecraft::date_lisible(), ip.clone(), p, recu.trim().to_string()));
                g.journal.truncate(300);
                enregistrer("pot_de_miel", &*g);
                drop(g);
                if !local {
                    crate::systeme::notifier_toujours("🍯 Quelqu'un fouille ton réseau", &format!("L'appareil {ip} a tenté de se connecter au faux service {nom} (port {p}). Un appareil ne fait pas ça normalement : virus, intrus ou scanner."));
                }
            }
        });
    }
}

impl PotDeMiel {
    pub fn new() -> Self {
        let mut m: Miel = charger("pot_de_miel");
        if m.ports.is_empty() {
            m.ports = vec![23, 2323, 8081];
        }
        PotDeMiel { m: Arc::new(Mutex::new(m)), etat: Arc::default(), lance: false }
    }
}

impl Outil for PotDeMiel {
    fn nom(&self) -> &'static str {
        "🍯 Pot de miel"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.lance || !self.m.lock().unwrap().actif {
            return;
        }
        self.lance = true;
        if !MIEL_LANCE.swap(true, Relaxed) {
            lancer_pieges(self.m.clone(), self.etat.clone());
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pot de miel");
        ui.weak("Outils ouvre de faux services (Telnet, FTP, page d'administration…) qui ne servent à rien. Personne chez toi n'a de raison d'y toucher : si un appareil s'y connecte, c'est qu'il fouille le réseau (virus, objet piraté, intrus). Tu es prévenu tout de suite.");
        let mut g = self.m.lock().unwrap();
        let avant = g.actif;
        ui.checkbox(&mut g.actif, RichText::new("Poser les pièges").strong());
        ui.horizontal_wrapped(|ui| {
            for (p, n, _) in PIEGES {
                let mut c = g.ports.contains(p);
                if ui.checkbox(&mut c, format!("{n} ({p})")).changed() {
                    if c {
                        g.ports.push(*p);
                    } else {
                        g.ports.retain(|x| x != p);
                    }
                }
            }
        });
        if g.actif != avant {
            enregistrer("pot_de_miel", &*g);
            if g.actif {
                self.lance = false;
            } else {
                MIEL_LANCE.store(false, Relaxed);
            }
        }
        let j = g.journal.clone();
        drop(g);
        ui.weak("Windows peut demander d'autoriser Outils dans le pare-feu : accepte pour les réseaux privés, sinon les pièges ne voient rien. Les changements de ports s'appliquent au prochain démarrage d'Outils.");
        for e in self.etat.lock().unwrap().iter() {
            ui.label(e);
        }
        ui.separator();
        ui.strong(format!("Tentatives ({})", j.len()));
        if j.is_empty() {
            ui.weak("Personne n'est tombé dans le piège : bon signe.");
        }
        for (h, ip, p, r) in j.iter().take(50) {
            ui.label(format!("{h} · {ip} → port {p}{}", if r.is_empty() { String::new() } else { format!(" · a envoyé : {}", r.chars().take(80).collect::<String>()) }));
        }
        if !j.is_empty() && ui.small_button("Vider").clicked() {
            let mut g = self.m.lock().unwrap();
            g.journal.clear();
            enregistrer("pot_de_miel", &*g);
        }
    }
    fn resume(&self) -> Option<String> {
        let g = self.m.lock().ok()?;
        (g.actif && !g.journal.is_empty()).then(|| format!("{} tentative(s) repérée(s)", g.journal.len()))
    }
}

// ===================================================================================================
// 25 · Durcissement de Windows
// ===================================================================================================

const SCRIPT_AUDIT: &str = r#"
$r = @{}
try { $r.bitlocker = (Get-BitLockerVolume -MountPoint $env:SystemDrive -ErrorAction Stop).ProtectionStatus.ToString() } catch { $r.bitlocker = 'inconnu' }
try { $r.secureboot = (Confirm-SecureBootUEFI -ErrorAction Stop).ToString() } catch { $r.secureboot = 'inconnu' }
try { $m = Get-MpComputerStatus -ErrorAction Stop; $r.defender = $m.AntivirusEnabled.ToString(); $r.temps_reel = $m.RealTimeProtectionEnabled.ToString(); $r.signatures = $m.AntivirusSignatureAge.ToString() } catch { $r.defender = 'inconnu' }
try { $r.parefeu = ((Get-NetFirewallProfile -ErrorAction Stop | Where-Object { -not $_.Enabled }).Count -eq 0).ToString() } catch { $r.parefeu = 'inconnu' }
try { $r.smb1 = (Get-SmbServerConfiguration -ErrorAction Stop).EnableSMB1Protocol.ToString() } catch { $r.smb1 = 'inconnu' }
try { $h = Get-HotFix -ErrorAction Stop | Where-Object { $_.InstalledOn } | Sort-Object InstalledOn -Descending | Select-Object -First 1; $r.maj = [int]((Get-Date) - $h.InstalledOn).TotalDays } catch { $r.maj = 'inconnu' }
try { $r.admins = (Get-LocalGroupMember -SID 'S-1-5-32-544' -ErrorAction Stop | Measure-Object).Count } catch { $r.admins = 'inconnu' }
try { $r.invite = (Get-LocalUser | Where-Object { $_.SID -like '*-501' }).Enabled.ToString() } catch { $r.invite = 'inconnu' }
try { $r.moi_admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator).ToString() } catch { $r.moi_admin = 'inconnu' }
$r.GetEnumerator() | ForEach-Object { $_.Key + '=' + $_.Value }
"#;

#[derive(Clone)]
pub struct Controle {
    pub nom: &'static str,
    pub ok: Option<bool>,
    pub detail: String,
    pub poids: u32,
    pub reparer: Option<&'static str>,
}

/// Contrôles d'après les réponses de PowerShell et du registre.
pub fn controles(r: &BTreeMap<String, String>, uac: Option<u32>, rdp: Option<u32>, hvci: Option<u32>, autorun: Option<u32>) -> Vec<Controle> {
    let v = |k: &str| r.get(k).map(|s| s.trim().to_string()).unwrap_or_else(|| "inconnu".into());
    let vrai = |k: &str| match v(k).to_lowercase().as_str() {
        "true" | "on" | "1" => Some(true),
        "false" | "off" | "0" => Some(false),
        _ => None,
    };
    let mut c = vec![];
    c.push(Controle { nom: "Antivirus actif", ok: vrai("defender"), detail: "Windows Defender (ou un autre antivirus) doit être actif.".into(), poids: 15, reparer: Some("windowsdefender:") });
    c.push(Controle { nom: "Protection en temps réel", ok: vrai("temps_reel"), detail: "Analyse les fichiers dès qu'ils arrivent.".into(), poids: 10, reparer: Some("windowsdefender:") });
    let sig: Option<u32> = v("signatures").parse().ok();
    c.push(Controle { nom: "Antivirus à jour", ok: sig.map(|d| d <= 3), detail: sig.map(|d| format!("dernière mise à jour il y a {d} jour(s)")).unwrap_or_default(), poids: 5, reparer: Some("windowsdefender:") });
    c.push(Controle { nom: "Pare-feu activé partout", ok: vrai("parefeu"), detail: "Bloque les connexions entrantes non voulues.".into(), poids: 10, reparer: Some("parefeu") });
    let maj: Option<u32> = v("maj").parse().ok();
    c.push(Controle { nom: "Mises à jour de Windows récentes", ok: maj.map(|d| d <= 45), detail: maj.map(|d| format!("dernière mise à jour il y a {d} jours")).unwrap_or_default(), poids: 15, reparer: Some("ms-settings:windowsupdate") });
    c.push(Controle { nom: "SMBv1 désactivé", ok: vrai("smb1").map(|x| !x), detail: "Vieux protocole de partage utilisé par le virus WannaCry.".into(), poids: 10, reparer: Some("smb1") });
    c.push(Controle { nom: "Contrôle de compte (UAC) actif", ok: uac.map(|x| x == 1), detail: "Demande une confirmation avant qu'un programme modifie le système.".into(), poids: 10, reparer: Some("uac") });
    c.push(Controle { nom: "Bureau à distance fermé", ok: rdp.map(|x| x == 1), detail: "Prise en main du PC depuis le réseau : à couper si tu ne t'en sers pas.".into(), poids: 5, reparer: Some("rdp") });
    c.push(Controle { nom: "Compte Invité désactivé", ok: vrai("invite").map(|x| !x), detail: "Un compte sans mot de passe ouvert à tous.".into(), poids: 5, reparer: Some("invite") });
    c.push(Controle { nom: "Disque chiffré (BitLocker)", ok: match v("bitlocker").as_str() { "On" => Some(true), "Off" => Some(false), _ => None }, detail: "Si le PC est volé, personne ne peut lire le disque. (Pas disponible sur Windows Famille : utilise le « chiffrement de l'appareil » s'il est proposé.)".into(), poids: 5, reparer: Some("ms-settings:deviceencryption") });
    c.push(Controle { nom: "Démarrage sécurisé (Secure Boot)", ok: vrai("secureboot"), detail: "Empêche les virus de se lancer avant Windows. Se règle dans le BIOS.".into(), poids: 5, reparer: None });
    c.push(Controle { nom: "Isolation du noyau (intégrité de la mémoire)", ok: hvci.map(|x| x == 1), detail: "Protège le cœur de Windows contre les pilotes malveillants (peut gêner quelques anti-triche anciens).".into(), poids: 3, reparer: Some("windowsdefender://coreisolation") });
    c.push(Controle { nom: "Exécution automatique des clés USB coupée", ok: autorun.map(|x| x == 255 || x == 0xFF || x >= 0x91), detail: "Une clé USB piégée ne peut rien lancer toute seule.".into(), poids: 2, reparer: Some("autorun") });
    let admins: Option<u32> = v("admins").parse().ok();
    c.push(Controle { nom: "Peu de comptes administrateurs", ok: admins.map(|n| n <= 2), detail: admins.map(|n| format!("{n} compte(s) administrateur(s)")).unwrap_or_default(), poids: 5, reparer: Some("ms-settings:otherusers") });
    c
}

pub fn note(c: &[Controle]) -> u32 {
    let total: u32 = c.iter().filter(|x| x.ok.is_some()).map(|x| x.poids).sum();
    let bons: u32 = c.iter().filter(|x| x.ok == Some(true)).map(|x| x.poids).sum();
    if total == 0 { 0 } else { bons * 100 / total }
}

fn reparer(quoi: &str) -> Result<String, String> {
    let ouvrir = |u: &str| {
        let _ = std::process::Command::new("explorer").arg(u).spawn();
        Ok("Réglage ouvert".to_string())
    };
    match quoi {
        "parefeu" => cmd("netsh", &["advfirewall", "set", "allprofiles", "state", "on"]).map(|_| "✔ Pare-feu activé".into()),
        "smb1" => ps("Set-SmbServerConfiguration -EnableSMB1Protocol $false -Force").map(|_| "✔ SMBv1 désactivé".into()),
        "uac" => ps(r"Set-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Name EnableLUA -Value 1").map(|_| "✔ UAC activé (redémarre le PC)".into()),
        "rdp" => ps(r"Set-ItemProperty -Path 'HKLM:\System\CurrentControlSet\Control\Terminal Server' -Name fDenyTSConnections -Value 1").map(|_| "✔ Bureau à distance fermé".into()),
        "invite" => ps("Get-LocalUser | Where-Object { $_.SID -like '*-501' } | Disable-LocalUser").map(|_| "✔ Compte Invité désactivé".into()),
        "autorun" => ps(r"New-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer' -Force | Out-Null; Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer' -Name NoDriveTypeAutoRun -Value 255").map(|_| "✔ Exécution automatique coupée".into()),
        u => ouvrir(u),
    }
}

pub struct Durcissement {
    r: Arc<Mutex<Option<Vec<Controle>>>>,
    en_cours: Arc<AtomicBool>,
    message: String,
}

impl Durcissement {
    pub fn new() -> Self {
        Durcissement { r: Arc::default(), en_cours: Arc::default(), message: String::new() }
    }

    fn lancer(&self) {
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        let (r, e) = (self.r.clone(), self.en_cours.clone());
        std::thread::spawn(move || {
            let sortie = ps(SCRIPT_AUDIT).unwrap_or_default();
            let m: BTreeMap<String, String> = sortie.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
            let d = crate::optimiseur::reg_dword;
            let c = controles(
                &m,
                d(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System", "EnableLUA"),
                d(r"HKLM\System\CurrentControlSet\Control\Terminal Server", "fDenyTSConnections"),
                d(r"HKLM\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity", "Enabled"),
                d(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer", "NoDriveTypeAutoRun").or_else(|| d(r"HKLM\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer", "NoDriveTypeAutoRun")),
            );
            *r.lock().unwrap() = Some(c);
            e.store(false, Relaxed);
        });
    }
}

impl Outil for Durcissement {
    fn nom(&self) -> &'static str {
        "🛡 Durcissement de Windows"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Durcissement de Windows");
        ui.weak("Vérifie les protections de Windows (antivirus, pare-feu, mises à jour, chiffrement, vieux protocoles…), donne une note et répare en un clic. Lance Outils en administrateur pour tout voir et tout corriger.");
        let en_cours = self.en_cours.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!en_cours, egui::Button::new("🔎 Vérifier")).clicked() {
                self.lancer();
            }
            if en_cours {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            ui.label(&self.message);
        });
        let Some(c) = self.r.lock().unwrap().clone() else {
            if !en_cours {
                self.lancer();
            }
            return;
        };
        let n = note(&c);
        ui.label(RichText::new(format!("{n} / 100")).size(30.0).strong().color(if n >= 85 { Color32::LIGHT_GREEN } else if n >= 60 { Color32::from_rgb(240, 200, 70) } else { Color32::LIGHT_RED }));
        let mut action = None;
        for x in &c {
            ui.horizontal_wrapped(|ui| {
                let (t, col) = match x.ok {
                    Some(true) => ("✔", Color32::LIGHT_GREEN),
                    Some(false) => ("✖", Color32::from_rgb(255, 150, 100)),
                    None => ("?", Color32::GRAY),
                };
                ui.colored_label(col, RichText::new(format!("{t} {}", x.nom)).strong());
                ui.weak(&x.detail);
                if x.ok == Some(false) {
                    if let Some(r) = x.reparer {
                        if ui.small_button(if r.contains(':') { "Ouvrir le réglage" } else { "🛠 Corriger" }).clicked() {
                            action = Some(r);
                        }
                    }
                }
            });
        }
        if let Some(a) = action {
            self.message = reparer(a).unwrap_or_else(|e| format!("✖ {e} (Outils en administrateur ?)"));
            if !a.contains(':') {
                self.lancer();
            }
        }
        ui.weak("« ? » : impossible à vérifier sans les droits administrateur, ou réglage absent de ta version de Windows.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protections() {
        let s = "\nParamètres Profil de domaine :\n----------------------------------------------------------------------\nÉtat                                  Actif\n\nParamètres Profil privé :\n----------------------------------------------------------------------\nÉtat                                  Inactif\n";
        let p = profils_parefeu(s);
        assert_eq!(p.len(), 2);
        assert!(p[0].1 && !p[1].1, "{p:?}");
        let en = "Domain Profile Settings:\n----\nState                                 ON\n";
        assert!(profils_parefeu(en)[0].1);
        let mut a = BTreeMap::new();
        a.insert("x".to_string(), "1".to_string());
        let mut b = a.clone();
        b.insert("y".to_string(), "2".to_string());
        b.insert("x".to_string(), "3".to_string());
        assert_eq!(differences(&a, &b).len(), 2);
        assert!(differences(&a, &a).is_empty());
        assert_eq!(differences(&b, &a).len(), 2);
        let mut r = BTreeMap::new();
        for (k, v) in [("defender", "True"), ("temps_reel", "True"), ("signatures", "1"), ("parefeu", "True"), ("maj", "10"), ("smb1", "False"), ("invite", "False"), ("bitlocker", "On"), ("secureboot", "True"), ("admins", "1")] {
            r.insert(k.to_string(), v.to_string());
        }
        let c = controles(&r, Some(1), Some(1), Some(1), Some(255));
        assert_eq!(note(&c), 100);
        r.insert("defender".into(), "False".into());
        let c = controles(&r, Some(0), Some(0), None, None);
        assert!(note(&c) < 80);
    }
}
