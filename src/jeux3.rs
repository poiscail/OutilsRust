//! Jeu : remappage de touches par jeu (16), comparatif PC / configuration requise (19), scènes OBS automatiques (21),
//! mods du Steam Workshop (23), nettoyage des jeux (24).
use crate::outil::{charger, enregistrer, jeu_actif, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

fn taille_dossier(d: &Path) -> u64 {
    let mut t = 0;
    let mut pile = vec![d.to_path_buf()];
    while let Some(p) = pile.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => pile.push(e.path()),
                Ok(m) => t += m.len(),
                _ => {}
            }
        }
    }
    t
}

/// Bibliothèques Steam (dossiers contenant « steamapps »).
pub fn bibliotheques_steam() -> Vec<PathBuf> {
    let Some(r) = crate::lanceur::steam_racine() else { return vec![] };
    let mut v = vec![r.clone()];
    if let Ok(t) = std::fs::read_to_string(r.join("steamapps").join("libraryfolders.vdf")) {
        v.extend(chemins_vdf(&t));
    }
    v.sort();
    v.dedup();
    v
}

pub fn chemins_vdf(t: &str) -> Vec<PathBuf> {
    let re = regex::Regex::new(r#""path"\s*"([^"]+)""#).unwrap();
    re.captures_iter(t).map(|c| PathBuf::from(c[1].replace("\\\\", "\\"))).collect()
}

/// appid -> nom, d'après les appmanifest de toutes les bibliothèques.
fn noms_steam() -> std::collections::HashMap<String, String> {
    let re_id = regex::Regex::new(r#""appid"\s*"(\d+)""#).unwrap();
    let re_nom = regex::Regex::new(r#""name"\s*"([^"]+)""#).unwrap();
    let mut m = std::collections::HashMap::new();
    for b in bibliotheques_steam() {
        for e in std::fs::read_dir(b.join("steamapps")).into_iter().flatten().flatten() {
            if !e.file_name().to_string_lossy().starts_with("appmanifest_") {
                continue;
            }
            let t = String::from_utf8_lossy(&std::fs::read(e.path()).unwrap_or_default()).into_owned();
            if let (Some(i), Some(n)) = (re_id.captures(&t), re_nom.captures(&t)) {
                m.insert(i[1].to_string(), n[1].to_string());
            }
        }
    }
    m
}

// ---------------------------------------------------------------------------------------------------
// 16 · Remappage de touches
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Regle {
    pub exe: String, // vide : partout
    pub de: u16,
    pub vers: u16,
    pub actif: bool,
}

/// Touches proposées (code, nom).
pub fn touches() -> Vec<(u16, String)> {
    let mut v: Vec<(u16, String)> = (b'A'..=b'Z').map(|c| (c as u16, (c as char).to_string())).collect();
    v.extend((b'0'..=b'9').map(|c| (c as u16, (c as char).to_string())));
    v.extend((1..=12).map(|f| (0x6F + f, format!("F{f}"))));
    v.extend((0..=9).map(|n| (0x60 + n, format!("Pavé num {n}"))));
    for (k, n) in [
        (0x20, "Espace"), (0x0D, "Entrée"), (0x1B, "Échap"), (0x09, "Tab"), (0x08, "Retour arrière"), (0x14, "Verr. Maj"), (0xA0, "Maj gauche"), (0xA1, "Maj droite"),
        (0xA2, "Ctrl gauche"), (0xA3, "Ctrl droit"), (0xA4, "Alt gauche"), (0x25, "Flèche ←"), (0x26, "Flèche ↑"), (0x27, "Flèche →"), (0x28, "Flèche ↓"),
        (0x2E, "Suppr"), (0x2D, "Inser"), (0x24, "Origine"), (0x23, "Fin"), (0x21, "Page ↑"), (0x22, "Page ↓"), (0x5B, "Windows"),
    ] {
        v.push((k, n.to_string()));
    }
    v
}

pub fn nom_touche(vk: u16) -> String {
    touches().into_iter().find(|t| t.0 == vk).map(|t| t.1).unwrap_or_else(|| format!("touche {vk:#04X}"))
}

/// Règle qui s'applique à cette touche pour cette appli (les règles d'un jeu passent avant « partout »).
pub fn regle_pour<'a>(regles: &'a [Regle], vk: u16, exe: &str) -> Option<&'a Regle> {
    let ok = |r: &&Regle| r.actif && r.de == vk;
    regles.iter().filter(ok).find(|r| !r.exe.is_empty() && r.exe.eq_ignore_ascii_case(exe)).or_else(|| regles.iter().filter(ok).find(|r| r.exe.is_empty()))
}

mod crochet {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const MARQUE: usize = 0x0075_7403; // nos frappes injectées
    pub static REGLES: Mutex<Vec<Regle>> = Mutex::new(Vec::new());
    static DEVANT: Mutex<(isize, String)> = Mutex::new((0, String::new()));

    fn exe_devant() -> String {
        unsafe {
            let h = GetForegroundWindow();
            let mut c = DEVANT.lock().unwrap();
            if c.0 != h as isize {
                let mut pid = 0;
                GetWindowThreadProcessId(h, &mut pid);
                *c = (h as isize, crate::reseau::nom_processus(pid));
            }
            c.1.clone()
        }
    }

    unsafe extern "system" fn clavier(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            let k = &*(lp as *const KBDLLHOOKSTRUCT);
            if k.dwExtraInfo != MARQUE {
                let regles = REGLES.lock().unwrap();
                if !regles.is_empty() {
                    let exe = exe_devant();
                    if let Some(r) = regle_pour(&regles, k.vkCode as u16, &exe) {
                        let haut = matches!(wp as u32, WM_KEYUP | WM_SYSKEYUP);
                        let mut i: INPUT = std::mem::zeroed();
                        i.r#type = INPUT_KEYBOARD;
                        i.Anonymous.ki = KEYBDINPUT { wVk: r.vers, wScan: MapVirtualKeyW(r.vers as u32, MAPVK_VK_TO_VSC) as u16, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: MARQUE };
                        SendInput(1, &i, std::mem::size_of::<INPUT>() as i32);
                        return 1; // la touche d'origine est avalée
                    }
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }

    pub fn lancer() {
        std::thread::spawn(|| unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(clavier), null_mut(), 0);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
        });
    }
}

pub struct Remappage {
    regles: Vec<Regle>,
    lance: bool,
}

impl Remappage {
    pub fn new() -> Self {
        let regles: Vec<Regle> = charger("remappage");
        let mut r = Remappage { regles, lance: false };
        r.appliquer();
        r
    }

    fn appliquer(&mut self) {
        *crochet::REGLES.lock().unwrap() = self.regles.clone();
        if !self.lance && self.regles.iter().any(|r| r.actif) {
            self.lance = true;
            crochet::lancer(); // le crochet clavier ne tourne que s'il y a des règles
        }
    }
}

impl Outil for Remappage {
    fn nom(&self) -> &'static str {
        "🔀 Remappage de touches"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Remappage de touches");
        ui.weak("Une touche devient une autre, partout ou seulement dans un jeu (nom de son .exe). Attention : certains jeux en ligne avec anti-triche n'aiment pas ça.");
        let avant = self.regles.clone();
        let liste = touches();
        let mut retirer = None;
        for (i, r) in self.regles.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.checkbox(&mut r.actif, "");
                for (id, v) in [("de", &mut r.de), ("vers", &mut r.vers)] {
                    egui::ComboBox::from_id_salt((id, i)).selected_text(nom_touche(*v)).height(300.0).show_ui(ui, |ui| {
                        for (k, n) in &liste {
                            ui.selectable_value(v, *k, n);
                        }
                    });
                    if id == "de" {
                        ui.label("→");
                    }
                }
                ui.label("dans");
                ui.add(egui::TextEdit::singleline(&mut r.exe).hint_text("partout (ou ex. RocketLeague.exe)").desired_width(220.0));
                if ui.small_button("🗑").clicked() {
                    retirer = Some(i);
                }
            });
        }
        if let Some(i) = retirer {
            self.regles.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Règle").clicked() {
                self.regles.push(Regle { exe: String::new(), de: 0x14, vers: 0xA2, actif: true });
            }
            if let Some(j) = jeu_actif() {
                ui.weak(format!("Jeu en cours : {j}"));
            }
        });
        if self.regles != avant {
            enregistrer("remappage", &self.regles);
            self.appliquer();
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 19 · Comparatif PC / configuration requise
// ---------------------------------------------------------------------------------------------------

/// Go demandés pour un motif (« 8 GB RAM », « 16 Go de mémoire », « 50 GB available space »…).
pub fn go_demandes(t: &str, mots: &str) -> Option<u32> {
    let re = regex::Regex::new(&format!(r"(?i)(\d+)\s*(?:GB|Go|G[oB])\s*(?:of\s*|de\s*|d')?(?:{mots})")).ok()?;
    re.captures(t).and_then(|c| c[1].parse().ok())
}

#[derive(Clone, Default)]
struct Config {
    nom: String,
    mini: String,
    reco: String,
}

fn ram_go() -> u32 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        GlobalMemoryStatusEx(&mut m);
        ((m.ullTotalPhys as f64) / 1_073_741_824.0).round() as u32
    }
}

