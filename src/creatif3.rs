//! Créatif, suite : zone de l'écran en GIF (57), miniatures YouTube (58), éditeur de sous-titres (59),
//! boîte à sons dans le micro (60), pixel art (61), métronome et boîte à rythmes (63).
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn texture(ctx: &egui::Context, nom: &str, img: &RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(nom, egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), Default::default())
}

// ---------------------------------------------------------------------------------------------------
// 57 · Zone de l'écran en GIF
// ---------------------------------------------------------------------------------------------------

/// Taille des images du GIF (le plus grand côté limité à `max`, dimensions paires).
pub fn taille_gif(l: u32, h: u32, max: u32) -> (u32, u32) {
    let (a, b) = if l.max(h) <= max { (l, h) } else if l >= h { (max, (h as u64 * max as u64 / l as u64) as u32) } else { ((l as u64 * max as u64 / h as u64) as u32, max) };
    (a.max(2) & !1, b.max(2) & !1)
}

#[derive(Default)]
struct Enregistrement {
    en_cours: bool,
    images: usize,
    debut: Option<Instant>,
    message: String,
    fichier: Option<PathBuf>,
}

pub struct ZoneGif {
    fps: u32,
    max: u32,
    duree: u32,
    e: Arc<Mutex<Enregistrement>>,
    stop: Arc<AtomicBool>,
}

impl ZoneGif {
    pub fn new() -> Self {
        ZoneGif { fps: 12, max: 640, duree: 15, e: Arc::default(), stop: Arc::default() }
    }

    fn lancer(&self, ctx: egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        self.stop.store(false, Relaxed);
        *self.e.lock().unwrap() = Enregistrement { en_cours: true, message: "Choisis la zone…".into(), ..Default::default() };
        let (e, stop, fps, max, duree) = (self.e.clone(), self.stop.clone(), self.fps, self.max, self.duree);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            let fin = |m: String, f: Option<PathBuf>| {
                let mut x = e.lock().unwrap();
                x.en_cours = false;
                x.message = m;
                x.fichier = f;
            };
            let Some(ecran) = crate::capture::win::capturer() else { return fin("capture impossible".into(), None) };
            let (ox, oy) = (ecran.x, ecran.y);
            let (r, _) = crate::capture::win::surimpression(crate::capture::win::Mode::Zone, ecran);
            let crate::capture::win::Retour::Zone(x, y, l, h) = r else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                return fin("Annulé.".into(), None);
            };
            let (gl, gh) = taille_gif(l as u32, h as u32, max);
            {
                let mut z = e.lock().unwrap();
                z.message = "● Enregistrement… Ctrl+Alt+G pour arrêter".into();
                z.debut = Some(Instant::now());
            }
            crate::systeme::notifier("GIF", "Enregistrement en cours : Ctrl+Alt+G pour arrêter.");
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let raccourci = unsafe { crate::outil::raccourci(std::ptr::null_mut(), 77, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'G' as u32) != 0 };
            let pas = Duration::from_secs_f64(1.0 / fps as f64);
            let t0 = Instant::now();
            let mut images: Vec<RgbaImage> = vec![];
            let mut prochain = Instant::now();
            while !stop.load(Relaxed) && t0.elapsed() < Duration::from_secs(duree as u64) {
                unsafe {
                    let mut m: MSG = std::mem::zeroed();
                    if PeekMessageW(&mut m, std::ptr::null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE) != 0 {
                        break;
                    }
                }
                if let Some(img) = crate::capture::win::capturer_zone(x + ox, y + oy, l, h) {
                    let img = if (gl, gh) != (img.width(), img.height()) { image::imageops::resize(&img, gl, gh, image::imageops::FilterType::Triangle) } else { img };
                    images.push(img);
                    e.lock().unwrap().images = images.len();
                }
                prochain += pas;
                if let Some(d) = prochain.checked_duration_since(Instant::now()) {
                    std::thread::sleep(d);
                }
            }
            if raccourci {
                unsafe { UnregisterHotKey(std::ptr::null_mut(), 77) };
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            e.lock().unwrap().message = format!("Création du GIF ({} images)…", images.len());
            ctx.request_repaint();
            let dossier = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("GIF");
            let _ = std::fs::create_dir_all(&dossier);
            let f = dossier.join(format!("GIF {}.gif", crate::minecraft::horodatage()));
            let r = (|| -> Result<(), String> {
                let fichier = std::io::BufWriter::new(std::fs::File::create(&f).map_err(|e| e.to_string())?);
                let mut enc = image::codecs::gif::GifEncoder::new_with_speed(fichier, 10);
                enc.set_repeat(image::codecs::gif::Repeat::Infinite).map_err(|e| e.to_string())?;
                let delai = image::Delay::from_numer_denom_ms(1000, fps);
                for img in images {
                    enc.encode_frame(image::Frame::from_parts(img, 0, 0, delai)).map_err(|e| e.to_string())?;
                }
                Ok(())
            })();
            match r {
                Ok(()) => {
                    let t = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    fin(format!("✔ GIF prêt ({})", crate::disque::taille_lisible(t)), Some(f));
                }
                Err(m) => fin(format!("✖ {m}"), None),
            }
            ctx.request_repaint();
        });
    }
}

