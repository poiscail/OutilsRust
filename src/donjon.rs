//! Le donjon du ménage (12) : ton PC devient un donjon. Chaque dossier est une salle, les fichiers
//! inutiles sont des monstres (doublons, installateurs oubliés, fichiers temporaires, géants
//! endormis…). Les vaincre les envoie à la corbeille : tu gagnes de l'expérience en libérant de la place.
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::sans_dossiers::{corbeille, dossiers_perso, maintenant, parcourir, taille_lisible};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Espece {
    Rat,
    Gobelin,
    Clone,
    Geant,
    Mimic,
    Fantome,
    Dragon,
}

impl Espece {
    pub fn nom(self) -> &'static str {
        match self {
            Espece::Rat => "Rat des caves",
            Espece::Gobelin => "Gobelin installateur",
            Espece::Clone => "Clone",
            Espece::Geant => "Géant endormi",
            Espece::Mimic => "Mimic",
            Espece::Fantome => "Fantôme",
            Espece::Dragon => "Dragon",
        }
    }
    pub fn sprite(self) -> &'static str {
        match self {
            Espece::Rat => "🐀",
            Espece::Gobelin => "👺",
            Espece::Clone => "👥",
            Espece::Geant => "🗿",
            Espece::Mimic => "📦",
            Espece::Fantome => "👻",
            Espece::Dragon => "🐉",
        }
    }
    fn couleur(self) -> Color32 {
        match self {
            Espece::Rat => Color32::from_rgb(170, 140, 110),
            Espece::Gobelin => Color32::from_rgb(110, 200, 90),
            Espece::Clone => Color32::from_rgb(150, 170, 255),
            Espece::Geant => Color32::from_rgb(190, 190, 200),
            Espece::Mimic => Color32::from_rgb(220, 170, 70),
            Espece::Fantome => Color32::from_rgb(230, 230, 255),
            Espece::Dragon => Color32::from_rgb(240, 80, 60),
        }
    }
    pub fn explication(self) -> &'static str {
        match self {
            Espece::Rat => "Fichier temporaire ou téléchargement jamais terminé : il ne sert plus à rien.",
            Espece::Gobelin => "Installateur déjà utilisé il y a longtemps : le programme est installé (ou abandonné), l'installateur traîne.",
            Espece::Clone => "Copie exacte d'un autre fichier : l'original reste en place.",
            Espece::Geant => "Très gros fichier pas touché depuis plus d'un an.",
            Espece::Mimic => "Archive déjà décompressée : son contenu est juste à côté, dans un dossier du même nom.",
            Espece::Fantome => "Dossier complètement vide.",
            Espece::Dragon => "Le plus gros fichier oublié de tout le donjon.",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Monstre {
    pub chemin: PathBuf,
    pub taille: u64,
    pub espece: Espece,
    pub detail: String,
    pub pv: u32,
    pub pv_max: u32,
}

/// Points de vie : un gros fichier se bat plus longtemps.
fn pv(taille: u64) -> u32 {
    (2.0 + ((taille as f64 / 1_048_576.0) + 1.0).log2() * 0.9).round().clamp(2.0, 18.0) as u32
}

/// Empreinte d'un fichier : début + fin + taille (rapide), puis tout le fichier s'il est petit.
fn empreinte(p: &Path, taille: u64) -> Option<[u8; 32]> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = sha2::Sha256::new();
    if taille <= 64 * 1024 * 1024 {
        let mut b = Vec::new();
        f.read_to_end(&mut b).ok()?;
        h.update(&b);
    } else {
        let mut b = vec![0u8; 1 << 20];
        f.read_exact(&mut b).ok()?;
        h.update(&b);
        f.seek(std::io::SeekFrom::Start(taille / 2)).ok()?;
        f.read_exact(&mut b).ok()?;
        h.update(&b);
        f.seek(std::io::SeekFrom::End(-(1 << 20))).ok()?;
        f.read_exact(&mut b).ok()?;
        h.update(&b);
        h.update(taille.to_le_bytes());
    }
    Some(h.finalize().into())
}