pub struct Comparatif {
    saisie: String,
    pc: Arc<Mutex<Option<String>>>,
    res: Arc<Mutex<Option<Result<Config, String>>>>,
}

impl Comparatif {
    pub fn new() -> Self {
        Comparatif { saisie: String::new(), pc: Arc::default(), res: Arc::default() }
    }
}

impl Outil for Comparatif {
    fn nom(&self) -> &'static str {
        "⚖ Ça tourne sur mon PC ?"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ça tourne sur mon PC ?");
        if self.pc.lock().unwrap().is_none() {
            *self.pc.lock().unwrap() = Some("…".into());
            let p = self.pc.clone();
            std::thread::spawn(move || {
                let s = crate::optimiseur::ps("$c=(Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim(); $g=(Get-CimInstance Win32_VideoController | ForEach-Object Name) -join ' + '; \"Processeur : $c`nCarte graphique : $g\"").unwrap_or_default();
                *p.lock().unwrap() = Some(s.trim().to_string());
            });
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.saisie).hint_text("lien Steam du jeu ou son numéro").desired_width(320.0));
            if ui.button("🔍 Comparer").clicked() {
                if let Some(id) = crate::promos::appid_de(&self.saisie) {
                    let r = self.res.clone();
                    *r.lock().unwrap() = None;
                    std::thread::spawn(move || {
                        let x = (|| {
                            let j: serde_json::Value = ureq::get("https://store.steampowered.com/api/appdetails").query("appids", &id.to_string()).query("l", "french")
                                .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
                            let d = &j[id.to_string()]["data"];
                            let p = &d["pc_requirements"];
                            Ok(Config {
                                nom: d["name"].as_str().unwrap_or("?").into(),
                                mini: crate::reseau2::texte_html(p["minimum"].as_str().unwrap_or("")),
                                reco: crate::reseau2::texte_html(p["recommended"].as_str().unwrap_or("")),
                            })
                        })();
                        *r.lock().unwrap() = Some(x);
                    });
                }
            }
        });
        ui.separator();
        let ram = ram_go();
        let pc = self.pc.lock().unwrap().clone().unwrap_or_default();
        ui.columns(3, |c| {
            c[0].strong("💻 Ton PC");
            c[0].label(&pc);
            c[0].label(format!("Mémoire : {ram} Go"));
            let r = self.res.lock().unwrap().clone();
            match r {
                None if !self.saisie.is_empty() => {
                    c[1].spinner();
                }
                Some(Err(e)) => {
                    c[1].colored_label(Color32::LIGHT_RED, e);
                }
                Some(Ok(cfg)) => {
                    for (k, (titre, t)) in [("Minimum", &cfg.mini), ("Recommandé", &cfg.reco)].into_iter().enumerate() {
                        let ui = &mut c[k + 1];
                        ui.strong(format!("{titre} · {}", cfg.nom));
                        if let Some(g) = go_demandes(t, "RAM|de mémoire|mémoire vive|Memory") {
                            if ram >= g { ui.colored_label(Color32::LIGHT_GREEN, format!("✔ Mémoire : {g} Go demandés")) } else { ui.colored_label(Color32::LIGHT_RED, format!("✖ Mémoire : {g} Go demandés")) };
                        }
                        egui::ScrollArea::vertical().id_salt(("cfg", k)).max_height(320.0).show(ui, |ui| {
                            ui.label(if t.is_empty() { "(non précisé)" } else { t.as_str() });
                        });
                    }
                }
                _ => {}
            }
        });
        ui.weak("La mémoire est vérifiée automatiquement ; pour le processeur et la carte graphique, compare les noms (un modèle plus récent ou un chiffre plus grand dans la même gamme est en général meilleur).");
    }
}

