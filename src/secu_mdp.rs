//! Mots de passe et comptes : solidité détaillée (motifs repérés, temps pour le casser),
//! audit du coffre (réutilisés, faibles, fuités, sites piratés, double authentification),
//! codes de double authentification (TOTP) chiffrés, et fuites de données des services que tu utilises.
use crate::outil::{charger, enregistrer, Cat, Outil};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ===================================================================================================
// 7 · Solidité d'un mot de passe
// ===================================================================================================

const COMMUNS: &[&str] = &[
    "123456", "password", "123456789", "12345678", "12345", "qwerty", "azerty", "azertyuiop", "motdepasse", "1234567", "111111", "123123", "abc123", "000000", "iloveyou",
    "soleil", "doudou", "chouchou", "loulou", "marseille", "football", "princesse", "nicolas", "camille", "julien", "bonjour", "coucou", "jetaime", "admin", "welcome",
    "monkey", "dragon", "master", "sunshine", "princess", "letmein", "shadow", "baseball", "superman", "michael", "minecraft", "fortnite", "pokemon", "naruto", "roblox",
    "liverpool", "arsenal", "chelsea", "barcelona", "psg", "olympique", "france", "paris", "bienvenue", "secret", "test", "qwertyuiop", "1q2w3e4r", "zaq12wsx", "passw0rd",
    "azerty123", "password1", "qwerty123", "123qwe", "aaaaaa", "654321", "121212", "666666", "696969", "7777777", "987654321", "batman", "spiderman", "starwars", "hello",
    "freedom", "whatever", "trustno1", "charlie", "jordan", "thomas", "maxime", "alexandre", "lucas", "chocolat", "papillon", "tigrou", "nounours", "amour", "chien", "chat",
];

const MOTS: &[&str] = &[
    "amour", "maison", "soleil", "chat", "chien", "bonjour", "france", "paris", "football", "jeux", "jeu", "pomme", "fraise", "bleu", "rouge", "vert", "noir", "blanc",
    "lune", "etoile", "dragon", "tigre", "lion", "loup", "princesse", "prince", "roi", "reine", "ange", "demon", "super", "mega", "ultra", "pro", "game", "gamer",
    "love", "baby", "angel", "black", "white", "blue", "red", "green", "happy", "money", "power", "dark", "light", "fire", "water", "summer", "winter", "spring",
    "password", "admin", "user", "login", "secret", "master", "killer", "ninja", "pirate", "minecraft", "fortnite", "pokemon", "mario", "zelda", "sonic", "naruto",
];

const CLAVIERS: &[&str] = &["azertyuiop", "qsdfghjklm", "wxcvbn", "qwertyuiop", "asdfghjkl", "zxcvbnm", "1234567890", "0987654321", "abcdefghijklmnopqrstuvwxyz", "zyxwvutsrqponmlkjihgfedcba", "azerty", "qwerty"];

/// Remplace les déguisements courants (« p@ssw0rd » → « password »).
pub fn sans_leet(m: &str) -> String {
    m.to_lowercase().chars().map(|c| match c {
        '@' | '4' => 'a',
        '0' => 'o',
        '1' | '!' => 'i',
        '3' => 'e',
        '$' | '5' => 's',
        '7' => 't',
        '8' => 'b',
        x => x,
    }).collect()
}

#[derive(Clone, Debug)]
pub struct AnalyseMdp {
    pub bits: f64,
    pub note: u8, // 0 à 4
    pub motifs: Vec<String>,
}

fn alphabet(m: &str) -> f64 {
    let mut n = 0;
    if m.chars().any(|c| c.is_ascii_lowercase()) {
        n += 26;
    }
    if m.chars().any(|c| c.is_ascii_uppercase()) {
        n += 26;
    }
    if m.chars().any(|c| c.is_ascii_digit()) {
        n += 10;
    }
    if m.chars().any(|c| !c.is_ascii_alphanumeric()) {
        n += 33;
    }
    (n.max(10)) as f64
}

