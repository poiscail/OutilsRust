//! L'agent (9) : tu dis ce que tu veux en français (« range mes téléchargements, compresse les
//! vidéos de plus de 1 Go et préviens-moi quand c'est fini ») et Outils enchaîne lui-même les
//! actions. Les demandes simples sont comprises tout de suite ; les autres passent par l'IA locale.
//! Rien de dangereux ne se fait sans ton accord, et les rangements s'annulent d'un clic.
use crate::outil::{charger, Cat, Outil};
use crate::sans_dossiers::{analyser, comprendre as comprendre_recherche, corbeille, dossier_dit, dossiers_perso, index, maintenant, mettre_a_jour, note, parcourir, taille_lisible, Genre};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Ranger,
    Chercher,
    GrosFichiers,
    Doublons,
    SupprimerDoublons,
    CompresserImages,
    CompresserVideos,
    ConvertirImages,
    RenommerPhotos,
    Zipper,
    CorbeilleVieux,
    EspaceDisque,
    Ouvrir,
    OuvrirOutil,
    Minuteur,
    Notifier,
    Dire,
}

const ACTIONS: [(Action, &str, &str); 17] = [
    (Action::Ranger, "ranger", "trie les fichiers d'un dossier dans des sous-dossiers (Images, Vidéos, Documents…)"),
    (Action::Chercher, "chercher", "cherche des fichiers ; texte = ce qu'on cherche (« photos de la plage cet été »)"),
    (Action::GrosFichiers, "gros_fichiers", "liste les plus gros fichiers ; nombre = combien"),
    (Action::Doublons, "doublons", "liste les fichiers en double"),
    (Action::SupprimerDoublons, "supprimer_doublons", "met les copies en double à la corbeille"),
    (Action::CompresserImages, "compresser_images", "fait des copies plus légères des photos ; nombre = qualité 1-100"),
    (Action::CompresserVideos, "compresser_videos", "fait des copies plus légères des vidéos ; nombre = taille minimale en Mo"),
    (Action::ConvertirImages, "convertir_images", "convertit les images ; texte = format (png, jpg, webp)"),
    (Action::RenommerPhotos, "renommer_photos", "renomme les photos avec leur date de prise de vue"),
    (Action::Zipper, "zipper", "crée une archive .zip d'un dossier"),
    (Action::CorbeilleVieux, "corbeille_vieux", "met à la corbeille les fichiers pas modifiés depuis « nombre » jours"),
    (Action::EspaceDisque, "espace_disque", "dit combien de place il reste"),
    (Action::Ouvrir, "ouvrir", "ouvre un dossier, un fichier ou un site ; texte = adresse si c'est un site"),
    (Action::OuvrirOutil, "ouvrir_outil", "ouvre un outil d'Outils ; texte = son nom"),
    (Action::Minuteur, "minuteur", "rappel dans « nombre » minutes ; texte = message"),
    (Action::Notifier, "notifier", "envoie une notification ; texte = message"),
    (Action::Dire, "dire", "répond simplement ; texte = la réponse"),
];

