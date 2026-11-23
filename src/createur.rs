//! Créateur de contenu : vidéos verticales sous-titrées pour TikTok/Shorts (29), studio de miniatures
//! et bannières avec modèles (30), générateur et éditeur de skins Minecraft (31).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText, Sense};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// ffmpeg prêt à lancer (téléchargé tout seul sous Windows ; à appeler dans un fil).
pub fn ffmpeg() -> Result<Command, String> {
    #[cfg(windows)]
    return crate::dependances::commande("ffmpeg");
    #[cfg(not(windows))]
    {
        let ok = Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false);
        if ok { Ok(Command::new("ffmpeg")) } else { Err("ffmpeg n'est pas installé (paquet « ffmpeg »)".into()) }
    }
}

/// « 1:23 », « 01:02:03.5 », « 83 » → secondes.
fn lire_temps(t: &str) -> Option<f64> {
    let t = t.trim().replace(',', ".");
    if t.is_empty() {
        return None;
    }
    t.split(':').try_fold(0.0, |acc, p| p.trim().parse::<f64>().ok().map(|v| acc * 60.0 + v))
}

fn lancer(mut c: Command) -> Result<(), String> {
    let o = c.output().map_err(|e| e.to_string())?;
    if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec").to_string()) }
}

fn texture(ctx: &egui::Context, nom: &str, img: &RgbaImage) -> egui::TextureHandle {
    let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    ctx.load_texture(nom, c, egui::TextureOptions::LINEAR)
}

fn texture_pixels(ctx: &egui::Context, nom: &str, img: &RgbaImage) -> egui::TextureHandle {
    let c = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    ctx.load_texture(nom, c, egui::TextureOptions::NEAREST)
}

/// Téléchargement vers un fichier en indiquant la progression.
fn telecharger(url: &str, dest: &Path, msg: &Arc<Mutex<String>>, quoi: &str) -> Result<(), String> {
    let r = ureq::get(url).set("User-Agent", "Outils").call().map_err(|e| e.to_string())?;
    let total: u64 = r.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let tmp = dest.with_extension("part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut lu = 0u64;
    let mut b = vec![0u8; 1 << 16];
    let mut r = r.into_reader();
    loop {
        let n = r.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut f, &b[..n]).map_err(|e| e.to_string())?;
        lu += n as u64;
        *msg.lock().unwrap() = if total > 0 { format!("Téléchargement de {quoi}… {} %", lu * 100 / total) } else { format!("Téléchargement de {quoi}… {} Mo", lu >> 20) };
    }
    drop(f);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------------
// 29 · Vidéos verticales (TikTok, Shorts, Reels) avec sous-titres automatiques
// ---------------------------------------------------------------------------------------------------

fn dossier_whisper() -> PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.join("whisper")).unwrap_or_default()
}

fn chercher(d: &Path, noms: &[&str]) -> Option<PathBuf> {
    for e in std::fs::read_dir(d).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(x) = chercher(&p, noms) {
                return Some(x);
            }
        } else if noms.iter().any(|n| p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(*n))) {
            return Some(p);
        }
    }
    None
}