pub fn analyser_mdp(m: &str) -> AnalyseMdp {
    let mut motifs = vec![];
    if m.is_empty() {
        return AnalyseMdp { bits: 0.0, note: 0, motifs };
    }
    let bas = m.to_lowercase();
    let leet = sans_leet(m);
    if COMMUNS.contains(&bas.as_str()) || COMMUNS.contains(&leet.as_str()) {
        motifs.push("fait partie des mots de passe les plus utilisés au monde : essayé en premier".into());
        return AnalyseMdp { bits: 10.0, note: 0, motifs };
    }
    let a = alphabet(m);
    // caractères « expliqués » par un motif : on compte le motif, pas chaque caractère
    let chars: Vec<char> = leet.chars().collect();
    let n = chars.len();
    let mut couvert = vec![false; n];
    let mut bits_motifs: f64 = 0.0;
    let marquer = |debut: usize, fin: usize, couvert: &mut Vec<bool>| {
        for c in couvert.iter_mut().take(fin).skip(debut) {
            *c = true;
        }
    };
    for k in CLAVIERS {
        for lon in (4..=k.len()).rev() {
            let mut trouve = false;
            for i in 0..=k.len() - lon {
                let bout = &k[i..i + lon];
                if let Some(p) = leet.find(bout) {
                    let d = leet[..p].chars().count();
                    if !couvert[d] {
                        marquer(d, d + lon, &mut couvert);
                        bits_motifs += 6.0;
                        motifs.push(format!("suite du clavier ou de l'alphabet « {bout} »"));
                        trouve = true;
                        break;
                    }
                }
            }
            if trouve {
                break;
            }
        }
    }
    for w in MOTS.iter().filter(|w| w.len() >= 4) {
        if let Some(p) = leet.find(w) {
            let d = leet[..p].chars().count();
            if !couvert[d] {
                marquer(d, d + w.chars().count(), &mut couvert);
                bits_motifs += 11.0;
                motifs.push(format!("mot courant « {w} »"));
            }
        }
    }
    // années et dates
    let b = bas.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        if !bas.is_char_boundary(i) || !bas.is_char_boundary(i + 4) {
            i += 1;
            continue;
        }
        let s = &bas[i..i + 4];
        if (s.starts_with("19") || s.starts_with("20")) && s.chars().all(|c| c.is_ascii_digit()) {
            let d = bas[..i].chars().count();
            if d < n && !couvert[d] {
                marquer(d, d + 4, &mut couvert);
                bits_motifs += 7.0;
                motifs.push(format!("année « {s} » (souvent une date de naissance)"));
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    // répétitions
    let mut k = 0;
    while k < n {
        let mut j = k;
        while j + 1 < n && chars[j + 1] == chars[k] {
            j += 1;
        }
        if j - k >= 2 {
            marquer(k, j + 1, &mut couvert);
            bits_motifs += 4.0;
            motifs.push(format!("caractère répété « {} »", chars[k].to_string().repeat(j - k + 1)));
        }
        k = j + 1;
    }
    if n >= 6 && n % 2 == 0 && chars[..n / 2] == chars[n / 2..] {
        motifs.push("le même morceau écrit deux fois".into());
        bits_motifs -= 6.0;
    }
    if m.chars().next().is_some_and(|c| c.is_uppercase()) && m.chars().skip(1).all(|c| !c.is_uppercase()) && m.chars().any(|c| c.is_lowercase()) {
        motifs.push("majuscule seulement au début (très prévisible)".into());
    }
    if m.chars().last().is_some_and(|c| c.is_ascii_digit() || "!?.".contains(c)) && m.chars().filter(|c| c.is_ascii_digit()).count() <= 3 {
        motifs.push("chiffre ou signe à la fin (le premier endroit essayé)".into());
    }
    let libres = couvert.iter().filter(|c| !**c).count() as f64;
    let bits = (libres * a.log2() + bits_motifs.max(0.0)).max(1.0);
    if n < 10 {
        motifs.push(format!("seulement {n} caractères : vise au moins 12 (ou 4 mots au hasard)"));
    }
    let note = match bits as u32 {
        0..=27 => 0,
        28..=44 => 1,
        45..=59 => 2,
        60..=79 => 3,
        _ => 4,
    };
    AnalyseMdp { bits, note, motifs }
}

pub fn duree_lisible(s: f64) -> String {
    if s < 1.0 {
        "moins d'une seconde".into()
    } else if s < 60.0 {
        format!("{s:.0} secondes")
    } else if s < 3600.0 {
        format!("{:.0} minutes", s / 60.0)
    } else if s < 86400.0 {
        format!("{:.0} heures", s / 3600.0)
    } else if s < 86400.0 * 365.0 {
        format!("{:.0} jours", s / 86400.0)
    } else if s < 86400.0 * 365.0 * 1000.0 {
        format!("{:.0} ans", s / 86400.0 / 365.0)
    } else if s < 86400.0 * 365.0 * 1e6 {
        format!("{:.0} siècles", s / 86400.0 / 365.0 / 100.0)
    } else {
        "plus que l'âge de l'univers".into()
    }
}

/// (pirate en ligne limité à 10 essais/s, pirate avec une carte graphique sur un fichier volé : 10 milliards/s)
pub fn temps_cassage(bits: f64) -> (String, String) {
    let essais = 2f64.powf(bits) / 2.0;
    (duree_lisible(essais / 10.0), duree_lisible(essais / 1e10))
}

/// Nombre de fuites du mot de passe (k-anonymat : seuls 5 caractères de son empreinte partent).
pub fn fuites_mdp(m: &str) -> Result<u64, String> {
    let (p, s) = crate::reseau3::decouper_sha1(m);
    let r = ureq::get(&format!("https://api.pwnedpasswords.com/range/{p}")).timeout(Duration::from_secs(10)).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    Ok(crate::reseau3::compter_fuites(&r, &s))
}

pub struct Solidite {
    mdp: String,
    voir: bool,
    fuite: Arc<Mutex<Option<Result<u64, String>>>>,
}

impl Solidite {
    pub fn new() -> Self {
        Solidite { mdp: String::new(), voir: false, fuite: Arc::default() }
    }
}

const NOTES: [(&str, Color32); 5] = [
    ("très faible", Color32::from_rgb(230, 80, 80)),
    ("faible", Color32::from_rgb(240, 140, 70)),
    ("moyen", Color32::from_rgb(240, 200, 70)),
    ("solide", Color32::from_rgb(140, 210, 110)),
    ("très solide", Color32::from_rgb(80, 210, 130)),
];

impl Outil for Solidite {
    fn nom(&self) -> &'static str {
        "🔑 Solidité d'un mot de passe"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Solidité d'un mot de passe");
        ui.weak("Tape un mot de passe : Outils montre comment un pirate le casserait (motifs repérés) et combien de temps ça prendrait. Rien ne quitte le PC, sauf si tu vérifies les fuites (5 caractères de son empreinte seulement).");
        ui.horizontal(|ui| {
            if ui.add(egui::TextEdit::singleline(&mut self.mdp).password(!self.voir).desired_width(320.0).font(egui::TextStyle::Heading)).changed() {
                *self.fuite.lock().unwrap() = None;
            }
            ui.checkbox(&mut self.voir, "voir");
        });
        if self.mdp.is_empty() {
            return;
        }
        let a = analyser_mdp(&self.mdp);
        let (t, c) = NOTES[a.note as usize];
        ui.add(egui::ProgressBar::new((a.note as f32 + 1.0) / 5.0).fill(c).text(RichText::new(format!("{t} · {:.0} bits", a.bits)).strong()).desired_width(360.0));
        let (en_ligne, vole) = temps_cassage(a.bits);
        ui.label(format!("🌐 Sur un site (essais limités) : {en_ligne}"));
        ui.label(format!("💻 Si le site se fait voler ses données (carte graphique de pirate) : {vole}"));
        if !a.motifs.is_empty() {
            ui.strong("Ce qu'un pirate repère :");
            for m in &a.motifs {
                ui.label(format!("• {m}"));
            }
        }
        ui.horizontal(|ui| {
            if ui.button("🕵 A-t-il déjà fuité ?").clicked() {
                let (m, f) = (self.mdp.clone(), self.fuite.clone());
                std::thread::spawn(move || *f.lock().unwrap() = Some(fuites_mdp(&m)));
            }
            match self.fuite.lock().unwrap().clone() {
                Some(Ok(0)) => {
                    ui.colored_label(Color32::LIGHT_GREEN, "✔ Jamais vu dans les fuites connues");
                }
                Some(Ok(n)) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("❗ Vu {n} fois dans des fuites : ne l'utilise plus nulle part"));
                }
                Some(Err(e)) => {
                    ui.label(format!("✖ {e}"));
                }
                None => {}
            }
        });
        ui.weak("Conseil : une phrase de 4 ou 5 mots au hasard (« cheval-agrafe-nuage-violon ») est plus solide et plus facile à retenir qu'un mot compliqué. Et un mot de passe différent par site (le coffre d'Outils s'en souvient).");
    }
}

