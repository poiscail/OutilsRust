//! Téléphone : petit serveur web sur le réseau local, ouvert en scannant un QR code.
//! - Télécommande : volume, lecture / pause, piste suivante, verrouiller, veille, serveur Minecraft.
//! - Fichiers : télécharger sur le téléphone les fichiers partagés, envoyer des fichiers vers le PC.
//! Protégé par une clé aléatoire dans l'adresse. Réglages dans %APPDATA%\Outils\telephone.json.
use crate::minecraft::Pilote;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub port: u16,
    pub cle: String,
    pub auto: bool, // démarre avec Outils
    pub dossier_reception: PathBuf,
    pub fichiers: Vec<PathBuf>,
    pub distance: crate::pont::Distance,
}

impl Default for Reglages {
    fn default() -> Self {
        let dl = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads").join("Depuis le téléphone");
        Reglages { port: 8642, cle: nouvelle_cle(), auto: false, dossier_reception: dl, fichiers: vec![], distance: crate::pont::Distance::nouvelle() }
    }
}

fn fichier() -> PathBuf {
    crate::outil::base().join("telephone.json")
}

pub fn nouvelle_cle() -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut b = [0u8; 14];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| A[*x as usize % A.len()] as char).collect()
}

/// Adresse IP du PC sur le réseau local.
pub fn ip_locale() -> Option<String> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.168.1.1:80").ok()?; // aucun paquet n'est envoyé : sert à choisir l'interface
    Some(s.local_addr().ok()?.ip().to_string())
}

pub struct Requete {
    pub methode: String,
    pub chemin: String,
    pub params: Vec<(String, String)>,
    pub longueur: u64,
    pub cookie_cle: Option<String>,
}

pub fn decoder_url(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lit la ligne de requête et les en-têtes (le corps reste dans le lecteur).
pub fn lire_requete(r: &mut impl BufRead) -> Option<Requete> {
    let mut l = String::new();
    r.read_line(&mut l).ok()?;
    let mut p = l.split_whitespace();
    let methode = p.next()?.to_string();
    let cible = p.next()?.to_string();
    let (chemin, requete) = cible.split_once('?').unwrap_or((&cible, ""));
    let params = requete.split('&').filter(|x| !x.is_empty()).map(|x| {
        let (a, b) = x.split_once('=').unwrap_or((x, ""));
        (decoder_url(a), decoder_url(b))
    }).collect();
    let (mut longueur, mut cookie_cle) = (0, None);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 || h.trim().is_empty() {
            break;
        }
        let (k, v) = h.split_once(':')?;
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => longueur = v.trim().parse().unwrap_or(0),
            "cookie" => cookie_cle = v.split(';').find_map(|c| c.trim().strip_prefix("k=").map(str::to_string)),
            _ => {}
        }
    }
    Some(Requete { methode, chemin: decoder_url(chemin), params, longueur, cookie_cle })
}

impl Requete {
    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str())
    }
}

/// Nom de fichier reçu sans dossier ni caractère interdit.
pub fn nom_recu(n: &str) -> String {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().map(|c| if r#":*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "fichier".into() } else { s }
}

fn libre(dossier: &Path, nom: &str) -> PathBuf {
    let p = dossier.join(nom);
    if !p.exists() {
        return p;
    }
    let (base, ext) = match nom.rsplit_once('.') {
        Some((b, e)) => (b.to_string(), format!(".{e}")),
        None => (nom.to_string(), String::new()),
    };
    (2..).map(|i| dossier.join(format!("{base} ({i}){ext}"))).find(|p| !p.exists()).unwrap()
}

fn repondre(s: &mut TcpStream, code: &str, type_: &str, corps: &[u8], extra: &str) {
    let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: {type_}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n{extra}\r\n", corps.len());
    let _ = s.write_all(corps);
}

fn touche(vk: u16) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let mk = |haut: bool| {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 };
        i
    };
    let seq = [mk(false), mk(true)];
    unsafe { SendInput(2, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

fn json_echappe(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

const PAGE: &str = include_str!("telephone.html");

// ---------------------------------------------------------------------------------------------------
// Appli Outils pour Android : API JSON, appairage par code, détection sur le Wi-Fi, notifications
// ---------------------------------------------------------------------------------------------------

/// Mesures du moniteur (CPU, GPU, FPS…), données par la fenêtre principale.
pub static MESURES: std::sync::OnceLock<crate::moniteur::Lecteur> = std::sync::OnceLock::new();
/// Code d'appairage à 6 chiffres et sa date de fin.
static CODE: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);
/// Notifications du PC à transmettre au téléphone : (numéro, titre, texte).
static NOTIFS: Mutex<std::collections::VecDeque<(u64, String, String)>> = Mutex::new(std::collections::VecDeque::new());
static NUMERO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Port UDP où le PC répond aux téléphones qui le cherchent.
pub const PORT_DECOUVERTE: u16 = 8643;
/// Fichiers de réglages qu'on peut synchroniser avec le téléphone.
pub const SYNCHRO: [&str; 4] = ["agenda", "ecole", "rebours", "sante"];

/// Toute notification du PC part aussi vers le téléphone appairé.
pub fn pousser_notif(titre: &str, texte: &str) {
    crate::pont::notifier_distance(titre, texte);
    let n = NUMERO.fetch_add(1, Relaxed) + 1;
    let mut q = NOTIFS.lock().unwrap();
    q.push_back((n, titre.to_string(), texte.to_string()));
    while q.len() > 50 {
        q.pop_front();
    }
}

pub fn notifs_depuis(depuis: u64) -> (u64, Vec<(u64, String, String)>) {
    let q = NOTIFS.lock().unwrap();
    (NUMERO.load(Relaxed), q.iter().filter(|x| x.0 > depuis).cloned().collect())
}

pub fn nouveau_code() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::getrandom(&mut b);
    let c = format!("{:06}", u32::from_le_bytes(b) % 1_000_000);
    *CODE.lock().unwrap() = Some((c.clone(), std::time::Instant::now() + Duration::from_secs(600)));
    c
}

fn code_valide(c: &str) -> bool {
    let mut g = CODE.lock().unwrap();
    match &*g {
        Some((code, fin)) if std::time::Instant::now() < *fin && code == c.trim() => {
            *g = None; // une seule fois
            true
        }
        _ => false,
    }
}

/// Adresses MAC des cartes réseau du PC (pour le réveiller depuis le téléphone).
fn macs() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    static CACHE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let o = std::process::Command::new("getmac").args(["/fo", "csv", "/nh"]).creation_flags(0x0800_0000).output();
        let t = o.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        t.lines().filter_map(|l| l.split(',').next()).map(|m| m.trim_matches('"').replace('-', ":")).filter(|m| m.len() == 17).collect()
    }).clone()
}

