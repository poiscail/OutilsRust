//! Minecraft, suite : règles du jeu (17) et monde (18 : heure, météo, spawn, difficulté), sauvegarde dans le cloud (19),
//! redémarrage programmé avec annonces (20), page de statut web (21), calculatrice Nether (22),
//! chat du serveur vers Discord (23), datapacks (24).
use crate::minecraft::Pilote;
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

fn sans_serveur(ui: &mut egui::Ui, mc: &Pilote) -> Option<PathBuf> {
    let d = mc.dossier();
    if d.is_none() {
        ui.weak("Aucun serveur configuré (onglet « Installer un serveur » ou « Serveur Minecraft »).");
    }
    d
}

/// (heure, minute) locales.
fn heure_minute() -> (u32, u32) {
    let h = crate::minecraft::horodatage();
    let n = |a: usize| h.get(a..a + 2).and_then(|x| x.parse().ok()).unwrap_or(0);
    (n(11), n(14))
}

// ---------------------------------------------------------------------------------------------------
// NBT (format des fichiers de Minecraft) : lecture et écriture
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    Str(String),
    List(u8, Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Nbt {
    fn id(&self) -> u8 {
        match self {
            Nbt::Byte(_) => 1,
            Nbt::Short(_) => 2,
            Nbt::Int(_) => 3,
            Nbt::Long(_) => 4,
            Nbt::Float(_) => 5,
            Nbt::Double(_) => 6,
            Nbt::ByteArray(_) => 7,
            Nbt::Str(_) => 8,
            Nbt::List(..) => 9,
            Nbt::Compound(_) => 10,
            Nbt::IntArray(_) => 11,
            Nbt::LongArray(_) => 12,
        }
    }

    pub fn get(&self, k: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(v) => v.iter().find(|(n, _)| n == k).map(|x| &x.1),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, k: &str) -> Option<&mut Nbt> {
        match self {
            Nbt::Compound(v) => v.iter_mut().find(|(n, _)| n == k).map(|x| &mut x.1),
            _ => None,
        }
    }

    /// Remplace ou ajoute une entrée d'un compound.
    pub fn mettre(&mut self, k: &str, val: Nbt) {
        if let Nbt::Compound(v) = self {
            match v.iter_mut().find(|(n, _)| n == k) {
                Some(x) => x.1 = val,
                None => v.push((k.to_string(), val)),
            }
        }
    }

    pub fn nombre(&self) -> Option<i64> {
        match self {
            Nbt::Byte(x) => Some(*x as i64),
            Nbt::Short(x) => Some(*x as i64),
            Nbt::Int(x) => Some(*x as i64),
            Nbt::Long(x) => Some(*x),
            _ => None,
        }
    }
}

struct Lecteur<'a>(&'a [u8], usize);

impl Lecteur<'_> {
    fn pr(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.0.get(self.1..self.1 + n).ok_or("fichier NBT tronqué")?;
        self.1 += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.pr(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes(self.pr(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(self.pr(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_be_bytes(self.pr(8)?.try_into().unwrap()))
    }
    fn texte(&mut self) -> Result<String, String> {
        let n = self.i16()? as u16 as usize;
        Ok(String::from_utf8_lossy(self.pr(n)?).into_owned())
    }
    fn n(&mut self) -> Result<usize, String> {
        let n = self.i32()?;
        if !(0..=50_000_000).contains(&n) {
            return Err("taille NBT invalide".into());
        }
        Ok(n as usize)
    }
    fn valeur(&mut self, t: u8, prof: u32) -> Result<Nbt, String> {
        if prof > 512 {
            return Err("NBT trop profond".into());
        }
        Ok(match t {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_bits(self.i32()? as u32)),
            6 => Nbt::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.n()?;
                Nbt::ByteArray(self.pr(n)?.iter().map(|b| *b as i8).collect())
            }
            8 => Nbt::Str(self.texte()?),
            9 => {
                let et = self.u8()?;
                let n = self.n()?;
                let mut v = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    v.push(self.valeur(et, prof + 1)?);
                }
                Nbt::List(et, v)
            }
            10 => {
                let mut v = vec![];
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.texte()?;
                    v.push((k, self.valeur(t, prof + 1)?));
                }
                Nbt::Compound(v)
            }
            11 => {
                let n = self.n()?;
                (0..n).map(|_| self.i32()).collect::<Result<_, _>>().map(Nbt::IntArray)?
            }
            12 => {
                let n = self.n()?;
                (0..n).map(|_| self.i64()).collect::<Result<_, _>>().map(Nbt::LongArray)?
            }
            _ => return Err(format!("type NBT inconnu {t}")),
        })
    }
}

/// Lit un NBT non compressé : (nom de la racine, valeur).
pub fn lire_nbt(b: &[u8]) -> Result<(String, Nbt), String> {
    let mut l = Lecteur(b, 0);
    let t = l.u8()?;
    let nom = l.texte()?;
    Ok((nom, l.valeur(t, 0)?))
}

fn ecrire_val(o: &mut Vec<u8>, v: &Nbt) {
    let texte = |o: &mut Vec<u8>, s: &str| {
        o.extend((s.len() as u16).to_be_bytes());
        o.extend(s.as_bytes());
    };
    match v {
        Nbt::Byte(x) => o.push(*x as u8),
        Nbt::Short(x) => o.extend(x.to_be_bytes()),
        Nbt::Int(x) => o.extend(x.to_be_bytes()),
        Nbt::Long(x) => o.extend(x.to_be_bytes()),
        Nbt::Float(x) => o.extend(x.to_bits().to_be_bytes()),
        Nbt::Double(x) => o.extend(x.to_bits().to_be_bytes()),
        Nbt::ByteArray(a) => {
            o.extend((a.len() as i32).to_be_bytes());
            o.extend(a.iter().map(|x| *x as u8));
        }
        Nbt::Str(s) => texte(o, s),
        Nbt::List(t, l) => {
            o.push(if l.is_empty() { *t } else { l[0].id() });
            o.extend((l.len() as i32).to_be_bytes());
            for x in l {
                ecrire_val(o, x);
            }
        }
        Nbt::Compound(c) => {
            for (k, x) in c {
                o.push(x.id());
                texte(o, k);
                ecrire_val(o, x);
            }
            o.push(0);
        }
        Nbt::IntArray(a) => {
            o.extend((a.len() as i32).to_be_bytes());
            for x in a {
                o.extend(x.to_be_bytes());
            }
        }
        Nbt::LongArray(a) => {
            o.extend((a.len() as i32).to_be_bytes());
            for x in a {
                o.extend(x.to_be_bytes());
            }
        }
    }
}

