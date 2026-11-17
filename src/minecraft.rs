//! Serveur Minecraft : lance le serveur Java (console, joueurs), sauvegardes du monde, mods, import de
//! modpack, mise en ligne playit.gg, mode veille, partage du modpack (packwiz) et kit pour les amis.
//! Même config que l'ancienne version : %APPDATA%\GestionServeurMinecraft.json.
use eframe::egui;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LIGNES: usize = 2000;
const UA: &str = "GestionServeurMinecraft/1.2";
type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn appdata() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default()
}

// ---------- config ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    java: String,
    jar: String,
    ram: String,
    auto: String,
    veille: bool,
    idle: String,
    adresse: String,
    pack_url: String,
    en_ligne: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { java: String::new(), jar: String::new(), ram: "2048".into(), auto: "0".into(), veille: false,
            idle: "0".into(), adresse: String::new(), pack_url: String::new(), en_ligne: false }
    }
}

fn fichier_config() -> PathBuf {
    appdata().join("GestionServeurMinecraft.json")
}

// ---------- serveur : fichiers ----------

pub fn java_par_defaut() -> String {
    let jdks = PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".jdks");
    let mut v: Vec<PathBuf> = fs::read_dir(jdks).into_iter().flatten().flatten()
        .map(|e| e.path().join("bin").join("java.exe")).filter(|p| p.is_file()).collect();
    v.sort();
    v.last().map(|p| p.display().to_string()).unwrap_or_else(|| "java".into())
}

static JOIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]: (\w+) joined the game").unwrap());
static LEAVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]: (\w+) left the game").unwrap());
static CLAIM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https://playit\.gg/claim/\S+").unwrap());
static ADRESSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[\w-]+(?:\.[\w-]+)*\.(?:joinmc\.link|playit\.gg|ply\.gg)(?::\d+)?\b").unwrap());

fn maj_joueurs(joueurs: &mut BTreeSet<String>, ligne: &str) {
    if let Some(m) = JOIN.captures(ligne) {
        joueurs.insert(m[1].to_string());
    } else if let Some(m) = LEAVE.captures(ligne) {
        joueurs.remove(&m[1]);
    }
}

fn propriete(dossier: &Path, cle: &str) -> Option<String> {
    let t = fs::read_to_string(dossier.join("server.properties")).ok()?;
    t.lines().find_map(|l| l.strip_prefix(cle)?.strip_prefix('=').map(|v| v.trim().to_string()))
}

pub fn nom_monde(dossier: &Path) -> String {
    propriete(dossier, "level-name").filter(|s| !s.is_empty()).unwrap_or_else(|| "world".into())
}

pub fn port(dossier: &Path) -> u16 {
    propriete(dossier, "server-port").and_then(|p| p.parse().ok()).unwrap_or(25565)
}

/// AAAAMMJJ_HHMMSS en heure locale.
fn horodatage() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut t) };
    format!("{:04}{:02}{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Zippe `src` avec des chemins relatifs à `base` ; ignore les fichiers illisibles (session.lock).
fn zipper(src: &Path, base: &Path, dest: &Path) -> R<()> {
    let mut z = zip::ZipWriter::new(fs::File::create(dest).map_err(err)?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut pile = vec![src.to_path_buf()];
    while let Some(d) = pile.pop() {
        for e in fs::read_dir(&d).map_err(err)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                pile.push(p);
                continue;
            }
            let Ok(data) = fs::read(&p) else { continue };
            let rel = p.strip_prefix(base).map_err(err)?.to_string_lossy().replace('\\', "/");
            z.start_file(rel, opt).map_err(err)?;
            z.write_all(&data).map_err(err)?;
        }
    }
    z.finish().map_err(err)?;
    Ok(())
}

/// Zippe le monde dans sauvegardes/ et ne garde que les `garder` plus récentes.
pub fn sauvegarder(dossier: &Path, garder: usize) -> R<PathBuf> {
    let monde = nom_monde(dossier);
    let src = dossier.join(&monde);
    if !src.is_dir() {
        return Err(format!("Monde introuvable : {}", src.display()));
    }
    let dest = dossier.join("sauvegardes");
    fs::create_dir_all(&dest).map_err(err)?;
    let chemin = dest.join(format!("{monde}_{}.zip", horodatage()));
    zipper(&src, dossier, &chemin)?;
    let prefixe = format!("{monde}_");
    let mut anciens: Vec<PathBuf> = fs::read_dir(&dest).map_err(err)?.flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefixe) && n.ends_with(".zip")))
        .collect();
    anciens.sort();
    let n = anciens.len().saturating_sub(garder);
    for vieux in &anciens[..n] {
        let _ = fs::remove_file(vieux);
    }
    Ok(chemin)
}

pub fn lister_mods(dossier: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dossier.join("mods")).into_iter().flatten().flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn ajouter_mod(dossier: &Path, fichier: &Path) -> io::Result<()> {
    let d = dossier.join("mods");
    fs::create_dir_all(&d)?;
    fs::copy(fichier, d.join(fichier.file_name().unwrap_or_default())).map(|_| ())
}

/// Lit un flux ligne par ligne (UTF-8 avec remplacement) jusqu'à la fin.
fn suivre(r: impl Read + Send + 'static, mut f: impl FnMut(String) + Send + 'static) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for l in BufReader::new(r).split(b'\n').map_while(Result::ok) {
            f(String::from_utf8_lossy(&l).trim_end().to_string());
        }
    })
}

enum Msg {
    Log(String),
    Adresse(String),
    Reveil,
    Sauvegarde(String),
    Modpack(R<InfoPack>, PathBuf),
    Partage(String, Option<String>),
    Kit(String, Option<PathBuf>),
}

// ---------- serveur : processus ----------

#[derive(Default)]
struct Etat {
    joueurs: BTreeSet<String>,
    pret: bool, // vrai une fois le monde chargé ("Done")
}

#[derive(Clone, Default)]
struct Serveur {
    child: Arc<Mutex<Option<Child>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    etat: Arc<Mutex<Etat>>,
}

impl Serveur {
    fn actif(&self) -> bool {
        self.child.lock().unwrap().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    fn pid(&self) -> Option<u32> {
        self.child.lock().unwrap().as_ref().map(|c| c.id())
    }

    fn demarrer(&self, java: &str, jar: &Path, ram: u32, tx: Sender<Msg>) -> io::Result<()> {
        if self.actif() {
            return Ok(());
        }
        *self.etat.lock().unwrap() = Etat::default();
        let mut c = Command::new(java).arg(format!("-Xmx{ram}M")).arg(format!("-Xms{ram}M")).arg("-jar").arg(jar)
            .arg("nogui").current_dir(jar.parent().unwrap_or(Path::new("."))).stdin(Stdio::piped())
            .stdout(Stdio::piped()).stderr(Stdio::piped()).creation_flags(CREATE_NO_WINDOW).spawn()?;
        let (out, erreurs) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
        *self.stdin.lock().unwrap() = c.stdin.take();
        *self.child.lock().unwrap() = Some(c);
        let t2 = tx.clone();
        suivre(erreurs, move |l| {
            let _ = t2.send(Msg::Log(l));
        });
        let etat = self.etat.clone();
        let t3 = tx.clone();
        let h = suivre(out, move |l| {
            {
                let mut e = etat.lock().unwrap();
                maj_joueurs(&mut e.joueurs, &l);
                e.pret |= l.contains("Done (");
            }
            let _ = t3.send(Msg::Log(l));
        });
        let etat = self.etat.clone();
        thread::spawn(move || {
            let _ = h.join();
            *etat.lock().unwrap() = Etat::default();
            let _ = tx.send(Msg::Log("[Serveur arrêté]".into()));
        });
        Ok(())
    }

    fn commande(&self, texte: &str) {
        if let Some(s) = self.stdin.lock().unwrap().as_mut() {
            let _ = writeln!(s, "{texte}").and_then(|_| s.flush());
        }
    }

    /// "stop" puis attend 60 s avant de tuer le processus.
    fn arreter(&self) {
        if !self.actif() {
            return;
        }
        self.commande("stop");
        let fin = Instant::now() + Duration::from_secs(60);
        while self.actif() {
            if Instant::now() > fin {
                if let Some(c) = self.child.lock().unwrap().as_mut() {
                    let _ = c.kill();
                }
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

/// Temps CPU (unités de 100 ns) et mémoire (octets) d'un processus.
fn mesure(pid: u32) -> Option<(u64, usize)> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let ok = GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0
            && GetProcessMemoryInfo(h, &mut m, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32) != 0;
        CloseHandle(h);
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        ok.then(|| (ft(k) + ft(u), m.WorkingSetSize))
    }
}

// ---------- mode veille ----------

fn varint(r: &mut impl Read) -> io::Result<u32> {
    let mut n = 0u32;
    for i in 0..5 {
        let mut b = [0u8];
        r.read_exact(&mut b)?;
        n |= ((b[0] & 0x7F) as u32) << (7 * i);
        if b[0] & 0x80 == 0 {
            return Ok(n);
        }
    }
    Err(io::Error::other("varint trop long"))
}

fn encode(mut n: u32) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        out.push(b | if n != 0 { 0x80 } else { 0 });
        if n == 0 {
            return out;
        }
    }
}

fn paquet(id: u32, data: &[u8]) -> Vec<u8> {
    let mut corps = encode(id);
    corps.extend_from_slice(data);
    let mut p = encode(corps.len() as u32);
    p.extend(corps);
    p
}

fn chaine(t: &str) -> Vec<u8> {
    let mut v = encode(t.len() as u32);
    v.extend_from_slice(t.as_bytes());
    v
}

fn lire_paquet(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let n = varint(r)? as usize;
    if n == 0 || n > 4096 {
        return Err(io::Error::other("paquet invalide"));
    }
    let mut buf = vec![0; n];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Écoute le port du serveur éteint : un ping affiche un message, une connexion réveille le serveur.
struct Veille {
    stop: Arc<AtomicBool>,
}

impl Veille {
    fn ouvrir(port: u16, tx: Sender<Msg>) -> io::Result<Veille> {
        let l = TcpListener::bind(("0.0.0.0", port))?;
        l.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let demarrage = Arc::new(AtomicBool::new(false)); // vrai une fois le réveil demandé
        let s = stop.clone();
        thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match l.accept() {
                    Ok((c, _)) => {
                        let (d, tx) = (demarrage.clone(), tx.clone());
                        thread::spawn(move || {
                            let _ = Veille::client(c, &d, &tx);
                        });
                    }
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            } // le socket se ferme ici : le port est libéré pour le serveur
        });
        Ok(Veille { stop })
    }

