//! Fichiers, suite : photos rangées par date ou lieu (25), compresseur d'images (26), envoi entre PC (27),
//! lien de partage temporaire (28), comparer deux dossiers (29), texte d'une image (30),
//! lecteur de musique (31), infos des musiques (32).
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

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

const IMAGES: [&str; 6] = ["jpg", "jpeg", "png", "webp", "bmp", "heic"];

fn est_image(p: &Path) -> bool {
    p.extension().is_some_and(|x| IMAGES.contains(&x.to_string_lossy().to_lowercase().as_str()))
}

fn images_de(d: &Path, sous: bool, v: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            if sous {
                images_de(&p, sous, v);
            }
        } else if est_image(&p) {
            v.push(p);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 25 · Ranger les photos par date ou par lieu
// ---------------------------------------------------------------------------------------------------

/// Coordonnées GPS (degrés décimaux) lues dans l'EXIF.
pub fn gps(p: &Path) -> Option<(f64, f64)> {
    let f = std::fs::File::open(p).ok()?;
    let e = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(f)).ok()?;
    let deg = |t: exif::Tag, r: exif::Tag| -> Option<f64> {
        let c = e.get_field(t, exif::In::PRIMARY)?;
        let v = match &c.value {
            exif::Value::Rational(v) if v.len() >= 3 => v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0,
            _ => return None,
        };
        let s = e.get_field(r, exif::In::PRIMARY).map(|x| x.display_value().to_string()).unwrap_or_default();
        Some(if s.contains('S') || s.contains('W') { -v } else { v })
    };
    let (la, lo) = (deg(exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef)?, deg(exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef)?);
    (la.abs() > 1e-6 || lo.abs() > 1e-6).then_some((la, lo))
}

/// « AAAA-MM-JJ HH.MM.SS » (EXIF), sinon date de modification du fichier.
fn date_photo(p: &Path) -> String {
    crate::images::date_exif(p).unwrap_or_else(|| {
        let t = std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
        let (a, m, j) = crate::quotidien2::date_de_jours(t.div_euclid(86_400));
        format!("{a:04}-{m:02}-{j:02}")
    })
}

const MOIS: [&str; 12] = ["01 janvier", "02 février", "03 mars", "04 avril", "05 mai", "06 juin", "07 juillet", "08 août", "09 septembre", "10 octobre", "11 novembre", "12 décembre"];

/// Sous-dossier de destination : « 2024/05 mai » ou « 2024/05 mai - Lyon ».
pub fn sous_dossier(date: &str, lieu: Option<&str>) -> PathBuf {
    let a = date.get(..4).unwrap_or("Sans date");
    let m = date.get(5..7).and_then(|m| m.parse::<usize>().ok()).filter(|m| (1..=12).contains(m)).map(|m| MOIS[m - 1]).unwrap_or("??");
    let mois = match lieu {
        Some(l) if !l.is_empty() => format!("{m} - {}", l.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], " ")),
        _ => m.to_string(),
    };
    PathBuf::from(a).join(mois)
}

fn ville(la: f64, lo: f64, cache: &mut HashMap<(i64, i64), String>) -> String {
    let k = ((la * 20.0).round() as i64, (lo * 20.0).round() as i64); // ≈ 5 km
    if let Some(v) = cache.get(&k) {
        return v.clone();
    }
    let v = ureq::get("https://api.bigdatacloud.net/data/reverse-geocode-client").timeout(Duration::from_secs(10))
        .query("latitude", &la.to_string()).query("longitude", &lo.to_string()).query("localityLanguage", "fr").call().ok()
        .and_then(|r| r.into_json::<serde_json::Value>().ok())
        .map(|j| [&j["city"], &j["locality"], &j["principalSubdivision"], &j["countryName"]].iter().find_map(|x| x.as_str().filter(|s| !s.is_empty()).map(String::from)).unwrap_or_default())
        .unwrap_or_default();
    cache.insert(k, v.clone());
    v
}

pub struct RangerPhotos {
    source: Option<PathBuf>,
    dest: Option<PathBuf>,
    par_lieu: bool,
    copier: bool,
    plan: Arc<Mutex<Option<Vec<(PathBuf, PathBuf)>>>>,
    e: Etat,
}

impl RangerPhotos {
    pub fn new() -> Self {
        RangerPhotos { source: None, dest: None, par_lieu: false, copier: true, plan: Arc::default(), e: Arc::default() }
    }
}

