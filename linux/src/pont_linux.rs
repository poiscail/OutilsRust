//! Lien avec le téléphone sous Linux : même protocole que la version Windows, l'appli Android marche
//! telle quelle (télécommande, état, jeux, fichiers, presse-papiers, Minecraft, notifications,
//! synchronisation, manette et souris).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const PORT: u16 = 8642;
const PORT_DECOUVERTE: u16 = 8643;
const PORT_ENTREES: u16 = 8644;
const SYNCHRO: [&str; 4] = ["agenda", "ecole", "rebours", "sante"];

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Reglages {
    pub cle: String,
    pub auto: bool,
    pub reception: PathBuf,
}

fn dossier_telechargements() -> PathBuf {
    Command::new("xdg-user-dir").arg("DOWNLOAD").output().ok().map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim())).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Téléchargements"))
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { cle: nouvelle_cle(), auto: true, reception: dossier_telechargements().join("Depuis le téléphone") }
    }
}

pub fn nouvelle_cle() -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut b = [0u8; 14];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| A[*x as usize % A.len()] as char).collect()
}

// ---------- notifications vers le téléphone ----------

static NOTIFS: Mutex<VecDeque<(u64, String, String)>> = Mutex::new(VecDeque::new());
static NUMERO: AtomicU64 = AtomicU64::new(0);

pub fn pousser_notif(titre: &str, texte: &str) {
    let n = NUMERO.fetch_add(1, Relaxed) + 1;
    let mut q = NOTIFS.lock().unwrap();
    q.push_back((n, titre.to_string(), texte.to_string()));
    while q.len() > 50 {
        q.pop_front();
    }
}

// ---------- code d'appairage ----------

static CODE: Mutex<Option<(String, Instant)>> = Mutex::new(None);

fn nouveau_code() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::getrandom(&mut b);
    let c = format!("{:06}", u32::from_le_bytes(b) % 1_000_000);
    *CODE.lock().unwrap() = Some((c.clone(), Instant::now() + Duration::from_secs(600)));
    c
}

fn code_valide(c: &str) -> bool {
    let mut g = CODE.lock().unwrap();
    match &*g {
        Some((code, fin)) if Instant::now() < *fin && code == c.trim() => {
            *g = None;
            true
        }
        _ => false,
    }
}

// ---------- requêtes HTTP ----------

pub fn decoder_url(s: &str) -> String {
    crate::secu_mdp::decoder_url(s)
}

pub struct Requete {
    pub methode: String,
    pub chemin: String,
    pub params: Vec<(String, String)>,
    pub longueur: u64,
}

impl Requete {
    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a == k).map(|(_, b)| b.as_str())
    }
}

pub fn lire_requete(r: &mut impl BufRead) -> Option<Requete> {
    let mut l = String::new();
    r.read_line(&mut l).ok()?;
    let mut p = l.split_whitespace();
    let methode = p.next()?.to_string();
    let cible = p.next()?.to_string();
    let (chemin, q) = cible.split_once('?').unwrap_or((&cible, ""));
    let params = q.split('&').filter(|x| !x.is_empty()).map(|x| {
        let (a, b) = x.split_once('=').unwrap_or((x, ""));
        (decoder_url(a), decoder_url(b))
    }).collect();
    let mut longueur = 0;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                longueur = v.trim().parse().unwrap_or(0);
            }
        }
    }
    Some(Requete { methode, chemin: decoder_url(chemin), params, longueur })
}

fn repondre(s: &mut TcpStream, code: &str, type_: &str, corps: &[u8]) {
    let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: {type_}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n", corps.len());
    let _ = s.write_all(corps);
}

fn json(s: &mut TcpStream, v: serde_json::Value) {
    repondre(s, "200 OK", "application/json; charset=utf-8", v.to_string().as_bytes());
}

fn texte(s: &mut TcpStream, code: &str, t: &str) {
    repondre(s, code, "text/plain; charset=utf-8", t.as_bytes());
}