fn nom_pc() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

/// Répond aux téléphones qui cherchent un PC avec Outils sur le Wi-Fi.
fn decouverte(port: u16, marche: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_DECOUVERTE)) else { return };
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut b = [0u8; 64];
        while marche.load(Relaxed) {
            if let Ok((n, de)) = s.recv_from(&mut b) {
                if &b[..n] == b"OUTILS?" {
                    let r = serde_json::json!({ "outils": 1, "port": port, "nom": nom_pc(), "version": env!("CARGO_PKG_VERSION") });
                    let _ = s.send_to(r.to_string().as_bytes(), de);
                }
            }
        }
    });
}

fn json(s: &mut TcpStream, v: serde_json::Value) {
    repondre(s, "200 OK", "application/json; charset=utf-8", v.to_string().as_bytes(), "");
}

fn fichier_synchro(nom: &str) -> Option<PathBuf> {
    SYNCHRO.contains(&nom).then(|| crate::outil::chemin(nom))
}

fn date_fichier(p: &Path) -> u64 {
    p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}


/// Actions du téléphone (réseau local ou accès à distance).
fn commande(c: &Contexte, action: &str, texte: Option<&str>) -> Result<String, String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let req_texte = texte.unwrap_or("");
    let r: Result<&str, String> = match action {
                "vol_plus" => {
                    for _ in 0..2 {
                        touche(VK_VOLUME_UP);
                    }
                    Ok("Volume +")
                }
                "vol_moins" => {
                    for _ in 0..2 {
                        touche(VK_VOLUME_DOWN);
                    }
                    Ok("Volume −")
                }
                "muet" => {
                    touche(VK_VOLUME_MUTE);
                    Ok("Muet")
                }
                "lecture" => {
                    touche(VK_MEDIA_PLAY_PAUSE);
                    Ok("Lecture / pause")
                }
                "suivant" => {
                    touche(VK_MEDIA_NEXT_TRACK);
                    Ok("Piste suivante")
                }
                "precedent" => {
                    touche(VK_MEDIA_PREV_TRACK);
                    Ok("Piste précédente")
                }
                "verrouiller" => {
                    unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
                    Ok("PC verrouillé")
                }
                "veille" => {
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_secs(2));
                        unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
                    });
                    Ok("Mise en veille")
                }
                "mc_demarrer" => c.mc.demarrer().map(|_| "Serveur Minecraft : démarrage"),
                "mc_sauvegarder" => {
                    c.mc.commande("save-all");
                    Ok("Serveur Minecraft : sauvegarde")
                }
                "mc_dire" => {
                    let t: String = req_texte.chars().filter(|c| !c.is_control()).take(200).collect();
                    c.mc.commande(&format!("say [Téléphone] {t}"));
                    Ok("Message envoyé dans le chat")
                }
                "mc_arreter" => {
                    let mc = c.mc.clone();
                    std::thread::spawn(move || mc.arreter());
                    Ok("Serveur Minecraft : arrêt")
                }
                "ouvrir" => {
                    if crate::pratique::url_autorisee(req_texte) {
                        use std::os::windows::process::CommandExt;
                        let _ = std::process::Command::new("cmd").args(["/c", "start", "", req_texte]).creation_flags(0x0800_0000).spawn();
                        Ok("Ouverture")
                    } else {
                        Err("site non autorisé".into())
                    }
                }
                _ => Err("action inconnue".into()),
            
    };
    r.map(String::from)
}

