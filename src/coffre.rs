//! Coffre à mots de passe : Argon2id (dérivation) + AES-256-GCM (chiffrement authentifié).
//! Même fichier que l'ancienne version Python : %APPDATA%\CoffreMotsDePasse\coffre.json.
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use eframe::egui::{self, Align, Align2, Color32, Key, Layout, RichText, TextEdit};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VERSION: u64 = 1;
const SYMBOLES: &str = "!@#$%^&*()-_=+[]{};:,.?";
const DELAI_PRESSE_PAPIERS: Duration = Duration::from_secs(20);
const DELAI_VERROU: Duration = Duration::from_secs(5 * 60);
const LONGUEUR_MIN_MAITRE: usize = 10;
const GRIS: Color32 = Color32::GRAY;
const ROUGE: Color32 = Color32::from_rgb(255, 107, 107);

// ---------- chiffrement ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Kdf {
    iterations: u32,
    memoire_kio: u32,
    voies: u32,
}

/// Paramètres Argon2id (proches des recommandations OWASP) ; stockés dans le fichier pour pouvoir évoluer.
pub const KDF_DEFAUT: Kdf = Kdf { iterations: 3, memoire_kio: 65536, voies: 4 };

impl Kdf {
    fn json(&self) -> Value {
        json!({"nom": "argon2id", "iterations": self.iterations, "memoire_kio": self.memoire_kio, "voies": self.voies})
    }