impl Action {
    pub fn dangereuse(self) -> bool {
        matches!(self, Action::SupprimerDoublons | Action::CorbeilleVieux | Action::RenommerPhotos)
    }
    fn icone(self) -> &'static str {
        use Action::*;
        match self {
            Ranger => "🗂",
            Chercher => "🔎",
            GrosFichiers => "🐘",
            Doublons | SupprimerDoublons => "👥",
            CompresserImages => "🖼",
            CompresserVideos => "🎞",
            ConvertirImages => "🔁",
            RenommerPhotos => "🏷",
            Zipper => "📦",
            CorbeilleVieux => "🗑",
            EspaceDisque => "💽",
            Ouvrir | OuvrirOutil => "📂",
            Minuteur => "⏰",
            Notifier => "🔔",
            Dire => "💬",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Etape {
    pub action: Action,
    pub dossier: String,
    pub texte: String,
    pub nombre: f64,
}

impl Default for Etape {
    fn default() -> Self {
        Etape { action: Action::Dire, dossier: String::new(), texte: String::new(), nombre: 0.0 }
    }
}

fn nom_dossier(d: &str) -> String {
    match dossier_dit(d) {
        Some(p) => dossiers_perso().into_iter().find(|x| x.2 == p).map(|x| x.0.to_string()).unwrap_or_else(|| p.display().to_string()),
        None if d.is_empty() => "Téléchargements".into(),
        None => d.to_string(),
    }
}

impl Etape {
    pub fn decrire(&self) -> String {
        let d = nom_dossier(&self.dossier);
        use Action::*;
        match self.action {
            Ranger => format!("Ranger « {d} » par type de fichier"),
            Chercher => format!("Chercher : {}", self.texte),
            GrosFichiers => format!("Trouver les {} plus gros fichiers{}", self.nombre.max(1.0) as u32, if self.dossier.is_empty() { String::new() } else { format!(" de « {d} »") }),
            Doublons => format!("Trouver les doublons dans « {d} »"),
            SupprimerDoublons => format!("Mettre à la corbeille les copies en double dans « {d} »"),
            CompresserImages => format!("Faire des copies plus légères des photos de « {d} » (qualité {})", qualite(self.nombre)),
            CompresserVideos => format!("Faire des copies plus légères des vidéos de plus de {} dans « {d} »", taille_lisible((min_mo(self.nombre) * 1_048_576.0) as u64)),
            ConvertirImages => format!("Convertir les images de « {d} » en {}", format_image(&self.texte)),
            RenommerPhotos => format!("Renommer les photos de « {d} » avec leur date"),
            Zipper => format!("Créer une archive .zip de « {d} »"),
            CorbeilleVieux => format!("Mettre à la corbeille les fichiers de « {d} » pas touchés depuis {} jours", jours_vieux(self.nombre)),
            EspaceDisque => "Regarder la place libre sur le disque".into(),
            Ouvrir => format!("Ouvrir {}", if self.texte.is_empty() { format!("« {d} »") } else { self.texte.clone() }),
            OuvrirOutil => format!("Ouvrir l'outil « {} »", self.texte),
            Minuteur => format!("Te rappeler dans {} min : {}", self.nombre.max(1.0) as u32, if self.texte.is_empty() { "⏰" } else { &self.texte }),
            Notifier => format!("Te prévenir : {}", if self.texte.is_empty() { "c'est fini !" } else { &self.texte }),
            Dire => self.texte.clone(),
        }
    }
}

fn qualite(n: f64) -> u8 {
    if n >= 10.0 && n <= 100.0 { n as u8 } else { 80 }
}
fn min_mo(n: f64) -> f64 {
    if n > 0.0 { n } else { 500.0 }
}
fn jours_vieux(n: f64) -> u32 {
    if n >= 1.0 { n as u32 } else { 90 }
}
fn format_image(t: &str) -> &'static str {
    let t = t.to_lowercase();
    if t.contains("png") {
        "png"
    } else if t.contains("webp") {
        "webp"
    } else {
        "jpg"
    }
}

// ---------------------------------------------------------------------------------------------------
// Comprendre sans IA
// ---------------------------------------------------------------------------------------------------

const VERBES: [&str; 30] = [
    "range", "trie", "organise", "classe", "compresse", "réduis", "allège", "préviens", "previens", "notifie", "avertis", "ouvre", "lance", "cherche", "trouve", "retrouve", "zippe", "supprime", "efface", "vire", "jette", "mets", "convertis", "renomme", "rappelle", "montre", "dis", "nettoie", "vide", "fais",
];

/// Découpe une demande en morceaux (« …, puis … et préviens-moi »).
pub fn morceaux(q: &str) -> Vec<String> {
    let bas = q.to_lowercase().replace(['’'], "'");
    let mut v = Vec::new();
    for p in bas.split([',', ';', '.', '\n']).flat_map(|p| p.split(" puis ")).flat_map(|p| p.split(" ensuite ")) {
        // « et » ne coupe que s'il est suivi d'un verbe d'action
        let mots: Vec<&str> = p.split_whitespace().collect();
        let mut cur: Vec<&str> = Vec::new();
        for (k, m) in mots.iter().enumerate() {
            let suivant = mots.get(k + 1).copied().unwrap_or("");
            if *m == "et" && VERBES.iter().any(|v| suivant.starts_with(v)) && !cur.is_empty() {
                v.push(cur.join(" "));
                cur.clear();
            } else if *m != "et" || !cur.is_empty() {
                cur.push(m);
            }
        }
        if !cur.is_empty() {
            v.push(cur.join(" "));
        }
    }
    v.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// (nombre, unité) dans une phrase : « 1 go », « 30 jours », « 10 minutes ».
fn quantite(s: &str) -> Option<(f64, String)> {
    let re = regex::Regex::new(r"(\d+(?:[.,]\d+)?)\s*(go|gb|giga|mo|mb|méga|mega|ko|jours?|semaines?|mois|ans?|minutes?|mn|min|heures?|h)\b").ok()?;
    let c = re.captures(s)?;
    Some((c[1].replace(',', ".").parse().ok()?, c[2].to_string()))
}

fn en_mo(n: f64, u: &str) -> f64 {
    match u {
        "go" | "gb" | "giga" => n * 1024.0,
        "ko" => n / 1024.0,
        _ => n,
    }
}

fn en_jours(n: f64, u: &str) -> f64 {
    match u.trim_end_matches('s') {
        "semaine" => n * 7.0,
        "moi" | "mois" => n * 30.0,
        "an" => n * 365.0,
        _ => n,
    }
}

/// Comprend un morceau de demande sans IA. None = pas compris.
pub fn regle(m: &str) -> Option<Etape> {
    let a = |x: &str| m.contains(x);
    let dossier = dossier_dit(m).map(|p| p.display().to_string()).unwrap_or_default();
    let q = quantite(m);
    let mut e = Etape { dossier: dossier.clone(), ..Default::default() };
    if a("rappelle") || a("minuteur") || (a("dans") && q.as_ref().is_some_and(|x| x.1.starts_with("min") || x.1.starts_with('h') || x.1 == "mn") && (a("préviens") || a("previens") || a("dis-moi"))) {
        e.action = Action::Minuteur;
        if let Some((n, u)) = &q {
            e.nombre = if u.starts_with('h') { n * 60.0 } else { *n };
        }
        e.texte = m.split_once(" de ").or_else(|| m.split_once(" d'")).or_else(|| m.split_once(" pour ")).map(|x| x.1.trim().to_string()).unwrap_or_default();
        return (e.nombre > 0.0).then_some(e);
    }
    if a("préviens") || a("previens") || a("notifie") || a("avertis") || a("dis-moi quand") || a("dis moi quand") {
        e.action = Action::Notifier;
        e.texte = "C'est fini !".into();
        e.dossier.clear();
        return Some(e);
    }
    if a("doublon") || a("en double") {
        e.action = if a("supprime") || a("efface") || a("enlève") || a("vire") || a("corbeille") || a("jette") { Action::SupprimerDoublons } else { Action::Doublons };
        return Some(e);
    }
    if a("compresse") || a("réduis") || a("allège") || a("plus légère") || a("plus léger") {
        if a("vidéo") || a("video") || a("film") {
            e.action = Action::CompresserVideos;
            e.nombre = q.map(|(n, u)| en_mo(n, &u)).unwrap_or(0.0);
        } else if a("photo") || a("image") {
            e.action = Action::CompresserImages;
        } else {
            e.action = Action::Zipper;
        }
        return Some(e);
    }
    if a("zip") || a("archive") {
        e.action = Action::Zipper;
        return Some(e);
    }
    if a("convertis") || a("convertir") || a("transforme") {
        e.action = Action::ConvertirImages;
        e.texte = format_image(m).into();
        return Some(e);
    }
    if a("renomme") && (a("photo") || a("image")) {
        e.action = Action::RenommerPhotos;
        return Some(e);
    }
    let mots: Vec<&str> = m.split(|c: char| !c.is_alphanumeric() && c != 'é').collect();
    if mots.iter().any(|w| w.starts_with("range") || *w == "trie" || w.starts_with("organise") || w.starts_with("classe") || *w == "ranger" || *w == "trier") {
        e.action = Action::Ranger;
        return Some(e);
    }
    if (a("supprime") || a("efface") || a("vire") || a("jette") || a("corbeille") || a("nettoie") || a("vide")) && (a("vieux") || a("ancien") || a("depuis") || a("plus de")) {
        e.action = Action::CorbeilleVieux;
        e.nombre = q.map(|(n, u)| en_jours(n, &u)).unwrap_or(0.0);
        return Some(e);
    }
    if a("gros fichier") || a("plus gros") || a("prend de la place") || a("prennent de la place") || a("prend toute la place") {
        e.action = Action::GrosFichiers;
        e.nombre = 10.0;
        return Some(e);
    }
    if a("espace") || a("place libre") || a("reste de place") || a("reste-t-il") || a("disque plein") {
        e.action = Action::EspaceDisque;
        return Some(e);
    }
    for v in ["cherche ", "trouve ", "retrouve ", "où est ", "où sont ", "montre-moi ", "montre moi "] {
        if let Some((_, reste)) = m.split_once(v) {
            e.action = Action::Chercher;
            e.texte = reste.trim().trim_start_matches("moi ").to_string();
            e.dossier.clear();
            return (!e.texte.is_empty()).then_some(e);
        }
    }
    for v in ["ouvre ", "lance "] {
        if let Some((_, reste)) = m.split_once(v) {
            let reste = reste.trim().trim_start_matches("l'outil ").trim_start_matches("le ").trim_start_matches("la ").trim_start_matches("les ").trim_start_matches("mon ").trim_start_matches("mes ");
            if !dossier.is_empty() {
                e.action = Action::Ouvrir;
            } else if reste.starts_with("http") || reste.contains(".com") || reste.contains(".fr") {
                e.action = Action::Ouvrir;
                e.texte = if reste.starts_with("http") { reste.to_string() } else { format!("https://{reste}") };
            } else {
                e.action = Action::OuvrirOutil;
                e.texte = reste.to_string();
            }
            return Some(e);
        }
    }
    None
}

/// Plan sans IA, et les morceaux pas compris.
pub fn planifier(q: &str) -> (Vec<Etape>, Vec<String>) {
    let mut plan = Vec::new();
    let mut incompris = Vec::new();
    for m in morceaux(q) {
        match regle(&m) {
            Some(e) => plan.push(e),
            None => incompris.push(m),
        }
    }
    // « préviens-moi » après une action longue : on garde l'ordre, rien à faire de plus
    (plan, incompris)
}

// ---------------------------------------------------------------------------------------------------
// Comprendre avec l'IA locale
// ---------------------------------------------------------------------------------------------------

fn consigne() -> String {
    let mut s = String::from("Tu es l'agent d'Outils, sur l'ordinateur de l'utilisateur. Tu transformes sa demande en une liste d'étapes JSON, dans l'ordre. Actions possibles :\n");
    for (_, cle, d) in ACTIONS {
        s += &format!("- {cle} : {d}\n");
    }
    s += "Champs : « dossier » = un dossier (téléchargements, bureau, documents, images, musique, vidéos, ou un chemin), vide si aucun ; « texte » ; « nombre » (0 si inutile).\nSi la demande n'est pas faisable avec ces actions, réponds une seule étape « dire » qui explique gentiment pourquoi, en français.\n";
    s += "Exemple : « fais de la place dans mes téléchargements » → [{\"action\":\"gros_fichiers\",\"dossier\":\"téléchargements\",\"texte\":\"\",\"nombre\":10},{\"action\":\"doublons\",\"dossier\":\"téléchargements\",\"texte\":\"\",\"nombre\":0}]";
    s
}

fn schema() -> serde_json::Value {
    let actions: Vec<&str> = ACTIONS.iter().map(|a| a.1).collect();
    serde_json::json!({
        "type": "object",
        "properties": { "etapes": { "type": "array", "items": { "type": "object", "properties": {
            "action": { "type": "string", "enum": actions },
            "dossier": { "type": "string" }, "texte": { "type": "string" }, "nombre": { "type": "number" }
        }, "required": ["action", "dossier", "texte", "nombre"] } } },
        "required": ["etapes"]
    })
}

/// Lit la réponse JSON de l'IA.
pub fn lire_plan(v: &serde_json::Value) -> Vec<Etape> {
    v["etapes"].as_array().map(|a| a.iter().filter_map(|x| serde_json::from_value::<Etape>(x.clone()).ok()).take(12).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------
// Faire
// ---------------------------------------------------------------------------------------------------

/// Déplacements faits (pour tout annuler).
pub type Journal = Arc<Mutex<Vec<(PathBuf, PathBuf)>>>;

fn libre(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let pere = p.parent().unwrap_or(Path::new("")).to_path_buf();
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..).map(|k| pere.join(format!("{stem} ({k}){ext}"))).find(|q| !q.exists()).unwrap()
}

fn deplacer(de: &Path, vers: PathBuf, j: &Journal) -> bool {
    let vers = libre(vers);
    if let Some(p) = vers.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if std::fs::rename(de, &vers).is_ok() {
        j.lock().unwrap().push((de.to_path_buf(), vers));
        true
    } else {
        false
    }
}

fn fichiers_directs(d: &Path) -> Vec<(PathBuf, u64, i64)> {
    std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| {
            let m = e.metadata().ok();
            let t = m.as_ref().map(|m| m.len()).unwrap_or(0);
            let md = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            (e.path(), t, md)
        })
        .collect()
}

fn dossier_rangement(g: Genre) -> &'static str {
    use Genre::*;
    match g {
        Photo => "Images",
        Video => "Vidéos",
        Musique => "Musique",
        Document | Pdf | Tableur | Presentation => "Documents",
        Archive => "Archives",
        Programme => "Programmes",
        _ => "Autres",
    }
}

fn espace_libre(p: &Path) -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let w: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
        let (mut libre, mut total, mut _l) = (0u64, 0u64, 0u64);
        let ok = unsafe { windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(w.as_ptr(), &mut libre, &mut total, &mut _l) };
        (ok != 0).then_some((libre, total))
    }
    #[cfg(not(windows))]
    {
        let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| (s.f_bavail as u64 * s.f_frsize as u64, s.f_blocks as u64 * s.f_frsize as u64))
    }
}

