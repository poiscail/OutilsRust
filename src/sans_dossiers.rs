//! Fichiers sans dossiers (1) : on retrouve un fichier par CE QU'IL EST, pas par l'endroit où il
//! est rangé. « photos de la plage cet été », « pdf reçu sur discord », « musique téléchargée hier
//! soir »… Outils comprend chaque fichier tout seul (genre, date, provenance, contenu) sans rien
//! déplacer. Contient aussi les petits outils de fichiers partagés par l'agent et le donjon.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------------------------------
// Outils communs : dossiers perso, corbeille, dates
// ---------------------------------------------------------------------------------------------------

pub fn maison() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default())
}

/// Dossiers personnels connus : (nom affiché, mots qui le désignent, chemin).
pub fn dossiers_perso() -> Vec<(&'static str, &'static [&'static str], PathBuf)> {
    let m = maison();
    let un = |noms: &[&str]| noms.iter().map(|n| m.join(n)).find(|p| p.is_dir()).unwrap_or_else(|| m.join(noms[0]));
    vec![
        ("Téléchargements", &["telechargement", "téléchargement", "telechargements", "téléchargements", "download", "downloads"][..], un(&["Downloads", "Téléchargements"])),
        ("Bureau", &["bureau", "desktop"][..], un(&["Desktop", "Bureau"])),
        ("Documents", &["document", "documents"][..], un(&["Documents"])),
        ("Images", &["image", "images", "photo", "photos", "pictures"][..], un(&["Pictures", "Images"])),
        ("Musique", &["musique", "musiques", "music"][..], un(&["Music", "Musique"])),
        ("Vidéos", &["video", "vidéo", "videos", "vidéos"][..], un(&["Videos", "Vidéos"])),
    ]
}

/// Retrouve un dossier dit en français (« téléchargements », « mon bureau », ou un vrai chemin).
pub fn dossier_dit(s: &str) -> Option<PathBuf> {
    let t = s.trim().trim_matches(['"', '\'']);
    if t.is_empty() {
        return None;
    }
    let p = PathBuf::from(t);
    if p.is_absolute() && p.is_dir() {
        return Some(p);
    }
    let bas = t.to_lowercase();
    dossiers_perso().into_iter().find(|(_, mots, _)| mots.iter().any(|m| bas.split(|c: char| !c.is_alphanumeric() && c != 'é').any(|w| w == *m))).map(|x| x.2)
}

/// Envoie à la corbeille (annulable).
pub fn corbeille(p: &Path) -> bool {
    #[cfg(windows)]
    {
        crate::disque::corbeille(p)
    }
    #[cfg(not(windows))]
    {
        // corbeille freedesktop (~/.local/share/Trash)
        let t = maison().join(".local/share/Trash");
        let (f, i) = (t.join("files"), t.join("info"));
        if std::fs::create_dir_all(&f).is_err() || std::fs::create_dir_all(&i).is_err() {
            return false;
        }
        let nom = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mut cible = nom.clone();
        let mut k = 1;
        while f.join(&cible).exists() {
            k += 1;
            cible = format!("{k}_{nom}");
        }
        let (a, mo, j) = civil(maintenant() / 86400);
        let info = format!("[Trash Info]\nPath={}\nDeletionDate={a:04}-{mo:02}-{j:02}T00:00:00\n", p.display());
        std::fs::write(i.join(format!("{cible}.trashinfo")), info).is_ok() && std::fs::rename(p, f.join(&cible)).is_ok()
    }
}

pub fn maintenant() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Jours depuis 1970 → (année, mois, jour).
pub fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, j)
}

