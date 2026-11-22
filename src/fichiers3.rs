//! Fichiers, suite : synchro clé USB (34), surveillance de dossiers (35), renommage d'épisodes (36),
//! recherche dans le contenu (37), corbeille automatique (38), installateurs oubliés (39),
//! vérification d'empreinte (40), gestionnaire de téléchargements (41).
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn heure_locale() -> String {
    let h = crate::minecraft::horodatage(); // AAAA-MM-JJ_HH-MM-SS
    format!("{}/{} {}", h.get(8..10).unwrap_or(""), h.get(5..7).unwrap_or(""), h.get(11..16).unwrap_or("").replace('-', ":"))
}

/// Minuscules sans accents.
pub fn simple(s: &str) -> String {
    s.to_lowercase().chars().map(|c| match c {
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'à' | 'â' | 'ä' => 'a',
        'î' | 'ï' => 'i',
        'ô' | 'ö' => 'o',
        'û' | 'ù' | 'ü' => 'u',
        'ç' => 'c',
        x => x,
    }).collect()
}

fn telechargements() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads")
}

fn taille(o: u64) -> String {
    crate::disque::taille_lisible(o)
}

// ---------------------------------------------------------------------------------------------------
// 34 · Synchro avec une clé USB
// ---------------------------------------------------------------------------------------------------