/// Trouve les monstres parmi des fichiers (chemin, taille, modification) et des dossiers vides.
pub fn detecter(fichiers: &[(PathBuf, u64, i64)], vides: &[PathBuf], maintenant: i64, telechargements: &Path, epargnes: &[PathBuf]) -> Vec<Monstre> {
    let mut v = Vec::new();
    let an = 365 * 86400;
    let ext = |p: &Path| p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mut deja = std::collections::HashSet::new();
    for (p, t, m) in fichiers {
        if epargnes.contains(p) {
            continue;
        }
        let e = ext(p);
        let nom = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let vieux = maintenant - m;
        let (espece, detail) = if (matches!(e.as_str(), "tmp" | "temp" | "crdownload" | "part" | "partial" | "download" | "!ut" | "dmp") || nom.ends_with(".old") || nom.starts_with("~")) && vieux > 2 * 86400 {
            (Espece::Rat, "fichier temporaire ou téléchargement interrompu".to_string())
        } else if matches!(e.as_str(), "exe" | "msi" | "msix") && p.parent() == Some(telechargements) && vieux > 30 * 86400 {
            (Espece::Gobelin, format!("installateur téléchargé il y a {} jours", vieux / 86400))
        } else if matches!(e.as_str(), "zip" | "rar" | "7z") && p.with_extension("").is_dir() && vieux > 7 * 86400 {
            (Espece::Mimic, "déjà décompressée juste à côté".to_string())
        } else if *t >= 300 * 1024 * 1024 && vieux > an && !matches!(e.as_str(), "vhd" | "vhdx" | "vmdk" | "pst" | "ost") {
            (Espece::Geant, format!("pas touché depuis {} an(s)", vieux / an))
        } else {
            continue;
        };
        deja.insert(p.clone());
        v.push(Monstre { chemin: p.clone(), taille: *t, espece, detail, pv: pv(*t), pv_max: pv(*t) });
    }
    // clones : même taille, puis même empreinte
    let mut par_taille: HashMap<u64, Vec<&(PathBuf, u64, i64)>> = HashMap::new();
    for f in fichiers.iter().filter(|f| f.1 >= 256 * 1024 && !deja.contains(&f.0) && !epargnes.contains(&f.0)) {
        par_taille.entry(f.1).or_default().push(f);
    }
    for (_, groupe) in par_taille.into_iter().filter(|g| g.1.len() > 1) {
        let mut par_emp: HashMap<[u8; 32], Vec<&(PathBuf, u64, i64)>> = HashMap::new();
        for f in groupe {
            if let Some(e) = empreinte(&f.0, f.1) {
                par_emp.entry(e).or_default().push(f);
            }
        }
        for (_, mut g) in par_emp.into_iter().filter(|g| g.1.len() > 1) {
            // l'original : le plus ancien, de préférence hors des téléchargements
            g.sort_by_key(|f| (f.0.parent() == Some(telechargements), f.2, f.0.to_string_lossy().len()));
            let original = g[0].0.clone();
            for f in &g[1..] {
                v.push(Monstre { chemin: f.0.clone(), taille: f.1, espece: Espece::Clone, detail: format!("copie de {}", original.display()), pv: pv(f.1), pv_max: pv(f.1) });
            }
        }
    }
    for d in vides.iter().filter(|d| !epargnes.contains(d)) {
        v.push(Monstre { chemin: d.clone(), taille: 0, espece: Espece::Fantome, detail: "dossier vide".into(), pv: 1, pv_max: 1 });
    }
    // le dragon : le plus gros de tous
    if let Some(b) = v.iter_mut().filter(|m| m.taille >= 500 * 1024 * 1024).max_by_key(|m| m.taille) {
        b.espece = Espece::Dragon;
        b.pv_max = (b.pv_max * 2).min(30);
        b.pv = b.pv_max;
    }
    v
}