/// (année, mois, jour) → jours depuis 1970.
pub fn jours(a: i64, m: i64, j: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let era = a.div_euclid(400);
    let yoe = a - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + j - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

pub fn date_courte(t: i64) -> String {
    let (a, m, j) = civil(t.div_euclid(86400));
    const COURTS: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
    format!("{j} {} {a}", COURTS[(m - 1) as usize])
}

pub fn taille_lisible(o: u64) -> String {
    match o {
        0..=1023 => format!("{o} o"),
        1024..=1_048_575 => format!("{:.0} Ko", o as f64 / 1024.0),
        1_048_576..=1_073_741_823 => format!("{:.1} Mo", o as f64 / 1_048_576.0),
        _ => format!("{:.2} Go", o as f64 / 1_073_741_824.0),
    }
}

// ---------------------------------------------------------------------------------------------------
// Ce qu'est un fichier
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Genre {
    Photo,
    Video,
    Musique,
    Document,
    Pdf,
    Tableur,
    Presentation,
    Archive,
    Programme,
    Code,
    Jeu,
    Police,
    #[default]
    Autre,
}

impl Genre {
    pub fn de(p: &Path) -> Genre {
        let e = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        use Genre::*;
        match e.as_str() {
            "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "gif" | "bmp" | "tif" | "tiff" | "cr2" | "nef" | "arw" | "dng" | "avif" => Photo,
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "3gp" | "mts" => Video,
            "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "opus" | "wma" => Musique,
            "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "pages" => Document,
            "pdf" => Pdf,
            "xls" | "xlsx" | "ods" | "csv" => Tableur,
            "ppt" | "pptx" | "odp" | "key" => Presentation,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" => Archive,
            "exe" | "msi" | "msix" | "appx" | "deb" | "rpm" | "appimage" | "dmg" | "bat" | "cmd" | "ps1" | "sh" | "apk" => Programme,
            "rs" | "py" | "js" | "ts" | "html" | "css" | "c" | "cpp" | "h" | "java" | "cs" | "go" | "json" | "xml" | "yml" | "yaml" | "toml" | "lua" | "php" => Code,
            "jar" | "mcworld" | "mcpack" | "mcaddon" | "schem" | "litematic" | "sav" | "nbt" => Jeu,
            "ttf" | "otf" | "woff" | "woff2" => Police,
            _ => Autre,
        }
    }
    pub fn icone(self) -> &'static str {
        use Genre::*;
        match self {
            Photo => "📷",
            Video => "🎞",
            Musique => "🎵",
            Document => "📄",
            Pdf => "📕",
            Tableur => "📊",
            Presentation => "📽",
            Archive => "📦",
            Programme => "⚙",
            Code => "💻",
            Jeu => "🎮",
            Police => "🔤",
            Autre => "📎",
        }
    }
    /// Mots français qui désignent ce genre.
    fn mots(self) -> &'static [&'static str] {
        use Genre::*;
        match self {
            Photo => &["photo", "photos", "image", "images", "selfie", "selfies", "capture", "captures", "screenshot", "dessin", "wallpaper", "fond"],
            Video => &["video", "vidéo", "videos", "vidéos", "film", "films", "clip", "clips"],
            Musique => &["musique", "musiques", "chanson", "chansons", "son", "sons", "audio", "mp3", "morceau", "morceaux", "titre", "titres"],
            Document => &["document", "documents", "doc", "docs", "word", "texte", "textes", "lettre", "lettres", "devoir", "devoirs"],
            Pdf => &["pdf", "pdfs"],
            Tableur => &["tableau", "tableaux", "excel", "tableur", "csv"],
            Presentation => &["présentation", "presentation", "présentations", "presentations", "powerpoint", "diaporama", "exposé", "expose"],
            Archive => &["archive", "archives", "zip", "rar", "compressé"],
            Programme => &["programme", "programmes", "installateur", "installateurs", "setup", "logiciel", "logiciels", "appli", "application", "exe"],
            Code => &["code", "script", "scripts"],
            Jeu => &["sauvegarde", "sauvegardes", "mod", "mods", "schematic", "monde", "mondes"],
            Police => &["police", "polices", "font", "fonts"],
            Autre => &[],
        }
    }
    pub const TOUS: [Genre; 13] = [Genre::Photo, Genre::Video, Genre::Musique, Genre::Document, Genre::Pdf, Genre::Tableur, Genre::Presentation, Genre::Archive, Genre::Programme, Genre::Code, Genre::Jeu, Genre::Police, Genre::Autre];
}

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Fiche {
    pub chemin: PathBuf,
    pub taille: u64,
    /// date de modification (secondes)
    pub modif: i64,
    /// date « vraie » : prise de vue d'une photo, sinon modification
    pub date: i64,
    pub genre: Genre,
    /// site d'où le fichier a été téléchargé (Windows le note)
    pub source: String,
    /// mots trouvés dans le fichier (texte, appareil photo, description de la photo par l'IA…)
    pub mots: String,
}

/// Provenance notée par Windows dans le flux caché « Zone.Identifier ».
fn source(p: &Path) -> String {
    #[cfg(windows)]
    {
        let mut s = p.as_os_str().to_owned();
        s.push(":Zone.Identifier");
        if let Ok(t) = std::fs::read_to_string(PathBuf::from(s)) {
            for cle in ["ReferrerUrl=", "HostUrl="] {
                if let Some(l) = t.lines().find(|l| l.starts_with(cle)) {
                    let u = &l[cle.len()..];
                    let hote = u.split("://").nth(1).unwrap_or(u).split(['/', '?', ':']).next().unwrap_or("");
                    if !hote.is_empty() && hote != "about" {
                        return hote.trim_start_matches("www.").to_lowercase();
                    }
                }
            }
        }
        String::new()
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        String::new()
    }
}

fn mots_de(texte: &str, max: usize) -> String {
    let mut v: Vec<String> = Vec::new();
    for w in texte.split(|c: char| !c.is_alphanumeric()) {
        if w.chars().count() >= 3 && w.chars().count() <= 24 && !w.chars().all(|c| c.is_ascii_digit()) {
            let w = w.to_lowercase();
            if !v.contains(&w) {
                v.push(w);
                if v.len() >= max {
                    break;
                }
            }
        }
    }
    v.join(" ")
}