/// Programme de reconnaissance vocale whisper.cpp (téléchargé la première fois sous Windows).
fn whisper(msg: &Arc<Mutex<String>>) -> Result<PathBuf, String> {
    let d = dossier_whisper();
    #[cfg(windows)]
    {
        if let Some(p) = chercher(&d, &["whisper-cli.exe", "main.exe"]) {
            return Ok(p);
        }
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        *msg.lock().unwrap() = "Recherche de whisper.cpp…".into();
        let j: serde_json::Value = ureq::get("https://api.github.com/repos/ggml-org/whisper.cpp/releases/latest").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
        let url = j["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| Some((a["name"].as_str()?.to_lowercase(), a["browser_download_url"].as_str()?.to_string())))
            .filter(|(n, _)| n.ends_with(".zip") && n.contains("x64") && n.contains("bin") && !n.contains("cublas") && !n.contains("cuda"))
            .min_by_key(|(n, _)| n.len())
            .map(|x| x.1)
            .ok_or("whisper.cpp introuvable sur GitHub")?;
        let z = d.join("whisper.zip");
        telecharger(&url, &z, msg, "whisper.cpp")?;
        let mut a = zip::ZipArchive::new(std::fs::File::open(&z).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        a.extract(&d).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&z);
        chercher(&d, &["whisper-cli.exe", "main.exe"]).ok_or_else(|| "whisper-cli.exe absent de l'archive".into())
    }
    #[cfg(not(windows))]
    {
        let _ = msg;
        if let Some(p) = chercher(&d, &["whisper-cli"]) {
            return Ok(p);
        }
        for n in ["whisper-cli", "whisper-cpp"] {
            if Command::new(n).arg("--help").output().is_ok() {
                return Ok(PathBuf::from(n));
            }
        }
        Err("whisper.cpp n'est pas installé (paquet « whisper-cpp », ou place whisper-cli dans ~/.config/outils/whisper)".into())
    }
}

/// Modèle de reconnaissance (base ≈ 150 Mo, small ≈ 490 Mo, plus précis).
fn modele(precis: bool, msg: &Arc<Mutex<String>>) -> Result<PathBuf, String> {
    let nom = if precis { "ggml-small.bin" } else { "ggml-base.bin" };
    let d = dossier_whisper();
    let p = d.join(nom);
    if !p.is_file() {
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        telecharger(&format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{nom}"), &p, msg, "modèle de voix (une seule fois)")?;
    }
    Ok(p)
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglagesShort {
    /// 0 : rogner au centre, 1 : vidéo entière sur fond flou
    pub mode: u8,
    pub position: f32,
    pub langue: String,
    pub precis: bool,
    pub majuscules: bool,
    pub jaune: bool,
    pub taille: u32,
    pub hauteur: u32,
}

impl Default for ReglagesShort {
    fn default() -> Self {
        ReglagesShort { mode: 0, position: 0.5, langue: "fr".into(), precis: false, majuscules: true, jaune: false, taille: 16, hauteur: 70 }
    }
}

/// Filtre ffmpeg qui passe la vidéo en 1080×1920 (et grave `st.srt` si demandé).
pub fn filtre_vertical(r: &ReglagesShort, sous_titres: bool) -> String {
    let mut f = if r.mode == 0 {
        format!("crop='min(iw,ih*9/16)':ih:'(iw-min(iw,ih*9/16))*{:.3}':0,scale=1080:1920,setsar=1", r.position.clamp(0.0, 1.0))
    } else {
        "split[a][b];[a]scale=1080:1920:force_original_aspect_ratio=increase,crop=1080:1920,boxblur=20:5[f];[b]scale=1080:-2[v];[f][v]overlay=(W-w)/2:(H-h)/2,setsar=1".to_string()
    };
    if sous_titres {
        let couleur = if r.jaune { "&H0000FFFF" } else { "&H00FFFFFF" };
        f += &format!(
            ",subtitles=st.srt:force_style='FontName=Arial,FontSize={},Bold=1,PrimaryColour={couleur},OutlineColour=&H00000000,BorderStyle=1,Outline=3,Shadow=0,Alignment=2,MarginV={}'",
            r.taille, r.hauteur
        );
    }
    f
}

/// Texte des sous-titres en majuscules (les lignes de numéros et de temps ne changent pas).
pub fn srt_majuscules(s: &str) -> String {
    s.lines().map(|l| if l.contains("-->") || l.trim().chars().all(|c| c.is_ascii_digit()) { l.to_string() } else { l.to_uppercase() }).collect::<Vec<_>>().join("\n")
}

pub struct Shorts {
    r: ReglagesShort,
    video: Option<PathBuf>,
    debut: String,
    fin: String,
    srt: Arc<Mutex<String>>,
    message: Arc<Mutex<String>>,
    occupe: Arc<std::sync::atomic::AtomicBool>,
}

impl Shorts {
    pub fn new() -> Self {
        Shorts { r: charger("shorts"), video: None, debut: String::new(), fin: String::new(), srt: Arc::default(), message: Arc::default(), occupe: Arc::default() }
    }

    fn coupe(&self, c: &mut Command) {
        if let Some(d) = lire_temps(&self.debut).filter(|d| *d > 0.0) {
            c.args(["-ss", &format!("{d}")]);
        }
        if let Some(f) = lire_temps(&self.fin).filter(|f| *f > 0.0) {
            c.args(["-to", &format!("{f}")]);
        }
    }

    fn tmp() -> PathBuf {
        std::env::temp_dir().join("outils_short")
    }

    fn transcrire(&self) {
        let Some(v) = self.video.clone() else { return };
        let (m, s, o, r) = (self.message.clone(), self.srt.clone(), self.occupe.clone(), self.r.clone());
        let mut extraire = match ffmpeg() {
            Ok(c) => c,
            Err(e) => {
                *m.lock().unwrap() = format!("✖ {e}");
                return;
            }
        };
        extraire.arg("-y").args(["-v", "error"]);
        self.coupe(&mut extraire);
        o.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(move || {
            let res = (|| -> Result<String, String> {
                let t = Shorts::tmp();
                std::fs::create_dir_all(&t).map_err(|e| e.to_string())?;
                *m.lock().unwrap() = "Extraction du son…".into();
                extraire.arg("-i").arg(&v).args(["-vn", "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le"]).arg(t.join("a.wav"));
                lancer(extraire)?;
                let w = whisper(&m)?;
                let md = modele(r.precis, &m)?;
                *m.lock().unwrap() = "Reconnaissance de la voix (sur le PC, sans internet)…".into();
                let mut c = Command::new(w);
                c.arg("-m").arg(md).args(["-l", if r.langue.is_empty() { "auto" } else { &r.langue }, "-osrt", "-ml", "22", "-sow", "-f"]).arg(t.join("a.wav")).arg("-of").arg(t.join("st"));
                #[cfg(windows)]
                std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000);
                lancer(c)?;
                let txt = std::fs::read_to_string(t.join("st.srt")).map_err(|_| "aucun sous-titre produit".to_string())?;
                Ok(if r.majuscules { srt_majuscules(&txt) } else { txt })
            })();
            match res {
                Ok(t) => {
                    *s.lock().unwrap() = t;
                    *m.lock().unwrap() = "✔ Sous-titres prêts : corrige-les si besoin, puis crée la vidéo.".into();
                }
                Err(e) => *m.lock().unwrap() = format!("✖ {e}"),
            }
            o.store(false, std::sync::atomic::Ordering::Relaxed);
        });
    }

    fn creer(&self) {
        let Some(v) = self.video.clone() else { return };
        let (m, o) = (self.message.clone(), self.occupe.clone());
        let srt = self.srt.lock().unwrap().clone();
        let filtre = filtre_vertical(&self.r, !srt.trim().is_empty());
        let mut c = match ffmpeg() {
            Ok(c) => c,
            Err(e) => {
                *m.lock().unwrap() = format!("✖ {e}");
                return;
            }
        };
        c.arg("-y").args(["-v", "error"]);
        self.coupe(&mut c);
        let sortie = v.with_file_name(format!("{} vertical.mp4", v.file_stem().unwrap_or_default().to_string_lossy()));
        c.arg("-i").arg(&v).args(["-vf", &filtre, "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"]).arg(&sortie);
        o.store(true, std::sync::atomic::Ordering::Relaxed);
        *m.lock().unwrap() = "Création de la vidéo verticale…".into();
        std::thread::spawn(move || {
            let t = Shorts::tmp();
            let r = std::fs::create_dir_all(&t).and_then(|_| std::fs::write(t.join("st.srt"), srt)).map_err(|e| e.to_string()).and_then(|_| {
                // chemin relatif pour le filtre subtitles (évite l'échappement des « C:\ »)
                c.current_dir(&t);
                lancer(c)
            });
            *m.lock().unwrap() = match r {
                Ok(()) => {
                    crate::outil::ouvrir(sortie.parent().unwrap_or(Path::new(".")));
                    format!("✔ {}", sortie.display())
                }
                Err(e) => format!("✖ {e}"),
            };
            o.store(false, std::sync::atomic::Ordering::Relaxed);
        });
    }
}

impl Outil for Shorts {
    fn nom(&self) -> &'static str {
        "📱 Vidéo verticale (TikTok, Shorts)"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Vidéo verticale avec sous-titres");
        ui.weak("Transforme une vidéo horizontale en format 9:16 (TikTok, YouTube Shorts, Reels) et ajoute des sous-titres écrits automatiquement à partir de la voix.");
        let avant = self.r.clone();
        let occupe = self.occupe.load(std::sync::atomic::Ordering::Relaxed);
        ui.horizontal_wrapped(|ui| {
            if ui.button("🎬 Choisir la vidéo…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Vidéos", &["mp4", "mkv", "mov", "webm", "avi"]).pick_file() {
                    self.video = Some(p);
                    self.srt.lock().unwrap().clear();
                }
            }
            if let Some(v) = &self.video {
                ui.label(v.file_name().unwrap_or_default().to_string_lossy());
            }
        });
        if let Some(p) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            self.video = Some(p);
            self.srt.lock().unwrap().clear();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Garder de");
            ui.add(egui::TextEdit::singleline(&mut self.debut).hint_text("0:00").desired_width(60.0));
            ui.label("à");
            ui.add(egui::TextEdit::singleline(&mut self.fin).hint_text("fin").desired_width(60.0));
            ui.weak("(Shorts : 60 s max conseillé)");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Cadrage :");
            ui.selectable_value(&mut self.r.mode, 0, "✂ Rogner");
            ui.selectable_value(&mut self.r.mode, 1, "🌫 Vidéo entière sur fond flou");
            if self.r.mode == 0 {
                ui.label("⬅ position ➡");
                ui.add(egui::Slider::new(&mut self.r.position, 0.0..=1.0).show_value(false));
            }
        });
        ui.separator();
        ui.strong("Sous-titres");
        ui.horizontal_wrapped(|ui| {
            ui.label("Langue");
            egui::ComboBox::from_id_salt("langue_short").selected_text(match self.r.langue.as_str() { "fr" => "Français", "en" => "Anglais", "es" => "Espagnol", _ => "Automatique" }).show_ui(ui, |ui| {
                for (c, n) in [("fr", "Français"), ("en", "Anglais"), ("es", "Espagnol"), ("auto", "Automatique")] {
                    ui.selectable_value(&mut self.r.langue, c.into(), n);
                }
            });
            ui.checkbox(&mut self.r.precis, "Plus précis (plus lent)");
            ui.checkbox(&mut self.r.majuscules, "MAJUSCULES");
            ui.checkbox(&mut self.r.jaune, "Jaune");
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Taille");
            ui.add(egui::Slider::new(&mut self.r.taille, 10..=30));
            ui.label("Hauteur");
            ui.add(egui::Slider::new(&mut self.r.hauteur, 10..=200));
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.video.is_some() && !occupe, egui::Button::new("📝 1. Écrire les sous-titres")).clicked() {
                self.transcrire();
            }
            if ui.add_enabled(self.video.is_some() && !occupe, egui::Button::new("🎬 2. Créer la vidéo verticale")).clicked() {
                self.creer();
            }
            if occupe {
                ui.spinner();
            }
        });
        ui.weak("Sans l'étape 1, la vidéo est créée sans sous-titres. La reconnaissance se fait sur le PC (whisper.cpp), rien n'est envoyé en ligne.");
        ui.label(self.message.lock().unwrap().clone());
        let mut s = self.srt.lock().unwrap();
        if !s.is_empty() {
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut *s).code_editor().desired_width(f32::INFINITY));
            });
        }
        drop(s);
        if occupe {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        if self.r != avant {
            enregistrer("shorts", &self.r);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 30 · Studio miniatures et bannières
// ---------------------------------------------------------------------------------------------------

/// Formats : nom, largeur, hauteur, zone visible partout (largeur, hauteur) si plus petite.
pub const FORMATS: [(&str, u32, u32, Option<(u32, u32)>); 7] = [
    ("Miniature YouTube", 1280, 720, None),
    ("Bannière YouTube", 2560, 1440, Some((1546, 423))),
    ("Bannière Twitch", 1200, 480, None),
    ("Bannière X / Twitter", 1500, 500, None),
    ("Bannière Discord", 960, 540, None),
    ("Image TikTok / Short", 1080, 1920, None),
    ("Avatar / logo", 800, 800, None),
];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum Modele {
    Neon,
    Blocs,
    Sobre,
    Flamme,
    Pastel,
    Retro,
}

pub const MODELES: [(Modele, &str); 6] = [(Modele::Neon, "Gaming néon"), (Modele::Blocs, "Blocs (pixel)"), (Modele::Sobre, "Sobre"), (Modele::Flamme, "Flamme"), (Modele::Pastel, "Pastel"), (Modele::Retro, "Rétro")];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Banniere {
    pub format: usize,
    pub modele: Modele,
    pub titre: String,
    pub sous_titre: String,
    pub badge: String,
    pub taille: f32,
    pub alignement: u8, // 0 gauche, 1 centre
    pub assombrir: f32,
}

impl Default for Banniere {
    fn default() -> Self {
        Banniere { format: 0, modele: Modele::Neon, titre: "MA CHAÎNE".into(), sous_titre: "Nouvelle vidéo chaque semaine".into(), badge: "NOUVEAU".into(), taille: 1.0, alignement: 1, assombrir: 0.2 }
    }
}

/// Couleurs du modèle : dégradé (haut, bas), texte, contour, badge.
pub fn couleurs(m: Modele) -> ([u8; 3], [u8; 3], [u8; 3], [u8; 3], [u8; 3]) {
    match m {
        Modele::Neon => ([60, 10, 120], [0, 180, 220], [255, 255, 255], [230, 0, 200], [0, 230, 255]),
        Modele::Blocs => ([90, 160, 60], [110, 75, 40], [255, 230, 60], [30, 30, 30], [220, 40, 40]),
        Modele::Sobre => ([25, 28, 36], [15, 16, 20], [245, 245, 245], [0, 0, 0], [70, 130, 255]),
        Modele::Flamme => ([220, 30, 20], [255, 160, 0], [255, 240, 80], [60, 0, 0], [20, 20, 20]),
        Modele::Pastel => ([255, 170, 210], [150, 200, 255], [255, 255, 255], [90, 60, 120], [255, 120, 160]),
        Modele::Retro => ([255, 90, 120], [40, 10, 80], [255, 230, 120], [80, 0, 80], [0, 220, 200]),
    }
}

fn melange(a: [u8; 3], b: [u8; 3], t: f32) -> Rgba<u8> {
    let f = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t.clamp(0.0, 1.0)) as u8;
    Rgba([f(0), f(1), f(2), 255])
}

/// Image couvrant tout le cadre (rognée au centre).
pub fn couvrir(img: &RgbaImage, l: u32, h: u32) -> RgbaImage {
    let e = (l as f32 / img.width() as f32).max(h as f32 / img.height() as f32);
    let (nl, nh) = (((img.width() as f32 * e).ceil() as u32).max(l), ((img.height() as f32 * e).ceil() as u32).max(h));
    let r = image::imageops::resize(img, nl, nh, image::imageops::FilterType::Triangle);
    image::imageops::crop_imm(&r, (nl - l) / 2, (nh - h) / 2, l, h).to_image()
}

fn rectangle(c: &mut RgbaImage, x: i64, y: i64, l: i64, h: i64, couleur: Rgba<u8>) {
    for yy in y.max(0)..(y + h).min(c.height() as i64) {
        for xx in x.max(0)..(x + l).min(c.width() as i64) {
            c.put_pixel(xx as u32, yy as u32, couleur);
        }
    }
}

pub fn fond_modele(m: Modele, l: u32, h: u32) -> RgbaImage {
    let (a, b, _, _, k) = couleurs(m);
    match m {
        Modele::Blocs => {
            let t = (h / 12).max(8);
            RgbaImage::from_fn(l, h, |x, y| {
                let (bx, by) = (x / t, y / t);
                let n = ((bx.wrapping_mul(7919) ^ by.wrapping_mul(104729)) % 9) as i32 - 4;
                let base = if by < 3 { a } else { b };
                let v = |c: u8| (c as i32 + n * 6).clamp(0, 255) as u8;
                Rgba([v(base[0]), v(base[1]), v(base[2]), 255])
            })
        }
        Modele::Retro => RgbaImage::from_fn(l, h, |x, y| {
            let t = y as f32 / h as f32;
            let mut p = melange(a, b, t);
            // soleil et lignes horizontales
            let (cx, cy, r) = (l as f32 / 2.0, h as f32 * 0.62, h as f32 * 0.3);
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d < r && y as f32 <= cy && (y as f32 / (h as f32 / 40.0)) as u32 % 3 != 0 {
                p = melange([255, 220, 90], [255, 80, 120], (y as f32 - (cy - r)) / r);
            }
            if y as f32 > cy && (y as f32 - cy) as u32 % (h / 24).max(4) < 2 {
                p = Rgba([k[0], k[1], k[2], 255]);
            }
            p
        }),
        Modele::Neon => RgbaImage::from_fn(l, h, |x, y| {
            let t = (x as f32 / l as f32 + y as f32 / h as f32) / 2.0;
            let mut p = melange(a, b, t);
            // bandes diagonales
            if (x + y) % (h / 4).max(20) < (h / 60).max(2) {
                p = melange([p[0], p[1], p[2]], [255, 255, 255], 0.18);
            }
            p
        }),
        _ => RgbaImage::from_fn(l, h, |_, y| melange(a, b, y as f32 / h as f32)),
    }
}

pub fn composer_banniere(b: &Banniere, fond: Option<&RgbaImage>, perso: Option<&RgbaImage>, police: Option<&ab_glyph::FontVec>) -> RgbaImage {
    let (_, l, h, zone) = FORMATS[b.format.min(FORMATS.len() - 1)];
    let (_, _, texte, contour, badge) = couleurs(b.modele);
    let mut c = match fond {
        Some(f) => couvrir(f, l, h),
        None => fond_modele(b.modele, l, h),
    };
    if fond.is_some() && b.assombrir > 0.0 {
        for p in c.pixels_mut() {
            for k in 0..3 {
                p.0[k] = (p.0[k] as f32 * (1.0 - b.assombrir)) as u8;
            }
        }
    }
    // zone utile (la bannière YouTube n'est entièrement visible que sur les télés)
    let (zl, zh) = zone.unwrap_or((l, h));
    let (zx, zy) = ((l - zl) / 2, (h - zh) / 2);
    if let Some(p) = perso {
        let ph = (zh as f32 * 0.95) as u32;
        let pl = (p.width() as f32 * ph as f32 / p.height() as f32) as u32;
        let r = image::imageops::resize(p, pl.max(1), ph.max(1), image::imageops::FilterType::Triangle);
        image::imageops::overlay(&mut c, &r, (zx + zl - pl) as i64 - (zl / 30) as i64, (zy + zh - ph) as i64);
    }
    if b.modele == Modele::Sobre {
        rectangle(&mut c, zx as i64, (zy + zh) as i64 - (zh / 40).max(4) as i64, zl as i64, (zh / 40).max(4) as i64, Rgba([badge[0], badge[1], badge[2], 255]));
    }
    let Some(f) = police else { return c };
    let t1 = (zh as f32 * 0.35).min(zl as f32 * 0.11) * b.taille;
    let t2 = t1 * 0.38;
    let marge = zl as f32 * 0.06;
    let lignes: Vec<&str> = b.titre.lines().filter(|l| !l.trim().is_empty()).collect();
    let total = lignes.len() as f32 * t1 * 1.05 + if b.sous_titre.is_empty() { 0.0 } else { t2 * 1.4 };
    let mut y = zy as f32 + (zh as f32 - total) / 2.0;
    let x_de = |larg: f32| if b.alignement == 1 { zx as f32 + (zl as f32 - larg) / 2.0 } else { zx as f32 + marge };
    for li in &lignes {
        let lt = crate::creatif::largeur(f, t1, li);
        crate::creatif::ecrire(&mut c, f, t1, x_de(lt), y, li, texte, Some((contour, (t1 / 14.0).max(2.0) as i32)));
        y += t1 * 1.05;
    }
    if !b.sous_titre.is_empty() {
        let lt = crate::creatif::largeur(f, t2, &b.sous_titre);
        crate::creatif::ecrire(&mut c, f, t2, x_de(lt), y + t2 * 0.3, &b.sous_titre, texte, Some((contour, (t2 / 10.0).max(1.0) as i32)));
    }
    if !b.badge.is_empty() {
        let tb = t1 * 0.32;
        let lb = crate::creatif::largeur(f, tb, &b.badge);
        let (bx, by) = (zx as f32 + zl as f32 * 0.04, zy as f32 + zh as f32 * 0.07);
        rectangle(&mut c, (bx - tb * 0.4) as i64, (by - tb * 0.2) as i64, (lb + tb * 0.8) as i64, (tb * 1.4) as i64, Rgba([badge[0], badge[1], badge[2], 255]));
        let clair = badge.iter().map(|v| *v as u32).sum::<u32>() > 450;
        crate::creatif::ecrire(&mut c, f, tb, bx, by, &b.badge, if clair { [20, 20, 20] } else { [255, 255, 255] }, None);
    }
    c
}

pub struct Studio {
    b: Banniere,
    fond: Option<RgbaImage>,
    perso: Option<RgbaImage>,
    police: Option<ab_glyph::FontVec>,
    apercu: Option<egui::TextureHandle>,
    sale: bool,
    message: String,
}

impl Studio {
    pub fn new() -> Self {
        Studio { b: charger("studio_banniere"), fond: None, perso: None, police: None, apercu: None, sale: true, message: String::new() }
    }
}

fn ouvrir_image() -> Option<RgbaImage> {
    let p = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file()?;
    image::open(p).ok().map(|i| i.to_rgba8())
}

impl Outil for Studio {
    fn nom(&self) -> &'static str {
        "🎨 Studio miniatures et bannières"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Studio miniatures et bannières");
        if self.police.is_none() {
            self.police = crate::creatif::police(&["impact.ttf", "arialbd.ttf", "segoeuib.ttf", "DejaVuSans-Bold.ttf", "LiberationSans-Bold.ttf", "NotoSans-Bold.ttf"]);
        }
        let avant = self.b.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("format_banniere").selected_text(format!("{} ({}×{})", FORMATS[self.b.format].0, FORMATS[self.b.format].1, FORMATS[self.b.format].2)).show_ui(ui, |ui| {
                for (k, f) in FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut self.b.format, k, format!("{} ({}×{})", f.0, f.1, f.2));
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Modèle");
            for (m, n) in MODELES {
                ui.selectable_value(&mut self.b.modele, m, n);
            }
        });
        ui.add(egui::TextEdit::multiline(&mut self.b.titre).desired_rows(1).desired_width(420.0).hint_text("Titre"));
        ui.add(egui::TextEdit::singleline(&mut self.b.sous_titre).desired_width(420.0).hint_text("Sous-titre"));
        ui.add(egui::TextEdit::singleline(&mut self.b.badge).desired_width(200.0).hint_text("Badge (ex. NOUVEAU, ÉP. 3)"));
        ui.horizontal_wrapped(|ui| {
            ui.label("Taille du texte");
            ui.add(egui::Slider::new(&mut self.b.taille, 0.4..=2.0).show_value(false));
            ui.selectable_value(&mut self.b.alignement, 0, "⬅");
            ui.selectable_value(&mut self.b.alignement, 1, "⬌");
            if ui.button("🖼 Photo de fond…").clicked() {
                self.fond = ouvrir_image();
                self.sale = true;
            }
            if self.fond.is_some() {
                ui.add(egui::Slider::new(&mut self.b.assombrir, 0.0..=0.8).text("assombrir"));
                if ui.small_button("✖ fond").clicked() {
                    self.fond = None;
                    self.sale = true;
                }
            }
            if ui.button("🧍 Personnage (PNG détouré)…").clicked() {
                self.perso = ouvrir_image();
                self.sale = true;
            }
            if self.perso.is_some() && ui.small_button("✖ perso").clicked() {
                self.perso = None;
                self.sale = true;
            }
        });
        if self.b != avant {
            enregistrer("studio_banniere", &self.b);
            self.sale = true;
        }
        let img = || composer_banniere(&self.b, self.fond.as_ref(), self.perso.as_ref(), self.police.as_ref());
        let (_, l, h, zone) = FORMATS[self.b.format];
        let echelle = (640.0 / l as f32).min(420.0 / h as f32);
        if self.sale {
            self.sale = false;
            let i = image::imageops::thumbnail(&img(), (l as f32 * echelle) as u32, (h as f32 * echelle) as u32);
            self.apercu = Some(texture(ui.ctx(), "studio", &i));
        }
        if let Some(t) = &self.apercu {
            let r = ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(l as f32 * echelle, h as f32 * echelle))).rect;
            if let Some((zl, zh)) = zone {
                let z = egui::Rect::from_center_size(r.center(), egui::vec2(zl as f32 * echelle, zh as f32 * echelle));
                ui.painter().rect_stroke(z, 0.0, egui::Stroke::new(1.0_f32, Color32::from_white_alpha(160)), egui::StrokeKind::Inside);
                ui.weak("Le cadre blanc est la partie visible sur ordinateur et téléphone (le reste n'apparaît que sur les télés).");
            }
        }
        ui.horizontal(|ui| {
            if ui.button(format!("💾 Enregistrer ({l}×{h})…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).add_filter("JPG", &["jpg"]).set_file_name(format!("{}.png", FORMATS[self.b.format].0.replace([' ', '/'], "_"))).save_file() {
                    self.message = img().save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
        });
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 31 · Skins Minecraft
// ---------------------------------------------------------------------------------------------------

/// Faces avant (x, y, largeur, hauteur) dans la texture 64×64 : tête, corps, bras droit, bras gauche, jambe droite, jambe gauche.
pub const AVANT: [(u32, u32, u32, u32); 6] = [(8, 8, 8, 8), (20, 20, 8, 12), (44, 20, 4, 12), (36, 52, 4, 12), (4, 20, 4, 12), (20, 52, 4, 12)];
pub const ARRIERE: [(u32, u32, u32, u32); 6] = [(24, 8, 8, 8), (32, 20, 8, 12), (52, 20, 4, 12), (44, 52, 4, 12), (12, 20, 4, 12), (28, 52, 4, 12)];
/// Calques (chapeau, veste…) : même ordre.
pub const CALQUE_AVANT: [(u32, u32, u32, u32); 6] = [(40, 8, 8, 8), (20, 36, 8, 12), (44, 36, 4, 12), (52, 52, 4, 12), (4, 36, 4, 12), (4, 52, 4, 12)];
pub const CALQUE_ARRIERE: [(u32, u32, u32, u32); 6] = [(56, 8, 8, 8), (32, 36, 8, 12), (52, 36, 4, 12), (60, 52, 4, 12), (12, 36, 4, 12), (12, 52, 4, 12)];
/// Position de chaque partie sur l'aperçu 16×32 (vu de face).
const PLACE: [(u32, u32); 6] = [(4, 0), (4, 8), (0, 8), (12, 8), (4, 20), (8, 20)];

/// Aperçu 16×32 d'une face (avant ou arrière) avec les calques.
pub fn apercu(skin: &RgbaImage, dos: bool) -> RgbaImage {
    let mut a = RgbaImage::new(16, 32);
    let (base, calque) = if dos { (&ARRIERE, &CALQUE_ARRIERE) } else { (&AVANT, &CALQUE_AVANT) };
    for k in 0..6 {
        // de dos, la droite et la gauche s'inversent
        let p = if dos { [0, 1, 3, 2, 5, 4][k] } else { k };
        for (zone, opaque) in [(base[k], true), (calque[k], false)] {
            let (x, y, l, h) = zone;
            for yy in 0..h {
                for xx in 0..l {
                    let px = *skin.get_pixel(x + xx, y + yy);
                    if opaque || px[3] > 0 {
                        let mut q = px;
                        if opaque {
                            q[3] = 255;
                        }
                        a.put_pixel(PLACE[p].0 + xx, PLACE[p].1 + yy, q);
                    }
                }
            }
        }
    }
    a
}

/// Ancien skin 64×32 → 64×64 (bras et jambe gauches copiés depuis la droite).
pub fn moderniser(s: &RgbaImage) -> RgbaImage {
    if s.height() == 64 {
        return s.clone();
    }
    let mut n = RgbaImage::new(64, 64);
    image::imageops::overlay(&mut n, s, 0, 0);
    let jambe = image::imageops::crop_imm(s, 0, 16, 16, 16).to_image();
    let bras = image::imageops::crop_imm(s, 40, 16, 16, 16).to_image();
    image::imageops::overlay(&mut n, &image::imageops::flip_horizontal(&jambe), 16, 48);
    image::imageops::overlay(&mut n, &image::imageops::flip_horizontal(&bras), 32, 48);
    n
}

#[derive(Clone, Copy)]
pub struct Tenue {
    pub peau: [u8; 3],
    pub cheveux: [u8; 3],
    pub yeux: [u8; 3],
    pub haut: [u8; 3],
    pub bas: [u8; 3],
    pub chaussures: [u8; 3],
    pub manches_courtes: bool,
    pub cheveux_longs: bool,
}

pub fn tenue_hasard(graine: u64) -> Tenue {
    let mut g = graine | 1;
    let mut h = |n: u64| {
        g ^= g << 13;
        g ^= g >> 7;
        g ^= g << 17;
        g % n
    };
    let peaux = [[255, 220, 190], [240, 190, 150], [200, 145, 100], [150, 100, 65], [100, 65, 40]];
    let cheveux = [[40, 25, 15], [90, 55, 25], [220, 180, 90], [160, 60, 20], [20, 20, 20], [230, 230, 230], [200, 60, 160], [60, 120, 230]];
    let vif = |h: &mut dyn FnMut(u64) -> u64| [h(200) as u8 + 30, h(200) as u8 + 30, h(200) as u8 + 30];
    Tenue {
        peau: peaux[h(5) as usize],
        cheveux: cheveux[h(8) as usize],
        yeux: [[60, 110, 220], [70, 140, 60], [100, 60, 30], [40, 40, 40]][h(4) as usize],
        haut: vif(&mut h),
        bas: vif(&mut h),
        chaussures: [[40, 40, 40], [240, 240, 240], [110, 70, 40], [180, 30, 30]][h(4) as usize],
        manches_courtes: h(2) == 0,
        cheveux_longs: h(3) == 0,
    }
}

/// Fabrique un skin complet à partir d'une tenue (léger grain pour faire « dessiné »).
pub fn generer(t: &Tenue, graine: u64) -> RgbaImage {
    let mut s = RgbaImage::new(64, 64);
    let grain = |x: u32, y: u32, c: [u8; 3]| {
        let n = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ graine as u32) % 13) as i32 - 6;
        Rgba([(c[0] as i32 + n).clamp(0, 255) as u8, (c[1] as i32 + n).clamp(0, 255) as u8, (c[2] as i32 + n).clamp(0, 255) as u8, 255])
    };
    let remplir = |s: &mut RgbaImage, x0: u32, y0: u32, l: u32, h: u32, c: [u8; 3]| {
        for y in y0..y0 + h {
            for x in x0..x0 + l {
                s.put_pixel(x, y, grain(x, y, c));
            }
        }
    };
    // tête (toutes les faces)
    remplir(&mut s, 0, 0, 32, 16, t.peau);
    remplir(&mut s, 8, 0, 8, 8, t.cheveux); // dessus
    remplir(&mut s, 24, 8, 8, 8, t.cheveux); // arrière
    remplir(&mut s, 0, 8, 32, 2, t.cheveux); // frange tout autour
    remplir(&mut s, 0, 10, 8, if t.cheveux_longs { 6 } else { 3 }, t.cheveux); // côté droit
    remplir(&mut s, 16, 10, 8, if t.cheveux_longs { 6 } else { 3 }, t.cheveux); // côté gauche
    remplir(&mut s, 8, 10, 1, 1, t.cheveux);
    remplir(&mut s, 15, 10, 1, 1, t.cheveux);
    // yeux et bouche
    for (x, c) in [(9, [255, 255, 255]), (10, t.yeux), (13, t.yeux), (14, [255, 255, 255])] {
        s.put_pixel(x, 12, Rgba([c[0], c[1], c[2], 255]));
    }
    let bouche = [t.peau[0].saturating_sub(70), t.peau[1].saturating_sub(70), t.peau[2].saturating_sub(60)];
    remplir(&mut s, 11, 14, 2, 1, bouche);
    // corps, bras, jambes
    remplir(&mut s, 16, 16, 24, 16, t.haut);
    for (x, y) in [(40, 16), (32, 48)] {
        remplir(&mut s, x, y, 16, 16, t.peau);
        remplir(&mut s, x, y, 16, if t.manches_courtes { 8 } else { 14 }, t.haut);
        remplir(&mut s, x + 4, y, 4, 4, t.haut); // dessus de l'épaule
    }
    for (x, y) in [(0, 16), (16, 48)] {
        remplir(&mut s, x, y, 16, 16, t.bas);
        remplir(&mut s, x, y + 13, 16, 3, t.chaussures);
        remplir(&mut s, x + 8, y, 4, 4, t.chaussures); // dessous du pied
    }
    // ceinture
    let ceinture = [t.bas[0] / 2, t.bas[1] / 2, t.bas[2] / 2];
    remplir(&mut s, 16, 30, 24, 2, ceinture);
    s
}

