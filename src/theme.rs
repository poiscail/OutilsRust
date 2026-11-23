//! Personnalisation : couleurs, transparence, zoom, image de fond, police, effets et options Windows.
//! Réglages dans %APPDATA%\Outils\theme.json, polices importées dans %APPDATA%\Outils\polices.
use crate::systeme;
use eframe::egui::{self, Color32, Pos2, Rect, RichText};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::f32::consts::TAU;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    sombre: bool,
    accent: [u8; 3],
    fond: [u8; 3],
    panneau: [u8; 3],
    texte: [u8; 3],
    opacite: f32,   // des panneaux, 0..1
    assombrir: f32, // voile noir sur l'image, 0..1
    zoom: f32,
    image: Option<PathBuf>,
    police: Option<String>, // nom du fichier dans le dossier des polices
    petales: bool,
    pluie: bool,
    lucioles: bool,
    neige: bool,
    intensite: f32, // nombre de particules, 0.3..3
    rotation: bool, // change d'image de fond tout seul
    rotation_min: u32,
    fond_windows: bool, // l'image de fond devient aussi celle du bureau
    notifs: bool,
    demarrage: bool,
    pub zone: bool,
    raccourci: bool,
    pub maj: bool,
    fonds_installes: Vec<String>, // fonds de base déjà déposés une fois (pas remis si supprimés)
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            sombre: true, accent: [90, 140, 220], fond: [20, 20, 24], panneau: [27, 27, 27], texte: [210, 210, 210], opacite: 1.0, assombrir: 0.3, zoom: 1.0, image: None,
            police: None, petales: false, pluie: false, lucioles: false, neige: false, intensite: 1.0, rotation: false, rotation_min: 10, fond_windows: false, notifs: false, demarrage: false, zone: false, raccourci: false, maj: true,
            fonds_installes: Vec::new(),
        }
    }
}

/// (nom, réglages) des thèmes prêts à l'emploi (seules les couleurs sont reprises)
fn prereglages() -> Vec<(&'static str, Reglages)> {
    let d = Reglages::default();
    vec![
        ("Sombre", d.clone()),
        ("Clair", Reglages { sombre: false, accent: [40, 110, 210], fond: [235, 235, 240], panneau: [248, 248, 248], texte: [30, 30, 30], ..d.clone() }),
        ("Sakura nuit", Reglages { accent: [255, 95, 162], fond: [18, 10, 16], panneau: [30, 16, 26], texte: [245, 220, 232], opacite: 0.78, assombrir: 0.35, ..d.clone() }),
        ("Néon Tokyo", Reglages { accent: [255, 60, 200], fond: [10, 8, 20], panneau: [20, 14, 34], texte: [235, 225, 255], opacite: 0.72, assombrir: 0.4, ..d.clone() }),
        ("Cyberpunk", Reglages { accent: [250, 230, 40], fond: [8, 10, 18], panneau: [16, 22, 36], texte: [120, 240, 255], opacite: 0.85, assombrir: 0.45, ..d.clone() }),
        ("Minimal", Reglages { sombre: false, accent: [20, 20, 20], fond: [250, 250, 250], panneau: [255, 255, 255], texte: [20, 20, 20], opacite: 1.0, assombrir: 0.0, ..d.clone() }),
        ("Forêt", Reglages { accent: [120, 200, 120], fond: [14, 22, 16], panneau: [22, 34, 26], texte: [220, 235, 215], opacite: 0.85, assombrir: 0.35, ..d.clone() }),
        ("Océan", Reglages { accent: [70, 190, 255], fond: [8, 18, 32], panneau: [14, 30, 50], texte: [215, 235, 250], opacite: 0.85, assombrir: 0.35, ..d.clone() }),
        ("Coucher de soleil", Reglages { accent: [255, 140, 70], fond: [30, 14, 20], panneau: [46, 22, 30], texte: [255, 228, 210], opacite: 0.82, assombrir: 0.3, ..d.clone() }),
        ("Café", Reglages { accent: [200, 150, 100], fond: [28, 22, 18], panneau: [40, 32, 26], texte: [235, 222, 205], opacite: 0.9, assombrir: 0.3, ..d.clone() }),
        ("Lavande", Reglages { sombre: false, accent: [140, 100, 220], fond: [240, 234, 250], panneau: [250, 246, 255], texte: [40, 30, 60], opacite: 0.92, assombrir: 0.1, ..d }),
    ]
}

/// Thème demandé par un profil (nom d'un thème prêt ou d'un de « mes thèmes »).
pub static DEMANDE: Mutex<Option<String>> = Mutex::new(None);

/// Noms de tous les thèmes : prêts puis « mes thèmes ».
pub fn noms_themes() -> Vec<String> {
    let mut v: Vec<String> = prereglages().into_iter().map(|(n, _)| n.to_string()).collect();
    v.extend(crate::outil::charger::<Vec<ThemePartage>>("mes_themes").into_iter().map(|t| t.nom));
    v
}