// ===================================================================================================
// 10 · Fuites de données des services
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
#[serde(default)]
pub struct Fuite {
    #[serde(rename = "Name")]
    pub nom: String,
    #[serde(rename = "Title")]
    pub titre: String,
    #[serde(rename = "Domain")]
    pub domaine: String,
    #[serde(rename = "BreachDate")]
    pub date: String,
    #[serde(rename = "PwnCount")]
    pub comptes: u64,
    #[serde(rename = "DataClasses")]
    pub donnees: Vec<String>,
}

pub fn traduire_donnee(d: &str) -> String {
    match d {
        "Email addresses" => "e-mails",
        "Passwords" => "mots de passe",
        "Usernames" => "pseudos",
        "Names" => "noms",
        "Phone numbers" => "numéros de téléphone",
        "IP addresses" => "adresses IP",
        "Dates of birth" => "dates de naissance",
        "Physical addresses" => "adresses postales",
        "Credit cards" => "cartes bancaires",
        "Partial credit card data" => "morceaux de cartes bancaires",
        "Genders" => "genre",
        "Geographic locations" => "positions",
        "Security questions and answers" => "questions secrètes",
        "Private messages" => "messages privés",
        "Bank account numbers" => "comptes bancaires",
        x => x,
    }
    .to_string()
}

fn cache_fuites() -> PathBuf {
    crate::outil::chemin("fuites_services")
}