impl Outil for RangerPhotos {
    fn nom(&self) -> &'static str {
        "🗓 Ranger les photos"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ranger les photos par date (et par lieu)");
        ui.weak("Les photos du téléphone ou d'un appareil, rangées en « 2024 / 05 mai - Lyon » grâce à la date et à la position enregistrées dans chaque photo.");
        ui.horizontal(|ui| {
            if ui.button("📁 Photos à ranger…").clicked() {
                self.source = rfd::FileDialog::new().pick_folder();
                *self.plan.lock().unwrap() = None;
            }
            ui.label(self.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        });
        ui.horizontal(|ui| {
            if ui.button("📁 Ranger dans…").clicked() {
                self.dest = rfd::FileDialog::new().pick_folder();
                *self.plan.lock().unwrap() = None;
            }
            ui.label(self.dest.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
        });
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.par_lieu, "Ajouter la ville (photos avec position GPS)").changed() {
                *self.plan.lock().unwrap() = None;
            }
            ui.radio_value(&mut self.copier, true, "Copier");
            ui.radio_value(&mut self.copier, false, "Déplacer");
        });
        let occupe = etat_ui(ui, &self.e);
        let (Some(src), Some(dst)) = (self.source.clone(), self.dest.clone()) else { return };
        if ui.add_enabled(!occupe, egui::Button::new("🔎 Préparer")).clicked() {
            let (plan, lieu) = (self.plan.clone(), self.par_lieu);
            en_fond(&self.e, move || {
                let mut v = vec![];
                images_de(&src, true, &mut v);
                let mut cache = HashMap::new();
                let mut p = vec![];
                for f in v {
                    let l = if lieu { gps(&f).map(|(a, b)| ville(a, b, &mut cache)) } else { None };
                    let d = dst.join(sous_dossier(&date_photo(&f), l.as_deref())).join(f.file_name().unwrap_or_default());
                    p.push((f, d));
                }
                let n = p.len();
                *plan.lock().unwrap() = Some(p);
                Ok(format!("{n} photo(s) à ranger."))
            });
        }
        let plan = self.plan.lock().unwrap().clone();
        if let Some(p) = plan {
            let mut dossiers: BTreeMap<PathBuf, usize> = BTreeMap::new();
            for (_, d) in &p {
                *dossiers.entry(d.parent().unwrap().to_path_buf()).or_default() += 1;
            }
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (d, n) in &dossiers {
                    ui.label(format!("📁 {} — {n} photo(s)", d.strip_prefix(self.dest.as_ref().unwrap()).unwrap_or(d).display()));
                }
            });
            if ui.add_enabled(!occupe && !p.is_empty(), egui::Button::new(format!("{} {} photo(s)", if self.copier { "📋 Copier" } else { "➡ Déplacer" }, p.len()))).clicked() {
                let (copier, pl) = (self.copier, self.plan.clone());
                en_fond(&self.e, move || {
                    let (mut ok, mut deja) = (0, 0);
                    for (a, b) in p {
                        if b.exists() {
                            deja += 1;
                            continue;
                        }
                        let _ = std::fs::create_dir_all(b.parent().unwrap());
                        let r = if copier { std::fs::copy(&a, &b).map(|_| ()) } else { std::fs::rename(&a, &b).or_else(|_| std::fs::copy(&a, &b).and_then(|_| std::fs::remove_file(&a))) };
                        if r.is_ok() {
                            ok += 1;
                        }
                    }
                    *pl.lock().unwrap() = None;
                    Ok(format!("✔ {ok} photo(s) rangée(s){}", if deja > 0 { format!(", {deja} déjà présente(s)") } else { String::new() }))
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 26 · Compresseur d'images
// ---------------------------------------------------------------------------------------------------

/// Nouvelles dimensions pour que le plus grand côté fasse au plus `max`.
pub fn reduire(l: u32, h: u32, max: u32) -> (u32, u32) {
    if l.max(h) <= max {
        return (l, h);
    }
    if l >= h { (max, (h as u64 * max as u64 / l as u64).max(1) as u32) } else { ((l as u64 * max as u64 / h as u64).max(1) as u32, max) }
}

fn compresser(p: &Path, dest: &Path, max: u32, qualite: u8, en_jpeg: bool) -> Result<(u64, u64), String> {
    let avant = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let img = image::open(p).map_err(|e| format!("{} : {e}", p.display()))?;
    let (l, h) = reduire(img.width(), img.height(), max);
    let img = if (l, h) != (img.width(), img.height()) { img.resize_exact(l, h, image::imageops::FilterType::Lanczos3) } else { img };
    let png = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) && !en_jpeg;
    let sortie = dest.join(p.file_stem().unwrap_or_default()).with_extension(if png { "png" } else { "jpg" });
    let f = std::io::BufWriter::new(std::fs::File::create(&sortie).map_err(|e| e.to_string())?);
    if png {
        let enc = image::codecs::png::PngEncoder::new_with_quality(f, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
        img.write_with_encoder(enc).map_err(|e| e.to_string())?;
    } else {
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(f, qualite);
        img.to_rgb8().write_with_encoder(enc).map_err(|e| e.to_string())?;
    }
    Ok((avant, std::fs::metadata(&sortie).map(|m| m.len()).unwrap_or(0)))
}

pub struct Compresseur {
    fichiers: Vec<PathBuf>,
    max: u32,
    qualite: u8,
    en_jpeg: bool,
    progres: Arc<Mutex<(usize, u64, u64)>>,
    e: Etat,
}

impl Compresseur {
    pub fn new() -> Self {
        Compresseur { fichiers: vec![], max: 1920, qualite: 82, en_jpeg: true, progres: Arc::default(), e: Arc::default() }
    }
}

impl Outil for Compresseur {
    fn nom(&self) -> &'static str {
        "🗜 Compresser des images"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Compresser des images");
        ui.weak("Alléger des photos avant de les envoyer (mail, Discord, site) : redimensionnées et recompressées dans un dossier « compressées ». Les originaux ne sont pas touchés.");
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in deposes {
            if p.is_dir() {
                images_de(&p, false, &mut self.fichiers);
            } else if est_image(&p) {
                self.fichiers.push(p);
            }
        }
        ui.horizontal(|ui| {
            if ui.button("🖼 Ajouter des images…").clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Images", &IMAGES).pick_files() {
                    self.fichiers.extend(v);
                }
            }
            if ui.button("📁 Un dossier…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    images_de(&d, false, &mut self.fichiers);
                }
            }
            if !self.fichiers.is_empty() && ui.button("Vider").clicked() {
                self.fichiers.clear();
            }
            ui.weak("(ou glisse-les ici)");
        });
        self.fichiers.dedup();
        let total: u64 = self.fichiers.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
        ui.label(format!("{} image(s), {}", self.fichiers.len(), taille(total)));
        ui.horizontal(|ui| {
            ui.label("Taille max");
            egui::ComboBox::from_id_salt("max_img").selected_text(format!("{} px", self.max)).show_ui(ui, |ui| {
                for m in [800, 1280, 1600, 1920, 2560, 3840, 100_000] {
                    ui.selectable_value(&mut self.max, m, if m == 100_000 { "taille d'origine".into() } else { format!("{m} px") });
                }
            });
            ui.label("Qualité");
            ui.add(egui::Slider::new(&mut self.qualite, 40..=95));
            ui.checkbox(&mut self.en_jpeg, "PNG → JPG");
        });
        let occupe = etat_ui(ui, &self.e);
        if occupe {
            let (n, ..) = *self.progres.lock().unwrap();
            ui.add(egui::ProgressBar::new(n as f32 / self.fichiers.len().max(1) as f32));
        }
        if ui.add_enabled(!occupe && !self.fichiers.is_empty(), egui::Button::new("🗜 Compresser")).clicked() {
            let (v, max, q, j, pr) = (self.fichiers.clone(), self.max, self.qualite, self.en_jpeg, self.progres.clone());
            *pr.lock().unwrap() = (0, 0, 0);
            en_fond(&self.e, move || {
                let dest = v[0].parent().unwrap_or(Path::new(".")).join("compressées");
                std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
                let mut erreurs = 0;
                for p in &v {
                    match compresser(p, &dest, max, q, j) {
                        Ok((a, b)) => {
                            let mut x = pr.lock().unwrap();
                            x.1 += a;
                            x.2 += b;
                        }
                        Err(_) => erreurs += 1,
                    }
                    pr.lock().unwrap().0 += 1;
                }
                let (_, a, b) = *pr.lock().unwrap();
                ouvrir(&dest);
                Ok(format!("✔ {} → {} ({}% de gagné){}", taille(a), taille(b), if a > 0 { 100 - b * 100 / a } else { 0 }, if erreurs > 0 { format!(", {erreurs} illisible(s)") } else { String::new() }))
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 27 · Envoyer des fichiers à un autre PC de la maison
// ---------------------------------------------------------------------------------------------------

const PORT_DECOUVERTE: u16 = 47_310;
const PORT_ENVOI: u16 = 47_311;

fn nom_pc() -> String {
    std::env::var("COMPUTERNAME").unwrap_or("PC".into())
}

fn dossier_recus() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Downloads").join("Reçus d'un autre PC")
}

/// En-tête d'un envoi : une ligne JSON terminée par \n.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Entete {
    pub de: String,
    pub nom: String,
    pub taille: u64,
}

pub fn nom_sur(n: &str) -> String {
    let n: String = n.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let n = n.trim().trim_start_matches('.').to_string();
    if n.is_empty() { "fichier".into() } else { n }
}

pub struct EnvoiPc {
    recevoir: Arc<AtomicBool>,
    lance: bool,
    pcs: Arc<Mutex<BTreeMap<String, String>>>, // nom → ip
    recus: Arc<Mutex<Vec<String>>>,
    envoi: Arc<Mutex<(bool, String, f32)>>,
}

impl EnvoiPc {
    pub fn new() -> Self {
        let recevoir = Arc::new(AtomicBool::new(charger::<Option<bool>>("envoi_pc").unwrap_or(false)));
        let mut e = EnvoiPc { recevoir, lance: false, pcs: Arc::default(), recus: Arc::default(), envoi: Arc::default() };
        if e.recevoir.load(Relaxed) {
            e.lancer_serveurs();
        }
        e
    }

    fn lancer_serveurs(&mut self) {
        if self.lance {
            return;
        }
        self.lance = true;
        let r = self.recevoir.clone();
        // réponse aux recherches
        std::thread::spawn(move || {
            let Ok(s) = std::net::UdpSocket::bind(("0.0.0.0", PORT_DECOUVERTE)) else { return };
            let mut b = [0u8; 64];
            while let Ok((n, de)) = s.recv_from(&mut b) {
                if r.load(Relaxed) && &b[..n] == b"OUTILS?" {
                    let _ = s.send_to(format!("OUTILS!{}", nom_pc()).as_bytes(), de);
                }
            }
        });
        let (r, recus) = (self.recevoir.clone(), self.recus.clone());
        std::thread::spawn(move || {
            let Ok(l) = std::net::TcpListener::bind(("0.0.0.0", PORT_ENVOI)) else { return };
            for s in l.incoming().flatten() {
                if !r.load(Relaxed) {
                    continue;
                }
                let recus = recus.clone();
                std::thread::spawn(move || {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
                    let mut lect = std::io::BufReader::new(s);
                    let mut ligne = String::new();
                    if std::io::BufRead::read_line(&mut lect, &mut ligne).is_err() {
                        return;
                    }
                    let Ok(e) = serde_json::from_str::<Entete>(ligne.trim()) else { return };
                    let d = dossier_recus();
                    let _ = std::fs::create_dir_all(&d);
                    let nom = nom_sur(&e.nom);
                    let mut dest = d.join(&nom);
                    let mut k = 2;
                    while dest.exists() {
                        dest = d.join(format!("({k}) {nom}"));
                        k += 1;
                    }
                    let ok = std::fs::File::create(&dest).and_then(|mut f| std::io::copy(&mut (&mut lect).take(e.taille), &mut f)).is_ok_and(|n| n == e.taille);
                    if ok {
                        recus.lock().unwrap().push(format!("📥 {nom} ({}) de {}", taille(e.taille), e.de));
                        crate::systeme::notifier("Fichier reçu", &format!("{nom} de {}", e.de));
                        let _ = lect.get_mut().write_all(b"OK");
                    } else {
                        let _ = std::fs::remove_file(&dest);
                    }
                });
            }
        });
    }

    fn chercher(&self) {
        let pcs = self.pcs.clone();
        std::thread::spawn(move || {
            let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") else { return };
            let _ = s.set_broadcast(true);
            let _ = s.set_read_timeout(Some(Duration::from_millis(400)));
            for _ in 0..3 {
                let _ = s.send_to(b"OUTILS?", ("255.255.255.255", PORT_DECOUVERTE));
                let t = Instant::now();
                let mut b = [0u8; 128];
                while t.elapsed() < Duration::from_millis(800) {
                    if let Ok((n, de)) = s.recv_from(&mut b) {
                        if let Some(nom) = std::str::from_utf8(&b[..n]).ok().and_then(|x| x.strip_prefix("OUTILS!")) {
                            if nom != nom_pc() {
                                pcs.lock().unwrap().insert(nom.to_string(), de.ip().to_string());
                            }
                        }
                    }
                }
            }
        });
    }

    fn envoyer(&self, ip: String, fichiers: Vec<PathBuf>) {
        let e = self.envoi.clone();
        *e.lock().unwrap() = (true, "Connexion…".into(), 0.0);
        std::thread::spawn(move || {
            let total: u64 = fichiers.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum::<u64>().max(1);
            let mut fait = 0u64;
            for p in &fichiers {
                let r = (|| -> Result<(), String> {
                    let n = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
                    let mut s = std::net::TcpStream::connect_timeout(&format!("{ip}:{PORT_ENVOI}").parse().map_err(|_| "adresse ?")?, Duration::from_secs(5)).map_err(|e| e.to_string())?;
                    let ent = Entete { de: nom_pc(), nom: p.file_name().unwrap_or_default().to_string_lossy().into_owned(), taille: n };
                    s.write_all(format!("{}\n", serde_json::to_string(&ent).unwrap()).as_bytes()).map_err(|e| e.to_string())?;
                    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
                    let mut buf = vec![0u8; 1 << 20];
                    loop {
                        let k = f.read(&mut buf).map_err(|e| e.to_string())?;
                        if k == 0 {
                            break;
                        }
                        s.write_all(&buf[..k]).map_err(|e| e.to_string())?;
                        fait += k as u64;
                        let mut x = e.lock().unwrap();
                        x.1 = format!("Envoi de {}…", ent.nom);
                        x.2 = fait as f32 / total as f32;
                    }
                    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
                    let mut ok = [0u8; 2];
                    s.read_exact(&mut ok).map_err(|_| "l'autre PC n'a pas confirmé".to_string())?;
                    Ok(())
                })();
                if let Err(m) = r {
                    *e.lock().unwrap() = (false, format!("✖ {} : {m}", p.display()), 0.0);
                    return;
                }
            }
            *e.lock().unwrap() = (false, format!("✔ {} fichier(s) envoyé(s)", fichiers.len()), 1.0);
        });
    }
}

impl Outil for EnvoiPc {
    fn nom(&self) -> &'static str {
        "📡 Envoyer à un autre PC"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Envoyer des fichiers à un autre PC de la maison");
        ui.weak("Sans clé USB ni internet : les deux PC doivent avoir Outils ouvert et être sur le même réseau.");
        let mut r = self.recevoir.load(Relaxed);
        if ui.checkbox(&mut r, format!("Ce PC ({}) peut recevoir des fichiers", nom_pc())).changed() {
            self.recevoir.store(r, Relaxed);
            enregistrer("envoi_pc", &Some(r));
            if r {
                self.lancer_serveurs();
            }
        }
        if r {
            ui.weak(format!("Les fichiers reçus vont dans {}. Si Windows demande l'accès au réseau pour Outils, accepte (réseaux privés).", dossier_recus().display()));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("PC trouvés");
            if ui.button("🔎 Chercher").clicked() {
                self.chercher();
            }
        });
        let pcs = self.pcs.lock().unwrap().clone();
        if pcs.is_empty() {
            ui.weak("Aucun pour l'instant : sur l'autre PC, coche « peut recevoir » puis clique « Chercher » ici.");
        }
        let (en_cours, msg, f) = self.envoi.lock().unwrap().clone();
        for (nom, ip) in pcs {
            ui.horizontal(|ui| {
                ui.label(format!("💻 {nom}"));
                ui.weak(&ip);
                if ui.add_enabled(!en_cours, egui::Button::new("📤 Envoyer des fichiers…")).clicked() {
                    if let Some(v) = rfd::FileDialog::new().pick_files() {
                        self.envoyer(ip.clone(), v);
                    }
                }
            });
        }
        if en_cours {
            ui.add(egui::ProgressBar::new(f).text(&msg));
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        } else if !msg.is_empty() {
            ui.label(msg);
        }
        let recus = self.recus.lock().unwrap().clone();
        if !recus.is_empty() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Reçus");
                if ui.small_button("📂 Ouvrir le dossier").clicked() {
                    ouvrir(&dossier_recus());
                }
            });
            for l in recus.iter().rev() {
                ui.label(l);
            }
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 28 · Lien de partage temporaire
// ---------------------------------------------------------------------------------------------------