/// Thème partageable : couleurs, effets et police (pas les options du programme).
#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ThemePartage {
    pub nom: String,
    sombre: bool,
    accent: [u8; 3],
    fond: [u8; 3],
    panneau: [u8; 3],
    texte: [u8; 3],
    opacite: f32,
    assombrir: f32,
    petales: bool,
    pluie: bool,
    lucioles: bool,
    neige: bool,
    intensite: f32,
    police: Option<String>,
}

impl ThemePartage {
    fn depuis(nom: &str, r: &Reglages) -> Self {
        ThemePartage { nom: nom.into(), sombre: r.sombre, accent: r.accent, fond: r.fond, panneau: r.panneau, texte: r.texte, opacite: r.opacite, assombrir: r.assombrir, petales: r.petales, pluie: r.pluie, lucioles: r.lucioles, neige: r.neige, intensite: r.intensite, police: r.police.clone() }
    }
    fn appliquer(&self, r: &mut Reglages) {
        (r.sombre, r.accent, r.fond, r.panneau, r.texte, r.opacite, r.assombrir) = (self.sombre, self.accent, self.fond, self.panneau, self.texte, self.opacite, self.assombrir);
        (r.petales, r.pluie, r.lucioles, r.neige) = (self.petales, self.pluie, self.lucioles, self.neige);
        if self.intensite > 0.0 {
            r.intensite = self.intensite;
        }
        // la police n'est reprise que si elle est installée ici
        if self.police.as_ref().is_none_or(|p| polices_dispo().contains(p)) {
            r.police = self.police.clone();
        }
    }
}

/// Code texte d'un thème, à coller dans un message (« OUTILS-THEME: » + JSON en Base64).
pub fn code_theme(t: &ThemePartage) -> String {
    use base64::Engine;
    format!("OUTILS-THEME:{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(t).unwrap_or_default()))
}

pub fn lire_code(s: &str) -> Option<ThemePartage> {
    use base64::Engine;
    let c = s.trim().strip_prefix("OUTILS-THEME:")?;
    let t: ThemePartage = serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(c.trim()).ok()?).ok()?;
    (!t.nom.trim().is_empty() && t.nom.len() <= 60).then_some(t)
}

/// Thèmes de la galerie : ceux fournis avec Outils, puis les nouveaux publiés en ligne.
fn charger_galerie() -> Vec<ThemePartage> {
    let mut v: Vec<ThemePartage> = serde_json::from_str(include_str!("../themes/galerie.json")).unwrap_or_default();
    let en_ligne = ureq::get("https://raw.githubusercontent.com/poiscail/OutilsRust/HEAD/themes/galerie.json")
        .timeout(Duration::from_secs(8))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Vec<ThemePartage>>().ok())
        .unwrap_or_default();
    for t in en_ligne {
        if !v.iter().any(|x| x.nom == t.nom) {
            v.push(t);
        }
    }
    v
}

fn donnees() -> PathBuf {
    crate::outil::base()
}

fn fichier() -> PathBuf {
    donnees().join("theme.json")
}

fn dossier_polices() -> PathBuf {
    donnees().join("polices")
}

fn polices_dispo() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dossier_polices()).into_iter().flatten().flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| matches!(n.rsplit('.').next().map(str::to_ascii_lowercase).as_deref(), Some("ttf" | "otf" | "ttc")))
        .collect();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

/// Vraie police lisible ? (egui plante sur un fichier invalide, donc on vérifie avant)
fn police_valide(b: &[u8]) -> bool {
    ab_glyph::FontRef::try_from_slice(b).is_ok()
}

/// Fonds livrés dans l'exe (la mise à jour ne télécharge que l'exe).
const FONDS_BASE: &[(&str, &[u8])] = &[
    ("fond_pagode_cascade_violet.jpg", include_bytes!("../fonds/fond_pagode_cascade_violet.jpg")),
    ("fond_pagode_lune_lac.jpg", include_bytes!("../fonds/fond_pagode_lune_lac.jpg")),
    ("fond_ruelle_pluie_tour.jpg", include_bytes!("../fonds/fond_ruelle_pluie_tour.jpg")),
    ("fond_sakura_torii_lune.jpg", include_bytes!("../fonds/fond_sakura_torii_lune.jpg")),
    ("fond_sanctuaire_fuji_pont.jpg", include_bytes!("../fonds/fond_sanctuaire_fuji_pont.jpg")),
    ("fond_tokyo_pluie_neon.jpg", include_bytes!("../fonds/fond_tokyo_pluie_neon.jpg")),
];

/// Dépose les fonds de base pas encore installés. Un fond déjà présent sous un autre format
/// (ancien .png) n'est pas dupliqué ; un fond supprimé par l'utilisateur n'est pas remis.
/// Renvoie true si la liste des fonds installés a changé.
fn installer_fonds_base(dossier: &Path, installes: &mut Vec<String>) -> bool {
    let mut change = false;
    for (nom, octets) in FONDS_BASE {
        if installes.iter().any(|n| n == nom) {
            continue;
        }
        let racine = Path::new(nom).file_stem().unwrap_or_default();
        let deja = fonds_dispo(dossier).iter().any(|p| p.file_stem() == Some(racine));
        if deja || (std::fs::create_dir_all(dossier).is_ok() && std::fs::write(dossier.join(nom), octets).is_ok()) {
            installes.push(nom.to_string());
            change = true;
        }
    }
    change
}