/// Nom de fichier reçu sans dossier ni caractère interdit.
pub fn nom_recu(n: &str) -> String {
    let base = n.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base.chars().map(|c| if c.is_control() || c == '/' { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "fichier".into() } else { s }
}

fn libre(d: &Path, nom: &str) -> PathBuf {
    let p = d.join(nom);
    if !p.exists() {
        return p;
    }
    let (b, e) = nom.rsplit_once('.').map(|(b, e)| (b.to_string(), format!(".{e}"))).unwrap_or((nom.to_string(), String::new()));
    (2..).map(|i| d.join(format!("{b} ({i}){e}"))).find(|p| !p.exists()).unwrap()
}

fn existe(prog: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| Path::new(d).join(prog).is_file())
}

fn lancer(prog: &str, args: &[&str]) -> bool {
    Command::new(prog).args(args).status().map(|s| s.success()).unwrap_or(false)
}

// ---------- son, médias, presse-papiers ----------

fn volume(action: &str) -> bool {
    if existe("wpctl") {
        let a = match action {
            "plus" => vec!["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"],
            "moins" => vec!["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"],
            _ => vec!["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"],
        };
        return lancer("wpctl", &a);
    }
    if existe("pactl") {
        let a = match action {
            "plus" => vec!["set-sink-volume", "@DEFAULT_SINK@", "+5%"],
            "moins" => vec!["set-sink-volume", "@DEFAULT_SINK@", "-5%"],
            _ => vec!["set-sink-mute", "@DEFAULT_SINK@", "toggle"],
        };
        return lancer("pactl", &a);
    }
    let a = match action {
        "plus" => vec!["-q", "set", "Master", "5%+"],
        "moins" => vec!["-q", "set", "Master", "5%-"],
        _ => vec!["-q", "set", "Master", "toggle"],
    };
    lancer("amixer", &a)
}

fn media(a: &str) -> bool {
    existe("playerctl") && lancer("playerctl", &[a])
}

fn wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
}

pub fn lire_presse() -> String {
    let o = if wayland() && existe("wl-paste") { Command::new("wl-paste").arg("-n").output() } else { Command::new("xclip").args(["-o", "-selection", "clipboard"]).output() };
    o.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
}

pub fn ecrire_presse(t: &str) -> bool {
    let c = if wayland() && existe("wl-copy") { Command::new("wl-copy").stdin(std::process::Stdio::piped()).spawn() } else { Command::new("xclip").args(["-selection", "clipboard"]).stdin(std::process::Stdio::piped()).spawn() };
    match c {
        Ok(mut p) => {
            let ok = p.stdin.take().is_some_and(|mut i| i.write_all(t.as_bytes()).is_ok());
            ok
        }
        Err(_) => false,
    }
}

pub use crate::pratique::url_autorisee;

fn commande(action: &str, t: Option<&str>) -> Result<String, String> {
    let ok = |b: bool, m: &str| if b { Ok(m.to_string()) } else { Err(format!("{m} : impossible (outil manquant ?)")) };
    match action {
        "vol_plus" => ok(volume("plus"), "Volume +"),
        "vol_moins" => ok(volume("moins"), "Volume −"),
        "muet" => ok(volume("muet"), "Muet"),
        "lecture" => ok(media("play-pause"), "Lecture / pause"),
        "suivant" => ok(media("next"), "Piste suivante"),
        "precedent" => ok(media("previous"), "Piste précédente"),
        "verrouiller" => ok(lancer("loginctl", &["lock-session"]), "PC verrouillé"),
        "veille" => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(1));
                let _ = Command::new("systemctl").arg("suspend").status();
            });
            Ok("Mise en veille".into())
        }
        "mc_demarrer" => crate::mc_linux::demarrer().map(|_| "Démarrage du serveur Minecraft".into()),
        "mc_arreter" => {
            crate::mc_linux::arreter();
            Ok("Arrêt du serveur Minecraft".into())
        }
        "mc_sauvegarder" => crate::mc_linux::sauvegarder().map(|_| "Monde sauvegardé".into()),
        "mc_dire" => ok(crate::mc_linux::commande(&format!("say {}", t.unwrap_or(""))), "Message envoyé"),
        "ouvrir" => {
            let u = t.unwrap_or("");
            if url_autorisee(u) {
                ok(Command::new("xdg-open").arg(u).spawn().is_ok(), "Ouverture")
            } else {
                Err("site non autorisé".into())
            }
        }
        _ => Err("commande inconnue".into()),
    }
}