/// Skin d'un joueur Java par son pseudo (API publique de Mojang).
pub fn skin_du_joueur(pseudo: &str) -> Result<RgbaImage, String> {
    use base64::Engine;
    let p = pseudo.trim();
    if p.is_empty() || p.len() > 16 || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("pseudo invalide".into());
    }
    let id: serde_json::Value = ureq::get(&format!("https://api.mojang.com/users/profiles/minecraft/{p}")).call().map_err(|_| "joueur introuvable".to_string())?.into_json().map_err(|e| e.to_string())?;
    let id = id["id"].as_str().ok_or("joueur introuvable")?;
    let prof: serde_json::Value = ureq::get(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{id}")).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let v = prof["properties"][0]["value"].as_str().ok_or("profil vide")?;
    let tex: serde_json::Value = serde_json::from_slice(&base64::engine::general_purpose::STANDARD.decode(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let url = tex["textures"]["SKIN"]["url"].as_str().ok_or("ce joueur a le skin par défaut")?;
    let mut b = Vec::new();
    ureq::get(url).call().map_err(|e| e.to_string())?.into_reader().take(1 << 20).read_to_end(&mut b).map_err(|e| e.to_string())?;
    let i = image::load_from_memory(&b).map_err(|e| e.to_string())?.to_rgba8();
    Ok(moderniser(&i))
}

pub struct Skins {
    skin: RgbaImage,
    couleur: [u8; 3],
    gomme: bool,
    pipette: bool,
    pseudo: String,
    graine: u64,
    tex: Option<egui::TextureHandle>,
    faces: Option<(egui::TextureHandle, egui::TextureHandle)>,
    sale: bool,
    annuler: Vec<RgbaImage>,
    message: Arc<Mutex<String>>,
    recu: Arc<Mutex<Option<RgbaImage>>>,
}

impl Skins {
    pub fn new() -> Self {
        let graine = crate::outil::hasard(u64::MAX);
        let skin = image::open(crate::outil::chemin("skin.png")).ok().map(|i| moderniser(&i.to_rgba8())).unwrap_or_else(|| generer(&tenue_hasard(graine), graine));
        Skins { skin, couleur: [220, 40, 40], gomme: false, pipette: false, pseudo: String::new(), graine, tex: None, faces: None, sale: true, annuler: Vec::new(), message: Arc::default(), recu: Arc::default() }
    }

    fn changer(&mut self, s: RgbaImage) {
        self.annuler.push(std::mem::replace(&mut self.skin, s));
        if self.annuler.len() > 40 {
            self.annuler.remove(0);
        }
        self.sale = true;
    }
}

impl Outil for Skins {
    fn nom(&self) -> &'static str {
        "🧑‍🎨 Skins Minecraft"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur et éditeur de skins Minecraft");
        if let Some(s) = self.recu.lock().unwrap().take() {
            self.annuler.push(std::mem::replace(&mut self.skin, s));
            self.sale = true;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("🎲 Générer un skin").clicked() {
                self.graine = self.graine.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let s = generer(&tenue_hasard(self.graine), self.graine);
                self.changer(s);
            }
            ui.add(egui::TextEdit::singleline(&mut self.pseudo).hint_text("pseudo d'un joueur").desired_width(140.0));
            if ui.button("📥 Récupérer son skin").clicked() {
                let (p, m, r) = (self.pseudo.clone(), self.message.clone(), self.recu.clone());
                *m.lock().unwrap() = "Recherche…".into();
                std::thread::spawn(move || match skin_du_joueur(&p) {
                    Ok(s) => {
                        *r.lock().unwrap() = Some(s);
                        *m.lock().unwrap() = format!("✔ Skin de {p}");
                    }
                    Err(e) => *m.lock().unwrap() = format!("✖ {e}"),
                });
            }
            if ui.button("📂 Ouvrir un PNG…").clicked() {
                if let Some(i) = rfd::FileDialog::new().add_filter("PNG", &["png"]).pick_file().and_then(|p| image::open(p).ok()) {
                    let i = i.to_rgba8();
                    if i.width() == 64 && (i.height() == 64 || i.height() == 32) {
                        self.changer(moderniser(&i));
                    } else {
                        *self.message.lock().unwrap() = "Un skin fait 64×64 pixels.".into();
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Pinceau");
            ui.color_edit_button_srgb(&mut self.couleur);
            ui.toggle_value(&mut self.pipette, "💧 Pipette");
            ui.toggle_value(&mut self.gomme, "🧽 Gomme (calques)");
            if ui.add_enabled(!self.annuler.is_empty(), egui::Button::new("↶ Annuler")).clicked() {
                if let Some(s) = self.annuler.pop() {
                    self.skin = s;
                    self.sale = true;
                }
            }
        });
        if self.sale {
            self.sale = false;
            self.tex = Some(texture_pixels(ui.ctx(), "skin_tex", &self.skin));
            self.faces = Some((texture_pixels(ui.ctx(), "skin_avant", &apercu(&self.skin, false)), texture_pixels(ui.ctx(), "skin_dos", &apercu(&self.skin, true))));
            let _ = self.skin.save(crate::outil::chemin("skin.png"));
        }
        ui.horizontal_top(|ui| {
            let z = 6.0;
            let (r, rep) = ui.allocate_exact_size(egui::vec2(64.0 * z, 64.0 * z), Sense::click_and_drag());
            // damier pour la transparence
            for y in 0..16 {
                for x in 0..16 {
                    let c = if (x + y) % 2 == 0 { Color32::from_gray(60) } else { Color32::from_gray(80) };
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(x as f32 * 4.0 * z, y as f32 * 4.0 * z), egui::vec2(4.0 * z, 4.0 * z)), 0.0, c);
                }
            }
            if let Some(t) = &self.tex {
                ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            if let Some(p) = rep.interact_pointer_pos() {
                let (x, y) = (((p.x - r.min.x) / z) as u32, ((p.y - r.min.y) / z) as u32);
                if x < 64 && y < 64 {
                    if self.pipette {
                        let c = self.skin.get_pixel(x, y);
                        self.couleur = [c[0], c[1], c[2]];
                        self.pipette = false;
                    } else {
                        let v = if self.gomme { Rgba([0, 0, 0, 0]) } else { Rgba([self.couleur[0], self.couleur[1], self.couleur[2], 255]) };
                        if *self.skin.get_pixel(x, y) != v {
                            if rep.drag_started() || rep.clicked() || self.annuler.is_empty() {
                                self.annuler.push(self.skin.clone());
                            }
                            self.skin.put_pixel(x, y, v);
                            self.sale = true;
                        }
                    }
                }
            }
            if let Some(h) = rep.hover_pos() {
                let (x, y) = (((h.x - r.min.x) / z) as u32, ((h.y - r.min.y) / z) as u32);
                let zone = ["tête", "corps", "bras droit", "bras gauche", "jambe droite", "jambe gauche"];
                let nom = AVANT.iter().chain(ARRIERE.iter()).chain(CALQUE_AVANT.iter()).chain(CALQUE_ARRIERE.iter()).position(|&(zx, zy, l, hh)| x >= zx && x < zx + l && y >= zy && y < zy + hh);
                if let Some(k) = nom {
                    rep.on_hover_text(format!("{} {}{}", zone[k % 6], if (k / 6) % 2 == 0 { "(devant)" } else { "(dos)" }, if k >= 12 { " · calque" } else { "" }));
                }
            }
            ui.vertical(|ui| {
                if let Some((a, d)) = &self.faces {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(a).fit_to_exact_size(egui::vec2(96.0, 192.0)));
                        ui.add(egui::Image::new(d).fit_to_exact_size(egui::vec2(96.0, 192.0)));
                    });
                    ui.weak("devant · dos");
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("💾 Enregistrer le skin…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("skin.png").save_file() {
                    *self.message.lock().unwrap() = self.skin.save(&p).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("🌐 Le mettre sur mon compte").on_hover_text("Ouvre la page officielle de Minecraft : choisis le fichier enregistré").clicked() {
                let _ = open_url("https://www.minecraft.net/msaprofile/mygames/editskin");
            }
        });
        ui.label(RichText::new(self.message.lock().unwrap().clone()));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
}

fn open_url(u: &str) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd").args(["/c", "start", "", u]).creation_flags(0x0800_0000).spawn()
    }
    #[cfg(not(windows))]
    Command::new("xdg-open").arg(u).spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn createur() {
        let r = ReglagesShort::default();
        let f = filtre_vertical(&r, true);
        assert!(f.starts_with("crop=") && f.contains("scale=1080:1920") && f.contains("subtitles=st.srt"));
        assert!(!filtre_vertical(&ReglagesShort { mode: 1, ..r.clone() }, false).contains("subtitles"));
        assert_eq!(srt_majuscules("1\n00:00:00,000 --> 00:00:01,000\nsalut à tous\n"), "1\n00:00:00,000 --> 00:00:01,000\nSALUT À TOUS");
        for (m, _) in MODELES {
            let b = Banniere { modele: m, format: 4, ..Default::default() };
            let i = composer_banniere(&b, None, None, None);
            assert_eq!((i.width(), i.height()), (960, 540));
        }
        let t = tenue_hasard(42);
        let s = generer(&t, 42);
        assert_eq!((s.width(), s.height()), (64, 64));
        // les faces avant sont entièrement opaques, les calques transparents
        for (x, y, l, h) in AVANT {
            assert!((0..l).all(|xx| s.get_pixel(x + xx, y + h - 1)[3] == 255));
        }
        assert_eq!(s.get_pixel(40, 8)[3], 0);
        let a = apercu(&s, false);
        assert_eq!((a.width(), a.height()), (16, 32));
        assert_eq!(a.get_pixel(5, 4)[3], 255);
        let vieux = RgbaImage::from_pixel(64, 32, Rgba([1, 2, 3, 255]));
        assert_eq!(moderniser(&vieux).height(), 64);
        assert_eq!(moderniser(&vieux).get_pixel(20, 55)[0], 1);
    }
}
