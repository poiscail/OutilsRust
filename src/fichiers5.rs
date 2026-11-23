//! Minecraft et fichiers, suite : constructions entre mondes (19), calculatrice Minecraft (20),
//! coffre de photos privées (21), convertisseur de documents (22), renommer les photos par date (23),
//! doublons de musique (24), fichiers récents (25), corbeille de sécurité (26).
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    o
}

fn taille(o: u64) -> String {
    crate::disque::taille_lisible(o)
}

fn maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn donnees() -> PathBuf {
    crate::outil::base()
}

// ---------------------------------------------------------------------------------------------------
// 26 · Corbeille de sécurité (utilisée par les autres outils avant d'effacer)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Abri {
    pub id: String,
    pub origine: String,
    pub quand: i64,
    pub par: String,
    pub taille: u64,
}

fn dossier_abri() -> PathBuf {
    donnees().join("corbeille_securite")
}

fn taille_chemin(p: &Path) -> u64 {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| taille_chemin(&e.path())).sum(),
        Ok(m) => m.len(),
        _ => 0,
    }
}

/// Met un fichier ou dossier à l'abri au lieu de l'effacer (gardé 30 jours). `par` : l'outil qui efface.
pub fn mettre_a_l_abri(p: &Path, par: &str) -> Result<(), String> {
    mettre_a_l_abri_vers(p, p, par)
}

/// Comme `mettre_a_l_abri`, avec l'endroit où le remettre (s'il diffère de là où il est).
pub fn mettre_a_l_abri_vers(p: &Path, origine: &Path, par: &str) -> Result<(), String> {
    let id = format!("{}_{}", maintenant(), crate::outil::hasard(1 << 30));
    let d = dossier_abri().join(&id);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let dest = d.join(p.file_name().ok_or("nom ?")?);
    let t = taille_chemin(p);
    // même disque : simple renommage ; sinon copie puis effacement
    if std::fs::rename(p, &dest).is_err() {
        if p.is_dir() {
            copier_dossier(p, &dest)?;
            std::fs::remove_dir_all(p).map_err(|e| e.to_string())?;
        } else {
            std::fs::copy(p, &dest).map_err(|e| e.to_string())?;
            std::fs::remove_file(p).map_err(|e| e.to_string())?;
        }
    }
    let mut l: Vec<Abri> = charger("corbeille_securite");
    l.push(Abri { id, origine: origine.display().to_string(), quand: maintenant(), par: par.into(), taille: t });
    enregistrer("corbeille_securite", &l);
    Ok(())
}

fn copier_dossier(a: &Path, b: &Path) -> Result<(), String> {
    std::fs::create_dir_all(b).map_err(|e| e.to_string())?;
    for e in std::fs::read_dir(a).map_err(|e| e.to_string())?.flatten() {
        let (x, y) = (e.path(), b.join(e.file_name()));
        if x.is_dir() {
            copier_dossier(&x, &y)?;
        } else {
            std::fs::copy(&x, &y).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn restaurer(a: &Abri) -> Result<(), String> {
    let d = dossier_abri().join(&a.id);
    let src = std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten().next().ok_or("élément introuvable")?.path();
    let dest = PathBuf::from(&a.origine);
    if dest.exists() {
        return Err("un fichier porte déjà ce nom à cet endroit".into());
    }
    let _ = std::fs::create_dir_all(dest.parent().unwrap_or(Path::new(".")));
    if std::fs::rename(&src, &dest).is_err() {
        if src.is_dir() {
            copier_dossier(&src, &dest)?;
        } else {
            std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        }
    }
    let _ = std::fs::remove_dir_all(&d);
    Ok(())
}

/// Efface pour de bon ce qui est à l'abri depuis plus de `jours`.
pub fn purger(jours: i64) -> usize {
    let mut l: Vec<Abri> = charger("corbeille_securite");
    let limite = maintenant() - jours * 86_400;
    let (vieux, garde): (Vec<Abri>, Vec<Abri>) = l.drain(..).partition(|a| a.quand < limite);
    for a in &vieux {
        let _ = std::fs::remove_dir_all(dossier_abri().join(&a.id));
    }
    enregistrer("corbeille_securite", &garde);
    vieux.len()
}

pub struct CorbeilleSecurite {
    message: String,
}

impl CorbeilleSecurite {
    pub fn new() -> Self {
        std::thread::spawn(|| loop {
            purger(30);
            std::thread::sleep(Duration::from_secs(6 * 3600));
        });
        CorbeilleSecurite { message: String::new() }
    }
}

impl Outil for CorbeilleSecurite {
    fn nom(&self) -> &'static str {
        "🛟 Corbeille de sécurité"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Corbeille de sécurité");
        ui.weak("Ce qu'Outils efface (régions Minecraft, mods retirés, corbeille Windows vidée automatiquement…) est gardé ici 30 jours avant de disparaître pour de bon.");
        let mut l: Vec<Abri> = charger("corbeille_securite");
        let total: u64 = l.iter().map(|a| a.taille).sum();
        ui.label(format!("{} élément(s) · {}", l.len(), taille(total)));
        let mut changer = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (k, a) in l.iter().enumerate().rev() {
                ui.horizontal(|ui| {
                    let jours = 30 - (maintenant() - a.quand) / 86_400;
                    ui.label(RichText::new(Path::new(&a.origine).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).strong());
                    ui.weak(format!("{} · par {} · encore {} j", taille(a.taille), a.par, jours.max(0)));
                    if ui.small_button("↩ Remettre").on_hover_text(&a.origine).clicked() {
                        changer = Some((k, true));
                    }
                    if ui.small_button("🗑").on_hover_text("Effacer pour de bon").clicked() {
                        changer = Some((k, false));
                    }
                });
            }
        });
        if let Some((k, remettre)) = changer {
            let a = l[k].clone();
            let r = if remettre { restaurer(&a) } else { std::fs::remove_dir_all(dossier_abri().join(&a.id)).map_err(|e| e.to_string()) };
            match r {
                Ok(()) => {
                    l.remove(k);
                    enregistrer("corbeille_securite", &l);
                    self.message = if remettre { format!("✔ Remis : {}", a.origine) } else { "Effacé.".into() };
                }
                Err(e) => self.message = format!("✖ {e}"),
            }
        }
        if !l.is_empty() && ui.button("🗑 Tout effacer maintenant").clicked() {
            purger(-1);
            self.message = "Corbeille de sécurité vidée.".into();
        }
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 19 · Constructions entre mondes (structures .nbt)
// ---------------------------------------------------------------------------------------------------

/// Mondes connus : (nom affiché, dossier du monde).
fn mondes(mc: &crate::minecraft::Pilote) -> Vec<(String, PathBuf)> {
    let mut v = vec![];
    if let Some(d) = mc.dossier() {
        let n = crate::minecraft::nom_monde(&d);
        v.push((format!("🖥 Serveur : {n}"), d.join(n)));
    }
    let saves = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join(".minecraft").join("saves");
    let mut s: Vec<(String, PathBuf)> = std::fs::read_dir(saves).into_iter().flatten().flatten().filter(|e| e.path().join("level.dat").exists()).map(|e| (format!("🎮 {}", e.file_name().to_string_lossy()), e.path())).collect();
    s.sort();
    v.extend(s);
    v
}

fn structures(monde: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for ns in std::fs::read_dir(monde.join("generated")).into_iter().flatten().flatten() {
        let d = ns.path().join("structures");
        let mut pile = vec![d];
        while let Some(p) = pile.pop() {
            for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    pile.push(e.path());
                } else if e.path().extension().is_some_and(|x| x == "nbt") {
                    v.push(e.path());
                }
            }
        }
    }
    v.sort();
    v
}