// ---------- état, jeux, fichiers ----------

fn etat_json() -> serde_json::Value {
    let lire = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let (total, dispo) = crate::systeme_linux::memoire(&lire("/proc/meminfo"));
    static CPU: Mutex<(u64, u64, f32)> = Mutex::new((0, 0, 0.0));
    let (a, t) = crate::systeme_linux::temps_cpu(&lire("/proc/stat"));
    let cpu = {
        let mut g = CPU.lock().unwrap();
        if g.1 > 0 && t > g.1 {
            g.2 = (a - g.0) as f32 / (t - g.1) as f32 * 100.0;
        }
        *g = (a, t, g.2);
        g.2
    };
    let (mc, joueurs) = match crate::mc_linux::statut() {
        None => ("arrêté", vec![]),
        Some((false, _)) => ("démarrage", vec![]),
        Some((true, j)) => ("en ligne", j),
    };
    let jeu = crate::lanceur_linux::JEU_EN_COURS.lock().unwrap().clone().and_then(|id| crate::lanceur_linux::jeux_installes().into_iter().find(|j| j.id == id).map(|j| j.nom));
    let macs: Vec<String> = std::fs::read_dir("/sys/class/net").into_iter().flatten().flatten().filter(|e| e.file_name() != "lo").filter_map(|e| std::fs::read_to_string(e.path().join("address")).ok()).map(|m| m.trim().to_uppercase()).filter(|m| m.len() == 17 && m != "00:00:00:00:00:00").collect();
    serde_json::json!({
        "nom": nom_pc(), "version": env!("CARGO_PKG_VERSION"),
        "cpu": cpu, "cpu_t": -1.0, "gpu": -1.0, "gpu_t": -1.0, "ram": if total > 0 { (total - dispo) as f64 * 100.0 / total as f64 } else { -1.0 }, "fps": -1.0, "jeu": jeu,
        "mc": mc, "joueurs": joueurs, "macs": macs,
    })
}

fn nom_pc() -> String {
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).unwrap_or_else(|_| "PC Linux".into())
}

/// Index des noms de fichiers du dossier personnel (pour la recherche depuis le téléphone).
static INDEX: Mutex<Option<(Instant, Vec<(String, u64, u64, bool)>)>> = Mutex::new(None);

fn chercher(q: &str) -> Vec<(String, u64, u64, bool)> {
    let vieux = INDEX.lock().unwrap().as_ref().is_none_or(|x| x.0.elapsed() > Duration::from_secs(600));
    if vieux {
        let mut v = vec![];
        let mut pile = vec![PathBuf::from(std::env::var("HOME").unwrap_or_default())];
        while let Some(d) = pile.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.starts_with('.') || n == "node_modules" || n == "target" {
                    continue;
                }
                let Ok(m) = e.metadata() else { continue };
                let date = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                if m.is_dir() {
                    pile.push(e.path());
                }
                v.push((e.path().display().to_string(), m.len(), date, m.is_dir()));
                if v.len() > 400_000 {
                    break;
                }
            }
        }
        *INDEX.lock().unwrap() = Some((Instant::now(), v));
    }
    let mots: Vec<String> = q.to_lowercase().split_whitespace().map(String::from).collect();
    let g = INDEX.lock().unwrap();
    let mut r: Vec<(String, u64, u64, bool)> = g.as_ref().map(|x| x.1.iter().filter(|(p, ..)| {
        let n = p.rsplit('/').next().unwrap_or("").to_lowercase();
        !mots.is_empty() && mots.iter().all(|m| n.contains(m.as_str()))
    }).take(200).cloned().collect()).unwrap_or_default();
    r.sort_by_key(|x| std::cmp::Reverse(x.2));
    r.truncate(60);
    r
}

