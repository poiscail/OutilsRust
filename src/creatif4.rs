//! Créatif et voix, suite : traducteur vocal (45), éditeur vidéo simple (49), memes vidéo (50),
//! bannières et logos (51), stickers et émojis Discord (52), correcteur d'orthographe partout (69), commandes vocales (70).
use crate::outil::{charger, enregistrer, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

fn texture(ctx: &egui::Context, nom: &str, img: &RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(nom, egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), Default::default())
}

// ---------------------------------------------------------------------------------------------------
// Voix : reconnaissance (Windows) et synthèse dans une langue donnée
// ---------------------------------------------------------------------------------------------------

/// Reconnaît une phrase au micro. D'abord la reconnaissance en ligne de Windows (meilleure), sinon celle du PC.
pub fn ecouter(langue: &str) -> Result<String, String> {
    match ecouter_winrt(langue) {
        Ok(t) if !t.trim().is_empty() => Ok(t),
        _ => ecouter_local(langue),
    }
}

fn ecouter_winrt(langue: &str) -> Result<String, String> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Media::SpeechRecognition::*;
    let e = |x: windows::core::Error| x.message();
    let l = Language::CreateLanguage(&HSTRING::from(langue)).map_err(e)?;
    let r = SpeechRecognizer::Create(&l).map_err(e)?;
    let c = SpeechRecognitionTopicConstraint::Create(SpeechRecognitionScenario::Dictation, &HSTRING::from("dictee")).map_err(e)?;
    r.Constraints().map_err(e)?.Append(&c).map_err(e)?;
    let comp = r.CompileConstraintsAsync().map_err(e)?.get().map_err(e)?;
    if comp.Status().map_err(e)? != SpeechRecognitionResultStatus::Success {
        return Err("reconnaissance en ligne désactivée".into());
    }
    let res = r.RecognizeAsync().map_err(e)?.get().map_err(e)?;
    Ok(res.Text().map_err(e)?.to_string())
}

fn ecouter_local(langue: &str) -> Result<String, String> {
    let script = format!(r#"[Console]::OutputEncoding=[Text.Encoding]::UTF8
Add-Type -AssemblyName System.Speech
try {{ $r = New-Object System.Speech.Recognition.SpeechRecognitionEngine((New-Object System.Globalization.CultureInfo('{langue}'))) }} catch {{ 'ERREUR:pas de reconnaissance vocale {langue} installée'; exit }}
$r.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
$r.SetInputToDefaultAudioDevice()
$r.InitialSilenceTimeout = [TimeSpan]::FromSeconds(6)
$r.EndSilenceTimeout = [TimeSpan]::FromSeconds(1.2)
$res = $r.Recognize([TimeSpan]::FromSeconds(25))
if ($res) {{ $res.Text }}"#);
    let t = crate::optimiseur::ps(&script)?;
    let t = t.trim();
    if let Some(m) = t.strip_prefix("ERREUR:") {
        return Err(m.into());
    }
    if t.is_empty() {
        return Err("rien entendu".into());
    }
    Ok(t.to_string())
}

/// Lit un texte à voix haute avec une voix de la langue voulue (si installée).
pub fn dire(texte: &str, culture: &str) {
    let script = format!("Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; try {{ $s.SelectVoiceByHints('NotSet', 'NotSet', 0, [Globalization.CultureInfo]'{culture}') }} catch {{}}; $s.Speak([Console]::In.ReadToEnd())");
    if let Ok(mut c) = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).stdin(Stdio::piped()).creation_flags(SANS_FENETRE).spawn() {
        if let Some(mut e) = c.stdin.take() {
            let _ = e.write_all(texte.as_bytes());
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 45 · Traducteur vocal
// ---------------------------------------------------------------------------------------------------

/// (nom, code de traduction, culture voix / reconnaissance)
pub const LANGUES: [(&str, &str, &str); 8] = [
    ("Français", "fr", "fr-FR"), ("Anglais", "en", "en-US"), ("Espagnol", "es", "es-ES"), ("Allemand", "de", "de-DE"),
    ("Italien", "it", "it-IT"), ("Japonais", "ja", "ja-JP"), ("Portugais", "pt", "pt-BR"), ("Arabe", "ar", "ar-SA"),
];

pub struct TraducteurVocal {
    de: usize,
    vers: usize,
    historique: Arc<Mutex<Vec<(String, String)>>>,
    e: Etat,
    texte: String,
}

impl TraducteurVocal {
    pub fn new() -> Self {
        TraducteurVocal { de: 0, vers: 1, historique: Arc::default(), e: Arc::default(), texte: String::new() }
    }

    fn traduire(&self, entendu: Option<String>) {
        let (de, vers, h) = (LANGUES[self.de], LANGUES[self.vers], self.historique.clone());
        en_fond(&self.e, move || {
            let texte = match entendu {
                Some(t) => t,
                None => ecouter(de.2)?,
            };
            let t = crate::traducteur::traduire_google(&texte, vers.1)?;
            dire(&t, vers.2);
            h.lock().unwrap().push((texte, t.clone()));
            Ok(format!("🔊 {t}"))
        });
    }
}

impl Outil for TraducteurVocal {
    fn nom(&self) -> &'static str {
        "🗣 Traducteur vocal"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Traducteur vocal");
        ui.weak("Appuie, parle : ta phrase est traduite et lue à voix haute dans l'autre langue. Pratique en voyage ou pour parler avec un joueur étranger.");
        ui.horizontal(|ui| {
            for (titre, k, id) in [("Je parle", &mut self.de, "de"), ("Traduire en", &mut self.vers, "vers")] {
                ui.label(titre);
                egui::ComboBox::from_id_salt(id).selected_text(LANGUES[*k].0).show_ui(ui, |ui| {
                    for (i, l) in LANGUES.iter().enumerate() {
                        ui.selectable_value(k, i, l.0);
                    }
                });
            }
            if ui.button("⇄").clicked() {
                std::mem::swap(&mut self.de, &mut self.vers);
            }
        });
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new(RichText::new("🎙 Parler").size(22.0)).min_size(egui::vec2(160.0, 56.0))).clicked() {
                *self.e.lock().unwrap() = (true, "🎙 Je t'écoute…".into());
                self.traduire(None);
            }
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.texte).hint_text("ou tape une phrase").desired_width(260.0));
                if ui.add_enabled(!occupe && !self.texte.trim().is_empty(), egui::Button::new("Traduire et lire")).clicked() {
                    let t = std::mem::take(&mut self.texte);
                    self.traduire(Some(t));
                }
            });
        });
        for (a, b) in self.historique.lock().unwrap().iter().rev().take(15) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.weak(a);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(b).size(17.0).strong());
                    if ui.small_button("🔊").clicked() {
                        dire(b, LANGUES[self.vers].2);
                    }
                    if ui.small_button("📋").clicked() {
                        ui.ctx().copy_text(b.clone());
                    }
                });
            });
        }
        ui.weak("Pour une meilleure reconnaissance : Paramètres Windows › Confidentialité › Voix › Reconnaissance vocale en ligne. Les voix des autres langues s'ajoutent dans Paramètres › Heure et langue › Voix.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 49 · Éditeur vidéo simple / 50 · Memes vidéo (ffmpeg)
// ---------------------------------------------------------------------------------------------------

/// Texte sûr pour le filtre drawtext de ffmpeg.
pub fn echapper_drawtext(t: &str) -> String {
    t.replace('\\', "\\\\\\\\").replace('\'', "\u{2019}").replace(':', "\\:").replace('%', "\\%").replace(',', "\\,")
}

/// Police pour ffmpeg (chemin avec « : » échappé).
fn police_ffmpeg(noms: &[&str]) -> String {
    let d = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join("Fonts");
    let p = noms.iter().map(|n| d.join(n)).find(|p| p.exists()).unwrap_or(d.join("arial.ttf"));
    p.display().to_string().replace('\\', "/").replace(':', "\\:")
}

#[derive(Clone, PartialEq, Debug)]
pub struct Clip {
    pub fichier: PathBuf,
    pub duree: f64,
    pub debut: f64,
    pub fin: f64,
    pub son: bool,
}

/// (durée, a une piste son) d'une vidéo.
fn sonder(p: &Path) -> Result<(f64, bool), String> {
    let o = crate::dependances::commande("ffprobe")?.args(["-v", "error", "-show_entries", "format=duration:stream=codec_type", "-of", "json"]).arg(p).output().map_err(|e| e.to_string())?;
    let j: serde_json::Value = serde_json::from_slice(&o.stdout).map_err(|e| e.to_string())?;
    let d = j["format"]["duration"].as_str().and_then(|x| x.parse().ok()).ok_or("durée inconnue (fichier vidéo ?)")?;
    let son = j["streams"].as_array().is_some_and(|s| s.iter().any(|x| x["codec_type"] == "audio"));
    Ok((d, son))
}

/// Arguments ffmpeg pour assembler les clips (coupés, mis à la même taille), avec texte et musique facultatifs.
pub fn arguments_montage(clips: &[Clip], (l, h): (u32, u32), texte: &str, police: &str, musique: Option<(&Path, f32)>, sortie: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec!["-y".into()];
    for c in clips {
        a.extend(["-ss".into(), format!("{:.3}", c.debut), "-t".into(), format!("{:.3}", (c.fin - c.debut).max(0.1)), "-i".into(), c.fichier.display().to_string()]);
    }
    // silence pour les clips sans son
    let mut silences = vec![];
    for (k, c) in clips.iter().enumerate() {
        if !c.son {
            silences.push(k);
            a.extend(["-f".into(), "lavfi".into(), "-t".into(), format!("{:.3}", (c.fin - c.debut).max(0.1)), "-i".into(), "anullsrc=r=44100:cl=stereo".into()]);
        }
    }
    let mut entree_musique = None;
    if let Some((m, _)) = musique {
        entree_musique = Some(clips.len() + silences.len());
        a.extend(["-stream_loop".into(), "-1".into(), "-i".into(), m.display().to_string()]);
    }
    let mut f = String::new();
    let mut chaine = String::new();
    for (k, c) in clips.iter().enumerate() {
        f.push_str(&format!("[{k}:v]scale={l}:{h}:force_original_aspect_ratio=decrease,pad={l}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{k}];"));
        let src = if c.son { format!("{k}:a") } else { format!("{}:a", clips.len() + silences.iter().position(|s| *s == k).unwrap()) };
        f.push_str(&format!("[{src}]aresample=44100,aformat=channel_layouts=stereo[a{k}];"));
        chaine.push_str(&format!("[v{k}][a{k}]"));
    }
    f.push_str(&format!("{chaine}concat=n={}:v=1:a=1[vc][ac]", clips.len()));
    let mut v = "vc".to_string();
    if !texte.trim().is_empty() {
        f.push_str(&format!(";[vc]drawtext=fontfile='{police}':text='{}':fontcolor=white:fontsize={}:borderw=4:bordercolor=black:x=(w-tw)/2:y=h-th-{}[vt]", echapper_drawtext(texte.trim()), h / 14, h / 18));
        v = "vt".into();
    }
    let mut au = "ac".to_string();
    if let (Some(i), Some((_, vol))) = (entree_musique, musique) {
        f.push_str(&format!(";[{i}:a]volume={vol:.2}[mu];[ac][mu]amix=inputs=2:duration=first:dropout_transition=0[am]"));
        au = "am".into();
    }
    a.extend(["-filter_complex".into(), f, "-map".into(), format!("[{v}]"), "-map".into(), format!("[{au}]"), "-c:v".into(), "libx264".into(), "-preset".into(), "veryfast".into(), "-crf".into(), "21".into(), "-c:a".into(), "aac".into(), "-b:a".into(), "160k".into(), "-movflags".into(), "+faststart".into(), sortie.display().to_string()]);
    a
}

fn lancer_ffmpeg(args: &[String]) -> Result<(), String> {
    let o = crate::dependances::commande("ffmpeg")?.args(args).output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&o.stderr);
        Err(e.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("échec de ffmpeg").to_string())
    }
}

