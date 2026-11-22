//! 🔎 Recherche instantanée de fichiers sur tout le PC (méthode d'Everything).
//!
//! * Mode ⚡ (administrateur, disques NTFS) : la table des fichiers (MFT) est lue d'un bloc avec
//!   `FSCTL_ENUM_USN_DATA` (quelques secondes pour un million de fichiers), puis le journal USN
//!   tient l'index à jour en temps réel, même pour ce qui a changé pendant qu'Outils était fermé.
//! * Mode normal (sans droits, clés USB, FAT/exFAT) : parcours des dossiers sur tous les cœurs avec
//!   `FindFirstFileExW` (FindExInfoBasic + LARGE_FETCH), puis surveillance avec `ReadDirectoryChangesW`.
//! * L'index est compact (tous les noms bout à bout, tableaux simples) et enregistré sur le disque :
//!   au lancement suivant, la recherche est prête tout de suite.
//! * Chaque frappe parcourt tout l'index sur tous les cœurs (quelques millisecondes), avec un classement
//!   intelligent, des filtres (ext:, type:, taille:, date:, dossier:, !exclure, jokers *) et une
//!   recherche floue si rien ne correspond.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, Key, RichText};
use memchr::memmem;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

pub const NOM: &str = "🔎 Recherche de fichiers";

pub const DOSSIER: u8 = 1;
pub const ATTR_CACHE: u8 = 2; // caché ou système (lui-même ou un dossier parent)
pub const SYSTEME: u8 = 4; // sous Windows, Program Files, AppData…
pub const PERSO: u8 = 8; // sous Bureau, Documents, Téléchargements…
pub const CACHE_PROPRE: u8 = 16; // attribut caché/système du fichier lui-même
pub const SUPPRIME: u8 = 128;
pub const INCONNU: u64 = u64::MAX;
pub const AUCUN: u32 = u32::MAX;

const SYS_RACINE: [&[u8]; 12] = [
    b"windows", b"program files", b"program files (x86)", b"programdata", b"$recycle.bin", b"system volume information",
    b"recovery", b"perflogs", b"$windows.~bt", b"$winreagent", b"windows.old", b"msocache",
];
const PERSO_NOMS: [&[u8]; 11] = [b"desktop", b"bureau", b"documents", b"downloads", b"telechargements", b"pictures", b"images", b"videos", b"music", b"musique", b"onedrive"];
/// Dossiers qu'on ne parcourt pas (inutiles et énormes).
const IGNORES: [&[u8]; 3] = [b"$recycle.bin", b"system volume information", b"winsxs"];

/// Minuscules sans accents : « Été » et « ete » se trouvent pareil.
pub fn normaliser(s: &str, out: &mut Vec<u8>) {
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase() as u8);
            continue;
        }
        let r = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "a",
            'ç' | 'Ç' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => "e",
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => "i",
            'ñ' | 'Ñ' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => "o",
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => "u",
            'ý' | 'ÿ' | 'Ý' => "y",
            'œ' | 'Œ' => "oe",
            'æ' | 'Æ' => "ae",
            'ß' => "ss",
            _ => {
                let mut b = [0; 4];
                for l in c.to_lowercase() {
                    out.extend_from_slice(l.encode_utf8(&mut b).as_bytes());
                }
                continue;
            }
        };
        out.extend_from_slice(r.as_bytes());
    }
}

pub fn norm(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len());
    normaliser(s, &mut v);
    v
}

/// Secondes Unix depuis un FILETIME Windows (centaines de ns depuis 1601).
pub fn depuis_filetime(ft: u64) -> u32 {
    (ft / 10_000_000).saturating_sub(11_644_473_600).min(u32::MAX as u64) as u32
}

// ===================================================================================================
// L'index
// ===================================================================================================

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Volume {
    pub racine: String, // « C: »
    pub idx: u32,
    pub mft: bool,
    pub journal: u64,
    pub usn: i64,
}

#[derive(Default)]
pub struct Index {
    noms: String,
    norm: Vec<u8>,
    nom: Vec<(u32, u16)>,
    nrm: Vec<(u32, u16)>,
    pub parent: Vec<u32>,
    pub taille: Vec<u64>,
    pub date: Vec<u32>,
    pub drap: Vec<u8>,
    pub prof: Vec<u8>,
    pub cle: Vec<u64>,
    par_cle: HashMap<u64, u32>,
    pub volumes: Vec<Volume>,
    pub vivants: usize,
}

impl Index {
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn nom(&self, i: u32) -> &str {
        let (o, l) = self.nom[i as usize];
        &self.noms[o as usize..o as usize + l as usize]
    }

    pub fn nrm(&self, i: u32) -> &[u8] {
        let (o, l) = self.nrm[i as usize];
        &self.norm[o as usize..o as usize + l as usize]
    }

    pub fn vivant(&self, i: u32) -> bool {
        self.drap[i as usize] & SUPPRIME == 0
    }

    /// Vivant, et tous ses dossiers parents aussi.
    pub fn vivant_complet(&self, i: u32) -> bool {
        let mut x = i;
        for _ in 0..300 {
            if x == AUCUN {
                return true;
            }
            if !self.vivant(x) {
                return false;
            }
            x = self.parent[x as usize];
        }
        false
    }

    pub fn par_cle(&self, c: u64) -> Option<u32> {
        self.par_cle.get(&c).copied()
    }

    fn poser_nom(&mut self, i: usize, nom: &str) {
        let nom = &nom[..nom.len().min(u16::MAX as usize / 4)];
        let a = (self.noms.len() as u32, nom.len() as u16);
        self.noms.push_str(nom);
        let d = self.norm.len();
        normaliser(nom, &mut self.norm);
        let b = (d as u32, (self.norm.len() - d) as u16);
        if i == self.nom.len() {
            self.nom.push(a);
            self.nrm.push(b);
        } else {
            self.nom[i] = a;
            self.nrm[i] = b;
        }
    }

    /// Drapeaux calculés d'après le dossier parent (qui doit être à jour).
    fn calcul(&self, parent: u32, nrm: &[u8], prof: u8, propres: u8) -> u8 {
        let mut d = propres & (DOSSIER | CACHE_PROPRE | SUPPRIME);
        if propres & CACHE_PROPRE != 0 {
            d |= ATTR_CACHE;
        }
        if parent == AUCUN || parent as usize >= self.len() {
            return d;
        }
        let p = parent as usize;
        d |= self.drap[p] & (SYSTEME | PERSO | ATTR_CACHE);
        if prof == 1 && SYS_RACINE.contains(&nrm) {
            d |= SYSTEME;
        }
        if prof == 3 {
            let gp = self.parent[p];
            if gp != AUCUN && matches!(self.nrm(gp), b"users" | b"utilisateurs") {
                if nrm == b"appdata" {
                    d |= SYSTEME;
                } else if PERSO_NOMS.contains(&nrm) {
                    d |= PERSO;
                }
            }
        }
        d
    }

    /// Ajoute une entrée dont le parent est déjà dans l'index (parcours des dossiers).
    pub fn ajouter(&mut self, nom: &str, parent: u32, taille: u64, date: u32, propres: u8, cle: u64) -> u32 {
        let i = self.len();
        self.poser_nom(i, nom);
        let prof = if parent == AUCUN { 0 } else { self.prof[parent as usize].saturating_add(1) };
        let d = self.calcul(parent, self.nrm(i as u32), prof, propres);
        self.parent.push(parent);
        self.taille.push(taille);
        self.date.push(date);
        self.drap.push(d);
        self.prof.push(prof);
        self.cle.push(cle);
        if cle != 0 {
            self.par_cle.insert(cle, i as u32);
        }
        if d & SUPPRIME == 0 {
            self.vivants += 1;
        }
        i as u32
    }

    /// Ajoute sans calculer les drapeaux (parents pas encore connus) : appeler `recalculer` ensuite.
    pub fn pousser_brut(&mut self, nom: &str, parent: u32, taille: u64, date: u32, propres: u8, cle: u64) -> u32 {
        let i = self.len();
        self.poser_nom(i, nom);
        self.parent.push(parent);
        self.taille.push(taille);
        self.date.push(date);
        self.drap.push(propres);
        self.prof.push(0);
        self.cle.push(cle);
        if cle != 0 {
            self.par_cle.insert(cle, i as u32);
        }
        i as u32
    }

    /// Profondeurs et drapeaux de toutes les entrées, dans n'importe quel ordre d'arrivée.
    pub fn recalculer(&mut self) {
        let n = self.len();
        let mut prof = vec![u8::MAX; n];
        let mut pile = vec![];
        for i in 0..n {
            let mut x = i;
            while prof[x] == u8::MAX && pile.len() < 250 {
                let p = self.parent[x];
                if p == AUCUN || p as usize >= n {
                    prof[x] = 0;
                    break;
                }
                pile.push(x);
                x = p as usize;
            }
            let mut b = if prof[x] == u8::MAX { 0 } else { prof[x] };
            if prof[x] == u8::MAX {
                prof[x] = 0;
            }
            while let Some(y) = pile.pop() {
                b = b.saturating_add(1);
                if prof[y] == u8::MAX {
                    prof[y] = b;
                }
            }
        }
        // ordre par profondeur : les parents avant les enfants
        let mut paquets: Vec<Vec<u32>> = vec![vec![]; 256];
        for (i, p) in prof.iter().enumerate() {
            paquets[*p as usize].push(i as u32);
        }
        self.prof = prof;
        self.vivants = 0;
        for i in paquets.into_iter().flatten() {
            let k = i as usize;
            let d = self.calcul(self.parent[k], self.nrm(i), self.prof[k], self.drap[k]);
            self.drap[k] = d;
            if d & SUPPRIME == 0 {
                self.vivants += 1;
            }
        }
    }

    pub fn supprimer(&mut self, i: u32) {
        let k = i as usize;
        if self.drap[k] & SUPPRIME == 0 {
            self.drap[k] |= SUPPRIME;
            self.vivants = self.vivants.saturating_sub(1);
        }
        if self.cle[k] != 0 {
            self.par_cle.remove(&self.cle[k]);
        }
    }

    /// Renomme et/ou déplace une entrée.
    pub fn renommer(&mut self, i: u32, nom: &str, parent: u32) {
        let k = i as usize;
        if self.nom(i) != nom {
            self.poser_nom(k, nom);
        }
        self.parent[k] = parent;
        self.prof[k] = if parent == AUCUN { 0 } else { self.prof[parent as usize].saturating_add(1) };
        self.drap[k] = self.calcul(parent, self.nrm(i), self.prof[k], self.drap[k]);
    }

    pub fn chemin(&self, i: u32) -> String {
        let mut v = vec![];
        let mut x = i;
        while x != AUCUN && v.len() < 300 {
            v.push(self.nom(x));
            x = self.parent[x as usize];
        }
        v.reverse();
        let c = v.join("\\");
        if v.len() == 1 { format!("{c}\\") } else { c }
    }