/// Taille (x, y, z) d'une structure .nbt (compressée gzip).
pub fn taille_structure(b: &[u8]) -> Option<(i32, i32, i32)> {
    let mut d = vec![];
    flate2::read::GzDecoder::new(b).read_to_end(&mut d).ok()?;
    let (_, n) = crate::mcplus3::lire_nbt(&d).ok()?;
    match n.get("size")? {
        crate::mcplus3::Nbt::List(_, l) if l.len() == 3 => Some((l[0].nombre()? as i32, l[1].nombre()? as i32, l[2].nombre()? as i32)),
        _ => None,
    }
}

fn nom_structure(monde: &Path, p: &Path) -> String {
    // generated/<espace>/structures/<chemin>.nbt → espace:chemin
    let rel = p.strip_prefix(monde.join("generated")).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let mut it = rel.splitn(3, '/');
    let ns = it.next().unwrap_or("minecraft");
    let _ = it.next();
    format!("{ns}:{}", it.next().unwrap_or("").trim_end_matches(".nbt"))
}

pub struct Structures {
    mc: crate::minecraft::Pilote,
    source: usize,
    cible: usize,
    message: String,
}

impl Structures {
    pub fn new(mc: crate::minecraft::Pilote) -> Self {
        Structures { mc, source: 0, cible: 1, message: String::new() }
    }
}