fn miniature_video(p: &Path, t: f64) -> Option<RgbaImage> {
    let tmp = std::env::temp_dir().join(format!("outils_mini_{}.png", crate::outil::hasard(1 << 40)));
    let ok = crate::dependances::commande("ffmpeg").ok()?.args(["-y", "-v", "error", "-ss", &format!("{t:.2}"), "-i"]).arg(p).args(["-frames:v", "1", "-vf", "scale=240:-2"]).arg(&tmp).status().ok()?.success();
    let r = if ok { image::open(&tmp).ok().map(|i| i.to_rgba8()) } else { None };
    let _ = std::fs::remove_file(tmp);
    r
}

pub struct EditeurVideo {
    clips: Arc<Mutex<Vec<Clip>>>,
    minis: Arc<Mutex<Vec<(PathBuf, f64, RgbaImage)>>>,
    textures: Vec<(PathBuf, f64, egui::TextureHandle)>,
    format: usize,
    texte: String,
    musique: Option<PathBuf>,
    volume: f32,
    e: Etat,
}

const FORMATS: [(&str, u32, u32); 4] = [("1080p (YouTube)", 1920, 1080), ("720p", 1280, 720), ("Vertical 9:16 (TikTok, Shorts)", 1080, 1920), ("Carré (Instagram)", 1080, 1080)];

impl EditeurVideo {
    pub fn new() -> Self {
        EditeurVideo { clips: Arc::default(), minis: Arc::default(), textures: vec![], format: 0, texte: String::new(), musique: None, volume: 0.35, e: Arc::default() }
    }

    fn ajouter(&self, v: Vec<PathBuf>) {
        let (c, m) = (self.clips.clone(), self.minis.clone());
        en_fond(&self.e, move || {
            for p in v {
                let (d, son) = sonder(&p)?;
                c.lock().unwrap().push(Clip { fichier: p.clone(), duree: d, debut: 0.0, fin: d, son });
                if let Some(i) = miniature_video(&p, d.min(1.0)) {
                    m.lock().unwrap().push((p, 0.0, i));
                }
            }
            Ok(String::new())
        });
    }
}