    pub fn racine(&self, lettre: &str) -> Option<u32> {
        self.volumes.iter().find(|v| v.racine.eq_ignore_ascii_case(lettre)).map(|v| v.idx)
    }

    /// Entrée d'un chemin complet (« C:\Users\Moi\Bureau »).
    pub fn trouver_chemin(&self, chemin: &str) -> Option<u32> {
        let mut morceaux = chemin.trim_end_matches('\\').split('\\');
        let mut cur = self.racine(morceaux.next()?)?;
        for m in morceaux.filter(|m| !m.is_empty()) {
            let n = norm(m);
            cur = (0..self.len() as u32).find(|&i| self.parent[i as usize] == cur && self.vivant(i) && self.nrm(i) == n.as_slice())?;
        }
        Some(cur)
    }

    pub fn enfants(&self, d: u32) -> Vec<u32> {
        (0..self.len() as u32).filter(|&i| self.parent[i as usize] == d && self.vivant(i)).collect()
    }

    // ---- enregistrement sur le disque ----

    pub fn ecrire(&self) -> Vec<u8> {
        let n = self.len();
        let garder: Vec<bool> = (0..n as u32).map(|i| self.vivant_complet(i)).collect();
        let mut nouv = vec![AUCUN; n];
        let mut k = 0u32;
        for i in 0..n {
            if garder[i] {
                nouv[i] = k;
                k += 1;
            }
        }
        let mut b = Vec::with_capacity(n * 40);
        b.extend_from_slice(b"OIDX");
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&(self.volumes.len() as u32).to_le_bytes());
        for v in &self.volumes {
            b.extend_from_slice(&(v.racine.len() as u16).to_le_bytes());
            b.extend_from_slice(v.racine.as_bytes());
            b.push(v.mft as u8);
            b.extend_from_slice(&v.journal.to_le_bytes());
            b.extend_from_slice(&v.usn.to_le_bytes());
            b.extend_from_slice(&nouv.get(v.idx as usize).copied().unwrap_or(AUCUN).to_le_bytes());
        }
        b.extend_from_slice(&k.to_le_bytes());
        for i in 0..n {
            if !garder[i] {
                continue;
            }
            let p = self.parent[i];
            b.extend_from_slice(&(if p == AUCUN { AUCUN } else { nouv[p as usize] }).to_le_bytes());
            b.push(self.drap[i] & (DOSSIER | CACHE_PROPRE));
            b.extend_from_slice(&self.taille[i].to_le_bytes());
            b.extend_from_slice(&self.date[i].to_le_bytes());
            b.extend_from_slice(&self.cle[i].to_le_bytes());
            let nm = self.nom(i as u32);
            b.extend_from_slice(&(nm.len() as u16).to_le_bytes());
            b.extend_from_slice(nm.as_bytes());
        }
        b
    }

    pub fn lire(b: &[u8]) -> Option<Index> {
        let mut p = 0usize;
        let mut prendre = |n: usize| -> Option<&[u8]> {
            let s = b.get(p..p + n)?;
            p += n;
            Some(s)
        };
        if prendre(4)? != b"OIDX" || u32::from_le_bytes(prendre(4)?.try_into().ok()?) != 1 {
            return None;
        }
        let u32_ = |s: &[u8]| u32::from_le_bytes(s.try_into().unwrap());
        let u64_ = |s: &[u8]| u64::from_le_bytes(s.try_into().unwrap());
        let nv = u32_(prendre(4)?);
        let mut ix = Index::default();
        for _ in 0..nv.min(64) {
            let l = u16::from_le_bytes(prendre(2)?.try_into().ok()?) as usize;
            let racine = String::from_utf8(prendre(l)?.to_vec()).ok()?;
            let mft = prendre(1)?[0] != 0;
            let journal = u64_(prendre(8)?);
            let usn = u64_(prendre(8)?) as i64;
            let idx = u32_(prendre(4)?);
            ix.volumes.push(Volume { racine, idx, mft, journal, usn });
        }
        let n = u32_(prendre(4)?) as usize;
        for _ in 0..n {
            let parent = u32_(prendre(4)?);
            let drap = prendre(1)?[0];
            let taille = u64_(prendre(8)?);
            let date = u32_(prendre(4)?);
            let cle = u64_(prendre(8)?);
            let l = u16::from_le_bytes(prendre(2)?.try_into().ok()?) as usize;
            let nom = std::str::from_utf8(prendre(l)?).ok()?;
            if parent != AUCUN && parent as usize >= n {
                return None;
            }
            ix.pousser_brut(nom, parent, taille, date, drap, cle);
        }
        ix.recalculer();
        Some(ix)
    }
}

// ===================================================================================================
// Requête et recherche
// ===================================================================================================

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Requete {
    pub termes: Vec<Vec<u8>>,
    pub exclus: Vec<Vec<u8>>,
    pub ext: Vec<Vec<u8>>,
    pub dossier: Vec<Vec<u8>>,
    pub taille_min: Option<u64>,
    pub taille_max: Option<u64>,
    pub date_min: Option<u32>,
    pub date_max: Option<u32>,
    pub seulement: Option<bool>, // Some(true) = dossiers, Some(false) = fichiers
}

impl Requete {
    pub fn vide(&self) -> bool {
        self.termes.is_empty() && self.ext.is_empty() && self.dossier.is_empty() && self.taille_min.is_none() && self.taille_max.is_none() && self.date_min.is_none() && self.date_max.is_none() && self.seulement.is_none()
    }
}

