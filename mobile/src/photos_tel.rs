//! Photos et vidéos : nettoyeur (floues, rafales, vieilles captures) (9), coffre-fort chiffré (11),
//! compression de vidéos pour WhatsApp / Discord (10).
use crate::commun::*;
use crate::fichiers_tel::{lister_tout, racine, Fichier};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText, Vec2};
use image::{imageops, GrayImage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ===================================================================================================
// 9 · Nettoyeur de photos
// ===================================================================================================

/// Empreinte visuelle (64 bits) : deux photos presque pareilles ont des empreintes proches.
pub fn dhash(g: &GrayImage) -> u64 {
    let p = imageops::resize(g, 9, 8, imageops::FilterType::Triangle);
    let mut h = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            h = (h << 1) | (p.get_pixel(x, y)[0] > p.get_pixel(x + 1, y)[0]) as u64;
        }
    }
    h
}

pub fn ecart(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Netteté : variance du laplacien (une photo floue a peu de contours nets).
pub fn nettete(g: &GrayImage) -> f64 {
    let (l, h) = g.dimensions();
    if l < 3 || h < 3 {
        return 0.0;
    }
    let mut v = Vec::with_capacity(((l - 2) * (h - 2)) as usize);
    for y in 1..h - 1 {
        for x in 1..l - 1 {
            let c = g.get_pixel(x, y)[0] as f64;
            let s = g.get_pixel(x - 1, y)[0] as f64 + g.get_pixel(x + 1, y)[0] as f64 + g.get_pixel(x, y - 1)[0] as f64 + g.get_pixel(x, y + 1)[0] as f64;
            v.push(s - 4.0 * c);
        }
    }
    let m = v.iter().sum::<f64>() / v.len() as f64;
    v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / v.len() as f64
}

#[derive(Clone)]
pub struct Photo {
    pub f: Fichier,
    pub hash: u64,
    pub nettete: f64,
    pub vignette: Option<image::RgbaImage>,
}

/// Rafales : photos presque identiques prises à moins de 30 s d'écart. La plus nette en premier.
pub fn rafales(photos: &[Photo]) -> Vec<Vec<usize>> {
    let mut ordre: Vec<usize> = (0..photos.len()).collect();
    ordre.sort_by_key(|k| photos[*k].f.date);
    let mut groupes: Vec<Vec<usize>> = vec![];
    let mut courant: Vec<usize> = vec![];
    for k in ordre {
        let proche = courant.last().is_some_and(|&d| photos[k].f.date.saturating_sub(photos[d].f.date) <= 30 && ecart(photos[k].hash, photos[d].hash) <= 10);
        if !proche {
            if courant.len() > 1 {
                groupes.push(std::mem::take(&mut courant));
            }
            courant.clear();
        }
        courant.push(k);
    }
    if courant.len() > 1 {
        groupes.push(courant);
    }
    for g in groupes.iter_mut() {
        g.sort_by(|a, b| photos[*b].nettete.partial_cmp(&photos[*a].nettete).unwrap());
    }
    groupes
}

fn analyser(p: &Path) -> Option<(u64, f64, image::RgbaImage)> {
    let i = image::open(p).ok()?;
    let petite = i.thumbnail(320, 320);
    let g = petite.to_luma8();
    Some((dhash(&g), nettete(&g), i.thumbnail(160, 160).to_rgba8()))
}

#[derive(Default)]
struct Resultat {
    photos: Vec<Photo>,
    rafales: Vec<Vec<usize>>,
    floues: Vec<usize>,
    captures: Vec<usize>,
}

pub struct Nettoyeur {
    e: Etat,
    mois: u64,
    avance: Arc<(AtomicUsize, AtomicUsize)>,
    res: Arc<Mutex<Option<Resultat>>>,
    textures: std::collections::HashMap<usize, egui::TextureHandle>,
    choisis: Vec<usize>,
    onglet: usize,
}

impl Nettoyeur {
    pub fn new() -> Self {
        Nettoyeur { e: Arc::default(), mois: 3, avance: Arc::default(), res: Arc::default(), textures: Default::default(), choisis: vec![], onglet: 0 }
    }

    fn grille(&mut self, ui: &mut egui::Ui, l: &[usize], meilleur: Option<usize>) {
        let res = self.res.lock().unwrap();
        let Some(r) = res.as_ref() else { return };
        let t = ((ui.available_width() - 16.0) / 3.0).min(140.0);
        ui.horizontal_wrapped(|ui| {
            for &k in l {
                let tex = self.textures.entry(k).or_insert_with(|| {
                    let v = r.photos[k].vignette.clone().unwrap_or_else(|| image::RgbaImage::new(1, 1));
                    ui.ctx().load_texture(format!("ph{k}"), egui::ColorImage::from_rgba_unmultiplied([v.width() as usize, v.height() as usize], v.as_raw()), Default::default())
                });
                let choisi = self.choisis.contains(&k);
                let (rect, rep) = ui.allocate_exact_size(Vec2::splat(t), egui::Sense::click());
                ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), if choisi { Color32::from_white_alpha(110) } else { Color32::WHITE });
                if choisi {
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "🗑", egui::FontId::proportional(34.0), ROUGE);
                }
                if meilleur == Some(k) {
                    ui.painter().text(rect.left_top() + Vec2::new(14.0, 14.0), egui::Align2::CENTER_CENTER, "⭐", egui::FontId::proportional(20.0), Color32::YELLOW);
                }
                if rep.clicked() {
                    if choisi {
                        self.choisis.retain(|x| *x != k);
                    } else {
                        self.choisis.push(k);
                    }
                }
            }
        });
    }
}