/// Corps multipart : (début, fin) autour du contenu du fichier.
pub fn multipart(limite: &str, champs: &[(&str, &str)], champ_fichier: &str, nom: &str) -> (Vec<u8>, Vec<u8>) {
    let mut d = String::new();
    for (k, v) in champs {
        d.push_str(&format!("--{limite}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"));
    }
    d.push_str(&format!("--{limite}\r\nContent-Disposition: form-data; name=\"{champ_fichier}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n", nom.replace('"', "")));
    (d.into_bytes(), format!("\r\n--{limite}--\r\n").into_bytes())
}

struct Compte<R> {
    r: R,
    n: Arc<AtomicU64>,
}

impl<R: Read> Read for Compte<R> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let k = self.r.read(b)?;
        self.n.fetch_add(k as u64, Relaxed);
        Ok(k)
    }
}

fn televerser(p: &Path, duree: &str, n: Arc<AtomicU64>) -> Result<String, String> {
    let lim = format!("----Outils{}", crate::outil::hasard(u64::MAX));
    let nom = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let (debut, fin) = multipart(&lim, &[("reqtype", "fileupload"), ("time", duree)], "fileToUpload", &nom);
    let f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let longueur = debut.len() as u64 + f.metadata().map_err(|e| e.to_string())?.len() + fin.len() as u64;
    let corps = Compte { r: std::io::Cursor::new(debut).chain(f).chain(std::io::Cursor::new(fin)), n };
    let r = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).build()
        .post("https://litterbox.catbox.moe/resources/internals/api.php")
        .set("Content-Type", &format!("multipart/form-data; boundary={lim}")).set("Content-Length", &longueur.to_string())
        .send(corps).map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let r = r.trim().to_string();
    if r.starts_with("https://") { Ok(r) } else { Err(r) }
}