fn decouper(q: &str) -> Vec<String> {
    let mut v = vec![];
    let mut cur = String::new();
    let mut guillemets = false;
    for c in q.chars() {
        match c {
            '"' => guillemets = !guillemets,
            c if c.is_whitespace() && !guillemets => {
                if !cur.is_empty() {
                    v.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        v.push(cur);
    }
    v
}

/// « 10mo », « 1,5 go », « 300k » → octets.
pub fn lire_taille(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase().replace(',', ".");
    let fin = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let n: f64 = s[..fin].parse().ok()?;
    let m: f64 = match s[fin..].trim() {
        "" | "o" | "b" => 1.0,
        "k" | "ko" | "kb" | "kio" => 1024.0,
        "m" | "mo" | "mb" | "mio" => 1024.0 * 1024.0,
        "g" | "go" | "gb" | "gio" => 1024.0 * 1024.0 * 1024.0,
        "t" | "to" | "tb" | "tio" => 1024f64.powi(4),
        _ => return None,
    };
    Some((n * m) as u64)
}

fn lire_date(s: &str, maintenant: u32) -> Option<(u32, u32)> {
    let jour = maintenant - maintenant % 86_400;
    let fin = u32::MAX;
    Some(match s {
        "aujourdhui" | "aujourd'hui" | "today" => (jour, fin),
        "hier" | "yesterday" => (jour.saturating_sub(86_400), jour),
        "semaine" | "week" => (maintenant.saturating_sub(7 * 86_400), fin),
        "mois" | "month" => (maintenant.saturating_sub(30 * 86_400), fin),
        "annee" | "année" | "an" | "year" => (maintenant.saturating_sub(365 * 86_400), fin),
        _ => {
            let p: Vec<i64> = s.split(['-', '/']).map(|x| x.parse().ok()).collect::<Option<_>>()?;
            let j = |a: i64, m: i64, d: i64| (crate::quotidien2::jours_depuis_1970(a as i32, m as u32, d as u32) * 86_400).max(0) as u32;
            match p.as_slice() {
                [a] if *a > 1970 => (j(*a, 1, 1), j(a + 1, 1, 1)),
                [a, m] if (1..=12).contains(m) => (j(*a, *m, 1), if *m == 12 { j(a + 1, 1, 1) } else { j(*a, m + 1, 1) }),
                [a, m, d] => (j(*a, *m, *d), j(*a, *m, *d) + 86_400),
                _ => return None,
            }
        }
    })
}

fn familles(t: &str) -> Option<&'static [&'static str]> {
    Some(match t {
        "image" | "images" | "photo" | "photos" => &["jpg", "jpeg", "png", "gif", "webp", "bmp", "heic", "tif", "tiff", "svg", "ico", "raw", "cr2", "nef", "avif"],
        "video" | "videos" | "film" => &["mp4", "mkv", "avi", "mov", "wmv", "webm", "flv", "m4v", "mpg", "mpeg", "ts"],
        "audio" | "son" | "musique" | "music" => &["mp3", "wav", "flac", "ogg", "m4a", "aac", "wma", "opus"],
        "doc" | "docs" | "document" | "documents" => &["pdf", "doc", "docx", "odt", "txt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp", "md", "csv"],
        "archive" | "archives" | "zip" => &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso"],
        "exe" | "programme" | "programmes" | "appli" => &["exe", "msi", "bat", "cmd", "lnk", "appx", "msix"],
        "code" => &["rs", "py", "js", "ts", "c", "cpp", "h", "java", "cs", "html", "css", "json", "go", "lua", "php"],
        "minecraft" | "mc" => &["jar", "mcworld", "mcpack", "mcaddon", "schem", "schematic", "litematic", "nbt", "mca"],
        _ => return None,
    })
}

pub fn analyser(q: &str, maintenant: u32) -> Requete {
    let mut r = Requete::default();
    for t in decouper(q) {
        let bas = t.to_lowercase();
        let (cle, val) = match bas.split_once(':') {
            Some((c, v)) if !c.is_empty() && c.len() <= 8 && c.chars().all(|x| x.is_alphabetic()) => (c.to_string(), v.to_string()),
            _ => (String::new(), bas.clone()),
        };
        match cle.as_str() {
            "ext" => r.ext.extend(val.split([';', ',', '|']).map(|e| norm(e.trim_start_matches('.'))).filter(|e| !e.is_empty())),
            "type" => match val.as_str() {
                "dossier" | "dossiers" | "folder" | "rep" => r.seulement = Some(true),
                "fichier" | "fichiers" | "file" => r.seulement = Some(false),
                v => {
                    if let Some(f) = familles(v) {
                        r.ext.extend(f.iter().map(|e| e.as_bytes().to_vec()));
                    }
                }
            },
            "taille" | "size" | "poids" => {
                let v = val.as_str();
                if let Some(x) = v.strip_prefix(">=").or(v.strip_prefix('>')) {
                    r.taille_min = lire_taille(x);
                } else if let Some(x) = v.strip_prefix("<=").or(v.strip_prefix('<')) {
                    r.taille_max = lire_taille(x);
                } else if let Some((a, b)) = v.split_once("..").or(v.split_once('-')) {
                    r.taille_min = lire_taille(a);
                    r.taille_max = lire_taille(b);
                } else {
                    r.taille_min = lire_taille(v);
                }
            }
            "date" | "dm" | "modif" => {
                let v = val.as_str();
                if let Some(x) = v.strip_prefix('>') {
                    r.date_min = lire_date(x, maintenant).map(|d| d.0);
                } else if let Some(x) = v.strip_prefix('<') {
                    r.date_max = lire_date(x, maintenant).map(|d| d.0);
                } else if let Some((a, b)) = lire_date(v, maintenant) {
                    r.date_min = Some(a);
                    r.date_max = (b != u32::MAX).then_some(b);
                }
            }
            "dossier" | "in" | "dans" => r.dossier.push(norm(&val)),
            _ => {
                if let Some(x) = t.strip_prefix('!').filter(|x| !x.is_empty()) {
                    r.exclus.push(norm(x));
                } else if t.contains(['\\', '/']) && !t.ends_with(['\\', '/']) {
                    // « minecraft\\saves » : le dernier morceau dans le nom, le reste dans le dossier
                    let (d, n) = t.rsplit_once(['\\', '/']).unwrap();
                    if !d.is_empty() {
                        r.dossier.push(norm(d));
                    }
                    r.termes.push(norm(n));
                } else {
                    let n = norm(&t);
                    if !n.is_empty() {
                        r.termes.push(n);
                    }
                }
            }
        }
    }
    r
}

pub fn a_joker(t: &[u8]) -> bool {
    t.contains(&b'*') || t.contains(&b'?')
}

/// Correspondance avec jokers sur le nom entier (« *.mp4 », « photo??.jpg »).
pub fn joker(nom: &[u8], m: &[u8]) -> bool {
    let (mut n, mut p) = (0, 0);
    let (mut etoile, mut retour) = (None, 0);
    while n < nom.len() {
        if p < m.len() && (m[p] == b'?' || m[p] == nom[n]) {
            n += 1;
            p += 1;
        } else if p < m.len() && m[p] == b'*' {
            etoile = Some(p);
            retour = n;
            p += 1;
        } else if let Some(e) = etoile {
            p = e + 1;
            retour += 1;
            n = retour;
        } else {
            return false;
        }
    }
    while p < m.len() && m[p] == b'*' {
        p += 1;
    }
    p == m.len()
}

fn debut_de_mot(nom: &[u8], t: &[u8]) -> bool {
    memmem::find_iter(nom, t).any(|p| p == 0 || !nom[p - 1].is_ascii_alphanumeric())
}

/// Note d'un résultat : nom exact > début du nom > début d'un mot > au milieu ; dossiers perso devant, système derrière.
pub fn score(nom: &[u8], t0: &[u8], drap: u8, prof: u8) -> i32 {
    let base = match nom.iter().rposition(|c| *c == b'.') {
        Some(p) if p > 0 => &nom[..p],
        _ => nom,
    };
    let mut s = if t0.is_empty() || a_joker(t0) {
        300
    } else if nom == t0 || base == t0 {
        1000
    } else if nom.starts_with(t0) {
        600
    } else if debut_de_mot(nom, t0) {
        400
    } else {
        200
    };
    s -= prof as i32 * 4 + nom.len() as i32 / 3;
    if drap & SYSTEME != 0 {
        s -= 500;
    }
    if drap & ATTR_CACHE != 0 {
        s -= 300;
    }
    if drap & PERSO != 0 {
        s += 150;
    }
    s
}

/// Recherche floue : les lettres dans l'ordre (« raprot » → « rapport »). Plus c'est serré, mieux c'est.
pub fn flou(nom: &[u8], t: &[u8]) -> Option<i32> {
    // tolère une inversion de deux lettres voisines en essayant aussi le motif corrigé
    let essai = |t: &[u8]| -> Option<i32> {
        let mut p = 0;
        let mut debut = None;
        let mut trous = 0;
        let mut precedent = 0;
        for (k, c) in nom.iter().enumerate() {
            if p < t.len() && *c == t[p] {
                if debut.is_none() {
                    debut = Some(k);
                } else {
                    trous += k - precedent - 1;
                }
                precedent = k;
                p += 1;
            }
        }
        (p == t.len()).then(|| 100 - trous as i32 * 4 - debut.unwrap_or(0) as i32)
    };
    let mut best = essai(t);
    for k in 0..t.len().saturating_sub(1) {
        let mut u = t.to_vec();
        u.swap(k, k + 1);
        if let Some(s) = essai(&u) {
            best = Some(best.map_or(s - 10, |b| b.max(s - 10)));
        }
    }
    best
}

#[derive(Default, Clone, Debug)]
pub struct Resultat {
    pub liste: Vec<u32>,
    pub total: usize,
    pub flou: bool,
}

fn termine_par_ext(nom: &[u8], ext: &[Vec<u8>]) -> bool {
    ext.iter().any(|e| nom.len() > e.len() && nom.ends_with(e) && nom[nom.len() - e.len() - 1] == b'.')
}

/// Cherche dans tout l'index, sur tous les cœurs. `verifier` donne (taille, date) d'un chemin pour les
/// entrées dont on ne les connaît pas encore (mode MFT).
pub fn chercher(ix: &Index, r: &Requete, max: usize, masquer: bool, verifier: &(dyn Fn(&str) -> Option<(u64, u32)> + Sync)) -> Resultat {
    if r.vide() {
        return Resultat::default();
    }
    let n = ix.len();
    let fils = std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4).clamp(1, 16);
    let part = n.div_ceil(fils).max(4096);
    let simples: Vec<memmem::Finder> = r.termes.iter().filter(|t| !a_joker(t)).map(|t| memmem::Finder::new(t)).collect();
    let jokers: Vec<&Vec<u8>> = r.termes.iter().filter(|t| a_joker(t)).collect();
    let exclus: Vec<memmem::Finder> = r.exclus.iter().map(|t| memmem::Finder::new(t)).collect();
    let t0: &[u8] = r.termes.first().map(|t| t.as_slice()).unwrap_or(b"");
    let besoin_taille = r.taille_min.is_some() || r.taille_max.is_some();
    let besoin_date = r.date_min.is_some() || r.date_max.is_some();
    // (note, index, à vérifier)
    let mut trouves: Vec<(i32, u32, bool)> = std::thread::scope(|s| {
        let mut h = vec![];
        for k in 0..n.div_ceil(part) {
            let (simples, jokers, exclus) = (&simples, &jokers, &exclus);
            h.push(s.spawn(move || {
                let mut v = vec![];
                for i in k * part..((k + 1) * part).min(n) {
                    let d = ix.drap[i];
                    if d & SUPPRIME != 0 || (masquer && d & (SYSTEME | ATTR_CACHE) != 0) || ix.parent[i] == AUCUN {
                        continue;
                    }
                    let nom = ix.nrm(i as u32);
                    if !simples.iter().all(|f| f.find(nom).is_some()) || !jokers.iter().all(|j| joker(nom, j)) || exclus.iter().any(|f| f.find(nom).is_some()) {
                        continue;
                    }
                    if let Some(dos) = r.seulement {
                        if (d & DOSSIER != 0) != dos {
                            continue;
                        }
                    }
                    if !r.ext.is_empty() && (d & DOSSIER != 0 || !termine_par_ext(nom, &r.ext)) {
                        continue;
                    }
                    let mut a_verifier = false;
                    if besoin_taille {
                        let t = ix.taille[i];
                        if d & DOSSIER != 0 {
                            continue;
                        }
                        if t == INCONNU {
                            a_verifier = true;
                        } else if r.taille_min.is_some_and(|m| t < m) || r.taille_max.is_some_and(|m| t > m) {
                            continue;
                        }
                    }
                    if besoin_date {
                        let t = ix.date[i];
                        if t == 0 {
                            a_verifier = true;
                        } else if r.date_min.is_some_and(|m| t < m) || r.date_max.is_some_and(|m| t >= m) {
                            continue;
                        }
                    }
                    if !ix.vivant_complet(i as u32) {
                        continue;
                    }
                    if !r.dossier.is_empty() {
                        let c = norm(&ix.chemin(ix.parent[i]));
                        if !r.dossier.iter().all(|x| memmem::find(&c, x).is_some()) {
                            continue;
                        }
                    }
                    v.push((score(nom, t0, d, ix.prof[i]), i as u32, a_verifier));
                }
                v
            }));
        }
        h.into_iter().flat_map(|x| x.join().unwrap_or_default()).collect()
    });
    let mut est_flou = false;
    if trouves.is_empty() && r.termes.len() == 1 && !a_joker(t0) && t0.len() >= 3 {
        est_flou = true;
        trouves = std::thread::scope(|s| {
            let mut h = vec![];
            for k in 0..n.div_ceil(part) {
                h.push(s.spawn(move || {
                    let mut v = vec![];
                    for i in k * part..((k + 1) * part).min(n) {
                        let d = ix.drap[i];
                        if d & SUPPRIME != 0 || (masquer && d & (SYSTEME | ATTR_CACHE) != 0) || ix.parent[i] == AUCUN {
                            continue;
                        }
                        if let Some(f) = flou(ix.nrm(i as u32), t0) {
                            if f > 40 && ix.vivant_complet(i as u32) {
                                v.push((f * 4 + score(b"", b"", d, ix.prof[i]) - 300, i as u32, false));
                            }
                        }
                    }
                    v
                }));
            }
            h.into_iter().flat_map(|x| x.join().unwrap_or_default()).collect()
        });
    }
    trouves.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(ix.nom[a.1 as usize].1.cmp(&ix.nom[b.1 as usize].1)));
    // vérification des tailles et dates inconnues (mode MFT), dans l'ordre du classement
    let mut verifies = 0;
    let mut liste = Vec::with_capacity(max.min(trouves.len()));
    let mut total = 0;
    for (_, i, a_verifier) in trouves {
        if a_verifier {
            if verifies >= 3000 {
                continue;
            }
            verifies += 1;
            let Some((t, d)) = verifier(&ix.chemin(i)) else { continue };
            if r.taille_min.is_some_and(|m| t < m) || r.taille_max.is_some_and(|m| t > m) || r.date_min.is_some_and(|m| d < m) || r.date_max.is_some_and(|m| d >= m) {
                continue;
            }
        }
        total += 1;
        if liste.len() < max {
            liste.push(i);
        }
    }
    Resultat { liste, total, flou: est_flou }
}

// ===================================================================================================
// Journal USN : lecture des enregistrements
// ===================================================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct Usn {
    pub frn: u64,
    pub parent: u64,
    pub usn: i64,
    pub date: u32,
    pub raison: u32,
    pub attrs: u32,
    pub nom: String,
}

pub const USN_CREATION: u32 = 0x100;
pub const USN_SUPPRESSION: u32 = 0x200;
pub const USN_RENOMME_ANCIEN: u32 = 0x1000;
pub const USN_RENOMME_NOUVEAU: u32 = 0x2000;
const MASQUE_48: u64 = 0xFFFF_FFFF_FFFF;