impl Outil for Nettoyeur {
    fn nom(&self) -> &'static str {
        "🧹 Nettoyeur de photos"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !acces_fichiers_ui(ui) {
            return;
        }
        let occupe = etat_ui(ui, &self.e);
        if occupe {
            let (f, t) = (self.avance.0.load(Relaxed), self.avance.1.load(Relaxed));
            if t > 0 {
                ui.add(egui::ProgressBar::new(f as f32 / t as f32).text(format!("{f} / {t} photos")));
            }
        }
        ui.horizontal(|ui| {
            ui.label("Photos des");
            for (m, n) in [(1, "dernier mois"), (3, "3 mois"), (12, "12 mois"), (0, "toutes")] {
                ui.selectable_value(&mut self.mois, m, n);
            }
        });
        if gros_bouton(ui, "🧹 Analyser mes photos") && !occupe {
            let (r, av, mois) = (self.res.clone(), self.avance.clone(), self.mois);
            self.textures.clear();
            self.choisis.clear();
            en_fond(&self.e, "Analyse des photos…", move || {
                let limite = if mois == 0 { 0 } else { maintenant().saturating_sub(mois * 31 * 86_400) };
                let mut f: Vec<Fichier> = ["DCIM", "Pictures"].iter().flat_map(|d| lister_tout(&racine().join(d), 50_000)).filter(|f| f.date >= limite && matches!(f.chemin.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(), Some("jpg" | "jpeg" | "png" | "webp"))).collect();
                f.sort_by_key(|x| std::cmp::Reverse(x.date));
                f.truncate(3000);
                av.1.store(f.len(), Relaxed);
                av.0.store(0, Relaxed);
                let mut photos = vec![];
                for x in f {
                    av.0.fetch_add(1, Relaxed);
                    let estcapture = x.chemin.to_string_lossy().to_lowercase().contains("screenshot");
                    match analyser(&x.chemin) {
                        Some((h, n, v)) => photos.push(Photo { f: x, hash: h, nettete: if estcapture { 1e9 } else { n }, vignette: Some(v) }),
                        None => continue,
                    }
                }
                let raf = rafales(&photos);
                let dans_rafale: std::collections::HashSet<usize> = raf.iter().flatten().copied().collect();
                let floues: Vec<usize> = (0..photos.len()).filter(|k| photos[*k].nettete < 25.0 && !dans_rafale.contains(k)).collect();
                let vieux = maintenant().saturating_sub(60 * 86_400);
                let captures: Vec<usize> = (0..photos.len()).filter(|k| photos[*k].f.chemin.to_string_lossy().to_lowercase().contains("screenshot") && photos[*k].f.date < vieux).collect();
                let bilan = format!("{} rafales, {} photos floues, {} vieilles captures", raf.len(), floues.len(), captures.len());
                *r.lock().unwrap() = Some(Resultat { photos, rafales: raf, floues, captures });
                Ok(bilan)
            });
        }
        if self.res.lock().unwrap().is_none() {
            ui.weak("Trouve les photos ratées : floues, prises en rafale (on garde la plus nette), et les vieilles captures d'écran.");
            return;
        }
        onglets(ui, &mut self.onglet, &["📸 Rafales", "🌫 Floues", "🖼 Vieilles captures"]);
        let (groupes, floues, captures) = {
            let r = self.res.lock().unwrap();
            let r = r.as_ref().unwrap();
            (r.rafales.clone(), r.floues.clone(), r.captures.clone())
        };
        match self.onglet {
            0 => {
                if !groupes.is_empty() && ui.button("Cocher tout sauf la plus nette de chaque rafale").clicked() {
                    for g in &groupes {
                        for k in &g[1..] {
                            if !self.choisis.contains(k) {
                                self.choisis.push(*k);
                            }
                        }
                    }
                }
                for g in groupes.iter().take(60) {
                    self.grille(ui, g, g.first().copied());
                    ui.separator();
                }
                if groupes.is_empty() {
                    ui.colored_label(VERT, "Aucune rafale.");
                }
            }
            1 => {
                if !floues.is_empty() && ui.button("Tout cocher").clicked() {
                    self.choisis.extend(floues.iter().filter(|k| !self.choisis.contains(k)).copied().collect::<Vec<_>>());
                }
                self.grille(ui, &floues, None);
            }
            _ => {
                if !captures.is_empty() && ui.button("Tout cocher").clicked() {
                    self.choisis.extend(captures.iter().filter(|k| !self.choisis.contains(k)).copied().collect::<Vec<_>>());
                }
                self.grille(ui, &captures, None);
            }
        }
        if !self.choisis.is_empty() && gros_bouton(ui, &format!("🗑 Mettre {} photo(s) à la corbeille d'Outils", self.choisis.len())) {
            let chemins: Vec<PathBuf> = {
                let r = self.res.lock().unwrap();
                self.choisis.iter().map(|k| r.as_ref().unwrap().photos[*k].f.chemin.clone()).collect()
            };
            let mut n = 0;
            for p in &chemins {
                if mettre_a_la_corbeille(p).is_ok() {
                    n += 1;
                }
            }
            if let Some(r) = self.res.lock().unwrap().as_mut() {
                let enlever: std::collections::HashSet<usize> = self.choisis.iter().copied().collect();
                r.rafales = r.rafales.iter().map(|g| g.iter().filter(|k| !enlever.contains(k)).copied().collect::<Vec<_>>()).filter(|g| g.len() > 1).collect();
                r.floues.retain(|k| !enlever.contains(k));
                r.captures.retain(|k| !enlever.contains(k));
            }
            self.choisis.clear();
            *self.e.lock().unwrap() = (false, format!("✔ {n} photo(s) à la corbeille (récupérables 30 jours dans Fichiers du téléphone › Corbeille)"));
        }
    }
}