/// Liste publique des fuites connues (Have I Been Pwned), gardée une semaine.
pub fn liste_fuites(forcer: bool) -> Result<Vec<Fuite>, String> {
    let f = cache_fuites();
    let recent = f.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(7 * 86400)).unwrap_or(false);
    if recent && !forcer {
        if let Ok(v) = serde_json::from_str::<Vec<Fuite>>(&std::fs::read_to_string(&f).unwrap_or_default()) {
            return Ok(v);
        }
    }
    let r = ureq::get("https://haveibeenpwned.com/api/v3/breaches").set("User-Agent", "Outils").timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let v: Vec<Fuite> = serde_json::from_str(&r).map_err(|e| e.to_string())?;
    let _ = std::fs::write(&f, r);
    Ok(v)
}

/// Domaine principal d'une adresse (« https://www.compte.steam.com/x » → « steam.com »).
pub fn domaine(s: &str) -> String {
    let h = s.trim().trim_start_matches("https://").trim_start_matches("http://").split(['/', '?', '#', ':']).next().unwrap_or("").to_lowercase();
    let p: Vec<&str> = h.split('.').filter(|x| !x.is_empty()).collect();
    if p.len() >= 3 && ["co", "com", "gouv", "org", "net", "ac"].contains(&p[p.len() - 2]) {
        p[p.len() - 3..].join(".")
    } else if p.len() >= 2 {
        p[p.len() - 2..].join(".")
    } else {
        h
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Suivis {
    services: Vec<String>,
    connues: Vec<String>,
    derniere: u64,
}

pub struct Fuites {
    liste: Arc<Mutex<Option<Result<Vec<Fuite>, String>>>>,
    s: Suivis,
    recherche: String,
    lance: bool,
}

impl Fuites {
    pub fn new() -> Self {
        Fuites { liste: Arc::default(), s: charger("fuites_suivis"), recherche: String::new(), lance: false }
    }

    fn charger_liste(&self, forcer: bool) {
        let l = self.liste.clone();
        std::thread::spawn(move || *l.lock().unwrap() = Some(liste_fuites(forcer)));
    }
}

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Outil for Fuites {
    fn nom(&self) -> &'static str {
        "📰 Fuites de données"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        // une fois par jour : nouvelles fuites des services suivis
        if self.lance || self.s.services.is_empty() || maintenant().saturating_sub(self.s.derniere) < 86400 {
            return;
        }
        self.lance = true;
        let services = self.s.services.clone();
        let connues = self.s.connues.clone();
        std::thread::spawn(move || {
            let Ok(v) = liste_fuites(true) else { return };
            let mut s: Suivis = charger("fuites_suivis");
            let nouvelles: Vec<&Fuite> = v.iter().filter(|f| services.contains(&f.nom) && !connues.contains(&f.nom)).collect();
            for f in &nouvelles {
                crate::systeme::notifier_toujours(&format!("Fuite de données : {}", f.titre), &format!("{} comptes, avec : {}. Change ton mot de passe sur ce site.", f.comptes, f.donnees.iter().map(|d| traduire_donnee(d)).collect::<Vec<_>>().join(", ")));
                s.connues.push(f.nom.clone());
            }
            s.derniere = maintenant();
            enregistrer("fuites_suivis", &s);
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fuites de données");
        ui.weak("Les sites qui se sont fait voler les données de leurs utilisateurs (liste publique Have I Been Pwned). Coche ceux où tu as un compte : Outils te dit quoi faire, et te prévient si une nouvelle fuite les touche.");
        ui.horizontal(|ui| {
            if ui.button("📧 Vérifier mon adresse e-mail").on_hover_text("Ouvre haveibeenpwned.com : tu y tapes ton adresse toi-même").clicked() {
                let _ = std::process::Command::new("explorer").arg("https://haveibeenpwned.com/").spawn();
            }
            if ui.button("⟳ Mettre la liste à jour").clicked() {
                self.charger_liste(true);
            }
        });
        if self.liste.lock().unwrap().is_none() {
            *self.liste.lock().unwrap() = Some(Ok(vec![]));
            self.charger_liste(false);
        }
        let l = self.liste.lock().unwrap().clone();
        let v = match l {
            Some(Ok(v)) if !v.is_empty() => v,
            Some(Err(e)) => {
                ui.label(format!("✖ {e}"));
                return;
            }
            _ => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(400));
                return;
            }
        };
        let mes: Vec<&Fuite> = v.iter().filter(|f| self.s.services.contains(&f.nom)).collect();
        if !mes.is_empty() {
            ui.strong(format!("Tes services touchés ({})", mes.len()));
            for f in &mes {
                ui.colored_label(Color32::from_rgb(255, 170, 80), format!("⚠ {} ({}) : {} comptes, {}", f.titre, f.date, f.comptes, f.donnees.iter().map(|d| traduire_donnee(d)).collect::<Vec<_>>().join(", ")));
            }
            ui.weak("À faire : change le mot de passe sur ces sites (et partout où tu utilisais le même), active la double authentification, et méfie-toi des e-mails qui citent ces services.");
            ui.separator();
        }
        ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("cherche un site (Canva, Twitch, Deezer…)").desired_width(300.0));
        let q = self.recherche.to_lowercase();
        let mut change = false;
        egui::ScrollArea::vertical().id_salt("fuites").show(ui, |ui| {
            let mut tri: Vec<&Fuite> = v.iter().filter(|f| q.is_empty() || f.titre.to_lowercase().contains(&q) || f.domaine.contains(&q)).collect();
            tri.sort_by(|a, b| b.date.cmp(&a.date));
            for f in tri.iter().take(if q.is_empty() { 40 } else { 200 }) {
                let mut suivi = self.s.services.contains(&f.nom);
                if ui.checkbox(&mut suivi, format!("{} · {} · {} comptes", f.titre, f.date, f.comptes)).on_hover_text(f.donnees.iter().map(|d| traduire_donnee(d)).collect::<Vec<_>>().join(", ")).changed() {
                    if suivi {
                        self.s.services.push(f.nom.clone());
                        self.s.connues.push(f.nom.clone());
                    } else {
                        self.s.services.retain(|s| *s != f.nom);
                    }
                    change = true;
                }
            }
        });
        if change {
            enregistrer("fuites_suivis", &self.s);
        }
    }
}