/// (lettre « E:\ », numéro de série, nom) des lecteurs autres que C:.
pub(crate) fn lecteurs() -> Vec<(String, u32, String)> {
    use windows_sys::Win32::Storage::FileSystem::{GetLogicalDrives, GetVolumeInformationW};
    let m = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| m & (1 << i) != 0 && *i >= 3)
        .filter_map(|i| {
            let d = format!("{}:\\", (b'A' + i) as char);
            let mut nom = [0u16; 261];
            let mut serie = 0u32;
            let ok = unsafe { GetVolumeInformationW(w(&d).as_ptr(), nom.as_mut_ptr(), 261, &mut serie, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
            (ok != 0).then(|| (d, serie, String::from_utf16_lossy(&nom).trim_end_matches('\0').to_string()))
        })
        .collect()
}

/// Faut-il recopier ? (absent, autre taille, ou plus récent de plus de 2 s — précision du FAT32).
pub fn a_copier(src: (u64, SystemTime), dst: Option<(u64, SystemTime)>) -> bool {
    match dst {
        None => true,
        Some((t, m)) => t != src.0 || src.1.duration_since(m).is_ok_and(|d| d > Duration::from_secs(2)),
    }
}

fn meta(p: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Copie src → dst (et supprime le surplus si miroir). Renvoie (copiés, supprimés).
fn synchroniser(src: &Path, dst: &Path, miroir: bool, arret: &AtomicBool) -> std::io::Result<(u32, u32)> {
    std::fs::create_dir_all(dst)?;
    let (mut c, mut s) = (0, 0);
    let mut vus = std::collections::HashSet::new();
    for e in std::fs::read_dir(src)?.flatten() {
        if arret.load(Relaxed) {
            break;
        }
        let (p, q) = (e.path(), dst.join(e.file_name()));
        vus.insert(e.file_name());
        if p.is_dir() {
            let (a, b) = synchroniser(&p, &q, miroir, arret)?;
            c += a;
            s += b;
        } else if let Some(m) = meta(&p) {
            if a_copier(m, meta(&q)) {
                std::fs::copy(&p, &q)?;
                c += 1;
            }
        }
    }
    if miroir {
        for e in std::fs::read_dir(dst)?.flatten() {
            if !vus.contains(&e.file_name()) && !e.file_name().to_string_lossy().starts_with("System Volume") {
                let p = e.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                s += 1;
            }
        }
    }
    Ok((c, s))
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Paire {
    pub source: String,
    pub serie: u32,
    pub nom_cle: String,
    pub sous_dossier: String,
    pub miroir: bool,
    pub auto: bool,
    pub derniere: String,
}

pub struct SyncUsb {
    paires: Arc<Mutex<Vec<Paire>>>,
    statut: Arc<Mutex<String>>,
    lancer: Arc<Mutex<Option<usize>>>,
}

impl SyncUsb {
    pub fn new() -> Self {
        let paires: Arc<Mutex<Vec<Paire>>> = Arc::new(Mutex::new(charger("synchro_usb")));
        let statut = Arc::new(Mutex::new(String::new()));
        let lancer: Arc<Mutex<Option<usize>>> = Arc::default();
        let (p, st, l) = (paires.clone(), statut.clone(), lancer.clone());
        std::thread::spawn(move || {
            let mut presents: Vec<u32> = lecteurs().iter().map(|x| x.1).collect();
            let arret = AtomicBool::new(false);
            loop {
                std::thread::sleep(Duration::from_secs(4));
                let lec = lecteurs();
                let demande = l.lock().unwrap().take();
                let liste = p.lock().unwrap().clone();
                for (i, pr) in liste.iter().enumerate() {
                    let Some((lettre, ..)) = lec.iter().find(|x| x.1 == pr.serie && pr.serie != 0) else { continue };
                    let branche = !presents.contains(&pr.serie);
                    if demande == Some(i) || (branche && pr.auto) {
                        *st.lock().unwrap() = format!("Synchro vers {} en cours…", pr.nom_cle);
                        let dst = PathBuf::from(lettre).join(&pr.sous_dossier);
                        let r = synchroniser(Path::new(&pr.source), &dst, pr.miroir, &arret);
                        let m = match r {
                            Ok((c, s)) => format!("✔ {} : {c} fichier(s) copié(s){}", pr.nom_cle, if s > 0 { format!(", {s} supprimé(s)") } else { String::new() }),
                            Err(e) => format!("✖ {} : {e}", pr.nom_cle),
                        };
                        crate::systeme::notifier("Synchro clé USB", &m);
                        *st.lock().unwrap() = m.clone();
                        let mut pp = p.lock().unwrap();
                        if let Some(x) = pp.get_mut(i) {
                            x.derniere = format!("{} — {m}", heure_locale());
                        }
                        enregistrer("synchro_usb", &*pp);
                    }
                }
                presents = lec.iter().map(|x| x.1).collect();
            }
        });
        SyncUsb { paires, statut, lancer }
    }
}

impl Outil for SyncUsb {
    fn nom(&self) -> &'static str {
        "🔌 Synchro clé USB"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Synchro avec une clé USB");
        ui.weak("Dès que la clé (ou le disque externe) est branchée, le dossier choisi y est recopié : seuls les fichiers nouveaux ou modifiés sont copiés.");
        let lec = lecteurs();
        let avant = self.paires.lock().unwrap().clone();
        let mut v = avant.clone();
        let mut retirer = None;
        for (i, p) in v.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                let branchee = lec.iter().find(|x| x.1 == p.serie);
                ui.horizontal(|ui| {
                    ui.strong(format!("📁 {}  →  🔌 {}\\{}", p.source, if p.nom_cle.is_empty() { "?" } else { &p.nom_cle }, p.sous_dossier));
                    match branchee {
                        Some(x) => ui.colored_label(Color32::LIGHT_GREEN, format!("branchée ({})", x.0)),
                        None => ui.weak("pas branchée"),
                    };
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut p.auto, "Automatique au branchement");
                    ui.checkbox(&mut p.miroir, "Miroir (supprime sur la clé ce qui n'existe plus)");
                    if ui.add_enabled(branchee.is_some(), egui::Button::new("▶ Synchroniser")).clicked() {
                        *self.lancer.lock().unwrap() = Some(i);
                    }
                    if ui.small_button("🗑").clicked() {
                        retirer = Some(i);
                    }
                });
                if !p.derniere.is_empty() {
                    ui.weak(&p.derniere);
                }
            });
        }
        if let Some(i) = retirer {
            v.remove(i);
        }
        ui.separator();
        ui.strong("Nouvelle synchro");
        ui.horizontal_wrapped(|ui| {
            if lec.is_empty() {
                ui.weak("Branche la clé USB pour l'ajouter.");
            }
            for (lettre, serie, nom) in &lec {
                if ui.button(format!("➕ {lettre} {nom}")).clicked() {
                    if let Some(src) = rfd::FileDialog::new().set_title("Dossier à recopier sur la clé").pick_folder() {
                        let sous = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        v.push(Paire { source: src.display().to_string(), serie: *serie, nom_cle: if nom.is_empty() { lettre.clone() } else { nom.clone() }, sous_dossier: sous, auto: true, ..Default::default() });
                    }
                }
            }
        });
        let s = self.statut.lock().unwrap().clone();
        if !s.is_empty() {
            ui.label(s);
        }
        if v != avant {
            enregistrer("synchro_usb", &v);
            *self.paires.lock().unwrap() = v;
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 35 · Surveillance de dossiers
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Changement {
    Ajoute,
    Modifie,
    Supprime,
}

type Cliche = HashMap<PathBuf, (u64, SystemTime)>;

/// Différences entre deux états d'un dossier.
pub fn differences(avant: &Cliche, apres: &Cliche) -> Vec<(Changement, PathBuf)> {
    let mut v: Vec<(Changement, PathBuf)> = apres.iter().filter_map(|(p, m)| match avant.get(p) {
        None => Some((Changement::Ajoute, p.clone())),
        Some(a) if a != m => Some((Changement::Modifie, p.clone())),
        _ => None,
    }).collect();
    v.extend(avant.keys().filter(|p| !apres.contains_key(*p)).map(|p| (Changement::Supprime, p.clone())));
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

fn cliche(d: &Path, sous: bool, c: &mut Cliche, limite: usize) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        if c.len() >= limite {
            return;
        }
        let p = e.path();
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            if sous {
                cliche(&p, sous, c, limite);
            }
        } else if !p.extension().is_some_and(|x| x == "tmp" || x == "part" || x == "crdownload") {
            c.insert(p, (m.len(), m.modified().unwrap_or(UNIX_EPOCH)));
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Surveille {
    pub dossier: String,
    pub sous_dossiers: bool,
    pub notifier: bool,
}

pub struct Surveillance {
    liste: Arc<Mutex<Vec<Surveille>>>,
    journal: Arc<Mutex<Vec<(String, Changement, PathBuf)>>>,
}

impl Surveillance {
    pub fn new() -> Self {
        let liste: Arc<Mutex<Vec<Surveille>>> = Arc::new(Mutex::new(charger("surveillance")));
        let journal: Arc<Mutex<Vec<(String, Changement, PathBuf)>>> = Arc::default();
        let (l, j) = (liste.clone(), journal.clone());
        std::thread::spawn(move || {
            let mut etats: HashMap<String, Cliche> = HashMap::new();
            loop {
                for s in l.lock().unwrap().clone() {
                    let mut c = Cliche::new();
                    cliche(Path::new(&s.dossier), s.sous_dossiers, &mut c, 20_000);
                    if let Some(a) = etats.get(&s.dossier) {
                        let d = differences(a, &c);
                        if !d.is_empty() {
                            if s.notifier {
                                let n = d[0].1.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                                let quoi = match d[0].0 {
                                    Changement::Ajoute => "nouveau",
                                    Changement::Modifie => "modifié",
                                    Changement::Supprime => "supprimé",
                                };
                                let plus = if d.len() > 1 { format!(" (+{} autres)", d.len() - 1) } else { String::new() };
                                crate::systeme::notifier("Dossier surveillé", &format!("{n} : {quoi}{plus}"));
                            }
                            let h = heure_locale();
                            let mut jj = j.lock().unwrap();
                            jj.extend(d.into_iter().map(|(k, p)| (h.clone(), k, p)));
                            let n = jj.len();
                            if n > 500 {
                                jj.drain(..n - 500);
                            }
                        }
                    }
                    etats.insert(s.dossier.clone(), c);
                }
                std::thread::sleep(Duration::from_secs(3));
            }
        });
        Surveillance { liste, journal }
    }
}

impl Outil for Surveillance {
    fn nom(&self) -> &'static str {
        "👁 Surveiller des dossiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn resume(&self) -> Option<String> {
        let n = self.journal.lock().unwrap().len();
        (n > 0).then(|| format!("{n} changement(s) dans les dossiers surveillés"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Surveiller des dossiers");
        ui.weak("Prévient quand un fichier apparaît, change ou disparaît (dossier partagé, Téléchargements, dossier de travail…).");
        let avant = self.liste.lock().unwrap().clone();
        let mut v = avant.clone();
        let mut retirer = None;
        for (i, s) in v.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("📁 {}", s.dossier));
                ui.checkbox(&mut s.sous_dossiers, "sous-dossiers");
                ui.checkbox(&mut s.notifier, "notification");
                if ui.small_button("🗑").clicked() {
                    retirer = Some(i);
                }
            });
        }
        if let Some(i) = retirer {
            v.remove(i);
        }
        if ui.button("➕ Surveiller un dossier…").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                v.push(Surveille { dossier: d.display().to_string(), sous_dossiers: false, notifier: true });
            }
        }
        if v != avant {
            enregistrer("surveillance", &v);
            *self.liste.lock().unwrap() = v;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Derniers changements");
            if ui.small_button("Effacer").clicked() {
                self.journal.lock().unwrap().clear();
            }
        });
        egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
            for (h, k, p) in self.journal.lock().unwrap().iter().rev().take(300) {
                ui.horizontal(|ui| {
                    ui.weak(h);
                    let (t, c) = match k {
                        Changement::Ajoute => ("➕", Color32::LIGHT_GREEN),
                        Changement::Modifie => ("✏", Color32::from_rgb(250, 210, 90)),
                        Changement::Supprime => ("✖", Color32::LIGHT_RED),
                    };
                    ui.colored_label(c, t);
                    if ui.link(p.display().to_string()).clicked() {
                        ouvrir(p.parent().unwrap_or(p));
                    }
                });
            }
        });
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 36 · Renommer des épisodes
// ---------------------------------------------------------------------------------------------------