// ===================================================================================================
// 11 · Coffre-fort de photos (chiffré)
// ===================================================================================================

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};

pub fn dossier_coffre() -> PathBuf {
    racine().join(".Outils-coffre")
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct EnteteCoffre {
    sel: Vec<u8>,
    cle_chiffree: Vec<u8>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Element {
    pub id: String,
    pub nom: String,
    pub origine: String,
    pub taille: u64,
    pub vignette: Vec<u8>, // PNG
}

pub fn deriver(code: &str, sel: &[u8]) -> Result<[u8; 32], String> {
    let mut k = [0u8; 32];
    let p = argon2::Params::new(19 * 1024, 2, 1, Some(32)).map_err(|e| e.to_string())?;
    argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p).hash_password_into(code.as_bytes(), sel, &mut k).map_err(|e| e.to_string())?;
    Ok(k)
}

/// Chiffre : nonce (12 octets) puis données chiffrées et authentifiées.
pub fn sceller(cle: &[u8; 32], donnees: &[u8]) -> Vec<u8> {
    let mut n = [0u8; 12];
    let _ = getrandom::getrandom(&mut n);
    let c = Aes256Gcm::new_from_slice(cle).unwrap();
    let mut v = n.to_vec();
    v.extend(c.encrypt(Nonce::from_slice(&n), donnees).unwrap_or_default());
    v
}

pub fn ouvrir(cle: &[u8; 32], b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 28 {
        return None;
    }
    Aes256Gcm::new_from_slice(cle).ok()?.decrypt(Nonce::from_slice(&b[..12]), &b[12..]).ok()
}

pub struct Coffre {
    cle: Option<[u8; 32]>,
    elements: Vec<Element>,
    code: String,
    code2: String,
    dernier: Instant,
    nav: crate::fichiers_tel::Navigateur,
    vue: Option<(String, egui::TextureHandle)>,
    textures: std::collections::HashMap<String, egui::TextureHandle>,
    message: String,
    ajouter: bool,
}

impl Coffre {
    pub fn new() -> Self {
        Coffre { cle: None, elements: vec![], code: String::new(), code2: String::new(), dernier: Instant::now(), nav: crate::fichiers_tel::Navigateur::new(), vue: None, textures: Default::default(), message: String::new(), ajouter: false }
    }

    fn entete() -> Option<EnteteCoffre> {
        let t = std::fs::read_to_string(dossier_coffre().join("coffre.json")).ok()?;
        serde_json::from_str(&t).ok()
    }

    fn sauver_index(&self) {
        if let Some(k) = &self.cle {
            let j = serde_json::to_vec(&self.elements).unwrap_or_default();
            let _ = std::fs::write(dossier_coffre().join("index.ocf"), sceller(k, &j));
        }
    }

    fn verrouiller(&mut self) {
        self.cle = None;
        self.elements.clear();
        self.textures.clear();
        self.vue = None;
        self.code.clear();
    }

    fn ranger(&mut self, chemins: Vec<PathBuf>) {
        let Some(k) = self.cle else { return };
        let mut n = 0;
        for p in chemins {
            let Ok(b) = std::fs::read(&p) else { continue };
            if b.len() > 400 << 20 {
                self.message = format!("{} est trop gros pour le coffre (400 Mo max).", p.display());
                continue;
            }
            let mut id = [0u8; 12];
            let _ = getrandom::getrandom(&mut id);
            let id: String = id.iter().map(|x| format!("{x:02x}")).collect();
            if std::fs::write(dossier_coffre().join(format!("{id}.ocf")), sceller(&k, &b)).is_err() {
                continue;
            }
            let mut vignette = vec![];
            if let Ok(i) = image::load_from_memory(&b) {
                let _ = i.thumbnail(200, 200).write_to(&mut std::io::Cursor::new(&mut vignette), image::ImageFormat::Png);
            }
            self.elements.push(Element { id, nom: p.file_name().unwrap_or_default().to_string_lossy().into(), origine: p.display().to_string(), taille: b.len() as u64, vignette });
            self.sauver_index();
            // l'original quitte la galerie
            if std::fs::remove_file(&p).is_ok() {
                crate::android::signaler_media(&[p.display().to_string()]);
            }
            n += 1;
        }
        self.message = format!("🔒 {n} fichier(s) mis au coffre");
    }
}

impl Outil for Coffre {
    fn nom(&self) -> &'static str {
        "🔒 Coffre-fort photos"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn tic(&mut self, _: &egui::Context) {
        // verrouillage automatique après 2 minutes sans y toucher
        if self.cle.is_some() && self.dernier.elapsed() > Duration::from_secs(120) {
            self.verrouiller();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.dernier = Instant::now();
        if !acces_fichiers_ui(ui) {
            return;
        }
        if self.cle.is_none() {
            let entete = Coffre::entete();
            ui.label(RichText::new(if entete.is_some() { "🔒 Coffre verrouillé" } else { "Créer ton coffre-fort" }).size(22.0).strong());
            ui.weak("Les photos mises au coffre sont chiffrées (AES-256) et disparaissent de la galerie. Sans le code, personne ne peut les voir — toi non plus : ne l'oublie pas !");
            ui.add(egui::TextEdit::singleline(&mut self.code).password(true).hint_text("code (au moins 4 caractères)").font(egui::FontId::proportional(22.0)).desired_width(f32::INFINITY));
            if entete.is_none() {
                ui.add(egui::TextEdit::singleline(&mut self.code2).password(true).hint_text("le même code, encore").font(egui::FontId::proportional(22.0)).desired_width(f32::INFINITY));
            }
            if gros_bouton(ui, if entete.is_some() { "🔓 Ouvrir" } else { "Créer le coffre" }) && self.code.chars().count() >= 4 {
                match entete {
                    Some(e) => match deriver(&self.code, &e.sel).ok().and_then(|k| ouvrir(&k, &e.cle_chiffree)) {
                        Some(cle) if cle.len() == 32 => {
                            let mut k = [0u8; 32];
                            k.copy_from_slice(&cle);
                            self.elements = std::fs::read(dossier_coffre().join("index.ocf")).ok().and_then(|b| ouvrir(&k, &b)).and_then(|j| serde_json::from_slice(&j).ok()).unwrap_or_default();
                            self.cle = Some(k);
                            self.message.clear();
                        }
                        _ => {
                            self.message = "✖ Code faux".into();
                            crate::android::vibrer(200);
                        }
                    },
                    None if self.code == self.code2 => {
                        let mut sel = [0u8; 16];
                        let mut cle = [0u8; 32];
                        let _ = getrandom::getrandom(&mut sel);
                        let _ = getrandom::getrandom(&mut cle);
                        if let Ok(kd) = deriver(&self.code, &sel) {
                            let _ = std::fs::create_dir_all(dossier_coffre());
                            let _ = std::fs::write(dossier_coffre().join(".nomedia"), b"");
                            let e = EnteteCoffre { sel: sel.to_vec(), cle_chiffree: sceller(&kd, &cle) };
                            let _ = std::fs::write(dossier_coffre().join("coffre.json"), serde_json::to_string(&e).unwrap_or_default());
                            self.cle = Some(cle);
                            self.elements.clear();
                            self.sauver_index();
                        }
                    }
                    None => self.message = "✖ Les deux codes sont différents".into(),
                }
            }
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("🔓 {} élément(s)", self.elements.len())).size(18.0).strong());
            if ui.button("🔒 Verrouiller").clicked() {
                self.verrouiller();
            }
            ui.toggle_value(&mut self.ajouter, "➕ Ajouter");
        });
        if self.cle.is_none() {
            return;
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if self.ajouter {
            self.nav.ui(ui);
            let choisis = self.nav.choisis.clone();
            if !choisis.is_empty() && gros_bouton(ui, &format!("🔒 Mettre {} fichier(s) au coffre", choisis.len())) {
                self.nav.choisis.clear();
                self.ranger(choisis);
                self.ajouter = false;
            }
            return;
        }
        if let Some((id, tex)) = &self.vue {
            let id = id.clone();
            let w = ui.available_width();
            ui.add(egui::Image::new(tex).max_width(w).maintain_aspect_ratio(true));
            ui.horizontal(|ui| {
                if ui.button("⬅ Retour").clicked() {
                    self.vue = None;
                }
                if ui.button("📤 Sortir du coffre").clicked() {
                    if let (Some(k), Some(e)) = (self.cle, self.elements.iter().find(|x| x.id == id).cloned()) {
                        if let Some(b) = std::fs::read(dossier_coffre().join(format!("{id}.ocf"))).ok().and_then(|b| ouvrir(&k, &b)) {
                            let d = racine().join("DCIM").join("Sortis du coffre");
                            let _ = std::fs::create_dir_all(&d);
                            let dest = libre(&d, &e.nom);
                            if std::fs::write(&dest, b).is_ok() {
                                let _ = std::fs::remove_file(dossier_coffre().join(format!("{id}.ocf")));
                                self.elements.retain(|x| x.id != id);
                                self.sauver_index();
                                crate::android::signaler_media(&[dest.display().to_string()]);
                                self.message = format!("✔ Remis dans {}", dest.display());
                                self.vue = None;
                            }
                        }
                    }
                }
            });
            return;
        }
        let t = ((ui.available_width() - 16.0) / 3.0).min(150.0);
        let mut ouvrir_id = None;
        ui.horizontal_wrapped(|ui| {
            for e in &self.elements {
                let tex = self.textures.entry(e.id.clone()).or_insert_with(|| {
                    let i = image::load_from_memory(&e.vignette).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::from_pixel(8, 8, image::Rgba([60, 60, 70, 255])));
                    ui.ctx().load_texture(format!("cof{}", e.id), egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()), Default::default())
                });
                let (r, rep) = ui.allocate_exact_size(Vec2::splat(t), egui::Sense::click());
                ui.painter().image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                if e.vignette.is_empty() {
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, icone(&e.nom, false), egui::FontId::proportional(30.0), Color32::WHITE);
                }
                if rep.clicked() {
                    ouvrir_id = Some(e.id.clone());
                }
            }
        });
        if let (Some(id), Some(k)) = (ouvrir_id, self.cle) {
            let img = std::fs::read(dossier_coffre().join(format!("{id}.ocf"))).ok().and_then(|b| ouvrir(&k, &b)).and_then(|b| image::load_from_memory(&b).ok());
            match img {
                Some(i) => {
                    let i = i.thumbnail(1600, 1600).to_rgba8();
                    self.vue = Some((id, ui.ctx().load_texture("vue_coffre", egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()), Default::default())));
                }
                None => self.message = "Ce fichier (vidéo…) ne s'affiche pas ici : « Sortir du coffre » pour le voir.".into(),
            }
        }
        if self.elements.is_empty() {
            ui.weak("Coffre vide : « ➕ Ajouter » pour y mettre des photos.");
        }
    }
}