    /// Bornes : un fichier piégé ne doit pas pouvoir bloquer le PC (mémoire ≤ 1 Gio).
    fn lire(v: &Value) -> Result<Kdf, Erreur> {
        let o = v.as_object().filter(|o| o.get("nom") == Some(&json!("argon2id")))
            .ok_or(Erreur::Invalide("Dérivation de clé inconnue"))?;
        let n = |k: &str, mini: u64, maxi: u64| o.get(k).and_then(Value::as_u64).filter(|x| (mini..=maxi).contains(x));
        let (iterations, voies) = (n("iterations", 1, 50), n("voies", 1, 16));
        match (iterations, voies, voies.and_then(|v| n("memoire_kio", 8 * v, 1_048_576))) {
            (Some(i), Some(v), Some(m)) => Ok(Kdf { iterations: i as u32, memoire_kio: m as u32, voies: v as u32 }),
            _ => Err(Erreur::Invalide("Paramètres de dérivation invalides")),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Erreur {
    MauvaisMotDePasse,
    Invalide(&'static str),
    ModifieAilleurs,
    Io(String),
}

impl fmt::Display for Erreur {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Erreur::MauvaisMotDePasse => f.write_str("Mot de passe maître incorrect (ou fichier altéré)"),
            Erreur::Invalide(s) => f.write_str(s),
            Erreur::ModifieAilleurs => f.write_str("Le coffre a été modifié par une autre fenêtre. Verrouillez puis rouvrez-le."),
            Erreur::Io(s) => f.write_str(s),
        }
    }
}

impl From<std::io::Error> for Erreur {
    fn from(e: std::io::Error) -> Self {
        Erreur::Io(e.to_string())
    }
}

fn aleatoire<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("générateur aléatoire du système indisponible");
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn deriver_cle(mdp: &str, sel: &[u8], kdf: Kdf) -> Result<[u8; 32], Erreur> {
    let p = argon2::Params::new(kdf.memoire_kio, kdf.iterations, kdf.voies, Some(32))
        .map_err(|_| Erreur::Invalide("Paramètres de dérivation invalides"))?;
    let mut cle = [0u8; 32];
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p)
        .hash_password_into(mdp.as_bytes(), sel, &mut cle)
        .map_err(|_| Erreur::Invalide("Paramètres de dérivation invalides"))?;
    Ok(cle)
}

/// En-tête authentifié (AAD), identique à `json.dumps(entete, sort_keys=True, separators=(",", ":"))`
/// (serde_json trie les clés des objets).
fn aad(kdf: &Value, sel: &str) -> String {
    json!({"version": VERSION, "kdf": kdf, "sel": sel}).to_string()
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct Entree {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub site: String,
    #[serde(default)]
    pub identifiant: String,
    #[serde(default)]
    pub mdp: String,
    #[serde(default)]
    pub notes: String,
    #[serde(flatten)]
    autres: Map<String, Value>,
}

/// Écrit dans un fichier temporaire du même dossier, force sur disque, puis remplace.
fn ecrire_atomique(chemin: &Path, texte: &str) -> std::io::Result<()> {
    let dossier = chemin.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dossier)?;
    let tmp = dossier.join(format!(".coffre-{}.tmp", hex(&aleatoire::<8>())));
    let r = (|| {
        let mut f = fs::File::create_new(&tmp)?;
        f.write_all(texte.as_bytes())?;
        f.sync_all()?;
        drop(f);
        let mut essai = 0;
        loop {
            match fs::rename(&tmp, chemin) {
                // un antivirus ou l'indexeur peut bloquer le fichier un court instant
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && essai < 9 => {
                    essai += 1;
                    std::thread::sleep(Duration::from_millis(100))
                }
                r => return r,
            }
        }
    })();
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

fn lire(chemin: &Path) -> Result<Value, Erreur> {
    let texte = fs::read(chemin)?;
    serde_json::from_slice(&texte).map_err(|_| Erreur::Invalide("Fichier coffre corrompu"))
}

pub struct Coffre {
    chemin: PathBuf,
    cle: [u8; 32],
    kdf: Value,
    sel: String,
    pub entrees: Vec<Entree>,
    nonce: Option<String>, // nonce du fichier tel qu'on l'a lu/écrit, pour détecter une écriture concurrente
}

impl Drop for Coffre {
    fn drop(&mut self) {
        self.cle.fill(0);
    }
}

impl Coffre {
    pub fn creer(chemin: &Path, mdp: &str, kdf: Kdf) -> Result<Coffre, Erreur> {
        if chemin.exists() {
            return Err(Erreur::Io("Un coffre existe déjà à cet emplacement".into()));
        }
        let sel = aleatoire::<16>();
        let mut c = Coffre { chemin: chemin.into(), cle: deriver_cle(mdp, &sel, kdf)?, kdf: kdf.json(),
            sel: B64.encode(sel), entrees: vec![], nonce: None };
        c.enregistrer()?;
        Ok(c)
    }

    pub fn ouvrir(chemin: &Path, mdp: &str) -> Result<Coffre, Erreur> {
        let corrompu = || Erreur::Invalide("Fichier coffre corrompu");
        let inattendu = || Erreur::Invalide("Contenu du coffre inattendu");
        let c = lire(chemin)?;
        let (Some(version), Some(kdf), Some(sel)) = (c.get("version"), c.get("kdf"), c.get("sel").and_then(Value::as_str))
        else {
            return Err(corrompu());
        };
        if version.as_u64() != Some(VERSION) {
            return Err(Erreur::Invalide("Version de coffre non prise en charge"));
        }
        let b = |k: &str| c.get(k).and_then(Value::as_str).and_then(|s| B64.decode(s).ok());
        let (Some(sel_b), Some(nonce), Some(ct)) = (b("sel"), b("nonce"), b("donnees")) else {
            return Err(corrompu());
        };
        if sel_b.len() < 16 || nonce.len() != 12 || ct.len() < 16 {
            return Err(corrompu());
        }
        let cle = deriver_cle(mdp, &sel_b, Kdf::lire(kdf)?)?;
        let clair = Aes256Gcm::new_from_slice(&cle).unwrap()
            .decrypt(Nonce::from_slice(&nonce), Payload { msg: &ct, aad: aad(kdf, sel).as_bytes() })
            .map_err(|_| Erreur::MauvaisMotDePasse)?;
        let donnees: Value = serde_json::from_slice(&clair).map_err(|_| inattendu())?;
        let entrees = match donnees.as_object().ok_or_else(inattendu)?.get("entrees") {
            None => vec![],
            Some(v) => serde_json::from_value(v.clone()).map_err(|_| inattendu())?,
        };
        Ok(Coffre { chemin: chemin.into(), cle, kdf: kdf.clone(), sel: sel.into(), entrees,
            nonce: c.get("nonce").and_then(Value::as_str).map(String::from) })
    }

    pub fn enregistrer(&mut self) -> Result<(), Erreur> {
        if let (Some(n), true) = (&self.nonce, self.chemin.exists()) {
            // fichier abîmé : le réécrire avec nos données valides
            let actuel = lire(&self.chemin).ok().and_then(|v| v.get("nonce").cloned()).filter(|v| !v.is_null());
            if actuel.is_some_and(|a| a != json!(n)) {
                return Err(Erreur::ModifieAilleurs);
            }
        }
        let nonce = aleatoire::<12>(); // nouveau nonce à chaque écriture
        let clair = serde_json::to_vec(&json!({"entrees": self.entrees})).unwrap();
        let ct = Aes256Gcm::new_from_slice(&self.cle).unwrap()
            .encrypt(Nonce::from_slice(&nonce), Payload { msg: &clair, aad: aad(&self.kdf, &self.sel).as_bytes() })
            .map_err(|_| Erreur::Io("Chiffrement impossible".into()))?;
        let nonce = B64.encode(nonce);
        let contenu = json!({"version": VERSION, "kdf": self.kdf, "sel": self.sel, "nonce": nonce, "donnees": B64.encode(ct)});
        ecrire_atomique(&self.chemin, &serde_json::to_string_pretty(&contenu).unwrap())?;
        self.nonce = Some(nonce);
        Ok(())
    }

    fn verifier_mot_de_passe(&self, mdp: &str) -> Result<bool, Erreur> {
        let sel = B64.decode(&self.sel).map_err(|_| Erreur::Invalide("Fichier coffre corrompu"))?;
        let cle = deriver_cle(mdp, &sel, Kdf::lire(&self.kdf)?)?;
        Ok(cle.iter().zip(self.cle).fold(0, |d, (a, b)| d | (a ^ b)) == 0) // comparaison en temps constant
    }

    pub fn changer_mot_de_passe(&mut self, ancien: &str, nouveau: &str) -> Result<(), Erreur> {
        if !self.verifier_mot_de_passe(ancien)? {
            return Err(Erreur::MauvaisMotDePasse);
        }
        let sel = aleatoire::<16>();
        let cle = deriver_cle(nouveau, &sel, KDF_DEFAUT)?;
        let avant = (self.cle, std::mem::replace(&mut self.kdf, KDF_DEFAUT.json()), std::mem::replace(&mut self.sel, B64.encode(sel)));
        self.cle = cle;
        let r = self.enregistrer();
        if r.is_err() {
            (self.cle, self.kdf, self.sel) = avant; // le fichier sur disque n'a pas changé
        }
        r
    }

    /// Rechiffre avec KDF_DEFAUT si le coffre a des paramètres plus faibles.
    pub fn renforcer_kdf(&mut self, mdp: &str) -> Result<bool, Erreur> {
        let k = Kdf::lire(&self.kdf)?;
        if k.iterations >= KDF_DEFAUT.iterations && k.memoire_kio >= KDF_DEFAUT.memoire_kio && k.voies >= KDF_DEFAUT.voies {
            return Ok(false);
        }
        self.changer_mot_de_passe(mdp, mdp)?;
        Ok(true)
    }
}

/// Estimation simple (longueur × taille de l'alphabet utilisé) : 'faible', 'moyen' ou 'fort'.
pub fn force_mot_de_passe(m: &str) -> &'static str {
    let classes: [(fn(&char) -> bool, usize); 4] = [
        (|c| c.is_ascii_lowercase(), 26),
        (|c| c.is_ascii_uppercase(), 26),
        (|c| c.is_ascii_digit(), 10),
        (|c| SYMBOLES.contains(*c), SYMBOLES.len()),
    ];
    let mut alphabet: usize = classes.iter().filter(|(f, _)| m.chars().any(|c| f(&c))).map(|(_, n)| n).sum();
    if m.chars().any(|c| !c.is_ascii() || (!c.is_ascii_alphanumeric() && !SYMBOLES.contains(c))) {
        alphabet += 32;
    }
    let bits = if alphabet > 0 { m.chars().count() as f64 * (alphabet as f64).log2() } else { 0.0 };
    if bits < 50.0 { "faible" } else if bits < 80.0 { "moyen" } else { "fort" }
}

/// Entier uniforme dans 0..n (rejet pour éviter le biais du modulo).
fn tirage(n: usize) -> usize {
    let n = n as u32;
    let zone = u32::MAX - u32::MAX % n;
    loop {
        let x = u32::from_le_bytes(aleatoire::<4>());
        if x < zone {
            return (x % n) as usize;
        }
    }
}

pub fn generer_mot_de_passe(longueur: usize, symboles: bool) -> String {
    let mut classes = vec!["abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ", "0123456789"];
    if symboles {
        classes.push(SYMBOLES);
    }
    let alphabet: Vec<char> = classes.concat().chars().collect();
    loop {
        // tirage uniforme, on recommence tant qu'une classe manque
        let m: String = (0..longueur).map(|_| alphabet[tirage(alphabet.len())]).collect();
        if longueur < classes.len() || classes.iter().all(|cl| m.chars().any(|c| cl.contains(c))) {
            return m;
        }
    }
}

/// 'https://www.github.com/login' -> 'github' : sert à vérifier que la fenêtre cible est la bonne.
fn mot_cle_site(site: &str) -> String {
    let s = site.to_lowercase();
    let s = s.rsplit("://").next().unwrap_or("").split('/').next().unwrap_or("");
    let p: Vec<&str> = s.split(':').next().unwrap_or("").trim().split('.').collect();
    (if p.len() >= 2 { p[p.len() - 2] } else { p[0] }).to_string()
}

// ---------- Windows : presse-papiers, saisie auto ----------

mod win {
    use std::ptr::null_mut;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn ouvrir() -> Result<(), String> {
        for _ in 0..20 {
            // une autre appli peut tenir le presse-papiers un instant
            if unsafe { OpenClipboard(null_mut()) } != 0 {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("Presse-papiers occupé".into())
    }

    unsafe fn poser(fmt: u32, data: &[u8]) -> Result<(), String> {
        let h = GlobalAlloc(GMEM_MOVEABLE, data.len());
        if h.is_null() {
            return Err("Mémoire insuffisante".into());
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), GlobalLock(h) as *mut u8, data.len());
        GlobalUnlock(h);
        if SetClipboardData(fmt, h).is_null() {
            GlobalFree(h);
            return Err("Copie impossible".into());
        }
        Ok(())
    }

    /// Copie exclue de l'historique (Win+V) et de la synchro cloud ; retourne le numéro de séquence.
    pub fn copier(texte: &str) -> Result<u32, String> {
        ouvrir()?;
        let r = unsafe {
            EmptyClipboard();
            let data: Vec<u8> = texte.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            poser(13, &data).and_then(|_| {
                for nom in ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                    let w: Vec<u16> = nom.encode_utf16().chain([0]).collect();
                    poser(RegisterClipboardFormatW(w.as_ptr()), &[0; 4])?;
                }
                Ok(())
            })
        };
        unsafe { CloseClipboard() };
        r.map(|_| unsafe { GetClipboardSequenceNumber() })
    }