/// Texte au début d'un fichier (texte brut, Word, PDF).
fn texte(p: &Path, genre: Genre, taille: u64) -> String {
    let e = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match e.as_str() {
        "txt" | "md" | "csv" | "rtf" | "html" | "json" | "xml" => {
            let mut b = vec![0u8; 6000];
            let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
            String::from_utf8_lossy(&b[..n]).into_owned()
        }
        "docx" | "odt" | "pptx" | "xlsx" if taille < 30_000_000 => {
            let interne = match e.as_str() {
                "docx" => "word/document.xml",
                "odt" => "content.xml",
                "pptx" => "ppt/slides/slide1.xml",
                _ => "xl/sharedStrings.xml",
            };
            let Ok(f) = std::fs::File::open(p) else { return String::new() };
            let Ok(mut z) = zip::ZipArchive::new(f) else { return String::new() };
            let Ok(mut x) = z.by_name(interne) else { return String::new() };
            let mut s = String::new();
            let _ = (&mut x).take(200_000).read_to_string(&mut s);
            // enlève les balises XML
            let mut o = String::with_capacity(s.len() / 3);
            let mut dans = false;
            for c in s.chars() {
                match c {
                    '<' => dans = true,
                    '>' => {
                        dans = false;
                        o.push(' ');
                    }
                    _ if !dans => o.push(c),
                    _ => {}
                }
                if o.len() > 8000 {
                    break;
                }
            }
            o
        }
        "pdf" if taille < 15_000_000 && genre == Genre::Pdf => std::panic::catch_unwind(|| {
            let d = lopdf::Document::load(p).ok()?;
            let pages: Vec<u32> = d.get_pages().keys().take(2).copied().collect();
            d.extract_text(&pages).ok()
        })
        .ok()
        .flatten()
        .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Date de prise de vue et appareil d'une photo.
fn exif(p: &Path) -> Option<(Option<i64>, String)> {
    let f = std::fs::File::open(p).ok()?;
    let e = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)).ok()?;
    let mut date = None;
    if let Some(c) = e.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        if let exif::Value::Ascii(v) = &c.value {
            if let Some(Ok(d)) = v.first().map(|x| exif::DateTime::from_ascii(x)) {
                date = Some(jours(d.year as i64, d.month as i64, d.day as i64) * 86400 + d.hour as i64 * 3600 + d.minute as i64 * 60);
            }
        }
    }
    let mut mots = String::new();
    for t in [exif::Tag::Make, exif::Tag::Model] {
        if let Some(c) = e.get_field(t, exif::In::PRIMARY) {
            mots += &c.display_value().to_string().replace('"', "");
            mots.push(' ');
        }
    }
    if e.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY).is_some() {
        mots += "gps localisée ";
    }
    Some((date, mots))
}

/// Analyse d'un fichier. `photos_ia` : descriptions faites par « Photos par leur contenu ».
pub fn analyser(p: &Path, taille: u64, modif: i64, photos_ia: &std::collections::HashMap<PathBuf, String>) -> Fiche {
    let genre = Genre::de(p);
    let mut date = modif;
    let mut mots = String::new();
    if genre == Genre::Photo {
        if let Some((d, m)) = exif(p) {
            date = d.unwrap_or(modif);
            mots += &m;
        }
        if let Some(d) = photos_ia.get(p) {
            mots += &mots_de(d, 40);
        }
    } else {
        mots += &mots_de(&texte(p, genre, taille), 80);
    }
    Fiche { chemin: p.to_path_buf(), taille, modif, date, genre, source: source(p), mots: mots.trim().to_string() }
}

const IGNORES: [&str; 12] = ["node_modules", ".git", "appdata", "$recycle.bin", "target", "__pycache__", ".cache", "cache", ".venv", "venv", "system volume information", ".trash"];