/// État du PC en JSON (pour l'appli et l'accès à distance).
fn etat_json(c: &Contexte) -> serde_json::Value {
    let (m, jeu) = MESURES.get().map(|l| l(true)).unwrap_or((crate::moniteur::Instantane { fps: -1.0, cpu: -1.0, cpu_t: -1.0, gpu: -1.0, gpu_t: -1.0, ram: -1.0 }, None));
    let (mc, joueurs) = match c.mc.statut() {
        None => ("arrêté", vec![]),
        Some((false, _)) => ("démarrage", vec![]),
        Some((true, j)) => ("en ligne", j),
    };
    serde_json::json!({
        "nom": nom_pc(), "version": env!("CARGO_PKG_VERSION"),
        "cpu": m.cpu, "cpu_t": m.cpu_t, "gpu": m.gpu, "gpu_t": m.gpu_t, "ram": m.ram, "fps": m.fps, "jeu": jeu,
        "mc": mc, "joueurs": joueurs, "macs": macs(),
    })
}

/// Requêtes de l'appli Android (déjà authentifiées). Renvoie faux si le chemin n'est pas de l'API.
fn api(s: &mut TcpStream, req: &Requete, lecteur: &mut BufReader<TcpStream>, c: &Contexte) -> bool {
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("GET", "/api/etat") => json(s, etat_json(c)),
        ("GET", "/api/jeux") => {
            let v: Vec<serde_json::Value> = crate::jeux4::jeux_installes().iter().map(|j| serde_json::json!({ "id": j.id, "nom": j.nom })).collect();
            json(s, serde_json::Value::Array(v));
        }
        ("POST", "/api/lancer") => {
            let id = req.param("id").unwrap_or("");
            match crate::jeux4::jeux_installes().into_iter().find(|j| j.id == id) {
                Some(j) => {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("cmd").args(["/c", "start", "", &j.cmd]).current_dir(if j.dir.is_empty() { "." } else { &j.dir }).creation_flags(0x0800_0000).spawn();
                    noter(c, format!("📱 Lancement de {}", j.nom));
                    repondre(s, "200 OK", "text/plain; charset=utf-8", format!("Lancement de {}", j.nom).as_bytes(), "");
                }
                None => repondre(s, "404 Not Found", "text/plain; charset=utf-8", "jeu introuvable".as_bytes(), ""),
            }
        }
        ("GET", "/api/chercher") => {
            let m = crate::recherche::moteur();
            if !m.demarre.load(Relaxed) {
                crate::recherche::demarrer(false);
            }
            let q = crate::recherche::analyser(req.param("q").unwrap_or(""), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as u32));
            let ix = m.index.read().unwrap();
            let r = crate::recherche::chercher(&ix, &q, 60, false, &|p: &str| std::fs::metadata(p).ok().map(|m| (m.len(), 0)));
            let v: Vec<serde_json::Value> = r.liste.iter().map(|&i| {
                let chemin = ix.chemin(i);
                let dossier = ix.drap[i as usize] & crate::recherche::DOSSIER != 0;
                let t = ix.taille[i as usize];
                let t = if t == crate::recherche::INCONNU && !dossier { std::fs::metadata(&chemin).map_or(0, |m| m.len()) } else if dossier { 0 } else { t };
                serde_json::json!({ "chemin": chemin, "nom": ix.nom(i), "taille": t, "date": ix.date[i as usize], "dossier": dossier })
            }).collect();
            json(s, serde_json::json!({ "total": r.total, "liste": v, "indexes": ix.vivants }));
        }
        ("GET", "/api/fichier") => {
            let p = PathBuf::from(req.param("chemin").unwrap_or(""));
            match std::fs::File::open(&p).ok().filter(|_| p.is_file()) {
                Some(mut fi) => {
                    let n = fi.metadata().map(|m| m.len()).unwrap_or(0);
                    let nom = p.file_name().unwrap_or_default().to_string_lossy().replace('"', "");
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {n}\r\nContent-Disposition: attachment; filename=\"{nom}\"\r\nConnection: close\r\n\r\n");
                    let _ = std::io::copy(&mut fi, s);
                    noter(c, format!("📤 {nom} envoyé au téléphone"));
                }
                None => repondre(s, "404 Not Found", "text/plain; charset=utf-8", "fichier introuvable".as_bytes(), ""),
            }
        }
        ("GET", "/api/ecran") => {
            let largeur: u32 = req.param("l").and_then(|x| x.parse().ok()).unwrap_or(960).clamp(320, 1920);
            let (l, h) = crate::capture::win::ecran_principal();
            match crate::capture::win::capturer_zone(0, 0, l, h) {
                Some(img) => {
                    let img = image::imageops::thumbnail(&img, largeur, (largeur as u64 * h as u64 / l.max(1) as u64) as u32);
                    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                    let mut b = vec![];
                    let _ = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut b, 60).encode_image(&rgb);
                    repondre(s, "200 OK", "image/jpeg", &b, "");
                }
                None => repondre(s, "500 Internal Server Error", "text/plain", b"capture impossible", ""),
            }
        }
        ("POST", "/api/ocr") => {
            let mut b = vec![];
            let _ = lecteur.take(req.longueur.min(16 << 20)).read_to_end(&mut b);
            let p = std::env::temp_dir().join(format!("outils_ocr_tel_{}.jpg", nouvelle_cle()));
            let r = std::fs::write(&p, &b).map_err(|e| e.to_string()).and_then(|_| crate::fichiers4::ocr(&p));
            let _ = std::fs::remove_file(&p);
            match r {
                Ok(texte) => {
                    let langue = req.param("langue").unwrap_or("fr");
                    let traduction = if texte.trim().is_empty() { String::new() } else { crate::traducteur::traduire_google(&texte, langue).unwrap_or_default() };
                    json(s, serde_json::json!({ "texte": texte, "traduction": traduction }));
                }
                Err(e) => repondre(s, "500 Internal Server Error", "text/plain; charset=utf-8", e.as_bytes(), ""),
            }
        }
        ("POST", "/api/webcam") => {
            let mut b = vec![];
            let _ = lecteur.take(req.longueur.min(4 << 20)).read_to_end(&mut b);
            *crate::pont::WEBCAM.lock().unwrap() = Some((b, std::time::Instant::now()));
            repondre(s, "200 OK", "text/plain", b"ok", "");
        }
        ("GET", "/api/pour_tel") => {
            let v: Vec<serde_json::Value> = std::mem::take(&mut *crate::pont::POUR_TEL.lock().unwrap());
            json(s, serde_json::Value::Array(v));
        }
        ("GET", "/api/distance") => {
            let d = crate::pont::distance().lock().unwrap().clone();
            json(s, serde_json::json!({ "actif": d.actif, "sujet": d.sujet, "cle": d.cle }));
        }
        ("GET", "/api/notifs") => {
            let (dernier, l) = notifs_depuis(req.param("depuis").and_then(|x| x.parse().ok()).unwrap_or(0));
            let v: Vec<serde_json::Value> = l.into_iter().map(|(n, t, m)| serde_json::json!({ "n": n, "titre": t, "texte": m })).collect();
            json(s, serde_json::json!({ "dernier": dernier, "liste": v }));
        }
        ("GET", "/api/synchro") => {
            let Some(f) = fichier_synchro(req.param("nom").unwrap_or("")) else {
                repondre(s, "400 Bad Request", "text/plain", b"nom inconnu", "");
                return true;
            };
            let contenu = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or(serde_json::Value::Null);
            json(s, serde_json::json!({ "t": date_fichier(&f), "contenu": contenu }));
        }
        ("POST", "/api/synchro") => {
            let Some(f) = fichier_synchro(req.param("nom").unwrap_or("")) else {
                repondre(s, "400 Bad Request", "text/plain", b"nom inconnu", "");
                return true;
            };
            let mut b = vec![];
            let _ = lecteur.take(req.longueur.min(8 << 20)).read_to_end(&mut b);
            if serde_json::from_slice::<serde_json::Value>(&b).is_err() {
                repondre(s, "400 Bad Request", "text/plain", b"json invalide", "");
                return true;
            }
            let _ = std::fs::create_dir_all(f.parent().unwrap());
            match std::fs::write(&f, &b) {
                Ok(()) => {
                    crate::outil::recharger_synchro(req.param("nom").unwrap_or(""));
                    noter(c, format!("🔄 {} reçu du téléphone", req.param("nom").unwrap_or("")));
                    json(s, serde_json::json!({ "t": date_fichier(&f) }));
                }
                Err(e) => repondre(s, "500 Internal Server Error", "text/plain; charset=utf-8", e.to_string().as_bytes(), ""),
            }
        }
        _ => return false,
    }
    true
}