impl Outil for ZoneGif {
    fn nom(&self) -> &'static str {
        "🎞 Zone de l'écran en GIF"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Enregistrer une zone de l'écran en GIF");
        ui.weak("Sélectionne une zone, Outils filme jusqu'à Ctrl+Alt+G (ou la durée max), puis crée le GIF : parfait pour montrer un bug, une astuce, une action en jeu sur Discord.");
        let x = self.e.lock().unwrap();
        let (en_cours, msg, f, n, d) = (x.en_cours, x.message.clone(), x.fichier.clone(), x.images, x.debut);
        drop(x);
        ui.add_enabled_ui(!en_cours, |ui| {
            ui.horizontal(|ui| {
                ui.label("Images / s");
                ui.add(egui::DragValue::new(&mut self.fps).range(5..=30));
                ui.label("Largeur max");
                egui::ComboBox::from_id_salt("gif_max").selected_text(format!("{} px", self.max)).show_ui(ui, |ui| {
                    for m in [320, 480, 640, 800, 1024] {
                        ui.selectable_value(&mut self.max, m, format!("{m} px"));
                    }
                });
                ui.label("Durée max");
                ui.add(egui::DragValue::new(&mut self.duree).range(2..=60).suffix(" s"));
            });
            if ui.button(RichText::new("✂ Choisir la zone et enregistrer").size(16.0)).clicked() {
                self.lancer(ui.ctx().clone());
            }
        });
        if en_cours {
            if let Some(d) = d {
                ui.label(RichText::new(format!("● {:.1} s · {n} images", d.elapsed().as_secs_f32())).color(Color32::LIGHT_RED).size(18.0));
            }
            if ui.button("⏹ Arrêter").clicked() {
                self.stop.store(true, Relaxed);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        ui.label(msg);
        if let Some(f) = f {
            ui.horizontal(|ui| {
                if ui.button("📂 Ouvrir").clicked() {
                    ouvrir(&f);
                }
                if ui.button("📋 Copier le chemin").clicked() {
                    ui.ctx().copy_text(f.display().to_string());
                }
                ui.weak(f.display().to_string());
            });
            ui.weak("Discord limite les fichiers à 10 Mo : baisse la largeur ou les images/s si le GIF est trop lourd.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 58 · Miniatures YouTube
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Miniature {
    pub titre: String,
    pub taille: f32,
    pub couleur: [u8; 3],
    pub contour: [u8; 3],
    pub position: u8, // 0 gauche, 1 centre, 2 droite
    pub assombrir: f32,
    pub bande: bool,
    pub perso_echelle: f32,
    pub perso_droite: bool,
}

impl Default for Miniature {
    fn default() -> Self {
        Miniature { titre: "MON TITRE\nINCROYABLE".into(), taille: 110.0, couleur: [255, 230, 40], contour: [0, 0, 0], position: 0, assombrir: 0.25, bande: true, perso_echelle: 0.9, perso_droite: true }
    }
}

pub const ML: u32 = 1280;
pub const MH: u32 = 720;

/// Image couvrant tout le cadre (rognée au centre).
pub fn couvrir(img: &RgbaImage, l: u32, h: u32) -> RgbaImage {
    let e = (l as f32 / img.width() as f32).max(h as f32 / img.height() as f32);
    let (nl, nh) = (((img.width() as f32 * e).ceil() as u32).max(l), ((img.height() as f32 * e).ceil() as u32).max(h));
    let r = image::imageops::resize(img, nl, nh, image::imageops::FilterType::Triangle);
    image::imageops::crop_imm(&r, (nl - l) / 2, (nh - h) / 2, l, h).to_image()
}

pub fn composer(m: &Miniature, fond: Option<&RgbaImage>, perso: Option<&RgbaImage>, police: Option<&ab_glyph::FontVec>) -> RgbaImage {
    let mut c = match fond {
        Some(f) => couvrir(f, ML, MH),
        None => RgbaImage::from_fn(ML, MH, |x, y| {
            let t = (x as f32 / ML as f32 + y as f32 / MH as f32) / 2.0;
            Rgba([(40.0 + 150.0 * t) as u8, 20, (90.0 + 60.0 * (1.0 - t)) as u8, 255])
        }),
    };
    if m.assombrir > 0.0 {
        for p in c.pixels_mut() {
            for k in 0..3 {
                p.0[k] = (p.0[k] as f32 * (1.0 - m.assombrir)) as u8;
            }
        }
    }
    if let Some(p) = perso {
        let h = (MH as f32 * m.perso_echelle) as u32;
        let l = (p.width() as f32 * h as f32 / p.height() as f32) as u32;
        let r = image::imageops::resize(p, l.max(1), h.max(1), image::imageops::FilterType::Triangle);
        let x = if m.perso_droite { ML as i64 - l as i64 - 20 } else { 20 };
        image::imageops::overlay(&mut c, &r, x, (MH - h) as i64);
    }
    if m.bande {
        for y in MH - 18..MH {
            for x in 0..ML {
                c.put_pixel(x, y, Rgba([230, 30, 40, 255]));
            }
        }
    }
    if let Some(f) = police {
        let lignes: Vec<&str> = m.titre.lines().filter(|l| !l.trim().is_empty()).collect();
        let n = lignes.len().max(1) as f32;
        let hl = m.taille * 1.05;
        let y0 = (MH as f32 - hl * n) / 2.0;
        for (k, l) in lignes.iter().enumerate() {
            let lt = crate::creatif::largeur(f, m.taille, l);
            let x = match m.position {
                0 => 50.0,
                1 => (ML as f32 - lt) / 2.0,
                _ => ML as f32 - lt - 50.0,
            };
            crate::creatif::ecrire(&mut c, f, m.taille, x, y0 + k as f32 * hl, l, m.couleur, Some((m.contour, (m.taille / 14.0).max(3.0) as i32)));
        }
    }
    c
}

pub struct Miniatures {
    m: Miniature,
    fond: Option<RgbaImage>,
    perso: Option<RgbaImage>,
    police: Option<ab_glyph::FontVec>,
    apercu: Option<egui::TextureHandle>,
    sale: bool,
    message: String,
}

impl Miniatures {
    pub fn new() -> Self {
        Miniatures { m: charger("miniature"), fond: None, perso: None, police: None, apercu: None, sale: true, message: String::new() }
    }
}

fn ouvrir_image() -> Option<RgbaImage> {
    let p = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file()?;
    image::open(p).ok().map(|i| i.to_rgba8())
}

impl Outil for Miniatures {
    fn nom(&self) -> &'static str {
        "🖼 Miniatures YouTube"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Miniatures YouTube");
        if self.police.is_none() {
            self.police = crate::creatif::police(&["impact.ttf", "arialbd.ttf", "segoeuib.ttf"]);
        }
        let avant = self.m.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("🖼 Image de fond…").clicked() {
                if let Some(i) = ouvrir_image() {
                    self.fond = Some(i);
                    self.sale = true;
                }
            }
            if ui.button("🧍 Personnage / objet (PNG détouré)…").clicked() {
                if let Some(i) = ouvrir_image() {
                    self.perso = Some(i);
                    self.sale = true;
                }
            }
            if self.perso.is_some() && ui.small_button("✖ perso").clicked() {
                self.perso = None;
                self.sale = true;
            }
        });
        ui.add(egui::TextEdit::multiline(&mut self.m.titre).desired_rows(2).desired_width(400.0).font(egui::TextStyle::Heading));
        ui.horizontal_wrapped(|ui| {
            ui.label("Taille");
            ui.add(egui::Slider::new(&mut self.m.taille, 40.0..=200.0).show_value(false));
            ui.color_edit_button_srgb(&mut self.m.couleur);
            ui.label("contour");
            ui.color_edit_button_srgb(&mut self.m.contour);
            for (k, n) in ["⬅", "⬌", "➡"].iter().enumerate() {
                ui.selectable_value(&mut self.m.position, k as u8, *n);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Assombrir le fond");
            ui.add(egui::Slider::new(&mut self.m.assombrir, 0.0..=0.8).show_value(false));
            ui.checkbox(&mut self.m.bande, "Bande rouge");
            if self.perso.is_some() {
                ui.label("Perso");
                ui.add(egui::Slider::new(&mut self.m.perso_echelle, 0.3..=1.2).show_value(false));
                ui.checkbox(&mut self.m.perso_droite, "à droite");
            }
        });
        if self.m != avant {
            enregistrer("miniature", &self.m);
            self.sale = true;
        }
        let img = || composer(&self.m, self.fond.as_ref(), self.perso.as_ref(), self.police.as_ref());
        if self.sale {
            self.sale = false;
            let i = image::imageops::thumbnail(&img(), 640, 360);
            self.apercu = Some(texture(ui.ctx(), "miniature", &i));
        }
        if let Some(t) = &self.apercu {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(640.0, 360.0)));
        }
        ui.horizontal(|ui| {
            if ui.button("💾 Enregistrer (1280×720)…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).add_filter("JPG", &["jpg"]).set_file_name("miniature.png").save_file() {
                    self.message = img().save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("📋 Copier").clicked() {
                crate::capture::win::copier_image(&img());
                self.message = "Copié.".into();
            }
        });
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 59 · Éditeur de sous-titres
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Ligne {
    pub debut: i64, // ms
    pub fin: i64,
    pub texte: String,
}

pub fn lire_temps(s: &str) -> Option<i64> {
    let s = s.trim().replace(',', ".");
    let (hms, ms) = s.split_once('.').unwrap_or((&s, "0"));
    let p: Vec<i64> = hms.split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let ms: i64 = format!("{ms:0<3}")[..3].parse().ok()?;
    match p.as_slice() {
        [h, m, s] => Some(((h * 60 + m) * 60 + s) * 1000 + ms),
        [m, s] => Some((m * 60 + s) * 1000 + ms),
        _ => None,
    }
}

pub fn ecrire_temps(t: i64) -> String {
    let t = t.max(0);
    format!("{:02}:{:02}:{:02},{:03}", t / 3_600_000, t / 60_000 % 60, t / 1000 % 60, t % 1000)
}

pub fn lire_srt(t: &str) -> Vec<Ligne> {
    let t = t.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    t.split("\n\n").filter_map(|b| {
        let mut l = b.trim().lines();
        let mut x = l.next()?;
        if !x.contains("-->") {
            x = l.next()?;
        }
        let (a, z) = x.split_once("-->")?;
        Some(Ligne { debut: lire_temps(a)?, fin: lire_temps(z.split_whitespace().next()?)?, texte: l.collect::<Vec<_>>().join("\n") })
    }).collect()
}

pub fn ecrire_srt(v: &[Ligne]) -> String {
    v.iter().enumerate().map(|(k, l)| format!("{}\n{} --> {}\n{}\n", k + 1, ecrire_temps(l.debut), ecrire_temps(l.fin), l.texte)).collect::<Vec<_>>().join("\n")
}

/// Change la cadence (ex. 23,976 → 25 i/s) : t × de / vers.
pub fn recadencer(v: &mut [Ligne], de: f64, vers: f64) {
    for l in v {
        l.debut = (l.debut as f64 * de / vers).round() as i64;
        l.fin = (l.fin as f64 * de / vers).round() as i64;
    }
}

pub struct SousTitres {
    fichier: Option<PathBuf>,
    l: Vec<Ligne>,
    decalage: i64,
    chercher: String,
    remplacer: String,
    message: String,
    modifie: bool,
}

impl SousTitres {
    pub fn new() -> Self {
        SousTitres { fichier: None, l: vec![], decalage: 0, chercher: String::new(), remplacer: String::new(), message: String::new(), modifie: false }
    }

    fn ouvrir(&mut self, p: PathBuf) {
        let b = std::fs::read(&p).unwrap_or_default();
        let t = String::from_utf8(b.clone()).unwrap_or_else(|_| b.iter().map(|c| *c as char).collect()); // anciens fichiers en Latin-1
        self.l = lire_srt(&t);
        self.message = format!("{} sous-titres", self.l.len());
        self.fichier = Some(p);
        self.modifie = false;
    }
}

impl Outil for SousTitres {
    fn nom(&self) -> &'static str {
        "💬 Éditeur de sous-titres"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éditeur de sous-titres (.srt)");
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.ouvrir(p);
        }
        ui.horizontal(|ui| {
            if ui.button("📂 Ouvrir un .srt…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt"]).pick_file() {
                    self.ouvrir(p);
                }
            }
            if let Some(f) = self.fichier.clone() {
                if ui.add_enabled(self.modifie, egui::Button::new("💾 Enregistrer")).clicked() {
                    let _ = std::fs::copy(&f, f.with_extension("srt.bak"));
                    self.message = std::fs::write(&f, ecrire_srt(&self.l)).map(|_| "✔ Enregistré (copie de l'ancien en .bak)".to_string()).unwrap_or_else(|e| e.to_string());
                    self.modifie = false;
                }
                if ui.button("Enregistrer sous…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Sous-titres", &["srt"]).set_file_name(f.file_name().unwrap_or_default().to_string_lossy()).save_file() {
                        self.message = std::fs::write(&p, ecrire_srt(&self.l)).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                    }
                }
                ui.weak(f.display().to_string());
            }
        });
        if self.l.is_empty() {
            ui.weak("Ouvre ou glisse un fichier .srt (sous-titres décalés, fautes, mots à remplacer…).");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Décaler tout de");
            ui.add(egui::DragValue::new(&mut self.decalage).speed(50).suffix(" ms"));
            if ui.button("Appliquer").clicked() && self.decalage != 0 {
                for l in &mut self.l {
                    l.debut += self.decalage;
                    l.fin += self.decalage;
                }
                self.message = format!("Décalé de {} s", self.decalage as f64 / 1000.0);
                self.decalage = 0;
                self.modifie = true;
            }
            ui.separator();
            ui.label("Cadence");
            for (n, de, vers) in [("23,976 → 25", 25.0, 23.976), ("25 → 23,976", 23.976, 25.0)] {
                if ui.small_button(n).on_hover_text("Quand les sous-titres dérivent petit à petit").clicked() {
                    recadencer(&mut self.l, de, vers);
                    self.modifie = true;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.chercher).hint_text("chercher").desired_width(140.0));
            ui.add(egui::TextEdit::singleline(&mut self.remplacer).hint_text("remplacer par").desired_width(140.0));
            if ui.button("Tout remplacer").clicked() && !self.chercher.is_empty() {
                let n: usize = self.l.iter().map(|l| l.texte.matches(&self.chercher).count()).sum();
                for l in &mut self.l {
                    l.texte = l.texte.replace(&self.chercher, &self.remplacer);
                }
                self.message = format!("{n} remplacement(s)");
                self.modifie |= n > 0;
            }
        });
        ui.label(&self.message);
        let mut retirer = None;
        egui::ScrollArea::vertical().show_rows(ui, 52.0, self.l.len(), |ui, lignes| {
            for k in lignes {
                let l = &mut self.l[k];
                ui.horizontal(|ui| {
                    ui.weak(format!("{:>4}", k + 1));
                    ui.vertical(|ui| {
                        let mut d = ecrire_temps(l.debut);
                        let mut f = ecrire_temps(l.fin);
                        if ui.add(egui::TextEdit::singleline(&mut d).desired_width(95.0).font(egui::TextStyle::Monospace)).changed() {
                            if let Some(t) = lire_temps(&d) {
                                l.debut = t;
                                self.modifie = true;
                            }
                        }
                        if ui.add(egui::TextEdit::singleline(&mut f).desired_width(95.0).font(egui::TextStyle::Monospace)).changed() {
                            if let Some(t) = lire_temps(&f) {
                                l.fin = t;
                                self.modifie = true;
                            }
                        }
                    });
                    if ui.add(egui::TextEdit::multiline(&mut l.texte).desired_rows(2).desired_width(420.0)).changed() {
                        self.modifie = true;
                    }
                    if ui.small_button("🗑").clicked() {
                        retirer = Some(k);
                    }
                });
            }
        });
        if let Some(k) = retirer {
            self.l.remove(k);
            self.modifie = true;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 60 · Boîte à sons (jouée dans le micro)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Son {
    pub nom: String,
    pub fichier: String,
    pub volume: f32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageSons {
    pub sons: Vec<Son>,
    pub dans_micro: bool,
    pub ecouter: bool,
    pub raccourcis: bool,
}

impl Default for ReglageSons {
    fn default() -> Self {
        ReglageSons { sons: vec![], dans_micro: true, ecouter: true, raccourcis: true }
    }
}

type Echantillons = (u16, u32, Arc<Vec<f32>>);

fn decoder(p: &str) -> Option<Echantillons> {
    use rodio::Source;
    let f = std::fs::File::open(p).ok()?;
    let d = rodio::Decoder::new(std::io::BufReader::new(f)).ok()?;
    let (c, r) = (d.channels(), d.sample_rate());
    let v: Vec<f32> = d.convert_samples::<f32>().take(r as usize * c as usize * 30).collect(); // 30 s max
    Some((c, r, Arc::new(v)))
}

enum Ordre {
    Jouer(usize),
    Stop,
    Recharger(ReglageSons),
}

fn fil_sons(r: ReglageSons) -> Sender<Ordre> {
    let (tx, rx) = channel::<Ordre>();
    std::thread::spawn(move || {
        use rodio::cpal::traits::{DeviceTrait, HostTrait};
        let mut r = r;
        let cable = rodio::cpal::default_host().output_devices().ok().and_then(|mut d| d.find(|x| x.name().is_ok_and(|n| n.to_lowercase().contains("cable input"))));
        let micro = cable.as_ref().and_then(|d| rodio::OutputStream::try_from_device(d).ok());
        let casque = rodio::OutputStream::try_default().ok();
        let mut cache: Vec<Option<Echantillons>> = r.sons.iter().map(|s| decoder(&s.fichier)).collect();
        let mut sinks: Vec<rodio::Sink> = vec![];
        for o in rx {
            match o {
                Ordre::Recharger(n) => {
                    cache = n.sons.iter().map(|s| decoder(&s.fichier)).collect();
                    r = n;
                }
                Ordre::Stop => sinks.clear(),
                Ordre::Jouer(k) => {
                    let Some(Some((c, t, v))) = cache.get(k) else { continue };
                    let vol = r.sons.get(k).map(|s| s.volume).unwrap_or(1.0);
                    sinks.retain(|s| !s.empty());
                    let mut sorties = vec![];
                    if r.dans_micro {
                        sorties.extend(micro.as_ref().map(|x| &x.1));
                    }
                    if r.ecouter {
                        sorties.extend(casque.as_ref().map(|x| &x.1));
                    }
                    for h in sorties {
                        if let Ok(s) = rodio::Sink::try_new(h) {
                            s.set_volume(vol);
                            s.append(rodio::buffer::SamplesBuffer::new(*c, *t, (**v).clone()));
                            sinks.push(s);
                        }
                    }
                }
            }
        }
    });
    tx
}

pub struct BoiteSons {
    r: ReglageSons,
    tx: Sender<Ordre>,
    cable: bool,
}

impl BoiteSons {
    pub fn new() -> Self {
        let r: ReglageSons = charger("boite_sons");
        let tx = fil_sons(r.clone());
        let actif = r.raccourcis;
        // Ctrl+Maj+F1…F9 : jouer le son n, Ctrl+Maj+F12 : tout arrêter
        // (pas Ctrl+Alt+chiffre : sur un clavier AZERTY c'est AltGr, qui sert à taper @ # { [ | \)
        let t2 = tx.clone();
        if actif {
            std::thread::spawn(move || unsafe {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
                use windows_sys::Win32::UI::WindowsAndMessaging::*;
                for k in 1..10u32 {
                    crate::outil::raccourci(std::ptr::null_mut(), 300 + k as i32, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, VK_F1 as u32 + k - 1);
                }
                crate::outil::raccourci(std::ptr::null_mut(), 300, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, VK_F12 as u32);
                let mut m: MSG = std::mem::zeroed();
                while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                    if m.message == WM_HOTKEY {
                        let k = m.wParam as usize - 300;
                        let _ = t2.send(if k == 0 { Ordre::Stop } else { Ordre::Jouer(k - 1) });
                    }
                }
            });
        }
        BoiteSons { r, tx, cable: false }
    }
}

impl Outil for BoiteSons {
    fn nom(&self) -> &'static str {
        "🔊 Boîte à sons"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Boîte à sons");
        ui.weak("Des sons à envoyer dans le micro (Discord, jeu) d'un clic ou avec Ctrl+Maj+F1…F9 de n'importe où. Ctrl+Maj+F12 arrête tout.");
        if !self.cable {
            use rodio::cpal::traits::{DeviceTrait, HostTrait};
            self.cable = rodio::cpal::default_host().output_devices().ok().is_some_and(|mut d| d.any(|x| x.name().is_ok_and(|n| n.to_lowercase().contains("cable input"))));
            if !self.cable {
                ui.colored_label(Color32::from_rgb(250, 210, 90), "Pour que les autres entendent les sons, installe VB-CABLE (bouton dans « 🎤 Micro : anti-bruit et voix ») et active le micro traité.");
            }
        }
        let avant = self.r.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.r.dans_micro, "Dans le micro (CABLE)");
            ui.checkbox(&mut self.r.ecouter, "Dans mon casque");
            ui.checkbox(&mut self.r.raccourcis, "Raccourcis Ctrl+Maj+F1…F9 (au prochain lancement)");
        });
        let mut retirer = None;
        ui.horizontal_wrapped(|ui| {
            for (k, s) in self.r.sons.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(150.0);
                    ui.vertical_centered(|ui| {
                        if ui.add(egui::Button::new(RichText::new(&s.nom).size(16.0)).min_size(egui::vec2(140.0, 46.0))).clicked() {
                            let _ = self.tx.send(Ordre::Jouer(k));
                        }
                        ui.horizontal(|ui| {
                            if k < 9 {
                                ui.weak(format!("Ctrl+Maj+F{}", k + 1));
                            }
                            ui.add(egui::Slider::new(&mut s.volume, 0.0..=1.5).show_value(false));
                            if ui.small_button("🗑").clicked() {
                                retirer = Some(k);
                            }
                        });
                    });
                });
            }
        });
        if let Some(k) = retirer {
            self.r.sons.remove(k);
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter des sons (mp3, wav, ogg)…").clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Sons", &["mp3", "wav", "ogg", "flac"]).pick_files() {
                    for p in v {
                        self.r.sons.push(Son { nom: p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), fichier: p.display().to_string(), volume: 1.0 });
                    }
                }
            }
            if ui.button("⏹ Tout arrêter").clicked() {
                let _ = self.tx.send(Ordre::Stop);
            }
        });
        ui.weak("Des sons gratuits : myinstants.com, freesound.org.");
        if self.r != avant {
            enregistrer("boite_sons", &self.r);
            let _ = self.tx.send(Ordre::Recharger(self.r.clone()));
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 61 · Pixel art
// ---------------------------------------------------------------------------------------------------

/// Remplissage (pot de peinture) d'une grille de couleurs.
pub fn remplir(g: &mut [[u8; 4]], l: usize, i: usize, c: [u8; 4]) {
    let cible = g[i];
    if cible == c {
        return;
    }
    let h = g.len() / l;
    let mut pile = vec![i];
    while let Some(k) = pile.pop() {
        if g[k] != cible {
            continue;
        }
        g[k] = c;
        let (x, y) = (k % l, k / l);
        if x > 0 {
            pile.push(k - 1);
        }
        if x + 1 < l {
            pile.push(k + 1);
        }
        if y > 0 {
            pile.push(k - l);
        }
        if y + 1 < h {
            pile.push(k + l);
        }
    }
}

const PALETTE: [[u8; 3]; 24] = [
    [0, 0, 0], [255, 255, 255], [127, 127, 127], [195, 195, 195], [136, 0, 21], [237, 28, 36], [255, 127, 39], [255, 242, 0],
    [34, 177, 76], [0, 162, 232], [63, 72, 204], [163, 73, 164], [185, 122, 87], [255, 174, 201], [255, 201, 14], [239, 228, 176],
    [181, 230, 29], [153, 217, 234], [112, 146, 190], [200, 191, 231], [255, 95, 162], [40, 20, 50], [90, 60, 40], [60, 120, 60],
];

#[derive(Clone, Copy, PartialEq)]
enum OutilPixel {
    Crayon,
    Gomme,
    Pot,
    Pipette,
}

pub struct PixelArt {
    l: usize,
    g: Vec<[u8; 4]>,
    couleur: [u8; 4],
    outil: OutilPixel,
    symetrie: bool,
    grille: bool,
    annuler: Vec<Vec<[u8; 4]>>,
    trait_en_cours: bool,
    message: String,
}

impl PixelArt {
    pub fn new() -> Self {
        let (l, g): (usize, Vec<[u8; 4]>) = charger::<Option<(usize, Vec<[u8; 4]>)>>("pixel_art").filter(|(l, g)| *l > 0 && g.len() == l * l).unwrap_or((32, vec![[0; 4]; 32 * 32]));
        PixelArt { l, g, couleur: [255, 95, 162, 255], outil: OutilPixel::Crayon, symetrie: false, grille: true, annuler: vec![], trait_en_cours: false, message: String::new() }
    }

    fn image(&self, echelle: u32) -> RgbaImage {
        let l = self.l as u32;
        RgbaImage::from_fn(l * echelle, l * echelle, |x, y| Rgba(self.g[(y / echelle) as usize * self.l + (x / echelle) as usize]))
    }
}

impl Outil for PixelArt {
    fn nom(&self) -> &'static str {
        "👾 Pixel art"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pixel art");
        ui.horizontal_wrapped(|ui| {
            for (o, n) in [(OutilPixel::Crayon, "✏ Crayon"), (OutilPixel::Gomme, "🧽 Gomme"), (OutilPixel::Pot, "🪣 Remplir"), (OutilPixel::Pipette, "💧 Pipette")] {
                ui.selectable_value(&mut self.outil, o, n);
            }
            ui.checkbox(&mut self.symetrie, "Symétrie ↔");
            ui.checkbox(&mut self.grille, "Grille");
            if ui.add_enabled(!self.annuler.is_empty(), egui::Button::new("↩ Annuler")).clicked() || ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                if let Some(g) = self.annuler.pop() {
                    self.g = g;
                }
            }
            for t in [16, 32, 64] {
                if ui.small_button(format!("{t}×{t}")).on_hover_text("Nouvelle image").clicked() {
                    self.annuler.push(self.g.clone());
                    self.l = t;
                    self.g = vec![[0; 4]; t * t];
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            let mut c3 = [self.couleur[0], self.couleur[1], self.couleur[2]];
            if ui.color_edit_button_srgb(&mut c3).changed() {
                self.couleur = [c3[0], c3[1], c3[2], 255];
            }
            for c in PALETTE {
                let b = egui::Button::new("").fill(Color32::from_rgb(c[0], c[1], c[2])).min_size(egui::vec2(18.0, 18.0)).selected(self.couleur == [c[0], c[1], c[2], 255]);
                if ui.add(b).clicked() {
                    self.couleur = [c[0], c[1], c[2], 255];
                    if self.outil == OutilPixel::Gomme || self.outil == OutilPixel::Pipette {
                        self.outil = OutilPixel::Crayon;
                    }
                }
            }
        });
        let cote = (ui.available_height() - 60.0).min(ui.available_width() - 20.0).clamp(256.0, 640.0);
        let c = cote / self.l as f32;
        let (zone, rep) = ui.allocate_exact_size(egui::vec2(cote, cote), egui::Sense::click_and_drag());
        let p = ui.painter_at(zone);
        // damier pour la transparence
        for y in 0..self.l {
            for x in 0..self.l {
                let r = egui::Rect::from_min_size(zone.min + egui::vec2(x as f32 * c, y as f32 * c), egui::vec2(c, c));
                let px = self.g[y * self.l + x];
                if px[3] == 0 {
                    p.rect_filled(r, 0.0, if (x + y) % 2 == 0 { Color32::from_gray(70) } else { Color32::from_gray(90) });
                } else {
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(px[0], px[1], px[2], px[3]));
                }
                if self.grille && c >= 8.0 {
                    p.rect_stroke(r, 0.0, egui::Stroke::new(0.5_f32, Color32::from_black_alpha(60)), egui::StrokeKind::Inside);
                }
            }
        }
        if let Some(pos) = rep.interact_pointer_pos() {
            let (x, y) = (((pos.x - zone.left()) / c) as usize, ((pos.y - zone.top()) / c) as usize);
            if x < self.l && y < self.l {
                let i = y * self.l + x;
                if rep.drag_started() || rep.clicked() {
                    if !self.trait_en_cours && self.outil != OutilPixel::Pipette {
                        self.annuler.push(self.g.clone());
                        if self.annuler.len() > 50 {
                            self.annuler.remove(0);
                        }
                    }
                    self.trait_en_cours = true;
                }
                let couleur = if self.outil == OutilPixel::Gomme { [0; 4] } else { self.couleur };
                let miroir = y * self.l + (self.l - 1 - x);
                match self.outil {
                    OutilPixel::Crayon | OutilPixel::Gomme => {
                        self.g[i] = couleur;
                        if self.symetrie {
                            self.g[miroir] = couleur;
                        }
                    }
                    OutilPixel::Pot if rep.clicked() => {
                        remplir(&mut self.g, self.l, i, couleur);
                        if self.symetrie {
                            remplir(&mut self.g, self.l, miroir, couleur);
                        }
                    }
                    OutilPixel::Pipette if rep.clicked() => {
                        if self.g[i][3] > 0 {
                            self.couleur = self.g[i];
                            self.outil = OutilPixel::Crayon;
                        }
                    }
                    _ => {}
                }
            }
        }
        if rep.drag_stopped() || rep.clicked() {
            self.trait_en_cours = false;
            enregistrer("pixel_art", &Some((self.l, self.g.clone())));
        }
        ui.horizontal(|ui| {
            for e in [1u32, 8, 16] {
                if ui.button(format!("💾 PNG ×{e}")).clicked() {
                    if let Some(f) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("pixel.png").save_file() {
                        self.message = self.image(e).save(&f).map(|_| format!("✔ {}", f.display())).unwrap_or_else(|e| e.to_string());
                    }
                }
            }
            if ui.button("📋 Copier (×16)").clicked() {
                crate::capture::win::copier_image(&self.image(16));
            }
            if ui.button("🖼 Importer une image…").clicked() {
                if let Some(i) = ouvrir_image() {
                    self.annuler.push(self.g.clone());
                    let r = image::imageops::resize(&i, self.l as u32, self.l as u32, image::imageops::FilterType::Nearest);
                    self.g = r.pixels().map(|p| p.0).collect();
                }
            }
        });
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 63 · Métronome et boîte à rythmes
// ---------------------------------------------------------------------------------------------------

pub const FREQ: u32 = 44_100;

/// Sons synthétisés : 0 grosse caisse, 1 caisse claire, 2 charleston, 3 clap, 4 clic, 5 clic accentué.
pub fn synthe(k: usize) -> Vec<f32> {
    let n = |s: f32| (s * FREQ as f32) as usize;
    let mut bruit = 0x1234_5678u32;
    let mut alea = move || {
        bruit ^= bruit << 13;
        bruit ^= bruit >> 17;
        bruit ^= bruit << 5;
        (bruit as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let t = |i: usize| i as f32 / FREQ as f32;
    match k {
        0 => (0..n(0.35)).map(|i| {
            let x = t(i);
            let f = 50.0 + 120.0 * (-x * 30.0).exp();
            (std::f32::consts::TAU * f * x).sin() * (-x * 9.0).exp() * 0.9
        }).collect(),
        1 => (0..n(0.2)).map(|i| {
            let x = t(i);
            (alea() * 0.6 + (std::f32::consts::TAU * 190.0 * x).sin() * 0.4) * (-x * 22.0).exp() * 0.7
        }).collect(),
        2 => {
            let mut prec = 0.0;
            (0..n(0.06)).map(|i| {
                let b = alea();
                let h = b - prec; // passe-haut grossier
                prec = b;
                h * (-t(i) * 70.0).exp() * 0.4
            }).collect()
        }
        3 => (0..n(0.25)).map(|i| {
            let x = t(i);
            let env = (-((x * 1000.0) % 11.0) * 0.4).exp().max((-x * 18.0).exp() * (x > 0.03) as u8 as f32);
            alea() * env * (-x * 12.0).exp() * 0.6
        }).collect(),
        _ => {
            let f = if k == 5 { 1760.0 } else { 1100.0 };
            (0..n(0.04)).map(|i| (std::f32::consts::TAU * f * t(i)).sin() * (-t(i) * 120.0).exp() * 0.8).collect()
        }
    }
}

/// Tempo d'après des tapes (ms entre chaque).
pub fn bpm_tapes(t: &[Instant]) -> Option<u32> {
    if t.len() < 2 {
        return None;
    }
    let ecarts: Vec<f64> = t.windows(2).map(|w| w[1].duration_since(w[0]).as_secs_f64()).filter(|e| *e > 0.15 && *e < 2.5).collect();
    if ecarts.is_empty() {
        return None;
    }
    Some((60.0 / (ecarts.iter().sum::<f64>() / ecarts.len() as f64)).round() as u32)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rythme {
    pub bpm: u32,
    pub temps: u32,
    pub boite: bool,
    pub pas: [[bool; 16]; 4],
    pub volume: f32,
}

impl Default for Rythme {
    fn default() -> Self {
        let mut pas = [[false; 16]; 4];
        for i in [0, 8] {
            pas[0][i] = true;
        }
        for i in [4, 12] {
            pas[1][i] = true;
        }
        for i in (0..16).step_by(2) {
            pas[2][i] = true;
        }
        Rythme { bpm: 100, temps: 4, boite: false, pas, volume: 0.8 }
    }
}

pub struct Metronome {
    r: Arc<Mutex<Rythme>>,
    marche: Arc<AtomicBool>,
    position: Arc<AtomicUsize>,
    tapes: Vec<Instant>,
}

impl Metronome {
    pub fn new() -> Self {
        Metronome { r: Arc::new(Mutex::new(charger("rythme"))), marche: Arc::default(), position: Arc::default(), tapes: vec![] }
    }

    fn lancer(&self) {
        if self.marche.swap(true, Relaxed) {
            return;
        }
        let (r, m, pos) = (self.r.clone(), self.marche.clone(), self.position.clone());
        std::thread::spawn(move || {
            let Ok((_flux, h)) = rodio::OutputStream::try_default() else {
                m.store(false, Relaxed);
                return;
            };
            let sons: Vec<Arc<Vec<f32>>> = (0..6).map(|k| Arc::new(synthe(k))).collect();
            let mut prochain = Instant::now();
            let mut i = 0usize;
            while m.load(Relaxed) {
                let c = r.lock().unwrap().clone();
                let jouer = |k: usize| {
                    let v: Vec<f32> = sons[k].iter().map(|x| x * c.volume).collect();
                    let _ = h.play_raw(rodio::buffer::SamplesBuffer::new(1, FREQ, v));
                };
                let pas_par_temps = if c.boite { 4 } else { 1 };
                if c.boite {
                    let p = i % 16;
                    for (k, ligne) in c.pas.iter().enumerate() {
                        if ligne[p] {
                            jouer(k);
                        }
                    }
                    pos.store(p, Relaxed);
                } else {
                    let p = i % c.temps.max(1) as usize;
                    jouer(if p == 0 { 5 } else { 4 });
                    pos.store(p, Relaxed);
                }
                i += 1;
                prochain += Duration::from_secs_f64(60.0 / c.bpm.max(20) as f64 / pas_par_temps as f64);
                let maintenant = Instant::now();
                if prochain > maintenant {
                    std::thread::sleep(prochain - maintenant);
                } else {
                    prochain = maintenant; // retard (PC occupé) : on ne rattrape pas en rafale
                }
            }
        });
    }
}

impl Outil for Metronome {
    fn nom(&self) -> &'static str {
        "🥁 Métronome et boîte à rythmes"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Métronome et boîte à rythmes");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        let marche = self.marche.load(Relaxed);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} BPM", r.bpm)).size(28.0).strong());
            ui.add(egui::Slider::new(&mut r.bpm, 30..=260).show_value(false));
            for d in [-5i32, -1, 1, 5] {
                if ui.small_button(format!("{d:+}")).clicked() {
                    r.bpm = (r.bpm as i32 + d).clamp(30, 260) as u32;
                }
            }
            if ui.button("👆 Taper le tempo").clicked() {
                let t = Instant::now();
                if self.tapes.last().is_some_and(|x| t.duration_since(*x) > Duration::from_secs(3)) {
                    self.tapes.clear();
                }
                self.tapes.push(t);
                if self.tapes.len() > 8 {
                    self.tapes.remove(0);
                }
                if let Some(b) = bpm_tapes(&self.tapes) {
                    r.bpm = b.clamp(30, 260);
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button(RichText::new(if marche { "⏹ Stop" } else { "▶ Lancer" }).size(18.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Space)) {
                if marche {
                    self.marche.store(false, Relaxed);
                } else {
                    self.lancer();
                }
            }
            ui.selectable_value(&mut r.boite, false, "🎵 Métronome");
            ui.selectable_value(&mut r.boite, true, "🥁 Boîte à rythmes");
            ui.label("🔊");
            ui.add(egui::Slider::new(&mut r.volume, 0.0..=1.0).show_value(false));
        });
        let pos = self.position.load(Relaxed);
        if !r.boite {
            ui.horizontal(|ui| {
                ui.label("Temps par mesure");
                for t in [2, 3, 4, 5, 6, 7] {
                    ui.selectable_value(&mut r.temps, t, t.to_string());
                }
            });
            ui.horizontal(|ui| {
                for k in 0..r.temps as usize {
                    let actif = marche && k == pos;
                    let c = if actif { if k == 0 { Color32::from_rgb(255, 95, 162) } else { Color32::WHITE } } else { Color32::from_gray(60) };
                    let (rr, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
                    ui.painter().circle_filled(rr.center(), 20.0, c);
                }
            });
        } else {
            egui::Grid::new("boite").spacing([3.0, 4.0]).show(ui, |ui| {
                for (k, n) in ["Grosse caisse", "Caisse claire", "Charleston", "Clap"].iter().enumerate() {
                    ui.label(*n);
                    for p in 0..16 {
                        let on = r.pas[k][p];
                        let mut c = if on { [Color32::from_rgb(255, 95, 162), Color32::from_rgb(120, 170, 255), Color32::from_rgb(250, 210, 90), Color32::from_rgb(110, 220, 130)][k] } else if p % 4 == 0 { Color32::from_gray(75) } else { Color32::from_gray(50) };
                        if marche && p == pos {
                            c = c.gamma_multiply(1.6);
                        }
                        if ui.add(egui::Button::new("").fill(c).min_size(egui::vec2(26.0, 26.0))).clicked() {
                            r.pas[k][p] = !on;
                        }
                    }
                    ui.end_row();
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button("Rythme de base").clicked() {
                    r.pas = Rythme::default().pas;
                }
                if ui.small_button("Effacer").clicked() {
                    r.pas = [[false; 16]; 4];
                }
            });
        }
        if marche {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
        }
        if r != avant {
            enregistrer("rythme", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(taille_gif(1920, 1080, 640), (640, 360));
        assert_eq!(taille_gif(301, 201, 640), (300, 200));
        assert_eq!(lire_temps("00:01:02,500"), Some(62_500));
        assert_eq!(lire_temps("1:02.5"), Some(62_500));
        assert_eq!(ecrire_temps(3_723_004), "01:02:03,004");
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\nSalut\r\nça va ?\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000 X1:0\r\nOui\r\n";
        let v = lire_srt(srt);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0], Ligne { debut: 1000, fin: 2500, texte: "Salut\nça va ?".into() });
        assert_eq!(lire_srt(&ecrire_srt(&v)), v);
        let mut w = v.clone();
        recadencer(&mut w, 25.0, 23.976);
        assert_eq!(w[1].debut, 3128);
        let mut g = vec![[0u8; 4]; 9];
        g[4] = [1, 1, 1, 1];
        remplir(&mut g, 3, 0, [9, 9, 9, 9]);
        assert_eq!(g.iter().filter(|c| **c == [9, 9, 9, 9]).count(), 8);
        assert_eq!(g[4], [1, 1, 1, 1]);
        let i = RgbaImage::from_pixel(200, 100, Rgba([1, 2, 3, 255]));
        let c = couvrir(&i, 50, 50);
        assert_eq!((c.width(), c.height()), (50, 50));
        let m = composer(&Miniature::default(), None, Some(&i), None);
        assert_eq!((m.width(), m.height()), (ML, MH));
        for k in 0..6 {
            let s = synthe(k);
            assert!(!s.is_empty() && s.iter().all(|x| x.is_finite() && x.abs() <= 1.0), "{k}");
        }
        let t0 = Instant::now();
        let t: Vec<Instant> = (0..4).map(|k| t0 + Duration::from_millis(500 * k)).collect();
        assert_eq!(bpm_tapes(&t), Some(120));
    }
}