fn api(s: &mut TcpStream, req: &Requete, lecteur: &mut BufReader<TcpStream>) -> bool {
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("GET", "/api/etat") => json(s, etat_json()),
        ("GET", "/api/trouver") => json(s, crate::sans_dossiers::trouver_json(req.param("q").unwrap_or(""))),
        ("GET", "/api/agent/plan") => json(s, crate::agent::plan_json(req.param("texte").unwrap_or(""))),
        ("POST", "/api/agent/lancer") => match crate::agent::lancer_distance(req.param("plan").unwrap_or("")) {
            Ok(t) => repondre(s, "200 OK", "text/plain; charset=utf-8", t.as_bytes()),
            Err(t) => repondre(s, "400 Bad Request", "text/plain; charset=utf-8", t.as_bytes()),
        },
        ("GET", "/api/agent/suivi") => json(s, crate::agent::suivi_json()),
        ("POST", "/api/agent/annuler") => repondre(s, "200 OK", "text/plain; charset=utf-8", crate::agent::annuler_distance().as_bytes()),
        ("GET", "/api/jeux") => {
            let v: Vec<serde_json::Value> = crate::lanceur_linux::jeux_installes().iter().map(|j| serde_json::json!({ "id": j.id, "nom": j.nom })).collect();
            json(s, serde_json::Value::Array(v));
        }
        ("POST", "/api/lancer") => {
            let id = req.param("id").unwrap_or("");
            match crate::lanceur_linux::jeux_installes().into_iter().find(|j| j.id == id) {
                Some(j) => {
                    crate::lanceur_linux::lancer(&j);
                    texte(s, "200 OK", &format!("Lancement de {}", j.nom));
                }
                None => texte(s, "404 Not Found", "jeu introuvable"),
            }
        }
        ("GET", "/api/chercher") => {
            let v: Vec<serde_json::Value> = chercher(req.param("q").unwrap_or("")).into_iter().map(|(p, t, d, dos)| serde_json::json!({ "chemin": p, "nom": p.rsplit('/').next().unwrap_or(""), "taille": t, "date": d, "dossier": dos })).collect();
            let n = INDEX.lock().unwrap().as_ref().map_or(0, |x| x.1.len());
            json(s, serde_json::json!({ "total": v.len(), "liste": v, "indexes": n }));
        }
        ("GET", "/api/fichier") => {
            let p = PathBuf::from(req.param("chemin").unwrap_or(""));
            let maison = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            match (p.starts_with(&maison) && !p.components().any(|c| c.as_os_str() == ".."), std::fs::File::open(&p)) {
                (true, Ok(mut f)) => {
                    let n = f.metadata().map(|m| m.len()).unwrap_or(0);
                    let nom = p.file_name().unwrap_or_default().to_string_lossy().replace('"', "");
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {n}\r\nContent-Disposition: attachment; filename=\"{nom}\"\r\nConnection: close\r\n\r\n");
                    let _ = std::io::copy(&mut f, s);
                }
                _ => texte(s, "404 Not Found", "introuvable (seuls les fichiers du dossier personnel sont accessibles)"),
            }
        }
        ("GET", "/api/pour_tel") => json(s, serde_json::json!([])),
        ("GET", "/api/distance") => json(s, serde_json::json!({ "actif": false, "sujet": "", "cle": "" })),
        ("GET", "/api/notifs") => {
            let depuis: u64 = req.param("depuis").and_then(|x| x.parse().ok()).unwrap_or(0);
            let q = NOTIFS.lock().unwrap();
            let v: Vec<serde_json::Value> = q.iter().filter(|x| x.0 > depuis).map(|(n, t, m)| serde_json::json!({ "n": n, "titre": t, "texte": m })).collect();
            json(s, serde_json::json!({ "dernier": NUMERO.load(Relaxed), "liste": v }));
        }
        ("GET", "/api/synchro") | ("POST", "/api/synchro") => {
            let nom = req.param("nom").unwrap_or("");
            if !SYNCHRO.contains(&nom) {
                texte(s, "400 Bad Request", "nom inconnu");
                return true;
            }
            let f = crate::outil::chemin(nom);
            let date = |f: &Path| f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            if req.methode == "GET" {
                let c = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or(serde_json::Value::Null);
                json(s, serde_json::json!({ "t": date(&f), "contenu": c }));
            } else {
                let mut b = vec![];
                let _ = lecteur.take(req.longueur.min(8 << 20)).read_to_end(&mut b);
                if serde_json::from_slice::<serde_json::Value>(&b).is_err() {
                    texte(s, "400 Bad Request", "json invalide");
                    return true;
                }
                let _ = std::fs::create_dir_all(f.parent().unwrap());
                match std::fs::write(&f, &b) {
                    Ok(()) => {
                        crate::outil::recharger_synchro(nom);
                        json(s, serde_json::json!({ "t": date(&f) }));
                    }
                    Err(e) => texte(s, "500 Internal Server Error", &e.to_string()),
                }
            }
        }
        _ => return false,
    }
    true
}