impl Outil for Structures {
    fn nom(&self) -> &'static str {
        "🏗 Constructions entre mondes"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Copier une construction d'un monde à l'autre");
        ui.weak("Avec un bloc de structure (/give @p structure_block), enregistre ta construction sous un nom dans le premier monde (48×48×48 max). Copie-la ici vers l'autre monde, puis charge-la avec un bloc de structure en mode « Charger ».");
        let m = mondes(&self.mc);
        if m.len() < 1 {
            ui.label("Aucun monde trouvé (ni serveur configuré, ni sauvegarde dans .minecraft/saves).");
            return;
        }
        self.source = self.source.min(m.len() - 1);
        self.cible = self.cible.min(m.len() - 1);
        ui.horizontal(|ui| {
            ui.label("De");
            egui::ComboBox::from_id_salt("monde_source").width(220.0).selected_text(&m[self.source].0).show_ui(ui, |ui| {
                for (k, (n, _)) in m.iter().enumerate() {
                    ui.selectable_value(&mut self.source, k, n);
                }
            });
            ui.label("vers");
            egui::ComboBox::from_id_salt("monde_cible").width(220.0).selected_text(&m[self.cible].0).show_ui(ui, |ui| {
                for (k, (n, _)) in m.iter().enumerate() {
                    ui.selectable_value(&mut self.cible, k, n);
                }
            });
        });
        let (src, dst) = (m[self.source].1.clone(), m[self.cible].1.clone());
        let l = structures(&src);
        if l.is_empty() {
            ui.weak("Aucune construction enregistrée dans ce monde.");
        }
        egui::Grid::new("structures").striped(true).show(ui, |ui| {
            for p in &l {
                let n = nom_structure(&src, p);
                ui.label(RichText::new(&n).monospace());
                let t = std::fs::read(p).ok().and_then(|b| taille_structure(&b));
                ui.weak(t.map(|(x, y, z)| format!("{x}×{y}×{z}")).unwrap_or_default());
                if ui.add_enabled(self.source != self.cible, egui::Button::new("➡ Copier")).clicked() {
                    let rel = p.strip_prefix(&src).unwrap_or(p);
                    let d = dst.join(rel);
                    let _ = std::fs::create_dir_all(d.parent().unwrap());
                    self.message = std::fs::copy(p, &d).map(|_| format!("✔ « {n} » copiée. Dans l'autre monde : bloc de structure › Charger › nom {n}")).unwrap_or_else(|e| e.to_string());
                }
                if ui.small_button("📤").on_hover_text("Exporter le fichier (pour un ami)").clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Structure", &["nbt"]).set_file_name(p.file_name().unwrap_or_default().to_string_lossy()).save_file() {
                        let _ = std::fs::copy(p, f);
                    }
                }
                ui.end_row();
            }
        });
        if ui.button("📥 Importer un fichier .nbt dans le monde de droite").clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter("Structure", &["nbt"]).pick_file() {
                let d = dst.join("generated").join("minecraft").join("structures");
                let _ = std::fs::create_dir_all(&d);
                let n = f.file_stem().unwrap_or_default().to_string_lossy().to_lowercase().replace(' ', "_");
                self.message = std::fs::copy(&f, d.join(format!("{n}.nbt"))).map(|_| format!("✔ Importée : charge-la avec le nom minecraft:{n}")).unwrap_or_else(|e| e.to_string());
            }
        }
        ui.label(&self.message);
        if self.mc.statut().is_some() && m[self.cible].0.starts_with("🖥") {
            ui.weak("Le serveur tourne : la structure sera visible après un redémarrage (ou /reload).");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 20 · Calculatrice Minecraft
// ---------------------------------------------------------------------------------------------------

/// (stacks, reste) pour un nombre d'objets et une taille de pile.
pub fn en_stacks(n: u64, pile: u64) -> (u64, u64) {
    (n / pile, n % pile)
}

/// Combustible : (nom, objets cuits par unité).
pub const COMBUSTIBLES: [(&str, f64); 7] = [("Charbon", 8.0), ("Bloc de charbon", 80.0), ("Bâton de blaze", 12.0), ("Seau de lave", 100.0), ("Bloc de varech séché", 20.0), ("Planche", 1.5), ("Bambou", 0.25)];

/// Secondes pour cuire `n` objets avec `fours` fours (10 s par objet, 5 s en haut-fourneau / fumoir).
pub fn temps_cuisson(n: u64, fours: u64, rapide: bool) -> f64 {
    let par = if rapide { 5.0 } else { 10.0 };
    (n as f64 / fours.max(1) as f64).ceil() * par
}

pub struct CalcMinecraft {
    objets: u64,
    pile: u64,
    cuire: u64,
    fours: u64,
    rapide: bool,
    entonnoirs: u64,
    repeteurs: u32,
    crans: u32,
}

impl CalcMinecraft {
    pub fn new() -> Self {
        CalcMinecraft { objets: 1000, pile: 64, cuire: 640, fours: 8, rapide: false, entonnoirs: 1, repeteurs: 2, crans: 4 }
    }
}

impl Outil for CalcMinecraft {
    fn nom(&self) -> &'static str {
        "🧮 Calculatrice Minecraft"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Calculatrice Minecraft");
        let gros = |t: String| RichText::new(t).size(17.0).strong();
        ui.strong("📦 Objets → piles, coffres, boîtes de shulker");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.objets).speed(16).suffix(" objets"));
            ui.selectable_value(&mut self.pile, 64, "piles de 64");
            ui.selectable_value(&mut self.pile, 16, "de 16 (perles, œufs…)");
            ui.selectable_value(&mut self.pile, 1, "non empilables");
        });
        let (s, r) = en_stacks(self.objets, self.pile);
        let piles = s + (r > 0) as u64;
        ui.label(gros(format!("{s} pile(s) + {r} · {:.1} coffre(s) · {:.1} grand(s) coffre(s) · {:.1} shulker(s)", piles as f64 / 27.0, piles as f64 / 54.0, piles as f64 / 27.0)));
        ui.separator();
        ui.strong("🔥 Fours");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.cuire).speed(8).suffix(" objets à cuire"));
            ui.add(egui::DragValue::new(&mut self.fours).range(1..=256).suffix(" four(s)"));
            ui.checkbox(&mut self.rapide, "haut-fourneau / fumoir (2× plus rapide)");
        });
        let t = temps_cuisson(self.cuire, self.fours, self.rapide);
        ui.label(gros(format!("⏱ {} min {:02} s", t as u64 / 60, t as u64 % 60)));
        ui.horizontal_wrapped(|ui| {
            for (n, k) in COMBUSTIBLES {
                ui.label(format!("{n} : {}", (self.cuire as f64 / k).ceil()));
                ui.separator();
            }
        });
        ui.separator();
        ui.strong("⬇ Entonnoirs");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.entonnoirs).range(1..=64).suffix(" entonnoir(s) en parallèle"));
        });
        let debit = 2.5 * self.entonnoirs as f64;
        ui.label(gros(format!("{debit} objets/s · {:.0} objets/heure · {} objets vidés en {:.0} s", debit * 3600.0, self.objets, self.objets as f64 / debit)));
        ui.separator();
        ui.strong("🔴 Redstone");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.repeteurs).range(1..=64).suffix(" répéteur(s)"));
            ui.label("réglés sur");
            ui.add(egui::DragValue::new(&mut self.crans).range(1..=4).suffix(" cran(s)"));
        });
        let ticks = self.repeteurs * self.crans;
        ui.label(gros(format!("Retard : {ticks} tick(s) redstone = {:.1} s · horloge en boucle : une impulsion toutes les {:.1} s", ticks as f64 / 10.0, ticks as f64 * 2.0 / 10.0)));
        ui.weak("1 tick redstone = 0,1 s (2 ticks de jeu). Un comparateur ajoute 1 tick, une torche 1 tick.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 21 · Coffre de photos privées
// ---------------------------------------------------------------------------------------------------

fn dossier_coffre_photos() -> PathBuf {
    donnees().join("photos_privees")
}

/// Chiffre une photo (nom + contenu) avec la clé du coffre : nonce (12) + données chiffrées.
pub fn sceller(cle: &[u8; 32], nom: &str, donnees: &[u8]) -> Vec<u8> {
    let c = Aes256Gcm::new_from_slice(cle).unwrap();
    let mut n = [0u8; 12];
    let _ = getrandom::getrandom(&mut n);
    let mut clair = (nom.len() as u16).to_le_bytes().to_vec();
    clair.extend(nom.as_bytes());
    clair.extend(donnees);
    let mut o = n.to_vec();
    o.extend(c.encrypt(Nonce::from_slice(&n), clair.as_slice()).unwrap_or_default());
    o
}

pub fn desceller(cle: &[u8; 32], b: &[u8]) -> Option<(String, Vec<u8>)> {
    if b.len() < 14 {
        return None;
    }
    let c = Aes256Gcm::new_from_slice(cle).ok()?;
    let clair = c.decrypt(Nonce::from_slice(&b[..12]), &b[12..]).ok()?;
    let l = u16::from_le_bytes([clair[0], clair[1]]) as usize;
    Some((String::from_utf8_lossy(clair.get(2..2 + l)?).into_owned(), clair[2 + l..].to_vec()))
}

/// Clé du coffre : tirée au hasard à la création, gardée chiffrée par le mot de passe (Argon2id + AES-GCM).
fn cle_coffre(mdp: &str) -> Result<[u8; 32], String> {
    let f = dossier_coffre_photos().join("cle.coffre");
    if f.exists() {
        let mut out = vec![];
        crate::coffrefichiers::dechiffrer_flux(&mut std::fs::File::open(&f).map_err(|e| e.to_string())?, &mut out, mdp)?;
        out.try_into().map_err(|_| "clé abîmée".to_string())
    } else {
        if mdp.chars().count() < 6 {
            return Err("choisis un mot de passe d'au moins 6 caractères".into());
        }
        let mut k = [0u8; 32];
        getrandom::getrandom(&mut k).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dossier_coffre_photos()).map_err(|e| e.to_string())?;
        let mut out = vec![];
        crate::coffrefichiers::chiffrer_flux(&mut k.as_slice(), &mut out, mdp, (65536, 3, 4))?;
        std::fs::write(&f, out).map_err(|e| e.to_string())?;
        Ok(k)
    }
}