/// (saison, épisode) trouvés dans un nom de fichier.
pub fn episode(nom: &str) -> Option<(u32, u32)> {
    let n = nom.to_lowercase();
    let motifs = [r"s(\d{1,2})[ ._-]?e(\d{1,3})", r"(?:^|[^\d])(\d{1,2})x(\d{2,3})(?:[^\d]|$)", r"saison[ ._-]?(\d{1,2}).*?[ée]p(?:isode)?[ ._-]?(\d{1,3})"];
    for m in motifs {
        if let Some(c) = regex::Regex::new(m).unwrap().captures(&n) {
            return Some((c[1].parse().ok()?, c[2].parse().ok()?));
        }
    }
    let c = regex::Regex::new(r"(?:[ée]p(?:isode)?|e)[ ._-]?(\d{1,4})(?:[^\d]|$)").unwrap().captures(&n)
        .or_else(|| regex::Regex::new(r"(?:^|[ ._\-\]])(\d{1,3})(?:[ ._\-\[(]|v\d|$)").unwrap().captures(n.split('.').next().unwrap_or(&n)))?;
    Some((1, c[1].parse().ok()?))
}

/// Nom de la série deviné : ce qui précède le numéro, sans les [crochets] ni les points.
pub fn serie_devinee(nom: &str) -> String {
    let sans = regex::Regex::new(r"\[[^\]]*\]|\([^)]*\)").unwrap().replace_all(nom, "");
    let coupe = regex::Regex::new(r"(?i)(s\d{1,2}[ ._-]?e\d|\d{1,2}x\d{2}|saison|[ée]p(isode)?[ ._-]?\d| - \d|[ ._]\d{1,3}[ ._])").unwrap();
    let debut = coupe.find(&sans).map(|m| &sans[..m.start()]).unwrap_or(&sans);
    debut.replace(['.', '_'], " ").trim().trim_end_matches('-').trim().to_string()
}

pub fn nouveau_nom(serie: &str, s: u32, e: u32, ext: &str) -> String {
    let ext = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    if serie.is_empty() { format!("S{s:02}E{e:02}{ext}") } else { format!("{serie} - S{s:02}E{e:02}{ext}") }
}

const VIDEOS: [&str; 11] = ["mkv", "mp4", "avi", "mov", "wmv", "m4v", "webm", "ts", "srt", "ass", "sub"];

pub struct Episodes {
    dossier: Option<PathBuf>,
    serie: String,
    saison: Option<u32>,
    message: String,
}

impl Episodes {
    pub fn new() -> Self {
        Episodes { dossier: None, serie: String::new(), saison: None, message: String::new() }
    }

    fn plan(&self) -> Vec<(PathBuf, Option<String>)> {
        let Some(d) = &self.dossier else { return vec![] };
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| VIDEOS.contains(&x.to_string_lossy().to_lowercase().as_str()))).collect();
        v.sort();
        v.into_iter().map(|p| {
            let n = p.file_name().unwrap().to_string_lossy().into_owned();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            let nn = episode(&n).map(|(s, e)| nouveau_nom(&self.serie, self.saison.unwrap_or(s), e, &ext));
            (p, nn)
        }).collect()
    }
}

impl Outil for Episodes {
    fn nom(&self) -> &'static str {
        "📺 Renommer des épisodes"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Renommer des épisodes de série");
        ui.weak("« [Team] My.Show.S01E03.1080p.WEB.mkv » → « My Show - S01E03.mkv ». Les sous-titres sont renommés pareil.");
        if ui.button("📁 Choisir le dossier de la saison…").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                let premier = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
                    .find(|n| VIDEOS[..9].iter().any(|x| n.to_lowercase().ends_with(x)));
                self.serie = premier.map(|n| serie_devinee(&n)).filter(|s| !s.is_empty()).unwrap_or_else(|| d.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default());
                self.dossier = Some(d);
                self.message.clear();
            }
        }
        if self.dossier.is_none() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Nom de la série");
            ui.text_edit_singleline(&mut self.serie);
            let mut forcer = self.saison.is_some();
            ui.checkbox(&mut forcer, "Forcer la saison");
            if forcer {
                let mut s = self.saison.unwrap_or(1);
                ui.add(egui::DragValue::new(&mut s).range(0..=99));
                self.saison = Some(s);
            } else {
                self.saison = None;
            }
        });
        let plan = self.plan();
        let a_faire = plan.iter().filter(|(p, n)| n.as_ref().is_some_and(|n| p.file_name().unwrap().to_string_lossy() != *n)).count();
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            egui::Grid::new("episodes").striped(true).show(ui, |ui| {
                for (p, n) in &plan {
                    ui.label(p.file_name().unwrap().to_string_lossy());
                    ui.label("→");
                    match n {
                        Some(n) => ui.label(RichText::new(n).color(Color32::LIGHT_GREEN)),
                        None => ui.weak("numéro introuvable, laissé tel quel"),
                    };
                    ui.end_row();
                }
            });
        });
        if ui.add_enabled(a_faire > 0, egui::Button::new(format!("✏ Renommer {a_faire} fichier(s)"))).clicked() {
            let mut annul = vec![];
            let mut err = 0;
            for (p, n) in plan {
                let Some(n) = n else { continue };
                let q = p.with_file_name(&n);
                if q == p {
                    continue;
                }
                if q.exists() || std::fs::rename(&p, &q).is_err() {
                    err += 1;
                } else {
                    annul.push((q.display().to_string(), p.display().to_string()));
                }
            }
            enregistrer("episodes_annuler", &annul);
            self.message = format!("✔ {} renommé(s){}", annul.len(), if err > 0 { format!(", {err} ignoré(s) (nom déjà pris)") } else { String::new() });
        }
        let annul: Vec<(String, String)> = charger("episodes_annuler");
        if !annul.is_empty() && ui.small_button("↩ Annuler le dernier renommage").clicked() {
            for (a, b) in &annul {
                let _ = std::fs::rename(a, b);
            }
            enregistrer::<Vec<(String, String)>>("episodes_annuler", &vec![]);
            self.message = "↩ Annulé.".into();
        }
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 37 · Recherche dans le contenu des fichiers
// ---------------------------------------------------------------------------------------------------