/// Enregistrements USN_RECORD_V2 à la suite (après les 8 premiers octets de la réponse).
pub fn lire_usn(b: &[u8]) -> Vec<Usn> {
    let mut v = vec![];
    let mut p = 0usize;
    while p + 60 <= b.len() {
        let u32_ = |o: usize| u32::from_le_bytes(b[p + o..p + o + 4].try_into().unwrap());
        let u64_ = |o: usize| u64::from_le_bytes(b[p + o..p + o + 8].try_into().unwrap());
        let long = u32_(0) as usize;
        if long < 60 || p + long > b.len() {
            break;
        }
        let majeure = u16::from_le_bytes([b[p + 4], b[p + 5]]);
        if majeure == 2 {
            let nl = u16::from_le_bytes([b[p + 56], b[p + 57]]) as usize;
            let no = u16::from_le_bytes([b[p + 58], b[p + 59]]) as usize;
            if no + nl <= long {
                let u: Vec<u16> = b[p + no..p + no + nl].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                v.push(Usn { frn: u64_(8) & MASQUE_48, parent: u64_(16) & MASQUE_48, usn: u64_(24) as i64, date: depuis_filetime(u64_(32)), raison: u32_(40), attrs: u32_(52), nom: String::from_utf16_lossy(&u) });
            }
        }
        p += long;
    }
    v
}

pub fn cle(vol: usize, frn: u64) -> u64 {
    ((vol as u64 + 1) << 48) | (frn & MASQUE_48)
}

/// Applique un changement du journal à l'index. Renvoie vrai si l'index a changé.
pub fn appliquer_usn(ix: &mut Index, vol: usize, u: &Usn) -> bool {
    let c = cle(vol, u.frn);
    if u.raison & USN_SUPPRESSION != 0 {
        if let Some(i) = ix.par_cle(c) {
            ix.supprimer(i);
            return true;
        }
        return false;
    }
    if u.raison & (USN_CREATION | USN_RENOMME_NOUVEAU) == 0 {
        return false;
    }
    let Some(p) = ix.par_cle(cle(vol, u.parent)) else { return false };
    let propres = if u.attrs & 0x10 != 0 { DOSSIER } else { 0 } | if u.attrs & 0x6 != 0 { CACHE_PROPRE } else { 0 };
    match ix.par_cle(c) {
        Some(i) => {
            ix.renommer(i, &u.nom, p);
            ix.date[i as usize] = u.date;
        }
        None => {
            ix.ajouter(&u.nom, p, INCONNU, u.date, propres, c);
        }
    }
    true
}

// ===================================================================================================
// Windows : MFT, parcours, surveillance
// ===================================================================================================

#[cfg(windows)]
mod win {
    use super::*;
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::IO::DeviceIoControl;

    const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00B3;
    const FSCTL_READ_USN_JOURNAL: u32 = 0x0009_00BB;
    const FSCTL_CREATE_USN_JOURNAL: u32 = 0x0009_00E7;
    const FSCTL_QUERY_USN_JOURNAL: u32 = 0x0009_00F4;

    pub fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Poignee(HANDLE);
    impl Drop for Poignee {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }
    unsafe impl Send for Poignee {}

    /// Lecteurs fixes et amovibles : (« C: », NTFS ?).
    pub fn lecteurs() -> Vec<(String, bool)> {
        let masque = unsafe { GetLogicalDrives() };
        let mut v = vec![];
        for k in 0..26 {
            if masque & (1 << k) == 0 {
                continue;
            }
            let racine = format!("{}:\\", (b'A' + k as u8) as char);
            let t = unsafe { GetDriveTypeW(w(&racine).as_ptr()) };
            if t != 2 && t != 3 {
                continue;
            }
            let mut fs = [0u16; 32];
            let ok = unsafe { GetVolumeInformationW(w(&racine).as_ptr(), null_mut(), 0, null_mut(), null_mut(), null_mut(), fs.as_mut_ptr(), 32) } != 0;
            if !ok {
                continue; // lecteur vide (carte SD absente…)
            }
            let nom = String::from_utf16_lossy(&fs[..fs.iter().position(|c| *c == 0).unwrap_or(0)]);
            v.push((racine[..2].to_string(), nom == "NTFS"));
        }
        v
    }

    fn ouvrir_volume(lettre: &str) -> Option<Poignee> {
        let h = unsafe { CreateFileW(w(&format!("\\\\.\\{lettre}")).as_ptr(), 0x8000_0000, FILE_SHARE_READ | FILE_SHARE_WRITE, null(), OPEN_EXISTING, 0, null_mut()) };
        (h != INVALID_HANDLE_VALUE).then_some(Poignee(h))
    }

    /// (identifiant du journal, premier USN valide, prochain USN)
    fn journal(h: &Poignee) -> Option<(u64, i64, i64)> {
        let mut d = [0u64; 16];
        let mut r = 0u32;
        let mut ok = unsafe { DeviceIoControl(h.0, FSCTL_QUERY_USN_JOURNAL, null(), 0, d.as_mut_ptr() as *mut c_void, 128, &mut r, null_mut()) } != 0;
        if !ok {
            // pas de journal sur ce disque : on en crée un (comme Everything)
            let c = [32u64 << 20, 8 << 20];
            unsafe { DeviceIoControl(h.0, FSCTL_CREATE_USN_JOURNAL, c.as_ptr() as *const c_void, 16, null_mut(), 0, &mut r, null_mut()) };
            ok = unsafe { DeviceIoControl(h.0, FSCTL_QUERY_USN_JOURNAL, null(), 0, d.as_mut_ptr() as *mut c_void, 128, &mut r, null_mut()) } != 0;
        }
        // USN_JOURNAL_DATA_V0 : id, FirstUsn, NextUsn, LowestValidUsn, MaxUsn…
        ok.then_some((d[0], d[3] as i64, d[2] as i64))
    }