pub fn ecrire_nbt(nom: &str, v: &Nbt) -> Vec<u8> {
    let mut o = vec![v.id()];
    o.extend((nom.len() as u16).to_be_bytes());
    o.extend(nom.as_bytes());
    ecrire_val(&mut o, v);
    o
}

fn lire_level(p: &Path) -> Result<(String, Nbt), String> {
    let mut b = vec![];
    flate2::read::GzDecoder::new(std::fs::File::open(p).map_err(|e| e.to_string())?).read_to_end(&mut b).map_err(|e| format!("level.dat illisible : {e}"))?;
    lire_nbt(&b)
}

fn ecrire_level(p: &Path, nom: &str, v: &Nbt) -> Result<(), String> {
    let _ = std::fs::copy(p, p.with_extension("dat_avant_outils"));
    let mut g = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    g.write_all(&ecrire_nbt(nom, v)).map_err(|e| e.to_string())?;
    let b = g.finish().map_err(|e| e.to_string())?;
    let tmp = p.with_extension("dat_tmp");
    std::fs::write(&tmp, b).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, p).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------------
// 17 / 18 · Règles et monde
// ---------------------------------------------------------------------------------------------------

/// (nom, description, est un nombre)
pub const REGLES: [(&str, &str, bool); 20] = [
    ("keepInventory", "Garder l'inventaire en mourant", false),
    ("doDaylightCycle", "Le jour et la nuit avancent", false),
    ("doWeatherCycle", "La météo change", false),
    ("mobGriefing", "Les monstres cassent des blocs (creepers…)", false),
    ("doFireTick", "Le feu se propage", false),
    ("doMobSpawning", "Les monstres apparaissent", false),
    ("doInsomnia", "Les phantoms apparaissent", false),
    ("doPatrolSpawning", "Patrouilles de pillards", false),
    ("doTraderSpawning", "Marchand ambulant", false),
    ("naturalRegeneration", "La vie remonte toute seule", false),
    ("fallDamage", "Dégâts de chute", false),
    ("fireDamage", "Dégâts de feu", false),
    ("drowningDamage", "Noyade", false),
    ("doImmediateRespawn", "Réapparition sans l'écran de mort", false),
    ("showDeathMessages", "Messages de mort dans le chat", false),
    ("announceAdvancements", "Annoncer les progrès", false),
    ("playersSleepingPercentage", "% de joueurs qui doivent dormir", true),
    ("randomTickSpeed", "Vitesse de pousse (3 = normal)", true),
    ("spawnRadius", "Rayon d'apparition", true),
    ("maxEntityCramming", "Entités entassées max", true),
];

/// « Gamerule keepInventory is currently set to: false » → ("keepInventory", "false")
pub fn lire_reponse_regle(l: &str) -> Option<(String, String)> {
    let c = regex::Regex::new(r"Gamerule (\w+) is currently set to: (\w+)").unwrap().captures(l)?;
    Some((c[1].to_string(), c[2].to_string()))
}

#[derive(Clone, Default)]
struct Monde {
    regles: Vec<(String, String)>,
    temps: Option<i64>,
    pluie: bool,
    orage: bool,
    spawn: (i32, i32, i32),
    difficulte: i64,
}

fn lire_monde(dossier: &Path) -> Result<Monde, String> {
    let (_, n) = lire_level(&dossier.join(crate::minecraft::nom_monde(dossier)).join("level.dat"))?;
    let d = n.get("Data").ok_or("level.dat sans Data")?;
    let regles = match d.get("GameRules") {
        Some(Nbt::Compound(v)) => v.iter().map(|(k, x)| (k.clone(), match x {
            Nbt::Str(s) => s.clone(),
            Nbt::Byte(b) => (if *b != 0 { "true" } else { "false" }).to_string(),
            x => x.nombre().map(|n| n.to_string()).unwrap_or_default(),
        })).collect(),
        _ => vec![],
    };
    let i = |k: &str| d.get(k).and_then(Nbt::nombre).unwrap_or(0);
    Ok(Monde { regles, temps: d.get("DayTime").and_then(Nbt::nombre), pluie: i("raining") != 0, orage: i("thundering") != 0, spawn: (i("SpawnX") as i32, i("SpawnY") as i32, i("SpawnZ") as i32), difficulte: i("Difficulty") })
}

fn modifier_monde(dossier: &Path, f: impl FnOnce(&mut Nbt)) -> Result<(), String> {
    let p = dossier.join(crate::minecraft::nom_monde(dossier)).join("level.dat");
    let (nom, mut n) = lire_level(&p)?;
    f(n.get_mut("Data").ok_or("level.dat sans Data")?);
    ecrire_level(&p, &nom, &n)
}

pub struct ReglesMonde {
    mc: Pilote,
    m: Arc<Mutex<Option<Result<Monde, String>>>>,
    spawn: (i32, i32, i32),
    e: Etat,
}

impl ReglesMonde {
    pub fn new(mc: Pilote) -> Self {
        ReglesMonde { mc, m: Arc::default(), spawn: (0, 64, 0), e: Arc::default() }
    }

    fn charger(&self, dossier: PathBuf) {
        let (m, mc) = (self.m.clone(), self.mc.clone());
        std::thread::spawn(move || {
            if mc.statut().is_some_and(|s| s.0) {
                // serveur en marche : on demande chaque règle à la console
                for (r, ..) in REGLES {
                    mc.commande(&format!("gamerule {r}"));
                }
                std::thread::sleep(Duration::from_millis(1500));
                let regles: Vec<(String, String)> = mc.dernieres().iter().filter_map(|l| lire_reponse_regle(l)).collect();
                let mut base = lire_monde(&dossier).unwrap_or_default();
                for (k, v) in regles {
                    match base.regles.iter_mut().find(|x| x.0 == k) {
                        Some(x) => x.1 = v,
                        None => base.regles.push((k, v)),
                    }
                }
                *m.lock().unwrap() = Some(Ok(base));
            } else {
                *m.lock().unwrap() = Some(lire_monde(&dossier));
            }
        });
    }
}