/// Texte d'un .docx / .odt / .pptx / .xlsx (balises XML retirées).
fn texte_bureautique(p: &Path) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(p).ok()?).ok()?;
    let noms: Vec<String> = z.file_names().filter(|n| *n == "word/document.xml" || *n == "content.xml" || n.starts_with("ppt/slides/slide") || *n == "xl/sharedStrings.xml").map(String::from).collect();
    let mut t = String::new();
    for n in noms {
        let mut s = String::new();
        z.by_name(&n).ok()?.read_to_string(&mut s).ok()?;
        t.push_str(&xml_en_texte(&s));
        t.push('\n');
    }
    Some(t)
}

/// Retire les balises : un saut de ligne par paragraphe.
pub fn xml_en_texte(x: &str) -> String {
    let x = regex::Regex::new(r"</(w:p|text:p|a:p|si)>").unwrap().replace_all(x, "\n");
    let x = regex::Regex::new(r"<[^>]*>").unwrap().replace_all(&x, "");
    x.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

/// Lignes qui contiennent la recherche (sans tenir compte des majuscules ni des accents).
pub fn lignes_trouvees(texte: &str, q: &str) -> Vec<(usize, String)> {
    let q = simple(q);
    texte.lines().enumerate().filter(|(_, l)| simple(l).contains(&q))
        .map(|(i, l)| (i + 1, l.trim().chars().take(160).collect())).take(5).collect()
}

fn texte_de(p: &Path) -> Option<String> {
    let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    let m = std::fs::metadata(p).ok()?;
    match ext.as_str() {
        "docx" | "odt" | "pptx" | "xlsx" | "ods" | "odp" => texte_bureautique(p),
        "pdf" if m.len() < 50 << 20 => {
            let d = lopdf::Document::load(p).ok()?;
            let pages: Vec<u32> = d.get_pages().keys().copied().collect();
            d.extract_text(&pages).ok()
        }
        "exe" | "dll" | "zip" | "7z" | "rar" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "mp3" | "mp4" | "mkv" | "iso" | "msi" | "bin" => None,
        _ if m.len() < 20 << 20 => {
            let b = std::fs::read(p).ok()?;
            if b.iter().take(4096).any(|c| *c == 0) {
                return None; // binaire
            }
            Some(String::from_utf8(b).unwrap_or_else(|e| e.into_bytes().iter().map(|c| *c as char).collect()))
        }
        _ => None,
    }
}

type Resultats = Arc<Mutex<(Vec<(PathBuf, Vec<(usize, String)>)>, u32, bool)>>;

pub struct RechercheContenu {
    dossier: String,
    q: String,
    r: Resultats,
    arret: Arc<AtomicBool>,
}

impl RechercheContenu {
    pub fn new() -> Self {
        RechercheContenu { dossier: std::env::var("USERPROFILE").map(|u| format!("{u}\\Documents")).unwrap_or_default(), q: String::new(), r: Arc::default(), arret: Arc::default() }
    }
}

fn parcourir(d: &Path, q: &str, r: &Resultats, arret: &AtomicBool) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        if arret.load(Relaxed) {
            return;
        }
        let p = e.path();
        let n = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !n.starts_with('.') && n != "node_modules" && n != "AppData" && n != "$Recycle.Bin" && n != "target" {
                parcourir(&p, q, r, arret);
            }
            continue;
        }
        r.lock().unwrap().1 += 1;
        let mut l = texte_de(&p).map(|t| lignes_trouvees(&t, q)).unwrap_or_default();
        if l.is_empty() && simple(&n).contains(&simple(q)) {
            l.push((0, "(dans le nom du fichier)".into()));
        }
        if !l.is_empty() {
            r.lock().unwrap().0.push((p, l));
        }
    }
}