    /// Lit toute la table des fichiers d'un volume NTFS. Demande les droits administrateur.
    pub fn mft(ix: &mut Index, vol: usize, lettre: &str, compte: &AtomicUsize) -> Result<(u64, i64), String> {
        let h = ouvrir_volume(lettre).ok_or("accès au disque refusé (droits administrateur nécessaires)")?;
        let (id, _, prochain) = journal(&h).ok_or("journal USN indisponible")?;
        let racine = ix.pousser_brut(lettre, AUCUN, INCONNU, 0, DOSSIER, cle(vol, 5));
        let debut = ix.len();
        let mut parents: Vec<u64> = vec![];
        let mut entree = [0u64; 3]; // MFT_ENUM_DATA_V0 : StartFileReferenceNumber, LowUsn, HighUsn
        entree[2] = prochain as u64;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let mut r = 0u32;
            let ok = unsafe { DeviceIoControl(h.0, FSCTL_ENUM_USN_DATA, entree.as_ptr() as *const c_void, 24, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut r, null_mut()) } != 0;
            if !ok || r <= 8 {
                let e = unsafe { GetLastError() };
                if !ok && e != 38 {
                    return Err(format!("lecture de la table des fichiers impossible (erreur {e})"));
                }
                break;
            }
            entree[0] = u64::from_le_bytes(buf[..8].try_into().unwrap());
            for u in lire_usn(&buf[8..r as usize]) {
                if u.frn < 16 || u.frn == 5 {
                    continue; // fichiers internes de NTFS ($MFT, $Bitmap…)
                }
                let propres = if u.attrs & 0x10 != 0 { DOSSIER } else { 0 } | if u.attrs & 0x6 != 0 { CACHE_PROPRE } else { 0 };
                ix.pousser_brut(&u.nom, AUCUN, INCONNU, u.date, propres, cle(vol, u.frn));
                parents.push(cle(vol, u.parent));
            }
            compte.store(ix.len(), Relaxed);
        }
        for (k, pc) in parents.into_iter().enumerate() {
            let i = debut + k;
            match ix.par_cle(pc) {
                Some(p) => ix.parent[i] = p,
                None => {
                    // orphelin (dossiers internes $Extend…) : rattaché puis masqué
                    ix.parent[i] = racine;
                    ix.drap[i] |= SUPPRIME;
                }
            }
        }
        ix.volumes.push(Volume { racine: lettre.into(), idx: racine, mft: true, journal: id, usn: prochain });
        Ok((id, prochain))
    }

    /// Rattrape le journal depuis `usn` sans attendre. Renvoie le nouvel USN, ou None s'il faut tout relire.
    pub fn rattraper(m: &Moteur, vol: usize, lettre: &str, id: u64, usn: i64) -> Option<i64> {
        let h = ouvrir_volume(lettre)?;
        let (id2, premier, prochain) = journal(&h)?;
        if id2 != id || usn < premier {
            return None;
        }
        let mut cur = usn;
        let mut buf = vec![0u8; 1 << 16];
        while cur < prochain {
            let entree: [u64; 5] = [cur as u64, (USN_CREATION | USN_SUPPRESSION | USN_RENOMME_ANCIEN | USN_RENOMME_NOUVEAU) as u64, 0, 0, id];
            let mut r = 0u32;
            let ok = unsafe { DeviceIoControl(h.0, FSCTL_READ_USN_JOURNAL, entree.as_ptr() as *const c_void, 40, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut r, null_mut()) } != 0;
            if !ok || r < 8 {
                return None;
            }
            let suivant = i64::from_le_bytes(buf[..8].try_into().unwrap());
            let changes = lire_usn(&buf[8..r as usize]);
            if !changes.is_empty() {
                let mut ix = m.index.write().unwrap();
                for u in &changes {
                    appliquer_usn(&mut ix, vol, u);
                }
            }
            if suivant <= cur {
                break;
            }
            cur = suivant;
        }
        Some(cur)
    }

    /// Suit le journal en continu (bloque en attendant les changements).
    pub fn suivre_usn(m: Arc<Moteur>, vol: usize, lettre: String, id: u64, mut usn: i64) {
        let g = GEN_USN.load(Relaxed);
        std::thread::spawn(move || {
            let Some(h) = ouvrir_volume(&lettre) else { return };
            let mut buf = vec![0u8; 1 << 16];
            loop {
                // READ_USN_JOURNAL_DATA_V0 : StartUsn, ReasonMask + ReturnOnlyOnClose, Timeout, BytesToWaitFor, UsnJournalID
                let entree: [u64; 5] = [usn as u64, (USN_CREATION | USN_SUPPRESSION | USN_RENOMME_ANCIEN | USN_RENOMME_NOUVEAU) as u64, 0, 1, id];
                let mut r = 0u32;
                let ok = unsafe { DeviceIoControl(h.0, FSCTL_READ_USN_JOURNAL, entree.as_ptr() as *const c_void, 40, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut r, null_mut()) } != 0;
                if !ok || r < 8 {
                    *m.etat.lock().unwrap() = format!("Le journal de {lettre} a été réinitialisé : relance l'indexation.");
                    return;
                }
                if GEN_USN.load(Relaxed) != g {
                    return; // l'index a été reconstruit : un nouveau fil a pris le relais
                }
                usn = i64::from_le_bytes(buf[..8].try_into().unwrap());
                let changes = lire_usn(&buf[8..r as usize]);
                let mut ix = m.index.write().unwrap();
                let mut change = false;
                for u in &changes {
                    change |= appliquer_usn(&mut ix, vol, u);
                }
                if let Some(v) = ix.volumes.iter_mut().find(|v| v.racine == lettre) {
                    v.usn = usn;
                }
                drop(ix);
                if change {
                    m.version.fetch_add(1, Relaxed);
                }
                m.modifie.store(true, Relaxed);
            }
        });
    }

    pub struct Entree {
        pub nom: String,
        pub taille: u64,
        pub date: u32,
        pub attrs: u32,
    }

    /// Contenu d'un dossier, avec l'appel le plus rapide de Windows.
    pub fn lister(chemin: &str) -> Option<Vec<Entree>> {
        let motif = if chemin.starts_with("\\\\") { format!("{}\\*", chemin.trim_end_matches('\\')) } else { format!("\\\\?\\{}\\*", chemin.trim_end_matches('\\')) };
        let mut d: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        let h = unsafe { FindFirstFileExW(w(&motif).as_ptr(), FindExInfoBasic, &mut d as *mut _ as *mut c_void, FindExSearchNameMatch, null(), FIND_FIRST_EX_LARGE_FETCH) };
        if h == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut v = vec![];
        loop {
            let l = d.cFileName.iter().position(|c| *c == 0).unwrap_or(260);
            let nom = String::from_utf16_lossy(&d.cFileName[..l]);
            if nom != "." && nom != ".." {
                let ft = ((d.ftLastWriteTime.dwHighDateTime as u64) << 32) | d.ftLastWriteTime.dwLowDateTime as u64;
                v.push(Entree { nom, taille: ((d.nFileSizeHigh as u64) << 32) | d.nFileSizeLow as u64, date: depuis_filetime(ft), attrs: d.dwFileAttributes });
            }
            if unsafe { FindNextFileW(h, &mut d) } == 0 {
                break;
            }
        }
        unsafe { FindClose(h) };
        Some(v)
    }

    pub fn propres(attrs: u32) -> u8 {
        (if attrs & 0x10 != 0 { DOSSIER } else { 0 }) | if attrs & 0x6 != 0 { CACHE_PROPRE } else { 0 }
    }

    /// Parcours parallèle à partir de dossiers déjà dans l'index.
    pub fn parcourir(ix: &RwLock<Index>, depart: Vec<(String, u32)>, fils: usize, compte: &AtomicUsize, version: &AtomicU64, fond: bool) {
        let file = Mutex::new((depart, 0usize)); // (à visiter, en cours)
        std::thread::scope(|s| {
            for _ in 0..fils.max(1) {
                s.spawn(|| {
                    if fond {
                        // priorité basse (processeur et disque) : ne ralentit ni le PC ni les jeux
                        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};
                        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
                    }
                    loop {
                    let tache = {
                        let mut f = file.lock().unwrap();
                        match f.0.pop() {
                            Some(t) => {
                                f.1 += 1;
                                Some(t)
                            }
                            None if f.1 == 0 => return,
                            None => None,
                        }
                    };
                    let Some((chemin, d)) = tache else {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    };
                    let mut sous = vec![];
                    if let Some(liste) = lister(&chemin) {
                        let mut x = ix.write().unwrap();
                        for e in &liste {
                            let i = x.ajouter(&e.nom, d, if e.attrs & 0x10 != 0 { INCONNU } else { e.taille }, e.date, propres(e.attrs), 0);
                            // dossiers : on descend, sauf les liens (jonctions) et les dossiers inutiles
                            if e.attrs & 0x10 != 0 && e.attrs & 0x400 == 0 && !IGNORES.contains(&x.nrm(i)) {
                                sous.push((format!("{}\\{}", chemin.trim_end_matches('\\'), e.nom), i));
                            }
                        }
                        compte.store(x.len(), Relaxed);
                    }
                    let mut f = file.lock().unwrap();
                    f.0.extend(sous);
                    f.1 -= 1;
                    version.fetch_add(1, Relaxed);
                    }
                });
            }
        });
    }

    /// Surveille un lecteur entier ; les dossiers modifiés arrivent dans `sales`.
    pub fn surveiller(lettre: String, sales: Arc<Mutex<HashSet<String>>>, complet: Arc<AtomicBool>) {
        std::thread::spawn(move || {
            let racine = format!("{lettre}\\");
            let h = unsafe { CreateFileW(w(&racine).as_ptr(), 1, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, null_mut()) };
            if h == INVALID_HANDLE_VALUE {
                return;
            }
            let _p = Poignee(h);
            let mut buf = vec![0u32; 16 << 10]; // 64 Ko alignés sur 4 octets
            loop {
                let mut r = 0u32;
                let ok = unsafe { ReadDirectoryChangesW(h, buf.as_mut_ptr() as *mut c_void, (buf.len() * 4) as u32, 1, 1 | 2 | 8 | 0x10, &mut r, null_mut(), None) } != 0;
                if !ok {
                    return;
                }
                if r == 0 {
                    complet.store(true, Relaxed); // trop de changements d'un coup : tout revérifier
                    continue;
                }
                let b = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, r as usize) };
                let mut p = 0usize;
                let mut s = sales.lock().unwrap();
                loop {
                    if p + 12 > b.len() {
                        break;
                    }
                    let suivant = u32::from_le_bytes(b[p..p + 4].try_into().unwrap()) as usize;
                    let l = u32::from_le_bytes(b[p + 8..p + 12].try_into().unwrap()) as usize;
                    if p + 12 + l > b.len() {
                        break;
                    }
                    let u: Vec<u16> = b[p + 12..p + 12 + l].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    let rel = String::from_utf16_lossy(&u);
                    let dossier = match rel.rfind('\\') {
                        Some(k) => format!("{lettre}\\{}", &rel[..k]),
                        None => lettre.clone(),
                    };
                    // nos propres fichiers d'index changent tout le temps : on les ignore
                    if !rel.to_lowercase().contains("\\outils\\index_fichiers") {
                        s.insert(dossier);
                    }
                    if suivant == 0 {
                        break;
                    }
                    p += suivant;
                }
            }
        });
    }
}

// ===================================================================================================
// Le moteur : index partagé, construction, mises à jour
// ===================================================================================================

pub struct Moteur {
    pub index: RwLock<Index>,
    pub etat: Mutex<String>,
    pub en_cours: AtomicBool,
    pub compte: AtomicUsize,
    pub version: AtomicU64,
    pub modifie: AtomicBool,
    pub ultra: AtomicBool,
    pub demarre: AtomicBool,
}

static MOTEUR: OnceLock<Arc<Moteur>> = OnceLock::new();
static FOCUS: AtomicBool = AtomicBool::new(false);
/// Change à chaque reconstruction : les anciens fils de suivi du journal s'arrêtent d'eux-mêmes.
static GEN_USN: AtomicU64 = AtomicU64::new(0);
/// Dossiers signalés comme modifiés par la surveillance, et « tout revérifier ».
static SALES: OnceLock<Arc<Mutex<HashSet<String>>>> = OnceLock::new();
static COMPLET: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn sales() -> Arc<Mutex<HashSet<String>>> {
    SALES.get_or_init(Arc::default).clone()
}

fn complet() -> Arc<AtomicBool> {
    COMPLET.get_or_init(Arc::default).clone()
}

pub fn moteur() -> Arc<Moteur> {
    MOTEUR.get_or_init(|| Arc::new(Moteur {
        index: RwLock::default(),
        etat: Mutex::new("Index pas encore construit.".into()),
        en_cours: AtomicBool::new(false),
        compte: AtomicUsize::new(0),
        version: AtomicU64::new(0),
        modifie: AtomicBool::new(false),
        ultra: AtomicBool::new(false),
        demarre: AtomicBool::new(false),
    })).clone()
}

fn fichier_index() -> std::path::PathBuf {
    crate::outil::chemin("index_fichiers").with_extension("bin")
}

fn sauver(m: &Moteur) {
    use std::io::Write;
    let octets = m.index.read().unwrap().ecrire();
    let f = fichier_index();
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let tmp = f.with_extension("tmp");
    let ok = std::fs::File::create(&tmp).ok().and_then(|fic| {
        let mut z = flate2::write::DeflateEncoder::new(std::io::BufWriter::new(fic), flate2::Compression::fast());
        z.write_all(&octets).ok()?;
        z.finish().ok()?.flush().ok()
    });
    if ok.is_some() {
        let _ = std::fs::rename(&tmp, &f);
    }
    m.modifie.store(false, Relaxed);
}

fn charger_index() -> Option<Index> {
    use std::io::Read;
    let f = std::fs::File::open(fichier_index()).ok()?;
    let mut b = vec![];
    flate2::read::DeflateDecoder::new(std::io::BufReader::new(f)).read_to_end(&mut b).ok()?;
    Index::lire(&b)
}

fn fils() -> usize {
    std::thread::available_parallelism().map(|x| x.get()).unwrap_or(4).clamp(2, 16)
}

#[cfg(windows)]
fn construire(m: &Arc<Moteur>, dest: &RwLock<Index>, admin: bool, fond: bool) -> Vec<(usize, String, u64, i64)> {
    let mut suivis = vec![];
    let mut a_parcourir = vec![];
    for (lettre, ntfs) in win::lecteurs() {
        let vol = dest.read().unwrap().volumes.len();
        if admin && ntfs {
            *m.etat.lock().unwrap() = format!("⚡ Lecture de la table des fichiers de {lettre}…");
            let mut ix = dest.write().unwrap();
            let avant = ix.len();
            match win::mft(&mut ix, vol, &lettre, &m.compte) {
                Ok((id, usn)) => {
                    suivis.push((vol, lettre.clone(), id, usn));
                    continue;
                }
                Err(e) => {
                    // on écarte ce qui a pu être lu et on passe au parcours des dossiers
                    for i in avant..ix.len() {
                        ix.supprimer(i as u32);
                    }
                    *m.etat.lock().unwrap() = format!("{lettre} : {e}");
                }
            }
        }
        let mut ix = dest.write().unwrap();
        let r = ix.ajouter(&lettre, AUCUN, INCONNU, 0, DOSSIER, 0);
        ix.volumes.push(Volume { racine: lettre.clone(), idx: r, mft: false, journal: 0, usn: 0 });
        a_parcourir.push((format!("{lettre}\\"), r));
    }
    dest.write().unwrap().recalculer();
    if !a_parcourir.is_empty() {
        *m.etat.lock().unwrap() = "Parcours des dossiers…".into();
        win::parcourir(dest, a_parcourir, fils(), &m.compte, &m.version, fond);
    }
    suivis
}