/// Tous les fichiers sous `racines` : (chemin, taille, modification).
pub fn parcourir(racines: &[PathBuf], stop: &AtomicBool, max: usize) -> Vec<(PathBuf, u64, i64)> {
    let mut v = Vec::new();
    let mut pile: Vec<PathBuf> = racines.to_vec();
    let mut vus = std::collections::HashSet::new();
    while let Some(d) = pile.pop() {
        if stop.load(Relaxed) || v.len() >= max || !vus.insert(d.clone()) {
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let nom = e.file_name().to_string_lossy().to_lowercase();
            let Ok(t) = e.file_type() else { continue };
            if t.is_symlink() {
                continue;
            }
            if t.is_dir() {
                if !nom.starts_with('.') && !IGNORES.contains(&nom.as_str()) {
                    pile.push(e.path());
                }
            } else if !nom.starts_with("~$") && nom != "desktop.ini" && nom != "thumbs.db" {
                let m = e.metadata().ok();
                let taille = m.as_ref().map(|m| m.len()).unwrap_or(0);
                let modif = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                v.push((e.path(), taille, modif));
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------
// Comprendre la demande
// ---------------------------------------------------------------------------------------------------

#[derive(Default, Debug, PartialEq, Clone)]
pub struct Requete {
    pub genres: Vec<Genre>,
    /// intervalle de dates [début, fin) en secondes
    pub periode: Option<(i64, i64)>,
    /// heures de la journée [début, fin)
    pub heures: Option<(i64, i64)>,
    pub taille_min: u64,
    pub taille_max: Option<u64>,
    pub internet: bool,
    pub dossier: Option<PathBuf>,
    /// dossier des téléchargements (calculé une fois, pas pour chaque fichier)
    pub telechargements: PathBuf,
    /// mots à trouver (chacun avec ses variantes)
    pub mots: Vec<Vec<String>>,
    /// ce qu'Outils a compris, à afficher
    pub compris: Vec<String>,
}

const VIDES: [&str; 54] = [
    "le", "la", "les", "un", "une", "des", "de", "du", "d", "l", "au", "aux", "et", "ou", "en", "sur", "sous", "avec", "pour", "dans", "mes", "mon", "ma", "tes", "ton", "ta", "ses", "son", "sa", "que", "qui", "quoi", "je", "j", "ai", "moi", "tous", "toutes", "tout", "fichier", "fichiers", "trouve", "trouver", "montre", "cherche", "chercher", "il", "y", "a", "par", "depuis", "via", "est", "était",
];

const SOURCES: [(&str, &[&str]); 14] = [
    ("discord", &["discord", "discordapp"]),
    ("whatsapp", &["whatsapp"]),
    ("mail", &["mail.google", "outlook", "live.com", "mail.yahoo", "orange.fr", "laposte", "free.fr", "proton"]),
    ("gmail", &["mail.google"]),
    ("email", &["mail.google", "outlook", "live.com", "mail.yahoo", "orange.fr", "laposte", "proton"]),
    ("telegram", &["telegram", "t.me"]),
    ("facebook", &["facebook", "fbcdn", "messenger"]),
    ("messenger", &["messenger", "facebook"]),
    ("instagram", &["instagram", "cdninstagram"]),
    ("drive", &["drive.google", "googleusercontent"]),
    ("onedrive", &["onedrive", "sharepoint", "1drv"]),
    ("youtube", &["youtube", "googlevideo"]),
    ("steam", &["steam"]),
    ("github", &["github"]),
];

fn variantes(w: &str) -> Vec<String> {
    let mut v = vec![w.to_string()];
    let s = w.trim_end_matches('s').trim_end_matches('x');
    if s.len() >= 3 && s != w {
        v.push(s.to_string());
    }
    // traductions (les descriptions de photos de l'IA sont en anglais)
    for x in crate::ia::mots_recherche(w) {
        if !v.contains(&x) {
            v.push(x);
        }
    }
    v
}

/// Comprend une demande en français. `maintenant` en secondes.
pub fn comprendre(q: &str, maintenant: i64) -> Requete {
    let mut r = Requete { telechargements: dossiers_perso()[0].2.clone(), ..Default::default() };
    let bas = q.to_lowercase().replace(['’', '\''], " ");
    let w: Vec<&str> = bas.split(|c: char| !(c.is_alphanumeric() || c == '-')).filter(|s| !s.is_empty()).collect();
    let jour0 = maintenant.div_euclid(86400);
    let (an, mois, _) = civil(jour0);
    let debut_jour = |d: i64| d * 86400;
    let mut pris = vec![false; w.len()];
    let periode = |r: &mut Requete, a: i64, b: i64, dit: String| {
        r.periode = Some((a, b));
        r.compris.push(format!("📅 {dit}"));
    };
    let mois_de = |m: &str| MOIS.iter().position(|x| *x == m || x.replace(['é', 'û'], "e") == m.replace(['é', 'û'], "e")).map(|k| k as i64 + 1);
    let debut_mois = |a: i64, m: i64| debut_jour(jours(a, m, 1));
    let mut i = 0;
    while i < w.len() {
        let x = w[i];
        let suivant = w.get(i + 1).copied().unwrap_or("");
        let prend = |n: usize, pris: &mut Vec<bool>| {
            for k in i..(i + n).min(pris.len()) {
                pris[k] = true;
            }
        };
        match x {
            "aujourd" | "aujourd-hui" => {
                periode(&mut r, debut_jour(jour0), maintenant + 1, "aujourd'hui".into());
                prend(if suivant == "hui" { 2 } else { 1 }, &mut pris);
            }
            "hier" => {
                periode(&mut r, debut_jour(jour0 - 1), debut_jour(jour0), "hier".into());
                prend(1, &mut pris);
            }
            "avant-hier" => {
                periode(&mut r, debut_jour(jour0 - 2), debut_jour(jour0 - 1), "avant-hier".into());
                prend(1, &mut pris);
            }
            "soir" | "matin" | "nuit" | "midi" | "après-midi" | "apres-midi" => {
                let h = match x {
                    "soir" => (18, 24),
                    "matin" => (5, 12),
                    "nuit" => (0, 5),
                    "midi" => (11, 14),
                    _ => (12, 18),
                };
                r.heures = Some(h);
                r.compris.push(format!("🕘 le {x}"));
                prend(1, &mut pris);
            }
            "semaine" | "mois" | "année" | "annee" | "an" => {
                let derniere = matches!(suivant, "dernière" | "derniere" | "dernier" | "passée" | "passé" | "passee" | "passe");
                let cette = i > 0 && matches!(w[i - 1], "cette" | "ce" | "cet");
                if derniere || cette {
                    let (a, b, dit) = match (x, derniere) {
                        ("semaine", false) => (debut_jour(jour0 - (jour0 + 3).rem_euclid(7)), maintenant + 1, "cette semaine"),
                        ("semaine", true) => {
                            let lundi = jour0 - (jour0 + 3).rem_euclid(7);
                            (debut_jour(lundi - 7), debut_jour(lundi), "la semaine dernière")
                        }
                        ("mois", false) => (debut_mois(an, mois), maintenant + 1, "ce mois-ci"),
                        ("mois", true) => {
                            let (a2, m2) = if mois == 1 { (an - 1, 12) } else { (an, mois - 1) };
                            (debut_mois(a2, m2), debut_mois(an, mois), "le mois dernier")
                        }
                        (_, false) => (debut_mois(an, 1), maintenant + 1, "cette année"),
                        (_, true) => (debut_mois(an - 1, 1), debut_mois(an, 1), "l'année dernière"),
                    };
                    periode(&mut r, a, b, dit.into());
                    if cette {
                        pris[i - 1] = true;
                    }
                    prend(if derniere { 2 } else { 1 }, &mut pris);
                }
            }
            "été" | "ete" | "hiver" | "printemps" | "automne" => {
                let m0 = match x {
                    "hiver" => 12,
                    "printemps" => 3,
                    "automne" => 9,
                    _ => 6,
                };
                let dernier = matches!(suivant, "dernier" | "passé" | "passe");
                // la saison la plus récente déjà commencée
                let mut a = if m0 == 12 { if mois == 12 { an } else { an - 1 } } else if mois >= m0 { an } else { an - 1 };
                if dernier {
                    a -= 1;
                }
                let (af, mf) = if m0 + 3 > 12 { (a + 1, m0 + 3 - 12) } else { (a, m0 + 3) };
                periode(&mut r, debut_mois(a, m0), debut_mois(af, mf), format!("{} {a}", if x == "ete" { "été" } else { x }));
                if i > 0 && matches!(w[i - 1], "cet" | "cette" | "ce") {
                    pris[i - 1] = true;
                }
                prend(if dernier { 2 } else { 1 }, &mut pris);
            }
            "récent" | "récents" | "recent" | "recents" | "récemment" | "recemment" | "nouveau" | "nouveaux" => {
                periode(&mut r, maintenant - 30 * 86400, maintenant + 1, "ces 30 derniers jours".into());
                prend(1, &mut pris);
            }
            "vieux" | "ancien" | "anciens" | "vieille" | "vieilles" | "oublié" | "oubliés" => {
                periode(&mut r, 0, maintenant - 365 * 86400, "il y a plus d'un an".into());
                prend(1, &mut pris);
            }
            "gros" | "grosse" | "grosses" | "énorme" | "énormes" | "lourd" | "lourds" | "lourde" | "lourdes" => {
                r.taille_min = 100 * 1024 * 1024;
                r.compris.push("🐘 plus de 100 Mo".into());
                prend(1, &mut pris);
            }
            "petit" | "petits" | "petite" | "petites" | "léger" | "légers" => {
                r.taille_max = Some(1024 * 1024);
                r.compris.push("🪶 moins de 1 Mo".into());
                prend(1, &mut pris);
            }
            "téléchargé" | "téléchargés" | "téléchargée" | "téléchargées" | "telecharge" | "telecharges" | "reçu" | "reçus" | "reçue" | "reçues" | "recu" | "recus" | "internet" => {
                r.internet = true;
                r.compris.push("🌐 venu d'internet".into());
                prend(1, &mut pris);
            }
            "il" if suivant == "y" && w.get(i + 2) == Some(&"a") => {
                // « il y a 3 jours / semaines / mois »
                if let (Some(n), Some(u)) = (w.get(i + 3).and_then(|s| s.parse::<i64>().ok()), w.get(i + 4)) {
                    let j = match u.trim_end_matches('s') {
                        "jour" => n,
                        "semaine" => n * 7,
                        "moi" | "mois" => n * 30,
                        "an" | "année" | "annee" => n * 365,
                        _ => 0,
                    };
                    if j > 0 {
                        let centre = jour0 - j;
                        let marge = (j / 4).max(1);
                        periode(&mut r, debut_jour(centre - marge), debut_jour(centre + marge + 1), format!("il y a {n} {u}"));
                        prend(5, &mut pris);
                    }
                }
            }
            _ => {
                if let Some(m) = mois_de(x) {
                    let annee = suivant.parse::<i64>().ok().filter(|a| (1980..2200).contains(a));
                    let a = annee.unwrap_or(if m <= mois { an } else { an - 1 });
                    let (af, mf) = if m == 12 { (a + 1, 1) } else { (a, m + 1) };
                    periode(&mut r, debut_mois(a, m), debut_mois(af, mf), format!("{} {a}", MOIS[(m - 1) as usize]));
                    prend(if annee.is_some() { 2 } else { 1 }, &mut pris);
                } else if let Some(a) = x.parse::<i64>().ok().filter(|a| (1980..2200).contains(a)) {
                    periode(&mut r, debut_mois(a, 1), debut_mois(a + 1, 1), format!("en {a}"));
                    prend(1, &mut pris);
                } else if let Some(g) = Genre::TOUS.iter().find(|g| g.mots().contains(&x)) {
                    // « dans mes images » désigne un dossier, pas forcément un genre
                    let dossier = i > 0 && matches!(w[i - 1], "dans" | "mes" | "mon") && dossier_dit(x).is_some() && matches!(x, "images" | "documents" | "musique" | "vidéos" | "videos");
                    if dossier {
                        r.dossier = dossier_dit(x);
                        r.compris.push(format!("📁 dans {x}"));
                    } else if !r.genres.contains(g) {
                        r.genres.push(*g);
                        r.compris.push(format!("{} {x}", g.icone()));
                    }
                    prend(1, &mut pris);
                } else if dossier_dit(x).is_some() && matches!(x, "téléchargements" | "telechargements" | "bureau" | "desktop" | "downloads") {
                    r.dossier = dossier_dit(x);
                    r.compris.push(format!("📁 {x}"));
                    prend(1, &mut pris);
                }
            }
        }
        i += 1;
    }
    for (k, x) in w.iter().enumerate() {
        if pris[k] || VIDES.contains(x) || x.chars().count() < 2 {
            continue;
        }
        let v = match SOURCES.iter().find(|(n, _)| n == x) {
            Some((n, hotes)) => {
                r.internet = true;
                r.compris.push(format!("🌐 {n}"));
                let mut v: Vec<String> = hotes.iter().map(|h| h.to_string()).collect();
                v.push(n.to_string());
                v
            }
            None => variantes(x),
        };
        r.mots.push(v);
    }
    if !r.mots.is_empty() {
        let liste: Vec<&str> = r.mots.iter().map(|v| v[0].as_str()).collect();
        r.compris.push(format!("🔎 {}", liste.join(", ")));
    }
    r
}

/// Score d'un fichier pour une demande (None = ne correspond pas).
pub fn note(f: &Fiche, r: &Requete) -> Option<usize> {
    if !r.genres.is_empty() && !r.genres.contains(&f.genre) {
        return None;
    }
    if let Some((a, b)) = r.periode {
        if f.date < a || f.date >= b {
            return None;
        }
    }
    if let Some((h0, h1)) = r.heures {
        let h = f.date.rem_euclid(86400) / 3600;
        if h < h0 || h >= h1 {
            return None;
        }
    }
    if f.taille < r.taille_min || r.taille_max.is_some_and(|m| f.taille > m) {
        return None;
    }
    if r.internet && f.source.is_empty() && f.chemin.parent() != Some(r.telechargements.as_path()) {
        return None;
    }
    if let Some(d) = &r.dossier {
        if !f.chemin.starts_with(d) {
            return None;
        }
    }
    let nom = f.chemin.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let parents = f.chemin.parent().map(|p| p.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut s = 1;
    for v in &r.mots {
        let mut trouve = 0;
        for m in v {
            if nom.contains(m.as_str()) {
                trouve = trouve.max(5);
            } else if f.source.contains(m.as_str()) {
                trouve = trouve.max(4);
            } else if f.mots.split(' ').any(|x| x == m || (m.len() >= 4 && x.starts_with(m.as_str()))) {
                trouve = trouve.max(3);
            } else if parents.contains(m.as_str()) {
                trouve = trouve.max(2);
            }
        }
        if trouve == 0 {
            return None;
        }
        s += trouve;
    }
    Some(s)
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Index {
    pub racines: Vec<PathBuf>,
    pub fiches: Vec<Fiche>,
    pub date: i64,
}

/// Index partagé (chargé une fois) : la recherche, l'agent et le donjon s'en servent.
pub fn index() -> Arc<Mutex<Index>> {
    static I: Mutex<Option<Arc<Mutex<Index>>>> = Mutex::new(None);
    I.lock().unwrap().get_or_insert_with(|| {
        let mut i: Index = charger("index_fichiers");
        if i.racines.is_empty() {
            i.racines = dossiers_perso().into_iter().map(|d| d.2).filter(|p| p.is_dir()).collect();
        }
        Arc::new(Mutex::new(i))
    }).clone()
}

/// Met l'index à jour (seuls les fichiers nouveaux ou modifiés sont relus).
pub fn mettre_a_jour(ix: &Arc<Mutex<Index>>, etat: &Arc<Mutex<String>>, stop: &AtomicBool) {
    let racines = ix.lock().unwrap().racines.clone();
    *etat.lock().unwrap() = "Je fais le tour de tes fichiers…".into();
    let tous = parcourir(&racines, stop, 300_000);
    let anciens: std::collections::HashMap<PathBuf, Fiche> = ix.lock().unwrap().fiches.iter().map(|f| (f.chemin.clone(), f.clone())).collect();
    let ia: serde_json::Value = charger("index_photos_ia");
    let photos_ia: std::collections::HashMap<PathBuf, String> = ia["photos"].as_array().map(|v| v.iter().filter_map(|x| Some((PathBuf::from(x[0].as_str()?), x[2].as_str()?.to_string()))).collect()).unwrap_or_default();
    let n = tous.len();
    let mut fiches = Vec::with_capacity(n);
    for (k, (p, t, m)) in tous.into_iter().enumerate() {
        if stop.load(Relaxed) {
            return;
        }
        match anciens.get(&p) {
            Some(f) if f.taille == t && f.modif == m => fiches.push(f.clone()),
            _ => fiches.push(analyser(&p, t, m, &photos_ia)),
        }
        if k % 500 == 0 {
            *etat.lock().unwrap() = format!("Je découvre tes fichiers : {k} / {n}");
        }
    }
    let mut i = ix.lock().unwrap();
    i.fiches = fiches;
    i.date = maintenant();
    enregistrer("index_fichiers", &*i);
    *etat.lock().unwrap() = format!("✔ {} fichiers connus", i.fiches.len());
}

pub struct SansDossiers {
    index: Arc<Mutex<Index>>,
    q: String,
    resultats: Vec<(usize, Fiche)>,
    compris: Vec<String>,
    derniere: Option<String>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    vignettes: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
}

impl SansDossiers {
    pub fn new() -> Self {
        let s = SansDossiers { index: index(), q: String::new(), resultats: vec![], compris: vec![], derniere: None, etat: Arc::default(), occupe: Arc::default(), stop: Arc::default(), vignettes: Default::default() };
        if maintenant() - s.index.lock().unwrap().date > 3600 {
            s.actualiser();
        }
        s
    }
    fn actualiser(&self) {
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (ix, et, oc, st) = (self.index.clone(), self.etat.clone(), self.occupe.clone(), self.stop.clone());
        std::thread::spawn(move || {
            mettre_a_jour(&ix, &et, &st);
            oc.store(false, Relaxed);
        });
    }
    fn chercher(&mut self) {
        let r = comprendre(&self.q, maintenant());
        self.compris = r.compris.clone();
        let i = self.index.lock().unwrap();
        let mut v: Vec<(usize, Fiche)> = i.fiches.iter().filter_map(|f| note(f, &r).map(|s| (s, f.clone()))).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.date.cmp(&a.1.date)));
        v.truncate(300);
        self.resultats = v;
        self.derniere = Some(self.q.clone());
    }
}

/// Raccourcis tout prêts, qui apprennent comment parler à l'outil.
fn collections(i: &Index) -> Vec<(String, String)> {
    let mut v = vec![
        ("🕘 Ces derniers jours".to_string(), "récents".to_string()),
        ("🌐 Arrivés d'internet cette semaine".into(), "téléchargés cette semaine".into()),
        ("📷 Photos de cet été".into(), "photos de cet été".into()),
        ("📕 PDF de ce mois-ci".into(), "pdf ce mois-ci".into()),
        ("🐘 Gros fichiers oubliés".into(), "gros vieux".into()),
        ("⚙ Installateurs".into(), "installateurs".into()),
    ];
    let mut sources: std::collections::HashMap<String, usize> = Default::default();
    for f in &i.fiches {
        if !f.source.is_empty() {
            let s = f.source.split('.').rev().nth(1).unwrap_or(&f.source).to_string();
            *sources.entry(s).or_default() += 1;
        }
    }
    let mut s: Vec<_> = sources.into_iter().collect();
    s.sort_by(|a, b| b.1.cmp(&a.1));
    for (nom, n) in s.into_iter().take(5) {
        v.push((format!("🌐 Venus de {nom} ({n})"), format!("reçus {nom}")));
    }
    let mut annees: Vec<i64> = i.fiches.iter().filter(|f| f.genre == Genre::Photo).map(|f| civil(f.date.div_euclid(86400)).0).collect();
    annees.sort();
    annees.dedup();
    for a in annees.into_iter().rev().take(4) {
        v.push((format!("📷 Photos {a}"), format!("photos {a}")));
    }
    v
}

impl Outil for SansDossiers {
    fn nom(&self) -> &'static str {
        "🗂 Fichiers sans dossiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fichiers sans dossiers");
        ui.weak("Plus besoin de te souvenir OÙ tu as rangé un fichier : dis ce que c'est. Outils comprend tout seul chaque fichier (genre, date, d'où il vient, ce qu'il contient) sans rien déplacer.");
        ui.add_space(4.0);
        let rep = ui.add(egui::TextEdit::singleline(&mut self.q).font(egui::TextStyle::Heading).desired_width(f32::INFINITY).hint_text("ex. : photos de la plage cet été · pdf reçu sur discord · musique téléchargée hier soir"));
        if rep.changed() || (self.derniere.is_none() && !self.q.is_empty()) {
            self.chercher();
        }
        ui.horizontal_wrapped(|ui| {
            if self.q.trim().is_empty() {
                ui.weak("Essaie :");
            } else {
                ui.weak("J'ai compris :");
                for c in &self.compris {
                    ui.label(RichText::new(c).background_color(ui.visuals().faint_bg_color));
                }
            }
        });
        if self.q.trim().is_empty() {
            let cols = collections(&self.index.lock().unwrap());
            ui.horizontal_wrapped(|ui| {
                for (t, q) in cols {
                    if ui.button(t).clicked() {
                        self.q = q;
                        self.chercher();
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            let n = self.index.lock().unwrap().fiches.len();
            let e = self.etat.lock().unwrap().clone();
            ui.weak(if e.is_empty() { format!("{n} fichiers connus") } else { e });
            if self.occupe.load(Relaxed) {
                ui.spinner();
            } else if ui.small_button("🔄 Actualiser").clicked() {
                self.actualiser();
            }
            egui::CollapsingHeader::new("📁 Où regarder").show(ui, |ui| {
                let mut i = self.index.lock().unwrap();
                let mut suppr = None;
                for (k, r) in i.racines.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(r.display().to_string());
                        if ui.small_button("✖").clicked() {
                            suppr = Some(k);
                        }
                    });
                }
                if let Some(k) = suppr {
                    i.racines.remove(k);
                }
                if ui.small_button("➕ Ajouter un dossier ou un disque").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        i.racines.push(d);
                    }
                }
            });
        });
        if self.occupe.load(Relaxed) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
        }
        ui.separator();
        if !self.q.trim().is_empty() && self.resultats.is_empty() {
            ui.label("Rien trouvé. Essaie avec moins de mots, ou une autre période.");
        }
        let resultats = self.resultats.clone();
        if !resultats.is_empty() {
            ui.weak(format!("{} fichier(s)", resultats.len()));
        }
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, 46.0, resultats.len(), |ui, plage| {
            for (_, f) in &resultats[plage] {
                ui.horizontal(|ui| {
                    ui.set_height(42.0);
                    // vignette pour les photos
                    let tex = if f.genre == Genre::Photo && f.taille < 40_000_000 {
                        self.vignettes.entry(f.chemin.clone()).or_insert_with(|| {
                            image::open(&f.chemin).ok().map(|i| {
                                let t = i.thumbnail(80, 80).to_rgba8();
                                ui.ctx().load_texture(f.chemin.to_string_lossy(), egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default())
                            })
                        }).clone()
                    } else {
                        None
                    };
                    match tex {
                        Some(t) => {
                            ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(40.0, 40.0)).corner_radius(4.0));
                        }
                        None => {
                            ui.add_sized([40.0, 40.0], egui::Label::new(RichText::new(f.genre.icone()).size(24.0)));
                        }
                    }
                    ui.vertical(|ui| {
                        let nom = f.chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::Label::new(RichText::new(nom).strong()).sense(egui::Sense::click())).on_hover_text("Ouvrir").clicked() {
                            crate::outil::ouvrir(&f.chemin);
                        }
                        ui.horizontal(|ui| {
                            ui.weak(date_courte(f.date));
                            ui.weak("·");
                            ui.weak(taille_lisible(f.taille));
                            if !f.source.is_empty() {
                                ui.weak("·");
                                ui.colored_label(Color32::from_rgb(110, 160, 230), format!("🌐 {}", f.source));
                            }
                            ui.weak("·");
                            let dossier = f.chemin.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            if ui.add(egui::Label::new(RichText::new(format!("📁 {dossier}")).weak()).sense(egui::Sense::click())).on_hover_text("Ouvrir le dossier").clicked() {
                                if let Some(p) = f.chemin.parent() {
                                    crate::outil::ouvrir(p);
                                }
                            }
                        });
                    });
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_et_demandes() {
        assert_eq!(civil(jours(2026, 11, 27)), (2026, 11, 27));
        assert_eq!(civil(jours(2024, 2, 29)), (2024, 2, 29));
        let t = jours(2026, 11, 27) * 86400 + 15 * 3600; // vendredi 27 novembre 2026, 15 h
        let r = comprendre("photos de la plage cet été", t);
        assert_eq!(r.genres, vec![Genre::Photo]);
        assert_eq!(r.periode, Some((jours(2026, 6, 1) * 86400, jours(2026, 9, 1) * 86400)));
        assert_eq!(r.mots.len(), 1);
        assert!(r.mots[0].contains(&"plage".to_string()) && r.mots[0].contains(&"beach".to_string()));
        let r = comprendre("pdf reçu sur discord", t);
        assert_eq!(r.genres, vec![Genre::Pdf]);
        assert!(r.internet && r.mots[0].contains(&"discordapp".to_string()));
        let r = comprendre("musique téléchargée hier soir", t);
        assert_eq!(r.genres, vec![Genre::Musique]);
        assert_eq!(r.periode, Some((jours(2026, 11, 26) * 86400, jours(2026, 11, 27) * 86400)));
        assert_eq!(r.heures, Some((18, 24)));
        assert!(r.mots.is_empty(), "{:?}", r.mots);
        let r = comprendre("gros vidéos de l'année dernière", t);
        assert_eq!(r.periode, Some((jours(2025, 1, 1) * 86400, jours(2026, 1, 1) * 86400)));
        assert_eq!(r.taille_min, 100 * 1024 * 1024);
        let r = comprendre("factures décembre", t);
        assert_eq!(r.periode, Some((jours(2025, 12, 1) * 86400, jours(2026, 1, 1) * 86400)));
        let r = comprendre("documents de la semaine dernière", t);
        assert_eq!(r.periode, Some((jours(2026, 11, 16) * 86400, jours(2026, 11, 23) * 86400)));
        let r = comprendre("photos 2024", t);
        assert_eq!(r.periode, Some((jours(2024, 1, 1) * 86400, jours(2025, 1, 1) * 86400)));
        // une fiche correspond… ou pas
        let f = Fiche { chemin: "/x/Vacances/IMG_001.jpg".into(), taille: 3_000_000, modif: 0, date: jours(2026, 7, 14) * 86400 + 11 * 3600, genre: Genre::Photo, source: String::new(), mots: "a sunny beach with people".into() };
        assert!(note(&f, &comprendre("photos de la plage cet été", t)).is_some());
        assert!(note(&f, &comprendre("photos de la plage l'été dernier", t)).is_none());
        assert!(note(&f, &comprendre("vacances", t)).is_some()); // nom du dossier
        assert!(note(&f, &comprendre("pdf", t)).is_none());
        let d = Fiche { chemin: "/x/Downloads/devoir.pdf".into(), taille: 100, date: t - 3600, genre: Genre::Pdf, source: "cdn.discordapp.com".into(), ..Default::default() };
        assert!(note(&d, &comprendre("pdf reçu sur discord", t)).is_some());
        assert!(note(&d, &comprendre("pdf reçu sur whatsapp", t)).is_none());
    }

    #[test]
    fn analyse() {
        let d = std::env::temp_dir().join(format!("sansdossiers_{}", std::process::id()));
        std::fs::create_dir_all(d.join("Cours")).unwrap();
        std::fs::write(d.join("Cours/notes.txt"), "Révisions : la photosynthèse des plantes vertes").unwrap();
        let mut z = zip::ZipWriter::new(std::fs::File::create(d.join("lettre.docx")).unwrap());
        z.start_file("word/document.xml", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, b"<w:t>Bonjour madame Dupont</w:t>").unwrap();
        z.finish().unwrap();
        let st = AtomicBool::new(false);
        let tous = parcourir(&[d.clone()], &st, 100);
        assert_eq!(tous.len(), 2);
        let fiches: Vec<Fiche> = tous.iter().map(|(p, t, m)| analyser(p, *t, *m, &Default::default())).collect();
        let t = fiches.iter().find(|f| f.genre == Genre::Document && f.chemin.ends_with("notes.txt")).unwrap();
        assert!(t.mots.contains("photosynthèse"));
        let w = fiches.iter().find(|f| f.chemin.ends_with("lettre.docx")).unwrap();
        assert!(w.mots.contains("dupont"), "{}", w.mots);
        let r = comprendre("document dupont", maintenant());
        assert!(note(w, &r).is_some());
        std::fs::remove_dir_all(&d).unwrap();
    }
}

/// Recherche demandée par le téléphone : ce qui a été compris et les fichiers trouvés.
pub fn trouver_json(q: &str) -> serde_json::Value {
    let ix = index();
    if ix.lock().unwrap().fiches.is_empty() {
        static EN_COURS: AtomicBool = AtomicBool::new(false);
        if !EN_COURS.swap(true, Relaxed) {
            let ix2 = ix.clone();
            std::thread::spawn(move || {
                mettre_a_jour(&ix2, &Arc::default(), &AtomicBool::new(false));
                EN_COURS.store(false, Relaxed);
            });
        }
        return serde_json::json!({ "total": 0, "liste": [], "compris": ["⏳ Outils découvre les fichiers du PC, réessaie dans une minute"] });
    }
    let r = comprendre(q, maintenant());
    let i = ix.lock().unwrap();
    let mut v: Vec<(usize, &Fiche)> = i.fiches.iter().filter_map(|f| note(f, &r).map(|s| (s, f))).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.date.cmp(&a.1.date)));
    let liste: Vec<serde_json::Value> = v.iter().take(80).map(|(_, f)| serde_json::json!({
        "chemin": f.chemin.display().to_string(),
        "nom": f.chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        "taille": f.taille, "date": f.date, "dossier": false,
        "icone": f.genre.icone(), "source": f.source,
    })).collect();
    serde_json::json!({ "total": v.len(), "liste": liste, "compris": r.compris })
}