impl Outil for RechercheContenu {
    fn nom(&self) -> &'static str {
        "🔍 Chercher dans les fichiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chercher un mot dans le contenu des fichiers");
        ui.weak("Fouille l'intérieur des textes, Word, Excel, PowerPoint, LibreOffice et PDF — pas seulement les noms.");
        let en_cours = self.r.lock().unwrap().2;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.dossier).desired_width(320.0));
            if ui.button("📁").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.dossier = d.display().to_string();
                }
            }
        });
        ui.horizontal(|ui| {
            let rep = ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("mot ou phrase").desired_width(320.0));
            let go = rep.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if en_cours {
                if ui.button("⏹ Arrêter").clicked() {
                    self.arret.store(true, Relaxed);
                }
            } else if (ui.button("🔍 Chercher").clicked() || go) && self.q.trim().len() >= 2 {
                self.arret.store(false, Relaxed);
                *self.r.lock().unwrap() = (vec![], 0, true);
                let (d, q, r, a) = (PathBuf::from(&self.dossier), self.q.trim().to_string(), self.r.clone(), self.arret.clone());
                std::thread::spawn(move || {
                    parcourir(&d, &q, &r, &a);
                    r.lock().unwrap().2 = false;
                });
            }
        });
        let r = self.r.lock().unwrap();
        ui.horizontal(|ui| {
            if r.2 {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
            ui.label(format!("{} fichier(s) lus · {} trouvé(s)", r.1, r.0.len()));
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (p, l) in &r.0 {
                ui.horizontal(|ui| {
                    if ui.link(RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).strong()).clicked() {
                        ouvrir(p);
                    }
                    if ui.small_button("📁").on_hover_text("Afficher dans l'explorateur").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
                    }
                    ui.weak(p.parent().map(|x| x.display().to_string()).unwrap_or_default());
                });
                for (n, t) in l {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        if *n > 0 {
                            ui.weak(format!("l.{n}"));
                        }
                        ui.label(t);
                    });
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 38 · Corbeille automatique
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageCorbeille {
    pub actif: bool,
    pub jours: u32,
    pub dernier: String,
}

impl Default for ReglageCorbeille {
    fn default() -> Self {
        ReglageCorbeille { actif: false, jours: 30, dernier: String::new() }
    }
}

/// Éléments de la corbeille plus vieux que `jours`.
fn trop_vieux(jours: u32) -> Vec<crate::fichiers2::Supprime> {
    let limite = maintenant() - jours as i64 * 86_400;
    crate::fichiers2::corbeille().into_iter().filter(|s| s.date < limite).collect()
}

fn vider_vieux(jours: u32) -> (u32, u64) {
    let (mut n, mut o) = (0, 0);
    for s in trop_vieux(jours) {
        // gardé encore 30 jours dans la corbeille de sécurité d'Outils, sous son nom d'origine
        let nom = s.origine.file_name().map(|n| n.to_owned()).unwrap_or_default();
        let renomme = s.donnees.with_file_name(&nom);
        let r = std::fs::rename(&s.donnees, &renomme).map_err(|e| e.to_string()).and_then(|_| crate::fichiers5::mettre_a_l_abri_vers(&renomme, &s.origine, "Corbeille automatique"));
        if r.is_ok() {
            let _ = std::fs::remove_file(&s.info);
            n += 1;
            o += s.taille;
        }
    }
    (n, o)
}

pub struct CorbeilleAuto {
    r: Arc<Mutex<ReglageCorbeille>>,
    apercu: Option<(usize, u64)>,
}

impl CorbeilleAuto {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageCorbeille>> = Arc::new(Mutex::new(charger("corbeille_auto")));
        let r2 = r.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(120));
            let c = r2.lock().unwrap().clone();
            if c.actif {
                let (n, o) = vider_vieux(c.jours);
                if n > 0 {
                    let mut x = r2.lock().unwrap();
                    x.dernier = format!("{} : {n} élément(s) supprimé(s), {} libérés", heure_locale(), taille(o));
                    enregistrer("corbeille_auto", &*x);
                    crate::coeur::noter("Corbeille automatique", &x.dernier);
                }
            }
            std::thread::sleep(Duration::from_secs(3600));
        });
        CorbeilleAuto { r, apercu: None }
    }
}