/// Remet à jour un dossier modifié (mode surveillance).
#[cfg(windows)]
fn resynchroniser(m: &Moteur, chemin: &str) {
    let Some(liste) = win::lister(chemin) else { return };
    let mut nouveaux = vec![];
    {
        let mut ix = m.index.write().unwrap();
        let Some(d) = ix.trouver_chemin(chemin) else { return };
        let enfants = ix.enfants(d);
        let presents: HashSet<Vec<u8>> = liste.iter().map(|e| norm(&e.nom)).collect();
        for &e in &enfants {
            if !presents.contains(ix.nrm(e)) {
                ix.supprimer(e);
            }
        }
        let connus: HashMap<Vec<u8>, u32> = enfants.iter().filter(|e| ix.vivant(**e)).map(|&e| (ix.nrm(e).to_vec(), e)).collect();
        for e in &liste {
            match connus.get(&norm(&e.nom)) {
                Some(&i) => {
                    if e.attrs & 0x10 == 0 {
                        ix.taille[i as usize] = e.taille;
                    }
                    ix.date[i as usize] = e.date;
                }
                None => {
                    let i = ix.ajouter(&e.nom, d, if e.attrs & 0x10 != 0 { INCONNU } else { e.taille }, e.date, win::propres(e.attrs), 0);
                    if e.attrs & 0x10 != 0 && e.attrs & 0x400 == 0 {
                        nouveaux.push((format!("{}\\{}", chemin.trim_end_matches('\\'), e.nom), i));
                    }
                }
            }
        }
    }
    if !nouveaux.is_empty() {
        win::parcourir(&m.index, nouveaux, 2, &m.compte, &m.version, true);
    }
    m.version.fetch_add(1, Relaxed);
    m.modifie.store(true, Relaxed);
}

/// Lance (une fois) le chargement de l'index, sa construction et sa mise à jour en continu.
pub fn demarrer(forcer: bool) {
    let m = moteur();
    if m.en_cours.swap(true, Relaxed) {
        return;
    }
    let premier = !m.demarre.swap(true, Relaxed);
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            let admin = crate::optimiseur::est_admin();
            m.ultra.store(admin, Relaxed);
            let debut = Instant::now();
            let mut pret = false;
            if premier && !forcer {
                *m.etat.lock().unwrap() = "Chargement de l'index…".into();
                if let Some(ix) = charger_index() {
                    let n = ix.vivants;
                    let tout_mft = !ix.volumes.is_empty() && ix.volumes.iter().all(|v| v.mft);
                    *m.index.write().unwrap() = ix;
                    m.compte.store(n, Relaxed);
                    m.version.fetch_add(1, Relaxed);
                    *m.etat.lock().unwrap() = format!("{n} fichiers prêts (index enregistré), mise à jour…");
                    // mode ⚡ : on rattrape le journal de chaque disque, sans tout relire
                    if admin && tout_mft {
                        let vols = m.index.read().unwrap().volumes.clone();
                        let mut ok = true;
                        let mut suivis = vec![];
                        for (k, v) in vols.iter().enumerate() {
                            match win::rattraper(&m, k, &v.racine, v.journal, v.usn) {
                                Some(u) => suivis.push((k, v.racine.clone(), v.journal, u)),
                                None => ok = false,
                            }
                        }
                        if ok && vols.len() == win::lecteurs().len() {
                            m.index.write().unwrap().recalculer();
                            GEN_USN.fetch_add(1, Relaxed);
                            for (k, l, id, u) in suivis {
                                win::suivre_usn(m.clone(), k, l, id, u);
                            }
                            pret = true;
                        }
                    }
                }
            }
            let (sales, complet) = (sales(), complet());
            if !pret {
                GEN_USN.fetch_add(1, Relaxed);
                // surveiller dès maintenant pour ne rien rater pendant la construction (une seule fois)
                if !admin && premier {
                    for (l, _) in win::lecteurs() {
                        win::surveiller(l, sales.clone(), complet.clone());
                    }
                }
                let progressif = m.index.read().unwrap().len() == 0;
                let neuf = RwLock::new(Index::default());
                let suivis = if progressif {
                    construire(&m, &m.index, admin, false)
                } else {
                    // l'ancien index sert pendant ce temps : on peut prendre son temps
                    let s = construire(&m, &neuf, admin, true);
                    *m.index.write().unwrap() = neuf.into_inner().unwrap();
                    s
                };
                for (k, l, id, u) in suivis.iter().cloned() {
                    win::suivre_usn(m.clone(), k, l, id, u);
                }
                // en mode ⚡, les disques non NTFS sont surveillés à l'ancienne
                if admin && premier {
                    let vols = m.index.read().unwrap().volumes.clone();
                    for v in vols.iter().filter(|v| !v.mft) {
                        win::surveiller(v.racine.clone(), sales.clone(), complet.clone());
                    }
                }
                m.version.fetch_add(1, Relaxed);
                sauver(&m);
            } else {
                sauver(&m);
            }
            let n = m.index.read().unwrap().vivants;
            m.compte.store(n, Relaxed);
            *m.etat.lock().unwrap() = format!("{} fichiers et dossiers indexés en {:.1} s{}", espaces(n), debut.elapsed().as_secs_f32(), if admin { " · ⚡ mode ultra-rapide (journal NTFS)" } else { "" });
            m.en_cours.store(false, Relaxed);
            // mises à jour en continu : dossiers modifiés et enregistrement régulier
            if premier {
                let m2 = m.clone();
                std::thread::spawn(move || {
                    let mut dernier = Instant::now();
                    loop {
                        std::thread::sleep(Duration::from_millis(800));
                        if crate::outil::DISCRET.load(Relaxed) {
                            continue; // un jeu tourne : les changements attendent la fin de la partie
                        }
                        if !m2.en_cours.load(Relaxed) {
                            if complet.swap(false, Relaxed) {
                                sales.lock().unwrap().clear();
                                demarrer(true);
                                continue;
                            }
                            let lot: Vec<String> = sales.lock().unwrap().drain().collect();
                            for d in lot {
                                resynchroniser(&m2, &d);
                            }
                        }
                        if m2.modifie.load(Relaxed) && dernier.elapsed() > Duration::from_secs(300) {
                            dernier = Instant::now();
                            sauver(&m2);
                        }
                    }
                });
            }
        }
        #[cfg(not(windows))]
        {
            let _ = (forcer, premier);
            m.en_cours.store(false, Relaxed);
        }
    });
}

/// Ctrl+Alt+F : ouvre la recherche par-dessus tout.
pub fn raccourci() {
    #[cfg(windows)]
    std::thread::spawn(|| unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        if crate::outil::raccourci(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'F' as u32) == 0 {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                FOCUS.store(true, Relaxed);
                crate::interface2::ouvrir_outil(NOM);
            }
        }
    });
}

pub fn espaces(n: usize) -> String {
    let s = n.to_string();
    let mut r = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            r.push('\u{202F}');
        }
        r.push(c);
    }
    r
}

fn maintenant() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
}

fn stat(chemin: &str) -> Option<(u64, u32)> {
    let m = std::fs::metadata(chemin).ok()?;
    let d = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as u32;
    Some((if m.is_dir() { INCONNU } else { m.len() }, d))
}

// ===================================================================================================
// L'outil
// ===================================================================================================

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Options {
    au_demarrage: bool,
    masquer: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { au_demarrage: true, masquer: false }
    }
}

type Reponse = Arc<Mutex<Option<(u64, Resultat, Duration)>>>;

pub struct RechercheFichiers {
    requete: String,
    lancee: Option<(String, u64, bool)>,
    generation: Arc<AtomicU64>,
    reponse: Reponse,
    res: Resultat,
    duree: Duration,
    choisi: usize,
    infos: HashMap<u32, (u64, u32)>,
    chemins: HashMap<u32, String>,
    opts: Options,
    historique: Arc<HashMap<String, u32>>,
    dernier_lancement: Instant,
    defiler: bool,
    aide: bool,
}

impl RechercheFichiers {
    pub fn new() -> Self {
        let opts: Options = charger("recherche_fichiers");
        if opts.au_demarrage {
            demarrer(false);
        }
        raccourci();
        RechercheFichiers {
            requete: String::new(), lancee: None, generation: Arc::default(), reponse: Arc::default(), res: Resultat::default(), duree: Duration::ZERO,
            choisi: 0, infos: HashMap::new(), chemins: HashMap::new(), opts, historique: Arc::new(charger("recherche_historique")), dernier_lancement: Instant::now(), defiler: false, aide: false,
        }
    }

    fn lancer(&mut self) {
        let m = moteur();
        let version = m.version.load(Relaxed);
        let cle = (self.requete.clone(), version, self.opts.masquer);
        if self.lancee.as_ref() == Some(&cle) {
            return;
        }
        // pendant la construction, on ne relance pas plus d'une fois par seconde
        if self.lancee.as_ref().is_some_and(|l| l.0 == cle.0 && l.2 == cle.2) && self.dernier_lancement.elapsed() < Duration::from_millis(1000) {
            return;
        }
        self.lancee = Some(cle);
        self.dernier_lancement = Instant::now();
        let g = self.generation.fetch_add(1, Relaxed) + 1;
        let (q, masquer, gen, rep, hist) = (self.requete.clone(), self.opts.masquer, self.generation.clone(), self.reponse.clone(), self.historique.clone());
        std::thread::spawn(move || {
            let debut = Instant::now();
            let r = analyser(&q, maintenant());
            let ix = m.index.read().unwrap();
            if gen.load(Relaxed) != g {
                return;
            }
            let mut res = chercher(&ix, &r, 5000, masquer, &stat);
            // fichiers déjà ouverts depuis la recherche : un peu plus haut
            if !hist.is_empty() {
                let mut notes: Vec<(i32, u32)> = res.liste.iter().enumerate().map(|(k, &i)| (-(k as i32) + hist.get(&ix.chemin(i)).map_or(0, |n| 300 * (*n).min(3) as i32), i)).collect();
                notes.sort_by(|a, b| b.0.cmp(&a.0));
                res.liste = notes.into_iter().map(|x| x.1).collect();
            }
            drop(ix);
            if gen.load(Relaxed) == g {
                *rep.lock().unwrap() = Some((g, res, debut.elapsed()));
            }
        });
    }

    fn noter_ouverture(&mut self, chemin: &str) {
        let mut h = (*self.historique).clone();
        *h.entry(chemin.to_string()).or_insert(0) += 1;
        if h.len() > 500 {
            let mut v: Vec<(String, u32)> = h.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            v.truncate(400);
            h = v.into_iter().collect();
        }
        enregistrer("recherche_historique", &h);
        self.historique = Arc::new(h);
    }
}