// ===================================================================================================
// 8 · Audit du coffre
// ===================================================================================================

/// Sites qui proposent la double authentification (à activer !).
pub const DOUBLE_AUTH: &[&str] = &[
    "google.com", "gmail.com", "microsoft.com", "live.com", "outlook.com", "apple.com", "icloud.com", "discord.com", "steampowered.com", "steamcommunity.com", "epicgames.com",
    "paypal.com", "amazon.fr", "amazon.com", "facebook.com", "instagram.com", "twitter.com", "x.com", "github.com", "twitch.tv", "ubisoft.com", "ea.com", "battle.net",
    "riotgames.com", "snapchat.com", "tiktok.com", "reddit.com", "dropbox.com", "proton.me", "linkedin.com", "roblox.com", "nintendo.com", "playstation.com", "xbox.com",
];

#[derive(Clone)]
struct Probleme {
    site: String,
    identifiant: String,
    soucis: Vec<(u8, String)>,
}

fn auditer(entrees: Vec<crate::coffre::Entree>, etat: &Mutex<String>) -> Vec<Probleme> {
    let fuites = liste_fuites(false).unwrap_or_default();
    let mut compte: HashMap<String, usize> = HashMap::new();
    for e in &entrees {
        if !e.mdp.is_empty() {
            *compte.entry(e.mdp.clone()).or_default() += 1;
        }
    }
    let mut fuites_mdp_cache: HashMap<String, u64> = HashMap::new();
    let mut v = vec![];
    for (k, e) in entrees.iter().enumerate() {
        *etat.lock().unwrap() = format!("{} / {}…", k + 1, entrees.len());
        let mut s = vec![];
        if e.mdp.is_empty() {
            continue;
        }
        let a = analyser_mdp(&e.mdp);
        if a.note <= 1 {
            s.push((2, format!("mot de passe {} ({})", NOTES[a.note as usize].0, temps_cassage(a.bits).1)));
        }
        let n = compte.get(&e.mdp).copied().unwrap_or(0);
        if n > 1 {
            s.push((2, format!("même mot de passe que {} autre(s) site(s) : si l'un fuit, tous tombent", n - 1)));
        }
        let f = *fuites_mdp_cache.entry(e.mdp.clone()).or_insert_with(|| {
            std::thread::sleep(Duration::from_millis(120));
            fuites_mdp(&e.mdp).unwrap_or(0)
        });
        if f > 0 {
            s.push((3, format!("mot de passe vu {f} fois dans des fuites : à changer tout de suite")));
        }
        let d = domaine(&e.site);
        for x in fuites.iter().filter(|x| !x.domaine.is_empty() && x.domaine == d) {
            s.push((2, format!("le site a été piraté ({}) : {} — change ce mot de passe si tu ne l'as pas fait depuis", x.date, x.donnees.iter().map(|d| traduire_donnee(d)).collect::<Vec<_>>().join(", "))));
        }
        if DOUBLE_AUTH.contains(&d.as_str()) {
            s.push((1, "ce site propose la double authentification : active-la (codes dans Quotidien › Codes 2FA)".into()));
        }
        if !s.is_empty() {
            v.push(Probleme { site: e.site.clone(), identifiant: e.identifiant.clone(), soucis: s });
        }
    }
    v.sort_by_key(|p| std::cmp::Reverse(p.soucis.iter().map(|s| s.0).max().unwrap_or(0)));
    v
}

pub struct AuditCoffre {
    mdp: String,
    r: Arc<Mutex<Option<Result<(usize, Vec<Probleme>), String>>>>,
    etat: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
}