/// Dossiers vides (sans sous-dossiers non vides), hors dossiers personnels.
pub fn dossiers_vides(racines: &[PathBuf], stop: &AtomicBool) -> Vec<PathBuf> {
    let mut v = Vec::new();
    fn vide(d: &Path, prof: usize, v: &mut Vec<PathBuf>, stop: &AtomicBool) -> bool {
        if prof > 12 || stop.load(Relaxed) {
            return false;
        }
        let Ok(it) = std::fs::read_dir(d) else { return false };
        let mut tout_vide = true;
        for e in it.flatten() {
            let nom = e.file_name().to_string_lossy().to_lowercase();
            match e.file_type() {
                Ok(t) if t.is_dir() && !t.is_symlink() => {
                    if nom.starts_with('.') || nom == "appdata" || nom == "node_modules" || !vide(&e.path(), prof + 1, v, stop) {
                        tout_vide = false;
                    }
                }
                _ => tout_vide = false,
            }
        }
        if tout_vide && prof > 0 {
            v.push(d.to_path_buf());
        }
        tout_vide
    }
    for r in racines {
        vide(r, 0, &mut v, stop);
    }
    // seulement le dossier vide le plus haut (ses sous-dossiers vides partent avec lui)
    v.sort();
    let mut garde: Vec<PathBuf> = Vec::new();
    for d in v.into_iter().rev() {
        garde.retain(|g| !g.starts_with(&d));
        garde.push(d);
    }
    garde
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
struct Progres {
    mo: f64,
    vaincus: u32,
    or: u32,
    epargnes: Vec<PathBuf>,
}

fn niveau(mo: f64) -> u32 {
    ((mo / 40.0).sqrt()) as u32 + 1
}

fn titre(n: u32) -> &'static str {
    match n {
        1..=2 => "Apprenti balayeur",
        3..=4 => "Chasseur de rats",
        5..=7 => "Chevalier du rangement",
        8..=11 => "Paladin de l'espace libre",
        12..=17 => "Tueur de dragons",
        _ => "Légende du disque dur",
    }
}

enum Vue {
    Carte,
    Salle(PathBuf),
}

struct Combat {
    monstre: Monstre,
    coups: Vec<(f32, u32, Vec2)>, // âge, dégâts, décalage
    secousse: f32,
    fini: Option<f32>,
}

pub struct Donjon {
    progres: Progres,
    monstres: Arc<Mutex<Vec<Monstre>>>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    vue: Vue,
    choisi: Option<usize>,
    combat: Option<Combat>,
    message: String,
    t: f32,
}

impl Donjon {
    pub fn new() -> Self {
        Donjon { progres: charger("donjon"), monstres: Arc::default(), etat: Arc::default(), occupe: Arc::default(), stop: Arc::default(), vue: Vue::Carte, choisi: None, combat: None, message: String::new(), t: 0.0 }
    }

    fn explorer(&self) {
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (ms, et, oc, st) = (self.monstres.clone(), self.etat.clone(), self.occupe.clone(), self.stop.clone());
        let racines = crate::sans_dossiers::index().lock().unwrap().racines.clone();
        let epargnes = self.progres.epargnes.clone();
        std::thread::spawn(move || {
            *et.lock().unwrap() = "Tu descends dans le donjon…".into();
            let f = parcourir(&racines, &st, 400_000);
            *et.lock().unwrap() = format!("{} salles fouillées, les monstres se réveillent…", f.len());
            let vides = dossiers_vides(&racines, &st);
            let dl = dossiers_perso()[0].2.clone();
            let mut m = detecter(&f, &vides, maintenant(), &dl, &epargnes);
            m.sort_by(|a, b| b.taille.cmp(&a.taille));
            let total: u64 = m.iter().map(|x| x.taille).sum();
            *et.lock().unwrap() = format!("{} monstres rôdent, ils occupent {}", m.len(), taille_lisible(total));
            *ms.lock().unwrap() = m;
            oc.store(false, Relaxed);
        });
    }