impl Outil for ReglesMonde {
    fn nom(&self) -> &'static str {
        "📜 Règles du jeu et monde"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Règles du jeu et monde");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let en_ligne = self.mc.statut().is_some_and(|s| s.0);
        ui.weak(if en_ligne { "Serveur en marche : les changements s'appliquent tout de suite." } else { "Serveur arrêté : les changements sont écrits dans level.dat (une copie de l'ancien est gardée)." });
        if self.m.lock().unwrap().is_none() {
            *self.m.lock().unwrap() = Some(Err("lecture…".into()));
            self.charger(d.clone());
        }
        let occupe = etat_ui(ui, &self.e);
        let monde = self.m.lock().unwrap().clone();
        let m = match monde {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                ui.weak(e);
                if ui.small_button("⟳").clicked() {
                    *self.m.lock().unwrap() = None;
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
                return;
            }
            None => return,
        };
        let mut appliquer: Vec<(String, String)> = vec![];
        let mut monde_cmd: Option<(String, Box<dyn FnOnce(&mut Nbt) + Send>)> = None;
        ui.strong("Monde");
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Heure : {}", m.temps.map(|t| { let h = (t % 24_000 + 6_000) % 24_000; format!("{}h{:02}", h / 1000, h % 1000 * 60 / 1000) }).unwrap_or("?".into())));
            for (n, t) in [("☀ Matin", 1_000), ("🌞 Midi", 6_000), ("🌇 Soir", 12_000), ("🌙 Nuit", 18_000)] {
                if ui.add_enabled(!occupe, egui::Button::new(n)).clicked() {
                    monde_cmd = Some((format!("time set {t}"), Box::new(move |d: &mut Nbt| d.mettre("DayTime", Nbt::Long(t)))));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Météo : {}", if m.orage { "orage" } else if m.pluie { "pluie" } else { "beau temps" }));
            for (n, cmd, p, o) in [("☀ Beau temps", "weather clear", false, false), ("🌧 Pluie", "weather rain", true, false), ("⛈ Orage", "weather thunder", true, true)] {
                if ui.add_enabled(!occupe, egui::Button::new(n)).clicked() {
                    monde_cmd = Some((cmd.to_string(), Box::new(move |d: &mut Nbt| {
                        d.mettre("raining", Nbt::Byte(p as i8));
                        d.mettre("thundering", Nbt::Byte(o as i8));
                        d.mettre("rainTime", Nbt::Int(if p { 12_000 } else { 0 }));
                        d.mettre("clearWeatherTime", Nbt::Int(if p { 0 } else { 12_000 }));
                    })));
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Difficulté :");
            for (k, n) in ["paisible", "facile", "normale", "difficile"].iter().enumerate() {
                let cmd = ["peaceful", "easy", "normal", "hard"][k];
                if ui.add_enabled(!occupe, egui::SelectableLabel::new(m.difficulte == k as i64, *n)).clicked() {
                    monde_cmd = Some((format!("difficulty {cmd}"), Box::new(move |d: &mut Nbt| d.mettre("Difficulty", Nbt::Byte(k as i8)))));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(format!("Spawn : {} {} {}", m.spawn.0, m.spawn.1, m.spawn.2));
            ui.add(egui::DragValue::new(&mut self.spawn.0).prefix("x "));
            ui.add(egui::DragValue::new(&mut self.spawn.1).prefix("y "));
            ui.add(egui::DragValue::new(&mut self.spawn.2).prefix("z "));
            if ui.add_enabled(!occupe, egui::Button::new("Déplacer le spawn")).clicked() {
                let s = self.spawn;
                monde_cmd = Some((format!("setworldspawn {} {} {}", s.0, s.1, s.2), Box::new(move |d: &mut Nbt| {
                    d.mettre("SpawnX", Nbt::Int(s.0));
                    d.mettre("SpawnY", Nbt::Int(s.1));
                    d.mettre("SpawnZ", Nbt::Int(s.2));
                })));
            }
        });
        ui.separator();
        ui.strong("Règles du jeu");
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
            egui::Grid::new("regles").striped(true).num_columns(2).show(ui, |ui| {
                for (r, desc, nombre) in REGLES {
                    let Some((_, v)) = m.regles.iter().find(|x| x.0 == r) else { continue };
                    ui.label(desc).on_hover_text(r);
                    if nombre {
                        let mut n: i64 = v.parse().unwrap_or(0);
                        if ui.add(egui::DragValue::new(&mut n).range(0..=1000)).changed() {
                            appliquer.push((r.to_string(), n.to_string()));
                        }
                    } else {
                        let mut b = v == "true";
                        if ui.checkbox(&mut b, "").changed() {
                            appliquer.push((r.to_string(), b.to_string()));
                        }
                    }
                    ui.end_row();
                }
            });
        });
        if let Some((cmd, f)) = monde_cmd {
            if en_ligne {
                self.mc.commande(&cmd);
            } else {
                let dd = d.clone();
                en_fond(&self.e, move || modifier_monde(&dd, f).map(|_| "✔ Monde modifié.".to_string()));
            }
            *self.m.lock().unwrap() = None;
        }
        if !appliquer.is_empty() {
            if en_ligne {
                for (r, v) in &appliquer {
                    self.mc.commande(&format!("gamerule {r} {v}"));
                }
            } else {
                let dd = d.clone();
                let a = appliquer.clone();
                en_fond(&self.e, move || {
                    modifier_monde(&dd, move |data| {
                        if let Some(g) = data.get_mut("GameRules") {
                            for (r, v) in a {
                                // on garde le type d'origine (texte dans les anciennes versions)
                                let nv = match g.get(&r) {
                                    Some(Nbt::Byte(_)) => Nbt::Byte((v == "true") as i8),
                                    Some(Nbt::Int(_)) => Nbt::Int(v.parse().unwrap_or(0)),
                                    _ => Nbt::Str(v),
                                };
                                g.mettre(&r, nv);
                            }
                        }
                    }).map(|_| "✔ Règles enregistrées.".to_string())
                });
            }
            // mise à jour immédiate de l'affichage
            if let Some(Ok(mm)) = self.m.lock().unwrap().as_mut() {
                for (r, v) in appliquer {
                    if let Some(x) = mm.regles.iter_mut().find(|x| x.0 == r) {
                        x.1 = v;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 19 · Sauvegarde dans le cloud
// ---------------------------------------------------------------------------------------------------

/// Dossiers synchronisés trouvés sur le PC : (nom, chemin).
pub fn dossiers_cloud() -> Vec<(String, PathBuf)> {
    let u = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    let mut v: Vec<(String, PathBuf)> = vec![];
    for var in ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"] {
        if let Ok(p) = std::env::var(var) {
            v.push(("OneDrive".into(), PathBuf::from(p)));
        }
    }
    for (n, p) in [("Google Drive", u.join("Google Drive")), ("Google Drive", u.join("Mon Drive")), ("Dropbox", u.join("Dropbox")), ("iCloud", u.join("iCloudDrive"))] {
        v.push((n.into(), p));
    }
    for l in ['G', 'H', 'I'] {
        v.push(("Google Drive".into(), PathBuf::from(format!("{l}:\\Mon Drive"))));
        v.push(("Google Drive".into(), PathBuf::from(format!("{l}:\\My Drive"))));
    }
    let mut vus = std::collections::HashSet::new();
    v.retain(|(_, p)| p.is_dir() && vus.insert(p.clone()));
    v
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageCloud {
    pub dossier: String,
    pub heures: u32,
    pub garder: usize,
    pub actif: bool,
    pub dernier: String,
    pub dernier_t: u64,
}

impl Default for ReglageCloud {
    fn default() -> Self {
        ReglageCloud { dossier: String::new(), heures: 6, garder: 5, actif: false, dernier: String::new(), dernier_t: 0 }
    }
}

fn envoyer_cloud(mc: &Pilote, r: &ReglageCloud) -> Result<String, String> {
    let zip = mc.sauvegarder()?;
    let monde = mc.dossier().map(|d| crate::minecraft::nom_monde(&d)).unwrap_or("monde".into());
    let dest = PathBuf::from(&r.dossier).join(format!("Minecraft - {monde}"));
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let cible = dest.join(zip.file_name().ok_or("nom ?")?);
    std::fs::copy(&zip, &cible).map_err(|e| e.to_string())?;
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dest).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "zip")).collect();
    v.sort();
    while v.len() > r.garder.max(1) {
        let _ = std::fs::remove_file(v.remove(0));
    }
    Ok(format!("✔ {} copié dans {}", cible.file_name().unwrap_or_default().to_string_lossy(), dest.display()))
}

fn secondes() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub struct SauvegardeCloud {
    mc: Pilote,
    r: Arc<Mutex<ReglageCloud>>,
    e: Etat,
}

impl SauvegardeCloud {
    pub fn new(mc: Pilote) -> Self {
        let r: Arc<Mutex<ReglageCloud>> = Arc::new(Mutex::new(charger("sauvegarde_cloud")));
        let (r2, mc2) = (r.clone(), mc.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(300));
            let c = r2.lock().unwrap().clone();
            if c.actif && !c.dossier.is_empty() && mc2.dossier().is_some() && secondes() >= c.dernier_t + c.heures as u64 * 3600 {
                let m = envoyer_cloud(&mc2, &c).unwrap_or_else(|e| format!("✖ {e}"));
                crate::coeur::noter("Sauvegarde cloud Minecraft", &m);
                let mut x = r2.lock().unwrap();
                x.dernier = format!("{} — {m}", crate::minecraft::horodatage());
                x.dernier_t = secondes();
                enregistrer("sauvegarde_cloud", &*x);
            }
        });
        SauvegardeCloud { mc, r, e: Arc::default() }
    }
}

impl Outil for SauvegardeCloud {
    fn nom(&self) -> &'static str {
        "☁ Sauvegarde du monde dans le cloud"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegarde du monde dans le cloud");
        ui.weak("Une copie du monde part régulièrement dans OneDrive, Google Drive ou Dropbox : même si le PC lâche, le monde est sauf.");
        if sans_serveur(ui, &self.mc).is_none() {
            return;
        }
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal_wrapped(|ui| {
            for (n, p) in dossiers_cloud() {
                let s = p.display().to_string();
                if ui.selectable_label(r.dossier == s, format!("☁ {n}")).on_hover_text(&s).clicked() {
                    r.dossier = s;
                }
            }
            if ui.button("📁 Autre dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    r.dossier = d.display().to_string();
                }
            }
        });
        if r.dossier.is_empty() {
            ui.label("Choisis un dossier synchronisé.");
        } else {
            ui.weak(format!("Destination : {}", r.dossier));
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Automatique toutes les");
            ui.add(egui::DragValue::new(&mut r.heures).range(1..=168).suffix(" h"));
            ui.label("· garder les");
            ui.add(egui::DragValue::new(&mut r.garder).range(1..=50));
            ui.label("dernières");
        });
        let occupe = etat_ui(ui, &self.e);
        if ui.add_enabled(!occupe && !r.dossier.is_empty(), egui::Button::new("☁ Envoyer une sauvegarde maintenant")).clicked() {
            let (mc, c, rr) = (self.mc.clone(), r.clone(), self.r.clone());
            en_fond(&self.e, move || {
                let m = envoyer_cloud(&mc, &c)?;
                let mut x = rr.lock().unwrap();
                x.dernier = format!("{} — {m}", crate::minecraft::horodatage());
                x.dernier_t = secondes();
                enregistrer("sauvegarde_cloud", &*x);
                Ok(m)
            });
        }
        if !r.dernier.is_empty() {
            ui.weak(format!("Dernière : {}", r.dernier));
        }
        if r != avant {
            let mut x = self.r.lock().unwrap();
            // le fil a pu mettre à jour « dernier » entre-temps
            r.dernier = x.dernier.clone();
            r.dernier_t = x.dernier_t;
            *x = r;
            enregistrer("sauvegarde_cloud", &*x);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 20 · Redémarrage programmé avec annonces
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageRedemarrage {
    pub actif: bool,
    pub heure: u32,
    pub minute: u32,
    pub message: String,
}

impl Default for ReglageRedemarrage {
    fn default() -> Self {
        ReglageRedemarrage { actif: false, heure: 5, minute: 0, message: "Redémarrage du serveur dans {temps} !".into() }
    }
}

/// Minutes avant l'heure prévue (0..1440), depuis (h, m) actuels.
pub fn minutes_avant(h: u32, m: u32, cible: (u32, u32)) -> u32 {
    let (a, b) = (h * 60 + m, cible.0 * 60 + cible.1);
    (b + 1440 - a) % 1440
}

pub fn annonce(modele: &str, temps: &str) -> String {
    crate::mcplus2::tellraw(&modele.replace("{temps}", temps))
}

pub struct Redemarrage {
    r: Arc<Mutex<ReglageRedemarrage>>,
    etat: Arc<Mutex<String>>,
}

impl Redemarrage {
    pub fn new(mc: Pilote) -> Self {
        let r: Arc<Mutex<ReglageRedemarrage>> = Arc::new(Mutex::new(charger("redemarrage_mc")));
        let etat: Arc<Mutex<String>> = Arc::default();
        let (r2, e2) = (r.clone(), etat.clone());
        std::thread::spawn(move || {
            let mut annonces_faites: Vec<u32> = vec![];
            loop {
                std::thread::sleep(Duration::from_secs(15));
                let c = r2.lock().unwrap().clone();
                if !c.actif || !mc.statut().is_some_and(|s| s.0) {
                    annonces_faites.clear();
                    continue;
                }
                let (h, m) = heure_minute();
                let reste = minutes_avant(h, m, (c.heure, c.minute));
                for a in [10, 5, 1] {
                    if reste == a && !annonces_faites.contains(&a) {
                        annonces_faites.push(a);
                        mc.commande(&annonce(&c.message, &format!("{a} minute{}", if a > 1 { "s" } else { "" })));
                    }
                }
                if reste == 0 && !annonces_faites.contains(&0) {
                    annonces_faites.push(0);
                    for s in [10, 5, 3, 2, 1] {
                        mc.commande(&annonce(&c.message, &format!("{s} s")));
                        std::thread::sleep(Duration::from_secs(if s == 10 { 5 } else if s == 5 { 2 } else { 1 }));
                    }
                    *e2.lock().unwrap() = format!("{h}h{m:02} : arrêt…");
                    mc.arreter();
                    std::thread::sleep(Duration::from_secs(5));
                    let r = mc.demarrer();
                    let msg = match r {
                        Ok(()) => format!("✔ Redémarré à {h}h{m:02}"),
                        Err(e) => format!("✖ Redémarrage raté : {e}"),
                    };
                    crate::coeur::noter("Redémarrage Minecraft", &msg);
                    *e2.lock().unwrap() = msg;
                    std::thread::sleep(Duration::from_secs(90)); // ne pas recommencer dans la même minute
                    annonces_faites.clear();
                }
            }
        });
        Redemarrage { r, etat }
    }
}

impl Outil for Redemarrage {
    fn nom(&self) -> &'static str {
        "🔁 Redémarrage programmé"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Redémarrage programmé du serveur");
        ui.weak("Un serveur qui tourne des jours finit par ramer : un redémarrage par jour (la nuit) le garde fluide. Les joueurs sont prévenus 10 min, 5 min, 1 min puis 10 s avant.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Chaque jour à");
            ui.add(egui::DragValue::new(&mut r.heure).range(0..=23).suffix(" h"));
            ui.add(egui::DragValue::new(&mut r.minute).range(0..=59).suffix(" min"));
        });
        ui.horizontal(|ui| {
            ui.label("Message");
            ui.add(egui::TextEdit::singleline(&mut r.message).desired_width(360.0));
        });
        ui.weak("{temps} est remplacé par « 5 minutes », « 10 s »…");
        if r.actif {
            let (h, m) = heure_minute();
            let reste = minutes_avant(h, m, (r.heure, r.minute));
            ui.label(format!("Prochain redémarrage dans {} h {:02}", reste / 60, reste % 60));
        }
        let e = self.etat.lock().unwrap().clone();
        if !e.is_empty() {
            ui.label(e);
        }
        if r != avant {
            enregistrer("redemarrage_mc", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 21 · Page de statut web
// ---------------------------------------------------------------------------------------------------

fn echapper(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// MOTD sans les codes couleur (§a…).
pub fn motd_propre(s: &str) -> String {
    let s = s.replace("\\u00A7", "§").replace("\\n", " ");
    let mut o = String::new();
    let mut saute = false;
    for c in s.chars() {
        if saute {
            saute = false;
            continue;
        }
        if c == '§' {
            saute = true;
            continue;
        }
        o.push(c);
    }
    o.trim().to_string()
}

fn page_statut(mc: &Pilote) -> String {
    let d = mc.dossier();
    let props = d.as_ref().and_then(|d| std::fs::read_to_string(d.join("server.properties")).ok()).unwrap_or_default();
    let motd = motd_propre(&crate::mcplus::propriete(&props, "motd").unwrap_or("Serveur Minecraft".into()));
    let max = crate::mcplus::propriete(&props, "max-players").unwrap_or("20".into());
    let version = crate::mcplus::version_du_jar(&mc.jar()).unwrap_or_default();
    let (etat, couleur, joueurs) = match mc.statut() {
        None => ("Hors ligne", "#ff6b6b", vec![]),
        Some((false, _)) => ("Démarrage…", "#ffd166", vec![]),
        Some((true, j)) => ("En ligne", "#06d6a0", j),
    };
    let liste: String = joueurs.iter().map(|j| format!("<div class=j><img src=\"https://mc-heads.net/avatar/{0}/40\" alt=\"\"><span>{0}</span></div>", echapper(j))).collect();
    format!(r#"<!doctype html><html lang=fr><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1"><meta http-equiv=refresh content=30>
<title>{titre}</title><style>body{{margin:0;font-family:system-ui,sans-serif;background:#120a10;color:#f5dce8;display:flex;justify-content:center;padding:24px}}
.c{{max-width:520px;width:100%;background:#1e1019;border:1px solid #3a1f33;border-radius:18px;padding:22px}}h1{{margin:0 0 6px;font-size:24px}}
.e{{display:inline-block;padding:4px 12px;border-radius:999px;background:{couleur};color:#120a10;font-weight:700}}.p{{color:#b49aaa;margin:10px 0 18px}}
.j{{display:flex;align-items:center;gap:12px;padding:8px 0;border-top:1px solid #3a1f33}}.j img{{border-radius:6px}}</style></head>
<body><div class=c><h1>⛏ {titre}</h1><span class=e>{etat}</span><div class=p>{n} / {max} joueurs{v}</div>{liste}{vide}</div></body></html>"#,
        titre = echapper(&motd), n = joueurs.len(), v = if version.is_empty() { String::new() } else { format!(" · Minecraft {version}") },
        vide = if joueurs.is_empty() && etat == "En ligne" { "<div class=p>Personne pour l'instant.</div>" } else { "" })
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageStatut {
    pub actif: bool,
    pub port: u16,
}

impl Default for ReglageStatut {
    fn default() -> Self {
        ReglageStatut { actif: false, port: 8095 }
    }
}

pub struct PageStatut {
    mc: Pilote,
    r: ReglageStatut,
    marche: Arc<AtomicBool>,
    arret: Arc<AtomicBool>,
    erreur: Arc<Mutex<Option<String>>>,
}

impl PageStatut {
    pub fn new(mc: Pilote) -> Self {
        let r: ReglageStatut = charger("page_statut");
        let mut p = PageStatut { mc, r: r.clone(), marche: Arc::default(), arret: Arc::default(), erreur: Arc::default() };
        if r.actif {
            p.demarrer();
        }
        p
    }

    fn demarrer(&mut self) {
        self.arret = Arc::default();
        let (mc, port, marche, arret, err) = (self.mc.clone(), self.r.port, self.marche.clone(), self.arret.clone(), self.erreur.clone());
        std::thread::spawn(move || {
            let l = match std::net::TcpListener::bind(("0.0.0.0", port)) {
                Ok(l) => l,
                Err(e) => {
                    *err.lock().unwrap() = Some(format!("port {port} indisponible : {e}"));
                    return;
                }
            };
            let _ = l.set_nonblocking(true);
            *err.lock().unwrap() = None;
            marche.store(true, Relaxed);
            while !arret.load(Relaxed) {
                match l.accept() {
                    Ok((mut s, _)) => {
                        let _ = s.set_nonblocking(false);
                        let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                        let mut b = [0u8; 2048];
                        let _ = s.read(&mut b);
                        let corps = page_statut(&mc);
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}", corps.len());
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
            marche.store(false, Relaxed);
        });
    }
}

impl Outil for PageStatut {
    fn nom(&self) -> &'static str {
        "🌐 Page de statut du serveur"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Page de statut du serveur");
        ui.weak("Une petite page web (en ligne ou non, qui est connecté, avec les têtes) à partager aux amis. Elle se met à jour toute seule.");
        let marche = self.marche.load(Relaxed);
        ui.horizontal(|ui| {
            ui.label("Port");
            ui.add_enabled(!marche, egui::DragValue::new(&mut self.r.port).range(1024..=65535));
            if marche {
                if ui.button("⏹ Arrêter la page").clicked() {
                    self.arret.store(true, Relaxed);
                    self.r.actif = false;
                    enregistrer("page_statut", &self.r);
                }
            } else if ui.button("▶ Lancer la page").clicked() {
                self.r.actif = true;
                enregistrer("page_statut", &self.r);
                self.demarrer();
            }
        });
        if let Some(e) = self.erreur.lock().unwrap().clone() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        if marche {
            let ip = crate::telephone::ip_locale().unwrap_or("localhost".into());
            for (t, u) in [("Sur ce PC", format!("http://localhost:{}", self.r.port)), ("Maison (Wi-Fi)", format!("http://{ip}:{}", self.r.port))] {
                ui.horizontal(|ui| {
                    ui.label(t);
                    if ui.link(&u).clicked() {
                        let _ = std::process::Command::new("explorer").arg(&u).spawn();
                    }
                    if ui.small_button("📋").clicked() {
                        ui.ctx().copy_text(u.clone());
                    }
                });
            }
            ui.weak(format!("Pour les amis hors de chez toi : ouvre aussi le port {} (outil « Ouvrir les ports ») et donne-leur ton IP publique avec ce port.", self.r.port));
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

// ---------------------------------------------------------------------------------------------------
// 22 · Calculatrice Nether ↔ surface
// ---------------------------------------------------------------------------------------------------

pub fn vers_nether(x: f64, z: f64) -> (f64, f64) {
    ((x / 8.0).floor(), (z / 8.0).floor())
}

pub fn vers_surface(x: f64, z: f64) -> (f64, f64) {
    (x * 8.0, z * 8.0)
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Lieu {
    pub nom: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub nether: bool,
}

pub struct CalcNether {
    x: f64,
    y: f64,
    z: f64,
    nether: bool,
    nom: String,
    lieux: Vec<Lieu>,
}

impl CalcNether {
    pub fn new() -> Self {
        CalcNether { x: 0.0, y: 64.0, z: 0.0, nether: false, nom: String::new(), lieux: charger("lieux_minecraft") }
    }
}

impl Outil for CalcNether {
    fn nom(&self) -> &'static str {
        "🧮 Coordonnées Nether"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Coordonnées Nether ↔ surface");
        ui.weak("Pour relier deux portails : 1 bloc dans le Nether = 8 blocs à la surface. Construis le portail du Nether aux coordonnées données.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.nether, false, "🌍 Je suis à la surface");
            ui.selectable_value(&mut self.nether, true, "🔥 Je suis dans le Nether");
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.x).prefix("X ").speed(1.0));
            ui.add(egui::DragValue::new(&mut self.y).prefix("Y ").speed(1.0));
            ui.add(egui::DragValue::new(&mut self.z).prefix("Z ").speed(1.0));
        });
        let (a, b) = if self.nether { vers_surface(self.x, self.z) } else { vers_nether(self.x, self.z) };
        ui.label(RichText::new(format!("{} : X {a:.0} · Y {:.0} · Z {b:.0}", if self.nether { "🌍 Surface" } else { "🔥 Nether" }, self.y.clamp(1.0, if self.nether { 300.0 } else { 120.0 }))).size(22.0).strong());
        if !self.nether && self.y > 120.0 {
            ui.weak("Dans le Nether, reste sous Y 120 (le plafond de bedrock est à 128).");
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("nom du lieu (base, village…)").desired_width(200.0));
            if ui.button("📌 Garder").clicked() && !self.nom.trim().is_empty() {
                self.lieux.push(Lieu { nom: std::mem::take(&mut self.nom), x: self.x, y: self.y, z: self.z, nether: self.nether });
                enregistrer("lieux_minecraft", &self.lieux);
            }
        });
        let mut retirer = None;
        egui::Grid::new("lieux").striped(true).show(ui, |ui| {
            for (k, l) in self.lieux.iter().enumerate() {
                let (s, n) = if l.nether { (vers_surface(l.x, l.z), (l.x, l.z)) } else { ((l.x, l.z), vers_nether(l.x, l.z)) };
                ui.label(RichText::new(&l.nom).strong());
                ui.label(format!("🌍 {:.0} / {:.0}", s.0, s.1));
                ui.label(format!("🔥 {:.0} / {:.0}", n.0, n.1));
                if ui.small_button("✖").clicked() {
                    retirer = Some(k);
                }
                ui.end_row();
            }
        });
        if let Some(k) = retirer {
            self.lieux.remove(k);
            enregistrer("lieux_minecraft", &self.lieux);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 23 · Chat du serveur vers Discord
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Evenement {
    Chat(String, String),
    Arrivee(String),
    Depart(String),
    Progres(String, String),
}

pub fn evenement(l: &str) -> Option<Evenement> {
    let corps = l.split_once("]: ").map(|x| x.1)?;
    if let Some(c) = regex::Regex::new(r"^(?:\[Not Secure\] )?<(\w{2,16})> (.+)$").unwrap().captures(corps) {
        return Some(Evenement::Chat(c[1].to_string(), c[2].to_string()));
    }
    if let Some(c) = regex::Regex::new(r"^(\w{2,16}) joined the game$").unwrap().captures(corps) {
        return Some(Evenement::Arrivee(c[1].to_string()));
    }
    if let Some(c) = regex::Regex::new(r"^(\w{2,16}) left the game$").unwrap().captures(corps) {
        return Some(Evenement::Depart(c[1].to_string()));
    }
    if let Some(c) = regex::Regex::new(r"^(\w{2,16}) has (?:made the advancement|completed the challenge|reached the goal) \[(.+)\]$").unwrap().captures(corps) {
        return Some(Evenement::Progres(c[1].to_string(), c[2].to_string()));
    }
    None
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageChat {
    pub actif: bool,
    pub webhook: String,
    pub arrivees: bool,
    pub progres: bool,
}

impl Default for ReglageChat {
    fn default() -> Self {
        ReglageChat { actif: false, webhook: String::new(), arrivees: true, progres: true }
    }
}

fn poster(webhook: &str, nom: &str, texte: &str) {
    let corps = serde_json::json!({ "username": nom, "avatar_url": format!("https://mc-heads.net/avatar/{nom}/64"), "content": texte, "allowed_mentions": { "parse": [] } });
    let _ = ureq::post(webhook).timeout(Duration::from_secs(10)).send_json(corps);
}

pub struct ChatDiscord {
    r: Arc<Mutex<ReglageChat>>,
    envoyes: Arc<Mutex<u32>>,
}

impl ChatDiscord {
    pub fn new(mc: Pilote) -> Self {
        let r: Arc<Mutex<ReglageChat>> = Arc::new(Mutex::new(charger("chat_discord")));
        let envoyes: Arc<Mutex<u32>> = Arc::default();
        let (r2, n2) = (r.clone(), envoyes.clone());
        std::thread::spawn(move || {
            let mut vues: std::collections::HashSet<String> = mc.dernieres().into_iter().collect();
            let mut en_ligne = true;
            let mut dernier_envoi = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if mc.statut().is_none() {
                    en_ligne = false;
                    continue;
                }
                let lignes = mc.dernieres();
                if !en_ligne {
                    vues = lignes.iter().cloned().collect();
                    en_ligne = true;
                    continue;
                }
                let c = r2.lock().unwrap().clone();
                for l in &lignes {
                    if !vues.insert(l.clone()) || !c.actif || !c.webhook.starts_with("https://") {
                        continue;
                    }
                    let msg = match evenement(l) {
                        Some(Evenement::Chat(j, t)) => Some((j, t)),
                        Some(Evenement::Arrivee(j)) if c.arrivees => Some((j.clone(), format!("➡ *{j} a rejoint le serveur*"))),
                        Some(Evenement::Depart(j)) if c.arrivees => Some((j.clone(), format!("⬅ *{j} a quitté le serveur*"))),
                        Some(Evenement::Progres(j, p)) if c.progres => Some((j.clone(), format!("🏆 *{j} a obtenu le progrès **{p}***"))),
                        _ => None,
                    };
                    if let Some((j, t)) = msg {
                        // Discord limite à ~5 messages / 2 s par webhook
                        let attente = Duration::from_millis(450).saturating_sub(dernier_envoi.elapsed());
                        std::thread::sleep(attente);
                        poster(&c.webhook, &j, &t);
                        dernier_envoi = Instant::now();
                        *n2.lock().unwrap() += 1;
                    }
                }
                if vues.len() > 3000 {
                    vues = lignes.into_iter().collect();
                }
            }
        });
        ChatDiscord { r, envoyes }
    }
}

impl Outil for ChatDiscord {
    fn nom(&self) -> &'static str {
        "💬 Chat du serveur vers Discord"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chat du serveur vers Discord");
        ui.weak("Les messages du chat Minecraft (et les arrivées, départs, progrès) sont recopiés dans un salon Discord, avec la tête de chaque joueur.");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.label("Lien du webhook : dans Discord, Paramètres du salon › Intégrations › Webhooks › Nouveau webhook › Copier l'URL.");
        ui.add(egui::TextEdit::singleline(&mut r.webhook).password(true).hint_text("https://discord.com/api/webhooks/…").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Actif");
            ui.checkbox(&mut r.arrivees, "Arrivées et départs");
            ui.checkbox(&mut r.progres, "Progrès");
            if ui.add_enabled(r.webhook.starts_with("https://"), egui::Button::new("Tester")).clicked() {
                let w = r.webhook.clone();
                std::thread::spawn(move || poster(&w, "Outils", "✅ Le chat du serveur Minecraft arrivera ici."));
            }
        });
        ui.label(format!("{} message(s) envoyé(s) depuis le lancement d'Outils.", self.envoyes.lock().unwrap()));
        if r != avant {
            enregistrer("chat_discord", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 24 · Datapacks
// ---------------------------------------------------------------------------------------------------

pub struct Datapacks {
    mc: Pilote,
    recherche: String,
    res: Arc<Mutex<Option<Result<Vec<(String, String, String)>, String>>>>, // (id, titre, description)
    e: Etat,
}

impl Datapacks {
    pub fn new(mc: Pilote) -> Self {
        Datapacks { mc, recherche: String::new(), res: Arc::default(), e: Arc::default() }
    }
}

impl Outil for Datapacks {
    fn nom(&self) -> &'static str {
        "📦 Datapacks"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Datapacks du monde");
        ui.weak("Des ajouts sans mods (nouvelles recettes, structures, têtes de joueurs, arbres qui tombent d'un coup…), même sur un serveur Vanilla.");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let monde = d.join(crate::minecraft::nom_monde(&d));
        let (actifs, off) = (monde.join("datapacks"), monde.join("datapacks_desactives"));
        let en_ligne = self.mc.statut().is_some_and(|s| s.0);
        let lister = |p: &Path| -> Vec<String> {
            let mut v: Vec<String> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            v
        };
        let mut deplacer: Option<(PathBuf, PathBuf, String, bool)> = None;
        for (titre, dossier, autre, actif) in [("✅ Actifs", &actifs, &off, true), ("⏸ Désactivés", &off, &actifs, false)] {
            let l = lister(dossier);
            if l.is_empty() && !actif {
                continue;
            }
            ui.strong(format!("{titre} ({})", l.len()));
            for n in l {
                ui.horizontal(|ui| {
                    ui.label(&n);
                    if ui.small_button(if actif { "Désactiver" } else { "Activer" }).clicked() {
                        deplacer = Some((dossier.join(&n), autre.join(&n), n.clone(), actif));
                    }
                    if ui.small_button("🗑").clicked() {
                        let p = dossier.join(&n);
                        let _ = crate::disque::corbeille(&p);
                    }
                });
            }
        }
        if let Some((de, vers, n, actif)) = deplacer {
            let _ = std::fs::create_dir_all(vers.parent().unwrap());
            if std::fs::rename(&de, &vers).is_ok() && en_ligne {
                self.mc.commande(&format!("datapack {} \"file/{n}\"", if actif { "disable" } else { "enable" }));
                self.mc.commande("reload");
            }
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter un datapack (.zip)…").clicked() {
                if let Some(fs) = rfd::FileDialog::new().add_filter("Datapack", &["zip"]).pick_files() {
                    let _ = std::fs::create_dir_all(&actifs);
                    for f in fs {
                        let _ = std::fs::copy(&f, actifs.join(f.file_name().unwrap_or_default()));
                    }
                    if en_ligne {
                        self.mc.commande("reload");
                    }
                }
            }
            if ui.button("📂 Dossier").clicked() {
                let _ = std::fs::create_dir_all(&actifs);
                crate::outil::ouvrir(&actifs);
            }
        });
        ui.separator();
        ui.strong("Trouver des datapacks (Modrinth)");
        let version = crate::mcplus::version_du_jar(&self.mc.jar()).unwrap_or_default();
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("ex. head drops, tree capitator, more crafting").desired_width(300.0));
            if ui.button("🔍").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let (q, res, v) = (self.recherche.clone(), self.res.clone(), version.clone());
                *res.lock().unwrap() = Some(Err("recherche…".into()));
                std::thread::spawn(move || {
                    let mut facettes = vec!["[\"project_type:datapack\"]".to_string()];
                    if !v.is_empty() {
                        facettes.push(format!("[\"versions:{v}\"]"));
                    }
                    let r = ureq::get("https://api.modrinth.com/v2/search").query("query", &q).query("facets", &format!("[{}]", facettes.join(","))).query("limit", "15").call()
                        .map_err(|e| e.to_string()).and_then(|r| r.into_json::<serde_json::Value>().map_err(|e| e.to_string())).map(|j| {
                            j["hits"].as_array().into_iter().flatten().filter_map(|h| Some((h["project_id"].as_str()?.to_string(), h["title"].as_str()?.to_string(), h["description"].as_str().unwrap_or("").to_string()))).collect()
                        });
                    *res.lock().unwrap() = Some(r);
                });
            }
        });
        let occupe = etat_ui(ui, &self.e);
        match self.res.lock().unwrap().clone() {
            Some(Ok(v)) => {
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (id, t, desc) in v {
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!occupe, egui::Button::new("📥")).clicked() {
                                let (a, ver, mc) = (actifs.clone(), version.clone(), self.mc.clone());
                                en_fond(&self.e, move || {
                                    let n = crate::mcplus::installer_projet(&id, "datapack", &ver, &a)?;
                                    if mc.statut().is_some_and(|s| s.0) {
                                        mc.commande("reload");
                                    }
                                    Ok(format!("✔ {n} installé"))
                                });
                            }
                            ui.strong(t);
                            ui.weak(desc.chars().take(90).collect::<String>());
                        });
                    }
                });
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbt_aller_retour() {
        let n = Nbt::Compound(vec![
            ("Data".into(), Nbt::Compound(vec![
                ("DayTime".into(), Nbt::Long(1234)),
                ("raining".into(), Nbt::Byte(1)),
                ("GameRules".into(), Nbt::Compound(vec![("keepInventory".into(), Nbt::Str("false".into()))])),
                ("Liste".into(), Nbt::List(3, vec![Nbt::Int(1), Nbt::Int(2)])),
                ("Vide".into(), Nbt::List(0, vec![])),
                ("Arr".into(), Nbt::LongArray(vec![1, -2])),
                ("F".into(), Nbt::Float(1.5)),
                ("D".into(), Nbt::Double(-2.25)),
                ("B".into(), Nbt::ByteArray(vec![1, -1])),
                ("I".into(), Nbt::IntArray(vec![7])),
                ("S".into(), Nbt::Short(-3)),
                ("Nom".into(), Nbt::Str("Ça marche".into())),
            ])),
        ]);
        let b = ecrire_nbt("", &n);
        let (nom, l) = lire_nbt(&b).unwrap();
        assert_eq!(nom, "");
        assert_eq!(l, n);
        let mut m = l;
        m.get_mut("Data").unwrap().get_mut("GameRules").unwrap().mettre("keepInventory", Nbt::Str("true".into()));
        assert_eq!(m.get("Data").unwrap().get("GameRules").unwrap().get("keepInventory"), Some(&Nbt::Str("true".into())));
        assert!(lire_nbt(&b[..b.len() - 3]).is_err());
    }

    #[test]
    fn divers() {
        assert_eq!(lire_reponse_regle("[12:00:00] [Server thread/INFO]: Gamerule keepInventory is currently set to: false"), Some(("keepInventory".into(), "false".into())));
        assert_eq!(minutes_avant(4, 50, (5, 0)), 10);
        assert_eq!(minutes_avant(5, 1, (5, 0)), 1439);
        assert_eq!(minutes_avant(5, 0, (5, 0)), 0);
        assert_eq!(vers_nether(100.0, -100.0), (12.0, -13.0));
        assert_eq!(vers_surface(12.0, -13.0), (96.0, -104.0));
        assert_eq!(motd_propre("§aMon §lServeur"), "Mon Serveur");
        assert_eq!(evenement("[12:00:00] [Server thread/INFO]: <Bob> salut tout le monde"), Some(Evenement::Chat("Bob".into(), "salut tout le monde".into())));
        assert_eq!(evenement("[12:00:00] [Async Chat Thread - #0/INFO]: [Not Secure] <Lea> hey"), Some(Evenement::Chat("Lea".into(), "hey".into())));
        assert_eq!(evenement("[12:00:00] [Server thread/INFO]: Bob joined the game"), Some(Evenement::Arrivee("Bob".into())));
        assert_eq!(evenement("[12:00:00] [Server thread/INFO]: Bob has made the advancement [Stone Age]"), Some(Evenement::Progres("Bob".into(), "Stone Age".into())));
        assert_eq!(evenement("[12:00:00] [Server thread/INFO]: Done (3.2s)!"), None);
    }
}