struct Contexte {
    r: Arc<Mutex<Reglages>>,
    mc: Pilote,
    journal: Arc<Mutex<Vec<String>>>,
}

fn noter_ui(j: &Arc<Mutex<Vec<String>>>, m: String) {
    let mut j = j.lock().unwrap();
    j.insert(0, m);
    j.truncate(30);
}

fn noter(c: &Contexte, m: String) {
    let mut j = c.journal.lock().unwrap();
    j.insert(0, m);
    j.truncate(30);
}

fn servir(mut s: TcpStream, c: &Contexte) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok(clone) = s.try_clone() else { return };
    let mut lecteur = BufReader::new(clone);
    let Some(req) = lire_requete(&mut lecteur) else { return };
    let reg = c.r.lock().unwrap().clone();
    // appairage de l'appli Android : le code à 6 chiffres affiché sur le PC donne la clé
    if req.chemin == "/api/appairer" {
        if req.param("code").is_some_and(code_valide) {
            noter(c, format!("📱 Téléphone associé ({})", req.param("appareil").unwrap_or("Android")));
            json(&mut s, serde_json::json!({ "cle": reg.cle, "nom": nom_pc() }));
        } else {
            std::thread::sleep(Duration::from_millis(800)); // pas d'essais à la chaîne
            repondre(&mut s, "403 Forbidden", "text/plain; charset=utf-8", "Code faux ou expiré : regarde le code affiché dans Outils sur le PC.".as_bytes(), "");
        }
        return;
    }
    let ok = req.param("k").or(req.cookie_cle.as_deref()) == Some(reg.cle.as_str());
    if !ok {
        repondre(&mut s, "403 Forbidden", "text/plain; charset=utf-8", "Clé absente ou expirée : rescanne le QR code dans Outils.".as_bytes(), "");
        return;
    }
    if req.chemin.starts_with("/api/") && api(&mut s, &req, &mut lecteur, c) {
        return;
    }
    let cookie = format!("Set-Cookie: k={}; Path=/; Max-Age=31536000; SameSite=Strict\r\n", reg.cle);
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("GET", "/") => repondre(&mut s, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes(), &cookie),
        ("GET", "/etat") => {
            let mc = match c.mc.statut() {
                None => "arrêté".to_string(),
                Some((false, _)) => "démarrage…".to_string(),
                Some((true, j)) => format!("en ligne · {} joueur(s)", j.len()),
            };
            let fichiers: Vec<String> = reg.fichiers.iter().enumerate().map(|(i, p)| {
                let taille = p.metadata().map(|m| crate::disque::taille_lisible(m.len())).unwrap_or_else(|_| "introuvable".into());
                format!("{{\"i\":{i},\"nom\":{},\"taille\":{}}}", json_echappe(&p.file_name().unwrap_or_default().to_string_lossy()), json_echappe(&taille))
            }).collect();
            let corps = format!("{{\"mc\":{},\"fichiers\":[{}]}}", json_echappe(&mc), fichiers.join(","));
            repondre(&mut s, "200 OK", "application/json", corps.as_bytes(), "");
        }
        ("GET", f) if f.starts_with("/f/") => {
            let p = f[3..].parse::<usize>().ok().and_then(|i| reg.fichiers.get(i)).cloned();
            match p.and_then(|p| std::fs::File::open(&p).ok().map(|f| (p, f))) {
                Some((p, mut fi)) => {
                    let n = fi.metadata().map(|m| m.len()).unwrap_or(0);
                    let nom = p.file_name().unwrap_or_default().to_string_lossy().replace('"', "");
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {n}\r\nContent-Disposition: attachment; filename=\"{nom}\"\r\nConnection: close\r\n\r\n");
                    let _ = std::io::copy(&mut fi, &mut s);
                    noter(c, format!("📤 {nom} envoyé au téléphone"));
                }
                None => repondre(&mut s, "404 Not Found", "text/plain", b"introuvable", ""),
            }
        }
        ("POST", "/envoyer") => {
            let nom = nom_recu(req.param("nom").unwrap_or("fichier"));
            let _ = std::fs::create_dir_all(&reg.dossier_reception);
            let dest = libre(&reg.dossier_reception, &nom);
            let res = std::fs::File::create(&dest).and_then(|mut f| std::io::copy(&mut (&mut lecteur).take(req.longueur), &mut f));
            match res {
                Ok(n) if n == req.longueur => {
                    noter(c, format!("📥 {nom} reçu ({})", crate::disque::taille_lisible(n)));
                    crate::systeme::notifier("Téléphone", &format!("{nom} reçu dans {}", reg.dossier_reception.display()));
                    repondre(&mut s, "200 OK", "text/plain; charset=utf-8", "ok".as_bytes(), "");
                }
                _ => {
                    let _ = std::fs::remove_file(&dest);
                    repondre(&mut s, "500 Internal Server Error", "text/plain; charset=utf-8", "réception interrompue".as_bytes(), "");
                }
            }
        }
        ("GET", "/presse") => {
            let t = crate::tradinstant::win::lire_presse_papiers().unwrap_or_default();
            repondre(&mut s, "200 OK", "text/plain; charset=utf-8", t.as_bytes(), "");
        }
        ("POST", "/presse") => {
            let mut b = vec![];
            let _ = (&mut lecteur).take(req.longueur.min(1 << 20)).read_to_end(&mut b);
            let t = String::from_utf8_lossy(&b).into_owned();
            if crate::pressepapiers::win::ecrire_texte(&t, None, None) {
                noter(c, format!("📋 Texte reçu dans le presse-papiers ({} caractères)", t.chars().count()));
                repondre(&mut s, "200 OK", "text/plain; charset=utf-8", "Copié sur le PC ✔".as_bytes(), "");
            } else {
                repondre(&mut s, "500 Internal Server Error", "text/plain; charset=utf-8", "presse-papiers occupé".as_bytes(), "");
            }
        }
        ("POST", a) if a.starts_with("/cmd/") => match commande(c, &a[5..], req.param("texte")) {
            Ok(m) => {
                noter(c, format!("📱 {m}"));
                repondre(&mut s, "200 OK", "text/plain; charset=utf-8", m.as_bytes(), "");
            }
            Err(e) => repondre(&mut s, "400 Bad Request", "text/plain; charset=utf-8", e.as_bytes(), ""),
        },
        _ => repondre(&mut s, "404 Not Found", "text/plain", b"introuvable", ""),
    }
}