    fn fermer(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn client(mut c: TcpStream, demarrage: &AtomicBool, tx: &Sender<Msg>) -> io::Result<()> {
        c.set_nonblocking(false)?;
        c.set_read_timeout(Some(Duration::from_secs(5)))?;
        c.set_write_timeout(Some(Duration::from_secs(5)))?;
        let p = lire_paquet(&mut c)?; // poignée de main : id, protocole, adresse, port, état
        let mut r = &p[1..];
        let proto = varint(&mut r)?;
        match p.last() {
            Some(1) => {
                lire_paquet(&mut c)?;
                let texte = if demarrage.load(Ordering::Relaxed) { "Serveur en démarrage…" } else { "Serveur en veille — connecte-toi pour le démarrer" };
                let j = json!({"version": {"name": "Veille", "protocol": proto}, "players": {"max": 0, "online": 0}, "description": {"text": texte}});
                c.write_all(&paquet(0, &chaine(&j.to_string())))?;
                let ping = lire_paquet(&mut c)?;
                c.write_all(&paquet(1, &ping[1..]))?; // écho du ping
            }
            Some(2) => {
                lire_paquet(&mut c)?; // sans lecture, la fermeture ferait un RST
                demarrage.store(true, Ordering::Relaxed);
                let j = json!({"text": "Le serveur démarre, reconnecte-toi dans une minute."});
                c.write_all(&paquet(0, &chaine(&j.to_string())))?;
                let _ = tx.send(Msg::Reveil);
            }
            _ => {}
        }
        Ok(())
    }
}

// ---------- réseau ----------

fn agent(timeout: u64) -> R<ureq::Agent> {
    let tls = native_tls::TlsConnector::new().map_err(err)?;
    Ok(ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(timeout)).user_agent(UA).build())
}

/// Corps et URL finale (après redirections).
fn lire_url(url: &str, timeout: u64) -> R<(Vec<u8>, String)> {
    let r = agent(timeout)?.get(url).call().map_err(err)?;
    let finale = r.get_url().to_string();
    let mut v = vec![];
    r.into_reader().take(1 << 30).read_to_end(&mut v).map_err(err)?;
    Ok((v, finale))
}

fn lire_json(url: &str) -> R<Value> {
    serde_json::from_slice(&lire_url(url, 60)?.0).map_err(err)
}

fn ecrire(dest: &Path, data: &[u8]) -> R<()> {
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p).map_err(err)?;
    }
    let tmp = dest.with_extension("tmp");
    fs::write(&tmp, data).map_err(err)?;
    fs::rename(&tmp, dest).map_err(err)
}

// ---------- playit.gg ----------

/// Agent officiel (github.com/playit-cloud/playit-agent) : adresse publique sans ouvrir de port.
#[derive(Clone, Default)]
struct Playit {
    child: Arc<Mutex<Option<Child>>>,
    claim_ouvert: Arc<AtomicBool>,
}

impl Playit {
    fn dossier() -> PathBuf {
        appdata().join("GestionServeurMinecraft").join("playit")
    }

    fn actif(&self) -> bool {
        self.child.lock().unwrap().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
    }

    fn telecharger() -> R<PathBuf> {
        let exe = Playit::dossier().join("playit.exe");
        if exe.is_file() {
            return Ok(exe);
        }
        let rel = lire_json("https://api.github.com/repos/playit-cloud/playit-agent/releases/latest")?;
        let url = rel["assets"].as_array().into_iter().flatten()
            .find(|a| a["name"] == "playit-windows-x86_64-signed.exe")
            .and_then(|a| a["browser_download_url"].as_str()).ok_or("agent introuvable dans la release")?;
        if !url.starts_with("https://github.com/playit-cloud/playit-agent/") {
            return Err("URL inattendue".into());
        }
        ecrire(&exe, &lire_url(url, 180)?.0)?;
        Ok(exe)
    }

    fn demarrer(&self, tx: Sender<Msg>) -> R<()> {
        if self.actif() {
            return Ok(());
        }
        let exe = Playit::telecharger()?;
        let mut c = Command::new(&exe).arg("--stdout").current_dir(Playit::dossier()).stdout(Stdio::piped())
            .stderr(Stdio::piped()).creation_flags(CREATE_NO_WINDOW).spawn().map_err(err)?;
        let (out, erreurs) = (c.stdout.take().unwrap(), c.stderr.take().unwrap());
        *self.child.lock().unwrap() = Some(c);
        for flux in [Box::new(out) as Box<dyn Read + Send>, Box::new(erreurs)] {
            let (tx, claim) = (tx.clone(), self.claim_ouvert.clone());
            suivre(flux, move |l| {
                if let Some(m) = CLAIM.find(&l) {
                    if !claim.swap(true, Ordering::Relaxed) {
                        let _ = Command::new("explorer").arg(m.as_str()).spawn(); // l'utilisateur valide lui-même la liaison
                    }
                } else if let Some(m) = ADRESSE.find(&l) {
                    let _ = tx.send(Msg::Adresse(m.as_str().to_string()));
                }
                let _ = tx.send(Msg::Log(l));
            });
        }
        Ok(())
    }

    fn arreter(&self) {
        if let Some(c) = self.child.lock().unwrap().as_ref() {
            // /T : playit.exe lance un processus fils qu'un simple kill laisserait tourner
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &c.id().to_string()])
                .creation_flags(CREATE_NO_WINDOW).output();
        }
    }
}

// ---------- modpack ----------

const HOTES: &[&str] = &["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"]; // spec .mrpack
const LOADERS: &[(&str, &str)] = &[("fabric-loader", "fabric"), ("quilt-loader", "quilt"), ("forge", "forge"), ("neoforge", "neoforge")];

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct InfoPack {
    pub mc: Option<String>,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub installes: usize,
    pub ignores: usize,
    pub echecs: Vec<String>,
    pub overrides: usize,
    pub sauvegarde_mods: Option<String>,
}

/// Chemin sous `dossier`, ou None s'il en sort (zip-slip).
fn sur(dossier: &Path, rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    (!rel.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))).then(|| dossier.join(p))
}