pub struct CoffrePhotos {
    mdp: String,
    cle: Option<[u8; 32]>,
    vu: Instant,
    photos: Arc<Mutex<Vec<(PathBuf, String, Option<egui::ColorImage>)>>>,
    textures: HashMap<PathBuf, egui::TextureHandle>,
    grande: Option<(PathBuf, egui::TextureHandle)>,
    e: Etat,
    effacer_originaux: bool,
}

impl CoffrePhotos {
    pub fn new() -> Self {
        CoffrePhotos { mdp: String::new(), cle: None, vu: Instant::now(), photos: Arc::default(), textures: HashMap::new(), grande: None, e: Arc::default(), effacer_originaux: true }
    }

    fn charger(&self) {
        let (cle, ph) = (self.cle.unwrap(), self.photos.clone());
        ph.lock().unwrap().clear();
        std::thread::spawn(move || {
            let mut v: Vec<PathBuf> = std::fs::read_dir(dossier_coffre_photos()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "ph")).collect();
            v.sort();
            for p in v {
                let Some((nom, b)) = std::fs::read(&p).ok().and_then(|b| desceller(&cle, &b)) else { continue };
                let mini = image::load_from_memory(&b).ok().map(|i| {
                    let t = i.thumbnail(220, 220).to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
                });
                ph.lock().unwrap().push((p, nom, mini));
            }
        });
    }

    fn verrouiller(&mut self) {
        self.cle = None;
        self.photos.lock().unwrap().clear();
        self.textures.clear();
        self.grande = None;
    }
}

impl Outil for CoffrePhotos {
    fn nom(&self) -> &'static str {
        "🔐 Coffre de photos privées"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Coffre de photos privées");
        // verrouillage auto après 5 min sans y toucher
        if self.cle.is_some() && self.vu.elapsed() > Duration::from_secs(300) {
            self.verrouiller();
        }
        if ui.ctx().input(|i| i.pointer.any_down() || !i.events.is_empty()) {
            self.vu = Instant::now();
        }
        let Some(cle) = self.cle else {
            let nouveau = !dossier_coffre_photos().join("cle.coffre").exists();
            ui.weak(if nouveau { "Choisis un mot de passe : les photos ajoutées sont chiffrées (AES-256) et invisibles ailleurs. Sans ce mot de passe, impossible de les récupérer." } else { "Entre le mot de passe du coffre." });
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.mdp).password(true).hint_text("mot de passe").desired_width(220.0));
                if ui.button(if nouveau { "Créer le coffre" } else { "🔓 Ouvrir" }).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    match cle_coffre(&self.mdp) {
                        Ok(k) => {
                            self.cle = Some(k);
                            self.vu = Instant::now();
                            self.charger();
                        }
                        Err(e) => *self.e.lock().unwrap() = (false, format!("✖ {e}")),
                    }
                    self.mdp.clear();
                }
            });
            etat_ui(ui, &self.e);
            return;
        };
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("➕ Ajouter des photos…")).clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Images", &["jpg", "jpeg", "png", "webp", "gif", "bmp"]).pick_files() {
                    let (ef, ph) = (self.effacer_originaux, self.photos.clone());
                    let _ = ph;
                    en_fond(&self.e, move || {
                        let mut n = 0;
                        for p in v {
                            let b = std::fs::read(&p).map_err(|e| e.to_string())?;
                            let nom = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                            let f = dossier_coffre_photos().join(format!("{}_{}.ph", maintenant(), crate::outil::hasard(1 << 40)));
                            std::fs::write(&f, sceller(&cle, &nom, &b)).map_err(|e| e.to_string())?;
                            if ef {
                                crate::disque::corbeille(&p);
                            }
                            n += 1;
                        }
                        Ok(format!("✔ {n} photo(s) mises au coffre{}", if ef { " (originaux à la corbeille : vide-la pour qu'ils disparaissent)" } else { "" }))
                    });
                    self.textures.clear();
                    self.cle = Some(cle);
                }
            }
            ui.checkbox(&mut self.effacer_originaux, "retirer les originaux");
            if ui.button("🔒 Verrouiller").clicked() {
                self.verrouiller();
            }
            if ui.small_button("⟳").clicked() {
                self.textures.clear();
                self.charger();
            }
        });
        if let Some((p, t)) = self.grande.clone() {
            let mut retour = false;
            ui.horizontal(|ui| {
                retour = ui.button("⬅ Retour").clicked();
                if ui.button("📤 Sortir du coffre…").clicked() {
                    if let Some((nom, b)) = std::fs::read(&p).ok().and_then(|b| desceller(&cle, &b)) {
                        if let Some(d) = rfd::FileDialog::new().set_file_name(&nom).save_file() {
                            if std::fs::write(&d, b).is_ok() {
                                let _ = std::fs::remove_file(&p);
                                retour = true;
                                self.textures.remove(&p);
                                self.photos.lock().unwrap().retain(|x| x.0 != p);
                            }
                        }
                    }
                }
                if ui.button("🗑 Supprimer").clicked() {
                    let _ = std::fs::remove_file(&p);
                    self.photos.lock().unwrap().retain(|x| x.0 != p);
                    retour = true;
                }
            });
            if retour {
                self.grande = None;
            } else {
                let l = ui.available_width().min(t.size()[0] as f32);
                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(l, l * t.size()[1] as f32 / t.size()[0] as f32)));
            }
            return;
        }
        let mut ouvrir_grand = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut ph = self.photos.lock().unwrap();
                for (p, nom, mini) in ph.iter_mut() {
                    if !self.textures.contains_key(p) {
                        if let Some(m) = mini.take() {
                            self.textures.insert(p.clone(), ui.ctx().load_texture(format!("ph_{}", p.display()), m, Default::default()));
                        }
                    }
                    if let Some(t) = self.textures.get(p) {
                        if ui.add(egui::ImageButton::new(egui::Image::new(t).fit_to_exact_size(egui::vec2(160.0, 160.0)))).on_hover_text(nom.as_str()).clicked() {
                            ouvrir_grand = Some(p.clone());
                        }
                    }
                }
                if ph.is_empty() {
                    ui.weak("Le coffre est vide.");
                }
            });
        });
        if let Some(p) = ouvrir_grand {
            if let Some(i) = std::fs::read(&p).ok().and_then(|b| desceller(&cle, &b)).and_then(|(_, b)| image::load_from_memory(&b).ok()) {
                let i = i.thumbnail(1600, 1600).to_rgba8();
                self.grande = Some((p, ui.ctx().load_texture("photo_privee", egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()), Default::default())));
            }
        }
        ui.weak("Verrouillage automatique après 5 minutes sans activité.");
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