fn fonds_dispo(dossier: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dossier).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")))
        .collect();
    v.sort();
    v
}

/// Image suivante dans la liste (la première si l'actuelle n'y est pas).
fn suivante(liste: &[PathBuf], actuelle: Option<&PathBuf>) -> Option<PathBuf> {
    let i = actuelle.and_then(|a| liste.iter().position(|p| p == a)).map_or(0, |i| i + 1);
    liste.get(i % liste.len().max(1)).cloned()
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn melange(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Zone de l'image (en coordonnées 0..1) à afficher pour couvrir l'écran sans déformer.
fn uv_couvrir(ecran: [f32; 2], image: [f32; 2]) -> Rect {
    let echelle = (ecran[0] / image[0]).max(ecran[1] / image[1]);
    let (lu, lv) = (ecran[0] / (image[0] * echelle), ecran[1] / (image[1] * echelle));
    Rect::from_min_max(Pos2::new(0.5 - lu / 2.0, 0.5 - lv / 2.0), Pos2::new(0.5 + lu / 2.0, 0.5 + lv / 2.0))
}

/// Nombre pseudo-aléatoire stable 0..1 (pétale i, propriété k).
fn hasard(i: u32, k: u32) -> f32 {
    let mut x = i.wrapping_mul(2_654_435_761) ^ k.wrapping_mul(40_503).wrapping_add(0x9e37);
    x ^= x >> 13;
    x = x.wrapping_mul(0x5bd1_e995);
    x ^= x >> 15;
    (x & 0xFFFF) as f32 / 65_535.0
}

/// Partagé avec le fil de rotation, qui tourne même fenêtre cachée (elle ne repeint plus alors).
#[derive(Default)]
struct Rotation {
    actif: bool,
    minutes: u32,
    bureau: bool,
    courante: Option<PathBuf>,
    nouvelle: bool, // le fil a changé d'image, le thème doit la reprendre
}

pub struct Theme {
    pub r: Reglages,
    a_appliquer: bool,
    a_police: bool,
    texture: Option<egui::TextureHandle>,
    texture_de: Option<PathBuf>,
    dossier_fonds: PathBuf,
    fond_applique: Option<PathBuf>,
    rotation: Arc<Mutex<Rotation>>,
    erreur: Option<String>,
    nom_theme: String,
    code_colle: String,
    galerie: Arc<Mutex<Option<Vec<ThemePartage>>>>,
}

impl Theme {
    pub fn new() -> Self {
        let mut r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let exe = std::env::current_exe().unwrap_or_default();
        let dossier_fonds = exe.parent().unwrap_or(Path::new(".")).join("fonds");
        let fonds_nouveaux = installer_fonds_base(&dossier_fonds, &mut r.fonds_installes);
        let rotation = Arc::new(Mutex::new(Rotation::default()));
        let (rot, dossier) = (rotation.clone(), dossier_fonds.clone());
        std::thread::spawn(move || {
            let mut dernier = Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(15));
                let mut r = rot.lock().unwrap();
                if !r.actif {
                    dernier = Instant::now();
                } else if dernier.elapsed() >= Duration::from_secs(60 * r.minutes.max(1) as u64) {
                    dernier = Instant::now();
                    if let Some(p) = suivante(&fonds_dispo(&dossier), r.courante.as_ref()) {
                        if r.bureau {
                            systeme::fond_bureau(&p);
                        }
                        r.courante = Some(p);
                        r.nouvelle = true;
                    }
                }
            }
        });
        let t = Theme { r, a_appliquer: true, a_police: true, texture: None, texture_de: None, dossier_fonds, fond_applique: None, rotation, erreur: None, nom_theme: String::new(), code_colle: String::new(), galerie: Arc::default() };
        t.appliquer_options();
        if fonds_nouveaux {
            t.enregistrer();
        }
        if t.r.demarrage {
            systeme::demarrage(true); // remet le bon chemin si l'exe a bougé
        }
        t
    }

    /// Pour l'économiseur d'écran : mêmes réglages et effets, sans fil ni effet sur Windows.
    pub fn pour_economiseur(dossier_fonds: PathBuf) -> Self {
        let mut r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        r.fond_windows = false;
        r.rotation = false;
        Theme { r, a_appliquer: true, a_police: true, texture: None, texture_de: None, dossier_fonds, fond_applique: None, rotation: Arc::default(), erreur: None, nom_theme: String::new(), code_colle: String::new(), galerie: Arc::default() }
    }

    pub fn dossier_fonds(&self) -> &Path {
        &self.dossier_fonds
    }

    pub fn fonds(&self) -> Vec<PathBuf> {
        fonds_dispo(&self.dossier_fonds)
    }

    pub fn image(&self) -> Option<&PathBuf> {
        self.r.image.as_ref()
    }

    pub fn choisir_image(&mut self, p: Option<PathBuf>) {
        self.r.image = p;
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(s) = serde_json::to_string_pretty(&self.r) {
            let _ = std::fs::write(f, s);
        }
    }

    fn appliquer_options(&self) {
        systeme::NOTIFS.store(self.r.notifs, Relaxed);
        systeme::regler(self.r.zone, self.r.raccourci);
    }

    fn appliquer(&self, ctx: &egui::Context) {
        let r = &self.r;
        let mut v = if r.sombre { egui::Visuals::dark() } else { egui::Visuals::light() };
        let alpha = (r.opacite.clamp(0.0, 1.0) * 255.0) as u8;
        let p = r.panneau;
        v.panel_fill = Color32::from_rgba_unmultiplied(p[0], p[1], p[2], alpha);
        v.window_fill = rgb(p);
        v.extreme_bg_color = rgb(melange(p, r.fond, 0.6));
        v.faint_bg_color = rgb(melange(p, r.texte, 0.04));
        v.override_text_color = Some(rgb(r.texte));
        v.hyperlink_color = rgb(r.accent);
        v.selection.bg_fill = rgb(melange(p, r.accent, 0.75));
        v.selection.stroke.color = rgb(r.texte);
        let w = &mut v.widgets;
        w.inactive.weak_bg_fill = rgb(melange(p, r.accent, 0.18));
        w.inactive.bg_fill = w.inactive.weak_bg_fill;
        w.hovered.weak_bg_fill = rgb(melange(p, r.accent, 0.45));
        w.hovered.bg_fill = w.hovered.weak_bg_fill;
        w.hovered.bg_stroke.color = rgb(r.accent);
        w.active.weak_bg_fill = rgb(r.accent);
        w.active.bg_fill = rgb(r.accent);
        w.open.weak_bg_fill = rgb(melange(p, r.accent, 0.3));
        ctx.set_visuals(v);
        ctx.set_zoom_factor(r.zoom.clamp(0.6, 2.0));
    }

    fn appliquer_police(&mut self, ctx: &egui::Context) {
        let mut f = egui::FontDefinitions::default();
        if let Some(n) = &self.r.police {
            match std::fs::read(dossier_polices().join(n)) {
                Ok(b) if police_valide(&b) => {
                    f.font_data.insert("perso".into(), Arc::new(egui::FontData::from_owned(b)));
                    // en tête : les emojis et symboles retombent sur les polices d'egui
                    f.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "perso".into());
                }
                _ => self.erreur = Some(format!("Police illisible : {n}")),
            }
        }
        ctx.set_fonts(f);
    }

    fn charger_image(&mut self, ctx: &egui::Context) {
        if self.texture_de == self.r.image {
            return;
        }
        self.texture_de = self.r.image.clone();
        self.texture = None;
        let Some(p) = &self.r.image else { return };
        match image::open(p) {
            Ok(img) => {
                let img = img.to_rgba8();
                let taille = [img.width() as usize, img.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(taille, img.as_raw());
                self.texture = Some(ctx.load_texture("fond", ci, egui::TextureOptions::LINEAR));
                self.erreur = None;
            }
            Err(e) => self.erreur = Some(format!("Image illisible : {e}")),
        }
    }

    fn synchroniser_rotation(&mut self) {
        let mut rot = self.rotation.lock().unwrap();
        if rot.nouvelle {
            rot.nouvelle = false;
            self.r.image = rot.courante.clone();
            if rot.bureau {
                self.fond_applique = self.r.image.clone(); // déjà mis par le fil
            }
            drop(rot);
            self.enregistrer();
            return;
        }
        rot.actif = self.r.rotation;
        rot.minutes = self.r.rotation_min;
        rot.bureau = self.r.fond_windows;
        rot.courante = self.r.image.clone();
    }

    fn peindre_petales(&self, ctx: &egui::Context, peintre: &egui::Painter, ecran: Rect) {
        let t = ctx.input(|i| i.time) as f32;
        for i in 0..40u32 {
            let vitesse = 25.0 + 45.0 * hasard(i, 1);
            let taille = 3.0 + 5.0 * hasard(i, 2);
            let haut = ecran.height() + 40.0;
            let y = (hasard(i, 3) * haut + t * vitesse) % haut - 20.0;
            let x = hasard(i, 4) * ecran.width() + (t * (0.4 + hasard(i, 5)) + TAU * hasard(i, 6)).sin() * 35.0 + y * 0.15;
            let x = x.rem_euclid(ecran.width() + 40.0) - 20.0;
            let battement = (t * (1.5 + 2.0 * hasard(i, 7)) + TAU * hasard(i, 8)).cos().abs().max(0.25); // le pétale tourne sur lui-même
            let rose = Color32::from_rgba_unmultiplied(255, (170.0 + 50.0 * hasard(i, 9)) as u8, (195.0 + 30.0 * hasard(i, 10)) as u8, 190);
            peintre.add(egui::Shape::ellipse_filled(ecran.min + egui::vec2(x, y), egui::vec2(taille, taille * 0.6 * battement), rose));
        }
    }

    fn nombre(&self, base: u32) -> u32 {
        (base as f32 * self.r.intensite.clamp(0.2, 4.0)).round() as u32
    }

    fn peindre_pluie(&self, ctx: &egui::Context, peintre: &egui::Painter, ecran: Rect) {
        let t = ctx.input(|i| i.time) as f32;
        let haut = ecran.height() + 80.0;
        for i in 0..self.nombre(90) {
            let vitesse = 700.0 + 500.0 * hasard(i, 21);
            let long = 10.0 + 18.0 * hasard(i, 22);
            let y = (hasard(i, 23) * haut + t * vitesse) % haut - 40.0;
            let x = (hasard(i, 24) * (ecran.width() + 60.0) + y * 0.12).rem_euclid(ecran.width() + 60.0) - 30.0;
            let a = (60.0 + 80.0 * hasard(i, 25)) as u8;
            let p = ecran.min + egui::vec2(x, y);
            peintre.line_segment([p, p + egui::vec2(long * 0.12, long)], egui::Stroke::new(1.0f32, Color32::from_rgba_unmultiplied(190, 200, 255, a)));
        }
    }

    fn peindre_neige(&self, ctx: &egui::Context, peintre: &egui::Painter, ecran: Rect) {
        let t = ctx.input(|i| i.time) as f32;
        let haut = ecran.height() + 20.0;
        for i in 0..self.nombre(70) {
            let vitesse = 15.0 + 35.0 * hasard(i, 31);
            let r = 1.0 + 2.5 * hasard(i, 32);
            let y = (hasard(i, 33) * haut + t * vitesse) % haut - 10.0;
            let x = (hasard(i, 34) * ecran.width() + (t * 0.6 + TAU * hasard(i, 35)).sin() * 20.0).rem_euclid(ecran.width());
            peintre.circle_filled(ecran.min + egui::vec2(x, y), r, Color32::from_rgba_unmultiplied(255, 255, 255, (120.0 + 100.0 * hasard(i, 36)) as u8));
        }
    }

    fn peindre_lucioles(&self, ctx: &egui::Context, peintre: &egui::Painter, ecran: Rect) {
        let t = ctx.input(|i| i.time) as f32;
        for i in 0..self.nombre(28) {
            // dérive lente + scintillement
            let x = ecran.width() * (hasard(i, 41) + 0.08 * (t * (0.15 + 0.2 * hasard(i, 42)) + TAU * hasard(i, 43)).sin()).rem_euclid(1.0);
            let y = ecran.height() * (0.25 + 0.75 * hasard(i, 44) + 0.06 * (t * (0.2 + 0.2 * hasard(i, 45)) + TAU * hasard(i, 46)).cos()).rem_euclid(1.0);
            let eclat = ((t * (0.8 + hasard(i, 47)) + TAU * hasard(i, 48)).sin() * 0.5 + 0.5).powf(2.0);
            let c = ecran.min + egui::vec2(x, y);
            let (r, g, b) = if hasard(i, 49) > 0.5 { (255, 230, 140) } else { (255, 150, 210) };
            peintre.circle_filled(c, 7.0, Color32::from_rgba_unmultiplied(r, g, b, (40.0 * eclat) as u8));
            peintre.circle_filled(c, 2.2, Color32::from_rgba_unmultiplied(r, g, b, (230.0 * eclat) as u8));
        }
    }

    /// À appeler au début de chaque image, avant les panneaux : peint le fond dessous.
    /// Réapplique couleurs et taille (après le mode grand écran).
    pub fn zoom(&self) -> f32 {
        self.r.zoom.clamp(0.6, 2.0)
    }

    pub fn reappliquer(&mut self) {
        self.a_appliquer = true;
    }

    pub fn peindre(&mut self, ctx: &egui::Context) {
        let demande = DEMANDE.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(n) = demande {
            if let Some((_, p)) = prereglages().into_iter().find(|(x, _)| *x == n) {
                let r = &mut self.r;
                (r.sombre, r.accent, r.fond, r.panneau, r.texte, r.opacite, r.assombrir) = (p.sombre, p.accent, p.fond, p.panneau, p.texte, p.opacite, p.assombrir);
            } else if let Some(t) = crate::outil::charger::<Vec<ThemePartage>>("mes_themes").into_iter().find(|t| t.nom == n) {
                t.appliquer(&mut self.r);
                self.a_police = true;
            }
            self.a_appliquer = true;
            self.enregistrer();
        }
        if self.a_police {
            self.appliquer_police(ctx);
            self.a_police = false;
        }
        if self.a_appliquer {
            self.appliquer(ctx);
            self.a_appliquer = false;
        }
        self.synchroniser_rotation();
        if !self.r.fond_windows {
            self.fond_applique = None;
        } else if self.fond_applique != self.r.image {
            if let Some(p) = &self.r.image {
                systeme::fond_bureau(p);
            }
            self.fond_applique = self.r.image.clone();
        }
        self.charger_image(ctx);
        let ecran = ctx.screen_rect();
        let peintre = ctx.layer_painter(egui::LayerId::background());
        peintre.rect_filled(ecran, 0.0, rgb(self.r.fond));
        if let Some(t) = &self.texture {
            let s = t.size_vec2();
            peintre.image(t.id(), ecran, uv_couvrir([ecran.width(), ecran.height()], [s.x, s.y]), Color32::WHITE);
            let voile = (self.r.assombrir.clamp(0.0, 1.0) * 255.0) as u8;
            peintre.rect_filled(ecran, 0.0, Color32::from_black_alpha(voile));
        }
        if self.r.pluie {
            self.peindre_pluie(ctx, &peintre, ecran);
        }
        if self.r.neige {
            self.peindre_neige(ctx, &peintre, ecran);
        }
        if self.r.lucioles {
            self.peindre_lucioles(ctx, &peintre, ecran);
        }
        if self.r.petales {
            self.peindre_petales(ctx, &peintre, ecran);
        }
        if self.r.petales || self.r.pluie || self.r.neige || self.r.lucioles {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }

    fn ajouter_police(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter("Polices", &["ttf", "otf", "ttc"]).pick_file() else { return };
        let Some(nom) = p.file_name().and_then(|n| n.to_str()).map(String::from) else { return };
        match std::fs::read(&p) {
            Ok(b) if police_valide(&b) => {
                let d = dossier_polices();
                match std::fs::create_dir_all(&d).and_then(|_| std::fs::write(d.join(&nom), &b)) {
                    Ok(()) => {
                        self.r.police = Some(nom);
                        self.erreur = None;
                    }
                    Err(e) => self.erreur = Some(format!("Copie impossible : {e}")),
                }
            }
            Ok(_) => self.erreur = Some(format!("{nom} n'est pas une police valide.")),
            Err(e) => self.erreur = Some(format!("Lecture impossible : {e}")),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.ui_contenu(ui));
    }

    fn ui_contenu(&mut self, ui: &mut egui::Ui) {
        let avant = self.r.clone();
        ui.heading("Personnaliser");
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Thèmes :");
            for (nom, p) in prereglages() {
                let c = Color32::from_rgb(p.accent[0], p.accent[1], p.accent[2]);
                if ui.button(RichText::new(format!("● {nom}")).color(c)).clicked() {
                    let r = &mut self.r;
                    (r.sombre, r.accent, r.fond, r.panneau, r.texte, r.opacite, r.assombrir) = (p.sombre, p.accent, p.fond, p.panneau, p.texte, p.opacite, p.assombrir);
                }
            }
        });
        let mut mes: Vec<ThemePartage> = crate::outil::charger("mes_themes");
        let avant_mes = mes.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Mes thèmes :");
            let mut retirer = None;
            for (k, t) in mes.iter().enumerate() {
                let c = Color32::from_rgb(t.accent[0], t.accent[1], t.accent[2]);
                let b = ui.button(RichText::new(format!("● {}", t.nom)).color(c)).on_hover_text("Clic droit : supprimer");
                if b.clicked() {
                    t.appliquer(&mut self.r);
                }
                b.context_menu(|ui| {
                    if ui.button("🗑 Supprimer").clicked() {
                        retirer = Some(k);
                        ui.close_menu();
                    }
                });
            }
            if let Some(k) = retirer {
                mes.remove(k);
            }
            ui.add(egui::TextEdit::singleline(&mut self.nom_theme).hint_text("nom").desired_width(110.0));
            if ui.button("💾 Garder ce thème").clicked() && !self.nom_theme.trim().is_empty() {
                let n = self.nom_theme.trim().to_string();
                mes.retain(|t| t.nom != n);
                mes.push(ThemePartage::depuis(&n, &self.r));
                self.nom_theme.clear();
            }
            if ui.button("📤 Exporter…").on_hover_text("Fichier .theme à envoyer à un ami").clicked() {
                let n = if self.nom_theme.trim().is_empty() { "Mon thème".to_string() } else { self.nom_theme.trim().to_string() };
                if let Some(p) = rfd::FileDialog::new().add_filter("Thème Outils", &["theme"]).set_file_name(format!("{n}.theme")).save_file() {
                    let _ = std::fs::write(p, serde_json::to_string_pretty(&ThemePartage::depuis(&n, &self.r)).unwrap_or_default());
                }
            }
            if ui.button("📥 Importer…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Thème Outils", &["theme", "json"]).pick_file() {
                    if let Some(t) = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<ThemePartage>(&s).ok()) {
                        t.appliquer(&mut self.r);
                        mes.retain(|x| x.nom != t.nom);
                        mes.push(t);
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("🔗 Copier le code du thème").on_hover_text("Un texte à coller dans Discord ou un message : ton ami le colle ici pour avoir le même thème").clicked() {
                let n = if self.nom_theme.trim().is_empty() { "Thème d'un ami".to_string() } else { self.nom_theme.trim().to_string() };
                ui.ctx().copy_text(code_theme(&ThemePartage::depuis(&n, &self.r)));
            }
            ui.add(egui::TextEdit::singleline(&mut self.code_colle).hint_text("colle un code OUTILS-THEME:…").desired_width(220.0));
            if !self.code_colle.trim().is_empty() {
                match lire_code(&self.code_colle) {
                    Some(t) => {
                        if ui.button(format!("✔ Appliquer « {} »", t.nom)).clicked() {
                            t.appliquer(&mut self.r);
                            mes.retain(|x| x.nom != t.nom);
                            mes.push(t);
                            self.code_colle.clear();
                        }
                    }
                    None => {
                        ui.colored_label(Color32::LIGHT_RED, "code invalide");
                    }
                }
            }
        });
        egui::CollapsingHeader::new("🌍 Galerie de thèmes").show(ui, |ui| {
            let g = self.galerie.lock().unwrap().clone();
            match g {
                None => {
                    let gal = self.galerie.clone();
                    *gal.lock().unwrap() = Some(Vec::new());
                    std::thread::spawn(move || *gal.lock().unwrap() = Some(charger_galerie()));
                }
                Some(v) if v.is_empty() => {
                    ui.spinner();
                }
                Some(v) => {
                    ui.horizontal_wrapped(|ui| {
                        for t in v {
                            let c = Color32::from_rgb(t.accent[0], t.accent[1], t.accent[2]);
                            let f = Color32::from_rgb(t.fond[0], t.fond[1], t.fond[2]);
                            let b = egui::Button::new(RichText::new(format!("● {}", t.nom)).color(c)).fill(f);
                            if ui.add(b).on_hover_text("Clic : essayer · ＋ : garder dans mes thèmes").clicked() {
                                t.appliquer(&mut self.r);
                            }
                            ui.menu_button("＋", |ui| {
                                if ui.button("Garder dans mes thèmes").clicked() {
                                    mes.retain(|x| x.nom != t.nom);
                                    mes.push(t.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                }
            }
        });
        if mes != avant_mes {
            crate::outil::enregistrer("mes_themes", &mes);
        }
        ui.separator();
        egui::Grid::new("couleurs").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("Mode");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.r.sombre, true, "Sombre");
                ui.radio_value(&mut self.r.sombre, false, "Clair");
            });
            ui.end_row();
            for (nom, c) in [("Accent", &mut self.r.accent), ("Fond", &mut self.r.fond), ("Panneaux", &mut self.r.panneau), ("Texte", &mut self.r.texte)] {
                ui.label(nom);
                ui.color_edit_button_srgb(c);
                ui.end_row();
            }
            ui.label("Opacité des panneaux");
            ui.add(egui::Slider::new(&mut self.r.opacite, 0.0..=1.0));
            ui.end_row();
            ui.label("Assombrir l'image");
            ui.add(egui::Slider::new(&mut self.r.assombrir, 0.0..=1.0));
            ui.end_row();
            ui.label("Taille de l'interface");
            ui.add(egui::Slider::new(&mut self.r.zoom, 0.6..=2.0).step_by(0.05));
            ui.end_row();
        });

        ui.separator();
        ui.strong("Police");
        ui.horizontal(|ui| {
            let actuelle = self.r.police.clone().unwrap_or_else(|| "Par défaut".into());
            egui::ComboBox::from_id_salt("police").selected_text(actuelle).width(240.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.r.police, None, "Par défaut");
                for n in polices_dispo() {
                    ui.selectable_value(&mut self.r.police, Some(n.clone()), n);
                }
            });
            if ui.button("Ajouter une police…").clicked() {
                self.ajouter_police();
            }
            if ui.button("Dossier des polices").clicked() {
                let d = dossier_polices();
                let _ = std::fs::create_dir_all(&d);
                let _ = std::process::Command::new("explorer").arg(&d).spawn();
            }
        });
        ui.small("Télécharge une police (.ttf / .otf), ajoute-la ici : elle est gardée et reste au choix.");

        ui.separator();
        ui.strong("Image de fond");
        ui.horizontal(|ui| {
            ui.label(self.r.image.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(|| "aucune".into()));
            if ui.button("Choisir…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                    self.r.image = Some(p);
                }
            }
            if self.r.image.is_some() && ui.button("Retirer").clicked() {
                self.r.image = None;
            }
            if ui.button("Dossier des fonds").clicked() {
                let _ = std::fs::create_dir_all(&self.dossier_fonds);
                let _ = std::process::Command::new("explorer").arg(&self.dossier_fonds).spawn();
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.r.rotation, "Changer d'image tout seul toutes les");
            ui.add_enabled(self.r.rotation, egui::DragValue::new(&mut self.r.rotation_min).range(1..=1440).suffix(" min"));
        });
        ui.checkbox(&mut self.r.fond_windows, "Mettre aussi l'image en fond d'écran Windows");
        if self.r.opacite > 0.95 && self.r.image.is_some() {
            ui.colored_label(Color32::GRAY, "Baissez l'opacité des panneaux pour voir l'image.");
        }
        if let Some(e) = &self.erreur {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let fonds = fonds_dispo(&self.dossier_fonds);
        if !fonds.is_empty() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for p in fonds {
                    let choisi = self.r.image.as_ref() == Some(&p);
                    let img = egui::Image::new(format!("file://{}", p.display())).fit_to_exact_size(egui::vec2(192.0, 108.0));
                    if ui.add(egui::ImageButton::new(img).selected(choisi)).on_hover_text(p.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                        self.r.image = Some(p);
                    }
                }
            });
        }

        ui.separator();
        ui.strong("Effets");
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.r.petales, "🌸 Pétales de sakura");
            ui.checkbox(&mut self.r.pluie, "🌧 Pluie");
            ui.checkbox(&mut self.r.neige, "❄ Neige");
            ui.checkbox(&mut self.r.lucioles, "✨ Lucioles");
        });
        ui.add(egui::Slider::new(&mut self.r.intensite, 0.3..=3.0).text("intensité"));

        ui.separator();
        ui.strong("Options");
        ui.checkbox(&mut self.r.notifs, "Notifications Windows (tri, joueurs Minecraft, nettoyage, minuteur)");
        ui.checkbox(&mut self.r.demarrage, "Démarrer avec Windows");
        ui.checkbox(&mut self.r.zone, "Icône dans la zone de notification (fermer la fenêtre la range)");
        ui.checkbox(&mut self.r.raccourci, "Raccourci global Ctrl+Alt+O pour afficher / ranger Outils");
        if systeme::raccourci_pris() {
            ui.colored_label(Color32::LIGHT_RED, "Ctrl+Alt+O est déjà pris par une autre application.");
        }
        ui.checkbox(&mut self.r.maj, "Chercher les mises à jour au démarrage");

        if self.r != avant {
            if self.r.police != avant.police {
                self.a_police = true;
            }
            if self.r.demarrage != avant.demarrage {
                systeme::demarrage(self.r.demarrage);
            }
            self.appliquer_options();
            self.a_appliquer = true;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_et_galerie() {
        let t = ThemePartage::depuis("Mon thème", &Reglages::default());
        assert!(lire_code(&code_theme(&t)) == Some(t));
        assert!(lire_code("OUTILS-THEME:zzz").is_none() && lire_code("bonjour").is_none());
        let g: Vec<ThemePartage> = serde_json::from_str(include_str!("../themes/galerie.json")).unwrap();
        assert!(g.len() >= 10 && g.iter().all(|t| !t.nom.is_empty()));
    }

    #[test]
    fn couvrir() {
        // image plus large que l'écran : on coupe les côtés
        let r = uv_couvrir([100.0, 100.0], [200.0, 100.0]);
        assert!((r.min.x - 0.25).abs() < 1e-6 && (r.max.x - 0.75).abs() < 1e-6);
        assert!((r.min.y - 0.0).abs() < 1e-6 && (r.max.y - 1.0).abs() < 1e-6);
        // même ratio : image entière
        let r = uv_couvrir([1920.0, 1080.0], [3840.0, 2160.0]);
        assert!((r.width() - 1.0).abs() < 1e-6 && (r.height() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn reglages_json() {
        let r: Reglages = serde_json::from_str(r#"{"accent":[1,2,3]}"#).unwrap();
        assert_eq!(r.accent, [1, 2, 3]);
        assert!(r.sombre && r.image.is_none() && r.maj && !r.notifs && r.rotation_min == 10);
        assert_eq!(melange([0, 0, 0], [200, 100, 50], 0.5), [100, 50, 25]);
    }

    #[test]
    fn rotation_et_police() {
        let l = [PathBuf::from("a"), PathBuf::from("b")];
        assert_eq!(suivante(&l, None), Some(PathBuf::from("a")));
        assert_eq!(suivante(&l, Some(&l[0])), Some(PathBuf::from("b")));
        assert_eq!(suivante(&l, Some(&l[1])), Some(PathBuf::from("a")));
        assert_eq!(suivante(&[], None), None);
        assert!(!police_valide(b"pas une police"));
        let arial = std::fs::read(r"C:\Windows\Fonts\arial.ttf").unwrap();
        assert!(police_valide(&arial));
    }

    #[test]
    fn fonds_base() {
        let d = std::env::temp_dir().join(format!("outils_fonds_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("fond_pagode_lune_lac.png"), b"ancien").unwrap(); // ancien format : pas de doublon
        let mut faits = Vec::new();
        assert!(installer_fonds_base(&d, &mut faits));
        assert_eq!(faits.len(), FONDS_BASE.len());
        assert!(!d.join("fond_pagode_lune_lac.jpg").exists());
        assert_eq!(fonds_dispo(&d).len(), FONDS_BASE.len());
        for (_, o) in FONDS_BASE {
            assert!(image::load_from_memory(o).is_ok());
        }
        std::fs::remove_file(d.join("fond_tokyo_pluie_neon.jpg")).unwrap(); // supprimé : pas remis
        assert!(!installer_fonds_base(&d, &mut faits));
        assert!(!d.join("fond_tokyo_pluie_neon.jpg").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