// ---------------------------------------------------------------------------------------------------
// 21 · Scènes OBS automatiques (obs-websocket 5)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ReglagesObs {
    pub actif: bool,
    pub port: u16,
    pub mot_de_passe: String,
    pub scene_jeu: String,
    pub scene_hors_jeu: String,
    pub enregistrer: bool,
}

/// Réponse à l'authentification d'obs-websocket : base64(sha256(base64(sha256(mdp + sel)) + défi)).
pub fn auth_obs(mdp: &str, sel: &str, defi: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use sha2::{Digest, Sha256};
    let secret = B64.encode(Sha256::digest(format!("{mdp}{sel}").as_bytes()));
    B64.encode(Sha256::digest(format!("{secret}{defi}").as_bytes()))
}

/// Ouvre une session obs-websocket, envoie les requêtes, renvoie les réponses.
fn obs(r: &ReglagesObs, requetes: &[(&str, serde_json::Value)]) -> Result<Vec<serde_json::Value>, String> {
    use tungstenite::Message;
    let port = if r.port == 0 { 4455 } else { r.port };
    let flux = std::net::TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(), Duration::from_secs(2)).map_err(|_| "OBS n'est pas ouvert, ou le serveur WebSocket est désactivé (OBS › Outils › Paramètres du serveur WebSocket)".to_string())?;
    let _ = flux.set_read_timeout(Some(Duration::from_secs(5)));
    let (mut ws, _) = tungstenite::client(format!("ws://127.0.0.1:{port}"), flux).map_err(|e| e.to_string())?;
    let lire = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| -> Result<serde_json::Value, String> {
        loop {
            match ws.read().map_err(|e| e.to_string())? {
                Message::Text(t) => return serde_json::from_str(&t).map_err(|e| e.to_string()),
                Message::Close(_) => return Err("OBS a refusé la connexion (mot de passe ?)".into()),
                _ => {}
            }
        }
    };
    let hello = lire(&mut ws)?;
    let mut identify = serde_json::json!({ "op": 1, "d": { "rpcVersion": 1 } });
    if let Some(a) = hello["d"]["authentication"].as_object() {
        identify["d"]["authentication"] = auth_obs(&r.mot_de_passe, a["salt"].as_str().unwrap_or(""), a["challenge"].as_str().unwrap_or("")).into();
    }
    ws.send(Message::Text(identify.to_string())).map_err(|e| e.to_string())?;
    let id = lire(&mut ws)?;
    if id["op"] != 2 {
        return Err("authentification refusée".into());
    }
    let mut reponses = vec![];
    for (k, (t, d)) in requetes.iter().enumerate() {
        ws.send(Message::Text(serde_json::json!({ "op": 6, "d": { "requestType": t, "requestId": k.to_string(), "requestData": d } }).to_string())).map_err(|e| e.to_string())?;
        loop {
            let m = lire(&mut ws)?;
            if m["op"] == 7 {
                reponses.push(m["d"].clone());
                break;
            }
        }
    }
    let _ = ws.close(None);
    Ok(reponses)
}