// ---------------------------------------------------------------------------------------------------
// 22 · Convertisseur de documents
// ---------------------------------------------------------------------------------------------------

/// Crée un PDF d'une page par image (JPEG intégré tel quel, autres formats convertis en JPEG).
pub fn images_vers_pdf(images: &[PathBuf], sortie: &Path) -> Result<usize, String> {
    use lopdf::{dictionary, Document, Object, Stream};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = vec![];
    for p in images {
        let img = image::open(p).map_err(|e| format!("{} : {e}", p.display()))?;
        let (l, h) = (img.width(), img.height());
        let mut jpeg = vec![];
        img.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 88)).map_err(|e| e.to_string())?;
        let flux = Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => l as i64, "Height" => h as i64, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode" }, jpeg);
        let img_id = doc.add_object(flux);
        // page A4 (595 × 842 pt) dans le sens de l'image, image centrée
        let (pl, ph) = if l > h { (842.0, 595.0) } else { (595.0, 842.0) };
        let e = (pl / l as f64).min(ph / h as f64);
        let (w, hh) = (l as f64 * e, h as f64 * e);
        let contenu = format!("q {w:.2} 0 0 {hh:.2} {:.2} {:.2} cm /Im0 Do Q", (pl - w) / 2.0, (ph - hh) / 2.0);
        let c_id = doc.add_object(Stream::new(dictionary! {}, contenu.into_bytes()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), Object::Real(pl as f32), Object::Real(ph as f32)],
            "Contents" => c_id, "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
        });
        kids.push(page.into());
    }
    let n = kids.len();
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n as i64 }));
    let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", cat);
    doc.compress();
    doc.save(sortie).map_err(|e| e.to_string())?;
    Ok(n)
}

/// Chaque page d'un PDF en PNG, avec le moteur PDF de Windows.
fn pdf_vers_images(pdf: &Path, dossier: &Path, largeur: u32) -> Result<usize, String> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Storage::StorageFile;
    let e = |x: windows::core::Error| x.message();
    let chemin = std::fs::canonicalize(pdf).map_err(|x| x.to_string())?.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let f = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).map_err(e)?.get().map_err(e)?;
    let doc = PdfDocument::LoadFromFileAsync(&f).map_err(e)?.get().map_err(|_| "PDF illisible (protégé par mot de passe ?)".to_string())?;
    let n = doc.PageCount().map_err(e)?;
    std::fs::create_dir_all(dossier).map_err(|x| x.to_string())?;
    let base = pdf.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    for i in 0..n {
        let page = doc.GetPage(i).map_err(e)?;
        let flux = InMemoryRandomAccessStream::new().map_err(e)?;
        let opt = PdfPageRenderOptions::new().map_err(e)?;
        opt.SetDestinationWidth(largeur).map_err(e)?;
        page.RenderWithOptionsToStreamAsync(&flux, &opt).map_err(e)?.get().map_err(e)?;
        let t = flux.Size().map_err(e)? as u32;
        let lect = DataReader::CreateDataReader(&flux.GetInputStreamAt(0).map_err(e)?).map_err(e)?;
        lect.LoadAsync(t).map_err(e)?.get().map_err(e)?;
        let mut b = vec![0u8; t as usize];
        lect.ReadBytes(&mut b).map_err(e)?;
        std::fs::write(dossier.join(format!("{base} - page {:02}.png", i + 1)), b).map_err(|x| x.to_string())?;
    }
    Ok(n as usize)
}

/// Document Office / LibreOffice → PDF, avec Word, Excel, PowerPoint ou LibreOffice s'ils sont installés.
fn office_vers_pdf(f: &Path) -> Result<PathBuf, String> {
    let sortie = f.with_extension("pdf");
    let ext = f.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (src, dst) = (f.display().to_string().replace('\'', "''"), sortie.display().to_string().replace('\'', "''"));
    let script = match ext.as_str() {
        "doc" | "docx" | "odt" | "rtf" => Some(format!("$a = New-Object -ComObject Word.Application; $a.Visible = $false; $d = $a.Documents.Open('{src}', $false, $true); $d.SaveAs2('{dst}', 17); $d.Close($false); $a.Quit()")),
        "xls" | "xlsx" | "ods" => Some(format!("$a = New-Object -ComObject Excel.Application; $a.Visible = $false; $a.DisplayAlerts = $false; $d = $a.Workbooks.Open('{src}'); $d.ExportAsFixedFormat(0, '{dst}'); $d.Close($false); $a.Quit()")),
        "ppt" | "pptx" | "odp" => Some(format!("$a = New-Object -ComObject PowerPoint.Application; $d = $a.Presentations.Open('{src}', $true, $false, $false); $d.SaveAs('{dst}', 32); $d.Close(); $a.Quit()")),
        _ => None,
    };
    if let Some(s) = script {
        if crate::optimiseur::ps(&s).is_ok() && sortie.exists() {
            return Ok(sortie);
        }
    }
    // LibreOffice
    for base in ["%ProgramFiles%", "%ProgramFiles(x86)%"] {
        let soffice = PathBuf::from(crate::optimiseur::exp(base)).join(r"LibreOffice\program\soffice.exe");
        if soffice.exists() {
            let ok = std::process::Command::new(soffice).args(["--headless", "--convert-to", "pdf", "--outdir"]).arg(f.parent().unwrap_or(Path::new("."))).arg(f).creation_flags(SANS_FENETRE).status().is_ok_and(|s| s.success());
            if ok && sortie.exists() {
                return Ok(sortie);
            }
        }
    }
    Err("il faut Microsoft Office ou LibreOffice (gratuit) installé pour ce type de document".into())
}