fn hote(url: &str) -> Option<&str> {
    url.strip_prefix("https://")?.split(['/', '?', '#']).next()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn decoder_url(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encoder_url(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn verifier(data: &[u8], hashes: &Value) -> R<()> {
    let ok = |algo: &str, calc: String| hashes.get(algo).and_then(Value::as_str).is_none_or(|h| h == calc);
    if ok("sha512", hex(&Sha512::digest(data))) && ok("sha1", hex(&Sha1::digest(data))) {
        Ok(())
    } else {
        Err("empreinte incorrecte".into())
    }
}

/// Zippe l'ancien dossier mods (s'il n'est pas vide) avant toute modification.
fn sauvegarder_mods(dossier: &Path) -> R<Option<String>> {
    if lister_mods(dossier).is_empty() {
        return Ok(None);
    }
    let d = dossier.join("sauvegardes");
    fs::create_dir_all(&d).map_err(err)?;
    let dest = d.join(format!("mods_{}.zip", horodatage()));
    zipper(&dossier.join("mods"), &dossier.join("mods"), &dest)?;
    Ok(Some(dest.display().to_string()))
}

/// Copie les overrides sans jamais écraser un fichier existant ni toucher au monde.
fn overrides(z: &mut zip::ZipArchive<fs::File>, dossier: &Path, prefixes: &[&str], monde: &str) -> usize {
    let mut n = 0;
    for pre in prefixes {
        // server-overrides d'abord : le premier fichier posé gagne
        for i in 0..z.len() {
            let Ok(mut f) = z.by_index(i) else { continue };
            let nom = f.name().to_string();
            let Some(rel) = nom.strip_prefix(pre).filter(|_| !f.is_dir()) else { continue };
            let Some(dest) = sur(dossier, rel) else { continue };
            if rel.split('/').next() == Some(monde) || dest.exists() {
                continue;
            }
            let _ = fs::create_dir_all(dest.parent().unwrap());
            if fs::File::create(&dest).and_then(|mut d| io::copy(&mut f, &mut d)).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// taches : (url, chemin relatif ou None, empreintes). Renvoie (ok, échecs). 8 téléchargements en parallèle.
fn telecharger_tout(taches: Vec<(String, Option<String>, Value)>, dossier: &Path, tx: &Sender<Msg>) -> (usize, Vec<String>) {
    let un = |(url, rel, hashes): &(String, Option<String>, Value)| -> R<String> {
        let (data, finale) = lire_url(url, 120)?;
        verifier(&data, hashes)?;
        let rel = rel.clone().unwrap_or_else(|| {
            let chemin = finale.split(['?', '#']).next().unwrap_or("");
            format!("mods/{}", decoder_url(chemin.rsplit('/').next().unwrap_or("")))
        });
        ecrire(&sur(dossier, &rel).ok_or("chemin refusé")?, &data)?;
        Ok(rel)
    };
    let suivant = AtomicUsize::new(0);
    let res = Mutex::new((0usize, vec![]));
    thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = suivant.fetch_add(1, Ordering::Relaxed);
                let Some(t) = taches.get(i) else { break };
                let r = un(t);
                let mut g = res.lock().unwrap();
                match r {
                    Ok(rel) => {
                        g.0 += 1;
                        let _ = tx.send(Msg::Log(format!("[Modpack] {}/{} {rel}", g.0 + g.1.len(), taches.len())));
                    }
                    Err(e) => g.1.push(format!("{} ({e})", t.1.as_deref().unwrap_or(&t.0))),
                }
            });
        }
    });
    res.into_inner().unwrap()
}

fn lire_entree(z: &mut zip::ZipArchive<fs::File>, nom: &str) -> R<Vec<u8>> {
    let mut v = vec![];
    z.by_name(nom).map_err(err)?.read_to_end(&mut v).map_err(err)?;
    Ok(v)
}

/// Installe un modpack (.mrpack Modrinth, .zip CurseForge ou simple .zip de mods) dans `dossier`.
fn installer(chemin: &Path, dossier: &Path, tx: &Sender<Msg>) -> R<InfoPack> {
    let monde = nom_monde(dossier);
    let mut info = InfoPack::default();
    let mut z = zip::ZipArchive::new(fs::File::open(chemin).map_err(err)?).map_err(err)?;
    let noms: Vec<String> = z.file_names().map(String::from).collect();
    info.sauvegarde_mods = sauvegarder_mods(dossier)?;
    if noms.iter().any(|n| n == "modrinth.index.json") {
        let idx: Value = serde_json::from_slice(&lire_entree(&mut z, "modrinth.index.json")?).map_err(err)?;
        let dep = &idx["dependencies"];
        info.mc = dep["minecraft"].as_str().map(String::from);
        for (k, v) in LOADERS {
            if let Some(ver) = dep[*k].as_str() {
                info.loader = Some(v.to_string());
                info.loader_version = Some(ver.to_string());
            }
        }
        let mut taches = vec![];
        for f in idx["files"].as_array().into_iter().flatten() {
            if f["env"]["server"] == "unsupported" {
                info.ignores += 1;
                continue;
            }
            let url = f["downloads"].as_array().into_iter().flatten().filter_map(Value::as_str)
                .find(|u| hote(u).is_some_and(|h| HOTES.contains(&h)));
            let path = f["path"].as_str().unwrap_or("").to_string();
            match url {
                Some(u) => taches.push((u.to_string(), Some(path), f["hashes"].clone())),
                None => info.echecs.push(format!("{path} (aucune URL autorisée)")),
            }
        }
        let (ok, ech) = telecharger_tout(taches, dossier, tx);
        info.installes = ok;
        info.echecs.extend(ech);
        info.overrides = overrides(&mut z, dossier, &["server-overrides/", "overrides/"], &monde);
    } else if noms.iter().any(|n| n == "manifest.json") {
        let man: Value = serde_json::from_slice(&lire_entree(&mut z, "manifest.json")?).map_err(err)?;
        info.mc = man["minecraft"]["version"].as_str().map(String::from);
        let ml = man["minecraft"]["modLoaders"].as_array().cloned().unwrap_or_default();
        let lid = ml.iter().find(|m| m["primary"] == true).or(ml.first()).and_then(|m| m["id"].as_str()).unwrap_or("");
        if let Some((l, v)) = lid.split_once('-') {
            info.loader = Some(l.into());
            info.loader_version = Some(v.into());
        }
        let taches = man["files"].as_array().into_iter().flatten().filter(|f| f["required"] != false)
            .map(|f| (format!("https://www.curseforge.com/api/v1/mods/{}/files/{}/download", f["projectID"], f["fileID"]), None, Value::Null))
            .collect();
        (info.installes, info.echecs) = telecharger_tout(taches, dossier, tx);
        let pre = format!("{}/", man["overrides"].as_str().unwrap_or("overrides"));
        info.overrides = overrides(&mut z, dossier, &[&pre], &monde);
    } else {
        // simple zip de mods
        for n in noms.iter().filter(|n| n.to_lowercase().ends_with(".jar")) {
            let base = n.rsplit('/').next().unwrap_or(n);
            if let Some(dest) = sur(dossier, &format!("mods/{base}")).filter(|d| !d.exists()) {
                ecrire(&dest, &lire_entree(&mut z, n)?)?;
                info.installes += 1;
            }
        }
    }
    if info.mc.is_some() && info.loader.is_some() {
        // mémorisé pour le partage et le kit
        let _ = fs::write(dossier.join("modpack_info.json"), serde_json::to_string(&info).unwrap());
    }
    Ok(info)
}

/// Télécharge fabric-server-launch.jar (serveur Fabric prêt à lancer).
fn installer_fabric(dossier: &Path, mc: &str, loader_version: &str) -> R<PathBuf> {
    let versions = lire_json("https://meta.fabricmc.net/v2/versions/installer")?;
    let inst = versions.as_array().into_iter().flatten().find(|v| v["stable"] == true)
        .and_then(|v| v["version"].as_str()).ok_or("installeur Fabric introuvable")?;
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{loader_version}/{inst}/server/jar");
    let dest = dossier.join("fabric-server-launch.jar");
    ecrire(&dest, &lire_url(&url, 120)?.0)?;
    Ok(dest)
}

// ---------- partage (packwiz) et kit amis ----------

fn toml(s: &str) -> String {
    serde_json::to_string(s).unwrap() // une chaîne JSON est une chaîne TOML valide
}

fn api_post(url: &str, corps: &Value) -> R<Value> {
    agent(60)?.post(url).send_json(corps.clone()).map_err(err)?.into_json().map_err(err)
}

/// Écrit un pack packwiz dans `sortie` d'après les mods du serveur. Renvoie (nb ajoutés, mods introuvables).
fn generer_pack(dossier: &Path, sortie: &Path, nom: &str, mc: &str, loader: &str, lv: &str) -> R<(usize, Vec<String>)> {
    let jars: Vec<String> = lister_mods(dossier).into_iter().filter(|j| j.ends_with(".jar")).collect();
    let mut sha1 = BTreeMap::new();
    for j in &jars {
        sha1.insert(j.clone(), hex(&Sha1::digest(fs::read(dossier.join("mods").join(j)).map_err(err)?)));
    }
    let versions = if jars.is_empty() { json!({}) } else {
        api_post("https://api.modrinth.com/v2/version_files", &json!({"hashes": sha1.values().collect::<Vec<_>>(), "algorithm": "sha1"}))?
    };
    let ids: BTreeSet<&str> = versions.as_object().into_iter().flatten().filter_map(|(_, v)| v["project_id"].as_str()).collect();
    let mut projets = BTreeMap::new();
    if !ids.is_empty() {
        let l = lire_json(&format!("https://api.modrinth.com/v2/projects?ids={}", encoder_url(&json!(ids).to_string())))?;
        for p in l.as_array().into_iter().flatten() {
            projets.insert(p["id"].as_str().unwrap_or("").to_string(), p.clone());
        }
    }
    let _ = fs::remove_dir_all(sortie.join("mods"));
    fs::create_dir_all(sortie.join("mods")).map_err(err)?;
    let (mut index, mut manquants) = (vec![], vec![]);
    let vide = json!({});
    for j in &jars {
        let f = versions.get(&sha1[j]).and_then(|v| v["files"].as_array()?.iter().find(|f| f["hashes"]["sha1"] == sha1[j].as_str()));
        let Some(f) = f else {
            manquants.push(j.clone());
            continue;
        };
        let p = projets.get(versions[&sha1[j]]["project_id"].as_str().unwrap_or("")).unwrap_or(&vide);
        let cote = if p["client_side"] == "unsupported" { "server" } else if p["server_side"] == "unsupported" { "client" } else { "both" };
        let meta = format!("name = {}\nfilename = {}\nside = \"{cote}\"\n\n[download]\nurl = {}\nhash-format = \"sha512\"\nhash = \"{}\"\n",
            toml(p["title"].as_str().unwrap_or(j)), toml(j), toml(f["url"].as_str().unwrap_or("")), f["hashes"]["sha512"].as_str().unwrap_or(""));
        let chemin = format!("mods/{}.pw.toml", j.strip_suffix(".jar").unwrap_or(j));
        fs::write(sortie.join(&chemin), &meta).map_err(err)?;
        index.push(format!("[[files]]\nfile = {}\nhash = \"{}\"\nmetafile = true\n", toml(&chemin), hex(&Sha256::digest(&meta))));
    }
    let idx = format!("hash-format = \"sha256\"\n\n{}", index.join("\n"));
    fs::write(sortie.join("index.toml"), &idx).map_err(err)?;
    let pack = format!("name = {}\npack-format = \"packwiz:1.1.0\"\n\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{}\"\n\n[versions]\nminecraft = {}\n{loader} = {}\n",
        toml(nom), hex(&Sha256::digest(&idx)), toml(mc), toml(lv));
    fs::write(sortie.join("pack.toml"), pack).map_err(err)?;
    Ok((index.len(), manquants))
}

/// Pousse le pack dans un dépôt GitHub public (créé s'il n'existe pas). Renvoie l'URL raw de pack.toml.
fn publier(sortie: &Path, repo: &str) -> R<String> {
    let run = |a: &[&str]| -> R<String> {
        let o = Command::new(a[0]).args(&a[1..]).current_dir(sortie).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| format!("{} : {e}", a[0]))?;
        if !o.status.success() {
            return Err(format!("{} : {}", a.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let owner = run(&["gh", "api", "user", "-q", ".login"])?;
    if !sortie.join(".git").is_dir() {
        run(&["git", "init", "-q", "-b", "main"])?;
    }
    run(&["git", "add", "-A"])?;
    if !run(&["git", "status", "--porcelain"])?.is_empty() {
        run(&["git", "-c", &format!("user.name={owner}"), "-c", &format!("user.email={owner}@users.noreply.github.com"), "commit", "-qm", "Mise à jour du modpack"])?;
    }
    if !run(&["git", "remote"])?.lines().any(|l| l == "origin") {
        run(&["gh", "repo", "create", &format!("{owner}/{repo}"), "--public", "--source", ".", "--remote", "origin"])?;
    }
    run(&["git", "push", "-q", "-u", "origin", "main"])?;
    Ok(format!("https://raw.githubusercontent.com/{owner}/{repo}/main/pack.toml"))
}

/// NBT non compressé minimal : une seule entrée de serveur.
fn servers_dat(nom: &str, adresse: &str) -> Vec<u8> {
    let s = |cle: &str, val: &str| {
        let mut v = vec![8u8];
        v.extend((cle.len() as u16).to_be_bytes());
        v.extend(cle.as_bytes());
        v.extend((val.len() as u16).to_be_bytes());
        v.extend(val.as_bytes());
        v
    };
    let mut v = vec![0x0a, 0, 0, 0x09];
    v.extend(7u16.to_be_bytes());
    v.extend(b"servers");
    v.push(0x0a);
    v.extend(1i32.to_be_bytes());
    v.extend(s("name", nom));
    v.extend(s("ip", adresse));
    v.extend([0, 0]);
    v
}

fn cache(url: &str, nom: &str) -> R<PathBuf> {
    let dest = appdata().join("GestionServeurMinecraft").join("cache").join(nom);
    if !dest.exists() {
        ecrire(&dest, &lire_url(url, 300)?.0)?;
    }
    Ok(dest)
}

const AIDE: &str = "KIT POUR REJOINDRE LE SERVEUR « {nom} »

1. Dézippe ce dossier où tu veux (Bureau, Documents…). Ne le lance pas depuis le zip.
2. Ouvre « prismlauncher.exe ».
3. Au premier lancement, choisis ta langue puis connecte-toi avec le compte Microsoft
   qui possède Minecraft Java (bouton « Comptes » → « Ajouter un compte Microsoft »).
   Si Prism propose d'installer Java, accepte.
4. Double-clique sur l'instance « {nom} » puis « Jouer ». Au démarrage, les mods du
   serveur se téléchargent tout seuls (une petite fenêtre s'ouvre, c'est normal).
5. Dans Minecraft : Multijoueur → « {nom} » → Rejoindre.
   Si le serveur dormait, il démarre : attends une minute puis réessaie.

Adresse du serveur : {adresse}
Version : Minecraft {mc} ({loader} {lv}) — ne la change pas.

Prism Launcher est un logiciel libre (licence GPL-3.0, voir LICENSE-PrismLauncher.txt),
fourni sans modification : https://prismlauncher.org
Si un mod manque ou plante, ferme le jeu et relance « Jouer » : la synchro se refait.
";

/// Crée KitServeur.zip : Prism portable + instance préréglée + synchro packwiz + aide.
fn generer_kit(sortie: &Path, nom: &str, info: &InfoPack, pack_url: &str, adresse: &str, tx: &Sender<Msg>) -> R<PathBuf> {
    let (mc, loader, lv) = (info.mc.as_deref().unwrap_or(""), info.loader.as_deref().unwrap_or(""), info.loader_version.as_deref().unwrap_or(""));
    let composant = match loader {
        "fabric" => "net.fabricmc.fabric-loader",
        "quilt" => "org.quiltmc.quilt-loader",
        "forge" => "net.minecraftforge",
        "neoforge" => "net.neoforged",
        _ => return Err(format!("loader inconnu : {loader}")),
    };
    let rel = lire_json("https://api.github.com/repos/PrismLauncher/PrismLauncher/releases/latest")?;
    let a = rel["assets"].as_array().into_iter().flatten()
        .find(|a| a["name"].as_str().is_some_and(|n| n.contains("Windows-MinGW-w64-Portable") && n.ends_with(".zip")))
        .ok_or("Prism Launcher portable introuvable")?;
    let _ = tx.send(Msg::Log("[Kit] Téléchargement de Prism Launcher (si besoin)…".into()));
    let prism = cache(a["browser_download_url"].as_str().unwrap_or(""), a["name"].as_str().unwrap_or("prism.zip"))?;
    let boot = cache("https://github.com/packwiz/packwiz-installer-bootstrap/releases/latest/download/packwiz-installer-bootstrap.jar", "packwiz-installer-bootstrap.jar")?;
    let licence = cache("https://raw.githubusercontent.com/PrismLauncher/PrismLauncher/develop/LICENSE", "LICENSE-PrismLauncher.txt")?;
    let kit = sortie.join("KitServeur");
    let _ = fs::remove_dir_all(&kit);
    zip::ZipArchive::new(fs::File::open(&prism).map_err(err)?).map_err(err)?.extract(&kit).map_err(err)?;
    let inst = kit.join("instances").join(nom);
    let mcdir = inst.join(".minecraft");
    fs::create_dir_all(&mcdir).map_err(err)?;
    fs::copy(&boot, mcdir.join("packwiz-installer-bootstrap.jar")).map_err(err)?;
    let pack = json!({"formatVersion": 1, "components": [{"uid": "net.minecraft", "version": mc, "important": true}, {"uid": composant, "version": lv}]});
    fs::write(inst.join("mmc-pack.json"), pack.to_string()).map_err(err)?;
    fs::write(inst.join("instance.cfg"), format!("[General]\nConfigVersion=1.2\nInstanceType=OneSix\nname={nom}\nOverrideCommands=true\nPreLaunchCommand=\"$INST_JAVA\" -jar packwiz-installer-bootstrap.jar {pack_url}\n")).map_err(err)?;
    fs::write(mcdir.join("servers.dat"), servers_dat(nom, adresse)).map_err(err)?;
    fs::copy(&licence, kit.join("LICENSE-PrismLauncher.txt")).map_err(err)?;
    let aide = AIDE.replace("{nom}", nom).replace("{adresse}", adresse).replace("{mc}", mc).replace("{loader}", loader).replace("{lv}", lv);
    fs::write(kit.join("LISEZ-MOI.txt"), aide).map_err(err)?;
    let z = sortie.join("KitServeur.zip");
    zipper(&kit, sortie, &z)?;
    let _ = fs::remove_dir_all(&kit);
    Ok(z)
}

// ---------- interface ----------

#[derive(PartialEq, Clone, Copy)]
enum Onglet {
    Console,
    Joueurs,
    Sauvegardes,
    Mods,
    EnLigne,
    Etat,
}

enum Dialogue {
    Info(String),
    RetirerMods(Vec<String>),
    Partager,
    Fabric { texte: String, dossier: PathBuf, mc: String, lv: String },
}

pub struct Minecraft {
    cfg: Config,
    srv: Serveur,
    playit: Playit,
    veille: Option<Veille>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    console: VecDeque<String>,
    cmd: String,
    historique: Vec<String>,
    pos_h: usize,
    onglet: Onglet,
    arret_en_cours: bool,
    sauvegarde_en_cours: bool,
    etat_b: String,
    prochaine_auto: Option<Instant>,
    auto_vu: String,
    vide_depuis: Option<Instant>,
    mods: Vec<String>,
    mods_sel: BTreeSet<String>,
    mods_jar: Option<String>,
    import_en_cours: bool,
    partage_en_cours: bool,
    kit_en_cours: bool,
    etat_p: String,
    dialogue: Option<Dialogue>,
    cpu: Option<(u32, Instant, u64)>,
    cpu_txt: String,
    ram_txt: String,
}

impl Minecraft {
    pub fn new() -> Self {
        let mut cfg: Config = fs::read(fichier_config()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if cfg.java.is_empty() {
            cfg.java = java_par_defaut();
        }
        let (tx, rx) = channel();
        let mut m = Minecraft { auto_vu: cfg.auto.clone(), cfg, srv: Serveur::default(), playit: Playit::default(), veille: None, tx, rx,
            console: VecDeque::new(), cmd: String::new(), historique: vec![], pos_h: 0, onglet: Onglet::Console,
            arret_en_cours: false, sauvegarde_en_cours: false, etat_b: String::new(), prochaine_auto: None, vide_depuis: None,
            mods: vec![], mods_sel: BTreeSet::new(), mods_jar: None, import_en_cours: false, partage_en_cours: false,
            kit_en_cours: false, etat_p: String::new(), dialogue: None, cpu: None, cpu_txt: "—".into(), ram_txt: "—".into() };
        if m.cfg.en_ligne {
            m.basculer_en_ligne();
        }
        m
    }

    fn enregistrer(&mut self) {
        self.cfg.en_ligne = self.playit.actif();
        if let Ok(t) = serde_json::to_string(&self.cfg) {
            let _ = fs::write(fichier_config(), t);
        }
    }

    fn dossier(&self) -> Option<PathBuf> {
        Path::new(&self.cfg.jar).parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf)
    }

    fn log(&mut self, l: String) {
        self.console.push_back(l);
        while self.console.len() > MAX_LIGNES {
            self.console.pop_front();
        }
    }

    fn info(&mut self, t: impl Into<String>) {
        self.dialogue = Some(Dialogue::Info(t.into()));
    }

    fn basculer(&mut self) {
        if self.srv.actif() {
            self.arret_en_cours = true;
            self.log("[Arrêt en cours…]".into());
            let s = self.srv.clone();
            thread::spawn(move || s.arreter());
        } else {
            self.demarrer_serveur(false);
        }
    }

    fn demarrer_serveur(&mut self, silencieux: bool) {
        let jar = PathBuf::from(&self.cfg.jar);
        let Some(ram) = self.cfg.ram.parse::<u32>().ok().filter(|r| *r >= 512 && jar.is_file()) else {
            if !silencieux {
                self.info("Choisis un .jar existant et une RAM d'au moins 512 Mo.");
            }
            return;
        };
        self.enregistrer();
        if let Some(v) = self.veille.take() {
            v.fermer(); // libère le port pour le serveur
            thread::sleep(Duration::from_millis(120));
        }
        match self.srv.demarrer(&self.cfg.java, &jar, ram, self.tx.clone()) {
            Ok(()) => self.reset_auto(),
            Err(e) => {
                let m = format!("Impossible de lancer Java : {e}");
                if silencieux { self.log(format!("[{m}]")) } else { self.info(m) }
            }
        }
    }

    fn basculer_en_ligne(&mut self) {
        if self.playit.actif() {
            self.playit.arreter();
            return;
        }
        let (p, tx) = (self.playit.clone(), self.tx.clone());
        thread::spawn(move || {
            if let Err(e) = p.demarrer(tx.clone()) {
                let _ = tx.send(Msg::Log(format!("[playit] Échec : {e}")));
            }
        });
    }

    fn envoyer(&mut self) {
        let t = self.cmd.trim().to_string();
        if !t.is_empty() && self.srv.actif() {
            self.srv.commande(&t);
            self.historique.push(t);
            self.pos_h = self.historique.len();
            self.cmd.clear();
        }
    }

    fn naviguer(&mut self, d: i64) {
        if !self.historique.is_empty() {
            self.pos_h = (self.pos_h as i64 + d).clamp(0, self.historique.len() as i64) as usize;
            self.cmd = self.historique.get(self.pos_h).cloned().unwrap_or_default();
        }
    }

    fn sauvegarde(&mut self) {
        let Some(dossier) = self.dossier().filter(|_| !self.sauvegarde_en_cours) else { return };
        self.sauvegarde_en_cours = true;
        self.etat_b = "Sauvegarde en cours…".into();
        let (s, tx) = (self.srv.clone(), self.tx.clone());
        thread::spawn(move || {
            let actif = s.actif();
            if actif {
                // fige les écritures pour avoir une copie cohérente
                s.commande("save-off");
                s.commande("save-all flush");
                thread::sleep(Duration::from_secs(4));
            }
            let msg = match sauvegarder(&dossier, 10) {
                Ok(p) => format!("Dernière sauvegarde : {}", p.file_name().unwrap_or_default().to_string_lossy()),
                Err(e) => format!("Échec de la sauvegarde : {e}"),
            };
            if actif {
                s.commande("save-on");
            }
            let _ = tx.send(Msg::Sauvegarde(msg));
        });
    }

    fn reset_auto(&mut self) {
        self.prochaine_auto = self.cfg.auto.parse::<u64>().ok().filter(|m| *m > 0).map(|m| Instant::now() + Duration::from_secs(m * 60));
    }

    fn maj_mods(&mut self) {
        self.mods = self.dossier().map(|d| lister_mods(&d)).unwrap_or_default();
        self.mods_sel.retain(|m| self.mods.contains(m));
        self.mods_jar = Some(self.cfg.jar.clone());
    }

    fn info_modpack(&mut self) -> Option<InfoPack> {
        let i = self.dossier().and_then(|d| fs::read(d.join("modpack_info.json")).ok()).and_then(|b| serde_json::from_slice(&b).ok());
        if i.is_none() {
            self.info("Importe d'abord un modpack (version de Minecraft et loader inconnus).");
        }
        i
    }

    fn importer_modpack(&mut self) {
        let Some(dossier) = self.dossier() else { return self.info("Choisis d'abord le .jar du serveur.") };
        if self.srv.actif() {
            return self.info("Arrête le serveur avant d'importer un modpack.");
        }
        let Some(p) = rfd::FileDialog::new().add_filter("Modpack", &["mrpack", "zip"]).pick_file() else { return };
        self.import_en_cours = true;
        self.onglet = Onglet::Console;
        self.log(format!("[Modpack] Import de {}…", p.file_name().unwrap_or_default().to_string_lossy()));
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = installer(&p, &dossier, &tx);
            let _ = tx.send(Msg::Modpack(r, dossier));
        });
    }

    fn fin_modpack(&mut self, r: R<InfoPack>, dossier: PathBuf) {
        self.import_en_cours = false;
        self.maj_mods();
        let info = match r {
            Ok(i) => i,
            Err(e) => return self.info(format!("Échec de l'import :\n{e}")),
        };
        let mut l = vec![format!("{} mod(s) installé(s), {} client-only ignoré(s), {} fichier(s) de config copié(s).", info.installes, info.ignores, info.overrides)];
        if !info.echecs.is_empty() {
            l.push(format!("{} échec(s) :\n{}", info.echecs.len(), info.echecs.iter().take(8).cloned().collect::<Vec<_>>().join("\n")));
        }
        if info.sauvegarde_mods.is_some() {
            l.push("Ancien dossier mods sauvegardé dans sauvegardes/.".into());
        }
        if let Some(lo) = &info.loader {
            l.push(format!("Requis : {lo} {} pour Minecraft {}.", info.loader_version.as_deref().unwrap_or(""), info.mc.as_deref().unwrap_or("")));
        }
        self.log(format!("[Modpack] {}", l[0]));
        let texte = l.join("\n");
        match (info.loader.as_deref(), info.mc) {
            (Some("fabric"), Some(mc)) => {
                let lv = info.loader_version.unwrap_or_default();
                self.dialogue = Some(Dialogue::Fabric { texte, dossier, mc, lv });
            }
            _ => self.info(texte),
        }
    }

    fn partager(&mut self) {
        let Some(info) = self.info_modpack() else { return };
        let Some(dossier) = self.dossier() else { return };
        self.partage_en_cours = true;
        self.etat_p = "Génération du pack…".into();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let r = (|| {
                let sortie = dossier.join("pack_partage");
                fs::create_dir_all(&sortie).map_err(err)?;
                let (n, manquants) = generer_pack(&dossier, &sortie, "Serveur Minecraft", info.mc.as_deref().unwrap_or(""),
                    info.loader.as_deref().unwrap_or(""), info.loader_version.as_deref().unwrap_or(""))?;
                let url = publier(&sortie, "ModpackServeur")?;
                let mut msg = format!("{n} mod(s) partagé(s)");
                if !manquants.is_empty() {
                    msg += &format!(", introuvables sur Modrinth (ignorés) : {}", manquants.join(", "));
                }
                Ok::<_, String>((msg, url))
            })();
            let _ = tx.send(match r {
                Ok((m, u)) => Msg::Partage(m, Some(u)),
                Err(e) => Msg::Partage(format!("Échec : {e}"), None),
            });
        });
    }

    fn kit(&mut self) {
        let Some(info) = self.info_modpack() else { return };
        if self.cfg.pack_url.is_empty() || self.cfg.adresse.is_empty() {
            return self.info("Il faut l'adresse publique (mise en ligne) et l'URL du pack (partage du modpack).");
        }
        let Some(dossier) = self.dossier() else { return };
        self.kit_en_cours = true;
        self.etat_p = "Création du kit…".into();
        let (tx, url, adr) = (self.tx.clone(), self.cfg.pack_url.clone(), self.cfg.adresse.clone());
        thread::spawn(move || {
            let _ = tx.send(match generer_kit(&dossier, "Serveur Minecraft", &info, &url, &adr, &tx) {
                Ok(z) => Msg::Kit(format!("Kit prêt : {}", z.display()), Some(z)),
                Err(e) => Msg::Kit(format!("Échec : {e}"), None),
            });
        });
    }

    /// Tourne à chaque image, même hors de l'onglet : messages, veille, arrêt auto, sauvegarde auto.
    pub fn tic(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Log(l) => self.log(l),
                Msg::Adresse(a) => self.cfg.adresse = a,
                Msg::Reveil => {
                    if !self.srv.actif() {
                        self.log("[Veille] Un joueur se connecte : démarrage du serveur".into());
                        self.demarrer_serveur(true);
                    }
                }
                Msg::Sauvegarde(t) => {
                    self.sauvegarde_en_cours = false;
                    self.etat_b = t;
                }
                Msg::Modpack(r, d) => self.fin_modpack(r, d),
                Msg::Partage(t, url) => {
                    self.partage_en_cours = false;
                    self.etat_p = t;
                    if let Some(u) = url {
                        self.cfg.pack_url = u;
                    }
                }
                Msg::Kit(t, z) => {
                    self.kit_en_cours = false;
                    self.etat_p = t;
                    if let Some(p) = z.as_ref().and_then(|z| z.parent()) {
                        let _ = Command::new("explorer").arg(p).spawn();
                    }
                }
            }
        }
        let actif = self.srv.actif();
        if !actif {
            self.arret_en_cours = false;
        }
        if self.mods_jar.as_deref() != Some(&self.cfg.jar) {
            self.maj_mods();
        }
        if self.auto_vu != self.cfg.auto {
            self.auto_vu = self.cfg.auto.clone();
            self.reset_auto();
        }
        self.gerer_veille(actif);
        if self.prochaine_auto.is_some_and(|t| Instant::now() >= t) {
            self.reset_auto();
            self.sauvegarde();
        }
    }

    fn gerer_veille(&mut self, actif: bool) {
        let d = self.dossier().filter(|_| self.cfg.veille && !actif && Path::new(&self.cfg.jar).is_file());
        match d {
            Some(d) if self.veille.is_none() => match Veille::ouvrir(port(&d), self.tx.clone()) {
                Ok(v) => self.veille = Some(v),
                Err(e) => {
                    self.cfg.veille = false;
                    self.log(format!("[Veille impossible : port occupé ({e})]"));
                }
            },
            Some(_) => {}
            None => {
                if let Some(v) = self.veille.take() {
                    v.fermer();
                }
            }
        }
        let mn = self.cfg.idle.parse::<u64>().unwrap_or(0);
        let (pret, vide) = {
            let e = self.srv.etat.lock().unwrap();
            (e.pret, e.joueurs.is_empty())
        };
        if actif && pret && vide && !self.arret_en_cours {
            let depuis = *self.vide_depuis.get_or_insert_with(Instant::now);
            if mn > 0 && depuis.elapsed() > Duration::from_secs(mn * 60) {
                self.log(format!("[Arrêt auto : personne depuis {mn} min]"));
                self.vide_depuis = None;
                self.basculer();
            }
        } else {
            self.vide_depuis = None;
        }
    }

    fn maj_cpu(&mut self) {
        let Some(pid) = self.srv.pid().filter(|_| self.srv.actif()) else {
            self.cpu = None;
            self.cpu_txt = "—".into();
            self.ram_txt = "—".into();
            return;
        };
        if self.cpu.is_some_and(|(p, t, _)| p == pid && t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        let Some((cpu, ram)) = mesure(pid) else { return };
        if let Some((_, t, avant)) = self.cpu.filter(|c| c.0 == pid) {
            let n = thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
            let pct = cpu.saturating_sub(avant) as f64 / (t.elapsed().as_secs_f64() * 1e7) / n * 100.0;
            self.cpu_txt = format!("{pct:.0} %");
        }
        self.ram_txt = format!("{} Mo", ram >> 20);
        self.cpu = Some((pid, Instant::now(), cpu));
    }

    fn dialogues(&mut self, ctx: &egui::Context) {
        let Some(d) = &self.dialogue else { return };
        let (titre, texte, oui_non) = match d {
            Dialogue::Info(t) => ("Serveur Minecraft", t.clone(), false),
            Dialogue::RetirerMods(n) => ("Retirer", format!("Supprimer {} mod(s) du dossier mods ?", n.len()), true),
            Dialogue::Partager => ("Partager", "Publier la liste de mods dans un dépôt GitHub PUBLIC (ModpackServeur) sur ton compte ?".into(), true),
            Dialogue::Fabric { texte, .. } => ("Modpack", format!("{texte}\n\nTélécharger le serveur Fabric correspondant et l'utiliser ?"), true),
        };
        let mut choix = None;
        egui::Window::new(titre).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(texte);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if oui_non {
                    if ui.button("Oui").clicked() {
                        choix = Some(true);
                    }
                    if ui.button("Non").clicked() {
                        choix = Some(false);
                    }
                } else if ui.button("OK").clicked() {
                    choix = Some(false);
                }
            });
        });
        let Some(oui) = choix else { return };
        match self.dialogue.take() {
            Some(Dialogue::RetirerMods(noms)) if oui => {
                if let Some(d) = self.dossier() {
                    for n in noms {
                        let _ = fs::remove_file(d.join("mods").join(n));
                    }
                }
                self.maj_mods();
            }
            Some(Dialogue::Partager) if oui => self.partager(),
            Some(Dialogue::Fabric { dossier, mc, lv, .. }) if oui => match installer_fabric(&dossier, &mc, &lv) {
                Ok(p) => {
                    self.cfg.jar = p.display().to_string();
                    self.log("[Modpack] Serveur Fabric prêt : fabric-server-launch.jar".into());
                }
                Err(e) => self.info(format!("Téléchargement impossible :\n{e}")),
            },
            _ => {}
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.dialogues(ui.ctx());
        let actif = self.srv.actif();
        egui::Grid::new("mc_haut").num_columns(3).spacing([8.0, 6.0]).show(ui, |ui| {
            let large = (ui.available_width() - 220.0).max(200.0);
            ui.label("Serveur (.jar)");
            ui.add(egui::TextEdit::singleline(&mut self.cfg.jar).desired_width(large));
            if ui.button("Parcourir…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Jar", &["jar"]).pick_file() {
                    self.cfg.jar = p.display().to_string();
                }
            }
            ui.end_row();
            ui.label("Java");
            ui.add(egui::TextEdit::singleline(&mut self.cfg.java).desired_width(large));
            ui.horizontal(|ui| {
                ui.label("RAM (Mo)");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.ram).desired_width(60.0));
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        let joueurs: Vec<String> = self.srv.etat.lock().unwrap().joueurs.iter().cloned().collect();
        ui.horizontal(|ui| {
            let (texte, couleur, etat) = if self.arret_en_cours {
                ("Arrêt…", egui::Color32::from_rgb(245, 166, 35), "Arrêt en cours")
            } else if actif {
                ("Arrêter", egui::Color32::from_rgb(61, 220, 132), "En marche")
            } else {
                ("Démarrer", egui::Color32::from_rgb(239, 83, 80), "Arrêté")
            };
            if ui.add_enabled(!self.arret_en_cours, egui::Button::new(texte)).clicked() {
                self.basculer();
            }
            ui.colored_label(couleur, "●");
            ui.label(etat);
            if actif {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.weak(format!("{} joueur(s)", joueurs.len())));
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (o, n) in [(Onglet::Console, "Console"), (Onglet::Joueurs, "Joueurs"), (Onglet::Sauvegardes, "Sauvegardes"),
                (Onglet::Mods, "Mods"), (Onglet::EnLigne, "En ligne"), (Onglet::Etat, "État")] {
                ui.selectable_value(&mut self.onglet, o, n);
            }
        });
        ui.separator();
        match self.onglet {
            Onglet::Console => self.ui_console(ui),
            Onglet::Joueurs => {
                for j in &joueurs {
                    ui.label(j);
                }
            }
            Onglet::Sauvegardes => {
                if ui.add_enabled(!self.sauvegarde_en_cours, egui::Button::new("Sauvegarder maintenant")).clicked() {
                    self.sauvegarde();
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Sauvegarde auto toutes les");
                    ui.add(egui::TextEdit::singleline(&mut self.cfg.auto).desired_width(40.0));
                    ui.label("minutes (0 = désactivée · les 10 dernières sont gardées)");
                });
                ui.weak(self.etat_b.as_str());
            }
            Onglet::Mods => self.ui_mods(ui),
            Onglet::EnLigne => self.ui_en_ligne(ui),
            Onglet::Etat => {
                self.maj_cpu();
                let lignes: [(&str, String); 6] = [
                    ("Serveur", if actif { "En ligne".into() } else { "Hors ligne".into() }),
                    ("Joueurs", if joueurs.is_empty() { "Aucun".into() } else { joueurs.join(", ") }),
                    ("CPU", self.cpu_txt.clone()),
                    ("RAM", self.ram_txt.clone()),
                    ("Adresse publique", if self.cfg.adresse.is_empty() { "—".into() } else { self.cfg.adresse.clone() }),
                    ("Mode veille", if self.veille.is_some() { "À l'écoute".into() } else { "Inactif".into() }),
                ];
                egui::Grid::new("mc_etat").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    for (k, v) in lignes {
                        ui.weak(k);
                        if k == "Serveur" {
                            ui.colored_label(if actif { egui::Color32::from_rgb(61, 220, 132) } else { egui::Color32::from_rgb(239, 83, 80) }, v);
                        } else {
                            ui.label(v);
                        }
                        ui.end_row();
                    }
                });
            }
        }
    }

    fn ui_console(&mut self, ui: &mut egui::Ui) {
        let h = (ui.available_height() - 34.0).max(50.0);
        let hl = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both().max_height(h).auto_shrink(false).stick_to_bottom(true).show_rows(ui, hl, self.console.len(), |ui, r| {
            for l in self.console.range(r) {
                let c = if l.contains("ERROR") || l.contains("Exception") {
                    egui::Color32::from_rgb(239, 83, 80)
                } else if l.contains("WARN") {
                    egui::Color32::from_rgb(245, 166, 35)
                } else if l.starts_with('[') && !l.contains("INFO") && !l.contains("]:") {
                    egui::Color32::GRAY
                } else {
                    ui.visuals().text_color()
                };
                ui.add(egui::Label::new(egui::RichText::new(l).monospace().color(c)).extend());
            }
        });
        ui.horizontal(|ui| {
            ui.monospace("›");
            let r = ui.add(egui::TextEdit::singleline(&mut self.cmd).desired_width(f32::INFINITY));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.envoyer();
                r.request_focus();
            }
            if r.has_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                    self.naviguer(-1);
                }
                if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                    self.naviguer(1);
                }
            }
        });
    }

    fn ui_mods(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Ajouter…").clicked() {
                match self.dossier() {
                    None => self.info("Choisis d'abord le .jar du serveur."),
                    Some(d) => {
                        for p in rfd::FileDialog::new().add_filter("Mods", &["jar"]).pick_files().unwrap_or_default() {
                            if let Err(e) = ajouter_mod(&d, &p) {
                                self.info(format!("Copie impossible : {e}"));
                            }
                        }
                        self.maj_mods();
                    }
                }
            }
            if ui.add_enabled(!self.mods_sel.is_empty(), egui::Button::new("Retirer")).clicked() {
                self.dialogue = Some(Dialogue::RetirerMods(self.mods_sel.iter().cloned().collect()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.import_en_cours, egui::Button::new("Importer un modpack…")).clicked() {
                    self.importer_modpack();
                }
            });
        });
        ui.weak("Ctrl+clic pour en sélectionner plusieurs.");
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for n in &self.mods {
                let sel = self.mods_sel.contains(n);
                if ui.selectable_label(sel, n).clicked() {
                    if !ui.input(|i| i.modifiers.command) {
                        self.mods_sel.clear();
                        self.mods_sel.insert(n.clone());
                    } else if sel {
                        self.mods_sel.remove(n);
                    } else {
                        self.mods_sel.insert(n.clone());
                    }
                }
            }
        });
    }

    fn ui_en_ligne(&mut self, ui: &mut egui::Ui) {
        let en_ligne = self.playit.actif();
        ui.horizontal(|ui| {
            if ui.button(if en_ligne { "Mettre hors ligne" } else { "Mettre en ligne" }).clicked() {
                self.basculer_en_ligne();
            }
            ui.add(egui::TextEdit::singleline(&mut self.cfg.adresse).desired_width((ui.available_width() - 70.0).max(100.0)));
            if ui.button("Copier").clicked() {
                ui.ctx().copy_text(self.cfg.adresse.clone());
            }
        });
        ui.weak("Première fois : une page playit.gg s'ouvre, valide-y ton compte puis crée un tunnel « Minecraft Java ». Colle l'adresse ici si elle n'apparaît pas toute seule.");
        ui.add_space(10.0);
        ui.checkbox(&mut self.cfg.veille, "Mode veille : le serveur s'éteint et démarre quand un ami se connecte");
        ui.horizontal(|ui| {
            ui.label("Arrêt auto après");
            ui.add(egui::TextEdit::singleline(&mut self.cfg.idle).desired_width(40.0));
            ui.label("minutes sans joueur (0 = jamais)");
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.partage_en_cours, egui::Button::new("Partager le modpack")).clicked() && self.info_modpack().is_some() {
                self.dialogue = Some(Dialogue::Partager);
            }
            if ui.add_enabled(!self.kit_en_cours, egui::Button::new("Générer le kit amis")).clicked() {
                self.kit();
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.cfg.pack_url).hint_text("URL du pack (pack.toml)").desired_width(f32::INFINITY));
        ui.weak(self.etat_p.as_str());
    }
}