fn icone(nom: &str, dossier: bool) -> &'static str {
    if dossier {
        return "📁";
    }
    let e = nom.rsplit('.').next().unwrap_or("").to_lowercase();
    for (f, i) in [("image", "🖼"), ("video", "🎬"), ("audio", "🎵"), ("doc", "📄"), ("archive", "🗜"), ("exe", "⚙"), ("code", "📝"), ("minecraft", "⛏")] {
        if familles(f).is_some_and(|l| l.contains(&e.as_str())) {
            return i;
        }
    }
    "📃"
}

fn date_texte(t: u32) -> String {
    if t == 0 {
        return String::new();
    }
    let (a, m, j) = crate::quotidien2::date_de_jours(t as i64 / 86_400);
    format!("{j:02}/{m:02}/{a}")
}

#[cfg(windows)]
fn montrer_dans_dossier(p: &str) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{p}")).spawn();
}

impl Outil for RechercheFichiers {
    fn nom(&self) -> &'static str {
        NOM
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let m = moteur();
        if !m.demarre.load(Relaxed) {
            demarrer(false);
        }
        // réponse de la recherche en cours
        if let Some((g, r, d)) = self.reponse.lock().unwrap().take() {
            if g == self.generation.load(Relaxed) {
                let meme = r.liste.first() == self.res.liste.first();
                self.res = r;
                self.duree = d;
                self.infos.clear();
                self.chemins.clear();
                if !meme {
                    self.choisi = 0;
                }
                self.choisi = self.choisi.min(self.res.liste.len().saturating_sub(1));
            }
        }
        let barre = ui.add(egui::TextEdit::singleline(&mut self.requete).hint_text("🔎 Rechercher un fichier sur tout le PC…   (Ctrl+Alt+F de n'importe où)").font(egui::FontId::proportional(22.0)).desired_width(f32::INFINITY).margin(egui::vec2(10.0, 8.0)));
        if FOCUS.swap(false, Relaxed) || ui.memory(|me| me.focused().is_none()) {
            barre.request_focus();
        }
        if self.requete.trim().is_empty() {
            self.res = Resultat::default();
            self.lancee = None;
        } else {
            self.lancer();
            if self.lancee.as_ref().is_some_and(|l| l.1 != m.version.load(Relaxed)) {
                ui.ctx().request_repaint_after(Duration::from_millis(1100));
            }
        }
        // ligne d'état
        let en_cours = m.en_cours.load(Relaxed);
        ui.horizontal_wrapped(|ui| {
            if en_cours {
                ui.spinner();
                ui.label(format!("{} · {} entrées lues", m.etat.lock().unwrap(), espaces(m.compte.load(Relaxed))));
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            } else {
                ui.weak(m.etat.lock().unwrap().clone());
            }
            if !self.requete.trim().is_empty() {
                ui.label(RichText::new(format!("· {} résultat{} en {} ms{}", espaces(self.res.total), if self.res.total > 1 { "s" } else { "" }, self.duree.as_millis(), if self.res.flou { " (orthographe approchée)" } else { "" })).strong());
            }
        });
        ui.horizontal_wrapped(|ui| {
            #[cfg(windows)]
            if !m.ultra.load(Relaxed) && !en_cours {
                if ui.button("⚡ Mode ultra-rapide (relancer en administrateur)").on_hover_text("Lit directement la table des fichiers NTFS : l'indexation passe de plusieurs minutes à quelques secondes, et les changements sont suivis en temps réel.").clicked() && crate::optimiseur::relancer_admin() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if ui.add_enabled(!en_cours, egui::Button::new("🔄 Réindexer")).clicked() {
                demarrer(true);
            }
            let mut o = self.opts.clone();
            ui.checkbox(&mut o.masquer, "Masquer fichiers système et cachés");
            ui.checkbox(&mut o.au_demarrage, "Indexer au démarrage d'Outils");
            if o.masquer != self.opts.masquer || o.au_demarrage != self.opts.au_demarrage {
                self.opts = o;
                enregistrer("recherche_fichiers", &self.opts);
            }
            ui.toggle_value(&mut self.aide, "❓ Filtres");
        });
        if self.aide {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::Grid::new("aide_filtres").striped(true).show(ui, |ui| {
                    for (a, b) in [
                        ("rapport maths", "les deux mots dans le nom (sans tenir compte des accents ni des majuscules)"),
                        ("\"devoir de maths\"", "l'expression exacte"),
                        ("*.mp4   photo??.jpg", "jokers : * = n'importe quoi, ? = une lettre"),
                        ("ext:mp4;mkv", "par extension"),
                        ("type:image  video  audio  doc  archive  exe  code  minecraft  dossier", "par famille"),
                        ("taille:>1go   taille:<500ko   taille:10mo-1go", "par taille"),
                        ("date:aujourdhui  hier  semaine  mois  2026  2026-05  >2026-01-01", "par date de modification"),
                        ("dossier:minecraft", "seulement dans un dossier dont le chemin contient ce mot"),
                        ("!temp", "exclure un mot"),
                        ("↑ ↓ Entrée", "choisir et ouvrir · Ctrl+Entrée : ouvrir le dossier · Ctrl+Maj+C : copier le chemin"),
                    ] {
                        ui.monospace(a);
                        ui.weak(b);
                        ui.end_row();
                    }
                });
            });
        }
        if self.res.liste.is_empty() {
            if !self.requete.trim().is_empty() && !en_cours && self.lancee.is_some() {
                ui.add_space(20.0);
                ui.weak("Aucun fichier trouvé.");
            }
            return;
        }
        // clavier
        let n = self.res.liste.len();
        let (haut, bas, page_h, page_b, entree, ctrl, copier) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown), i.key_pressed(Key::PageUp), i.key_pressed(Key::PageDown), i.key_pressed(Key::Enter), i.modifiers.ctrl, i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(Key::C)));
        let avant = self.choisi;
        if haut {
            self.choisi = self.choisi.saturating_sub(1);
        }
        if bas {
            self.choisi = (self.choisi + 1).min(n - 1);
        }
        if page_h {
            self.choisi = self.choisi.saturating_sub(15);
        }
        if page_b {
            self.choisi = (self.choisi + 15).min(n - 1);
        }
        if avant != self.choisi {
            self.defiler = true;
        }
        let ix = m.index.read().unwrap();
        let chemin_de = |s: &mut Self, i: u32| -> String { s.chemins.entry(i).or_insert_with(|| ix.chemin(i)).clone() };
        let mut ouvrir_p: Option<(String, bool)> = None;
        if entree && self.choisi < n {
            let i = self.res.liste[self.choisi];
            if (i as usize) < ix.len() {
                ouvrir_p = Some((chemin_de(self, i), ctrl));
            }
        }
        if copier && self.choisi < n {
            let i = self.res.liste[self.choisi];
            if (i as usize) < ix.len() {
                ui.ctx().copy_text(chemin_de(self, i));
            }
        }
        ui.separator();
        let hauteur = 24.0;
        let largeur = ui.available_width();
        let col_nom = (largeur * 0.36).max(200.0);
        let col_dossier = (largeur - col_nom - 200.0).max(150.0);
        ui.horizontal(|ui| {
            ui.add_sized([col_nom, 16.0], egui::Label::new(RichText::new("Nom").strong()));
            ui.add_sized([col_dossier, 16.0], egui::Label::new(RichText::new("Dossier").strong()));
            ui.add_sized([80.0, 16.0], egui::Label::new(RichText::new("Taille").strong()));
            ui.add_sized([90.0, 16.0], egui::Label::new(RichText::new("Modifié").strong()));
        });
        let defiler = std::mem::take(&mut self.defiler);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, hauteur, n, |ui, lignes| {
            for k in lignes {
                let i = self.res.liste[k];
                if i as usize >= ix.len() || !ix.vivant(i) {
                    continue;
                }
                let dossier = ix.drap[i as usize] & DOSSIER != 0;
                let nom = ix.nom(i).to_string();
                let chemin = chemin_de(self, i);
                let parent = chemin.rsplit_once('\\').map(|x| x.0.to_string()).unwrap_or_default();
                let (mut t, mut d) = (ix.taille[i as usize], ix.date[i as usize]);
                if t == INCONNU && !dossier || d == 0 {
                    let x = *self.infos.entry(i).or_insert_with(|| stat(&chemin).unwrap_or((INCONNU, 0)));
                    if t == INCONNU {
                        t = x.0;
                    }
                    if d == 0 {
                        d = x.1;
                    }
                }
                let choisi = k == self.choisi;
                let r = ui.horizontal(|ui| {
                    ui.set_height(hauteur);
                    let couleur = if ix.drap[i as usize] & (SYSTEME | ATTR_CACHE) != 0 { Color32::GRAY } else { ui.visuals().strong_text_color() };
                    ui.add_sized([col_nom, hauteur], egui::Label::new(RichText::new(format!("{} {nom}", icone(&nom, dossier))).color(couleur)).truncate());
                    ui.add_sized([col_dossier, hauteur], egui::Label::new(RichText::new(&parent).weak()).truncate());
                    ui.add_sized([80.0, hauteur], egui::Label::new(if dossier || t == INCONNU { String::new() } else { crate::disque::taille_lisible(t) }));
                    ui.add_sized([90.0, hauteur], egui::Label::new(date_texte(d)));
                }).response;
                let rep = ui.interact(r.rect, ui.id().with(("ligne", k)), egui::Sense::click());
                if choisi {
                    ui.painter().rect_filled(r.rect, 4.0, Color32::from_rgba_unmultiplied(255, 95, 162, 40));
                    if defiler {
                        ui.scroll_to_rect(r.rect, None);
                    }
                } else if rep.hovered() {
                    ui.painter().rect_filled(r.rect, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 12));
                }
                if rep.clicked() {
                    self.choisi = k;
                }
                if rep.double_clicked() {
                    ouvrir_p = Some((chemin.clone(), false));
                }
                rep.on_hover_text(&chemin).context_menu(|ui| {
                    if ui.button("▶ Ouvrir").clicked() {
                        ouvrir_p = Some((chemin.clone(), false));
                        ui.close_menu();
                    }
                    if ui.button("📂 Ouvrir le dossier").clicked() {
                        ouvrir_p = Some((chemin.clone(), true));
                        ui.close_menu();
                    }
                    if ui.button("📋 Copier le chemin").clicked() {
                        ui.ctx().copy_text(chemin.clone());
                        ui.close_menu();
                    }
                    if ui.button("📋 Copier le nom").clicked() {
                        ui.ctx().copy_text(nom.clone());
                        ui.close_menu();
                    }
                });
            }
        });
        drop(ix);
        if let Some((p, dossier)) = ouvrir_p {
            self.noter_ouverture(&p);
            #[cfg(windows)]
            if dossier {
                montrer_dans_dossier(&p);
            } else {
                crate::outil::ouvrir(std::path::Path::new(&p));
            }
            #[cfg(not(windows))]
            let _ = dossier;
        }
        if self.res.total > n {
            ui.weak(format!("Les {} meilleurs sont affichés : précise ta recherche pour voir les autres.", espaces(n)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exemple() -> Index {
        let mut ix = Index::default();
        let c = ix.ajouter("C:", AUCUN, INCONNU, 0, DOSSIER, 0);
        ix.volumes.push(Volume { racine: "C:".into(), idx: c, ..Default::default() });
        let u = ix.ajouter("Users", c, INCONNU, 0, DOSSIER, 0);
        let moi = ix.ajouter("Moi", u, INCONNU, 0, DOSSIER, 0);
        let bureau = ix.ajouter("Desktop", moi, INCONNU, 0, DOSSIER, 0);
        let appdata = ix.ajouter("AppData", moi, INCONNU, 0, DOSSIER, 0);
        let win = ix.ajouter("Windows", c, INCONNU, 0, DOSSIER, 0);
        ix.ajouter("Rapport de stage.docx", bureau, 20_000, 1_700_000_000, 0, 0);
        ix.ajouter("rapport.pdf", win, 5_000, 1_700_000_000, 0, 0);
        ix.ajouter("vieux_rapport.txt", appdata, 100, 1_600_000_000, CACHE_PROPRE, 0);
        ix.ajouter("Vacances été.mp4", bureau, 3 << 30, 1_750_000_000, 0, 0);
        ix.ajouter("Minecraft", bureau, INCONNU, 1_750_000_000, DOSSIER, 0);
        ix
    }

    fn sans_stat(_: &str) -> Option<(u64, u32)> {
        None
    }

    fn noms(ix: &Index, r: &Resultat) -> Vec<String> {
        r.liste.iter().map(|i| ix.nom(*i).to_string()).collect()
    }

    #[test]
    fn normalisation_et_requete() {
        assert_eq!(norm("Vacances ÉTÉ Œuvre"), b"vacances ete oeuvre");
        assert_eq!(lire_taille("1,5 go"), Some(1_610_612_736));
        assert_eq!(lire_taille("300k"), Some(307_200));
        let r = analyser("rapport \"de stage\" !temp ext:docx;.pdf taille:>10ko dossier:Bureau type:fichier", 1_700_000_000);
        assert_eq!(r.termes, vec![b"rapport".to_vec(), b"de stage".to_vec()]);
        assert_eq!(r.exclus, vec![b"temp".to_vec()]);
        assert_eq!(r.ext, vec![b"docx".to_vec(), b"pdf".to_vec()]);
        assert_eq!(r.taille_min, Some(10_240));
        assert_eq!(r.dossier, vec![b"bureau".to_vec()]);
        assert_eq!(r.seulement, Some(false));
        let r = analyser("date:2026-05", 0);
        assert_eq!(r.date_min, Some(crate::quotidien2::jours_depuis_1970(2026, 5, 1) as u32 * 86_400));
        assert!(analyser("type:video", 0).ext.contains(&b"mkv".to_vec()));
        let r = analyser("minecraft\\saves", 0);
        assert_eq!((r.dossier, r.termes), (vec![b"minecraft".to_vec()], vec![b"saves".to_vec()]));
        assert!(joker(b"vacances.mp4", b"*.mp4"));
        assert!(joker(b"photo12.jpg", b"photo??.jpg"));
        assert!(!joker(b"photo123.jpg", b"photo??.jpg"));
        assert!(flou(b"rapport.pdf", b"raprot").is_some());
        assert!(flou(b"rapport.pdf", b"xyz").is_none());
        assert_eq!(espaces(1234567), "1\u{202F}234\u{202F}567");
    }

    #[test]
    fn drapeaux_et_chemins() {
        let ix = exemple();
        let i = (0..ix.len() as u32).find(|i| ix.nom(*i) == "Rapport de stage.docx").unwrap();
        assert_eq!(ix.chemin(i), "C:\\Users\\Moi\\Desktop\\Rapport de stage.docx");
        assert!(ix.drap[i as usize] & PERSO != 0);
        let j = (0..ix.len() as u32).find(|i| ix.nom(*i) == "rapport.pdf").unwrap();
        assert!(ix.drap[j as usize] & SYSTEME != 0);
        let k = (0..ix.len() as u32).find(|i| ix.nom(*i) == "vieux_rapport.txt").unwrap();
        assert!(ix.drap[k as usize] & (SYSTEME | ATTR_CACHE) == SYSTEME | ATTR_CACHE);
        assert_eq!(ix.trouver_chemin("C:\\users\\moi\\DESKTOP"), ix.parent.get(i as usize).copied());
        assert_eq!(ix.chemin(0), "C:\\");
        // enregistrement et relecture : mêmes chemins et drapeaux
        let b = ix.ecrire();
        let iy = Index::lire(&b).unwrap();
        assert_eq!(iy.len(), ix.len());
        for x in 0..ix.len() as u32 {
            assert_eq!(iy.chemin(x), ix.chemin(x));
            assert_eq!(iy.drap[x as usize], ix.drap[x as usize]);
        }
        assert!(Index::lire(&b[..b.len() - 3]).is_none());
    }

    #[test]
    fn recherche() {
        let mut ix = exemple();
        let r = chercher(&ix, &analyser("rapport", 0), 100, false, &sans_stat);
        let v = noms(&ix, &r);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], "Rapport de stage.docx"); // dans le Bureau : devant
        assert_eq!(v[2], "vieux_rapport.txt"); // caché dans AppData : derrière
        assert_eq!(chercher(&ix, &analyser("rapport", 0), 100, true, &sans_stat).total, 1);
        assert_eq!(noms(&ix, &chercher(&ix, &analyser("ete", 0), 100, false, &sans_stat)), vec!["Vacances été.mp4"]);
        assert_eq!(chercher(&ix, &analyser("*.mp4", 0), 100, false, &sans_stat).total, 1);
        assert_eq!(chercher(&ix, &analyser("taille:>1go", 0), 100, false, &sans_stat).total, 1);
        assert_eq!(chercher(&ix, &analyser("rapport !stage", 0), 100, false, &sans_stat).total, 2);
        assert_eq!(chercher(&ix, &analyser("type:dossier mine", 0), 100, false, &sans_stat).total, 1);
        assert_eq!(chercher(&ix, &analyser("rapport dossier:desktop", 0), 100, false, &sans_stat).total, 1);
        let f = chercher(&ix, &analyser("vacnces", 0), 100, false, &sans_stat);
        assert!(f.flou && f.total >= 1);
        // un dossier supprimé fait disparaître ce qu'il contient
        let bureau = ix.trouver_chemin("C:\\Users\\Moi\\Desktop").unwrap();
        ix.supprimer(bureau);
        assert_eq!(chercher(&ix, &analyser("rapport", 0), 100, false, &sans_stat).total, 2);
        // tailles inconnues (mode MFT) : vérifiées à la demande
        let mut iz = Index::default();
        let c = iz.ajouter("C:", AUCUN, INCONNU, 0, DOSSIER, 0);
        iz.ajouter("gros.iso", c, INCONNU, 0, 0, 0);
        iz.ajouter("petit.iso", c, INCONNU, 0, 0, 0);
        let v = |p: &str| Some(if p.contains("gros") { (5 << 30, 1) } else { (10, 1) });
        assert_eq!(noms(&iz, &chercher(&iz, &analyser("iso taille:>1go", 0), 100, false, &v)), vec!["gros.iso"]);
    }

    #[test]
    fn journal_usn() {
        // un enregistrement USN_RECORD_V2 fabriqué à la main
        let fabriquer = |frn: u64, parent: u64, raison: u32, attrs: u32, nom: &str| {
            let u: Vec<u8> = nom.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            let long = (60 + u.len() + 7) & !7;
            let mut b = vec![0u8; long];
            b[0..4].copy_from_slice(&(long as u32).to_le_bytes());
            b[4..6].copy_from_slice(&2u16.to_le_bytes());
            b[8..16].copy_from_slice(&(frn | (3 << 48)).to_le_bytes());
            b[16..24].copy_from_slice(&parent.to_le_bytes());
            b[32..40].copy_from_slice(&(133_000_000_000_000_000u64).to_le_bytes());
            b[40..44].copy_from_slice(&raison.to_le_bytes());
            b[52..56].copy_from_slice(&attrs.to_le_bytes());
            b[56..58].copy_from_slice(&(u.len() as u16).to_le_bytes());
            b[58..60].copy_from_slice(&60u16.to_le_bytes());
            b[60..60 + u.len()].copy_from_slice(&u);
            b
        };
        let mut flux = fabriquer(100, 5, USN_CREATION, 0x10, "Jeux");
        flux.extend(fabriquer(101, 100, USN_CREATION, 0x20, "sauvegarde.zip"));
        let v = lire_usn(&flux);
        assert_eq!(v.len(), 2);
        assert_eq!((v[1].frn, v[1].parent, v[1].nom.as_str()), (101, 100, "sauvegarde.zip"));
        assert!(v[0].date > 1_600_000_000);
        let mut ix = Index::default();
        let c = ix.ajouter("D:", AUCUN, INCONNU, 0, DOSSIER, cle(0, 5));
        ix.volumes.push(Volume { racine: "D:".into(), idx: c, mft: true, ..Default::default() });
        for u in &v {
            assert!(appliquer_usn(&mut ix, 0, u));
        }
        let i = ix.par_cle(cle(0, 101)).unwrap();
        assert_eq!(ix.chemin(i), "D:\\Jeux\\sauvegarde.zip");
        // renommage puis suppression
        let r = lire_usn(&fabriquer(101, 5, USN_RENOMME_NOUVEAU, 0x20, "save.zip"));
        appliquer_usn(&mut ix, 0, &r[0]);
        assert_eq!(ix.chemin(i), "D:\\save.zip");
        let s = lire_usn(&fabriquer(101, 5, USN_SUPPRESSION, 0x20, "save.zip"));
        appliquer_usn(&mut ix, 0, &s[0]);
        assert!(!ix.vivant(i) && ix.par_cle(cle(0, 101)).is_none());
        // ordre d'arrivée quelconque (comme la MFT) : recalculer remet tout d'aplomb
        let mut iy = Index::default();
        let f = iy.pousser_brut("photo.jpg", 2, 1, 1, 0, 0);
        let r = iy.pousser_brut("C:", AUCUN, INCONNU, 0, DOSSIER, 0);
        iy.pousser_brut("Users", r, INCONNU, 0, DOSSIER, 0);
        iy.parent[f as usize] = 2;
        iy.recalculer();
        assert_eq!(iy.prof[f as usize], 2);
        assert_eq!(iy.vivants, 3);
    }
}