pub struct Obs {
    r: Arc<Mutex<ReglagesObs>>,
    scenes: Vec<String>,
    e: Etat,
}

impl Obs {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglagesObs>> = Arc::new(Mutex::new(charger("obs")));
        let r2 = r.clone();
        std::thread::spawn(move || {
            let mut avant = jeu_actif();
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let jeu = jeu_actif();
                if jeu.is_some() == avant.is_some() {
                    continue;
                }
                avant = jeu.clone();
                let reg = r2.lock().unwrap().clone();
                if !reg.actif {
                    continue;
                }
                let scene = if jeu.is_some() { &reg.scene_jeu } else { &reg.scene_hors_jeu };
                let mut req: Vec<(&str, serde_json::Value)> = vec![];
                if !scene.is_empty() {
                    req.push(("SetCurrentProgramScene", serde_json::json!({ "sceneName": scene })));
                }
                if reg.enregistrer {
                    req.push((if jeu.is_some() { "StartRecord" } else { "StopRecord" }, serde_json::json!({})));
                }
                if !req.is_empty() {
                    match obs(&reg, &req) {
                        Ok(_) => crate::coeur::noter("OBS", &format!("{} : {scene}", if jeu.is_some() { "jeu lancé" } else { "jeu fermé" })),
                        Err(e) => crate::coeur::noter("OBS", &e),
                    }
                }
            }
        });
        Obs { r, scenes: vec![], e: Arc::default() }
    }
}