/// Exécute une étape. `dire` reçoit les lignes de compte rendu.
pub fn faire(e: &Etape, journal: &Journal, stop: &AtomicBool, dire: &dyn Fn(String)) -> Result<String, String> {
    let d = if e.dossier.trim().is_empty() { dossiers_perso()[0].2.clone() } else { dossier_dit(&e.dossier).ok_or(format!("je ne trouve pas le dossier « {} »", e.dossier))? };
    use Action::*;
    match e.action {
        Ranger => {
            let mut n = 0;
            let mut dossiers = std::collections::HashSet::new();
            for (p, _, m) in fichiers_directs(&d) {
                let nom = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                // pas les téléchargements en cours, ni ce qui vient d'arriver
                if [".crdownload", ".part", ".tmp", ".download"].iter().any(|x| nom.ends_with(x)) || nom == "desktop.ini" || maintenant() - m < 120 {
                    continue;
                }
                let sous = dossier_rangement(Genre::de(&p));
                if d.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(sous)) {
                    continue;
                }
                if deplacer(&p, d.join(sous).join(p.file_name().unwrap()), journal) {
                    n += 1;
                    dossiers.insert(sous);
                }
            }
            Ok(format!("{n} fichiers rangés dans {} dossiers", dossiers.len()))
        }
        Chercher => {
            let ix = index();
            if ix.lock().unwrap().fiches.is_empty() {
                dire("Je fais d'abord connaissance avec tes fichiers…".into());
                mettre_a_jour(&ix, &Arc::default(), stop);
            }
            let r = comprendre_recherche(&e.texte, maintenant());
            let i = ix.lock().unwrap();
            let mut v: Vec<(usize, &crate::sans_dossiers::Fiche)> = i.fiches.iter().filter_map(|f| note(f, &r).map(|s| (s, f))).collect();
            v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.date.cmp(&a.1.date)));
            for (_, f) in v.iter().take(8) {
                dire(format!("   {} {}", f.genre.icone(), f.chemin.display()));
            }
            Ok(format!("{} fichier(s) trouvé(s) — l'outil « Fichiers sans dossiers » les montre tous", v.len()))
        }
        GrosFichiers => {
            let racines = if e.dossier.trim().is_empty() { index().lock().unwrap().racines.clone() } else { vec![d] };
            let mut v = parcourir(&racines, stop, 500_000);
            v.sort_by(|a, b| b.1.cmp(&a.1));
            let n = (e.nombre.max(1.0) as usize).min(50);
            for (p, t, _) in v.iter().take(n) {
                dire(format!("   {} — {}", taille_lisible(*t), p.display()));
            }
            Ok(format!("ensemble : {}", taille_lisible(v.iter().take(n).map(|x| x.1).sum())))
        }
        Doublons | SupprimerDoublons => {
            let f = parcourir(&[d.clone()], stop, 300_000);
            let m: Vec<_> = crate::donjon::detecter(&f, &[], maintenant(), &dossiers_perso()[0].2, &[]).into_iter().filter(|m| m.espece == crate::donjon::Espece::Clone).collect();
            let total: u64 = m.iter().map(|x| x.taille).sum();
            if e.action == Doublons {
                for x in m.iter().take(15) {
                    dire(format!("   {} ({})", x.chemin.display(), x.detail));
                }
                return Ok(format!("{} copie(s) en double, {} à gagner", m.len(), taille_lisible(total)));
            }
            let n = m.iter().filter(|x| corbeille(&x.chemin)).count();
            Ok(format!("{n} copie(s) mise(s) à la corbeille, {} libérés", taille_lisible(total)))
        }
        CompresserImages | ConvertirImages => {
            let sortie = d.join(if e.action == CompresserImages { "Compressées" } else { "Converties" });
            let format = if e.action == CompresserImages { "jpg" } else { format_image(&e.texte) };
            let (mut n, mut avant, mut apres) = (0, 0u64, 0u64);
            for (p, t, _) in fichiers_directs(&d) {
                if stop.load(Relaxed) {
                    break;
                }
                if Genre::de(&p) != Genre::Photo || (e.action == CompresserImages && t < 300 * 1024) {
                    continue;
                }
                let Ok(img) = image::open(&p) else { continue };
                let img = if e.action == CompresserImages && (img.width() > 2560 || img.height() > 2560) { img.resize(2560, 2560, image::imageops::FilterType::Lanczos3) } else { img };
                let _ = std::fs::create_dir_all(&sortie);
                let cible = libre(sortie.join(p.with_extension(format).file_name().unwrap()));
                let ok = match format {
                    "jpg" => std::fs::File::create(&cible).ok().is_some_and(|f| image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), qualite(e.nombre)).encode_image(&img.to_rgb8()).is_ok()),
                    _ => img.save(&cible).is_ok(),
                };
                if ok {
                    n += 1;
                    avant += t;
                    apres += std::fs::metadata(&cible).map(|m| m.len()).unwrap_or(0);
                    dire(format!("   ✔ {}", cible.file_name().unwrap().to_string_lossy()));
                }
            }
            Ok(if e.action == CompresserImages {
                format!("{n} photo(s) : {} → {} (dans « Compressées », les originales n'ont pas bougé)", taille_lisible(avant), taille_lisible(apres))
            } else {
                format!("{n} image(s) converties dans « Converties »")
            })
        }
        CompresserVideos => {
            let min = (min_mo(e.nombre) * 1_048_576.0) as u64;
            let videos: Vec<_> = fichiers_directs(&d).into_iter().filter(|(p, t, _)| Genre::de(p) == Genre::Video && *t >= min).collect();
            if videos.is_empty() {
                return Ok(format!("aucune vidéo de plus de {}", taille_lisible(min)));
            }
            let sortie = d.join("Compressées");
            let _ = std::fs::create_dir_all(&sortie);
            let (mut avant, mut apres) = (0u64, 0u64);
            for (k, (p, t, _)) in videos.iter().enumerate() {
                if stop.load(Relaxed) {
                    break;
                }
                dire(format!("   🎞 {}/{} : {} ({})…", k + 1, videos.len(), p.file_name().unwrap().to_string_lossy(), taille_lisible(*t)));
                let cible = libre(sortie.join(p.with_extension("mp4").file_name().unwrap()));
                let mut c = crate::createur::ffmpeg()?;
                c.arg("-y").arg("-i").arg(p).args(["-c:v", "libx264", "-crf", "27", "-preset", "veryfast", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]).arg(&cible);
                c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    c.creation_flags(0x0800_0000);
                }
                if c.status().map(|s| s.success()).unwrap_or(false) {
                    avant += t;
                    apres += std::fs::metadata(&cible).map(|m| m.len()).unwrap_or(0);
                }
            }
            Ok(format!("{} → {} (dans « Compressées », les originales n'ont pas bougé)", taille_lisible(avant), taille_lisible(apres)))
        }
        RenommerPhotos => {
            let mut n = 0;
            for (p, t, m) in fichiers_directs(&d) {
                if Genre::de(&p) != Genre::Photo {
                    continue;
                }
                let f = analyser(&p, t, m, &Default::default());
                let (a, mo, j) = crate::sans_dossiers::civil(f.date.div_euclid(86400));
                let s = f.date.rem_euclid(86400);
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let nom = format!("{a:04}-{mo:02}-{j:02} {:02}.{:02}.{:02}.{ext}", s / 3600, s / 60 % 60, s % 60);
                if p.file_name().is_some_and(|x| x.to_string_lossy() == nom) {
                    continue;
                }
                if deplacer(&p, d.join(nom), journal) {
                    n += 1;
                }
            }
            Ok(format!("{n} photo(s) renommée(s)"))
        }
        Zipper => {
            let cible = libre(d.with_extension("zip"));
            let mut z = zip::ZipWriter::new(std::fs::File::create(&cible).map_err(|e| e.to_string())?);
            let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
            let f = parcourir(&[d.clone()], stop, 1_000_000);
            for (p, _, _) in &f {
                let rel = p.strip_prefix(&d).unwrap_or(p).to_string_lossy().replace('\\', "/");
                if z.start_file(rel, opt).is_ok() {
                    if let Ok(mut src) = std::fs::File::open(p) {
                        let _ = std::io::copy(&mut src, &mut z);
                    }
                }
            }
            z.finish().map_err(|e| e.to_string())?;
            Ok(format!("{} fichiers dans {} ({})", f.len(), cible.display(), taille_lisible(std::fs::metadata(&cible).map(|m| m.len()).unwrap_or(0))))
        }
        CorbeilleVieux => {
            let limite = maintenant() - jours_vieux(e.nombre) as i64 * 86400;
            let (mut n, mut total) = (0, 0u64);
            for (p, t, m) in fichiers_directs(&d) {
                if m < limite && corbeille(&p) {
                    n += 1;
                    total += t;
                }
            }
            Ok(format!("{n} fichier(s) à la corbeille, {} libérés", taille_lisible(total)))
        }
        EspaceDisque => {
            let (l, t) = espace_libre(&d).ok_or("impossible de lire le disque")?;
            Ok(format!("il reste {} de libre sur {} ({:.0} %)", taille_lisible(l), taille_lisible(t), l as f64 / t.max(1) as f64 * 100.0))
        }
        Ouvrir => {
            if e.texte.starts_with("http") {
                #[cfg(windows)]
                let _ = std::process::Command::new("explorer").arg(&e.texte).spawn();
                #[cfg(not(windows))]
                let _ = std::process::Command::new("xdg-open").arg(&e.texte).spawn();
                return Ok(format!("{} ouvert", e.texte));
            }
            crate::outil::ouvrir(&d);
            Ok(format!("{} ouvert", d.display()))
        }
        OuvrirOutil => {
            #[cfg(windows)]
            {
                let noms = crate::interface2::NOMS.lock().unwrap().clone();
                let cherche = e.texte.to_lowercase();
                let mots = crate::ia::mots_recherche(&cherche);
                let meilleur = noms.iter().map(|n| (mots.iter().filter(|m| n.to_lowercase().contains(m.as_str())).count(), n)).filter(|x| x.0 > 0).max_by_key(|x| x.0);
                match meilleur {
                    Some((_, n)) => {
                        crate::interface2::ouvrir_outil(n);
                        Ok(format!("« {n} » ouvert"))
                    }
                    None => Err(format!("je ne connais pas d'outil « {} »", e.texte)),
                }
            }
            #[cfg(not(windows))]
            Err(format!("ouvre « {} » depuis le menu à gauche", e.texte))
        }
        Minuteur => {
            let min = e.nombre.max(1.0);
            let t = if e.texte.is_empty() { "C'est l'heure !".to_string() } else { e.texte.clone() };
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs_f64(min * 60.0));
                crate::systeme::notifier("⏰ Rappel", &t);
            });
            Ok(format!("je te préviens dans {} min", min as u32))
        }
        Notifier => {
            crate::systeme::notifier("🤖 Outils", if e.texte.is_empty() { "C'est fini !" } else { &e.texte });
            Ok("notification envoyée".into())
        }
        Dire => Ok(e.texte.clone()),
    }
}