impl AuditCoffre {
    pub fn new() -> Self {
        AuditCoffre { mdp: String::new(), r: Arc::default(), etat: Arc::default(), en_cours: Arc::default() }
    }
}

impl Outil for AuditCoffre {
    fn nom(&self) -> &'static str {
        "🧾 Audit du coffre"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Audit du coffre à mots de passe");
        ui.weak("Passe en revue tous les mots de passe du coffre (Quotidien › Coffre) : faibles, réutilisés, fuités, sites piratés, double authentification à activer. Les mots de passe ne sont jamais affichés ni envoyés (pour les fuites : 5 caractères de leur empreinte).");
        let en_cours = self.en_cours.load(Relaxed);
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.mdp).password(true).hint_text("mot de passe maître du coffre").desired_width(240.0));
            let go = ui.add_enabled(!en_cours && !self.mdp.is_empty(), egui::Button::new("🔎 Auditer")).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go && !en_cours {
                let m = std::mem::take(&mut self.mdp);
                self.en_cours.store(true, Relaxed);
                let (res, e, c) = (self.r.clone(), self.etat.clone(), self.en_cours.clone());
                std::thread::spawn(move || {
                    let chemin = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("CoffreMotsDePasse").join("coffre.json");
                    let x = match crate::coffre::Coffre::ouvrir(&chemin, &m) {
                        Ok(cf) => {
                            let entrees = cf.entrees.clone();
                            drop(cf);
                            let n = entrees.len();
                            Ok((n, auditer(entrees, &e)))
                        }
                        Err(err) => Err(err.to_string()),
                    };
                    *res.lock().unwrap() = Some(x);
                    c.store(false, Relaxed);
                });
            }
            if en_cours {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        });
        let r = self.r.lock().unwrap().clone();
        match r {
            None => {}
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, format!("✖ {e}"));
            }
            Some(Ok((n, v))) => {
                let graves = v.iter().filter(|p| p.soucis.iter().any(|s| s.0 >= 2)).count();
                let score = if n == 0 { 100 } else { 100 - graves * 100 / n };
                ui.label(RichText::new(format!("Note du coffre : {score} / 100 · {graves} compte(s) à corriger sur {n}")).size(17.0).strong());
                egui::ScrollArea::vertical().id_salt("audit_coffre").show(ui, |ui| {
                    for p in &v {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.strong(format!("{} {}", p.site, if p.identifiant.is_empty() { String::new() } else { format!("· {}", p.identifiant) }));
                            for (g, t) in &p.soucis {
                                ui.colored_label([Color32::GRAY, Color32::from_rgb(200, 200, 120), Color32::from_rgb(255, 170, 80), Color32::LIGHT_RED][*g as usize], format!("• {t}"));
                            }
                        });
                    }
                });
            }
        }
    }
}

// ===================================================================================================
// 9 · Codes de double authentification (TOTP)
// ===================================================================================================

pub fn base32(s: &str) -> Option<Vec<u8>> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u64;
    let mut n = 0;
    let mut v = vec![];
    for c in s.chars().filter(|c| !c.is_whitespace() && *c != '=' && *c != '-') {
        let x = A.iter().position(|a| *a as char == c.to_ascii_uppercase())? as u64;
        bits = (bits << 5) | x;
        n += 5;
        if n >= 8 {
            v.push((bits >> (n - 8)) as u8);
            n -= 8;
        }
    }
    (!v.is_empty()).then_some(v)
}

fn hmac<D: sha2::Digest>(cle: &[u8], msg: &[u8], bloc: usize) -> Vec<u8> {
    let mut k = if cle.len() > bloc { D::digest(cle).to_vec() } else { cle.to_vec() };
    k.resize(bloc, 0);
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut h = D::new();
    h.update(&ipad);
    h.update(msg);
    let i = h.finalize();
    let mut h = D::new();
    h.update(&opad);
    h.update(&i);
    h.finalize().to_vec()
}

/// Code TOTP (RFC 6238) pour un instant donné.
pub fn totp(secret: &[u8], t: u64, periode: u64, chiffres: u32, sha256: bool) -> String {
    let c = (t / periode.max(1)).to_be_bytes();
    let h = if sha256 { hmac::<sha2::Sha256>(secret, &c, 64) } else { hmac::<sha1::Sha1>(secret, &c, 64) };
    let o = (h[h.len() - 1] & 0x0f) as usize;
    let n = u32::from_be_bytes([h[o] & 0x7f, h[o + 1], h[o + 2], h[o + 3]]) % 10u32.pow(chiffres);
    format!("{n:0width$}", width = chiffres as usize)
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Compte2fa {
    pub nom: String,
    pub secret: String,
    pub chiffres: u32,
    pub periode: u64,
    pub sha256: bool,
}

/// « moi%40mail.fr » → « moi@mail.fr »
pub fn decoder_url(s: &str) -> String {
    let b = s.as_bytes();
    let mut v = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                v.push(x);
                i += 3;
                continue;
            }
        }
        v.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&v).to_string()
}