    /// Salles = dossiers qui contiennent des monstres.
    fn salles(&self) -> Vec<(PathBuf, Vec<usize>, u64)> {
        let m = self.monstres.lock().unwrap();
        let mut s: Vec<(PathBuf, Vec<usize>, u64)> = Vec::new();
        for (k, x) in m.iter().enumerate() {
            let d = x.chemin.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            match s.iter_mut().find(|e| e.0 == d) {
                Some(e) => {
                    e.1.push(k);
                    e.2 += x.taille;
                }
                None => s.push((d, vec![k], x.taille)),
            }
        }
        s.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.len().cmp(&a.1.len())));
        s
    }

    fn vaincre(&mut self, m: &Monstre) -> bool {
        if !corbeille(&m.chemin) {
            self.message = format!("✖ Impossible d'envoyer {} à la corbeille (ouvert ou protégé ?)", m.chemin.display());
            return false;
        }
        let avant = niveau(self.progres.mo);
        self.progres.mo += m.taille as f64 / 1_048_576.0;
        self.progres.vaincus += 1;
        self.progres.or += 1 + (m.taille / (50 * 1_048_576)) as u32 + if m.espece == Espece::Dragon { 100 } else { 0 };
        let apres = niveau(self.progres.mo);
        self.message = if apres > avant {
            format!("🎉 NIVEAU {apres} ! Tu es maintenant « {} ».", titre(apres))
        } else {
            format!("⚔ {} vaincu ! +{} d'espace libre (il est dans la corbeille).", m.espece.nom(), taille_lisible(m.taille))
        };
        self.monstres.lock().unwrap().retain(|x| x.chemin != m.chemin);
        enregistrer("donjon", &self.progres);
        true
    }

    fn dessiner_salle(&mut self, ui: &mut egui::Ui, dossier: &Path, ids: &[usize]) {
        let ms = self.monstres.lock().unwrap().clone();
        let l = ui.available_width().min(900.0);
        let colonnes = ((l / 110.0) as usize).max(3);
        let rangs = ids.len().div_ceil(colonnes).max(1);
        let h = 120.0 + rangs as f32 * 110.0;
        let (zone, _) = ui.allocate_exact_size(Vec2::new(l, h), Sense::hover());
        let p = ui.painter_at(zone);
        // sol en dalles et murs
        p.rect_filled(zone, 8.0, Color32::from_rgb(38, 34, 44));
        let dalle = 40.0;
        let mut y = zone.top() + 30.0;
        while y < zone.bottom() {
            let mut x = zone.left() + if ((y - zone.top()) / dalle) as i32 % 2 == 0 { 0.0 } else { dalle / 2.0 };
            while x < zone.right() {
                p.rect_stroke(Rect::from_min_size(Pos2::new(x, y), Vec2::splat(dalle)), 2.0, Stroke::new(1.0_f32, Color32::from_rgb(52, 47, 60)), egui::StrokeKind::Inside);
                x += dalle;
            }
            y += dalle;
        }
        p.rect_filled(Rect::from_min_size(zone.min, Vec2::new(l, 28.0)), 8.0, Color32::from_rgb(70, 60, 70));
        for k in 0..((l / 180.0) as i32 + 1) {
            let x = zone.left() + 40.0 + k as f32 * 180.0;
            let f = (self.t * 9.0 + k as f32 * 1.7).sin() * 2.0;
            p.circle_filled(Pos2::new(x, zone.top() + 14.0 + f * 0.3), 6.0 + f, Color32::from_rgba_unmultiplied(255, 170, 60, 200));
            p.text(Pos2::new(x, zone.top() + 22.0), Align2::CENTER_CENTER, "🔥", FontId::proportional(14.0), Color32::from_rgb(255, 150, 40));
        }
        let nom = dossier.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dossier.display().to_string());
        p.text(Pos2::new(zone.center().x, zone.top() + 46.0), Align2::CENTER_CENTER, format!("Salle « {nom} »"), FontId::proportional(16.0), Color32::from_rgb(230, 220, 200));
        // monstres
        for (n, &k) in ids.iter().enumerate() {
            let Some(m) = ms.get(k) else { continue };
            let c = Pos2::new(zone.left() + 60.0 + (n % colonnes) as f32 * 110.0, zone.top() + 120.0 + (n / colonnes) as f32 * 110.0);
            let r = Rect::from_center_size(c, Vec2::splat(90.0));
            let rep = ui.interact(r, ui.id().with(("monstre", k)), Sense::click());
            let flotte = (self.t * 2.0 + n as f32).sin() * 3.0;
            let taille = if m.espece == Espece::Dragon { 56.0 } else { 38.0 + (m.pv_max as f32).min(12.0) };
            if rep.hovered() || self.choisi == Some(k) {
                p.circle_filled(c, 40.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
            }
            p.text(c + Vec2::new(0.0, flotte - 8.0), Align2::CENTER_CENTER, m.espece.sprite(), FontId::proportional(taille), m.espece.couleur());
            let barre = Rect::from_min_size(c + Vec2::new(-28.0, 26.0), Vec2::new(56.0, 5.0));
            p.rect_filled(barre, 2.0, Color32::from_gray(30));
            p.rect_filled(Rect::from_min_size(barre.min, Vec2::new(56.0 * m.pv as f32 / m.pv_max as f32, 5.0)), 2.0, Color32::from_rgb(220, 60, 60));
            p.text(c + Vec2::new(0.0, 40.0), Align2::CENTER_CENTER, taille_lisible(m.taille), FontId::proportional(11.0), Color32::from_gray(200));
            if rep.clicked() {
                self.choisi = Some(k);
            }
            rep.on_hover_text(format!("{} — {}", m.espece.nom(), m.chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }

    fn dessiner_combat(&mut self, ui: &mut egui::Ui, dt: f32) {
        let niv = niveau(self.progres.mo);
        let Some(c) = &mut self.combat else { return };
        let (zone, rep) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(700.0), 300.0), Sense::click());
        let p = ui.painter_at(zone);
        p.rect_filled(zone, 10.0, Color32::from_rgb(25, 20, 30));
        c.secousse = (c.secousse - dt * 4.0).max(0.0);
        let s = Vec2::new((self_t_hash(c.coups.len()) - 0.5) * 16.0 * c.secousse, 0.0);
        let centre = zone.center() + Vec2::new(90.0, -10.0) + s;
        let m = &c.monstre;
        let echelle = if m.espece == Espece::Dragon { 110.0 } else { 80.0 };
        let alpha = c.fini.map(|f| (1.0 - f).clamp(0.0, 1.0)).unwrap_or(1.0);
        let col = if c.secousse > 0.6 { Color32::WHITE } else { m.espece.couleur() };
        p.text(centre, Align2::CENTER_CENTER, m.espece.sprite(), FontId::proportional(echelle * (1.0 + c.fini.unwrap_or(0.0))), col.gamma_multiply(alpha));
        // héros
        let heros = zone.center() + Vec2::new(-170.0, 20.0);
        p.text(heros, Align2::CENTER_CENTER, "🧙", FontId::proportional(64.0), Color32::from_rgb(200, 180, 255));
        p.text(heros + Vec2::new(0.0, 52.0), Align2::CENTER_CENTER, format!("Niveau {niv}"), FontId::proportional(13.0), Color32::from_gray(220));
        // barre de vie du monstre
        let b = Rect::from_min_size(Pos2::new(centre.x - 80.0, zone.top() + 24.0), Vec2::new(160.0, 10.0));
        p.rect_filled(b, 3.0, Color32::from_gray(40));
        p.rect_filled(Rect::from_min_size(b.min, Vec2::new(160.0 * m.pv as f32 / m.pv_max as f32, 10.0)), 3.0, Color32::from_rgb(220, 60, 60));
        p.text(b.center_top() - Vec2::new(0.0, 6.0), Align2::CENTER_BOTTOM, m.espece.nom(), FontId::proportional(14.0), Color32::WHITE);
        // dégâts qui s'envolent
        for (age, d, dec) in &mut c.coups {
            *age += dt;
            let a = (1.0 - *age).clamp(0.0, 1.0);
            p.text(centre + *dec - Vec2::new(0.0, *age * 60.0), Align2::CENTER_CENTER, format!("-{d}"), FontId::proportional(22.0), Color32::from_rgb(255, 230, 80).gamma_multiply(a));
        }
        c.coups.retain(|x| x.0 < 1.0);
        if c.fini.is_none() {
            p.text(zone.center_bottom() - Vec2::new(0.0, 16.0), Align2::CENTER_CENTER, "Clique pour frapper !", FontId::proportional(14.0), Color32::from_gray(180));
            if rep.clicked() {
                let degats = 1 + niv / 3 + (crate::outil::hasard(3) as u32 == 0) as u32;
                c.monstre.pv = c.monstre.pv.saturating_sub(degats);
                c.coups.push((0.0, degats, Vec2::new(crate::outil::hasard(40) as f32 - 20.0, -30.0)));
                c.secousse = 1.0;
                if c.monstre.pv == 0 {
                    c.fini = Some(0.0);
                }
            }
        } else if let Some(f) = &mut c.fini {
            *f += dt * 1.5;
        }
        ui.ctx().request_repaint();
    }
}