impl Outil for EditeurVideo {
    fn nom(&self) -> &'static str {
        "🎬 Éditeur vidéo simple"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éditeur vidéo simple");
        ui.weak("Assemble des clips (coupés au début et à la fin), ajoute un texte et une musique, puis exporte en MP4.");
        let occupe = etat_ui(ui, &self.e);
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !deposes.is_empty() && !occupe {
            self.ajouter(deposes);
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("➕ Ajouter des vidéos…")).clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Vidéos", &["mp4", "mkv", "mov", "avi", "webm"]).pick_files() {
                    self.ajouter(v);
                }
            }
            ui.weak("(ou glisse-les ici)");
        });
        for (p, t, img) in self.minis.lock().unwrap().drain(..) {
            let tex = texture(ui.ctx(), &format!("clip_{}", p.display()), &img);
            self.textures.push((p, t, tex));
        }
        let mut clips = self.clips.lock().unwrap();
        let mut retirer = None;
        let mut monter = None;
        for (k, c) in clips.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some((_, _, t)) = self.textures.iter().find(|x| x.0 == c.fichier) {
                        ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(120.0, 68.0)));
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(format!("{}. {}", k + 1, c.fichier.file_name().unwrap_or_default().to_string_lossy()));
                            if k > 0 && ui.small_button("⬆").clicked() {
                                monter = Some(k);
                            }
                            if ui.small_button("✖").clicked() {
                                retirer = Some(k);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Début");
                            ui.add(egui::Slider::new(&mut c.debut, 0.0..=c.duree).step_by(0.1).suffix(" s"));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Fin");
                            ui.add(egui::Slider::new(&mut c.fin, 0.0..=c.duree).step_by(0.1).suffix(" s"));
                        });
                        if c.fin <= c.debut {
                            c.fin = (c.debut + 0.5).min(c.duree);
                        }
                    });
                });
            });
        }
        if let Some(k) = monter {
            clips.swap(k, k - 1);
        }
        if let Some(k) = retirer {
            clips.remove(k);
        }
        let total: f64 = clips.iter().map(|c| c.fin - c.debut).sum();
        let liste = clips.clone();
        drop(clips);
        if liste.is_empty() {
            return;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Format");
            egui::ComboBox::from_id_salt("format_video").selected_text(FORMATS[self.format].0).show_ui(ui, |ui| {
                for (k, f) in FORMATS.iter().enumerate() {
                    ui.selectable_value(&mut self.format, k, f.0);
                }
            });
            ui.weak(format!("durée totale {}:{:02}", total as u64 / 60, total as u64 % 60));
        });
        ui.horizontal(|ui| {
            ui.label("Texte en bas");
            ui.add(egui::TextEdit::singleline(&mut self.texte).desired_width(300.0));
        });
        ui.horizontal(|ui| {
            if ui.button("🎵 Musique de fond…").clicked() {
                self.musique = rfd::FileDialog::new().add_filter("Audio", &["mp3", "wav", "ogg", "m4a", "flac"]).pick_file();
            }
            if let Some(m) = &self.musique {
                ui.label(m.file_name().unwrap_or_default().to_string_lossy());
                ui.add(egui::Slider::new(&mut self.volume, 0.05..=1.0).text("volume"));
                if ui.small_button("✖").clicked() {
                    self.musique = None;
                }
            }
        });
        if ui.add_enabled(!occupe, egui::Button::new(RichText::new("💾 Exporter en MP4…").strong())).clicked() {
            if let Some(f) = rfd::FileDialog::new().add_filter("MP4", &["mp4"]).set_file_name("montage.mp4").save_file() {
                let (_, l, h) = FORMATS[self.format];
                let args = arguments_montage(&liste, (l, h), &self.texte, &police_ffmpeg(&["arialbd.ttf", "segoeuib.ttf"]), self.musique.as_deref().map(|m| (m, self.volume)), &f);
                *self.e.lock().unwrap() = (true, "Export (une minute de vidéo ≈ une minute de calcul)…".into());
                let e = self.e.clone();
                std::thread::spawn(move || {
                    let r = lancer_ffmpeg(&args);
                    *e.lock().unwrap() = (false, match r {
                        Ok(()) => {
                            ouvrir(&f);
                            format!("✔ Vidéo exportée : {}", f.display())
                        }
                        Err(m) => format!("✖ {m}"),
                    });
                });
            }
        }
    }
}

/// Filtre de mème : texte en haut et en bas, en Impact blanc contouré de noir.
pub fn filtre_meme(haut: &str, bas: &str, police: &str) -> String {
    let mut f = vec!["scale='min(720,iw)':-2".to_string()];
    for (t, y) in [(haut, "h/20"), (bas, "h-th-h/20")] {
        if !t.trim().is_empty() {
            f.push(format!("drawtext=fontfile='{police}':text='{}':fontcolor=white:fontsize=h/9:borderw=5:bordercolor=black:x=(w-tw)/2:y={y}", echapper_drawtext(&t.trim().to_uppercase())));
        }
    }
    f.join(",")
}

pub struct MemesVideo {
    fichier: Option<PathBuf>,
    duree: f64,
    debut: f64,
    fin: f64,
    haut: String,
    bas: String,
    gif: bool,
    apercu: Option<egui::TextureHandle>,
    e: Etat,
}

impl MemesVideo {
    pub fn new() -> Self {
        MemesVideo { fichier: None, duree: 0.0, debut: 0.0, fin: 0.0, haut: String::new(), bas: String::new(), gif: false, apercu: None, e: Arc::default() }
    }
}