impl Drop for Minecraft {
    /// Fermeture du hub : arrête proprement le serveur et l'agent playit.
    fn drop(&mut self) {
        self.enregistrer();
        self.srv.arreter();
        self.playit.arreter();
        if let Some(v) = self.veille.take() {
            v.fermer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("outils_mc_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn joueurs() {
        let mut j = BTreeSet::new();
        maj_joueurs(&mut j, "[10:00:00] [Server thread/INFO]: Bob joined the game");
        assert!(j.contains("Bob"));
        maj_joueurs(&mut j, "[10:01:00] [Server thread/INFO]: Bob left the game");
        assert!(j.is_empty());
    }

    #[test]
    fn proprietes() {
        let d = temp("props");
        assert_eq!((nom_monde(&d), port(&d)), ("world".to_string(), 25565));
        fs::write(d.join("server.properties"), "level-name=monde\nserver-port=25570\n").unwrap();
        assert_eq!((nom_monde(&d), port(&d)), ("monde".to_string(), 25570));
    }

    #[test]
    fn sauvegarde_rotation_et_mods() {
        let d = temp("sauv");
        fs::create_dir_all(d.join("world").join("region")).unwrap();
        fs::write(d.join("world").join("level.dat"), "x").unwrap();
        fs::write(d.join("world").join("region").join("r.mca"), "y").unwrap();
        let dest = d.join("sauvegardes");
        fs::create_dir_all(&dest).unwrap();
        for n in ["20000101_000000", "20000102_000000"] {
            fs::write(dest.join(format!("world_{n}.zip")), "").unwrap();
        }
        let z = sauvegarder(&d, 2).unwrap();
        let noms: Vec<String> = zip::ZipArchive::new(fs::File::open(&z).unwrap()).unwrap().file_names().map(String::from).collect();
        assert!(noms.contains(&"world/level.dat".to_string()) && noms.contains(&"world/region/r.mca".to_string()));
        let reste: Vec<_> = fs::read_dir(&dest).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(reste.len(), 2);
        assert!(!reste.iter().any(|n| n == "world_20000101_000000.zip"));
        fs::write(d.join("a.jar"), "m").unwrap();
        ajouter_mod(&d, &d.join("a.jar")).unwrap();
        assert_eq!(lister_mods(&d), ["a.jar"]);
    }

    #[test]
    fn modpack_hors_ligne() {
        let d = temp("pack");
        fs::create_dir_all(d.join("mods")).unwrap();
        fs::write(d.join("mods").join("vieux.jar"), "v").unwrap();
        fs::create_dir_all(d.join("world")).unwrap();
        fs::write(d.join("world").join("level.dat"), "garde").unwrap();
        fs::write(d.join("config.txt"), "garde").unwrap();
        let idx = json!({"dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.0"},
            "files": [{"path": "mods/client.jar", "env": {"server": "unsupported"}, "downloads": []},
                      {"path": "mods/mauvais.jar", "downloads": ["https://evil.example/x.jar"]}]});
        let pack = d.join("p.mrpack");
        {
            let mut z = zip::ZipWriter::new(fs::File::create(&pack).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            for (n, c) in [("modrinth.index.json", idx.to_string()), ("overrides/config.txt", "ecrase".into()), ("overrides/nouveau.txt", "ok".into()),
                ("overrides/world/level.dat", "ecrase".into()), ("overrides/../../evasion.txt", "x".into())] {
                z.start_file(n, o).unwrap();
                z.write_all(c.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let (tx, _rx) = channel();
        let info = installer(&pack, &d, &tx).unwrap();
        assert_eq!((info.mc.as_deref(), info.loader.as_deref(), info.loader_version.as_deref()), (Some("1.21.1"), Some("fabric"), Some("0.16.0")));
        assert_eq!((info.ignores, info.installes, info.echecs.len(), info.overrides), (1, 0, 1, 1));
        assert_eq!(fs::read_to_string(d.join("config.txt")).unwrap(), "garde");
        assert_eq!(fs::read_to_string(d.join("world").join("level.dat")).unwrap(), "garde");
        assert!(d.join("nouveau.txt").exists() && !d.parent().unwrap().join("evasion.txt").exists());
        assert!(info.sauvegarde_mods.is_some() && d.join("modpack_info.json").exists());
    }

    #[test]
    fn urls() {
        assert_eq!(hote("https://cdn.modrinth.com/data/x.jar"), Some("cdn.modrinth.com"));
        assert_eq!(hote("http://cdn.modrinth.com/x"), None);
        assert_eq!(decoder_url("Mon%20mod%2B.jar"), "Mon mod+.jar");
        assert_eq!(decoder_url("a%2"), "a%2");
        assert_eq!(encoder_url("[\"a b\"]"), "%5B%22a%20b%22%5D");
        assert!(sur(Path::new("C:\\x"), "../y").is_none() && sur(Path::new("C:\\x"), "C:/y").is_none() && sur(Path::new("C:\\x"), "a/b").is_some());
    }

    #[test]
    fn nbt_servers_dat() {
        let v = servers_dat("S", "a.b");
        assert_eq!(&v[..4], &[0x0a, 0, 0, 0x09]);
        assert_eq!(&v[v.len() - 2..], &[0, 0]);
        assert_eq!(v.len(), 3 + 1 + 2 + 7 + 1 + 4 + (1 + 2 + 4 + 2 + 1) + (1 + 2 + 2 + 2 + 3) + 2);
    }

    #[test]
    fn veille_ping_et_reveil() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let (tx, rx) = channel();
        let v = Veille::ouvrir(port, tx).unwrap();
        let poignee = |etat: u8| {
            let mut d = encode(767);
            d.extend(chaine("localhost"));
            d.extend(port.to_be_bytes());
            d.push(etat);
            paquet(0, &d)
        };
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        c.write_all(&poignee(1)).unwrap();
        c.write_all(&paquet(0, &[])).unwrap();
        let r = lire_paquet(&mut c).unwrap();
        let mut s = &r[1..];
        let n = varint(&mut s).unwrap() as usize;
        let j: Value = serde_json::from_slice(&s[..n]).unwrap();
        assert_eq!((j["version"]["name"].as_str(), j["version"]["protocol"].as_u64()), (Some("Veille"), Some(767)));
        c.write_all(&paquet(1, &42u64.to_be_bytes())).unwrap();
        assert_eq!(lire_paquet(&mut c).unwrap(), paquet(1, &42u64.to_be_bytes())[1..].to_vec());
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        c.write_all(&poignee(2)).unwrap();
        c.write_all(&paquet(0, &chaine("Bob"))).unwrap();
        assert!(matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(Msg::Reveil)));
        v.fermer();
    }
}