/// Petit bruit déterministe pour la secousse.
fn self_t_hash(n: usize) -> f32 {
    ((n as f32 * 12.9898).sin() * 43758.547).fract().abs()
}

impl Outil for Donjon {
    fn nom(&self) -> &'static str {
        "⚔ Donjon du ménage"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.t += dt;
        let niv = niveau(self.progres.mo);
        ui.horizontal(|ui| {
            ui.heading("⚔ Donjon du ménage");
            ui.add_space(20.0);
            ui.label(RichText::new(format!("🧙 Niveau {niv} · {}", titre(niv))).strong().color(Color32::from_rgb(200, 180, 255)));
            ui.label(format!("✨ {} libérés · 💀 {} vaincus · 🪙 {}", taille_lisible((self.progres.mo * 1_048_576.0) as u64), self.progres.vaincus, self.progres.or));
        });
        // barre d'expérience
        let (a, b) = (((niv - 1) as f64).powi(2) * 40.0, (niv as f64).powi(2) * 40.0);
        ui.add(egui::ProgressBar::new(((self.progres.mo - a) / (b - a)) as f32).desired_height(6.0).desired_width(300.0));
        ui.weak("Chaque dossier est une salle, les fichiers inutiles sont des monstres. Les vaincre les envoie à la corbeille : rien n'est perdu pour de vrai.");
        let vide = self.monstres.lock().unwrap().is_empty();
        ui.horizontal(|ui| {
            if self.occupe.load(Relaxed) {
                ui.spinner();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            } else if ui.button(RichText::new(if vide { "🕯 Entrer dans le donjon" } else { "🔄 Explorer à nouveau" }).strong()).clicked() {
                self.vue = Vue::Carte;
                self.explorer();
            }
            ui.label(self.etat.lock().unwrap().clone());
        });
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).color(Color32::GOLD));
        }
        ui.separator();
        // combat en cours
        if self.combat.is_some() {
            self.dessiner_combat(ui, dt);
            let fin = self.combat.as_ref().and_then(|c| c.fini).is_some_and(|f| f > 1.0);
            if fin {
                let m = self.combat.take().unwrap().monstre;
                self.vaincre(&m);
                self.choisi = None;
            } else if ui.button("🏃 Fuir").clicked() {
                self.combat = None;
            }
            return;
        }
        match &self.vue {
            Vue::Carte => {
                let salles = self.salles();
                if salles.is_empty() {
                    if !self.occupe.load(Relaxed) && !self.etat.lock().unwrap().is_empty() {
                        ui.label(RichText::new("🏆 Plus aucun monstre ! Ton PC est propre.").size(18.0));
                    }
                    return;
                }
                ui.label(RichText::new("🗺 Carte du donjon").strong());
                let ms = self.monstres.lock().unwrap().clone();
                if let Some(d) = ms.iter().find(|m| m.espece == Espece::Dragon) {
                    ui.colored_label(Color32::from_rgb(240, 100, 80), format!("🐉 Un dragon de {} dort dans « {} »…", taille_lisible(d.taille), d.chemin.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (d, ids, total) in salles {
                            let nom = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.display().to_string());
                            let sprites: String = ids.iter().take(6).filter_map(|k| ms.get(*k)).map(|m| m.espece.sprite()).collect::<Vec<_>>().join("");
                            let (r, rep) = ui.allocate_exact_size(Vec2::new(170.0, 96.0), Sense::click());
                            let p = ui.painter_at(r);
                            p.rect_filled(r, 6.0, if rep.hovered() { Color32::from_rgb(62, 55, 72) } else { Color32::from_rgb(45, 40, 52) });
                            p.rect_stroke(r, 6.0, Stroke::new(2.0_f32, Color32::from_rgb(90, 80, 70)), egui::StrokeKind::Inside);
                            p.text(r.center_top() + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, &nom, FontId::proportional(13.0), Color32::from_rgb(230, 220, 200));
                            p.text(r.center(), Align2::CENTER_CENTER, sprites, FontId::proportional(20.0), Color32::from_rgb(220, 200, 170));
                            p.text(r.center_bottom() - Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, format!("{} monstre(s) · {}", ids.len(), taille_lisible(total)), FontId::proportional(11.0), Color32::from_gray(180));
                            if rep.on_hover_text(d.display().to_string()).clicked() {
                                self.vue = Vue::Salle(d.clone());
                                self.choisi = None;
                            }
                        }
                    });
                });
            }
            Vue::Salle(d) => {
                let d = d.clone();
                if ui.button("⬅ Retour à la carte").clicked() {
                    self.vue = Vue::Carte;
                    return;
                }
                let ids = self.salles().into_iter().find(|s| s.0 == d).map(|s| s.1).unwrap_or_default();
                if ids.is_empty() {
                    ui.label("🏆 Salle nettoyée !");
                    return;
                }
                self.dessiner_salle(ui, &d, &ids);
                // sort de zone : tous les rats d'un coup
                let ms = self.monstres.lock().unwrap().clone();
                let rats: Vec<Monstre> = ids.iter().filter_map(|k| ms.get(*k)).filter(|m| matches!(m.espece, Espece::Rat | Espece::Fantome)).cloned().collect();
                if rats.len() >= 3 {
                    let ok = niv >= 3;
                    if ui.add_enabled(ok, egui::Button::new(format!("🌪 Tornade : chasser les {} rats et fantômes d'un coup", rats.len()))).on_disabled_hover_text("Débloqué au niveau 3").clicked() {
                        for r in rats {
                            self.vaincre(&r);
                        }
                    }
                }
                if let Some(m) = self.choisi.and_then(|k| ms.get(k)).cloned() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new(format!("{} {}", m.espece.sprite(), m.espece.nom())).size(18.0).color(m.espece.couleur()));
                        ui.label(RichText::new(m.chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).strong());
                        ui.weak(format!("{} · {}", taille_lisible(m.taille), m.detail));
                        ui.label(m.espece.explication());
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("⚔ Combattre (→ corbeille)").strong()).clicked() {
                                self.combat = Some(Combat { monstre: m.clone(), coups: vec![], secousse: 0.0, fini: None });
                            }
                            if ui.button("🕊 Épargner (le garder)").on_hover_text("Il ne reviendra plus").clicked() {
                                self.progres.epargnes.push(m.chemin.clone());
                                enregistrer("donjon", &self.progres);
                                self.monstres.lock().unwrap().retain(|x| x.chemin != m.chemin);
                                self.choisi = None;
                            }
                            if ui.button("👁 Voir").clicked() {
                                crate::outil::ouvrir(if m.espece == Espece::Fantome { &m.chemin } else { m.chemin.parent().unwrap_or(&m.chemin) });
                            }
                        });
                    });
                } else {
                    ui.weak("Clique sur un monstre pour savoir ce que c'est.");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monstres() {
        let d = std::env::temp_dir().join(format!("donjon_{}", std::process::id()));
        let dl = d.join("Downloads");
        std::fs::create_dir_all(dl.join("jeu")).unwrap();
        std::fs::create_dir_all(d.join("Vide/Plus vide")).unwrap();
        std::fs::create_dir_all(d.join("Photos")).unwrap();
        let gros = vec![7u8; 300 * 1024];
        std::fs::write(d.join("Photos/a.jpg"), &gros).unwrap();
        std::fs::write(dl.join("a (1).jpg"), &gros).unwrap();
        std::fs::write(dl.join("setup.exe"), b"MZ").unwrap();
        std::fs::write(dl.join("jeu.zip"), b"PK").unwrap();
        std::fs::write(dl.join("film.mp4.crdownload"), b"..").unwrap();
        std::fs::write(dl.join("rien.txt"), b"x").unwrap();
        std::fs::write(dl.join("jeu/lisezmoi.txt"), b"x").unwrap();
        let st = AtomicBool::new(false);
        let maintenant = maintenant() + 60 * 86400; // dans deux mois
        let f = parcourir(&[d.clone()], &st, 100);
        let vides = dossiers_vides(&[d.clone()], &st);
        assert_eq!(vides, vec![d.join("Vide")]);
        let m = detecter(&f, &vides, maintenant, &dl, &[]);
        let espece = |n: &str| m.iter().find(|x| x.chemin.file_name().is_some_and(|f| f == n)).map(|x| x.espece);
        assert_eq!(espece("setup.exe"), Some(Espece::Gobelin));
        assert_eq!(espece("jeu.zip"), Some(Espece::Mimic));
        assert_eq!(espece("film.mp4.crdownload"), Some(Espece::Rat));
        assert_eq!(espece("a (1).jpg"), Some(Espece::Clone)); // la copie, pas l'original
        assert_eq!(espece("a.jpg"), None);
        assert_eq!(espece("rien.txt"), None);
        assert_eq!(espece("Vide"), Some(Espece::Fantome));
        // épargné = ne revient pas
        let m2 = detecter(&f, &vides, maintenant, &dl, &[dl.join("setup.exe")]);
        assert!(!m2.iter().any(|x| x.chemin.ends_with("setup.exe")));
        // la corbeille marche (Linux : ~/.local/share/Trash)
        #[cfg(not(windows))]
        {
            std::env::set_var("HOME", &d);
            assert!(corbeille(&dl.join("setup.exe")));
            assert!(d.join(".local/share/Trash/files/setup.exe").exists());
        }
        assert!(pv(1) < pv(5_000_000_000));
        assert_eq!(niveau(0.0), 1);
        assert!(niveau(10_000.0) > 10);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