impl Outil for Obs {
    fn nom(&self) -> &'static str {
        "🎬 Scènes OBS automatiques"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scènes OBS automatiques");
        ui.weak("Change de scène (et lance l'enregistrement si tu veux) quand un jeu démarre ou s'arrête. Dans OBS : Outils › Paramètres du serveur WebSocket › activer.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.checkbox(&mut r.actif, "Activer");
        ui.horizontal(|ui| {
            ui.label("Mot de passe WebSocket");
            ui.add(egui::TextEdit::singleline(&mut r.mot_de_passe).password(true).desired_width(180.0));
            ui.label("Port");
            let mut p = if r.port == 0 { 4455 } else { r.port };
            if ui.add(egui::DragValue::new(&mut p).range(1024..=65535)).changed() {
                r.port = p;
            }
        });
        let occupe = etat_ui(ui, &self.e);
        if ui.add_enabled(!occupe, egui::Button::new("🔌 Tester et lire les scènes")).clicked() {
            match obs(&r, &[("GetSceneList", serde_json::json!({}))]) {
                Ok(v) => {
                    self.scenes = v.first().and_then(|d| d["responseData"]["scenes"].as_array().cloned()).unwrap_or_default().iter().filter_map(|s| s["sceneName"].as_str().map(String::from)).collect();
                    self.scenes.reverse();
                    *self.e.lock().unwrap() = (false, format!("✔ Connecté : {} scène(s)", self.scenes.len()));
                }
                Err(m) => *self.e.lock().unwrap() = (false, format!("✖ {m}")),
            }
        }
        for (titre, v) in [("Scène pendant un jeu", &mut r.scene_jeu), ("Scène hors jeu", &mut r.scene_hors_jeu)] {
            ui.horizontal(|ui| {
                ui.label(titre);
                egui::ComboBox::from_id_salt(titre).selected_text(if v.is_empty() { "ne pas changer" } else { v.as_str() }).show_ui(ui, |ui| {
                    ui.selectable_value(v, String::new(), "ne pas changer");
                    for s in &self.scenes {
                        ui.selectable_value(v, s.clone(), s);
                    }
                });
            });
        }
        ui.checkbox(&mut r.enregistrer, "Enregistrer avec OBS pendant chaque jeu");
        if r != avant {
            enregistrer("obs", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 23 · Mods du Steam Workshop / 24 · Nettoyage des jeux
// ---------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct ModWorkshop {
    jeu: String,
    id: String,
    nom: String,
    chemin: PathBuf,
    taille: u64,
}

fn lister_workshop() -> Vec<ModWorkshop> {
    let noms = noms_steam();
    let mut v = vec![];
    for b in bibliotheques_steam() {
        for jeu in std::fs::read_dir(b.join("steamapps/workshop/content")).into_iter().flatten().flatten() {
            let appid = jeu.file_name().to_string_lossy().into_owned();
            for m in std::fs::read_dir(jeu.path()).into_iter().flatten().flatten() {
                let id = m.file_name().to_string_lossy().into_owned();
                v.push(ModWorkshop { jeu: noms.get(&appid).cloned().unwrap_or(format!("jeu {appid}")), nom: format!("mod {id}"), taille: taille_dossier(&m.path()), chemin: m.path(), id });
            }
        }
    }
    // noms des mods (API publique de Steam, sans clé)
    for paquet in v.chunks_mut(100) {
        let mut form: Vec<(String, String)> = vec![("itemcount".into(), paquet.len().to_string())];
        form.extend(paquet.iter().enumerate().map(|(i, m)| (format!("publishedfileids[{i}]"), m.id.clone())));
        let f: Vec<(&str, &str)> = form.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        if let Ok(j) = ureq::post("https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/").send_form(&f).and_then(|r| Ok(r.into_json::<serde_json::Value>()?)) {
            for d in j["response"]["publishedfiledetails"].as_array().into_iter().flatten() {
                if let (Some(id), Some(t)) = (d["publishedfileid"].as_str(), d["title"].as_str()) {
                    if let Some(m) = paquet.iter_mut().find(|m| m.id == id) {
                        m.nom = t.to_string();
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| b.taille.cmp(&a.taille));
    v
}

pub struct Workshop {
    liste: Arc<Mutex<Option<Vec<ModWorkshop>>>>,
    lance: bool,
}

impl Workshop {
    pub fn new() -> Self {
        Workshop { liste: Arc::default(), lance: false }
    }
}

impl Outil for Workshop {
    fn nom(&self) -> &'static str {
        "🧱 Mods du Steam Workshop"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mods du Steam Workshop");
        if !self.lance {
            self.lance = true;
            let l = self.liste.clone();
            std::thread::spawn(move || *l.lock().unwrap() = Some(lister_workshop()));
        }
        let Some(liste) = self.liste.lock().unwrap().clone() else {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        };
        let total: u64 = liste.iter().map(|m| m.taille).sum();
        ui.label(RichText::new(format!("{} mod(s) · {}", liste.len(), crate::disque::taille_lisible(total))).strong());
        ui.weak("Pour qu'un mod supprimé ne revienne pas, désabonne-toi aussi sur sa page Workshop (bouton 🌐).");
        let mut retirer = None;
        egui::ScrollArea::vertical().id_salt("workshop").show(ui, |ui| {
            for (i, m) in liste.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.strong(crate::disque::taille_lisible(m.taille));
                    ui.label(&m.nom);
                    ui.weak(&m.jeu);
                    if ui.small_button("🌐").on_hover_text("Page Workshop").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("steam://url/CommunityFilePage/{}", m.id)).spawn();
                    }
                    if ui.small_button("🗑").clicked() && crate::disque::corbeille(&m.chemin) {
                        retirer = Some(i);
                    }
                });
            }
        });
        if let Some(i) = retirer {
            if let Some(l) = self.liste.lock().unwrap().as_mut() {
                l.remove(i);
            }
        }
    }
}

#[derive(Clone)]
struct Cache {
    nom: String,
    chemin: PathBuf,
    taille: u64,
    coche: bool,
}

fn caches_jeux() -> Vec<Cache> {
    let local = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default());
    let noms = noms_steam();
    let mut v: Vec<(String, PathBuf)> = vec![
        ("Cache de shaders DirectX (Windows)".into(), local.join("D3DSCache")),
        ("Cache de shaders NVIDIA (DirectX)".into(), local.join(r"NVIDIA\DXCache")),
        ("Cache de shaders NVIDIA (OpenGL)".into(), local.join(r"NVIDIA\GLCache")),
        ("Cache de shaders AMD".into(), local.join(r"AMD\DxCache")),
        ("Cache de shaders AMD (DXC)".into(), local.join(r"AMD\DxcCache")),
        ("Rapports de plantage des jeux".into(), local.join("CrashDumps")),
        ("Cache web d'Epic Games".into(), local.join(r"EpicGamesLauncher\Saved\webcache_4430")),
    ];
    for b in bibliotheques_steam() {
        for e in std::fs::read_dir(b.join("steamapps/shadercache")).into_iter().flatten().flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            v.push((format!("Shaders Steam · {}", noms.get(&id).cloned().unwrap_or(format!("jeu {id}"))), e.path()));
        }
        v.push(("Fichiers temporaires de Steam".into(), b.join("steamapps/temp")));
    }
    let mut c: Vec<Cache> = v.into_iter().filter(|(_, p)| p.is_dir()).map(|(nom, chemin)| Cache { taille: taille_dossier(&chemin), nom, chemin, coche: false }).filter(|c| c.taille > 0).collect();
    c.sort_by(|a, b| b.taille.cmp(&a.taille));
    c
}