// ===================================================================================================
// 10 · Compresseur de vidéos (MediaCodec : décodage → surface → encodage H.264, son recopié)
// ===================================================================================================

#[derive(Clone, Copy, PartialEq)]
pub enum Cible {
    Discord,
    WhatsApp,
    Bonne,
}

/// Débit vidéo (bits/s) pour une durée et une cible.
pub fn debit_cible(c: Cible, duree_s: f64) -> i32 {
    match c {
        // Discord : 10 Mo en tout, son compris (128 kb/s), avec 5 % de marge
        Cible::Discord => ((10.0 * 8.0 * 1_000_000.0 * 0.95 / duree_s.max(1.0)) - 128_000.0).clamp(250_000.0, 8_000_000.0) as i32,
        Cible::WhatsApp => 1_500_000,
        Cible::Bonne => 4_000_000,
    }
}

#[cfg(target_os = "android")]
mod codec {
    use ndk_sys::*;
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::os::unix::io::AsRawFd;
    use std::path::Path;

    unsafe fn chaine(f: *mut AMediaFormat, k: *const c_char) -> Option<String> {
        let mut s: *const c_char = std::ptr::null();
        AMediaFormat_getString(f, k, &mut s).then(|| CStr::from_ptr(s).to_string_lossy().into_owned())
    }