pub struct ConvDocs {
    images: Vec<PathBuf>,
    largeur: u32,
    e: Etat,
}

impl ConvDocs {
    pub fn new() -> Self {
        ConvDocs { images: vec![], largeur: 1654, e: Arc::default() }
    }
}

impl Outil for ConvDocs {
    fn nom(&self) -> &'static str {
        "📑 Convertisseur de documents"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Convertisseur de documents");
        let occupe = etat_ui(ui, &self.e);
        ui.strong("🖼 Images → un PDF");
        ui.weak("Photos d'un cours, scans de feuilles… une page par image, dans l'ordre choisi.");
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter des images…").clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp"]).pick_files() {
                    self.images.extend(v);
                }
            }
            if !self.images.is_empty() && ui.button("Vider").clicked() {
                self.images.clear();
            }
        });
        let mut monter = None;
        let mut retirer = None;
        for (k, p) in self.images.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.weak(format!("{}.", k + 1));
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                if k > 0 && ui.small_button("⬆").clicked() {
                    monter = Some(k);
                }
                if ui.small_button("✖").clicked() {
                    retirer = Some(k);
                }
            });
        }
        if let Some(k) = monter {
            self.images.swap(k, k - 1);
        }
        if let Some(k) = retirer {
            self.images.remove(k);
        }
        if ui.add_enabled(!occupe && !self.images.is_empty(), egui::Button::new("📄 Créer le PDF…")).clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("document.pdf").save_file() {
                let v = self.images.clone();
                en_fond(&self.e, move || {
                    let n = images_vers_pdf(&v, &f)?;
                    ouvrir(&f);
                    Ok(format!("✔ PDF de {n} page(s) créé"))
                });
            }
        }
        ui.separator();
        ui.strong("📄 PDF → images");
        ui.horizontal(|ui| {
            ui.label("Largeur des images");
            egui::ComboBox::from_id_salt("largeur_pdf").selected_text(format!("{} px", self.largeur)).show_ui(ui, |ui| {
                for l in [800, 1240, 1654, 2480] {
                    ui.selectable_value(&mut self.largeur, l, format!("{l} px"));
                }
            });
            if ui.add_enabled(!occupe, egui::Button::new("Choisir un PDF…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_file() {
                    let l = self.largeur;
                    en_fond(&self.e, move || {
                        let d = p.with_extension("").with_file_name(format!("{} - pages", p.file_stem().unwrap_or_default().to_string_lossy()));
                        let n = pdf_vers_images(&p, &d, l)?;
                        ouvrir(&d);
                        Ok(format!("✔ {n} page(s) en images"))
                    });
                }
            }
        });
        ui.separator();
        ui.strong("📝 Word, Excel, PowerPoint, LibreOffice → PDF");
        if ui.add_enabled(!occupe, egui::Button::new("Choisir des documents…")).clicked() {
            if let Some(v) = rfd::FileDialog::new().add_filter("Documents", &["doc", "docx", "odt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp"]).pick_files() {
                en_fond(&self.e, move || {
                    let mut ok = 0;
                    let mut der = String::new();
                    for f in &v {
                        match office_vers_pdf(f) {
                            Ok(_) => ok += 1,
                            Err(e) => der = e,
                        }
                    }
                    if ok == 0 { Err(der) } else { Ok(format!("✔ {ok} PDF créé(s) à côté des originaux{}", if der.is_empty() { String::new() } else { format!(" ({der})") })) }
                });
            }
        }
        ui.weak("Utilise Microsoft Office s'il est installé, sinon LibreOffice.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 23 · Renommer les photos par date
// ---------------------------------------------------------------------------------------------------

/// « 2024-05-03 14.22.10 » + extension ; les doublons prennent « (2) », « (3)… ».
pub fn noms_par_date(entrees: &[(String, String)]) -> Vec<String> {
    let mut vus: HashMap<String, u32> = HashMap::new();
    entrees.iter().map(|(date, ext)| {
        let k = vus.entry(format!("{date}.{ext}")).or_insert(0);
        *k += 1;
        if *k == 1 { format!("{date}.{ext}") } else { format!("{date} ({k}).{ext}") }
    }).collect()
}

pub struct RenommerPhotos {
    dossier: Option<PathBuf>,
    plan: Vec<(PathBuf, String)>,
    message: String,
}

impl RenommerPhotos {
    pub fn new() -> Self {
        RenommerPhotos { dossier: None, plan: vec![], message: String::new() }
    }

    fn preparer(&mut self) {
        let Some(d) = &self.dossier else { return };
        let mut v: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| ["jpg", "jpeg", "png", "heic", "webp", "mp4", "mov"].contains(&x.to_string_lossy().to_lowercase().as_str()))).collect();
        v.sort();
        let dates: Vec<(String, String)> = v.iter().map(|p| {
            let d = crate::images::date_exif(p).unwrap_or_else(|| {
                let t = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0) + crate::pressepapiers::decalage();
                let (a, m, j) = crate::quotidien2::date_de_jours(t.div_euclid(86_400));
                format!("{a:04}-{m:02}-{j:02} {:02}.{:02}.{:02}", t.rem_euclid(86_400) / 3600, t.rem_euclid(3600) / 60, t.rem_euclid(60))
            });
            (d, p.extension().unwrap_or_default().to_string_lossy().to_lowercase())
        }).collect();
        self.plan = v.into_iter().zip(noms_par_date(&dates)).collect();
    }
}