fn vider(d: &Path) -> u64 {
    let mut libere = 0;
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let t = if e.path().is_dir() { taille_dossier(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) };
        let ok = if e.path().is_dir() { std::fs::remove_dir_all(e.path()).is_ok() } else { std::fs::remove_file(e.path()).is_ok() };
        if ok {
            libere += t;
        }
    }
    libere
}

pub struct NettoyageJeux {
    liste: Arc<Mutex<Option<Vec<Cache>>>>,
    lance: bool,
    e: Etat,
}

impl NettoyageJeux {
    pub fn new() -> Self {
        NettoyageJeux { liste: Arc::default(), lance: false, e: Arc::default() }
    }
}

impl Outil for NettoyageJeux {
    fn nom(&self) -> &'static str {
        "🧽 Nettoyage des jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Nettoyage des jeux");
        ui.weak("Caches de shaders et fichiers temporaires : ils se recréent tout seuls (le premier lancement d'un jeu peut être un peu plus long). Ferme les jeux et Steam avant.");
        if !self.lance {
            self.lance = true;
            let l = self.liste.clone();
            std::thread::spawn(move || *l.lock().unwrap() = Some(caches_jeux()));
        }
        let occupe = etat_ui(ui, &self.e);
        let mut l = self.liste.lock().unwrap();
        let Some(liste) = l.as_mut() else {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(300));
            return;
        };
        let coches: u64 = liste.iter().filter(|c| c.coche).map(|c| c.taille).sum();
        ui.horizontal(|ui| {
            if ui.small_button("Tout cocher").clicked() {
                liste.iter_mut().for_each(|c| c.coche = true);
            }
            if ui.add_enabled(!occupe && coches > 0, egui::Button::new(format!("🧽 Nettoyer ({})", crate::disque::taille_lisible(coches)))).clicked() {
                let a: Vec<PathBuf> = liste.iter().filter(|c| c.coche).map(|c| c.chemin.clone()).collect();
                let li = self.liste.clone();
                en_fond(&self.e, move || {
                    let l: u64 = a.iter().map(|d| vider(d)).sum();
                    *li.lock().unwrap() = Some(caches_jeux());
                    Ok(format!("✔ {} libérés.", crate::disque::taille_lisible(l)))
                });
            }
        });
        egui::ScrollArea::vertical().id_salt("caches_jeux").show(ui, |ui| {
            for c in liste.iter_mut() {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut c.coche, "");
                    ui.strong(crate::disque::taille_lisible(c.taille));
                    ui.label(&c.nom);
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regles_et_lectures() {
        let r = vec![
            Regle { exe: String::new(), de: 0x14, vers: 0xA2, actif: true },
            Regle { exe: "Jeu.exe".into(), de: 0x14, vers: 0x20, actif: true },
            Regle { exe: String::new(), de: 0x41, vers: 0x42, actif: false },
        ];
        assert_eq!(regle_pour(&r, 0x14, "jeu.exe").unwrap().vers, 0x20); // la règle du jeu d'abord
        assert_eq!(regle_pour(&r, 0x14, "autre.exe").unwrap().vers, 0xA2);
        assert!(regle_pour(&r, 0x41, "x.exe").is_none()); // désactivée
        assert_eq!(go_demandes("Mémoire : 16 Go de mémoire", "RAM|de mémoire"), Some(16));
        assert_eq!(go_demandes("Memory: 8 GB RAM", "RAM|de mémoire"), Some(8));
        assert_eq!(chemins_vdf(r#""1" { "path" "D:\\SteamLibrary" }"#), vec![PathBuf::from("D:\\SteamLibrary")]);
        // valeur de référence calculée avec la formule de la documentation d'obs-websocket
        assert_eq!(auth_obs("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
    }
}