    unsafe fn entier(f: *mut AMediaFormat, k: *const c_char) -> Option<i32> {
        let mut v = 0;
        AMediaFormat_getInt32(f, k, &mut v).then_some(v)
    }

    pub fn duree(p: &Path) -> Option<f64> {
        unsafe {
            let fi = std::fs::File::open(p).ok()?;
            let ex = AMediaExtractor_new();
            let ok = AMediaExtractor_setDataSourceFd(ex, fi.as_raw_fd(), 0, fi.metadata().ok()?.len() as i64).0 == 0;
            let mut d = None;
            if ok {
                for t in 0..AMediaExtractor_getTrackCount(ex) {
                    let f = AMediaExtractor_getTrackFormat(ex, t);
                    let mut us = 0i64;
                    if AMediaFormat_getInt64(f, AMEDIAFORMAT_KEY_DURATION, &mut us) {
                        d = Some(d.unwrap_or(0.0f64).max(us as f64 / 1e6));
                    }
                    AMediaFormat_delete(f);
                }
            }
            AMediaExtractor_delete(ex);
            d
        }
    }

    /// Ré-encode la vidéo au débit voulu (même taille d'image) ; le son est recopié tel quel.
    pub fn compresser(entree: &Path, sortie: &Path, debit: i32, avance: &dyn Fn(f32)) -> Result<(), String> {
        unsafe {
            let fi = std::fs::File::open(entree).map_err(|e| e.to_string())?;
            let fo = std::fs::File::create(sortie).map_err(|e| e.to_string())?;
            let ex = AMediaExtractor_new();
            if AMediaExtractor_setDataSourceFd(ex, fi.as_raw_fd(), 0, fi.metadata().map_err(|e| e.to_string())?.len() as i64).0 != 0 {
                AMediaExtractor_delete(ex);
                return Err("vidéo illisible".into());
            }
            let (mut piste_v, mut piste_a) = (None, None);
            let (mut fmt_v, mut fmt_a) = (std::ptr::null_mut(), std::ptr::null_mut());
            for t in 0..AMediaExtractor_getTrackCount(ex) {
                let f = AMediaExtractor_getTrackFormat(ex, t);
                let mime = chaine(f, AMEDIAFORMAT_KEY_MIME).unwrap_or_default();
                if mime.starts_with("video/") && piste_v.is_none() {
                    piste_v = Some(t);
                    fmt_v = f;
                } else if mime.starts_with("audio/") && piste_a.is_none() {
                    piste_a = Some(t);
                    fmt_a = f;
                } else {
                    AMediaFormat_delete(f);
                }
            }
            let Some(pv) = piste_v else {
                AMediaExtractor_delete(ex);
                return Err("pas de piste vidéo".into());
            };
            AMediaExtractor_selectTrack(ex, pv);
            if let Some(pa) = piste_a {
                AMediaExtractor_selectTrack(ex, pa);
            }
            let (l, h) = (entier(fmt_v, AMEDIAFORMAT_KEY_WIDTH).unwrap_or(0), entier(fmt_v, AMEDIAFORMAT_KEY_HEIGHT).unwrap_or(0));
            let rotation = entier(fmt_v, AMEDIAFORMAT_KEY_ROTATION).unwrap_or(0);
            let mut duree_us = 0i64;
            AMediaFormat_getInt64(fmt_v, AMEDIAFORMAT_KEY_DURATION, &mut duree_us);
            let mime_v = chaine(fmt_v, AMEDIAFORMAT_KEY_MIME).unwrap_or_default();
            // encodeur H.264 alimenté par une surface
            let enc = AMediaCodec_createEncoderByType(c"video/avc".as_ptr());
            let fe = AMediaFormat_new();
            AMediaFormat_setString(fe, AMEDIAFORMAT_KEY_MIME, c"video/avc".as_ptr());
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_WIDTH, l);
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_HEIGHT, h);
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_BIT_RATE, debit);
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_FRAME_RATE, 30);
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_I_FRAME_INTERVAL, 2);
            AMediaFormat_setInt32(fe, AMEDIAFORMAT_KEY_COLOR_FORMAT, 0x7F00_0789);
            let mut surface: *mut ANativeWindow = std::ptr::null_mut();
            let ok = !enc.is_null() && AMediaCodec_configure(enc, fe, std::ptr::null_mut(), std::ptr::null_mut(), AMEDIACODEC_CONFIGURE_FLAG_ENCODE as u32).0 == 0 && AMediaCodec_createInputSurface(enc, &mut surface).0 == 0 && AMediaCodec_start(enc).0 == 0;
            let c_mime = std::ffi::CString::new(mime_v).unwrap_or_default();
            let dec = AMediaCodec_createDecoderByType(c_mime.as_ptr());
            let ok = ok && !dec.is_null() && AMediaCodec_configure(dec, fmt_v, surface, std::ptr::null_mut(), 0).0 == 0 && AMediaCodec_start(dec).0 == 0;
            let mux = AMediaMuxer_new(fo.as_raw_fd(), OutputFormat::AMEDIAMUXER_OUTPUT_FORMAT_MPEG_4);
            let mut resultat = if ok && !mux.is_null() { Ok(()) } else { Err("encodeur vidéo indisponible sur ce téléphone".to_string()) };
            if resultat.is_ok() {
                AMediaMuxer_setOrientationHint(mux, rotation);
                let (mut fin_entree, mut fin_dec, mut fin) = (false, false, false);
                let (mut tv, mut ta) = (-1isize, -1isize);
                let mut son_en_attente: Vec<(Vec<u8>, AMediaCodecBufferInfo)> = vec![];
                let mut tampon = vec![0u8; 1 << 20];
                let mut info: AMediaCodecBufferInfo = std::mem::zeroed();
                let debut = std::time::Instant::now();
                while !fin {
                    if debut.elapsed().as_secs() > 3600 {
                        resultat = Err("trop long".into());
                        break;
                    }
                    // 1. lecture : vidéo vers le décodeur, son mis de côté
                    if !fin_entree {
                        let t = AMediaExtractor_getSampleTrackIndex(ex);
                        if t < 0 {
                            let i = AMediaCodec_dequeueInputBuffer(dec, 10_000);
                            if i >= 0 {
                                AMediaCodec_queueInputBuffer(dec, i as usize, 0, 0, 0, AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32);
                                fin_entree = true;
                            }
                        } else if Some(t as usize) == piste_a {
                            let n = AMediaExtractor_readSampleData(ex, tampon.as_mut_ptr(), tampon.len());
                            if n > 0 {
                                let mut bi: AMediaCodecBufferInfo = std::mem::zeroed();
                                bi.size = n as i32;
                                bi.presentationTimeUs = AMediaExtractor_getSampleTime(ex);
                                bi.flags = if AMediaExtractor_getSampleFlags(ex) & 1 != 0 { 1 } else { 0 };
                                son_en_attente.push((tampon[..n as usize].to_vec(), bi));
                            }
                            AMediaExtractor_advance(ex);
                        } else {
                            let i = AMediaCodec_dequeueInputBuffer(dec, 10_000);
                            if i >= 0 {
                                let mut taille = 0usize;
                                let b = AMediaCodec_getInputBuffer(dec, i as usize, &mut taille);
                                let n = AMediaExtractor_readSampleData(ex, b, taille);
                                let pts = AMediaExtractor_getSampleTime(ex);
                                AMediaCodec_queueInputBuffer(dec, i as usize, 0, n.max(0) as usize, pts as u64, 0);
                                AMediaExtractor_advance(ex);
                                if duree_us > 0 {
                                    avance((pts as f32 / duree_us as f32).clamp(0.0, 1.0));
                                }
                            }
                        }
                    }
                    // 2. décodeur → surface de l'encodeur
                    if !fin_dec {
                        let i = AMediaCodec_dequeueOutputBuffer(dec, &mut info, 10_000);
                        if i >= 0 {
                            AMediaCodec_releaseOutputBuffer(dec, i as usize, info.size > 0);
                            if info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32 != 0 {
                                AMediaCodec_signalEndOfInputStream(enc);
                                fin_dec = true;
                            }
                        }
                    }
                    // 3. encodeur → fichier
                    let i = AMediaCodec_dequeueOutputBuffer(enc, &mut info, 10_000);
                    if i == AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED as isize {
                        let f = AMediaCodec_getOutputFormat(enc);
                        tv = AMediaMuxer_addTrack(mux, f);
                        if !fmt_a.is_null() {
                            ta = AMediaMuxer_addTrack(mux, fmt_a);
                        }
                        AMediaMuxer_start(mux);
                        AMediaFormat_delete(f);
                    } else if i >= 0 {
                        let mut taille = 0usize;
                        let b = AMediaCodec_getOutputBuffer(enc, i as usize, &mut taille);
                        if info.flags & AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG as u32 == 0 && info.size > 0 && tv >= 0 {
                            AMediaMuxer_writeSampleData(mux, tv as usize, b, &info);
                        }
                        AMediaCodec_releaseOutputBuffer(enc, i as usize, false);
                        if info.flags & AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM as u32 != 0 {
                            fin = true;
                        }
                    }
                    // le son part dès que le fichier est prêt
                    if ta >= 0 && !son_en_attente.is_empty() {
                        for (d, bi) in son_en_attente.drain(..) {
                            AMediaMuxer_writeSampleData(mux, ta as usize, d.as_ptr(), &bi);
                        }
                    }
                }
                if tv >= 0 {
                    AMediaMuxer_stop(mux);
                }
            }
            if !mux.is_null() {
                AMediaMuxer_delete(mux);
            }
            if !dec.is_null() {
                AMediaCodec_stop(dec);
                AMediaCodec_delete(dec);
            }
            if !enc.is_null() {
                AMediaCodec_stop(enc);
                AMediaCodec_delete(enc);
            }
            if !surface.is_null() {
                ANativeWindow_release(surface);
            }
            AMediaFormat_delete(fe);
            AMediaFormat_delete(fmt_v);
            if !fmt_a.is_null() {
                AMediaFormat_delete(fmt_a);
            }
            AMediaExtractor_delete(ex);
            resultat
        }
    }
}