    /// Vide le presse-papiers seulement si personne n'y a rien copié depuis.
    pub fn effacer_si_inchange(seq: u32) -> Result<bool, String> {
        if unsafe { GetClipboardSequenceNumber() } != seq {
            return Ok(false);
        }
        ouvrir()?;
        unsafe {
            EmptyClipboard();
            CloseClipboard();
        }
        Ok(true)
    }

    /// Vrai quand l'écran Windows est verrouillé (le bureau de saisie n'est plus accessible).
    pub fn session_verrouillee() -> bool {
        let h = unsafe { OpenInputDesktop(0, 0, 0x0100) }; // DESKTOP_SWITCHDESKTOP
        if h.is_null() {
            return true;
        }
        unsafe { CloseDesktop(h) };
        false
    }

    pub fn verr_maj() -> bool {
        unsafe { GetKeyState(0x14) & 1 != 0 }
    }

    pub fn premier_plan() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    /// (poignée, titre, est_une_autre_appli) de la fenêtre au premier plan.
    pub fn fenetre_cible() -> (isize, String, bool) {
        let h = unsafe { GetForegroundWindow() };
        if h.is_null() {
            return (0, String::new(), false);
        }
        let mut pid = 0u32;
        let mut buf = [0u16; 256];
        let n = unsafe {
            GetWindowThreadProcessId(h, &mut pid);
            GetWindowTextW(h, buf.as_mut_ptr(), 256)
        };
        (h as isize, String::from_utf16_lossy(&buf[..n.max(0) as usize]), pid != std::process::id())
    }

    pub fn activer(h: isize) {
        unsafe { SetForegroundWindow(h as _) };
    }

    pub fn reduire(h: isize) {
        unsafe { ShowWindow(h as _, SW_MINIMIZE) };
    }

    pub fn restaurer(h: isize) {
        unsafe {
            ShowWindow(h as _, SW_RESTORE);
            SetForegroundWindow(h as _);
        }
    }

    pub enum Morceau<'a> {
        Texte(&'a str),
        Touche(u16),
    }
    pub const VK_TAB: u16 = 0x09;
    pub const VK_RETURN: u16 = 0x0D;