fn servir(mut s: TcpStream, journal: &Mutex<Vec<String>>) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let Ok(c) = s.try_clone() else { return };
    let mut lecteur = BufReader::new(c);
    let Some(req) = lire_requete(&mut lecteur) else { return };
    let r: Reglages = charger("telephone_linux");
    let noter = |m: String| {
        let mut j = journal.lock().unwrap();
        j.insert(0, m);
        j.truncate(30);
    };
    if req.chemin == "/api/appairer" {
        if req.param("code").is_some_and(code_valide) {
            noter(format!("📱 Téléphone associé ({})", req.param("appareil").unwrap_or("Android")));
            json(&mut s, serde_json::json!({ "cle": r.cle, "nom": nom_pc() }));
        } else {
            std::thread::sleep(Duration::from_millis(800));
            texte(&mut s, "403 Forbidden", "Code faux ou expiré : regarde le code affiché dans Outils sur le PC.");
        }
        return;
    }
    if req.param("k") != Some(r.cle.as_str()) {
        texte(&mut s, "403 Forbidden", "Clé absente ou expirée : associe à nouveau le téléphone.");
        return;
    }
    if req.chemin.starts_with("/api/") && api(&mut s, &req, &mut lecteur) {
        return;
    }
    match (req.methode.as_str(), req.chemin.as_str()) {
        ("POST", "/envoyer") => {
            let nom = nom_recu(req.param("nom").unwrap_or("fichier"));
            let _ = std::fs::create_dir_all(&r.reception);
            let dest = libre(&r.reception, &nom);
            let res = std::fs::File::create(&dest).and_then(|mut f| std::io::copy(&mut (&mut lecteur).take(req.longueur), &mut f));
            match res {
                Ok(n) if n == req.longueur => {
                    noter(format!("📥 {nom} reçu"));
                    crate::systeme::notifier("Téléphone", &format!("{nom} reçu dans {}", r.reception.display()));
                    texte(&mut s, "200 OK", "ok");
                }
                _ => {
                    let _ = std::fs::remove_file(&dest);
                    texte(&mut s, "500 Internal Server Error", "réception interrompue");
                }
            }
        }
        ("GET", "/presse") => texte(&mut s, "200 OK", &lire_presse()),
        ("POST", "/presse") => {
            let mut b = vec![];
            let _ = (&mut lecteur).take(req.longueur.min(1 << 20)).read_to_end(&mut b);
            if ecrire_presse(&String::from_utf8_lossy(&b)) {
                noter("📋 Texte reçu dans le presse-papiers".into());
                texte(&mut s, "200 OK", "Copié sur le PC ✔");
            } else {
                texte(&mut s, "500 Internal Server Error", "presse-papiers indisponible (installe wl-clipboard ou xclip)");
            }
        }
        ("POST", a) if a.starts_with("/cmd/") => match commande(&a[5..], req.param("texte")) {
            Ok(m) => {
                noter(format!("📱 {m}"));
                texte(&mut s, "200 OK", &m);
            }
            Err(e) => texte(&mut s, "400 Bad Request", &e),
        },
        _ => texte(&mut s, "404 Not Found", "introuvable"),
    }
}

// ---------- manette et souris (xdotool sous X11, ydotool sous Wayland) ----------