impl Outil for MemesVideo {
    fn nom(&self) -> &'static str {
        "🤣 Memes vidéo"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Memes vidéo");
        ui.weak("Un texte en haut, un texte en bas, sur un extrait de vidéo : en MP4 ou en GIF pour Discord.");
        let occupe = etat_ui(ui, &self.e);
        let depose = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        let mut choisi = depose;
        if ui.button("🎞 Choisir une vidéo…").clicked() {
            choisi = rfd::FileDialog::new().add_filter("Vidéos", &["mp4", "mkv", "mov", "avi", "webm", "gif"]).pick_file();
        }
        if let Some(p) = choisi {
            match sonder(&p) {
                Ok((d, _)) => {
                    (self.duree, self.debut, self.fin) = (d, 0.0, d.min(10.0));
                    self.apercu = miniature_video(&p, d.min(1.0)).map(|i| texture(ui.ctx(), "meme_apercu", &i));
                    self.fichier = Some(p);
                }
                Err(e) => *self.e.lock().unwrap() = (false, format!("✖ {e}")),
            }
        }
        let Some(p) = self.fichier.clone() else { return };
        ui.horizontal(|ui| {
            if let Some(t) = &self.apercu {
                ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(240.0, 135.0)));
            }
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.haut).hint_text("texte du haut").desired_width(300.0));
                ui.add(egui::TextEdit::singleline(&mut self.bas).hint_text("texte du bas").desired_width(300.0));
                ui.horizontal(|ui| {
                    ui.label("De");
                    ui.add(egui::Slider::new(&mut self.debut, 0.0..=self.duree).step_by(0.1).suffix(" s"));
                });
                ui.horizontal(|ui| {
                    ui.label("À");
                    ui.add(egui::Slider::new(&mut self.fin, 0.0..=self.duree).step_by(0.1).suffix(" s"));
                });
                ui.checkbox(&mut self.gif, "Exporter en GIF (plus lourd, sans son)");
            });
        });
        if self.fin <= self.debut {
            self.fin = (self.debut + 0.5).min(self.duree);
        }
        if ui.add_enabled(!occupe, egui::Button::new(RichText::new("💾 Créer le mème…").strong())).clicked() {
            let ext = if self.gif { "gif" } else { "mp4" };
            if let Some(f) = rfd::FileDialog::new().add_filter(ext, &[ext]).set_file_name(format!("meme.{ext}")).save_file() {
                let vf = filtre_meme(&self.haut, &self.bas, &police_ffmpeg(&["impact.ttf", "arialbd.ttf"]));
                let mut a: Vec<String> = vec!["-y".into(), "-ss".into(), format!("{:.2}", self.debut), "-t".into(), format!("{:.2}", self.fin - self.debut), "-i".into(), p.display().to_string()];
                if self.gif {
                    a.extend(["-filter_complex".into(), format!("[0:v]{vf},fps=15,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse"), "-loop".into(), "0".into()]);
                } else {
                    a.extend(["-vf".into(), vf, "-c:v".into(), "libx264".into(), "-crf".into(), "23".into(), "-preset".into(), "veryfast".into(), "-c:a".into(), "aac".into(), "-movflags".into(), "+faststart".into()]);
                }
                a.push(f.display().to_string());
                en_fond(&self.e, move || {
                    lancer_ffmpeg(&a)?;
                    let t = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    ouvrir(&f);
                    Ok(format!("✔ Mème créé ({}){}", crate::disque::taille_lisible(t), if t > 10 << 20 { " : plus de 10 Mo, raccourcis l'extrait pour Discord" } else { "" }))
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 51 · Bannières et logos
// ---------------------------------------------------------------------------------------------------

/// (nom, largeur, hauteur, zone sûre en % de la largeur et de la hauteur)
pub const MODELES: [(&str, u32, u32, f32, f32); 6] = [
    ("Icône de serveur Discord", 512, 512, 1.0, 1.0),
    ("Bannière de serveur Discord", 960, 540, 1.0, 1.0),
    ("Bannière de profil Discord", 680, 240, 1.0, 1.0),
    ("Bannière YouTube", 2560, 1440, 0.6, 0.29),
    ("Bannière Twitch", 1200, 480, 1.0, 1.0),
    ("Photo de profil", 800, 800, 1.0, 1.0),
];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Banniere {
    pub modele: usize,
    pub motif: usize,
    pub palette: usize,
    pub graine: u64,
    pub titre: String,
    pub sous_titre: String,
    pub couleur: [u8; 3],
    pub rond: bool,
}

impl Default for Banniere {
    fn default() -> Self {
        Banniere { modele: 0, motif: 2, palette: 0, graine: 7, titre: "MON SERVEUR".into(), sous_titre: String::new(), couleur: [255, 255, 255], rond: false }
    }
}

pub fn dessiner_banniere(b: &Banniere, logo: Option<&RgbaImage>, police: Option<&ab_glyph::FontVec>, fond: Option<&RgbaImage>) -> RgbaImage {
    let (_, l, h, sl, sh) = MODELES[b.modele.min(MODELES.len() - 1)];
    let mut img = match fond {
        Some(f) => crate::creatif3::couvrir(f, l, h),
        None => crate::creatif2::generer(crate::creatif2::Motif::TOUS[b.motif % 7], b.palette, b.graine, l, h),
    };
    // zone sûre (visible partout) : le contenu y est centré
    let (zl, zh) = ((l as f32 * sl) as u32, (h as f32 * sh) as u32);
    let (zx, zy) = ((l - zl) / 2, (h - zh) / 2);
    let mut x_texte = zx as f32 + zl as f32 * 0.06;
    if let Some(lg) = logo {
        let t = (zh as f32 * 0.7) as u32;
        let r = image::imageops::resize(lg, t, t, image::imageops::FilterType::Lanczos3);
        let ly = zy + (zh - t) / 2;
        let lx = if b.titre.trim().is_empty() { zx + (zl - t) / 2 } else { zx + (zl as f32 * 0.06) as u32 };
        image::imageops::overlay(&mut img, &r, lx as i64, ly as i64);
        x_texte = lx as f32 + t as f32 + zl as f32 * 0.04;
    }
    if let Some(f) = police {
        let place = zx as f32 + zl as f32 * 0.94 - x_texte;
        let mut taille = zh as f32 * if b.sous_titre.trim().is_empty() { 0.32 } else { 0.26 };
        while crate::creatif::largeur(f, taille, &b.titre) > place && taille > 12.0 {
            taille *= 0.92;
        }
        let centre = logo.is_none();
        let lt = crate::creatif::largeur(f, taille, &b.titre);
        let x = if centre { (l as f32 - lt) / 2.0 } else { x_texte };
        let y = zy as f32 + zh as f32 / 2.0 - taille * if b.sous_titre.trim().is_empty() { 0.55 } else { 0.85 };
        crate::creatif::ecrire(&mut img, f, taille, x, y, &b.titre, b.couleur, Some(([0, 0, 0], (taille / 18.0).max(2.0) as i32)));
        if !b.sous_titre.trim().is_empty() {
            let ts = taille * 0.42;
            let ls = crate::creatif::largeur(f, ts, &b.sous_titre);
            let xs = if centre { (l as f32 - ls) / 2.0 } else { x_texte };
            crate::creatif::ecrire(&mut img, f, ts, xs, y + taille * 1.15, &b.sous_titre, b.couleur, Some(([0, 0, 0], 2)));
        }
    }
    if b.rond && l == h {
        let r = l as f32 / 2.0;
        for (x, y, p) in img.enumerate_pixels_mut() {
            let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
            if d > r {
                p.0[3] = 0;
            } else if d > r - 1.0 {
                p.0[3] = ((r - d) * 255.0) as u8;
            }
        }
    }
    img
}

pub struct Bannieres {
    b: Banniere,
    logo: Option<RgbaImage>,
    fond: Option<RgbaImage>,
    police: Option<ab_glyph::FontVec>,
    apercu: Option<egui::TextureHandle>,
    sale: bool,
    message: String,
}

impl Bannieres {
    pub fn new() -> Self {
        Bannieres { b: charger("banniere"), logo: None, fond: None, police: None, apercu: None, sale: true, message: String::new() }
    }
}

impl Outil for Bannieres {
    fn nom(&self) -> &'static str {
        "🏳 Bannières et logos"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bannières et logos");
        if self.police.is_none() {
            self.police = crate::creatif::police(&["segoeuib.ttf", "arialbd.ttf", "impact.ttf"]);
        }
        let avant = self.b.clone();
        ui.horizontal_wrapped(|ui| {
            for (k, m) in MODELES.iter().enumerate() {
                ui.selectable_value(&mut self.b.modele, k, format!("{} ({}×{})", m.0, m.1, m.2));
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.b.titre).hint_text("titre").desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut self.b.sous_titre).hint_text("sous-titre").desired_width(220.0));
            ui.color_edit_button_srgb(&mut self.b.couleur);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Fond");
            for (k, n) in ["Montagnes", "Ville", "Pétales", "Bokeh", "Vagues", "Polygones", "Dégradé"].iter().enumerate() {
                if ui.selectable_label(self.b.motif == k && self.fond.is_none(), *n).clicked() {
                    self.b.motif = k;
                    self.fond = None;
                    self.sale = true;
                }
            }
            if ui.small_button("🎲").clicked() {
                self.b.graine = crate::outil::hasard(1 << 40);
                self.b.palette = crate::outil::hasard(6) as usize;
            }
            if ui.small_button("🖼 Mon image…").clicked() {
                self.fond = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file().and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
                self.sale = true;
            }
        });
        ui.horizontal(|ui| {
            if ui.button("🏷 Logo (PNG)…").clicked() {
                self.logo = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file().and_then(|p| image::open(p).ok()).map(|i| i.to_rgba8());
                self.sale = true;
            }
            if self.logo.is_some() && ui.small_button("✖ logo").clicked() {
                self.logo = None;
                self.sale = true;
            }
            let (_, l, h, ..) = MODELES[self.b.modele];
            if l == h {
                ui.checkbox(&mut self.b.rond, "Rond (transparent autour)");
            }
        });
        if self.b != avant {
            enregistrer("banniere", &self.b);
            self.sale = true;
        }
        let image = || dessiner_banniere(&self.b, self.logo.as_ref(), self.police.as_ref(), self.fond.as_ref());
        if self.sale {
            self.sale = false;
            let i = image();
            let a = image::imageops::thumbnail(&i, 640, (640 * i.height() / i.width()).max(1));
            self.apercu = Some(texture(ui.ctx(), "banniere", &a));
        }
        if let Some(t) = &self.apercu {
            let r = ui.add(egui::Image::new(t).fit_to_original_size(1.0)).rect;
            let (_, _, _, sl, sh) = MODELES[self.b.modele];
            if sl < 1.0 {
                let z = egui::Rect::from_center_size(r.center(), egui::vec2(r.width() * sl, r.height() * sh));
                ui.painter().rect_stroke(z, 0.0, egui::Stroke::new(1.5_f32, Color32::from_rgb(255, 120, 170)), egui::StrokeKind::Inside);
                ui.weak("Le cadre rose = ce qui est visible sur téléphone ; le reste n'apparaît que sur grand écran.");
            }
        }
        ui.horizontal(|ui| {
            if ui.button("💾 Enregistrer en PNG…").clicked() {
                if let Some(f) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("banniere.png").save_file() {
                    self.message = image().save(&f).map(|_| format!("✔ {}", f.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("📋 Copier").clicked() {
                crate::capture::win::copier_image(&image());
                self.message = "Copié.".into();
            }
        });
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 52 · Stickers et émojis Discord
// ---------------------------------------------------------------------------------------------------

/// Rend transparent le fond uni (couleur des coins, à `tolerance` près) en partant des bords.
pub fn retirer_fond(img: &mut RgbaImage, tolerance: u32) {
    let (l, h) = (img.width() as i64, img.height() as i64);
    let ref_ = *img.get_pixel(0, 0);
    let proche = |p: &Rgba<u8>| p.0[3] > 0 && (0..3).map(|k| (p.0[k] as i32 - ref_.0[k] as i32).unsigned_abs()).sum::<u32>() <= tolerance * 3;
    let mut vu = vec![false; (l * h) as usize];
    let mut pile: Vec<(i64, i64)> = (0..l).flat_map(|x| [(x, 0), (x, h - 1)]).chain((0..h).flat_map(|y| [(0, y), (l - 1, y)])).collect();
    while let Some((x, y)) = pile.pop() {
        if x < 0 || y < 0 || x >= l || y >= h || vu[(y * l + x) as usize] {
            continue;
        }
        vu[(y * l + x) as usize] = true;
        let p = img.get_pixel_mut(x as u32, y as u32);
        if !proche(p) {
            continue;
        }
        p.0[3] = 0;
        pile.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
}

/// Recadre au contenu non transparent.
pub fn recadrer(img: &RgbaImage) -> RgbaImage {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 8 {
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x0 > x1 {
        return img.clone();
    }
    image::imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

/// Met l'image dans un carré `cote` (marge comprise) et ajoute un contour blanc d'épaisseur `bord`.
pub fn sticker(img: &RgbaImage, cote: u32, bord: u32) -> RgbaImage {
    let marge = bord + 2;
    let place = cote - 2 * marge;
    let e = (place as f32 / img.width() as f32).min(place as f32 / img.height() as f32);
    let r = image::imageops::resize(img, ((img.width() as f32 * e) as u32).max(1), ((img.height() as f32 * e) as u32).max(1), image::imageops::FilterType::Lanczos3);
    let mut c = RgbaImage::new(cote, cote);
    let (ox, oy) = ((cote - r.width()) / 2, (cote - r.height()) / 2);
    if bord > 0 {
        let b = bord as i64;
        for (x, y, p) in r.enumerate_pixels() {
            if p.0[3] > 100 {
                for dy in -b..=b {
                    for dx in -b..=b {
                        if dx * dx + dy * dy <= b * b {
                            let (cx, cy) = (ox as i64 + x as i64 + dx, oy as i64 + y as i64 + dy);
                            if cx >= 0 && cy >= 0 && cx < cote as i64 && cy < cote as i64 {
                                c.put_pixel(cx as u32, cy as u32, Rgba([255, 255, 255, 255]));
                            }
                        }
                    }
                }
            }
        }
    }
    image::imageops::overlay(&mut c, &r, ox as i64, oy as i64);
    c
}

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut b = vec![];
    let enc = image::codecs::png::PngEncoder::new_with_quality(&mut b, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
    let _ = img.write_with_encoder(enc);
    b
}

pub struct Stickers {
    source: Option<RgbaImage>,
    retirer: bool,
    tolerance: u32,
    bord: u32,
    apercu: Option<(RgbaImage, egui::TextureHandle)>,
    sale: bool,
    message: String,
}

impl Stickers {
    pub fn new() -> Self {
        Stickers { source: None, retirer: true, tolerance: 30, bord: 8, apercu: None, sale: false, message: String::new() }
    }
}

impl Outil for Stickers {
    fn nom(&self) -> &'static str {
        "😺 Stickers et émojis Discord"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stickers et émojis Discord");
        ui.weak("Transforme une image en émoji (128×128, moins de 256 Ko) ou en sticker (320×320, moins de 512 Ko) avec fond transparent et contour blanc.");
        let depose = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
        let mut choisi = depose;
        if ui.button("🖼 Choisir une image…").clicked() {
            choisi = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"]).pick_file();
        }
        if let Some(i) = choisi.and_then(|p| image::open(p).ok()) {
            self.source = Some(i.to_rgba8());
            self.sale = true;
        }
        let Some(src) = &self.source else { return };
        ui.horizontal(|ui| {
            self.sale |= ui.checkbox(&mut self.retirer, "Retirer le fond uni").changed();
            if self.retirer {
                self.sale |= ui.add(egui::Slider::new(&mut self.tolerance, 0..=120).text("tolérance")).changed();
            }
            self.sale |= ui.add(egui::Slider::new(&mut self.bord, 0..=20).text("contour")).changed();
        });
        if self.sale {
            self.sale = false;
            let mut i = src.clone();
            if i.width() > 1024 || i.height() > 1024 {
                i = image::imageops::thumbnail(&i, 1024, 1024);
            }
            if self.retirer {
                retirer_fond(&mut i, self.tolerance);
            }
            let s = sticker(&recadrer(&i), 320, self.bord);
            let t = texture(ui.ctx(), "sticker", &s);
            self.apercu = Some((s, t));
        }
        if let Some((s, t)) = &self.apercu {
            let (r, _) = ui.allocate_exact_size(egui::vec2(320.0, 320.0), egui::Sense::hover());
            for y in 0..10 {
                for x in 0..10 {
                    let c = if (x + y) % 2 == 0 { Color32::from_gray(70) } else { Color32::from_gray(95) };
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(x as f32 * 32.0, y as f32 * 32.0), egui::vec2(32.0, 32.0)), 0.0, c);
                }
            }
            ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            ui.horizontal(|ui| {
                for (n, cote, max) in [("😀 Émoji 128×128", 128u32, 256u64 << 10), ("🏷 Sticker 320×320", 320, 512 << 10)] {
                    if ui.button(format!("💾 {n}")).clicked() {
                        if let Some(f) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(if cote == 128 { "emoji.png" } else { "sticker.png" }).save_file() {
                            let img = if cote == 320 { s.clone() } else { image::imageops::resize(s, 128, 128, image::imageops::FilterType::Lanczos3) };
                            let b = png(&img);
                            self.message = match std::fs::write(&f, &b) {
                                Ok(()) if b.len() as u64 <= max => format!("✔ {} ({})", f.display(), crate::disque::taille_lisible(b.len() as u64)),
                                Ok(()) => format!("⚠ Enregistré mais trop lourd pour Discord ({})", crate::disque::taille_lisible(b.len() as u64)),
                                Err(e) => e.to_string(),
                            };
                        }
                    }
                }
                if ui.button("📋 Copier").clicked() {
                    crate::capture::win::copier_image(s);
                }
            });
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 69 · Correcteur d'orthographe partout (LanguageTool)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Faute {
    pub debut: usize, // en unités UTF-16 (comme LanguageTool)
    pub longueur: usize,
    pub remplacement: Option<String>,
    pub message: String,
}

pub fn lire_fautes(j: &serde_json::Value) -> Vec<Faute> {
    j["matches"].as_array().into_iter().flatten().filter_map(|m| Some(Faute {
        debut: m["offset"].as_u64()? as usize,
        longueur: m["length"].as_u64()? as usize,
        remplacement: m["replacements"].as_array().and_then(|r| r.first()).and_then(|r| r["value"].as_str()).map(String::from),
        message: m["message"].as_str().unwrap_or("").to_string(),
    })).collect()
}

/// Applique la première suggestion de chaque faute (de la fin vers le début).
pub fn corriger(texte: &str, fautes: &[Faute]) -> String {
    let mut u: Vec<u16> = texte.encode_utf16().collect();
    let mut f: Vec<&Faute> = fautes.iter().filter(|x| x.remplacement.is_some()).collect();
    f.sort_by(|a, b| b.debut.cmp(&a.debut));
    let mut fin_prec = usize::MAX;
    for x in f {
        let fin = x.debut + x.longueur;
        if fin > u.len() || fin > fin_prec {
            continue; // chevauchement
        }
        u.splice(x.debut..fin, x.remplacement.as_ref().unwrap().encode_utf16());
        fin_prec = x.debut;
    }
    String::from_utf16_lossy(&u)
}

fn verifier(texte: &str) -> Result<Vec<Faute>, String> {
    let j: serde_json::Value = ureq::post("https://api.languagetool.org/v2/check").timeout(Duration::from_secs(20))
        .send_form(&[("text", texte), ("language", "auto"), ("preferredVariants", "fr")]).map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(lire_fautes(&j))
}

fn coller() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let t = |vk: u16, haut: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } } };
    let seq = [t(VK_CONTROL, false), t(b'V' as u16, false), t(b'V' as u16, true), t(VK_CONTROL, true)];
    unsafe { SendInput(4, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

pub struct Correcteur {
    actif: Arc<AtomicBool>,
    texte: String,
    fautes: Arc<Mutex<Option<Result<Vec<Faute>, String>>>>,
    pris: bool,
}

impl Correcteur {
    pub fn new() -> Self {
        let actif = Arc::new(AtomicBool::new(charger::<Option<bool>>("correcteur").unwrap_or(true)));
        let a = actif.clone();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        // Ctrl+Alt+Y sur un texte sélectionné : corrigé et remplacé
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            if crate::outil::raccourci(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'Y' as u32) == 0 {
                enregistrer("correcteur_pris", &Some(true));
                return;
            }
            enregistrer("correcteur_pris", &Some(false));
            let mut m: MSG = std::mem::zeroed();
            while GetMessageW(&mut m, std::ptr::null_mut(), 0, 0) > 0 {
                if m.message == WM_HOTKEY && a.load(Relaxed) {
                    let _ = tx.send(());
                }
            }
        });
        std::thread::spawn(move || {
            for () in rx {
                let Some(t) = crate::tradinstant::win::copier_selection() else { continue };
                let r = verifier(&t);
                match r {
                    Ok(f) if f.iter().any(|x| x.remplacement.is_some()) => {
                        let c = corriger(&t, &f);
                        if crate::pressepapiers::win::ecrire_texte(&c, None, None) {
                            std::thread::sleep(Duration::from_millis(60));
                            coller();
                        }
                        let n = f.iter().filter(|x| x.remplacement.is_some()).count();
                        crate::tradinstant::win::bulle(&format!("✔ {n} correction(s)"), Some(t));
                    }
                    Ok(_) => crate::tradinstant::win::bulle("✔ Aucune faute trouvée", None),
                    Err(e) => crate::tradinstant::win::bulle(&format!("Correcteur indisponible : {e}"), None),
                }
            }
        });
        Correcteur { actif, texte: String::new(), fautes: Arc::default(), pris: charger::<Option<bool>>("correcteur_pris").unwrap_or(false) }
    }
}

impl Outil for Correcteur {
    fn nom(&self) -> &'static str {
        "✔ Correcteur d'orthographe"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Correcteur d'orthographe partout");
        let mut a = self.actif.load(Relaxed);
        if ui.checkbox(&mut a, "Ctrl+Alt+Y sur un texte sélectionné (n'importe où) : il est corrigé et remplacé").changed() {
            self.actif.store(a, Relaxed);
            enregistrer("correcteur", &Some(a));
        }
        if self.pris {
            ui.colored_label(Color32::LIGHT_RED, "Ctrl+Alt+Y est déjà pris par une autre appli.");
        }
        ui.weak("Si la correction ne te plaît pas, Ctrl+Z dans l'appli remet ton texte. Le texte est vérifié par LanguageTool (service en ligne) : n'y mets rien de secret.");
        ui.separator();
        let r = ui.add(egui::TextEdit::multiline(&mut self.texte).hint_text("ou colle un texte ici").desired_rows(8).desired_width(f32::INFINITY));
        if r.changed() {
            *self.fautes.lock().unwrap() = None;
        }
        ui.horizontal(|ui| {
            if ui.button("🔎 Vérifier").clicked() && !self.texte.trim().is_empty() {
                let (t, f) = (self.texte.clone(), self.fautes.clone());
                *f.lock().unwrap() = Some(Err("vérification…".into()));
                std::thread::spawn(move || *f.lock().unwrap() = Some(verifier(&t)));
            }
            if let Some(Ok(f)) = self.fautes.lock().unwrap().clone() {
                if !f.is_empty() && ui.button("✔ Tout corriger").clicked() {
                    self.texte = corriger(&self.texte, &f);
                    *self.fautes.lock().unwrap() = None;
                }
            }
        });
        match self.fautes.lock().unwrap().clone() {
            Some(Ok(f)) if f.is_empty() => {
                ui.colored_label(Color32::LIGHT_GREEN, "✔ Aucune faute trouvée.");
            }
            Some(Ok(f)) => {
                let u: Vec<u16> = self.texte.encode_utf16().collect();
                for x in &f {
                    let mot = String::from_utf16_lossy(u.get(x.debut..x.debut + x.longueur).unwrap_or(&[]));
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(mot).color(Color32::LIGHT_RED).strikethrough());
                        if let Some(r) = &x.remplacement {
                            ui.label(RichText::new(format!("→ {r}")).color(Color32::LIGHT_GREEN));
                        }
                        ui.weak(&x.message);
                    });
                }
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 70 · Commandes vocales
// ---------------------------------------------------------------------------------------------------

/// Nom d'outil prononçable : sans émoji ni parenthèses, en minuscules.
pub fn prononcable(nom: &str) -> String {
    let n = regex::Regex::new(r"\([^)]*\)").unwrap().replace_all(nom, "");
    let n: String = n.chars().filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '\'' || *c == '-').collect();
    n.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ordre {
    Ouvrir(String),
    Lancer(String),
    Capture,
    Verrouiller,
    Veille,
    VolumePlus,
    VolumeMoins,
    Muet,
    Heure,
    Meteo,
}

/// Phrases reconnues → ordre.
pub fn phrases(outils: &[String], jeux: &[String]) -> Vec<(String, Ordre)> {
    let mut v = vec![
        ("outils capture d'écran".to_string(), Ordre::Capture),
        ("outils verrouille le pc".into(), Ordre::Verrouiller),
        ("outils mets le pc en veille".into(), Ordre::Veille),
        ("outils monte le son".into(), Ordre::VolumePlus),
        ("outils baisse le son".into(), Ordre::VolumeMoins),
        ("outils coupe le son".into(), Ordre::Muet),
        ("outils quelle heure est-il".into(), Ordre::Heure),
        ("outils quel temps fait-il".into(), Ordre::Meteo),
    ];
    for o in outils {
        let p = prononcable(o);
        if p.len() >= 3 {
            v.push((format!("outils ouvre {p}"), Ordre::Ouvrir(o.clone())));
        }
    }
    for j in jeux {
        let p = prononcable(j);
        if p.len() >= 2 {
            v.push((format!("outils lance {p}"), Ordre::Lancer(j.clone())));
        }
    }
    v
}

fn touche(vk: u16) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let t = |haut: bool| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } } };
    let s = [t(false), t(true)];
    unsafe { SendInput(2, s.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

fn executer(o: &Ordre, jeux: &[crate::lanceur::Jeu]) -> String {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    match o {
        Ordre::Ouvrir(n) => {
            crate::interface2::ouvrir_outil(n);
            format!("Ouverture de {}", prononcable(n))
        }
        Ordre::Lancer(n) => {
            if let Some(j) = jeux.iter().find(|j| j.nom == *n) {
                let _ = Command::new("cmd").args(["/c", "start", "", &j.cmd]).creation_flags(SANS_FENETRE).spawn();
            }
            format!("Lancement de {n}")
        }
        Ordre::Capture => {
            crate::interface2::ouvrir_outil("📸 Capture");
            "Capture".into()
        }
        Ordre::Verrouiller => {
            unsafe { windows_sys::Win32::System::Shutdown::LockWorkStation() };
            "Verrouillé".into()
        }
        Ordre::Veille => {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(2));
                unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
            });
            "Mise en veille".into()
        }
        Ordre::VolumePlus => {
            for _ in 0..5 {
                touche(VK_VOLUME_UP);
            }
            "Son plus fort".into()
        }
        Ordre::VolumeMoins => {
            for _ in 0..5 {
                touche(VK_VOLUME_DOWN);
            }
            "Son moins fort".into()
        }
        Ordre::Muet => {
            touche(VK_VOLUME_MUTE);
            "Son coupé".into()
        }
        Ordre::Heure => {
            let h = crate::minecraft::horodatage();
            let t = format!("Il est {} heures {}", h.get(9..11).unwrap_or("").parse::<u32>().unwrap_or(0), h.get(11..13).unwrap_or(""));
            dire(&t, "fr-FR");
            t
        }
        Ordre::Meteo => {
            crate::interface2::ouvrir_outil("🌤 Météo");
            "Météo".into()
        }
    }
}

pub struct CommandesVocales {
    actif: Arc<AtomicBool>,
    journal: Arc<Mutex<Vec<String>>>,
    marche: Arc<AtomicBool>,
    erreur: Arc<Mutex<String>>,
}

impl CommandesVocales {
    pub fn new() -> Self {
        let c = CommandesVocales { actif: Arc::new(AtomicBool::new(charger::<Option<bool>>("commandes_vocales").unwrap_or(false))), journal: Arc::default(), marche: Arc::default(), erreur: Arc::default() };
        if c.actif.load(Relaxed) {
            c.demarrer();
        }
        c
    }

    fn demarrer(&self) {
        if self.marche.swap(true, Relaxed) {
            return;
        }
        let (a, j, m, err) = (self.actif.clone(), self.journal.clone(), self.marche.clone(), self.erreur.clone());
        std::thread::spawn(move || {
            // les noms d'outils arrivent quand la fenêtre principale a démarré
            std::thread::sleep(Duration::from_secs(3));
            let outils = crate::interface2::NOMS.lock().unwrap().clone();
            let jeux = crate::jeux4::jeux_installes();
            let ph = phrases(&outils, &jeux.iter().map(|g| g.nom.clone()).collect::<Vec<_>>());
            let liste: Vec<String> = ph.iter().map(|p| format!("'{}'", p.0.replace('\'', "''"))).collect();
            let script = format!(r#"[Console]::OutputEncoding=[Text.Encoding]::UTF8
Add-Type -AssemblyName System.Speech
try {{ $r = New-Object System.Speech.Recognition.SpeechRecognitionEngine((New-Object System.Globalization.CultureInfo('fr-FR'))) }} catch {{ [Console]::Out.WriteLine('ERREUR:aucune reconnaissance vocale française installée'); exit }}
$c = New-Object System.Speech.Recognition.Choices
$c.Add([string[]]@({}))
$g = New-Object System.Speech.Recognition.GrammarBuilder($c)
$g.Culture = $r.RecognizerInfo.Culture
$r.LoadGrammar((New-Object System.Speech.Recognition.Grammar($g)))
$r.SetInputToDefaultAudioDevice()
while ($true) {{ $x = $r.Recognize(); if ($x -and $x.Confidence -gt 0.6) {{ [Console]::Out.WriteLine('OK|' + $x.Text); [Console]::Out.Flush() }} }}"#, liste.join(","));
            // script dans un fichier UTF-8 avec BOM : la liste des phrases dépasserait la longueur
            // maximale d'une ligne de commande, et sans BOM PowerShell 5 casse les accents
            let fichier = std::env::temp_dir().join("outils_commandes_vocales.ps1");
            let mut contenu = vec![0xEF, 0xBB, 0xBF];
            contenu.extend_from_slice(script.as_bytes());
            if std::fs::write(&fichier, contenu).is_err() {
                *err.lock().unwrap() = "impossible d'écrire le script de reconnaissance".into();
                m.store(false, Relaxed);
                return;
            }
            let enfant = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"]).arg(&fichier).stdout(Stdio::piped()).stdin(Stdio::null()).stderr(Stdio::null()).creation_flags(SANS_FENETRE).spawn();
            let Ok(mut enfant) = enfant else {
                *err.lock().unwrap() = "PowerShell introuvable".into();
                m.store(false, Relaxed);
                return;
            };
            let sortie = enfant.stdout.take().unwrap();
            // décocher « Écouter » arrête tout de suite le micro (sans attendre la prochaine phrase)
            let enfant = Arc::new(Mutex::new(enfant));
            {
                let (e, a) = (enfant.clone(), a.clone());
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(400));
                    let mut c = e.lock().unwrap();
                    if !a.load(Relaxed) {
                        let _ = c.kill();
                        return;
                    }
                    if matches!(c.try_wait(), Ok(Some(_))) {
                        return;
                    }
                });
            }
            for l in std::io::BufReader::new(sortie).lines().map_while(Result::ok) {
                if !a.load(Relaxed) {
                    break;
                }
                if let Some(e) = l.strip_prefix("ERREUR:") {
                    *err.lock().unwrap() = e.to_string();
                    break;
                }
                if let Some(t) = l.strip_prefix("OK|") {
                    if let Some((_, o)) = ph.iter().find(|p| p.0 == t) {
                        let r = executer(o, &jeux);
                        let mut jj = j.lock().unwrap();
                        jj.push(format!("🎙 « {t} » → {r}"));
                        if jj.len() > 30 {
                            jj.remove(0);
                        }
                    }
                }
            }
            let _ = enfant.lock().unwrap().kill();
            m.store(false, Relaxed);
        });
    }
}