pub struct LienTemporaire {
    duree: &'static str,
    e: Etat,
    lien: Arc<Mutex<Option<String>>>,
    envoye: Arc<AtomicU64>,
    total: u64,
}

impl LienTemporaire {
    pub fn new() -> Self {
        LienTemporaire { duree: "24h", e: Arc::default(), lien: Arc::default(), envoye: Arc::default(), total: 1 }
    }
}

impl Outil for LienTemporaire {
    fn nom(&self) -> &'static str {
        "🔗 Lien de partage temporaire"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Lien de partage temporaire");
        ui.weak("Envoie un fichier (jusqu'à 1 Go) sur internet et obtiens un lien à donner : il s'efface tout seul après la durée choisie. Ne partage rien de privé : toute personne qui a le lien peut le télécharger.");
        ui.horizontal(|ui| {
            ui.label("Durée");
            for d in ["1h", "12h", "24h", "72h"] {
                ui.selectable_value(&mut self.duree, d, d);
            }
        });
        let occupe = etat_ui(ui, &self.e);
        if occupe {
            ui.add(egui::ProgressBar::new(self.envoye.load(Relaxed) as f32 / self.total as f32).show_percentage());
        }
        let depose = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        let mut choisi = depose;
        if ui.add_enabled(!occupe, egui::Button::new(RichText::new("📤 Choisir un fichier…").size(16.0))).clicked() {
            choisi = rfd::FileDialog::new().pick_file();
        }
        ui.weak("(ou glisse-le ici)");
        if let (Some(p), false) = (choisi, occupe) {
            let n = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            if n > 1 << 30 {
                *self.e.lock().unwrap() = (false, "✖ Plus de 1 Go : trop gros pour ce service.".into());
            } else {
                self.total = n.max(1);
                self.envoye.store(0, Relaxed);
                let (d, l, c) = (self.duree, self.lien.clone(), self.envoye.clone());
                en_fond(&self.e, move || {
                    let u = televerser(&p, d, c)?;
                    *l.lock().unwrap() = Some(u.clone());
                    Ok(format!("✔ En ligne pour {d}"))
                });
            }
        }
        if let Some(l) = self.lien.lock().unwrap().clone() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&l).monospace().size(16.0));
                if ui.button("📋 Copier").clicked() {
                    ui.ctx().copy_text(l.clone());
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 29 · Comparer deux dossiers
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Diff {
    SeulementA,
    SeulementB,
    PlusRecentA,
    PlusRecentB,
    Identique,
}

pub type Inventaire = BTreeMap<String, (u64, SystemTime)>;

fn inventaire(racine: &Path, d: &Path, v: &mut Inventaire) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            inventaire(racine, &p, v);
        } else if let Ok(m) = e.metadata() {
            let rel = p.strip_prefix(racine).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            v.insert(rel, (m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
        }
    }
}

pub fn comparer(a: &Inventaire, b: &Inventaire) -> Vec<(String, Diff)> {
    let mut cles: Vec<&String> = a.keys().chain(b.keys()).collect();
    cles.sort();
    cles.dedup();
    cles.into_iter().map(|k| {
        let d = match (a.get(k), b.get(k)) {
            (Some(_), None) => Diff::SeulementA,
            (None, Some(_)) => Diff::SeulementB,
            (Some(x), Some(y)) => {
                let ecart = |p: SystemTime, q: SystemTime| p.duration_since(q).is_ok_and(|d| d > Duration::from_secs(2));
                if x.0 == y.0 && !ecart(x.1, y.1) && !ecart(y.1, x.1) {
                    Diff::Identique
                } else if ecart(y.1, x.1) {
                    Diff::PlusRecentB
                } else {
                    Diff::PlusRecentA
                }
            }
            (None, None) => unreachable!(),
        };
        (k.clone(), d)
    }).collect()
}

pub struct ComparerDossiers {
    a: Option<PathBuf>,
    b: Option<PathBuf>,
    res: Arc<Mutex<Option<Vec<(String, Diff)>>>>,
    voir_identiques: bool,
    e: Etat,
}

impl ComparerDossiers {
    pub fn new() -> Self {
        ComparerDossiers { a: None, b: None, res: Arc::default(), voir_identiques: false, e: Arc::default() }
    }
}