    fn touche(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    /// Tape chaque morceau dans la fenêtre au premier plan (SendInput, comme KeePassXC).
    pub fn taper(morceaux: &[Morceau]) -> Result<(), String> {
        let mut seq = vec![];
        for m in morceaux {
            match m {
                Morceau::Touche(vk) => seq.extend([touche(*vk, 0, 0), touche(*vk, 0, KEYEVENTF_KEYUP)]),
                // unités UTF-16 : gère aussi les caractères hors BMP
                Morceau::Texte(t) => {
                    for c in t.encode_utf16() {
                        seq.extend([touche(0, c, KEYEVENTF_UNICODE), touche(0, c, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]);
                    }
                }
            }
        }
        let n = unsafe { SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        for i in seq.iter_mut() {
            unsafe { std::ptr::write_volatile(i, std::mem::zeroed()) }; // ne pas laisser le mot de passe dans ce tampon
        }
        if n as usize != seq.len() {
            return Err("Saisie bloquée par Windows".into());
        }
        Ok(())
    }
}

// ---------- interface ----------

#[derive(Default)]
struct FormEntree {
    id: Option<String>,
    site: String,
    identifiant: String,
    mdp: String,
    notes: String,
    longueur: usize,
    symboles: bool,
    visible: bool,
    erreur: String,
}

enum Dialogue {
    Entree(FormEntree),
    Maitre { ancien: String, nouveau: String, conf: String, erreur: String },
    Confirmer { h: isize, titre: String, ident: String, mdp: String, site: String },
}

enum Retour {
    Statut(String),
    Confirmer { h: isize, titre: String, ident: String, mdp: String, site: String },
}

enum Action {
    Copier(String),
    Remplir(Entree),
    Modifier(Entree),
    Supprimer(String),
    Nouveau,
    Verrouiller,
    Maitre,
    Exporter,
}

pub struct CoffreUi {
    chemin: PathBuf,
    coffre: Option<Coffre>,
    mdp: String,
    conf: String,
    info: String,
    focus: bool,
    recherche: String,
    selection: Option<String>,
    visible: bool,
    confirmer_suppr: Option<String>,
    statut: String,
    copie: Option<(u32, Instant)>,
    activite: Instant,
    dernier_tic: Instant,
    dialogue: Option<Dialogue>,
    moi: isize, // fenêtre du hub, pour la réduire/restaurer pendant la saisie auto
    retour: Arc<Mutex<Option<Retour>>>,
}

impl Drop for CoffreUi {
    fn drop(&mut self) {
        self.effacer_presse_papiers();
    }
}

impl CoffreUi {
    /// Mots de passe faibles, si le coffre est ouvert (pour l'accueil).
    pub fn faibles(&self) -> Option<usize> {
        self.coffre.as_ref().map(|c| c.entrees.iter().filter(|e| force_mot_de_passe(&e.mdp) == "faible").count())
    }
}

fn titre60(t: &str) -> String {
    t.chars().take(60).collect()
}

/// Saisie auto (fil séparé : la fenêtre est réduite, egui peut ne plus se redessiner).
fn fil_saisie(moi: isize, ident: String, mdp: String, site: String, confirmee: Option<isize>,
              retour: Arc<Mutex<Option<Retour>>>, ctx: egui::Context) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(if confirmee.is_some() { 400 } else { 600 }));
        let (h, titre, autre_appli) = win::fenetre_cible();
        let r = if !autre_appli || titre.is_empty() || confirmee.is_some_and(|c| c != h) {
            win::restaurer(moi);
            Retour::Statut("Saisie annulée : cliquez d'abord dans le champ du site, puis sur Remplir.".into())
        } else {
            let cle = mot_cle_site(&site);
            // Entrée seulement si la fenêtre semble être le bon site
            let valider = cle.chars().count() >= 3 && titre.to_lowercase().contains(&cle);
            if !valider && confirmee.is_none() {
                // Fenêtre inattendue (messagerie, autre appli…) : ne pas y taper le mot de passe sans accord.
                win::restaurer(moi);
                Retour::Confirmer { h, titre, ident, mdp, site }
            } else {
                let mut m = vec![];
                if !ident.is_empty() {
                    m.extend([win::Morceau::Texte(&ident), win::Morceau::Touche(win::VK_TAB)]);
                }
                m.push(win::Morceau::Texte(&mdp));
                if valider {
                    m.push(win::Morceau::Touche(win::VK_RETURN));
                }
                match win::taper(&m) {
                    Ok(()) => Retour::Statut(format!("Saisi dans « {} »{}", titre60(&titre),
                        if valider { "" } else { " — vérifiez puis appuyez sur Entrée" })),
                    Err(e) => {
                        win::restaurer(moi);
                        Retour::Statut(format!("Saisie impossible : {e}"))
                    }
                }
            }
        };
        *retour.lock().unwrap() = Some(r);
        ctx.request_repaint();
    });
}

fn champ_mdp(ui: &mut egui::Ui, texte: &mut String, largeur: f32) -> egui::Response {
    ui.add(TextEdit::singleline(texte).password(true).desired_width(largeur))
}