impl Outil for CorbeilleAuto {
    fn nom(&self) -> &'static str {
        "🗑 Corbeille automatique"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Corbeille automatique");
        ui.weak("Supprime définitivement ce qui traîne dans la corbeille depuis trop longtemps (vérifié toutes les heures).");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Vider ce qui est dans la corbeille depuis plus de");
            ui.add(egui::DragValue::new(&mut r.jours).range(1..=365).suffix(" jours"));
        });
        if ui.button("🔎 Voir ce qui serait supprimé maintenant").clicked() {
            let v = trop_vieux(r.jours);
            self.apercu = Some((v.len(), v.iter().map(|s| s.taille).sum()));
        }
        if let Some((n, o)) = self.apercu {
            ui.horizontal(|ui| {
                ui.label(format!("{n} élément(s), {}", taille(o)));
                if n > 0 && ui.button("🗑 Supprimer maintenant").clicked() {
                    let (n, o) = vider_vieux(r.jours);
                    r.dernier = format!("{} : {n} élément(s) supprimé(s), {} libérés", heure_locale(), taille(o));
                    self.apercu = None;
                }
            });
        }
        if !r.dernier.is_empty() {
            ui.weak(format!("Dernier passage : {}", r.dernier));
        }
        if r != avant {
            enregistrer("corbeille_auto", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 39 · Installateurs oubliés dans Téléchargements
// ---------------------------------------------------------------------------------------------------

pub fn est_installateur(nom: &str) -> bool {
    let n = nom.to_lowercase();
    [".msi", ".msix", ".msixbundle", ".appx", ".appxbundle"].iter().any(|e| n.ends_with(e))
        || n.ends_with(".exe") // dans Téléchargements, un .exe est presque toujours un installateur
        || n.ends_with(".iso")
}

pub struct Installateurs {
    jours: u32,
    liste: Option<Vec<(PathBuf, u64, u32, bool)>>, // chemin, taille, âge en jours, coché
    message: String,
}

impl Installateurs {
    pub fn new() -> Self {
        Installateurs { jours: 7, liste: None, message: String::new() }
    }

    fn analyser(&mut self) {
        let now = SystemTime::now();
        let mut v = vec![];
        let d = telechargements();
        let mut dossiers = vec![d.clone()];
        dossiers.extend(std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        for dd in dossiers {
            for e in std::fs::read_dir(&dd).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                let Ok(m) = e.metadata() else { continue };
                if m.is_file() && est_installateur(&n) {
                    let age = m.modified().ok().and_then(|t| now.duration_since(t).ok()).map(|d| (d.as_secs() / 86_400) as u32).unwrap_or(0);
                    v.push((e.path(), m.len(), age, age >= self.jours));
                }
            }
        }
        v.sort_by(|a, b| b.1.cmp(&a.1));
        self.liste = Some(v);
    }
}

impl Outil for Installateurs {
    fn nom(&self) -> &'static str {
        "📦 Installateurs oubliés"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Installateurs oubliés dans Téléchargements");
        ui.weak("Les setup.exe, .msi et .iso déjà utilisés ne servent plus : on peut les jeter (ils vont à la corbeille).");
        ui.horizontal(|ui| {
            ui.label("Cocher ceux de plus de");
            ui.add(egui::DragValue::new(&mut self.jours).range(0..=365).suffix(" jours"));
            if ui.button("🔎 Analyser").clicked() || self.liste.is_none() {
                self.analyser();
            }
        });
        let Some(l) = &mut self.liste else { return };
        if l.is_empty() {
            ui.label("✔ Rien à nettoyer.");
            return;
        }
        let choisi: u64 = l.iter().filter(|x| x.3).map(|x| x.1).sum();
        egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
            egui::Grid::new("installateurs").striped(true).show(ui, |ui| {
                for (p, t, age, c) in l.iter_mut() {
                    ui.checkbox(c, p.file_name().unwrap_or_default().to_string_lossy());
                    ui.label(taille(*t));
                    ui.weak(if *age == 0 { "aujourd'hui".into() } else { format!("il y a {age} j") });
                    ui.end_row();
                }
            });
        });
        if ui.add_enabled(choisi > 0, egui::Button::new(format!("🗑 Mettre à la corbeille ({})", taille(choisi)))).clicked() {
            let n = l.iter().filter(|x| x.3).filter(|x| crate::disque::corbeille(&x.0)).count();
            self.message = format!("✔ {n} fichier(s) à la corbeille, {} libérés.", taille(choisi));
            self.analyser();
        }
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 40 · Vérifier une empreinte (SHA-256…)
// ---------------------------------------------------------------------------------------------------

pub fn normaliser_empreinte(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase()
}

/// Algorithme d'après la longueur de l'empreinte attendue.
pub fn algo_pour(e: &str) -> Option<&'static str> {
    match normaliser_empreinte(e).len() {
        40 => Some("SHA-1"),
        64 => Some("SHA-256"),
        128 => Some("SHA-512"),
        _ => None,
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn calculer(p: &Path, progres: &Mutex<f32>) -> std::io::Result<[String; 3]> {
    use sha2::Digest;
    let total = std::fs::metadata(p)?.len().max(1);
    let mut f = std::fs::File::open(p)?;
    let (mut a, mut b, mut c) = (sha1::Sha1::new(), sha2::Sha256::new(), sha2::Sha512::new());
    let mut buf = vec![0u8; 1 << 20];
    let mut lu = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        a.update(&buf[..n]);
        b.update(&buf[..n]);
        c.update(&buf[..n]);
        lu += n as u64;
        *progres.lock().unwrap() = lu as f32 / total as f32;
    }
    Ok([hex(&a.finalize()), hex(&b.finalize()), hex(&c.finalize())])
}

pub struct Empreinte {
    fichier: Option<PathBuf>,
    attendu: String,
    progres: Arc<Mutex<f32>>,
    r: Arc<Mutex<Option<Result<[String; 3], String>>>>,
}

impl Empreinte {
    pub fn new() -> Self {
        Empreinte { fichier: None, attendu: String::new(), progres: Arc::default(), r: Arc::default() }
    }

    fn lancer(&mut self, p: PathBuf) {
        // un fichier .sha256 à côté ? on le prend comme empreinte attendue
        for ext in ["sha256", "sha256sum", "sha512", "sha1"] {
            let s = PathBuf::from(format!("{}.{ext}", p.display()));
            if let Ok(t) = std::fs::read_to_string(&s) {
                self.attendu = t.split_whitespace().next().unwrap_or("").to_string();
            }
        }
        self.fichier = Some(p.clone());
        *self.r.lock().unwrap() = None;
        let (pr, r) = (self.progres.clone(), self.r.clone());
        std::thread::spawn(move || *r.lock().unwrap() = Some(calculer(&p, &pr).map_err(|e| e.to_string())));
    }
}

impl Outil for Empreinte {
    fn nom(&self) -> &'static str {
        "🔏 Vérifier un fichier (SHA-256)"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Vérifier qu'un fichier téléchargé est intact");
        ui.weak("Compare l'empreinte du fichier avec celle donnée par le site (SHA-256, SHA-1 ou SHA-512). Si elles diffèrent, le fichier est abîmé ou a été modifié.");
        if let Some(f) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.lancer(f);
        }
        ui.horizontal(|ui| {
            if ui.button("📄 Choisir le fichier…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    self.lancer(p);
                }
            }
            ui.weak("(ou glisse-le sur la fenêtre)");
        });
        ui.horizontal(|ui| {
            ui.label("Empreinte donnée par le site");
            ui.add(egui::TextEdit::singleline(&mut self.attendu).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
        let Some(f) = &self.fichier else { return };
        ui.label(RichText::new(f.file_name().unwrap_or_default().to_string_lossy()).strong());
        match self.r.lock().unwrap().clone() {
            None => {
                ui.add(egui::ProgressBar::new(*self.progres.lock().unwrap()).show_percentage());
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            Some(Ok(h)) => {
                let att = normaliser_empreinte(&self.attendu);
                for (n, v) in ["SHA-1", "SHA-256", "SHA-512"].iter().zip(&h) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{n:8}"));
                        ui.monospace(if v.len() > 64 { format!("{}…", &v[..64]) } else { v.clone() });
                        if ui.small_button("📋").clicked() {
                            ui.ctx().copy_text(v.clone());
                        }
                    });
                }
                if !att.is_empty() {
                    match algo_pour(&att) {
                        None => {
                            ui.colored_label(Color32::from_rgb(250, 210, 90), "Empreinte attendue de longueur inconnue (MD5 n'est pas géré : demande la SHA-256 au site).");
                        }
                        Some(a) => {
                            let i = ["SHA-1", "SHA-256", "SHA-512"].iter().position(|x| *x == a).unwrap();
                            if h[i] == att {
                                ui.colored_label(Color32::LIGHT_GREEN, RichText::new(format!("✔ Identique ({a}) : le fichier est intact.")).size(20.0));
                            } else {
                                ui.colored_label(Color32::LIGHT_RED, RichText::new(format!("✖ Différent ({a}) : ne l'ouvre pas, retélécharge-le.")).size(20.0));
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 41 · Gestionnaire de téléchargements
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EtatDl {
    Attente,
    EnCours,
    Pause,
    Fini,
    Erreur(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Dl {
    #[serde(default)]
    pub id: u64,
    pub url: String,
    pub dest: PathBuf,
    pub total: u64,
    pub recu: u64,
    pub etat: EtatDl,
    #[serde(skip)]
    pub vitesse: f64,
}

/// Nom du fichier : Content-Disposition, sinon fin de l'URL (sans paramètres, décodée).
pub fn nom_fichier(url: &str, disposition: Option<&str>) -> String {
    if let Some(d) = disposition {
        if let Some(c) = regex::Regex::new(r#"(?i)filename\*=UTF-8''([^;]+)"#).unwrap().captures(d) {
            return decoder_url(c[1].trim());
        }
        if let Some(c) = regex::Regex::new(r#"(?i)filename="?([^";]+)"?"#).unwrap().captures(d) {
            return c[1].trim().to_string();
        }
    }
    let chemin = url.split(['?', '#']).next().unwrap_or("").trim_end_matches('/');
    let n = decoder_url(chemin.rsplit('/').next().unwrap_or(""));
    if n.is_empty() || chemin.matches('/').count() <= 2 { "telechargement".into() } else { n }
}

pub fn decoder_url(s: &str) -> String {
    let b = s.as_bytes();
    let mut v = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(x) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                v.push(x);
                i += 3;
                continue;
            }
        }
        v.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    let n = String::from_utf8_lossy(&v).into_owned();
    n.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) { '_' } else { c }).collect()
}

fn libre(p: PathBuf) -> PathBuf {
    if !p.exists() && !PathBuf::from(format!("{}.part", p.display())).exists() {
        return p;
    }
    let (s, e) = (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p.extension().map(|x| format!(".{}", x.to_string_lossy())).unwrap_or_default());
    (2..).map(|i| p.with_file_name(format!("{s} ({i}){e}"))).find(|q| !q.exists() && !PathBuf::from(format!("{}.part", q.display())).exists()).unwrap()
}

type Liste = Arc<Mutex<Vec<Dl>>>;

fn sauver(l: &[Dl]) {
    let v: Vec<&Dl> = l.iter().filter(|d| d.etat != EtatDl::Fini).collect();
    enregistrer("telechargements", &v);
}

/// Modifie le téléchargement `id` (s'il est encore dans la liste).
fn avec(l: &Liste, id: u64, f: impl FnOnce(&mut Dl)) {
    if let Some(d) = l.lock().unwrap().iter_mut().find(|d| d.id == id) {
        f(d);
    }
}

fn telecharger(l: Liste, id: u64, arret: Arc<AtomicBool>) {
    let Some((url, dest)) = l.lock().unwrap().iter().find(|d| d.id == id).map(|x| (x.url.clone(), x.dest.clone())) else { return };
    let part = PathBuf::from(format!("{}.part", dest.display()));
    let recu = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let r = (|| -> Result<(), String> {
        let ag = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).timeout_read(Duration::from_secs(60)).user_agent("Mozilla/5.0 Outils").build();
        let mut req = ag.get(&url);
        if recu > 0 {
            req = req.set("Range", &format!("bytes={recu}-"));
        }
        let rep = req.call().map_err(|e| e.to_string())?;
        let reprise = rep.status() == 206;
        let longueur: u64 = rep.header("Content-Length").and_then(|x| x.parse().ok()).unwrap_or(0);
        let mut deja = if reprise { recu } else { 0 };
        avec(&l, id, |d| {
            d.total = if longueur > 0 { deja + longueur } else { 0 };
            d.recu = deja;
        });
        let mut f = std::fs::OpenOptions::new().create(true).write(true).append(reprise).truncate(!reprise).open(&part).map_err(|e| e.to_string())?;
        let mut lecteur = rep.into_reader();
        let mut buf = vec![0u8; 256 << 10];
        let (mut t0, mut o0) = (std::time::Instant::now(), deja);
        loop {
            if arret.load(Relaxed) {
                f.flush().ok();
                return Err("pause".into());
            }
            let n = lecteur.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            deja += n as u64;
            let dt = t0.elapsed().as_secs_f64();
            let v = (dt > 1.0).then(|| (deja - o0) as f64 / dt);
            avec(&l, id, |d| {
                d.recu = deja;
                if let Some(v) = v {
                    d.vitesse = v;
                }
            });
            if v.is_some() {
                t0 = std::time::Instant::now();
                o0 = deja;
            }
        }
        drop(f);
        std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
        Ok(())
    })();
    let etat = match r {
        Ok(()) => {
            crate::systeme::notifier("Téléchargement terminé", &dest.file_name().unwrap_or_default().to_string_lossy());
            EtatDl::Fini
        }
        Err(e) if e == "pause" => EtatDl::Pause,
        Err(e) => EtatDl::Erreur(e),
    };
    avec(&l, id, |d| {
        d.vitesse = 0.0;
        d.etat = etat;
    });
    sauver(&l.lock().unwrap());
}

pub struct Telechargements {
    l: Liste,
    arrets: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    url: String,
    simultanes: Arc<Mutex<usize>>,
}

impl Telechargements {
    pub fn new() -> Self {
        let mut v: Vec<Dl> = charger("telechargements");
        for (k, d) in v.iter_mut().enumerate() {
            d.id = k as u64 + 1;
            if d.etat == EtatDl::EnCours {
                d.etat = EtatDl::Attente; // reprise au démarrage
            }
        }
        let l: Liste = Arc::new(Mutex::new(v));
        let arrets: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>> = Arc::default();
        let simultanes = Arc::new(Mutex::new(3usize));
        let (l2, a2, s2) = (l.clone(), arrets.clone(), simultanes.clone());
        std::thread::spawn(move || loop {
            {
                let mut ll = l2.lock().unwrap();
                let actifs = ll.iter().filter(|d| d.etat == EtatDl::EnCours).count();
                if actifs < *s2.lock().unwrap() {
                    if let Some(i) = ll.iter().position(|d| d.etat == EtatDl::Attente) {
                        ll[i].etat = EtatDl::EnCours;
                        let id = ll[i].id;
                        let a = Arc::new(AtomicBool::new(false));
                        a2.lock().unwrap().insert(id, a.clone());
                        let l3 = l2.clone();
                        std::thread::spawn(move || telecharger(l3, id, a));
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        });
        Telechargements { l, arrets, url: String::new(), simultanes }
    }

    fn ajouter(&mut self, url: &str) {
        let url = url.trim().to_string();
        if !url.starts_with("http") {
            return;
        }
        let l = self.l.clone();
        // en fond : une requête HEAD donne le vrai nom quand le serveur l'indique
        std::thread::spawn(move || {
            let disp = ureq::AgentBuilder::new().timeout(Duration::from_secs(8)).build().head(&url).call().ok().and_then(|r| r.header("Content-Disposition").map(String::from));
            let mut l = l.lock().unwrap();
            let dest = libre(telechargements().join(nom_fichier(&url, disp.as_deref())));
            let id = l.iter().map(|d| d.id).max().unwrap_or(0) + 1;
            l.push(Dl { id, url, dest, total: 0, recu: 0, etat: EtatDl::Attente, vitesse: 0.0 });
            sauver(&l);
        });
    }
}

impl Outil for Telechargements {
    fn nom(&self) -> &'static str {
        "⏬ Gestionnaire de téléchargements"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn resume(&self) -> Option<String> {
        let n = self.l.lock().unwrap().iter().filter(|d| d.etat == EtatDl::EnCours).count();
        (n > 0).then(|| format!("⏬ {n} téléchargement(s) en cours"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Gestionnaire de téléchargements");
        ui.weak("Colle un lien direct vers un fichier : téléchargement en pause / reprise, même après avoir fermé Outils ou perdu la connexion.");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("https://…").desired_width(420.0));
            let entree = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("⬇ Télécharger").clicked() || entree {
                let u = std::mem::take(&mut self.url);
                for x in u.split_whitespace() {
                    self.ajouter(x);
                }
            }
            if ui.button("📋 Depuis le presse-papiers").clicked() {
                if let Some(t) = crate::tradinstant::win::lire_presse_papiers() {
                    for x in t.split_whitespace().filter(|x| x.starts_with("http")) {
                        self.ajouter(x);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("En même temps :");
            let mut s = *self.simultanes.lock().unwrap();
            if ui.add(egui::DragValue::new(&mut s).range(1..=8)).changed() {
                *self.simultanes.lock().unwrap() = s;
            }
            if ui.button("📁 Ouvrir Téléchargements").clicked() {
                ouvrir(&telechargements());
            }
        });
        ui.separator();
        let mut ll = self.l.lock().unwrap();
        let mut retirer = None;
        let mut changer = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for d in ll.iter_mut().rev() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(d.dest.file_name().unwrap_or_default().to_string_lossy());
                        ui.weak(d.url.chars().take(70).collect::<String>());
                    });
                    let f = if d.total > 0 { d.recu as f32 / d.total as f32 } else { 0.0 };
                    ui.horizontal(|ui| {
                        let txt = match &d.etat {
                            EtatDl::Attente => "en attente".to_string(),
                            EtatDl::EnCours => format!("{} / {} · {}/s", taille(d.recu), if d.total > 0 { taille(d.total) } else { "?".into() }, taille(d.vitesse as u64)),
                            EtatDl::Pause => format!("en pause · {}", taille(d.recu)),
                            EtatDl::Fini => format!("✔ terminé · {}", taille(d.recu)),
                            EtatDl::Erreur(e) => format!("✖ {e}"),
                        };
                        ui.add(egui::ProgressBar::new(if d.etat == EtatDl::Fini { 1.0 } else { f }).desired_width(260.0).text(txt));
                        match d.etat {
                            EtatDl::EnCours => {
                                if ui.small_button("⏸").clicked() {
                                    if let Some(a) = self.arrets.lock().unwrap().get(&d.id) {
                                        a.store(true, Relaxed);
                                    }
                                }
                            }
                            EtatDl::Pause | EtatDl::Erreur(_) => {
                                if ui.small_button("▶").clicked() {
                                    d.etat = EtatDl::Attente;
                                    changer = true;
                                }
                            }
                            EtatDl::Fini => {
                                if ui.small_button("📂").clicked() {
                                    ouvrir(&d.dest);
                                }
                            }
                            EtatDl::Attente => {}
                        }
                        if d.etat != EtatDl::EnCours && ui.small_button("✖").on_hover_text("Retirer de la liste").clicked() {
                            retirer = Some(d.id);
                        }
                    });
                });
            }
        });
        if let Some(id) = retirer {
            if let Some(d) = ll.iter().find(|d| d.id == id) {
                if d.etat != EtatDl::Fini {
                    let _ = std::fs::remove_file(format!("{}.part", d.dest.display()));
                }
            }
            ll.retain(|d| d.id != id);
            changer = true;
        }
        if changer {
            sauver(&ll);
        }
        if ll.iter().any(|d| d.etat == EtatDl::EnCours || d.etat == EtatDl::Attente) {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synchro_et_surveillance() {
        let t = UNIX_EPOCH + Duration::from_secs(1000);
        assert!(a_copier((5, t), None));
        assert!(!a_copier((5, t), Some((5, t + Duration::from_secs(1)))));
        assert!(!a_copier((5, t + Duration::from_secs(1)), Some((5, t))));
        assert!(a_copier((5, t + Duration::from_secs(10)), Some((5, t))));
        assert!(a_copier((6, t), Some((5, t))));
        let mut a = Cliche::new();
        a.insert("x".into(), (1, t));
        a.insert("y".into(), (1, t));
        let mut b = Cliche::new();
        b.insert("x".into(), (2, t));
        b.insert("z".into(), (1, t));
        assert_eq!(differences(&a, &b), vec![(Changement::Modifie, "x".into()), (Changement::Supprime, "y".into()), (Changement::Ajoute, "z".into())]);
    }

    #[test]
    fn episodes() {
        assert_eq!(episode("[Team] My.Show.S01E03.1080p.WEB.mkv"), Some((1, 3)));
        assert_eq!(episode("show 2x05 hdtv.avi"), Some((2, 5)));
        assert_eq!(episode("One Piece - Episode 1071 [1080p].mkv"), Some((1, 1071)));
        assert_eq!(episode("[Sub] Frieren - 07 (1080p).mkv"), Some((1, 7)));
        assert_eq!(episode("Saison 3 épisode 12.mp4"), Some((3, 12)));
        assert_eq!(serie_devinee("[Team] My.Show.S01E03.1080p.WEB.mkv"), "My Show");
        assert_eq!(serie_devinee("[Sub] Frieren - 07 (1080p).mkv"), "Frieren");
        assert_eq!(nouveau_nom("My Show", 1, 3, "mkv"), "My Show - S01E03.mkv");
    }

    #[test]
    fn contenu_empreinte_dl() {
        assert_eq!(xml_en_texte("<w:p><w:r><w:t>Salut &amp; bye</w:t></w:r></w:p><w:p><w:t>2</w:t></w:p>"), "Salut & bye\n2\n");
        assert_eq!(normaliser_empreinte(" AB cd:12 "), "abcd12");
        assert_eq!(algo_pour(&"a".repeat(64)), Some("SHA-256"));
        assert_eq!(algo_pour("abc"), None);
        assert!(est_installateur("VSCodeUserSetup-x64.exe"));
        assert!(est_installateur("python.msi"));
        assert!(!est_installateur("photo.jpg"));
        assert_eq!(nom_fichier("https://ex.com/a/b/Mon%20Fichier.zip?x=1", None), "Mon Fichier.zip");
        assert_eq!(nom_fichier("https://ex.com/dl?id=3", Some("attachment; filename=\"vrai.exe\"")), "vrai.exe");
        assert_eq!(nom_fichier("https://ex.com/", None), "telechargement");
        assert_eq!(nom_fichier("https://ex.com/x", Some("attachment; filename*=UTF-8''caf%C3%A9.txt")), "café.txt");
    }
}