/// Lit une adresse « otpauth://totp/Site:moi?secret=…&issuer=… » (QR code des sites).
pub fn lire_otpauth(u: &str) -> Option<Compte2fa> {
    let reste = u.trim().strip_prefix("otpauth://totp/")?;
    let (libelle, q) = reste.split_once('?').unwrap_or((reste, ""));
    let mut c = Compte2fa { nom: decoder_url(libelle), chiffres: 6, periode: 30, ..Default::default() };
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k.to_lowercase().as_str() {
            "secret" => c.secret = v.to_uppercase(),
            "issuer" if !c.nom.contains(':') => c.nom = format!("{} : {}", decoder_url(v), c.nom),
            "digits" => c.chiffres = v.parse().unwrap_or(6),
            "period" => c.periode = v.parse().unwrap_or(30),
            "algorithm" => c.sha256 = v.eq_ignore_ascii_case("SHA256"),
            _ => {}
        }
    }
    base32(&c.secret).map(|_| c)
}

#[derive(Serialize, Deserialize, Default)]
struct Fichier2fa {
    sel: Vec<u8>,
    nonce: Vec<u8>,
    donnees: Vec<u8>,
}

fn cle_de(code: &str, sel: &[u8]) -> Option<[u8; 32]> {
    let mut k = [0u8; 32];
    let p = argon2::Params::new(32768, 3, 1, Some(32)).ok()?;
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p).hash_password_into(code.as_bytes(), sel, &mut k).ok()?;
    Some(k)
}

fn chiffrer2fa(k: &[u8; 32], comptes: &[Compte2fa], sel: &[u8]) -> Fichier2fa {
    let mut nonce = [0u8; 12];
    let _ = getrandom::getrandom(&mut nonce);
    let c = Aes256Gcm::new_from_slice(k).expect("clé");
    let donnees = c.encrypt(Nonce::from_slice(&nonce), serde_json::to_vec(comptes).unwrap_or_default().as_slice()).unwrap_or_default();
    Fichier2fa { sel: sel.to_vec(), nonce: nonce.to_vec(), donnees }
}

fn dechiffrer2fa(k: &[u8; 32], f: &Fichier2fa) -> Option<Vec<Compte2fa>> {
    let c = Aes256Gcm::new_from_slice(k).ok()?;
    let b = c.decrypt(Nonce::from_slice(&f.nonce), f.donnees.as_slice()).ok()?;
    serde_json::from_slice(&b).ok()
}

pub struct Codes2fa {
    cle: Option<[u8; 32]>,
    comptes: Vec<Compte2fa>,
    code: String,
    nouveau: Compte2fa,
    lien: String,
    message: String,
    dernier: Instant,
}

impl Codes2fa {
    pub fn new() -> Self {
        Codes2fa { cle: None, comptes: vec![], code: String::new(), nouveau: Compte2fa { chiffres: 6, periode: 30, ..Default::default() }, lien: String::new(), message: String::new(), dernier: Instant::now() }
    }

    fn sauver(&self) {
        let Some(k) = &self.cle else { return };
        let f: Fichier2fa = charger("codes_2fa");
        enregistrer("codes_2fa", &chiffrer2fa(k, &self.comptes, &f.sel));
    }
}