/// Nom de touche xdotool pour un nom envoyé par le téléphone.
pub fn touche_x(n: &str) -> Option<String> {
    Some(match n {
        "haut" => "Up".into(),
        "bas" => "Down".into(),
        "gauche" => "Left".into(),
        "droite" => "Right".into(),
        "espace" => "space".into(),
        "entree" => "Return".into(),
        "echap" => "Escape".into(),
        "tab" => "Tab".into(),
        "retour" => "BackSpace".into(),
        "maj" => "shift".into(),
        "ctrl" => "ctrl".into(),
        "alt" => "alt".into(),
        "win" => "super".into(),
        "suppr" => "Delete".into(),
        x if x.len() == 1 && x.chars().all(|c| c.is_ascii_alphanumeric()) => x.to_string(),
        x if x.starts_with('f') && x[1..].parse::<u8>().is_ok_and(|k| (1..=12).contains(&k)) => x.to_uppercase(),
        _ => return None,
    })
}

/// Commande xdotool pour une entrée du téléphone (« k espace down », « m 5 -3 », « c g », « w 2 », « t texte »).
pub fn xdotool(c: &str) -> Option<Vec<String>> {
    let (cmd, reste) = c.split_once(' ').unwrap_or((c, ""));
    let mut p = reste.split_whitespace();
    let b = |s: Option<&str>| match s {
        Some("d") => "3",
        Some("m") => "2",
        _ => "1",
    };
    Some(match cmd {
        "m" => vec!["mousemove_relative".into(), "--".into(), p.next()?.into(), p.next()?.into()],
        "c" => vec!["click".into(), b(p.next()).into()],
        "b" => {
            let bt = b(p.next());
            vec![if p.next()? == "down" { "mousedown".into() } else { "mouseup".into() }, bt.into()]
        }
        "w" => {
            let n: i32 = p.next()?.parse().ok()?;
            vec!["click".into(), "--repeat".into(), n.unsigned_abs().min(20).to_string(), if n > 0 { "4".into() } else { "5".into() }]
        }
        "t" => vec!["type".into(), "--".into(), reste.to_string()],
        "k" => {
            let t = touche_x(p.next()?)?;
            match p.next() {
                Some("down") => vec!["keydown".into(), t],
                Some("up") => vec!["keyup".into(), t],
                _ => vec!["key".into(), t],
            }
        }
        _ => return None,
    })
}

fn ecouter_entrees(cle: String, marche: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_ENTREES)) else { return };
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut b = [0u8; 2048];
        let outil = "xdotool"; // sous Wayland, ne pilote que les fenêtres XWayland (jeux Steam/Proton compris)
        while marche.load(Relaxed) {
            let Ok((n, _)) = s.recv_from(&mut b) else { continue };
            let t = String::from_utf8_lossy(&b[..n]).to_string();
            let Some((k, cmds)) = t.split_once('|') else { continue };
            if k != cle {
                continue;
            }
            for c in cmds.split('\n') {
                if let Some(a) = xdotool(c) {
                    let _ = Command::new(outil).args(&a).status();
                }
            }
        }
    });
}

fn decouverte(marche: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(s) = UdpSocket::bind(("0.0.0.0", PORT_DECOUVERTE)) else { return };
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut b = [0u8; 64];
        while marche.load(Relaxed) {
            if let Ok((n, de)) = s.recv_from(&mut b) {
                if &b[..n] == b"OUTILS?" {
                    let r = serde_json::json!({ "outils": 1, "port": PORT, "nom": nom_pc(), "version": env!("CARGO_PKG_VERSION") });
                    let _ = s.send_to(r.to_string().as_bytes(), de);
                }
            }
        }
    });
}

struct Serveur {
    marche: Arc<AtomicBool>,
    journal: Arc<Mutex<Vec<String>>>,
    erreur: Mutex<Option<String>>,
}

fn serveur() -> &'static Serveur {
    static S: OnceLock<Serveur> = OnceLock::new();
    S.get_or_init(|| Serveur { marche: Arc::default(), journal: Arc::default(), erreur: Mutex::new(None) })
}

pub fn demarrer() {
    let s = serveur();
    if s.marche.swap(true, Relaxed) {
        return;
    }
    let r: Reglages = charger("telephone_linux");
    enregistrer("telephone_linux", &r);
    match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => {
            *s.erreur.lock().unwrap() = None;
            let (m, j) = (s.marche.clone(), s.journal.clone());
            std::thread::spawn(move || {
                for c in l.incoming().flatten() {
                    if !m.load(Relaxed) {
                        break;
                    }
                    let j = j.clone();
                    std::thread::spawn(move || servir(c, &j));
                }
            });
            decouverte(s.marche.clone());
            ecouter_entrees(r.cle, s.marche.clone());
        }
        Err(e) => {
            *s.erreur.lock().unwrap() = Some(format!("Port {PORT} indisponible : {e}"));
            s.marche.store(false, Relaxed);
        }
    }
}