impl CoffreUi {
    pub fn new() -> Self {
        let appdata = std::env::var_os("APPDATA").map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from)).unwrap_or_default();
        CoffreUi {
            chemin: appdata.join("CoffreMotsDePasse").join("coffre.json"),
            coffre: None, mdp: String::new(), conf: String::new(), info: String::new(), focus: true,
            recherche: String::new(), selection: None, visible: false, confirmer_suppr: None, statut: String::new(),
            copie: None, activite: Instant::now(), dernier_tic: Instant::now(), dialogue: None, moi: 0,
            retour: Arc::new(Mutex::new(None)),
        }
    }

    /// Tâche périodique (même hors de l'onglet) : verrouillage auto, presse-papiers, retour de la saisie auto.
    pub fn tic(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| !i.events.is_empty()) {
            self.activite = Instant::now();
        }
        let retour = self.retour.lock().unwrap().take();
        if let (Some(r), true) = (retour, self.coffre.is_some()) {
            match r {
                Retour::Statut(s) => self.statut = s,
                Retour::Confirmer { h, titre, ident, mdp, site } => {
                    self.dialogue = Some(Dialogue::Confirmer { h, titre, ident, mdp, site })
                }
            }
        }
        if self.dernier_tic.elapsed() >= Duration::from_secs(1) {
            self.dernier_tic = Instant::now();
            if self.coffre.is_some() && (self.activite.elapsed() > DELAI_VERROU || win::session_verrouillee()) {
                self.verrouiller();
            }
        }
        if self.copie.is_some_and(|(_, fin)| Instant::now() >= fin) {
            self.effacer_presse_papiers();
            if self.coffre.is_some() {
                self.statut = "Presse-papiers effacé".into();
            }
        }
    }

    fn effacer_presse_papiers(&mut self) {
        if let Some((seq, _)) = self.copie.take() {
            let _ = win::effacer_si_inchange(seq);
        }
    }

    fn copier(&mut self, valeur: String) {
        match win::copier(&valeur) {
            Ok(seq) => {
                self.copie = Some((seq, Instant::now() + DELAI_PRESSE_PAPIERS));
                // effacement garanti même si la fenêtre est réduite et ne se redessine plus
                std::thread::spawn(move || {
                    std::thread::sleep(DELAI_PRESSE_PAPIERS);
                    let _ = win::effacer_si_inchange(seq);
                });
            }
            Err(e) => self.statut = format!("Copie impossible : {e}"),
        }
    }

    fn verrouiller(&mut self) {
        self.effacer_presse_papiers();
        self.coffre = None;
        self.selection = None;
        self.dialogue = None;
        self.confirmer_suppr = None;
        self.visible = false;
        self.statut.clear();
        self.focus = true;
    }

    /// Sauvegarde ; en cas d'échec, restaure l'état précédent pour rester cohérent avec le disque.
    fn sauver(&mut self, avant: Vec<Entree>) -> bool {
        let Some(c) = self.coffre.as_mut() else { return false };
        match c.enregistrer() {
            Ok(()) => true,
            Err(e) => {
                c.entrees = avant;
                self.statut = format!("Impossible d'enregistrer le coffre : {e}");
                false
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if self.coffre.is_none() {
            self.ecran_verrou(ui);
        } else {
            self.ecran_principal(ui);
            self.dialogues(&ui.ctx().clone());
        }
    }

    fn ecran_verrou(&mut self, ui: &mut egui::Ui) {
        let existe = self.chemin.exists();
        let mut valider = false;
        ui.vertical_centered(|ui| {
            ui.add_space(100.0);
            ui.heading("Coffre à mots de passe");
            ui.label(RichText::new(if existe { "Entrez votre mot de passe maître".to_string() } else {
                format!("Créez votre mot de passe maître ({LONGUEUR_MIN_MAITRE} caractères min.)")
            }).color(GRIS));
            ui.add_space(12.0);
            let r = champ_mdp(ui, &mut self.mdp, 300.0);
            if std::mem::take(&mut self.focus) {
                r.request_focus();
            }
            let entree = |r: &egui::Response, ui: &egui::Ui| r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            valider |= entree(&r, ui);
            if !existe {
                ui.label(RichText::new("Confirmation").color(GRIS));
                let r = champ_mdp(ui, &mut self.conf, 300.0);
                valider |= entree(&r, ui);
                ui.label(RichText::new("Il ne pourra pas être récupéré en cas d'oubli.").color(GRIS));
            }
            if win::verr_maj() {
                ui.label(RichText::new("Attention : Verr. Maj est activé").color(GRIS));
            }
            if !self.info.is_empty() {
                ui.colored_label(ROUGE, &self.info);
            }
            ui.add_space(8.0);
            valider |= ui.button(if existe { "Déverrouiller" } else { "Créer le coffre" }).clicked();
        });
        if !valider {
            return;
        }
        let m = std::mem::take(&mut self.mdp);
        let r = if existe {
            Coffre::ouvrir(&self.chemin, &m).map(|mut c| {
                let _ = c.renforcer_kdf(&m); // en cas d'échec le coffre reste ouvert avec ses anciens paramètres
                c
            })
        } else if m.chars().count() < LONGUEUR_MIN_MAITRE {
            self.info = format!("Au moins {LONGUEUR_MIN_MAITRE} caractères.");
            self.mdp = m;
            return;
        } else if m != self.conf {
            self.info = "Les deux mots de passe diffèrent.".into();
            self.mdp = m;
            return;
        } else {
            Coffre::creer(&self.chemin, &m, KDF_DEFAUT)
        };
        match r {
            Ok(c) => {
                self.coffre = Some(c);
                self.conf.clear();
                self.info.clear();
                self.activite = Instant::now();
            }
            Err(Erreur::MauvaisMotDePasse) => {
                self.info = "Mot de passe maître incorrect.".into();
                self.focus = true;
            }
            Err(e) => self.info = format!("Erreur : {e}"),
        }
    }

    fn ecran_principal(&mut self, ui: &mut egui::Ui) {
        let coffre = self.coffre.as_ref().unwrap();
        let q = self.recherche.trim().to_lowercase();
        let mut liste: Vec<&Entree> = coffre.entrees.iter()
            .filter(|e| q.is_empty() || [&e.site, &e.identifiant, &e.notes].iter().any(|v| v.to_lowercase().contains(&q)))
            .collect();
        liste.sort_by_key(|e| e.site.to_lowercase());
        let liste: Vec<(String, String, String)> = liste.iter().map(|e| (e.id.clone(), e.site.clone(), e.identifiant.clone())).collect();
        let sel = self.selection.as_ref().filter(|id| liste.iter().any(|l| &l.0 == *id))
            .and_then(|id| coffre.entrees.iter().find(|e| &e.id == id)).cloned();
        let n = coffre.entrees.len();
        let vide = n == 0;

        let mut action = None;
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::N)) {
            action = Some(Action::Nouveau);
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::L)) {
            action = Some(Action::Verrouiller);
        }

        egui::TopBottomPanel::top("coffre_haut").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Coffre");
                ui.add_space(12.0);
                ui.add(TextEdit::singleline(&mut self.recherche).hint_text("Rechercher").desired_width(260.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("Options", |ui| {
                        if ui.button("Changer le mot de passe maître…").clicked() {
                            action = Some(Action::Maitre);
                            ui.close_menu();
                        }
                        if ui.button("Exporter une sauvegarde chiffrée…").clicked() {
                            action = Some(Action::Exporter);
                            ui.close_menu();
                        }
                    });
                    if ui.button("Verrouiller").clicked() {
                        action = Some(Action::Verrouiller);
                    }
                    if ui.button("+  Nouveau").clicked() {
                        action = Some(Action::Nouveau);
                    }
                });
            });
            ui.add_space(4.0);
        });

        let statut = match self.copie {
            Some((_, fin)) => format!("Copié — effacé du presse-papiers dans {} s",
                fin.saturating_duration_since(Instant::now()).as_secs()),
            None if !self.statut.is_empty() => self.statut.clone(),
            None => format!("{n} entrée{}", if n > 1 { "s" } else { "" }),
        };
        egui::TopBottomPanel::bottom("coffre_statut").show_inside(ui, |ui| {
            ui.label(RichText::new(statut).color(GRIS));
        });

        egui::SidePanel::right("coffre_detail").exact_width(320.0).resizable(false).show_inside(ui, |ui| {
            let Some(en) = &sel else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(if vide { "Aucune entrée.\nCliquez sur « + Nouveau » pour commencer." }
                        else { "Sélectionnez une entrée." }).color(GRIS));
                });
                return;
            };
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(&en.site).size(18.0).strong()).wrap());
            if !en.mdp.is_empty() && ui.button("Remplir le site").clicked() {
                action = Some(Action::Remplir(en.clone()));
            }
            for (titre, valeur, masque) in [("Identifiant", &en.identifiant, false), ("Mot de passe", &en.mdp, true)] {
                ui.add_space(12.0);
                ui.label(RichText::new(titre).color(GRIS));
                ui.horizontal(|ui| {
                    let texte = if masque && !self.visible { "••••••••••••" } else if valeur.is_empty() { "—" } else { valeur };
                    ui.label(RichText::new(texte).size(15.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !valeur.is_empty() && ui.button("Copier").clicked() {
                            action = Some(Action::Copier(valeur.clone()));
                        }
                        if masque && ui.button(if self.visible { "Masquer" } else { "Afficher" }).clicked() {
                            self.visible = !self.visible;
                        }
                    });
                });
            }
            if !en.notes.is_empty() {
                ui.add_space(12.0);
                ui.label(RichText::new("Notes").color(GRIS));
                ui.add(egui::Label::new(&en.notes).wrap());
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(8.0);
                if self.confirmer_suppr.as_ref() == Some(&en.id) {
                    ui.horizontal(|ui| {
                        if ui.button("Oui, supprimer").clicked() {
                            action = Some(Action::Supprimer(en.id.clone()));
                        }
                        if ui.button("Non").clicked() {
                            self.confirmer_suppr = None;
                        }
                    });
                    ui.colored_label(ROUGE, format!("Supprimer « {} » ?", en.site));
                } else {
                    ui.horizontal(|ui| {
                        if ui.button("Modifier").clicked() {
                            action = Some(Action::Modifier(en.clone()));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(RichText::new("Supprimer").color(ROUGE)).clicked() {
                                self.confirmer_suppr = Some(en.id.clone());
                            }
                        });
                    });
                }
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Grid::new("coffre_liste").num_columns(2).striped(true).min_col_width(200.0).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label(RichText::new("SITE / SERVICE").small().color(GRIS));
                    ui.label(RichText::new("IDENTIFIANT").small().color(GRIS));
                    ui.end_row();
                    for (id, site, ident) in &liste {
                        let r = ui.selectable_label(self.selection.as_ref() == Some(id), site);
                        if r.clicked() && self.selection.as_ref() != Some(id) {
                            self.selection = Some(id.clone());
                            self.visible = false;
                            self.confirmer_suppr = None;
                        }
                        if r.double_clicked() {
                            if let Some(en) = self.coffre.as_ref().and_then(|c| c.entrees.iter().find(|e| &e.id == id)) {
                                action = Some(Action::Modifier(en.clone()));
                            }
                        }
                        ui.label(ident);
                        ui.end_row();
                    }
                });
            });
        });

        match action {
            None => {}
            Some(Action::Copier(v)) => self.copier(v),
            Some(Action::Remplir(en)) => {
                self.moi = win::premier_plan(); // le hub est devant : l'utilisateur vient de cliquer
                win::reduire(self.moi);
                fil_saisie(self.moi, en.identifiant, en.mdp, en.site, None, self.retour.clone(), ui.ctx().clone());
            }
            Some(Action::Modifier(en)) => {
                self.dialogue = Some(Dialogue::Entree(FormEntree {
                    id: Some(en.id), site: en.site, identifiant: en.identifiant, mdp: en.mdp, notes: en.notes,
                    longueur: 20, symboles: true, ..Default::default()
                }))
            }
            Some(Action::Nouveau) => {
                self.dialogue = Some(Dialogue::Entree(FormEntree {
                    mdp: generer_mot_de_passe(20, true), longueur: 20, symboles: true, ..Default::default()
                }))
            }
            Some(Action::Supprimer(id)) => {
                let c = self.coffre.as_mut().unwrap();
                let avant = c.entrees.clone();
                c.entrees.retain(|e| e.id != id);
                self.selection = None;
                self.confirmer_suppr = None;
                self.sauver(avant);
            }
            Some(Action::Verrouiller) => self.verrouiller(),
            Some(Action::Maitre) => {
                self.dialogue = Some(Dialogue::Maitre { ancien: String::new(), nouveau: String::new(), conf: String::new(), erreur: String::new() })
            }
            Some(Action::Exporter) => self.exporter(),
        }
    }

    fn exporter(&mut self) {
        let Some(dest) = rfd::FileDialog::new().set_title("Exporter une sauvegarde chiffrée")
            .set_file_name("coffre-sauvegarde.json").add_filter("Coffre chiffré", &["json"]).save_file() else { return };
        let meme = fs::canonicalize(&dest).ok().is_some_and(|d| fs::canonicalize(&self.chemin).ok() == Some(d));
        self.statut = if meme {
            "Export impossible : choisissez un autre emplacement que le coffre lui-même.".into()
        } else {
            match fs::copy(&self.chemin, &dest) {
                Ok(_) => format!("Sauvegarde exportée : {}", dest.display()),
                Err(e) => format!("Export impossible : {e}"),
            }
        };
    }

    fn dialogues(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dialogue.take() else { return };
        let mut garder = !ctx.input(|i| i.key_pressed(Key::Escape));
        let mut valider = false;
        let fenetre = |titre: &str| egui::Window::new(titre.to_string()).collapsible(false).resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0]);
        match &mut d {
            Dialogue::Entree(f) => {
                let autres: Vec<String> = self.coffre.as_ref().map(|c| c.entrees.iter()
                    .filter(|e| Some(&e.id) != f.id.as_ref() && !f.mdp.is_empty() && e.mdp == f.mdp)
                    .take(3).map(|e| e.site.clone()).collect()).unwrap_or_default();
                fenetre(if f.id.is_some() { "Modifier l'entrée" } else { "Nouvelle entrée" }).show(ctx, |ui| {
                    ui.label(RichText::new("Site / service").color(GRIS));
                    ui.add(TextEdit::singleline(&mut f.site).desired_width(380.0));
                    ui.label(RichText::new("Identifiant").color(GRIS));
                    ui.add(TextEdit::singleline(&mut f.identifiant).desired_width(380.0));
                    ui.label(RichText::new("Mot de passe").color(GRIS));
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut f.mdp).password(!f.visible).desired_width(230.0));
                        if ui.button(if f.visible { "Masquer" } else { "Afficher" }).clicked() {
                            f.visible = !f.visible;
                        }
                        if ui.button("Générer").clicked() {
                            f.mdp = generer_mot_de_passe(f.longueur, f.symboles);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Longueur").color(GRIS));
                        ui.add(egui::DragValue::new(&mut f.longueur).range(8..=64));
                        ui.checkbox(&mut f.symboles, "Symboles");
                    });
                    if !f.mdp.is_empty() {
                        let mut t = format!("Force : {}", force_mot_de_passe(&f.mdp));
                        if !autres.is_empty() {
                            t += &format!("  ·  déjà utilisé pour {}", autres.join(", "));
                        }
                        ui.label(RichText::new(t).color(GRIS));
                    }
                    ui.label(RichText::new("Notes").color(GRIS));
                    ui.add(TextEdit::multiline(&mut f.notes).desired_rows(4).desired_width(380.0));
                    if !f.erreur.is_empty() {
                        ui.colored_label(ROUGE, &f.erreur);
                    }
                    ui.horizontal(|ui| {
                        valider = ui.button("Enregistrer").clicked();
                        if ui.button("Annuler").clicked() {
                            garder = false;
                        }
                    });
                });
                if valider {
                    let site = f.site.trim().to_string();
                    if site.is_empty() || f.mdp.is_empty() {
                        f.erreur = "Le site et le mot de passe sont obligatoires.".into();
                    } else if let Some(c) = self.coffre.as_mut() {
                        let avant = c.entrees.clone();
                        let notes = f.notes.trim().to_string();
                        let maj = |e: &mut Entree| {
                            (e.site, e.identifiant, e.mdp, e.notes) = (site.clone(), f.identifiant.clone(), f.mdp.clone(), notes.clone())
                        };
                        match f.id.as_ref().and_then(|id| c.entrees.iter_mut().find(|e| &e.id == id)) {
                            Some(e) => maj(e),
                            None => {
                                let mut e = Entree { id: hex(&aleatoire::<16>()), ..Default::default() };
                                maj(&mut e);
                                self.selection = Some(e.id.clone());
                                c.entrees.push(e);
                            }
                        }
                        garder = false;
                        if self.sauver(avant) {
                            self.statut = "Entrée enregistrée".into();
                        }
                    }
                }
            }
            Dialogue::Maitre { ancien, nouveau, conf, erreur } => {
                fenetre("Changer le mot de passe maître").show(ctx, |ui| {
                    for (lib, champ) in [("Mot de passe actuel".to_string(), &mut *ancien),
                        (format!("Nouveau ({LONGUEUR_MIN_MAITRE} caractères min.)"), &mut *nouveau), ("Confirmation".into(), &mut *conf)] {
                        ui.label(RichText::new(lib).color(GRIS));
                        champ_mdp(ui, champ, 320.0);
                    }
                    if !erreur.is_empty() {
                        ui.colored_label(ROUGE, &*erreur);
                    }
                    ui.horizontal(|ui| {
                        valider = ui.button("Changer").clicked();
                        if ui.button("Annuler").clicked() {
                            garder = false;
                        }
                    });
                });
                if valider {
                    if nouveau.chars().count() < LONGUEUR_MIN_MAITRE {
                        *erreur = format!("Au moins {LONGUEUR_MIN_MAITRE} caractères.");
                    } else if nouveau != conf {
                        *erreur = "Les deux mots de passe diffèrent.".into();
                    } else if let Some(c) = self.coffre.as_mut() {
                        match c.changer_mot_de_passe(ancien, nouveau) {
                            Ok(()) => {
                                garder = false;
                                self.statut = "Mot de passe maître changé".into();
                            }
                            Err(Erreur::MauvaisMotDePasse) => *erreur = "Mot de passe actuel incorrect.".into(),
                            Err(e) => *erreur = format!("Erreur : {e}"),
                        }
                    }
                }
            }
            Dialogue::Confirmer { h, titre, ident, mdp, site } => {
                let mut oui = false;
                fenetre("Remplir").show(ctx, |ui| {
                    ui.label(format!("La fenêtre « {} » ne semble pas être « {site} ».\nY taper l'identifiant et le mot de passe ?", titre60(titre)));
                    ui.horizontal(|ui| {
                        oui = ui.button("Oui").clicked();
                        if ui.button("Non").clicked() {
                            garder = false;
                            self.statut = "Saisie annulée".into();
                        }
                    });
                });
                if oui {
                    garder = false;
                    win::reduire(self.moi);
                    win::activer(*h);
                    fil_saisie(self.moi, std::mem::take(ident), std::mem::take(mdp), std::mem::take(site), Some(*h),
                        self.retour.clone(), ctx.clone());
                }
            }
        }
        if garder {
            self.dialogue = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAPIDE: Kdf = Kdf { iterations: 1, memoire_kio: 64, voies: 1 };

    fn dossier() -> PathBuf {
        let d = std::env::temp_dir().join(format!("coffre-test-{}", hex(&aleatoire::<6>())));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn aad_comme_python() {
        assert_eq!(aad(&KDF_DEFAUT.json(), "c2Vs"),
            r#"{"kdf":{"iterations":3,"memoire_kio":65536,"nom":"argon2id","voies":4},"sel":"c2Vs","version":1}"#);
    }

    #[test]
    fn ouvre_un_coffre_python() {
        let c = Coffre::ouvrir(Path::new("src/coffre_python.json"), "motdepasse-test").unwrap();
        assert_eq!(c.entrees.len(), 1);
        let e = &c.entrees[0];
        assert_eq!((e.id.as_str(), e.site.as_str(), e.identifiant.as_str(), e.mdp.as_str(), e.notes.as_str()),
            ("a1", "github.com", "moi", "Sécret!42", "note é"));
        assert_eq!(Coffre::ouvrir(Path::new("src/coffre_python.json"), "faux").err(), Some(Erreur::MauvaisMotDePasse));
    }

    #[test]
    fn aller_retour_et_conflit() {
        let f = dossier().join("coffre.json");
        let mut c = Coffre::creer(&f, "maitre-123456", RAPIDE).unwrap();
        assert!(Coffre::creer(&f, "x", RAPIDE).is_err());
        c.entrees.push(Entree { id: "1".into(), site: "été".into(), mdp: "p".into(), ..Default::default() });
        c.enregistrer().unwrap();
        let mut d = Coffre::ouvrir(&f, "maitre-123456").unwrap();
        assert_eq!(d.entrees, c.entrees);
        assert_eq!(Coffre::ouvrir(&f, "autre").err(), Some(Erreur::MauvaisMotDePasse));
        c.enregistrer().unwrap(); // une autre fenêtre écrit
        assert_eq!(d.enregistrer(), Err(Erreur::ModifieAilleurs));
        let _ = fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn en_tete_altere_ou_invalide() {
        let f = dossier().join("coffre.json");
        Coffre::creer(&f, "maitre-123456", RAPIDE).unwrap();
        let mut v = lire(&f).unwrap();
        v["kdf"]["iterations"] = json!(2);
        fs::write(&f, v.to_string()).unwrap();
        assert_eq!(Coffre::ouvrir(&f, "maitre-123456").err(), Some(Erreur::MauvaisMotDePasse));
        for (k, val) in [("iterations", json!(2.0)), ("memoire_kio", json!(2_000_000)), ("voies", json!(0))] {
            let mut w = v.clone();
            w["kdf"][k] = val;
            fs::write(&f, w.to_string()).unwrap();
            assert_eq!(Coffre::ouvrir(&f, "maitre-123456").err(), Some(Erreur::Invalide("Paramètres de dérivation invalides")));
        }
        v["version"] = json!(2);
        fs::write(&f, v.to_string()).unwrap();
        assert_eq!(Coffre::ouvrir(&f, "x").err(), Some(Erreur::Invalide("Version de coffre non prise en charge")));
        let _ = fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn changer_et_renforcer() {
        let f = dossier().join("coffre.json");
        let mut c = Coffre::creer(&f, "ancien-123456", RAPIDE).unwrap();
        assert_eq!(c.changer_mot_de_passe("faux", "x"), Err(Erreur::MauvaisMotDePasse));
        assert!(c.renforcer_kdf("ancien-123456").unwrap());
        assert_eq!(Kdf::lire(&c.kdf).unwrap(), KDF_DEFAUT);
        assert!(!c.renforcer_kdf("ancien-123456").unwrap());
        c.changer_mot_de_passe("ancien-123456", "nouveau-123456").unwrap();
        assert!(Coffre::ouvrir(&f, "nouveau-123456").is_ok());
        let _ = fs::remove_dir_all(f.parent().unwrap());
    }

    #[test]
    fn force_et_generateur() {
        assert_eq!(force_mot_de_passe("abc"), "faible");
        assert_eq!(force_mot_de_passe("motdepasse"), "faible");
        assert_eq!(force_mot_de_passe("Abcdefgh1234"), "moyen");
        assert_eq!(force_mot_de_passe(&generer_mot_de_passe(20, true)), "fort");
        let m = generer_mot_de_passe(20, true);
        assert_eq!(m.chars().count(), 20);
        assert!(m.chars().any(|c| SYMBOLES.contains(c)) && m.chars().any(|c| c.is_ascii_digit()));
        assert!(!generer_mot_de_passe(30, false).chars().any(|c| SYMBOLES.contains(c)));
        assert_eq!(generer_mot_de_passe(2, true).len(), 2);
    }

    #[test]
    fn mot_cle() {
        assert_eq!(mot_cle_site("https://www.github.com/login"), "github");
        assert_eq!(mot_cle_site("localhost:8080"), "localhost");
        assert_eq!(mot_cle_site("Discord"), "discord");
    }
}