impl Outil for Codes2fa {
    fn nom(&self) -> &'static str {
        "🔢 Codes 2FA"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.cle.is_some() && self.dernier.elapsed() > Duration::from_secs(120) {
            self.cle = None;
            self.comptes.clear();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.dernier = Instant::now();
        ui.heading("Codes de double authentification");
        ui.weak("Les codes à 6 chiffres qui changent toutes les 30 secondes (comme Google Authenticator), sur le PC. Les secrets sont chiffrés avec ton code ; verrouillage après 2 minutes. Garde aussi les codes de secours donnés par chaque site.");
        let f: Fichier2fa = charger("codes_2fa");
        if self.cle.is_none() {
            let premiere = f.sel.is_empty();
            ui.label(if premiere { "Choisis un code pour protéger tes secrets (au moins 6 caractères) :" } else { "Code pour ouvrir :" });
            let r = ui.add(egui::TextEdit::singleline(&mut self.code).password(true).desired_width(220.0));
            if (ui.button(if premiere { "Créer" } else { "🔓 Ouvrir" }).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && self.code.chars().count() >= 6 {
                if premiere {
                    let mut sel = [0u8; 16];
                    let _ = getrandom::getrandom(&mut sel);
                    if let Some(k) = cle_de(&self.code, &sel) {
                        enregistrer("codes_2fa", &chiffrer2fa(&k, &[], &sel));
                        self.cle = Some(k);
                    }
                } else {
                    match cle_de(&self.code, &f.sel).and_then(|k| dechiffrer2fa(&k, &f).map(|c| (k, c))) {
                        Some((k, c)) => {
                            self.cle = Some(k);
                            self.comptes = c;
                            self.message.clear();
                        }
                        None => self.message = "✖ Code faux".into(),
                    }
                }
                self.code.clear();
            }
            ui.label(&self.message);
            return;
        }
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        let mut retirer = None;
        for (k, c) in self.comptes.iter().enumerate() {
            let Some(s) = base32(&c.secret) else { continue };
            let code = totp(&s, t, c.periode, c.chiffres, c.sha256);
            let reste = c.periode - t % c.periode.max(1);
            ui.horizontal(|ui| {
                ui.add_sized([200.0, 20.0], egui::Label::new(&c.nom));
                let joli = if code.len() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.clone() };
                if ui.add(egui::Button::new(RichText::new(joli).monospace().size(22.0).strong())).on_hover_text("Copier").clicked() {
                    ui.ctx().copy_text(code.clone());
                }
                ui.add(egui::ProgressBar::new(reste as f32 / c.periode as f32).desired_width(60.0).text(format!("{reste} s")));
                if ui.small_button("🗑").clicked() {
                    retirer = Some(k);
                }
            });
        }
        if let Some(k) = retirer {
            self.comptes.remove(k);
            self.sauver();
        }
        ui.separator();
        ui.strong("➕ Ajouter un compte");
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.lien).hint_text("lien otpauth://… (du QR code)").desired_width(300.0));
            if ui.button("Ajouter").clicked() {
                match lire_otpauth(&self.lien) {
                    Some(c) => {
                        self.comptes.push(c);
                        self.lien.clear();
                        self.sauver();
                        self.message = "✔ Ajouté".into();
                    }
                    None => self.message = "Lien illisible".into(),
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.nom).hint_text("nom (ex. Discord)").desired_width(140.0));
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.secret).hint_text("clé secrète (lettres et chiffres)").desired_width(220.0));
            if ui.button("Ajouter").clicked() {
                if base32(&self.nouveau.secret).is_some() && !self.nouveau.nom.is_empty() {
                    let mut c = std::mem::replace(&mut self.nouveau, Compte2fa { chiffres: 6, periode: 30, ..Default::default() });
                    c.secret = c.secret.replace(' ', "").to_uppercase();
                    self.comptes.push(c);
                    self.sauver();
                    self.message = "✔ Ajouté".into();
                } else {
                    self.message = "Clé secrète invalide".into();
                }
            }
        });
        ui.weak("Sur le site, choisis « appli d'authentification », puis « je ne peux pas scanner le code » pour voir la clé secrète. Tu peux aussi scanner le QR avec le téléphone et copier le lien.");
        ui.horizontal(|ui| {
            ui.label(&self.message);
            if ui.button("🔒 Verrouiller").clicked() {
                self.cle = None;
                self.comptes.clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidite_et_codes() {
        assert_eq!(analyser_mdp("azerty").note, 0);
        assert_eq!(analyser_mdp("P@ssw0rd").note, 0);
        let a = analyser_mdp("Soleil1990!");
        assert!(a.note <= 2, "{a:?}");
        assert!(a.motifs.iter().any(|m| m.contains("1990")));
        assert!(analyser_mdp("cheval-agrafe-nuage-violon").note >= 3);
        assert!(analyser_mdp("Xk9#mQ2$vL7&pR4!").note == 4);
        assert!(analyser_mdp("aaaaaaaaaaaa").note <= 1);
        assert_eq!(duree_lisible(30.0), "30 secondes");
        // RFC 6238 : secret ASCII « 12345678901234567890 », t = 59 → 94287082
        assert_eq!(totp(b"12345678901234567890", 59, 30, 8, false), "94287082");
        assert_eq!(totp(b"12345678901234567890", 1111111109, 30, 8, false), "07081804");
        assert_eq!(base32("GEZDGNBVGY3TQOJQ").unwrap(), b"1234567890");
        let c = lire_otpauth("otpauth://totp/Discord:moi%40mail.fr?secret=JBSWY3DPEHPK3PXP&issuer=Discord").unwrap();
        assert_eq!((c.secret.as_str(), c.chiffres, c.periode), ("JBSWY3DPEHPK3PXP", 6, 30));
        assert!(c.nom.contains("moi@mail.fr"));
        assert_eq!(domaine("https://www.store.steampowered.com/login"), "steampowered.com");
        assert_eq!(domaine("impots.gouv.fr"), "impots.gouv.fr");
        assert_eq!(domaine("bbc.co.uk"), "bbc.co.uk");
        assert_eq!(traduire_donnee("Passwords"), "mots de passe");
        let k = [7u8; 32];
        let f = chiffrer2fa(&k, &[c.clone()], b"sel");
        assert_eq!(dechiffrer2fa(&k, &f).unwrap()[0].secret, c.secret);
        assert!(dechiffrer2fa(&[8u8; 32], &f).is_none());
    }
}