impl Outil for RenommerPhotos {
    fn nom(&self) -> &'static str {
        "📸 Renommer les photos par date"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Renommer les photos par date");
        ui.weak("« IMG_4823.jpg » devient « 2024-05-03 14.22.10.jpg » (date de prise de vue) : les photos se rangent toutes seules dans l'ordre, même venant de plusieurs téléphones.");
        if ui.button("📁 Choisir le dossier…").clicked() {
            self.dossier = rfd::FileDialog::new().pick_folder();
            self.preparer();
            self.message.clear();
        }
        if self.plan.is_empty() {
            return;
        }
        let a_faire = self.plan.iter().filter(|(p, n)| p.file_name().is_some_and(|x| x.to_string_lossy() != *n)).count();
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("photos_dates").striped(true).show(ui, |ui| {
                for (p, n) in self.plan.iter().take(500) {
                    ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                    ui.label("→");
                    ui.label(RichText::new(n).color(Color32::LIGHT_GREEN));
                    ui.end_row();
                }
            });
        });
        if ui.add_enabled(a_faire > 0, egui::Button::new(format!("✏ Renommer {a_faire} fichier(s)"))).clicked() {
            let mut annul = vec![];
            // en deux temps pour éviter les collisions entre anciens et nouveaux noms
            let tmp: Vec<(PathBuf, PathBuf, PathBuf)> = self.plan.iter().filter(|(p, n)| p.file_name().is_some_and(|x| x.to_string_lossy() != *n)).map(|(p, n)| (p.clone(), p.with_file_name(format!("~outils~{}", crate::outil::hasard(1 << 40))), p.with_file_name(n))).collect();
            for (a, t, _) in &tmp {
                let _ = std::fs::rename(a, t);
            }
            let mut err = 0;
            for (a, t, b) in &tmp {
                if b.exists() || std::fs::rename(t, b).is_err() {
                    let _ = std::fs::rename(t, a);
                    err += 1;
                } else {
                    annul.push((b.display().to_string(), a.display().to_string()));
                }
            }
            enregistrer("photos_dates_annuler", &annul);
            self.message = format!("✔ {} renommé(s){}", annul.len(), if err > 0 { format!(", {err} ignoré(s)") } else { String::new() });
            self.preparer();
        }
        let annul: Vec<(String, String)> = charger("photos_dates_annuler");
        if !annul.is_empty() && ui.small_button("↩ Annuler le dernier renommage").clicked() {
            for (a, b) in &annul {
                let _ = std::fs::rename(a, b);
            }
            enregistrer::<Vec<(String, String)>>("photos_dates_annuler", &vec![]);
            self.message = "↩ Annulé.".into();
            self.preparer();
        }
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 24 · Doublons de musique
// ---------------------------------------------------------------------------------------------------

/// Clé de comparaison : artiste + titre simplifiés (sans « feat. », parenthèses, ponctuation).
pub fn cle_musique(artiste: &str, titre: &str) -> String {
    let t = regex::Regex::new(r"(?i)\s*[\(\[][^\)\]]*[\)\]]|\s+(feat|ft)\.?\s.*$").unwrap().replace_all(titre, "");
    let n = |s: &str| crate::fichiers3::simple(s).chars().filter(|c| c.is_alphanumeric()).collect::<String>();
    format!("{}|{}", n(artiste), n(&t))
}

pub struct DoublonsMusique {
    groupes: Arc<Mutex<Option<Vec<Vec<(PathBuf, u64, u64)>>>>>,
    e: Etat,
}

impl DoublonsMusique {
    pub fn new() -> Self {
        DoublonsMusique { groupes: Arc::default(), e: Arc::default() }
    }
}

impl Outil for DoublonsMusique {
    fn nom(&self) -> &'static str {
        "🎶 Doublons de musique"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Doublons de musique");
        ui.weak("Trouve la même chanson en plusieurs exemplaires (formats ou qualités différents, noms de fichiers différents) grâce à ses infos et sa durée.");
        let occupe = etat_ui(ui, &self.e);
        if ui.add_enabled(!occupe, egui::Button::new("📁 Analyser un dossier de musique…")).clicked() {
            if let Some(d) = rfd::FileDialog::new().pick_folder() {
                let g = self.groupes.clone();
                en_fond(&self.e, move || {
                    let mut v = vec![];
                    let mut pile = vec![d];
                    while let Some(p) = pile.pop() {
                        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                            let x = e.path();
                            if x.is_dir() {
                                pile.push(x);
                            } else if x.extension().is_some_and(|y| ["mp3", "flac", "m4a", "ogg", "wav", "opus", "aac"].contains(&y.to_string_lossy().to_lowercase().as_str())) {
                                v.push(x);
                            }
                        }
                    }
                    let mut par_cle: BTreeMap<String, Vec<(PathBuf, u64, u64)>> = BTreeMap::new();
                    for p in v {
                        let i = crate::fichiers4::lire_infos(&p);
                        if i.titre.trim().is_empty() {
                            continue;
                        }
                        let t = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                        par_cle.entry(cle_musique(&i.artiste, &i.titre)).or_default().push((p, i.duree, t));
                    }
                    // même clé ET durée à 3 s près
                    let mut groupes = vec![];
                    for (_, mut l) in par_cle {
                        l.sort_by_key(|x| x.1);
                        let mut courant: Vec<(PathBuf, u64, u64)> = vec![];
                        for x in l {
                            if courant.last().is_some_and(|y| x.1.abs_diff(y.1) > 3) {
                                if courant.len() > 1 {
                                    groupes.push(std::mem::take(&mut courant));
                                } else {
                                    courant.clear();
                                }
                            }
                            courant.push(x);
                        }
                        if courant.len() > 1 {
                            groupes.push(courant);
                        }
                    }
                    for gr in &mut groupes {
                        gr.sort_by(|a, b| b.2.cmp(&a.2)); // le plus gros (meilleure qualité) en premier
                    }
                    let n = groupes.len();
                    *g.lock().unwrap() = Some(groupes);
                    Ok(format!("{n} chanson(s) en double"))
                });
            }
        }
        let Some(gr) = self.groupes.lock().unwrap().clone() else { return };
        let gagne: u64 = gr.iter().flat_map(|g| g.iter().skip(1)).map(|x| x.2).sum();
        if !gr.is_empty() {
            ui.label(format!("En gardant le meilleur de chaque : {} récupérables.", taille(gagne)));
            if ui.add_enabled(!occupe, egui::Button::new("🗑 Garder le meilleur de chaque, le reste à la corbeille")).clicked() {
                let mut n = 0;
                for g in &gr {
                    for x in g.iter().skip(1) {
                        if crate::disque::corbeille(&x.0) {
                            n += 1;
                        }
                    }
                }
                *self.e.lock().unwrap() = (false, format!("✔ {n} fichier(s) à la corbeille"));
                *self.groupes.lock().unwrap() = Some(vec![]);
            }
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for g in &gr {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    for (k, (p, d, t)) in g.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(if k == 0 { "⭐" } else { "  " });
                            ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                            ui.weak(format!("{}:{:02} · {}", d / 60, d % 60, taille(*t)));
                            if ui.small_button("▶").on_hover_text("Écouter").clicked() {
                                ouvrir(p);
                            }
                        });
                    }
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 25 · Fichiers ouverts récemment
// ---------------------------------------------------------------------------------------------------