impl Outil for ComparerDossiers {
    fn nom(&self) -> &'static str {
        "🔀 Comparer deux dossiers"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparer deux dossiers");
        ui.weak("Trouve ce qui manque ou a changé entre deux dossiers (ex. le PC et un disque de sauvegarde), puis recopie ce qu'il faut.");
        for (n, c) in [("A", &mut self.a), ("B", &mut self.b)] {
            ui.horizontal(|ui| {
                if ui.button(format!("📁 Dossier {n}…")).clicked() {
                    *c = rfd::FileDialog::new().pick_folder();
                    *self.res.lock().unwrap() = None;
                }
                ui.label(c.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
            });
        }
        let occupe = etat_ui(ui, &self.e);
        let (Some(a), Some(b)) = (self.a.clone(), self.b.clone()) else { return };
        if ui.add_enabled(!occupe, egui::Button::new("🔎 Comparer")).clicked() {
            let res = self.res.clone();
            let (a, b) = (a.clone(), b.clone());
            en_fond(&self.e, move || {
                let (mut x, mut y) = (Inventaire::new(), Inventaire::new());
                inventaire(&a, &a, &mut x);
                inventaire(&b, &b, &mut y);
                let r = comparer(&x, &y);
                let n = r.iter().filter(|d| d.1 != Diff::Identique).count();
                *res.lock().unwrap() = Some(r);
                Ok(format!("{n} différence(s)."))
            });
        }
        let Some(r) = self.res.lock().unwrap().clone() else { return };
        let compte = |d: Diff| r.iter().filter(|x| x.1 == d).count();
        ui.label(format!("Seulement dans A : {} · seulement dans B : {} · plus récent dans A : {} · plus récent dans B : {} · identiques : {}",
            compte(Diff::SeulementA), compte(Diff::SeulementB), compte(Diff::PlusRecentA), compte(Diff::PlusRecentB), compte(Diff::Identique)));
        ui.checkbox(&mut self.voir_identiques, "Afficher aussi les identiques");
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (k, d) in r.iter().filter(|x| self.voir_identiques || x.1 != Diff::Identique).take(2000) {
                let (t, c) = match d {
                    Diff::SeulementA => ("A seul", Color32::from_rgb(120, 170, 255)),
                    Diff::SeulementB => ("B seul", Color32::from_rgb(255, 120, 170)),
                    Diff::PlusRecentA => ("A + récent", Color32::from_rgb(250, 210, 90)),
                    Diff::PlusRecentB => ("B + récent", Color32::from_rgb(250, 160, 90)),
                    Diff::Identique => ("=", Color32::GRAY),
                };
                ui.horizontal(|ui| {
                    ui.colored_label(c, RichText::new(format!("{t:10}")).monospace());
                    ui.label(k);
                });
            }
        });
        let copier = |de: &Path, vers: &Path, liste: Vec<String>| -> Result<String, String> {
            let mut n = 0;
            for k in &liste {
                let (s, d) = (de.join(k), vers.join(k));
                let _ = std::fs::create_dir_all(d.parent().unwrap());
                if std::fs::copy(&s, &d).is_ok() {
                    n += 1;
                }
            }
            Ok(format!("✔ {n} fichier(s) copié(s)"))
        };
        ui.horizontal_wrapped(|ui| {
            let vers_b: Vec<String> = r.iter().filter(|x| matches!(x.1, Diff::SeulementA | Diff::PlusRecentA)).map(|x| x.0.clone()).collect();
            let vers_a: Vec<String> = r.iter().filter(|x| matches!(x.1, Diff::SeulementB | Diff::PlusRecentB)).map(|x| x.0.clone()).collect();
            if ui.add_enabled(!occupe && !vers_b.is_empty(), egui::Button::new(format!("A → B ({})", vers_b.len()))).on_hover_text("Copie dans B ce qui manque ou est plus récent dans A").clicked() {
                let (a, b, res) = (a.clone(), b.clone(), self.res.clone());
                en_fond(&self.e, move || {
                    let m = copier(&a, &b, vers_b);
                    *res.lock().unwrap() = None;
                    m
                });
            }
            if ui.add_enabled(!occupe && !vers_a.is_empty(), egui::Button::new(format!("B → A ({})", vers_a.len()))).on_hover_text("Copie dans A ce qui manque ou est plus récent dans B").clicked() {
                let (a, b, res) = (a.clone(), b.clone(), self.res.clone());
                en_fond(&self.e, move || {
                    let m = copier(&b, &a, vers_a);
                    *res.lock().unwrap() = None;
                    m
                });
            }
            if ui.add_enabled(!occupe, egui::Button::new("⇄ Les deux sens")).on_hover_text("Chaque dossier reçoit ce qui lui manque, la version la plus récente gagne").clicked() {
                let (a, b, res, r2) = (a.clone(), b.clone(), self.res.clone(), r.clone());
                en_fond(&self.e, move || {
                    let ab: Vec<String> = r2.iter().filter(|x| matches!(x.1, Diff::SeulementA | Diff::PlusRecentA)).map(|x| x.0.clone()).collect();
                    let ba: Vec<String> = r2.iter().filter(|x| matches!(x.1, Diff::SeulementB | Diff::PlusRecentB)).map(|x| x.0.clone()).collect();
                    let m1 = copier(&a, &b, ab)?;
                    let m2 = copier(&b, &a, ba)?;
                    *res.lock().unwrap() = None;
                    Ok(format!("{m1} vers B, {} vers A", m2.trim_start_matches("✔ ")))
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 30 · Lire le texte d'une image (OCR de Windows)
// ---------------------------------------------------------------------------------------------------

fn ocr(p: &Path) -> Result<String, String> {
    use windows::core::HSTRING;
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::{FileAccessMode, StorageFile};
    let e = |x: windows::core::Error| x.message();
    let chemin = std::fs::canonicalize(p).map_err(|x| x.to_string())?;
    let chemin = chemin.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let f = StorageFile::GetFileFromPathAsync(&HSTRING::from(chemin)).map_err(e)?.get().map_err(e)?;
    let flux = f.OpenAsync(FileAccessMode::Read).map_err(e)?.get().map_err(e)?;
    let dec = BitmapDecoder::CreateAsync(&flux).map_err(e)?.get().map_err(e)?;
    let bmp = dec.GetSoftwareBitmapAsync().map_err(e)?.get().map_err(e)?;
    let moteur = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| "aucune langue OCR installée (Paramètres › Heure et langue › Langue › Français › Options)".to_string())?;
    let r = moteur.RecognizeAsync(&bmp).map_err(e)?.get().map_err(e)?;
    let lignes = r.Lines().map_err(e)?;
    let mut t = String::new();
    for l in lignes {
        t.push_str(&l.Text().map_err(e)?.to_string());
        t.push('\n');
    }
    Ok(t)
}

fn image_temp(img: &image::RgbaImage) -> Result<PathBuf, String> {
    let p = std::env::temp_dir().join("outils_ocr.png");
    img.save(&p).map_err(|e| e.to_string())?;
    Ok(p)
}

pub struct TexteImage {
    texte: Arc<Mutex<String>>,
    e: Etat,
    attente: Arc<AtomicBool>,
}

impl TexteImage {
    pub fn new() -> Self {
        TexteImage { texte: Arc::default(), e: Arc::default(), attente: Arc::default() }
    }

    fn lancer(&self, p: PathBuf) {
        let t = self.texte.clone();
        en_fond(&self.e, move || {
            let r = ocr(&p)?;
            let n = r.split_whitespace().count();
            *t.lock().unwrap() = r;
            Ok(if n == 0 { "Aucun texte trouvé.".into() } else { format!("✔ {n} mot(s) lus.") })
        });
    }
}

impl Outil for TexteImage {
    fn nom(&self) -> &'static str {
        "🔤 Texte d'une image (OCR)"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Lire le texte d'une image");
        ui.weak("Récupère le texte d'une capture, d'une photo de document ou d'une image (avec le moteur de reconnaissance intégré à Windows, rien n'est envoyé sur internet).");
        let occupe = etat_ui(ui, &self.e);
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.lancer(p);
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(!occupe && !self.attente.load(Relaxed), |ui| {
                if ui.button("🖼 Ouvrir une image…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "webp", "tif", "tiff"]).pick_file() {
                        self.lancer(p);
                    }
                }
                if ui.button("📋 Image du presse-papiers").on_hover_text("Fais d'abord Win+Maj+S pour capturer une zone").clicked() {
                    match crate::pressepapiers::win::lire(true) {
                        Some(crate::pressepapiers::win::Contenu::Image(dib)) => match crate::pressepapiers::dib_rgba(&dib).and_then(|(l, h, px)| image::RgbaImage::from_raw(l, h, px)) {
                            Some(img) => match image_temp(&img) {
                                Ok(p) => self.lancer(p),
                                Err(m) => *self.e.lock().unwrap() = (false, m),
                            },
                            None => *self.e.lock().unwrap() = (false, "Image illisible.".into()),
                        },
                        _ => *self.e.lock().unwrap() = (false, "Pas d'image dans le presse-papiers : fais Win+Maj+S puis réessaie.".into()),
                    }
                }
                if ui.button("🪟 Fenêtre au premier plan (3 s)").clicked() {
                    self.attente.store(true, Relaxed);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    let (a, t, e, ctx) = (self.attente.clone(), self.texte.clone(), self.e.clone(), ui.ctx().clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(3));
                        let r = crate::capture::win::capturer().ok_or("capture impossible".to_string()).and_then(|ec| {
                            let (x, y, l, h) = crate::capture::win::fenetre_active(&ec).unwrap_or((0, 0, ec.w, ec.h));
                            image_temp(&ec.rgba(x, y, l, h))
                        }).and_then(|p| ocr(&p));
                        *e.lock().unwrap() = (false, match r {
                            Ok(x) => {
                                let n = x.split_whitespace().count();
                                *t.lock().unwrap() = x;
                                format!("✔ {n} mot(s) lus.")
                            }
                            Err(m) => format!("✖ {m}"),
                        });
                        a.store(false, Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    });
                }
            });
        });
        if self.attente.load(Relaxed) {
            ui.label("📸 Capture dans 3 s…");
        }
        let mut t = self.texte.lock().unwrap();
        if !t.is_empty() {
            ui.horizontal(|ui| {
                if ui.button("📋 Copier le texte").clicked() {
                    ui.ctx().copy_text(t.clone());
                }
                if ui.button("🌐 Traduire").on_hover_text("Ouvre l'onglet Traducteur avec ce texte copié").clicked() {
                    ui.ctx().copy_text(t.clone());
                }
            });
            ui.add(egui::TextEdit::multiline(&mut *t).desired_rows(14).desired_width(f32::INFINITY));
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 31 · Lecteur de musique / 32 · Infos des musiques
// ---------------------------------------------------------------------------------------------------

const AUDIO: [&str; 7] = ["mp3", "flac", "ogg", "wav", "m4a", "aac", "opus"];

fn est_audio(p: &Path) -> bool {
    p.extension().is_some_and(|x| AUDIO.contains(&x.to_string_lossy().to_lowercase().as_str()))
}

fn musiques_de(d: &Path, v: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            musiques_de(&p, v);
        } else if est_audio(&p) {
            v.push(p);
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Infos {
    pub titre: String,
    pub artiste: String,
    pub album: String,
    pub annee: String,
    pub genre: String,
    pub piste: String,
    pub duree: u64,
    pub pochette: Option<Vec<u8>>,
}

pub(crate) fn lire_infos(p: &Path) -> Infos {
    use lofty::prelude::*;
    let Ok(f) = lofty::read_from_path(p) else { return Infos { titre: nom_fichier(p), ..Default::default() } };
    let duree = f.properties().duration().as_secs();
    let t = f.primary_tag().or_else(|| f.first_tag());
    let s = |x: Option<std::borrow::Cow<str>>| x.map(|c| c.into_owned()).unwrap_or_default();
    match t {
        Some(t) => Infos {
            titre: Some(s(t.title())).filter(|x| !x.is_empty()).unwrap_or_else(|| nom_fichier(p)),
            artiste: s(t.artist()),
            album: s(t.album()),
            annee: t.year().map(|y| y.to_string()).unwrap_or_default(),
            genre: s(t.genre()),
            piste: t.track().map(|y| y.to_string()).unwrap_or_default(),
            duree,
            pochette: t.pictures().first().map(|x| x.data().to_vec()),
        },
        None => Infos { titre: nom_fichier(p), duree, ..Default::default() },
    }
}

fn ecrire_infos(p: &Path, i: &Infos, nouvelle_pochette: Option<Vec<u8>>) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::picture::{MimeType, Picture, PictureType};
    use lofty::prelude::*;
    use lofty::tag::Tag;
    let mut f = lofty::read_from_path(p).map_err(|e| e.to_string())?;
    if f.primary_tag().is_none() {
        let t = f.primary_tag_type();
        f.insert_tag(Tag::new(t));
    }
    let t = f.primary_tag_mut().ok_or("pas de tag possible pour ce fichier")?;
    let met = |t: &mut Tag, v: &str, set: fn(&mut Tag, String), rm: fn(&mut Tag)| if v.trim().is_empty() { rm(t) } else { set(t, v.trim().to_string()) };
    met(t, &i.titre, |t, v| t.set_title(v), |t| t.remove_title());
    met(t, &i.artiste, |t, v| t.set_artist(v), |t| t.remove_artist());
    met(t, &i.album, |t, v| t.set_album(v), |t| t.remove_album());
    met(t, &i.genre, |t, v| t.set_genre(v), |t| t.remove_genre());
    match i.annee.trim().parse::<u32>() {
        Ok(y) => t.set_year(y),
        Err(_) => t.remove_year(),
    }
    match i.piste.trim().parse::<u32>() {
        Ok(y) => t.set_track(y),
        Err(_) => t.remove_track(),
    }
    if let Some(img) = nouvelle_pochette {
        t.remove_picture_type(PictureType::CoverFront);
        let mime = if img.starts_with(&[0x89, b'P', b'N', b'G']) { MimeType::Png } else { MimeType::Jpeg };
        t.push_picture(Picture::new_unchecked(PictureType::CoverFront, Some(mime), None, img));
    }
    f.save_to_path(p, WriteOptions::default()).map_err(|e| e.to_string())
}

fn nom_fichier(p: &Path) -> String {
    p.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

/// « Artiste - Titre » → (artiste, titre)
pub fn deviner_depuis_nom(n: &str) -> Option<(String, String)> {
    let n = regex::Regex::new(r"^\d{1,3}[ .\-_]+").unwrap().replace(n, "");
    let n = regex::Regex::new(r"(?i)\s*[\(\[][^\)\]]*(official|lyrics|audio|video|clip|hd)[^\)\]]*[\)\]]").unwrap().replace_all(&n, "").to_string();
    let (a, t) = n.split_once(" - ")?;
    Some((a.trim().to_string(), t.trim().to_string()))
}

/// Recherche iTunes : (artiste, titre, album, année, genre, url de la pochette 600 px).
fn chercher_en_ligne(q: &str) -> Result<Vec<(String, String, String, String, String, String)>, String> {
    let j: serde_json::Value = ureq::get("https://itunes.apple.com/search").timeout(Duration::from_secs(15)).query("term", q).query("entity", "song").query("limit", "8").query("country", "fr")
        .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(j["results"].as_array().into_iter().flatten().map(|r| {
        let s = |k: &str| r[k].as_str().unwrap_or("").to_string();
        (s("artistName"), s("trackName"), s("collectionName"), s("releaseDate").get(..4).unwrap_or("").to_string(), s("primaryGenreName"), s("artworkUrl100").replace("100x100", "600x600"))
    }).collect())
}

struct Lecture {
    _flux: rodio::OutputStream,
    sink: rodio::Sink,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
struct ReglageMusique {
    dossiers: Vec<String>,
    playlists: BTreeMap<String, Vec<String>>,
    volume: f32,
}

pub struct Musique {
    r: ReglageMusique,
    biblio: Arc<Mutex<Option<Vec<(PathBuf, Infos)>>>>,
    file: Vec<PathBuf>,
    pos: usize,
    lecture: Option<Lecture>,
    infos: Option<Infos>,
    pochette: Option<(PathBuf, egui::TextureHandle)>,
    filtre: String,
    vue: Option<String>, // None : bibliothèque ; Some(nom) : playlist
    nouvelle_pl: String,
    aleatoire: bool,
    erreur: String,
    // édition des infos (32)
    edition: Option<(PathBuf, Infos, Option<Vec<u8>>)>,
    proposes: Arc<Mutex<Option<Result<Vec<(String, String, String, String, String, String)>, String>>>>,
    e: Etat,
}

impl Musique {
    pub fn new() -> Self {
        let mut r: ReglageMusique = charger("musique");
        if r.dossiers.is_empty() {
            r.dossiers.push(PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Music").display().to_string());
        }
        if r.volume == 0.0 {
            r.volume = 0.7;
        }
        Musique { r, biblio: Arc::default(), file: vec![], pos: 0, lecture: None, infos: None, pochette: None, filtre: String::new(), vue: None, nouvelle_pl: String::new(), aleatoire: false, erreur: String::new(), edition: None, proposes: Arc::default(), e: Arc::default() }
    }

    fn scanner(&self) {
        let (b, d) = (self.biblio.clone(), self.r.dossiers.clone());
        *b.lock().unwrap() = Some(vec![]);
        std::thread::spawn(move || {
            let mut v = vec![];
            for x in d {
                musiques_de(Path::new(&x), &mut v);
            }
            v.sort();
            let l: Vec<(PathBuf, Infos)> = v.into_iter().map(|p| {
                let mut i = lire_infos(&p);
                i.pochette = None; // chargée à la lecture seulement
                (p, i)
            }).collect();
            *b.lock().unwrap() = Some(l);
        });
    }

    fn jouer(&mut self, k: usize) {
        let Some(p) = self.file.get(k).cloned() else { return };
        self.pos = k;
        self.erreur.clear();
        let r = (|| -> Result<Lecture, String> {
            let (flux, h) = rodio::OutputStream::try_default().map_err(|e| format!("sortie audio : {e}"))?;
            let sink = rodio::Sink::try_new(&h).map_err(|e| e.to_string())?;
            let f = std::fs::File::open(&p).map_err(|e| e.to_string())?;
            let dec = rodio::Decoder::new(std::io::BufReader::new(f)).map_err(|e| format!("format non lu : {e}"))?;
            sink.append(dec);
            sink.set_volume(self.r.volume);
            Ok(Lecture { _flux: flux, sink })
        })();
        match r {
            Ok(l) => {
                self.lecture = Some(l);
                self.infos = Some(lire_infos(&p));
                self.pochette = None;
            }
            Err(e) => {
                self.lecture = None;
                self.erreur = format!("{} : {e}", nom_fichier(&p));
            }
        }
    }

    fn suivante(&mut self) {
        if self.file.is_empty() {
            return;
        }
        let k = if self.aleatoire { crate::outil::hasard(self.file.len() as u64) as usize } else { (self.pos + 1) % self.file.len() };
        self.jouer(k);
    }

    fn ui_edition(&mut self, ui: &mut egui::Ui) {
        let Some((p, i, nv)) = &mut self.edition else { return };
        ui.heading("✏ Infos de la musique");
        ui.weak(p.display().to_string());
        egui::Grid::new("infos_musique").num_columns(2).show(ui, |ui| {
            for (n, v) in [("Titre", &mut i.titre), ("Artiste", &mut i.artiste), ("Album", &mut i.album), ("Année", &mut i.annee), ("Genre", &mut i.genre), ("Piste", &mut i.piste)] {
                ui.label(n);
                ui.add(egui::TextEdit::singleline(v).desired_width(320.0));
                ui.end_row();
            }
        });
        ui.horizontal(|ui| {
            if ui.button("🖼 Pochette…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("Image", &["jpg", "jpeg", "png"]).pick_file() {
                    *nv = std::fs::read(f).ok();
                }
            }
            if nv.is_some() {
                ui.label("✔ nouvelle pochette");
            }
            if ui.button("🔍 Trouver les infos en ligne").clicked() {
                let q = if !i.artiste.is_empty() { format!("{} {}", i.artiste, i.titre) } else { deviner_depuis_nom(&nom_fichier(p)).map(|(a, t)| format!("{a} {t}")).unwrap_or(i.titre.clone()) };
                let pr = self.proposes.clone();
                *pr.lock().unwrap() = None;
                std::thread::spawn(move || *pr.lock().unwrap() = Some(chercher_en_ligne(&q)));
            }
        });
        let props = self.proposes.lock().unwrap().clone();
        let mut choisi = None;
        if let Some(Ok(v)) = &props {
            for (k, (a, t, al, an, g, _)) in v.iter().enumerate() {
                if ui.small_button(format!("{a} — {t} ({al}, {an}, {g})")).clicked() {
                    choisi = Some(k);
                }
            }
        } else if let Some(Err(e)) = &props {
            ui.weak(e);
        }
        if let (Some(k), Some(Ok(v))) = (choisi, props) {
            let (a, t, al, an, g, img) = v[k].clone();
            (i.artiste, i.titre, i.album, i.annee, i.genre) = (a, t, al, an, g);
            let n = self.nouvelle_pochette_depuis(&img);
            if let Some((_, _, nv)) = &mut self.edition {
                *nv = n;
            }
            *self.proposes.lock().unwrap() = None;
            return;
        }
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("💾 Enregistrer")).clicked() {
                let (p, i, nv) = self.edition.clone().unwrap();
                let b = self.biblio.clone();
                en_fond(&self.e, move || {
                    ecrire_infos(&p, &i, nv)?;
                    if let Some(l) = b.lock().unwrap().as_mut() {
                        if let Some(x) = l.iter_mut().find(|x| x.0 == p) {
                            x.1 = Infos { pochette: None, ..i };
                        }
                    }
                    Ok("✔ Infos enregistrées.".into())
                });
            }
            if ui.button("Fermer").clicked() {
                self.edition = None;
            }
        });
    }

    fn nouvelle_pochette_depuis(&self, url: &str) -> Option<Vec<u8>> {
        let mut b = vec![];
        ureq::get(url).timeout(Duration::from_secs(10)).call().ok()?.into_reader().read_to_end(&mut b).ok()?;
        Some(b)
    }
}

fn mm_ss(s: u64) -> String {
    format!("{}:{:02}", s / 60, s % 60)
}

impl Outil for Musique {
    fn nom(&self) -> &'static str {
        "🎵 Lecteur de musique"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn resume(&self) -> Option<String> {
        let l = self.lecture.as_ref()?;
        if l.sink.is_paused() || l.sink.empty() {
            return None;
        }
        let i = self.infos.as_ref()?;
        Some(format!("🎵 {}{}", i.titre, if i.artiste.is_empty() { String::new() } else { format!(" — {}", i.artiste) }))
    }
    fn tic(&mut self, ctx: &egui::Context) {
        // enchaîner les morceaux, même quand l'onglet n'est pas affiché
        if self.lecture.as_ref().is_some_and(|l| l.sink.empty()) {
            self.suivante();
            ctx.request_repaint();
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.edition.is_some() {
            self.ui_edition(ui);
            return;
        }
        ui.heading("Lecteur de musique");
        if self.biblio.lock().unwrap().is_none() {
            self.scanner();
        }
        // ---- lecture en cours ----
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(i) = &self.infos {
                    let p = self.file.get(self.pos).cloned().unwrap_or_default();
                    if self.pochette.as_ref().is_none_or(|x| x.0 != p) {
                        if let Some(img) = i.pochette.as_ref().and_then(|b| image::load_from_memory(b).ok()) {
                            let t = img.thumbnail(160, 160).to_rgba8();
                            let tex = ui.ctx().load_texture("pochette", egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default());
                            self.pochette = Some((p, tex));
                        }
                    }
                    match &self.pochette {
                        Some((_, t)) => {
                            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(80.0, 80.0)));
                        }
                        None => {
                            ui.label(RichText::new("🎵").size(56.0));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&i.titre).size(18.0).strong());
                        ui.label(format!("{}{}", i.artiste, if i.album.is_empty() { String::new() } else { format!(" · {}", i.album) }));
                        if let Some(l) = &self.lecture {
                            let pos = l.sink.get_pos().as_secs();
                            let mut f = if i.duree > 0 { pos as f32 / i.duree as f32 } else { 0.0 };
                            ui.horizontal(|ui| {
                                ui.label(mm_ss(pos));
                                if ui.add(egui::Slider::new(&mut f, 0.0..=1.0).show_value(false)).changed() && i.duree > 0 {
                                    let _ = l.sink.try_seek(Duration::from_secs_f32(f * i.duree as f32));
                                }
                                ui.label(mm_ss(i.duree));
                            });
                        }
                    });
                } else {
                    ui.weak("Choisis une musique ci-dessous.");
                }
            });
            ui.horizontal(|ui| {
                if ui.button("⏮").clicked() && !self.file.is_empty() {
                    let k = (self.pos + self.file.len() - 1) % self.file.len();
                    self.jouer(k);
                }
                let en_pause = self.lecture.as_ref().is_none_or(|l| l.sink.is_paused());
                if ui.button(RichText::new(if en_pause { "▶" } else { "⏸" }).size(18.0)).clicked() {
                    match &self.lecture {
                        Some(l) if en_pause => l.sink.play(),
                        Some(l) => l.sink.pause(),
                        None => self.jouer(self.pos),
                    }
                }
                if ui.button("⏭").clicked() {
                    self.suivante();
                }
                ui.toggle_value(&mut self.aleatoire, "🔀");
                ui.label("🔊");
                if ui.add(egui::Slider::new(&mut self.r.volume, 0.0..=1.0).show_value(false)).changed() {
                    if let Some(l) = &self.lecture {
                        l.sink.set_volume(self.r.volume);
                    }
                    enregistrer("musique", &self.r);
                }
            });
            if !self.erreur.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, &self.erreur);
            }
        });
        if self.lecture.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        // ---- bibliothèque et playlists ----
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.vue.is_none(), "📚 Bibliothèque").clicked() {
                self.vue = None;
            }
            for n in self.r.playlists.keys().cloned().collect::<Vec<_>>() {
                let b = ui.selectable_label(self.vue.as_ref() == Some(&n), format!("📃 {n}"));
                if b.clicked() {
                    self.vue = Some(n.clone());
                }
                b.context_menu(|ui| {
                    if ui.button("🗑 Supprimer la playlist").clicked() {
                        self.r.playlists.remove(&n);
                        enregistrer("musique", &self.r);
                        self.vue = None;
                        ui.close_menu();
                    }
                });
            }
            ui.add(egui::TextEdit::singleline(&mut self.nouvelle_pl).hint_text("nouvelle playlist").desired_width(120.0));
            if ui.small_button("➕").clicked() && !self.nouvelle_pl.trim().is_empty() {
                self.r.playlists.entry(std::mem::take(&mut self.nouvelle_pl).trim().to_string()).or_default();
                enregistrer("musique", &self.r);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 titre, artiste, album").desired_width(240.0));
            if ui.small_button("📁 Ajouter un dossier").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.r.dossiers.push(d.display().to_string());
                    enregistrer("musique", &self.r);
                    self.scanner();
                }
            }
            if ui.small_button("⟳").clicked() {
                self.scanner();
            }
        });
        let biblio = self.biblio.lock().unwrap().clone().unwrap_or_default();
        let q = self.filtre.to_lowercase();
        let liste: Vec<(PathBuf, Infos)> = match &self.vue {
            None => biblio.clone(),
            Some(n) => self.r.playlists.get(n).map(|v| v.iter().map(|s| {
                let p = PathBuf::from(s);
                let i = biblio.iter().find(|x| x.0 == p).map(|x| x.1.clone()).unwrap_or_else(|| Infos { titre: nom_fichier(&p), ..Default::default() });
                (p, i)
            }).collect()).unwrap_or_default(),
        };
        let liste: Vec<(PathBuf, Infos)> = liste.into_iter().filter(|(_, i)| q.is_empty() || format!("{} {} {}", i.titre, i.artiste, i.album).to_lowercase().contains(&q)).collect();
        if liste.is_empty() {
            ui.weak(if biblio.is_empty() { "Aucune musique trouvée dans tes dossiers (Musique par défaut)." } else { "Rien ici." });
        }
        let actuelle = self.file.get(self.pos).cloned();
        let (mut jouer, mut editer, mut ajouter, mut retirer) = (None, None, None, None);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("pistes").striped(true).num_columns(4).show(ui, |ui| {
                for (k, (p, i)) in liste.iter().enumerate() {
                    let en_cours = actuelle.as_ref() == Some(p);
                    let r = ui.selectable_label(en_cours, if en_cours { format!("▶ {}", i.titre) } else { i.titre.clone() });
                    if r.double_clicked() || r.clicked() {
                        jouer = Some(k);
                    }
                    r.context_menu(|ui| {
                        if ui.button("✏ Modifier les infos").clicked() {
                            editer = Some(p.clone());
                            ui.close_menu();
                        }
                        for n in self.r.playlists.keys() {
                            if ui.button(format!("➕ Ajouter à « {n} »")).clicked() {
                                ajouter = Some((n.clone(), p.clone()));
                                ui.close_menu();
                            }
                        }
                        if self.vue.is_some() && ui.button("✖ Retirer de la playlist").clicked() {
                            retirer = Some(p.clone());
                            ui.close_menu();
                        }
                        if ui.button("📂 Afficher le fichier").clicked() {
                            let _ = std::process::Command::new("explorer").arg(format!("/select,{}", p.display())).spawn();
                            ui.close_menu();
                        }
                    });
                    ui.label(&i.artiste);
                    ui.weak(&i.album);
                    ui.weak(if i.duree > 0 { mm_ss(i.duree) } else { String::new() });
                    ui.end_row();
                }
            });
        });
        ui.weak("Clic : écouter · clic droit : playlist, modifier les infos et la pochette");
        if let Some(k) = jouer {
            self.file = liste.iter().map(|x| x.0.clone()).collect();
            self.jouer(k);
        }
        if let Some(p) = editer {
            let i = lire_infos(&p);
            self.edition = Some((p, Infos { pochette: None, ..i }, None));
            *self.proposes.lock().unwrap() = None;
        }
        if let Some((n, p)) = ajouter {
            self.r.playlists.entry(n).or_default().push(p.display().to_string());
            enregistrer("musique", &self.r);
        }
        if let (Some(p), Some(n)) = (retirer, self.vue.clone()) {
            if let Some(v) = self.r.playlists.get_mut(&n) {
                v.retain(|x| Path::new(x) != p);
            }
            enregistrer("musique", &self.r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(sous_dossier("2024-05-03 14.22.10", None), PathBuf::from("2024").join("05 mai"));
        assert_eq!(sous_dossier("2024-12-01", Some("Lyon")), PathBuf::from("2024").join("12 décembre - Lyon"));
        assert_eq!(reduire(4000, 3000, 1920), (1920, 1440));
        assert_eq!(reduire(1000, 3000, 1500), (500, 1500));
        assert_eq!(reduire(800, 600, 1920), (800, 600));
        assert_eq!(nom_sur("../a:b?.txt"), "_a_b_.txt");
        assert_eq!(nom_sur("   "), "fichier");
        let e = Entete { de: "PC".into(), nom: "a.txt".into(), taille: 3 };
        assert_eq!(serde_json::from_str::<Entete>(&serde_json::to_string(&e).unwrap()).unwrap(), e);
        let (d, f) = multipart("X", &[("time", "1h")], "f", "a\"b.txt");
        let d = String::from_utf8(d).unwrap();
        assert!(d.contains("name=\"time\"\r\n\r\n1h\r\n") && d.contains("filename=\"ab.txt\""));
        assert_eq!(f, b"\r\n--X--\r\n");
        assert_eq!(deviner_depuis_nom("01 - Daft Punk - One More Time (Official Video)"), Some(("Daft Punk".into(), "One More Time".into())));
        assert_eq!(deviner_depuis_nom("rien"), None);
    }

    #[test]
    fn comparaison() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let mut a = Inventaire::new();
        let mut b = Inventaire::new();
        a.insert("x".into(), (1, t));
        a.insert("y".into(), (1, t + Duration::from_secs(10)));
        a.insert("z".into(), (1, t));
        b.insert("y".into(), (1, t));
        b.insert("z".into(), (1, t + Duration::from_secs(1)));
        b.insert("w".into(), (2, t));
        assert_eq!(comparer(&a, &b), vec![("w".into(), Diff::SeulementB), ("x".into(), Diff::SeulementA), ("y".into(), Diff::PlusRecentA), ("z".into(), Diff::Identique)]);
    }
}