pub struct Telephone {
    code: Option<String>,
    lance: bool,
}

impl Telephone {
    pub fn new() -> Self {
        Telephone { code: None, lance: false }
    }
}

impl Outil for Telephone {
    fn nom(&self) -> &'static str {
        "📱 Téléphone"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if !self.lance {
            self.lance = true;
            let r: Reglages = charger("telephone_linux");
            if r.auto {
                demarrer();
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Lien avec le téléphone");
        ui.weak("Avec l'appli Outils pour Android : télécommande, état du PC, jeux, fichiers, presse-papiers, Minecraft, notifications, manette et souris. Le téléphone et le PC doivent être sur le même Wi-Fi.");
        let s = serveur();
        let marche = s.marche.load(Relaxed);
        let mut r: Reglages = charger("telephone_linux");
        ui.horizontal(|ui| {
            if marche {
                ui.label(RichText::new("● Actif").color(Color32::LIGHT_GREEN).strong());
            } else if ui.button("▶ Démarrer").clicked() {
                demarrer();
            }
            if ui.checkbox(&mut r.auto, "Démarrer avec Outils").changed() {
                enregistrer("telephone_linux", &r);
            }
        });
        if let Some(e) = s.erreur.lock().unwrap().clone() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        ui.separator();
        ui.strong("Associer un téléphone");
        ui.label("Dans l'appli : PC et réseau › Mon PC › « Chercher mon PC sur le Wi-Fi », puis tape ce code :");
        match &self.code {
            Some(c) => {
                ui.label(RichText::new(c).size(36.0).strong().monospace());
                ui.weak("Valable 10 minutes, une seule fois.");
            }
            None => {
                if ui.button("Afficher un code").clicked() {
                    self.code = Some(nouveau_code());
                }
            }
        }
        if let Some(ip) = crate::telephone::ip_locale() {
            ui.weak(format!("Adresse du PC : {ip}:{PORT}"));
        }
        ui.weak(format!("Fichiers reçus : {}", r.reception.display()));
        let manque: Vec<&str> = [("playerctl", "lecture / pause"), ("xdotool", "manette et souris (X11)"), ("wl-copy", "presse-papiers (Wayland)")].iter().filter(|(p, _)| !existe(p)).map(|x| x.1).collect();
        if !manque.is_empty() {
            ui.colored_label(Color32::from_rgb(255, 200, 100), format!("Pour tout faire marcher, installe aussi : {} (paquets playerctl, xdotool, wl-clipboard).", manque.join(", ")));
        }
        ui.separator();
        ui.strong("Activité");
        for l in s.journal.lock().unwrap().iter() {
            ui.label(l);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
    fn resume(&self) -> Option<String> {
        serveur().marche.load(Relaxed).then(|| "Prêt pour le téléphone".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrees() {
        assert_eq!(xdotool("k espace down").unwrap(), vec!["keydown", "space"]);
        assert_eq!(xdotool("k w").unwrap(), vec!["key", "w"]);
        assert_eq!(xdotool("m 5 -3").unwrap(), vec!["mousemove_relative", "--", "5", "-3"]);
        assert_eq!(xdotool("c d").unwrap(), vec!["click", "3"]);
        assert_eq!(xdotool("w -2").unwrap(), vec!["click", "--repeat", "2", "5"]);
        assert!(xdotool("k ;rm").is_none());
        assert!(url_autorisee("https://www.youtube.com/watch?v=x"));
        assert!(!url_autorisee("https://www.youtube.com.evil.fr"));
        assert_eq!(nom_recu("../../etc/passwd"), "passwd");
        let mut c = std::io::Cursor::new(b"POST /cmd/ouvrir?k=abc&texte=https%3A%2F%2Fwww.netflix.com HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_vec());
        let r = lire_requete(&mut c).unwrap();
        assert_eq!(r.param("texte"), Some("https://www.netflix.com"));
    }
}