pub struct Compresseur {
    nav: crate::fichiers_tel::Navigateur,
    cible: Cible,
    e: Etat,
    avance: Arc<Mutex<f32>>,
}

impl Compresseur {
    pub fn new() -> Self {
        let mut nav = crate::fichiers_tel::Navigateur::new();
        nav.dossier = racine().join("DCIM").join("Camera");
        Compresseur { nav, cible: Cible::WhatsApp, e: Arc::default(), avance: Arc::default() }
    }
}

impl Outil for Compresseur {
    fn nom(&self) -> &'static str {
        "🎞 Compresser une vidéo"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !acces_fichiers_ui(ui) {
            return;
        }
        let occupe = etat_ui(ui, &self.e);
        if occupe {
            let a = *self.avance.lock().unwrap();
            ui.add(egui::ProgressBar::new(a).show_percentage());
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.cible, Cible::Discord, "Discord (moins de 10 Mo)");
            ui.selectable_value(&mut self.cible, Cible::WhatsApp, "WhatsApp / léger");
            ui.selectable_value(&mut self.cible, Cible::Bonne, "Bonne qualité");
        });
        ui.weak("Choisis une vidéo. La nouvelle version est enregistrée à côté de l'originale (« … compressée.mp4 »). Fonction expérimentale : dis-moi si ta vidéo ne passe pas.");
        self.nav.ui(ui);
        let videos: Vec<PathBuf> = self.nav.choisis.iter().filter(|p| famille(&p.to_string_lossy()) == "Vidéos").cloned().collect();
        if !videos.is_empty() && gros_bouton(ui, &format!("🎞 Compresser {} vidéo(s)", videos.len())) {
            self.nav.choisis.clear();
            let (c, a) = (self.cible, self.avance.clone());
            en_fond(&self.e, "Compression…", move || {
                let mut bilan = vec![];
                for v in videos {
                    let sortie = libre(v.parent().unwrap_or(Path::new(".")), &format!("{} compressée.mp4", v.file_stem().unwrap_or_default().to_string_lossy()));
                    #[cfg(target_os = "android")]
                    {
                        let d = codec::duree(&v).unwrap_or(60.0);
                        let a2 = a.clone();
                        codec::compresser(&v, &sortie, debit_cible(c, d), &move |x| *a2.lock().unwrap() = x)?;
                    }
                    #[cfg(not(target_os = "android"))]
                    {
                        let _ = (c, &a);
                        return Err("seulement sur Android".into());
                    }
                    #[allow(unreachable_code)]
                    {
                        let (t1, t2) = (v.metadata().map(|m| m.len()).unwrap_or(0), sortie.metadata().map(|m| m.len()).unwrap_or(0));
                        crate::android::signaler_media(&[sortie.display().to_string()]);
                        bilan.push(format!("{} → {}", taille(t1), taille(t2)));
                    }
                }
                Ok(format!("✔ {}", bilan.join(" · ")))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photos() {
        // image nette (damier) contre image floue (dégradé)
        let net = GrayImage::from_fn(64, 64, |x, y| image::Luma([if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 }]));
        let flou = GrayImage::from_fn(64, 64, |x, _| image::Luma([(x * 4) as u8]));
        assert!(nettete(&net) > 1000.0);
        assert!(nettete(&flou) < 5.0);
        assert_eq!(ecart(dhash(&net), dhash(&net)), 0);
        let inverse = GrayImage::from_fn(64, 64, |x, _| image::Luma([255 - (x * 4) as u8]));
        assert_eq!(ecart(dhash(&flou), dhash(&inverse)), 64);
        let f = |d: u64| Fichier { chemin: PathBuf::from(format!("/p/{d}.jpg")), taille: 1, date: d };
        let ph = vec![
            Photo { f: f(100), hash: 0b1111, nettete: 10.0, vignette: None },
            Photo { f: f(105), hash: 0b1110, nettete: 50.0, vignette: None },
            Photo { f: f(500), hash: 0b1111, nettete: 30.0, vignette: None },
        ];
        let r = rafales(&ph);
        assert_eq!(r, vec![vec![1, 0]]); // la plus nette d'abord ; la photo prise bien plus tard n'en fait pas partie
        assert!(debit_cible(Cible::Discord, 60.0) < 1_300_000);
        assert_eq!(debit_cible(Cible::Discord, 1.0), 8_000_000);
    }

    #[test]
    fn coffre() {
        let k = deriver("1234", b"sel de test 16oc").unwrap();
        let s = sceller(&k, b"photo secrete");
        assert_eq!(ouvrir(&k, &s).unwrap(), b"photo secrete");
        let k2 = deriver("1235", b"sel de test 16oc").unwrap();
        assert!(ouvrir(&k2, &s).is_none());
    }
}
