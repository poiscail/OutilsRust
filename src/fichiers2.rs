//! Fichiers : archives (82), destructeur (88), récupération depuis la corbeille (89), gros fichiers oubliés (90),
//! déplacer un dossier sur un autre disque sans le casser (91), dossiers favoris (92).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const SANS_FENETRE: u32 = 0x0800_0000;

type Etat = Arc<Mutex<(bool, String)>>;

fn en_fond(e: &Etat, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, "En cours…".into());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

fn afficher_etat(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (occupe, m) = e.lock().unwrap().clone();
    if occupe {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    occupe
}

// ---------------------------------------------------------------------------------------------------
// 82 · Archives
// ---------------------------------------------------------------------------------------------------

fn sept_zip() -> Option<PathBuf> {
    let pf = PathBuf::from(std::env::var("ProgramFiles").unwrap_or("C:\\Program Files".into())).join(r"7-Zip\7z.exe");
    if pf.exists() {
        return Some(pf);
    }
    Command::new("7z").creation_flags(SANS_FENETRE).output().ok().map(|_| PathBuf::from("7z"))
}

pub fn extraire(a: &Path) -> Result<PathBuf, String> {
    let dest = a.with_extension("");
    let dest = if dest.exists() { a.with_file_name(format!("{} (extrait)", dest.file_name().unwrap_or_default().to_string_lossy())) } else { dest };
    let ext = a.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    if ext == "zip" {
        let mut z = zip::ZipArchive::new(std::fs::File::open(a).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        z.extract(&dest).map_err(|e| e.to_string())?;
        return Ok(dest);
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    if ext == "7z" {
        // décompresseur 7z intégré (pur Rust)
        return sevenz_rust2::decompress_file(a, &dest).map(|_| dest.clone()).map_err(|e| format!("7z : {e}"));
    }
    // rar, tar, gz, xz, bz2, iso, cab… : tar.exe intégré à Windows (libarchive)
    let tar = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join(r"System32\tar.exe");
    let o = Command::new(tar).arg("-xf").arg(a).arg("-C").arg(&dest).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        return Ok(dest);
    }
    // dernier recours : 7-Zip s'il est installé
    if let Some(s) = sept_zip() {
        let o = Command::new(s).arg("x").arg(a).arg(format!("-o{}", dest.display())).arg("-y").creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
        if o.status.success() {
            return Ok(dest);
        }
    }
    let _ = std::fs::remove_dir(&dest);
    Err(if ext == "rar" {
        "ce .rar n'a pas pu être ouvert : Windows 11 (version 23H2 ou plus récente) sait les lire tout seul ; sur un Windows plus ancien, il faut 7-Zip.".into()
    } else {
        String::from_utf8_lossy(&o.stderr).trim().to_string()
    })
}

pub struct Archives {
    liste: Vec<PathBuf>,
    e: Etat,
}

impl Archives {
    pub fn new() -> Self {
        Archives { liste: vec![], e: Arc::default() }
    }
}

impl Outil for Archives {
    fn nom(&self) -> &'static str {
        "📦 Archives (zip, rar, 7z)"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Archives");
        self.liste.extend(ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>()));
        ui.horizontal(|ui| {
            if ui.button("➕ Fichiers…").clicked() {
                self.liste.extend(rfd::FileDialog::new().pick_files().unwrap_or_default());
            }
            if ui.button("➕ Dossier…").clicked() {
                self.liste.extend(rfd::FileDialog::new().pick_folder());
            }
            if ui.button("Vider").clicked() {
                self.liste.clear();
            }
            match sept_zip() {
                Some(_) => ui.weak("zip, 7z, rar, tar, gz, iso…"),
                None => ui.weak("zip, 7z, tar, gz, iso… et rar (Windows 11 récent) : rien à installer"),
            };
        });
        let mut retirer = None;
        for (i, p) in self.liste.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("✖").clicked() {
                    retirer = Some(i);
                }
                ui.label(if p.is_dir() { "📁" } else { "📄" });
                ui.label(p.display().to_string());
            });
        }
        if let Some(i) = retirer {
            self.liste.remove(i);
        }
        let occupe = afficher_etat(ui, &self.e);
        ui.horizontal(|ui| {
            let archives: Vec<PathBuf> = self.liste.iter().filter(|p| p.is_file() && ["zip", "rar", "7z", "tar", "gz", "tgz", "xz", "bz2", "cab", "iso"].contains(&p.extension().unwrap_or_default().to_string_lossy().to_lowercase().as_str())).cloned().collect();
            if ui.add_enabled(!occupe && !archives.is_empty(), egui::Button::new(format!("📤 Extraire ({})", archives.len()))).clicked() {
                en_fond(&self.e, move || {
                    let mut ok = vec![];
                    for a in &archives {
                        ok.push(extraire(a).map(|d| d.display().to_string()).unwrap_or_else(|e| format!("{} : {e}", a.display())));
                    }
                    Ok(ok.join("\n"))
                });
            }
            if ui.add_enabled(!occupe && !self.liste.is_empty(), egui::Button::new("🗜 Créer un zip")).clicked() {
                if let Some(dest) = rfd::FileDialog::new().add_filter("Zip", &["zip"]).set_file_name("Archive.zip").save_file() {
                    let l = self.liste.clone();
                    en_fond(&self.e, move || {
                        let mut z = zip::ZipWriter::new(std::fs::File::create(&dest).map_err(|e| e.to_string())?);
                        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
                        for racine in &l {
                            let base = racine.parent().unwrap_or(Path::new("")).to_path_buf();
                            let mut pile = vec![racine.clone()];
                            while let Some(p) = pile.pop() {
                                if p.is_dir() {
                                    pile.extend(std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()));
                                } else if let Ok(mut f) = std::fs::File::open(&p) {
                                    let rel = p.strip_prefix(&base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                                    z.start_file(rel, opt).map_err(|e| e.to_string())?;
                                    std::io::copy(&mut f, &mut z).map_err(|e| e.to_string())?;
                                }
                            }
                        }
                        z.finish().map_err(|e| e.to_string())?;
                        Ok(format!("✔ {}", dest.display()))
                    });
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 88 · Destructeur
// ---------------------------------------------------------------------------------------------------

/// Écrase le fichier avec du hasard, le renomme, puis le supprime.
pub fn detruire_fichier(p: &Path) -> std::io::Result<()> {
    use std::io::{Seek, Write};
    let n = p.metadata()?.len();
    {
        let mut f = std::fs::OpenOptions::new().write(true).open(p)?;
        let mut buf = vec![0u8; 1 << 20];
        let mut reste = n;
        while reste > 0 {
            let k = reste.min(buf.len() as u64) as usize;
            let _ = getrandom::getrandom(&mut buf[..k]);
            f.write_all(&buf[..k])?;
            reste -= k as u64;
        }
        f.flush()?;
        f.sync_all()?;
        f.seek(std::io::SeekFrom::Start(0))?;
        f.set_len(0)?;
    }
    let anonyme = p.with_file_name(format!("{:016x}", crate::outil::hasard(u64::MAX)));
    std::fs::rename(p, &anonyme)?;
    std::fs::remove_file(anonyme)
}

pub fn detruire(p: &Path) -> std::io::Result<u32> {
    if p.is_dir() {
        let mut n = 0;
        for e in std::fs::read_dir(p)?.flatten() {
            n += detruire(&e.path())?;
        }
        std::fs::remove_dir(p)?;
        Ok(n)
    } else {
        detruire_fichier(p).map(|_| 1)
    }
}

pub struct Destructeur {
    liste: Vec<PathBuf>,
    confirme: bool,
    e: Etat,
}

impl Destructeur {
    pub fn new() -> Self {
        Destructeur { liste: vec![], confirme: false, e: Arc::default() }
    }
}

impl Outil for Destructeur {
    fn nom(&self) -> &'static str {
        "🔥 Destructeur de fichiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Destructeur de fichiers");
        ui.colored_label(Color32::from_rgb(255, 170, 80), "⚠ Suppression définitive : pas de corbeille, pas de retour en arrière.");
        ui.weak("Le contenu est écrasé par des données aléatoires avant la suppression. Sur un SSD, Windows peut garder des copies ailleurs : pour un SSD, le chiffrement (Coffre de fichiers, BitLocker) protège mieux.");
        self.liste.extend(ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect::<Vec<_>>()));
        ui.horizontal(|ui| {
            if ui.button("➕ Fichiers…").clicked() {
                self.liste.extend(rfd::FileDialog::new().pick_files().unwrap_or_default());
            }
            if ui.button("➕ Dossier…").clicked() {
                self.liste.extend(rfd::FileDialog::new().pick_folder());
            }
            if ui.button("Vider").clicked() {
                self.liste.clear();
            }
        });
        for p in &self.liste {
            ui.label(format!("{} {}", if p.is_dir() { "📁" } else { "📄" }, p.display()));
        }
        ui.checkbox(&mut self.confirme, "Je comprends que ces fichiers seront perdus pour toujours");
        let occupe = afficher_etat(ui, &self.e);
        if ui.add_enabled(self.confirme && !occupe && !self.liste.is_empty(), egui::Button::new(RichText::new("🔥 Détruire").color(Color32::LIGHT_RED))).clicked() {
            let l = std::mem::take(&mut self.liste);
            self.confirme = false;
            en_fond(&self.e, move || {
                let mut n = 0;
                for p in &l {
                    n += detruire(p).map_err(|e| format!("{} : {e}", p.display()))?;
                }
                Ok(format!("✔ {n} fichier(s) détruit(s)."))
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 89 · Récupération (corbeille, y compris les fichiers cachés à l'explorateur)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Supprime {
    pub origine: PathBuf,
    pub taille: u64,
    pub date: i64, // secondes UTC
    pub donnees: PathBuf, // fichier $R…
    pub info: PathBuf,    // fichier $I…
}

/// Lit un fichier $I de la corbeille (versions 1 et 2) : (taille, date de suppression UTC, chemin d'origine).
pub fn lire_info(b: &[u8]) -> Option<(u64, i64, String)> {
    if b.len() < 24 {
        return None;
    }
    let version = u64::from_le_bytes(b[0..8].try_into().ok()?);
    let taille = u64::from_le_bytes(b[8..16].try_into().ok()?);
    let ft = i64::from_le_bytes(b[16..24].try_into().ok()?);
    let date = ft / 10_000_000 - 11_644_473_600;
    let utf16: Vec<u16> = match version {
        1 => b.get(24..24 + 520)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect(),
        2 => {
            let n = u32::from_le_bytes(b.get(24..28)?.try_into().ok()?) as usize;
            b.get(28..28 + n * 2)?.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
        }
        _ => return None,
    };
    let chemin = String::from_utf16_lossy(&utf16).trim_end_matches('\0').to_string();
    Some((taille, date, chemin))
}

pub fn corbeille() -> Vec<Supprime> {
    let mut v = vec![];
    for (d, _, _) in crate::systeme::disques() {
        let racine = PathBuf::from(&d).join("$Recycle.Bin");
        for sid in std::fs::read_dir(&racine).into_iter().flatten().flatten() {
            for e in std::fs::read_dir(sid.path()).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if let Some(suite) = n.strip_prefix("$I") {
                    let donnees = sid.path().join(format!("$R{suite}"));
                    if let (Ok(b), true) = (std::fs::read(e.path()), donnees.exists()) {
                        if let Some((taille, date, origine)) = lire_info(&b) {
                            v.push(Supprime { origine: origine.into(), taille, date, donnees, info: e.path() });
                        }
                    }
                }
            }
        }
    }
    v.sort_by(|a, b| b.date.cmp(&a.date));
    v
}

pub struct Recuperation {
    liste: Option<Vec<Supprime>>,
    recherche: String,
    message: String,
}

impl Recuperation {
    pub fn new() -> Self {
        Recuperation { liste: None, recherche: String::new(), message: String::new() }
    }
}

impl Outil for Recuperation {
    fn nom(&self) -> &'static str {
        "♻ Récupération de fichiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Récupération de fichiers");
        if self.liste.is_none() {
            self.liste = Some(corbeille());
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍 nom ou dossier").desired_width(220.0));
            if ui.button("⟳").clicked() {
                self.liste = Some(corbeille());
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        let off = crate::pressepapiers::decalage();
        let q = self.recherche.to_lowercase();
        let mut restaurer = None;
        egui::ScrollArea::vertical().id_salt("corbeille").max_height(ui.available_height() - 90.0).show(ui, |ui| {
            for (i, s) in self.liste.iter().flatten().enumerate().filter(|(_, s)| q.is_empty() || s.origine.to_string_lossy().to_lowercase().contains(&q)).take(500) {
                ui.horizontal(|ui| {
                    if ui.small_button("↩ Restaurer").clicked() {
                        restaurer = Some(i);
                    }
                    ui.label(s.origine.file_name().unwrap_or_default().to_string_lossy());
                    ui.weak(format!("{} · {} · {}", s.origine.parent().map(|p| p.display().to_string()).unwrap_or_default(), crate::disque::taille_lisible(s.taille), crate::notes::format_local(s.date, off)));
                });
            }
            if self.liste.as_ref().is_some_and(|l| l.is_empty()) {
                ui.weak("La corbeille est vide.");
            }
        });
        if let Some(i) = restaurer {
            let s = self.liste.as_ref().unwrap()[i].clone();
            let mut cible = s.origine.clone();
            let mut k = 2;
            while cible.exists() {
                cible = s.origine.with_file_name(format!("{} ({k}){}", s.origine.file_stem().unwrap_or_default().to_string_lossy(),
                    s.origine.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()));
                k += 1;
            }
            let _ = std::fs::create_dir_all(cible.parent().unwrap_or(Path::new(".")));
            self.message = match std::fs::rename(&s.donnees, &cible) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&s.info);
                    self.liste.as_mut().unwrap().remove(i);
                    format!("✔ Restauré : {}", cible.display())
                }
                Err(e) => format!("✖ {e}"),
            };
        }
        ui.separator();
        ui.strong("Fichiers supprimés définitivement (corbeille vidée, Maj+Suppr)");
        ui.weak("Outil officiel de Microsoft « Windows File Recovery » (en ligne de commande). Plus tu utilises le disque après la suppression, moins il y a de chances.");
        ui.horizontal(|ui| {
            if ui.button("Installer depuis le Microsoft Store").clicked() {
                let _ = Command::new("explorer").arg("ms-windows-store://pdp/?ProductId=9N26S50LN705").spawn();
            }
            if ui.button("Exemple de commande").clicked() {
                ui.ctx().copy_text(r#"winfr C: D:\Recup /regular /n \Users\*\Documents\*.docx"#.into());
                self.message = "Commande copiée : récupère les .docx de Documents de C: vers D:\\Recup (à lancer en admin, sur un autre disque).".into();
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 90 · Gros fichiers oubliés
// ---------------------------------------------------------------------------------------------------

/// Vrai si le fichier n'a ni été ouvert ni modifié depuis `jours`.
pub fn oublie(acces: Option<SystemTime>, modif: Option<SystemTime>, maintenant: SystemTime, jours: u64) -> bool {
    let dernier = acces.max(modif);
    dernier.is_some_and(|t| maintenant.duration_since(t).unwrap_or_default() > Duration::from_secs(jours * 86400))
}

pub struct GrosFichiers {
    dossier: PathBuf,
    min_mo: u64,
    mois: u64,
    resultats: Arc<Mutex<Option<Vec<(PathBuf, u64, SystemTime)>>>>,
    e: Etat,
}

impl GrosFichiers {
    pub fn new() -> Self {
        GrosFichiers { dossier: PathBuf::from(std::env::var("USERPROFILE").unwrap_or("C:\\".into())), min_mo: 500, mois: 12, resultats: Arc::default(), e: Arc::default() }
    }
}

impl Outil for GrosFichiers {
    fn nom(&self) -> &'static str {
        "🐘 Gros fichiers oubliés"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Gros fichiers oubliés");
        ui.horizontal(|ui| {
            ui.monospace(self.dossier.display().to_string());
            if ui.button("Changer…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.dossier = d;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Plus de");
            ui.add(egui::DragValue::new(&mut self.min_mo).range(10..=100_000).suffix(" Mo"));
            ui.label("pas ouverts depuis");
            ui.add(egui::DragValue::new(&mut self.mois).range(1..=120).suffix(" mois"));
        });
        let occupe = afficher_etat(ui, &self.e);
        if ui.add_enabled(!occupe, egui::Button::new("🔍 Chercher")).clicked() {
            let (d, min, jours, r) = (self.dossier.clone(), self.min_mo << 20, self.mois * 30, self.resultats.clone());
            en_fond(&self.e, move || {
                let maintenant = SystemTime::now();
                let mut v = vec![];
                let mut pile = vec![d];
                while let Some(x) = pile.pop() {
                    for e in std::fs::read_dir(&x).into_iter().flatten().flatten() {
                        let Ok(m) = e.metadata() else { continue };
                        if m.is_dir() && !m.file_type().is_symlink() {
                            pile.push(e.path());
                        } else if m.len() >= min && oublie(m.accessed().ok(), m.modified().ok(), maintenant, jours) {
                            v.push((e.path(), m.len(), m.accessed().ok().max(m.modified().ok()).unwrap_or(maintenant)));
                        }
                    }
                }
                v.sort_by(|a, b| b.1.cmp(&a.1));
                let total: u64 = v.iter().map(|x| x.1).sum();
                let n = v.len();
                *r.lock().unwrap() = Some(v);
                Ok(format!("{n} fichier(s), {} au total.", crate::disque::taille_lisible(total)))
            });
        }
        let mut retirer = None;
        if let Some(v) = self.resultats.lock().unwrap().as_ref() {
            egui::ScrollArea::vertical().id_salt("gros").show(ui, |ui| {
                for (i, (p, t, d)) in v.iter().enumerate().take(300) {
                    ui.horizontal(|ui| {
                        if ui.small_button("📂").clicked() {
                            crate::disque::montrer_dans_explorateur(p);
                        }
                        if ui.small_button("🗑").clicked() && crate::disque::corbeille(p) {
                            retirer = Some(i);
                        }
                        ui.strong(crate::disque::taille_lisible(*t));
                        ui.label(p.display().to_string());
                        let j = SystemTime::now().duration_since(*d).unwrap_or_default().as_secs() / 86400;
                        ui.weak(format!("il y a {} mois", j / 30));
                    });
                }
            });
        }
        if let Some(i) = retirer {
            if let Some(v) = self.resultats.lock().unwrap().as_mut() {
                v.remove(i);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 91 · Déplacer un dossier en laissant une jonction
// ---------------------------------------------------------------------------------------------------

fn copier(src: &Path, dest: &Path) -> std::io::Result<u64> {
    if src.is_dir() {
        std::fs::create_dir_all(dest)?;
        let mut n = 0;
        for e in std::fs::read_dir(src)?.flatten() {
            n += copier(&e.path(), &dest.join(e.file_name()))?;
        }
        Ok(n)
    } else {
        std::fs::copy(src, dest)
    }
}

pub fn deplacer_avec_jonction(src: &Path, dossier_dest: &Path) -> Result<PathBuf, String> {
    let dest = dossier_dest.join(src.file_name().ok_or("dossier invalide")?);
    if dest.exists() {
        return Err(format!("{} existe déjà", dest.display()));
    }
    copier(src, &dest).map_err(|e| format!("copie : {e}"))?;
    if let Err(e) = std::fs::remove_dir_all(src) {
        return Err(format!("copie faite dans {}, mais l'original n'a pas pu être supprimé (jeu ouvert ?) : {e}", dest.display()));
    }
    let o = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(src).arg(&dest).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("jonction impossible : {}", String::from_utf8_lossy(&o.stdout).trim()));
    }
    Ok(dest)
}

pub struct Deplacer {
    src: Option<PathBuf>,
    dest: Option<PathBuf>,
    e: Etat,
}

impl Deplacer {
    pub fn new() -> Self {
        Deplacer { src: None, dest: None, e: Arc::default() }
    }
}

impl Outil for Deplacer {
    fn nom(&self) -> &'static str {
        "🔀 Déplacer vers un autre disque"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Déplacer un dossier sans le casser");
        ui.weak("Déplace un jeu ou un gros dossier sur un autre disque et laisse à sa place une « jonction » : les programmes continuent de le trouver à l'ancien endroit.");
        ui.horizontal(|ui| {
            if ui.button("1. Dossier à déplacer…").clicked() {
                self.src = rfd::FileDialog::new().pick_folder();
            }
            if let Some(s) = &self.src {
                ui.monospace(s.display().to_string());
            }
        });
        ui.horizontal(|ui| {
            if ui.button("2. Vers (dossier sur l'autre disque)…").clicked() {
                self.dest = rfd::FileDialog::new().pick_folder();
            }
            if let Some(d) = &self.dest {
                ui.monospace(d.display().to_string());
            }
        });
        let occupe = afficher_etat(ui, &self.e);
        if ui.add_enabled(!occupe && self.src.is_some() && self.dest.is_some(), egui::Button::new("🔀 Déplacer")).clicked() {
            let (s, d) = (self.src.clone().unwrap(), self.dest.clone().unwrap());
            en_fond(&self.e, move || deplacer_avec_jonction(&s, &d).map(|p| format!("✔ Déplacé dans {} ; l'ancien chemin y mène toujours.", p.display())));
        }
        ui.weak("Ferme le jeu / le programme avant. Pour un jeu Steam, l'option « Déplacer le dossier d'installation » de Steam est aussi possible.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 92 · Dossiers favoris
// ---------------------------------------------------------------------------------------------------

pub struct Favoris {
    liste: Vec<(String, PathBuf)>,
}

impl Favoris {
    pub fn new() -> Self {
        let mut liste: Vec<(String, PathBuf)> = charger("favoris");
        if liste.is_empty() {
            let u = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
            liste = vec![("Téléchargements".into(), u.join("Downloads")), ("Documents".into(), u.join("Documents")), ("Images".into(), u.join("Pictures")),
                ("Bureau".into(), u.join("Desktop")), ("AppData".into(), PathBuf::from(std::env::var("APPDATA").unwrap_or_default()))];
        }
        Favoris { liste }
    }
}

impl Outil for Favoris {
    fn nom(&self) -> &'static str {
        "⭐ Dossiers favoris"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dossiers favoris");
        let avant = self.liste.clone();
        ui.horizontal_wrapped(|ui| {
            for (n, p) in &self.liste {
                if ui.add(egui::Button::new(RichText::new(format!("📁 {n}")).size(16.0)).min_size(egui::vec2(180.0, 44.0))).on_hover_text(p.display().to_string()).clicked() {
                    crate::outil::ouvrir(p);
                }
            }
        });
        ui.separator();
        if ui.button("➕ Ajouter un dossier…").clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                let n = d.file_name().unwrap_or_default().to_string_lossy().into_owned();
                self.liste.push((n, d));
            }
        }
        for p in ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).filter(|p| p.is_dir()).collect::<Vec<_>>()) {
            let n = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.liste.push((n, p));
        }
        let mut retirer = None;
        let mut monter = None;
        for (i, (n, p)) in self.liste.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                if i > 0 && ui.small_button("⬆").clicked() {
                    monter = Some(i);
                }
                if ui.small_button("✖").clicked() {
                    retirer = Some(i);
                }
                ui.add(egui::TextEdit::singleline(n).desired_width(160.0));
                ui.weak(p.display().to_string());
                if !p.exists() {
                    ui.colored_label(Color32::LIGHT_RED, "introuvable");
                }
            });
        }
        if let Some(i) = monter {
            self.liste.swap(i, i - 1);
        }
        if let Some(i) = retirer {
            self.liste.remove(i);
        }
        if self.liste != avant {
            enregistrer("favoris", &self.liste);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_corbeille() {
        let chemin: Vec<u16> = "C:\\a.txt".encode_utf16().chain([0]).collect();
        let mut b = vec![];
        b.extend(2u64.to_le_bytes());
        b.extend(1234u64.to_le_bytes());
        b.extend(((1_700_000_000i64 + 11_644_473_600) * 10_000_000).to_le_bytes());
        b.extend((chemin.len() as u32).to_le_bytes());
        for c in chemin {
            b.extend(c.to_le_bytes());
        }
        assert_eq!(lire_info(&b), Some((1234, 1_700_000_000, "C:\\a.txt".to_string())));
        assert_eq!(lire_info(&[0; 5]), None);
    }

    #[test]
    fn oublis() {
        let m = SystemTime::now();
        let vieux = m - Duration::from_secs(400 * 86400);
        assert!(oublie(Some(vieux), Some(vieux), m, 365));
        assert!(!oublie(Some(m), Some(vieux), m, 365));
        assert!(!oublie(None, None, m, 365));
    }

    #[test]
    fn destruction() {
        let d = std::env::temp_dir().join(format!("outils_detr_{}", std::process::id()));
        std::fs::create_dir_all(d.join("s")).unwrap();
        std::fs::write(d.join("s/a"), vec![7u8; 3000]).unwrap();
        std::fs::write(d.join("b"), "x").unwrap();
        assert_eq!(detruire(&d).unwrap(), 2);
        assert!(!d.exists());
    }
}