/// Remet les fichiers déplacés à leur place.
pub fn annuler(j: &Journal) -> usize {
    let mut v = j.lock().unwrap();
    let mut n = 0;
    while let Some((de, vers)) = v.pop() {
        if !de.exists() && std::fs::rename(&vers, &de).is_ok() {
            n += 1;
            // supprime les dossiers de rangement redevenus vides
            if let Some(p) = vers.parent() {
                let _ = std::fs::remove_dir(p);
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum Ligne {
    Moi(String),
    Plan,
    Info(String),
    Ok(String),
    Erreur(String),
}

pub struct Agent {
    saisie: String,
    lignes: Arc<Mutex<Vec<Ligne>>>,
    plan: Arc<Mutex<Vec<(Etape, bool)>>>,
    occupe: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    journal: Journal,
    llama: Arc<Mutex<Option<crate::ia::Llama>>>,
    etat: Arc<Mutex<String>>,
    avec_ia: bool,
}

impl Agent {
    pub fn new() -> Self {
        Agent { saisie: String::new(), lignes: Arc::default(), plan: Arc::default(), occupe: Arc::default(), stop: Arc::default(), journal: Arc::default(), llama: Arc::default(), etat: Arc::default(), avec_ia: true }
    }

    fn demander(&mut self) {
        let q = std::mem::take(&mut self.saisie).trim().to_string();
        if q.is_empty() || self.occupe.load(Relaxed) {
            return;
        }
        self.lignes.lock().unwrap().push(Ligne::Moi(q.clone()));
        let (plan, incompris) = planifier(&q);
        if incompris.is_empty() || !self.avec_ia {
            if !incompris.is_empty() {
                self.lignes.lock().unwrap().push(Ligne::Erreur(format!("Je n'ai pas compris : « {} ». Active l'IA pour les demandes plus libres.", incompris.join(" », « "))));
            }
            if !plan.is_empty() {
                *self.plan.lock().unwrap() = plan.into_iter().map(|e| (e, false)).collect();
                self.lignes.lock().unwrap().push(Ligne::Plan);
            }
            return;
        }
        // l'IA prend le relais
        self.occupe.store(true, Relaxed);
        let (li, pl, oc, ll, et) = (self.lignes.clone(), self.plan.clone(), self.occupe.clone(), self.llama.clone(), self.etat.clone());
        let choix = crate::ia::MODELES[charger::<serde_json::Value>("assistant_ia")["modele"].as_u64().unwrap_or(1).min(2) as usize];
        std::thread::spawn(move || {
            let r = (|| -> Result<Vec<Etape>, String> {
                let modele = crate::ia::fichier(choix.2, choix.1, &et, &format!("l'IA « {} » ({}, une seule fois)", choix.0, choix.3))?;
                let mut l = ll.lock().unwrap();
                if l.as_ref().is_none_or(|x| x.modele() != modele) {
                    *l = None;
                    *l = Some(crate::ia::Llama::demarrer(&modele, None, 8767, &et)?);
                }
                *et.lock().unwrap() = "L'IA réfléchit…".into();
                let msg = serde_json::json!([{"role": "system", "content": consigne()}, {"role": "user", "content": q}]);
                let v = l.as_ref().ok_or("IA arrêtée")?.json(msg, schema(), 600)?;
                Ok(lire_plan(&v))
            })();
            *et.lock().unwrap() = String::new();
            match r {
                Ok(p) if !p.is_empty() => {
                    *pl.lock().unwrap() = p.into_iter().map(|e| (e, false)).collect();
                    li.lock().unwrap().push(Ligne::Plan);
                }
                Ok(_) => li.lock().unwrap().push(Ligne::Erreur("L'IA n'a pas trouvé comment faire.".into())),
                Err(e) => li.lock().unwrap().push(Ligne::Erreur(format!("IA : {e}"))),
            }
            oc.store(false, Relaxed);
        });
    }

    fn lancer(&mut self) {
        let plan: Vec<Etape> = self.plan.lock().unwrap().drain(..).map(|x| x.0).collect();
        let resume: Vec<String> = plan.iter().map(|e| format!("{} {}", e.action.icone(), e.decrire())).collect();
        {
            let mut l = self.lignes.lock().unwrap();
            if let Some(k) = l.iter().rposition(|x| *x == Ligne::Plan) {
                l[k] = Ligne::Info(format!("Plan :\n{}", resume.join("\n")));
            }
        }
        self.occupe.store(true, Relaxed);
        self.stop.store(false, Relaxed);
        let (li, oc, st, jo) = (self.lignes.clone(), self.occupe.clone(), self.stop.clone(), self.journal.clone());
        std::thread::spawn(move || {
            for e in plan {
                if st.load(Relaxed) {
                    li.lock().unwrap().push(Ligne::Erreur("⏹ Arrêté.".into()));
                    break;
                }
                li.lock().unwrap().push(Ligne::Info(format!("{} {}…", e.action.icone(), e.decrire())));
                let li2 = li.clone();
                let dire = move |s: String| li2.lock().unwrap().push(Ligne::Info(s));
                match faire(&e, &jo, &st, &dire) {
                    Ok(s) => li.lock().unwrap().push(Ligne::Ok(s)),
                    Err(s) => li.lock().unwrap().push(Ligne::Erreur(s)),
                }
            }
            oc.store(false, Relaxed);
        });
    }
}

const EXEMPLES: [&str; 6] = [
    "range mes téléchargements et préviens-moi quand c'est fini",
    "compresse les vidéos de plus de 1 Go dans mes vidéos",
    "trouve les pdf reçus sur discord ce mois-ci",
    "quels sont les plus gros fichiers ? et combien reste-t-il de place",
    "supprime les doublons de mes images",
    "rappelle-moi dans 20 minutes de sortir le gâteau",
];

impl Outil for Agent {
    fn nom(&self) -> &'static str {
        "🤖 Agent : dis-lui quoi faire"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Agent : dis-lui quoi faire");
        ui.weak("Écris ce que tu veux, comme à une personne. Outils prépare un plan, tu vérifies, il fait tout. Rien de risqué ne se fait sans ton accord.");
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal(|ui| {
            let r = ui.add_enabled(!occupe, egui::TextEdit::singleline(&mut self.saisie).desired_width(ui.available_width() - 110.0).hint_text("ex. : range mes téléchargements et préviens-moi quand c'est fini"));
            if (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.add_enabled(!occupe, egui::Button::new(RichText::new("➤ Demander").strong())).clicked() {
                self.demander();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.avec_ia, "🧠 IA pour les demandes libres").on_hover_text("Les demandes simples sont comprises tout de suite. Les autres utilisent l'IA locale (téléchargée une fois, sans internet ensuite).");
            ui.weak("Essaie :");
            for e in EXEMPLES {
                if ui.small_button(e).clicked() && !occupe {
                    self.saisie = e.into();
                    self.demander();
                }
            }
        });
        let e = self.etat.lock().unwrap().clone();
        if !e.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(e);
            });
        }
        ui.separator();
        let lignes = self.lignes.lock().unwrap().clone();
        let mut action = None;
        egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink(false).max_height(ui.available_height() - 40.0).show(ui, |ui| {
            for l in &lignes {
                match l {
                    Ligne::Moi(t) => {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            egui::Frame::new().fill(ui.visuals().selection.bg_fill).corner_radius(8.0).inner_margin(8.0).show(ui, |ui| {
                                ui.label(RichText::new(t).color(Color32::WHITE));
                            });
                        });
                    }
                    Ligne::Plan => {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new("📋 Voilà ce que je vais faire :").strong());
                            let mut plan = self.plan.lock().unwrap();
                            let mut suppr = None;
                            for (k, (e, ok)) in plan.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}. {}", k + 1, e.action.icone()));
                                    if e.action.dangereuse() {
                                        ui.checkbox(ok, RichText::new(e.decrire()).color(Color32::from_rgb(240, 150, 80))).on_hover_text("Action qui supprime ou renomme : coche pour accepter");
                                    } else {
                                        ui.label(e.decrire());
                                    }
                                    if ui.small_button("✖").on_hover_text("Enlever cette étape").clicked() {
                                        suppr = Some(k);
                                    }
                                });
                            }
                            if let Some(k) = suppr {
                                plan.remove(k);
                            }
                            let pret = !plan.is_empty() && plan.iter().all(|(e, ok)| !e.action.dangereuse() || *ok);
                            drop(plan);
                            ui.horizontal(|ui| {
                                if ui.add_enabled(pret, egui::Button::new(RichText::new("▶ C'est parti").strong())).on_disabled_hover_text("Coche les étapes en orange pour les accepter").clicked() {
                                    action = Some(true);
                                }
                                if ui.button("Annuler").clicked() {
                                    action = Some(false);
                                }
                            });
                        });
                    }
                    Ligne::Info(t) => {
                        ui.label(t);
                    }
                    Ligne::Ok(t) => {
                        ui.colored_label(Color32::from_rgb(110, 210, 120), format!("✔ {t}"));
                    }
                    Ligne::Erreur(t) => {
                        ui.colored_label(Color32::from_rgb(240, 120, 100), format!("✖ {t}"));
                    }
                }
            }
        });
        match action {
            Some(true) => self.lancer(),
            Some(false) => {
                self.plan.lock().unwrap().clear();
                let mut l = self.lignes.lock().unwrap();
                if let Some(k) = l.iter().rposition(|x| *x == Ligne::Plan) {
                    l[k] = Ligne::Info("(plan annulé)".into());
                }
            }
            None => {}
        }
        ui.horizontal(|ui| {
            if occupe {
                ui.spinner();
                if ui.button("⏹ Arrêter").clicked() {
                    self.stop.store(true, Relaxed);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            }
            let n = self.journal.lock().unwrap().len();
            if n > 0 && !occupe && ui.button(format!("↩ Tout remettre comme avant ({n} fichiers déplacés)")).clicked() {
                let k = annuler(&self.journal);
                self.lignes.lock().unwrap().push(Ligne::Ok(format!("{k} fichiers remis à leur place")));
            }
            if !lignes.is_empty() && !occupe && ui.small_button("🧹 Effacer la conversation").clicked() {
                self.lignes.lock().unwrap().clear();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comprendre_sans_ia() {
        let (p, inc) = planifier("Range mes téléchargements, compresse les vidéos de plus de 1 Go et préviens-moi quand c'est fini");
        assert!(inc.is_empty(), "{inc:?}");
        assert_eq!(p.iter().map(|e| e.action).collect::<Vec<_>>(), vec![Action::Ranger, Action::CompresserVideos, Action::Notifier]);
        assert_eq!(p[1].nombre, 1024.0);
        let (p, _) = planifier("rappelle-moi dans 20 minutes de sortir le gâteau");
        assert_eq!((p[0].action, p[0].nombre, p[0].texte.as_str()), (Action::Minuteur, 20.0, "sortir le gâteau"));
        let (p, _) = planifier("trouve les pdf reçus sur discord ce mois-ci");
        assert_eq!((p[0].action, p[0].texte.as_str()), (Action::Chercher, "les pdf reçus sur discord ce mois-ci"));
        let (p, _) = planifier("supprime les fichiers de mon bureau pas touchés depuis 6 mois");
        assert_eq!((p[0].action, p[0].nombre), (Action::CorbeilleVieux, 180.0));
        assert!(p[0].action.dangereuse());
        let (p, _) = planifier("supprime les doublons de mes images puis dis-moi quand c'est fini");
        assert_eq!(p.iter().map(|e| e.action).collect::<Vec<_>>(), vec![Action::SupprimerDoublons, Action::Notifier]);
        let (p, _) = planifier("ouvre le planétarium");
        assert_eq!((p[0].action, p[0].texte.as_str()), (Action::OuvrirOutil, "planétarium"));
        let (_, inc) = planifier("écris-moi un poème sur la mer");
        assert_eq!(inc.len(), 1);
        // « photos et vidéos » ne coupe pas la phrase
        assert_eq!(morceaux("range les photos et vidéos et préviens-moi").len(), 2);
        // réponse de l'IA
        let v = serde_json::json!({"etapes": [{"action": "gros_fichiers", "dossier": "", "texte": "", "nombre": 5}, {"action": "inconnue"}]});
        let p = lire_plan(&v);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].action, Action::GrosFichiers);
        assert!(schema()["properties"]["etapes"]["items"]["properties"]["action"]["enum"].as_array().unwrap().len() == 17);
    }

    #[test]
    fn faire_et_annuler() {
        let d = std::env::temp_dir().join(format!("agent_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let vieux = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for n in ["a.jpg", "b.pdf", "c.mp3", "d.zip", "e.xyz"] {
            std::fs::write(d.join(n), b"x").unwrap();
            std::fs::File::options().write(true).open(d.join(n)).unwrap().set_modified(vieux).unwrap();
        }
        let j: Journal = Arc::default();
        let st = AtomicBool::new(false);
        let e = Etape { action: Action::Ranger, dossier: d.display().to_string(), ..Default::default() };
        let r = faire(&e, &j, &st, &|_| {}).unwrap();
        assert!(r.starts_with("5 fichiers"), "{r}");
        assert!(d.join("Images/a.jpg").exists() && d.join("Documents/b.pdf").exists() && d.join("Autres/e.xyz").exists());
        assert_eq!(annuler(&j), 5);
        assert!(d.join("a.jpg").exists() && !d.join("Images").exists());
        // zip
        let e = Etape { action: Action::Zipper, dossier: d.display().to_string(), ..Default::default() };
        faire(&e, &j, &st, &|_| {}).unwrap();
        assert!(d.with_extension("zip").exists());
        let e = Etape { action: Action::EspaceDisque, dossier: d.display().to_string(), ..Default::default() };
        assert!(faire(&e, &j, &st, &|_| {}).unwrap().contains("libre"));
        std::fs::remove_file(d.with_extension("zip")).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
    }
}