const PS_RECENTS: &str = r#"$s = New-Object -ComObject WScript.Shell
$o = Get-ChildItem "$env:APPDATA\Microsoft\Windows\Recent" -Filter *.lnk -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 300 | ForEach-Object {
  $t = $s.CreateShortcut($_.FullName).TargetPath
  if ($t) { [pscustomobject]@{ c = $t; d = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm') } }
}
ConvertTo-Json -Compress -InputObject @($o)"#;

#[derive(Deserialize, Clone)]
struct Recent {
    c: String,
    d: String,
}

pub fn genre_fichier(p: &str) -> &'static str {
    let e = Path::new(p).extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    match e.as_str() {
        "doc" | "docx" | "odt" | "pdf" | "txt" | "rtf" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "odp" | "md" | "csv" => "📄 Documents",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "psd" => "🖼 Images",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "🎬 Vidéos",
        "mp3" | "flac" | "wav" | "ogg" | "m4a" => "🎵 Musique",
        "zip" | "rar" | "7z" => "📦 Archives",
        "" => "📁 Dossiers",
        _ => "🔹 Autres",
    }
}

pub struct Recents {
    l: Arc<Mutex<Option<Vec<Recent>>>>,
    filtre: Option<&'static str>,
    q: String,
}

impl Recents {
    pub fn new() -> Self {
        Recents { l: Arc::default(), filtre: None, q: String::new() }
    }
}

impl Outil for Recents {
    fn nom(&self) -> &'static str {
        "🕘 Fichiers récents"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Fichiers ouverts récemment");
        if self.l.lock().unwrap().is_none() {
            *self.l.lock().unwrap() = Some(vec![]);
            let l = self.l.clone();
            std::thread::spawn(move || {
                let v = crate::optimiseur::ps(PS_RECENTS).ok().and_then(|s| serde_json::from_str::<Vec<Recent>>(s.trim()).ok()).unwrap_or_default();
                *l.lock().unwrap() = Some(v);
            });
        }
        let l = self.l.lock().unwrap().clone().unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.filtre.is_none(), "Tous").clicked() {
                self.filtre = None;
            }
            for g in ["📄 Documents", "🖼 Images", "🎬 Vidéos", "🎵 Musique", "📦 Archives", "📁 Dossiers", "🔹 Autres"] {
                if ui.selectable_label(self.filtre == Some(g), g).clicked() {
                    self.filtre = Some(g);
                }
            }
            ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("🔍").desired_width(140.0));
            if ui.small_button("⟳").clicked() {
                *self.l.lock().unwrap() = None;
            }
        });
        if l.is_empty() {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(400));
            return;
        }
        let q = self.q.to_lowercase();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in l.iter().filter(|r| self.filtre.is_none_or(|f| genre_fichier(&r.c) == f) && (q.is_empty() || r.c.to_lowercase().contains(&q))) {
                let p = Path::new(&r.c);
                let existe = p.exists();
                ui.horizontal(|ui| {
                    ui.weak(&r.d);
                    let n = RichText::new(p.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or(r.c.clone()));
                    if ui.add_enabled(existe, egui::Link::new(n)).clicked() {
                        ouvrir(p);
                    }
                    if existe && ui.small_button("📁").on_hover_text("Afficher dans le dossier").clicked() {
                        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", r.c)).spawn();
                    }
                    ui.weak(p.parent().map(|x| x.display().to_string()).unwrap_or_default());
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(en_stacks(1000, 64), (15, 40));
        assert_eq!(temps_cuisson(640, 8, false), 800.0);
        assert_eq!(temps_cuisson(9, 8, true), 10.0);
        let k = [7u8; 32];
        let b = sceller(&k, "plage.jpg", b"donnees");
        assert_eq!(desceller(&k, &b), Some(("plage.jpg".into(), b"donnees".to_vec())));
        assert_eq!(desceller(&[8u8; 32], &b), None);
        assert_eq!(noms_par_date(&[("2024-05-03 14.22.10".into(), "jpg".into()), ("2024-05-03 14.22.10".into(), "jpg".into()), ("2024-05-04 09.00.00".into(), "png".into())]),
            vec!["2024-05-03 14.22.10.jpg", "2024-05-03 14.22.10 (2).jpg", "2024-05-04 09.00.00.png"]);
        assert_eq!(cle_musique("Daft Punk", "One More Time (Radio Edit)"), cle_musique("daft punk", "One more time"));
        assert_eq!(cle_musique("A", "Song feat. B"), cle_musique("A", "Song"));
        assert_eq!(genre_fichier(r"C:\x\cours.PDF"), "📄 Documents");
        assert_eq!(genre_fichier(r"C:\Jeux"), "📁 Dossiers");
        let d = std::env::temp_dir().join(format!("outils_pdf_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let i = d.join("a.png");
        image::RgbImage::from_pixel(40, 20, image::Rgb([200, 30, 60])).save(&i).unwrap();
        assert_eq!(images_vers_pdf(&[i.clone(), i], &d.join("x.pdf")).unwrap(), 2);
        assert_eq!(lopdf::Document::load(d.join("x.pdf")).unwrap().get_pages().len(), 2);
        let _ = std::fs::remove_dir_all(d);
    }
}