impl Outil for CommandesVocales {
    fn nom(&self) -> &'static str {
        "🎙 Commandes vocales"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Commandes vocales");
        ui.weak("Parle à Outils : chaque phrase commence par « Outils ». Tout est reconnu sur le PC, sans internet.");
        let mut a = self.actif.load(Relaxed);
        if ui.checkbox(&mut a, "Écouter en permanence (micro par défaut)").changed() {
            self.actif.store(a, Relaxed);
            enregistrer("commandes_vocales", &Some(a));
            if a {
                self.erreur.lock().unwrap().clear();
                self.demarrer();
            }
        }
        let e = self.erreur.lock().unwrap().clone();
        if !e.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, format!("{e} (Paramètres › Heure et langue › Voix : ajoute la reconnaissance vocale française)"));
        } else if self.marche.load(Relaxed) {
            ui.colored_label(Color32::LIGHT_GREEN, "🎙 J'écoute…");
        }
        egui::Grid::new("phrases_vocales").striped(true).show(ui, |ui| {
            for (p, d) in [("Outils, ouvre la météo", "ouvre n'importe quel outil par son nom"), ("Outils, lance Minecraft", "lance un jeu de ta bibliothèque"), ("Outils, capture d'écran", ""), ("Outils, monte / baisse / coupe le son", ""), ("Outils, verrouille le PC", ""), ("Outils, mets le PC en veille", ""), ("Outils, quelle heure est-il", "répond à voix haute")] {
                ui.label(RichText::new(p).strong());
                ui.weak(d);
                ui.end_row();
            }
        });
        for l in self.journal.lock().unwrap().iter().rev() {
            ui.label(l);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_et_texte() {
        assert_eq!(echapper_drawtext("Salut: 100% l'été, ok"), "Salut\\: 100\\% l\u{2019}été\\, ok");
        let clips = vec![
            Clip { fichier: "a.mp4".into(), duree: 10.0, debut: 1.0, fin: 4.0, son: true },
            Clip { fichier: "b.mp4".into(), duree: 5.0, debut: 0.0, fin: 5.0, son: false },
        ];
        let a = arguments_montage(&clips, (1280, 720), "Fin", "C\\:/Windows/Fonts/arial.ttf", Some((Path::new("m.mp3"), 0.3)), Path::new("out.mp4"));
        let f = &a[a.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(f.contains("[0:a]aresample") && f.contains("[2:a]aresample") && f.contains("concat=n=2:v=1:a=1[vc][ac]"));
        assert!(f.contains("[3:a]volume=0.30[mu]") && f.contains("drawtext"));
        assert!(a.contains(&"anullsrc=r=44100:cl=stereo".to_string()));
        assert_eq!(a.last().unwrap(), "out.mp4");
        assert!(filtre_meme("quand", "", "x").contains("text='QUAND'"));
        let j: serde_json::Value = serde_json::from_str(r#"{"matches":[{"offset":3,"length":4,"message":"Faute","replacements":[{"value":"suis"}]},{"offset":12,"length":4,"message":"x","replacements":[]}]}"#).unwrap();
        let f = lire_fautes(&j);
        assert_eq!(f.len(), 2);
        assert_eq!(corriger("Je suit très conten", &f[..1]), "Je suis très conten");
        assert_eq!(corriger("é ab", &[Faute { debut: 2, longueur: 2, remplacement: Some("xy".into()), message: String::new() }]), "é xy");
        assert_eq!(prononcable("🌤 Météo"), "météo");
        assert_eq!(prononcable("🎛 Profils (Jeu, Travail, Nuit)"), "profils");
        let p = phrases(&["🌤 Météo".into()], &["Minecraft".into()]);
        assert!(p.iter().any(|x| x.0 == "outils ouvre météo") && p.iter().any(|x| x.0 == "outils lance minecraft"));
    }

    #[test]
    fn images() {
        let mut i = RgbaImage::from_pixel(20, 20, Rgba([255, 255, 255, 255]));
        for y in 5..15 {
            for x in 5..15 {
                i.put_pixel(x, y, Rgba([200, 0, 0, 255]));
            }
        }
        retirer_fond(&mut i, 10);
        assert_eq!(i.get_pixel(0, 0).0[3], 0);
        assert_eq!(i.get_pixel(10, 10).0[3], 255);
        let r = recadrer(&i);
        assert_eq!((r.width(), r.height()), (10, 10));
        let s = sticker(&r, 128, 4);
        assert_eq!((s.width(), s.height()), (128, 128));
        assert_eq!(s.get_pixel(0, 0).0[3], 0);
        assert_eq!(s.get_pixel(64, 64).0, [200, 0, 0, 255]);
    }
}