pub struct Telephone {
    r: Arc<Mutex<Reglages>>,
    mc: Pilote,
    marche: Arc<AtomicBool>,
    erreur: Arc<Mutex<Option<String>>>,
    journal: Arc<Mutex<Vec<String>>>,
    qr: Option<(String, egui::TextureHandle)>,
    cle_partagee: Arc<Mutex<Option<Arc<Mutex<String>>>>>,
    webcam: Option<(std::time::Instant, egui::TextureHandle)>,
}

impl Telephone {
    pub fn new(mc: Pilote) -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let auto = r.auto;
        let mut t = Telephone { r: Arc::new(Mutex::new(r)), mc, marche: Arc::default(), erreur: Arc::default(), journal: Arc::default(), qr: None, cle_partagee: Arc::default(), webcam: None };
        t.enregistrer(); // garde la clé générée
        if auto {
            t.demarrer();
        }
        t
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&*self.r.lock().unwrap()).unwrap_or_default());
    }

    fn demarrer(&mut self) {
        if self.marche.load(Relaxed) {
            return;
        }
        let port = self.r.lock().unwrap().port;
        let ecoute = match TcpListener::bind(("0.0.0.0", port)) {
            Ok(l) => l,
            Err(e) => {
                *self.erreur.lock().unwrap() = Some(format!("Port {port} indisponible : {e}"));
                return;
            }
        };
        let _ = ecoute.set_nonblocking(true);
        *self.erreur.lock().unwrap() = None;
        self.marche.store(true, Relaxed);
        let ctx = Arc::new(Contexte { r: self.r.clone(), mc: self.mc.clone(), journal: self.journal.clone() });
        let marche = self.marche.clone();
        decouverte(port, marche.clone());
        let cle = Arc::new(Mutex::new(self.r.lock().unwrap().cle.clone()));
        *self.cle_partagee.lock().unwrap() = Some(cle.clone());
        crate::pont::ecouter_entrees(cle.clone(), marche.clone());
        crate::pont::ecouter_son(cle, marche.clone());
        *crate::pont::distance().lock().unwrap() = self.r.lock().unwrap().distance.clone();
        let c2 = ctx.clone();
        crate::pont::ecouter_distance(Arc::new(move |cmd: &str| -> String {
            if cmd == "etat" {
                return etat_json(&c2).to_string();
            }
            if let Some(id) = cmd.strip_prefix("lancer ") {
                return match crate::jeux4::jeux_installes().into_iter().find(|j| j.id == id) {
                    Some(j) => {
                        use std::os::windows::process::CommandExt;
                        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &j.cmd]).current_dir(if j.dir.is_empty() { "." } else { &j.dir }).creation_flags(0x0800_0000).spawn();
                        noter(&c2, format!("🌍 Lancement de {} (à distance)", j.nom));
                        serde_json::json!({ "ok": format!("Lancement de {}", j.nom) }).to_string()
                    }
                    None => serde_json::json!({ "erreur": "jeu introuvable" }).to_string(),
                };
            }
            let (a, t) = cmd.split_once(' ').unwrap_or((cmd, ""));
            match commande(&c2, a, Some(t)) {
                Ok(m) => {
                    noter(&c2, format!("🌍 {m} (à distance)"));
                    serde_json::json!({ "ok": m }).to_string()
                }
                Err(e) => serde_json::json!({ "erreur": e }).to_string(),
            }
        }));
        std::thread::spawn(move || {
            while marche.load(Relaxed) {
                match ecoute.accept() {
                    Ok((s, _)) => {
                        let _ = s.set_nonblocking(false);
                        let c = ctx.clone();
                        std::thread::spawn(move || servir(s, &c));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(80)),
                }
            }
        });
    }

    fn adresse(&self) -> Option<String> {
        let r = self.r.lock().unwrap();
        ip_locale().map(|ip| format!("http://{ip}:{}/?k={}", r.port, r.cle))
    }

    pub fn resume(&self) -> String {
        if self.marche.load(Relaxed) { "Serveur actif (scanne le QR code)".into() } else { "Arrêté".into() }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Téléphone");
        ui.weak("Le téléphone doit être sur le même Wi-Fi que le PC. Au premier lancement, Windows demande d'autoriser Outils dans le pare-feu : accepte pour les réseaux privés.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        let marche = self.marche.load(Relaxed);
        ui.horizontal(|ui| {
            if marche {
                if ui.button("⏹ Arrêter").clicked() {
                    self.marche.store(false, Relaxed);
                }
                ui.colored_label(Color32::LIGHT_GREEN, "● en marche");
            } else if ui.button("▶ Démarrer").clicked() {
                self.demarrer();
            }
            ui.label("Port");
            ui.add_enabled(!marche, egui::DragValue::new(&mut r.port).range(1024..=65535));
            ui.checkbox(&mut r.auto, "Démarrer avec Outils");
            if ui.button("🔑 Nouvelle clé").on_hover_text("Les anciens QR codes ne marcheront plus").clicked() {
                r.cle = nouvelle_cle();
                self.qr = None;
                if let Some(c) = self.cle_partagee.lock().unwrap().as_ref() {
                    *c.lock().unwrap() = r.cle.clone();
                }
            }
        });
        if let Some(e) = self.erreur.lock().unwrap().clone() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if marche {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("📱 Appli Outils pour Android :");
                    let code = CODE.lock().unwrap().clone().filter(|(_, fin)| std::time::Instant::now() < *fin);
                    match code {
                        Some((c, fin)) => {
                            ui.label("dans l'appli, « Mon PC » › « Associer », puis tape le code");
                            ui.label(egui::RichText::new(format!("{} {}", &c[..3], &c[3..])).size(26.0).strong().monospace());
                            ui.weak(format!("(encore {} min)", (fin - std::time::Instant::now()).as_secs() / 60 + 1));
                        }
                        None => {
                            if ui.button("Associer un téléphone").clicked() {
                                nouveau_code();
                            }
                            ui.weak("ou scanne le QR code ci-dessous depuis l'appli.");
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button("🔔 Faire sonner le téléphone").on_hover_text("Il sonne fort, même en silencieux (dans les 10 secondes)").clicked() {
                        crate::pont::faire_sonner(&r.cle);
                        noter_ui(&self.journal, "🔔 Le téléphone va sonner".into());
                    }
                    if ui.button("📤 Envoyer des fichiers au téléphone…").clicked() {
                        for f in rfd::FileDialog::new().pick_files().unwrap_or_default() {
                            crate::pont::envoyer_au_tel(&f);
                            noter_ui(&self.journal, format!("📤 {} part vers le téléphone", f.file_name().unwrap_or_default().to_string_lossy()));
                        }
                    }
                    if r.distance.sujet.is_empty() {
                        r.distance = crate::pont::Distance::nouvelle();
                    }
                    ui.checkbox(&mut r.distance.actif, "🌍 Accès depuis l'extérieur").on_hover_text("Le téléphone pilote le PC et reçoit ses notifications même hors de la maison, par le relais ntfy.sh. Tout est chiffré avec une clé connue seulement du PC et du téléphone.");
                });
                // caméra et micro du téléphone
                let image = crate::pont::WEBCAM.lock().unwrap().clone().filter(|(_, t)| t.elapsed() < Duration::from_secs(3));
                if let Some((jpeg, quand)) = image {
                    if self.webcam.as_ref().is_none_or(|(q, _)| *q != quand) {
                        if let Ok(i) = image::load_from_memory(&jpeg) {
                            let i = i.to_rgba8();
                            let ci = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
                            match &mut self.webcam {
                                Some((q, t)) => {
                                    t.set(ci, Default::default());
                                    *q = quand;
                                }
                                None => self.webcam = Some((quand, ui.ctx().load_texture("webcam_tel", ci, Default::default()))),
                            }
                        }
                    }
                    if let Some((_, t)) = &self.webcam {
                        ui.label("📷 Caméra du téléphone (capture cette fenêtre dans OBS pour t'en servir comme webcam) :");
                        ui.add(egui::Image::new(t).max_width(480.0));
                    }
                    ui.ctx().request_repaint_after(Duration::from_millis(80));
                }
                ui.horizontal_wrapped(|ui| {
                    let mut webcam = crate::webcam_virtuelle::ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
                    if ui.checkbox(&mut webcam, "📹 Vraie webcam pour Windows (Discord, Zoom, Teams, OBS)").on_hover_text("La caméra du téléphone devient la webcam « DirectShow Softcam ». Ouvre « Webcam et micro pour le PC » dans l'appli du téléphone.").changed() {
                        crate::webcam_virtuelle::activer(webcam);
                    }
                    let e = crate::webcam_virtuelle::ETAT.lock().unwrap().clone();
                    if !e.is_empty() {
                        ui.label(e);
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("🎮 Vraie manette Xbox :");
                    let e = crate::manette_virtuelle::ETAT.lock().unwrap().clone();
                    ui.weak(if e.is_empty() { "dans l'appli, Manette pour le PC › profil « Vraie manette Xbox »".to_string() } else { e });
                    if ui.small_button("Installer le pilote ViGEmBus").on_hover_text("Nécessaire une seule fois pour que Windows voie la manette").clicked() {
                        let j = self.journal.clone();
                        std::thread::spawn(move || {
                            let m = match crate::manette_virtuelle::installer_pilote() {
                                Ok(_) => "✔ Pilote de manette installé".to_string(),
                                Err(e) => format!("✖ Pilote de manette : {e}"),
                            };
                            noter_ui(&j, m);
                        });
                    }
                });
                if crate::pont::MICRO_ACTIF.lock().unwrap().is_some() {
                    ui.colored_label(Color32::LIGHT_GREEN, "🎙 Le micro du téléphone est envoyé dans le câble virtuel : choisis « CABLE Output » comme micro dans Discord ou OBS.");
                }
            });
        }
        ui.columns(2, |c| {
            let ui = &mut c[0];
            if marche {
                match self.adresse() {
                    Some(url) => {
                        if self.qr.as_ref().is_none_or(|(u, _)| u != &url) {
                            if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
                                let w = code.width();
                                let marge = 4;
                                let n = w + 2 * marge;
                                let couleurs = code.to_colors();
                                let mut px = vec![255u8; n * n];
                                for y in 0..w {
                                    for x in 0..w {
                                        if couleurs[y * w + x] == qrcode::Color::Dark {
                                            px[(y + marge) * n + x + marge] = 0;
                                        }
                                    }
                                }
                                let img = egui::ColorImage::from_gray([n, n], &px);
                                let t = ui.ctx().load_texture("qr", img, egui::TextureOptions::NEAREST);
                                self.qr = Some((url.clone(), t));
                            }
                        }
                        if let Some((_, t)) = &self.qr {
                            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(240.0, 240.0)));
                        }
                        ui.label("Scanne avec l'appareil photo du téléphone, ou ouvre :");
                        ui.monospace(&url);
                        if ui.small_button("Copier l'adresse").clicked() {
                            ui.ctx().copy_text(url);
                        }
                    }
                    None => {
                        ui.colored_label(Color32::LIGHT_RED, "Pas de réseau local détecté.");
                    }
                }
            } else {
                ui.weak("Démarre le serveur pour afficher le QR code.");
            }
            let ui = &mut c[1];
            ui.strong("Fichiers à récupérer sur le téléphone");
            let mut retirer = None;
            for (i, p) in r.fichiers.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    if !p.exists() {
                        ui.colored_label(Color32::LIGHT_RED, "introuvable");
                    }
                    if ui.small_button("✖").clicked() {
                        retirer = Some(i);
                    }
                });
            }
            if let Some(i) = retirer {
                r.fichiers.remove(i);
            }
            if ui.button("➕ Ajouter des fichiers…").clicked() {
                if let Some(v) = rfd::FileDialog::new().pick_files() {
                    r.fichiers.extend(v);
                }
            }
            let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
            r.fichiers.extend(deposes.into_iter().filter(|p| p.is_file()));
            ui.add_space(8.0);
            ui.strong("Fichiers reçus du téléphone");
            ui.horizontal(|ui| {
                ui.monospace(r.dossier_reception.display().to_string());
                if ui.small_button("Changer…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        r.dossier_reception = d;
                    }
                }
                if ui.small_button("📂").clicked() {
                    let _ = std::fs::create_dir_all(&r.dossier_reception);
                    let _ = std::process::Command::new("explorer").arg(&r.dossier_reception).spawn();
                }
            });
            ui.add_space(8.0);
            ui.strong("Activité");
            for l in self.journal.lock().unwrap().iter().take(12) {
                ui.label(l);
            }
        });
        if marche {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        if r != avant {
            *crate::pont::distance().lock().unwrap() = r.distance.clone();
            *self.r.lock().unwrap() = r;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requetes() {
        let brut = b"POST /envoyer?nom=photo%20%C3%A9t%C3%A9.jpg&k=abc HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\nCookie: a=1; k=zzz\r\n\r\nhello";
        let mut r = BufReader::new(&brut[..]);
        let q = lire_requete(&mut r).unwrap();
        assert_eq!(q.methode, "POST");
        assert_eq!(q.chemin, "/envoyer");
        assert_eq!(q.param("nom"), Some("photo été.jpg"));
        assert_eq!(q.param("k"), Some("abc"));
        assert_eq!(q.longueur, 5);
        assert_eq!(q.cookie_cle.as_deref(), Some("zzz"));
        let mut corps = String::new();
        r.read_to_string(&mut corps).unwrap();
        assert_eq!(corps, "hello");
    }

    #[test]
    fn noms_surs() {
        assert_eq!(nom_recu("../../Windows/evil.exe"), "evil.exe");
        assert_eq!(nom_recu("C:\\a\\b:c.txt"), "b_c.txt");
        assert_eq!(nom_recu(".."), "fichier");
        assert_eq!(decoder_url("a%2"), "a%2");
    }

    #[test]
    fn cles() {
        let (a, b) = (nouvelle_cle(), nouvelle_cle());
        assert_eq!(a.len(), 14);
        assert_ne!(a, b);
    }
}
