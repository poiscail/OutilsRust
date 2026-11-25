//! Son et image en direct : visualiseur musical (26), karaoké avec score (21), stop motion à la webcam (23).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, RichText, Stroke};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------------
// Capture du son (Windows : WASAPI ; Linux : PulseAudio / PipeWire avec parec)
// ---------------------------------------------------------------------------------------------------

/// Passe le son (mono) au rappel jusqu'à `stop`. `micro` : le micro, sinon ce qui sort des haut-parleurs.
pub fn capturer(micro: bool, stop: Arc<AtomicBool>, mut rappel: impl FnMut(&[f32], u32) + Send + 'static) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::son::flux_audio(&stop, micro, &mut rappel)
    }
    #[cfg(not(windows))]
    {
        let mut c = std::process::Command::new("parec");
        c.args(["--raw", "--format=float32le", "--channels=1", "--rate=44100", "--latency-msec=30"]);
        if !micro {
            c.args(["-d", "@DEFAULT_MONITOR@"]);
        }
        let mut enfant = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().map_err(|_| "parec introuvable (paquet pulseaudio-utils)".to_string())?;
        let mut sortie = enfant.stdout.take().ok_or("pas de sortie")?;
        let mut b = vec![0u8; 4096];
        let mut v = Vec::with_capacity(1024);
        while !stop.load(Relaxed) {
            let n = sortie.read(&mut b).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            v.clear();
            v.extend(b[..n - n % 4].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])));
            rappel(&v, 44100);
        }
        let _ = enfant.kill();
        Ok(())
    }
}

/// FFT radix 2 en place (re, im de taille puissance de 2).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut l = 2;
    while l <= n {
        let a = -TAU / l as f32;
        for d in (0..n).step_by(l) {
            for k in 0..l / 2 {
                let (s, c) = (a * k as f32).sin_cos();
                let (x, y) = (re[d + k + l / 2], im[d + k + l / 2]);
                let (tr, ti) = (x * c - y * s, x * s + y * c);
                re[d + k + l / 2] = re[d + k] - tr;
                im[d + k + l / 2] = im[d + k] - ti;
                re[d + k] += tr;
                im[d + k] += ti;
            }
        }
        l <<= 1;
    }
}

/// Niveaux (0..1) de `n` bandes de fréquences espacées logarithmiquement entre 40 Hz et 16 kHz.
pub fn bandes(e: &[f32], freq: u32, n: usize) -> Vec<f32> {
    let t = e.len().next_power_of_two().min(4096);
    let mut re: Vec<f32> = (0..t).map(|k| e.get(k).copied().unwrap_or(0.0) * (0.5 - 0.5 * (TAU * k as f32 / t as f32).cos())).collect();
    let mut im = vec![0.0; t];
    fft(&mut re, &mut im);
    let module: Vec<f32> = (0..t / 2).map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() / t as f32).collect();
    let (f0, f1) = (40f32, 16000f32.min(freq as f32 / 2.0));
    (0..n)
        .map(|b| {
            let a = f0 * (f1 / f0).powf(b as f32 / n as f32);
            let z = f0 * (f1 / f0).powf((b + 1) as f32 / n as f32);
            let (ka, kz) = ((a * t as f32 / freq as f32) as usize, ((z * t as f32 / freq as f32) as usize).max((a * t as f32 / freq as f32) as usize + 1));
            let m = module[ka.min(module.len() - 1)..kz.min(module.len())].iter().fold(0.0f32, |x, y| x.max(*y));
            // en décibels, ramené à 0..1
            ((20.0 * (m + 1e-6).log10() + 70.0) / 60.0).clamp(0.0, 1.0)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------
// 26 · Visualiseur musical
// ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Signal {
    echantillons: Vec<f32>,
    freq: u32,
}

pub struct Visualiseur {
    signal: Arc<Mutex<Signal>>,
    stop: Arc<AtomicBool>,
    marche: bool,
    niveaux: Vec<f32>,
    mode: u8,
    teinte: f32,
    erreur: Arc<Mutex<String>>,
}

impl Visualiseur {
    pub fn new() -> Self {
        Visualiseur { signal: Arc::default(), stop: Arc::new(AtomicBool::new(false)), marche: false, niveaux: vec![0.0; 64], mode: charger::<Option<u8>>("visualiseur").unwrap_or(0), teinte: 0.0, erreur: Arc::default() }
    }

    fn demarrer(&mut self) {
        self.stop = Arc::new(AtomicBool::new(false));
        let (s, st, e) = (self.signal.clone(), self.stop.clone(), self.erreur.clone());
        std::thread::spawn(move || {
            let r = capturer(false, st, move |v, f| {
                let mut x = s.lock().unwrap();
                x.freq = f;
                x.echantillons.extend_from_slice(v);
                let n = x.echantillons.len();
                if n > 4096 {
                    x.echantillons.drain(..n - 4096);
                }
            });
            if let Err(m) = r {
                *e.lock().unwrap() = m;
            }
        });
        self.marche = true;
    }
}

impl Drop for Visualiseur {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

impl Outil for Visualiseur {
    fn nom(&self) -> &'static str {
        "🌈 Visualiseur musical"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.marche {
            self.demarrer();
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Visualiseur musical");
            for (k, n) in ["📊 Barres", "⭕ Cercle", "〰 Onde", "🔥 Miroir"].iter().enumerate() {
                if ui.selectable_label(self.mode == k as u8, *n).clicked() {
                    self.mode = k as u8;
                    enregistrer("visualiseur", &Some(self.mode));
                }
            }
            ui.weak("Mets de la musique n'importe où sur le PC.");
        });
        let e = self.erreur.lock().unwrap().clone();
        if !e.is_empty() {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
        let (onde, freq) = {
            let s = self.signal.lock().unwrap();
            (s.echantillons.clone(), s.freq.max(8000))
        };
        let n = 64;
        let nouveaux = if onde.len() >= 1024 { bandes(&onde[onde.len() - 2048.min(onde.len())..], freq, n) } else { vec![0.0; n] };
        for (a, b) in self.niveaux.iter_mut().zip(nouveaux) {
            // monte vite, retombe doucement
            *a = if b > *a { b } else { *a * 0.88 + b * 0.12 };
        }
        let energie = self.niveaux.iter().take(8).sum::<f32>() / 8.0;
        self.teinte = (self.teinte + 0.002 + energie * 0.01) % 1.0;
        let (r, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        let p = ui.painter_at(r);
        p.rect_filled(r, 8.0, Color32::from_rgb(8, 8, 14));
        let couleur = |k: usize, v: f32| Color32::from(egui::ecolor::Hsva::new((self.teinte + k as f32 / n as f32 * 0.5) % 1.0, 0.85, 0.4 + 0.6 * v, 1.0));
        match self.mode {
            0 | 3 => {
                let l = r.width() / n as f32;
                for (k, v) in self.niveaux.iter().enumerate() {
                    let h = v * r.height() * if self.mode == 3 { 0.48 } else { 0.95 };
                    let x = r.left() + k as f32 * l;
                    let rect = if self.mode == 3 { egui::Rect::from_center_size(Pos2::new(x + l / 2.0, r.center().y), egui::vec2(l - 2.0, h * 2.0)) } else { egui::Rect::from_min_max(Pos2::new(x + 1.0, r.bottom() - h), Pos2::new(x + l - 1.0, r.bottom())) };
                    p.rect_filled(rect, 2.0, couleur(k, *v));
                }
            }
            1 => {
                let c = r.center();
                let base = r.width().min(r.height()) * 0.22 * (1.0 + energie * 0.3);
                for (k, v) in self.niveaux.iter().enumerate() {
                    for sens in [1.0, -1.0] {
                        let a = sens * (k as f32 + 0.5) / n as f32 * TAU / 2.0 - TAU / 4.0;
                        let d = egui::vec2(a.cos(), a.sin());
                        p.line_segment([c + d * base, c + d * (base + v * base * 1.3)], Stroke::new(r.width() / 200.0 + 2.0, couleur(k, *v)));
                    }
                }
                p.circle_stroke(c, base * 0.95, Stroke::new(2.0_f32, couleur(0, energie)));
            }
            _ => {
                let pts: Vec<Pos2> = onde.iter().rev().take(1024).rev().enumerate().map(|(k, v)| Pos2::new(r.left() + k as f32 / 1024.0 * r.width(), r.center().y - v.clamp(-1.0, 1.0) * r.height() * 0.45)).collect();
                if pts.len() > 1 {
                    p.add(egui::Shape::line(pts, Stroke::new(3.0_f32, couleur(10, 0.9))));
                }
            }
        }
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------------
// 21 · Karaoké
// ---------------------------------------------------------------------------------------------------

/// Paroles synchronisées « [mm:ss.xx] texte » → (secondes, texte), triées.
pub fn lire_lrc(t: &str) -> Vec<(f32, String)> {
    let mut v = Vec::new();
    for l in t.lines() {
        let mut reste = l.trim();
        let mut temps = Vec::new();
        while let Some(r) = reste.strip_prefix('[') {
            let Some((dedans, apres)) = r.split_once(']') else { break };
            if let Some((m, s)) = dedans.split_once(':') {
                if let (Ok(m), Ok(s)) = (m.trim().parse::<f32>(), s.trim().replace(',', ".").parse::<f32>()) {
                    temps.push(m * 60.0 + s);
                }
            }
            reste = apres;
        }
        for t in temps {
            v.push((t, reste.trim().to_string()));
        }
    }
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Hauteur fondamentale (Hz) d'un bloc par autocorrélation, si le son est assez « chanté ».
pub fn hauteur(e: &[f32], freq: u32) -> Option<f32> {
    let n = e.len();
    let energie: f32 = e.iter().map(|x| x * x).sum::<f32>() / n.max(1) as f32;
    if n < 256 || energie < 1e-5 {
        return None;
    }
    let (min, max) = ((freq as f32 / 1000.0) as usize, ((freq as f32 / 80.0) as usize).min(n / 2));
    let r0: f32 = e.iter().map(|x| x * x).sum();
    let mut meilleur = (0usize, 0f32);
    for lag in min.max(2)..max {
        let r: f32 = e[..n - lag].iter().zip(&e[lag..]).map(|(a, b)| a * b).sum::<f32>() / r0;
        if r > meilleur.1 {
            meilleur = (lag, r);
        }
    }
    (meilleur.1 > 0.5).then(|| freq as f32 / meilleur.0 as f32)
}

/// Écart en demi-tons entre deux hauteurs, sans tenir compte de l'octave (0..6).
pub fn ecart_notes(a: f32, b: f32) -> f32 {
    let d = (12.0 * (a / b).log2()).rem_euclid(12.0);
    d.min(12.0 - d)
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct ResultatLrc {
    #[serde(rename = "trackName")]
    titre: String,
    #[serde(rename = "artistName")]
    artiste: String,
    #[serde(rename = "syncedLyrics")]
    synchro: Option<String>,
}

#[derive(Default)]
struct Preparation {
    instrumental: Option<PathBuf>,
    melodie: Vec<Option<f32>>, // hauteur toutes les 50 ms
    duree: f32,
    message: String,
    en_cours: bool,
}

pub struct Karaoke {
    fichier: Option<PathBuf>,
    recherche: String,
    resultats: Arc<Mutex<Vec<ResultatLrc>>>,
    paroles: Vec<(f32, String)>,
    prep: Arc<Mutex<Preparation>>,
    sans_voix: bool,
    lecture: Option<Instant>,
    stop: Arc<AtomicBool>,
    micro: Arc<Mutex<Vec<f32>>>,
    micro_freq: Arc<Mutex<u32>>,
    score: (u32, u32),
    dernier_tic: usize,
    meilleurs: Vec<(String, u32)>,
}

impl Karaoke {
    pub fn new() -> Self {
        Karaoke { fichier: None, recherche: String::new(), resultats: Arc::default(), paroles: Vec::new(), prep: Arc::default(), sans_voix: true, lecture: None, stop: Arc::new(AtomicBool::new(false)), micro: Arc::default(), micro_freq: Arc::new(Mutex::new(48000)), score: (0, 0), dernier_tic: 0, meilleurs: charger("karaoke_scores") }
    }

    fn preparer(&self) {
        let Some(f) = self.fichier.clone() else { return };
        let (p, sans_voix) = (self.prep.clone(), self.sans_voix);
        p.lock().unwrap().en_cours = true;
        p.lock().unwrap().message = "Préparation de la chanson…".into();
        std::thread::spawn(move || {
            let r = (|| -> Result<(PathBuf, Vec<Option<f32>>, f32), String> {
                let wav = std::env::temp_dir().join("outils_karaoke.wav");
                let mut c = crate::createur::ffmpeg()?;
                c.args(["-y", "-v", "error", "-i"]).arg(&f).arg("-vn");
                if sans_voix {
                    // la voix est au centre : gauche − droite l'enlève
                    c.args(["-af", "pan=stereo|c0=c0-c1|c1=c1-c0"]);
                }
                c.args(["-ar", "44100", "-c:a", "pcm_s16le"]).arg(&wav);
                let o = c.output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec").to_string());
                }
                // mélodie : la partie centrale (voix) en mono 8 kHz
                let mut c = crate::createur::ffmpeg()?;
                c.args(["-v", "error", "-i"]).arg(&f).args(["-vn", "-af", "pan=mono|c0=0.5*c0+0.5*c1,highpass=f=150,lowpass=f=1200", "-ar", "8000", "-f", "f32le", "-"]);
                let o = c.output().map_err(|e| e.to_string())?;
                let e: Vec<f32> = o.stdout.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
                let melodie = (0..e.len().saturating_sub(1024)).step_by(400).map(|k| hauteur(&e[k..k + 1024], 8000)).collect();
                Ok((wav, melodie, e.len() as f32 / 8000.0))
            })();
            let mut x = p.lock().unwrap();
            x.en_cours = false;
            match r {
                Ok((w, m, d)) => {
                    x.instrumental = Some(w);
                    x.melodie = m;
                    x.duree = d;
                    x.message = "✔ Prêt : lance la chanson et chante !".into();
                }
                Err(e) => x.message = format!("✖ {e}"),
            }
        });
    }

    fn chercher(&self) {
        let (q, r) = (self.recherche.clone(), self.resultats.clone());
        std::thread::spawn(move || {
            let v: Vec<ResultatLrc> = ureq::get("https://lrclib.net/api/search").query("q", &q).set("User-Agent", "Outils (karaoke)").call().ok().and_then(|x| x.into_json().ok()).unwrap_or_default();
            *r.lock().unwrap() = v.into_iter().filter(|x| x.synchro.as_ref().is_some_and(|s| !s.is_empty())).take(8).collect();
        });
    }

    fn jouer(&mut self) {
        let Some(w) = self.prep.lock().unwrap().instrumental.clone() else { return };
        self.arreter();
        self.stop = Arc::new(AtomicBool::new(false));
        let st = self.stop.clone();
        jouer_fichier(w, st.clone());
        let (m, mf) = (self.micro.clone(), self.micro_freq.clone());
        m.lock().unwrap().clear();
        std::thread::spawn(move || {
            let _ = capturer(true, st, move |v, f| {
                *mf.lock().unwrap() = f;
                let mut x = m.lock().unwrap();
                x.extend_from_slice(v);
                let n = x.len();
                if n > 16384 {
                    x.drain(..n - 16384);
                }
            });
        });
        self.lecture = Some(Instant::now());
        self.score = (0, 0);
        self.dernier_tic = 0;
    }

    fn arreter(&mut self) {
        self.stop.store(true, Relaxed);
        self.lecture = None;
    }
}

/// Joue un fichier WAV jusqu'à `stop`.
fn jouer_fichier(f: PathBuf, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            let Ok((_flux, h)) = rodio::OutputStream::try_default() else { return };
            let Ok(sink) = rodio::Sink::try_new(&h) else { return };
            let Ok(fichier) = std::fs::File::open(&f) else { return };
            let Ok(d) = rodio::Decoder::new(std::io::BufReader::new(fichier)) else { return };
            sink.append(d);
            while !sink.empty() && !stop.load(Relaxed) {
                std::thread::sleep(Duration::from_millis(30));
            }
            sink.stop();
        }
        #[cfg(not(windows))]
        {
            let Ok(mut c) = std::process::Command::new("paplay").arg(&f).spawn().or_else(|_| std::process::Command::new("aplay").arg("-q").arg(&f).spawn()) else { return };
            while !stop.load(Relaxed) {
                if matches!(c.try_wait(), Ok(Some(_))) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            let _ = c.kill();
        }
    });
}

impl Drop for Karaoke {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

impl Outil for Karaoke {
    fn nom(&self) -> &'static str {
        "🎤 Karaoké"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Karaoké");
        ui.horizontal_wrapped(|ui| {
            if ui.button("🎵 Choisir une chanson…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Musique", &["mp3", "m4a", "flac", "wav", "ogg", "opus", "webm", "mp4"]).pick_file() {
                    self.recherche = p.file_stem().unwrap_or_default().to_string_lossy().replace(['_', '-'], " ");
                    self.fichier = Some(p);
                    self.paroles.clear();
                    self.prep.lock().unwrap().instrumental = None;
                    self.preparer();
                    self.chercher();
                }
            }
            if let Some(f) = &self.fichier {
                ui.label(f.file_name().unwrap_or_default().to_string_lossy());
            }
            if ui.checkbox(&mut self.sans_voix, "Enlever la voix du chanteur").changed() && self.fichier.is_some() {
                self.preparer();
            }
        });
        let (pret, msg, en_cours) = {
            let p = self.prep.lock().unwrap();
            (p.instrumental.is_some(), p.message.clone(), p.en_cours)
        };
        ui.horizontal(|ui| {
            if en_cours {
                ui.spinner();
            }
            ui.label(msg);
        });
        if self.fichier.is_some() && self.paroles.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Paroles :");
                ui.add(egui::TextEdit::singleline(&mut self.recherche).desired_width(260.0).hint_text("artiste titre"));
                if ui.button("🔎 Chercher").clicked() {
                    self.chercher();
                }
                if ui.button("📂 Fichier .lrc…").clicked() {
                    if let Some(t) = rfd::FileDialog::new().add_filter("Paroles", &["lrc"]).pick_file().and_then(|p| std::fs::read_to_string(p).ok()) {
                        self.paroles = lire_lrc(&t);
                    }
                }
            });
            let res = self.resultats.lock().unwrap().clone();
            for r in res {
                if ui.button(format!("📝 {} — {}", r.artiste, r.titre)).clicked() {
                    self.paroles = lire_lrc(r.synchro.as_deref().unwrap_or_default());
                }
            }
            ui.weak("Les paroles synchronisées viennent de lrclib.net (base libre).");
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(pret && self.lecture.is_none(), egui::Button::new(RichText::new("▶ Chanter").size(18.0))).clicked() {
                self.jouer();
            }
            if ui.add_enabled(self.lecture.is_some(), egui::Button::new(RichText::new("⏹ Stop").size(18.0))).clicked() {
                self.arreter();
            }
            if self.score.1 > 0 {
                ui.label(RichText::new(format!("🎯 Score : {} %", self.score.0 * 100 / self.score.1)).size(18.0).strong());
            }
        });
        let Some(debut) = self.lecture else {
            if !self.meilleurs.is_empty() {
                ui.separator();
                ui.strong("🏆 Meilleurs scores");
                for (c, s) in self.meilleurs.iter().take(8) {
                    ui.label(format!("{s} % — {c}"));
                }
            }
            return;
        };
        let t = debut.elapsed().as_secs_f32();
        let duree = self.prep.lock().unwrap().duree;
        // score toutes les 50 ms : la note chantée est-elle celle de la chanson ?
        let tic = (t / 0.05) as usize;
        let ligne = self.paroles.iter().rposition(|(d, _)| *d <= t);
        let chante = ligne.is_some_and(|k| !self.paroles[k].1.is_empty() && self.paroles.get(k + 1).is_none_or(|n| n.0 > t));
        if tic > self.dernier_tic {
            self.dernier_tic = tic;
            let melodie = self.prep.lock().unwrap().melodie.get(tic).copied().flatten();
            if let (true, Some(m)) = (chante || self.paroles.is_empty(), melodie) {
                let f = *self.micro_freq.lock().unwrap();
                let bloc: Vec<f32> = {
                    let x = self.micro.lock().unwrap();
                    let pas = (f / 8000).max(1) as usize;
                    x.iter().rev().take(1024 * pas).rev().step_by(pas).copied().collect()
                };
                self.score.1 += 1;
                if hauteur(&bloc, f / (f / 8000).max(1)).is_some_and(|v| ecart_notes(v, m) <= 1.0) {
                    self.score.0 += 1;
                }
            }
        }
        if t > duree + 1.0 && duree > 0.0 {
            self.arreter();
            if self.score.1 > 20 {
                let nom = self.fichier.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                self.meilleurs.push((nom, self.score.0 * 100 / self.score.1));
                self.meilleurs.sort_by(|a, b| b.1.cmp(&a.1));
                self.meilleurs.truncate(20);
                enregistrer("karaoke_scores", &self.meilleurs);
            }
        }
        ui.add(egui::ProgressBar::new(if duree > 0.0 { t / duree } else { 0.0 }).show_percentage());
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ui.available_height().max(260.0)), egui::Sense::hover());
        let p = ui.painter_at(r);
        p.rect_filled(r, 10.0, Color32::from_rgb(20, 12, 40));
        match ligne {
            None if !self.paroles.is_empty() => {
                let reste = self.paroles[0].0 - t;
                p.text(r.center(), egui::Align2::CENTER_CENTER, format!("♪ {:.0}…", reste.max(0.0)), FontId::proportional(36.0), Color32::WHITE);
            }
            None => {
                p.text(r.center(), egui::Align2::CENTER_CENTER, "♪ (pas de paroles)", FontId::proportional(30.0), Color32::GRAY);
            }
            Some(k) => {
                for (d, j) in (-1i32..=3).enumerate() {
                    let i = k as i32 + j;
                    if i < 0 || i as usize >= self.paroles.len() {
                        continue;
                    }
                    let texte = &self.paroles[i as usize].1;
                    let y = r.top() + r.height() * (0.18 + d as f32 * 0.18);
                    let (taille, couleur) = if j == 0 { (40.0, Color32::from_rgb(255, 220, 80)) } else { (24.0, Color32::from_white_alpha(if j < 0 { 60 } else { 150 })) };
                    p.text(Pos2::new(r.center().x, y), egui::Align2::CENTER_CENTER, texte, FontId::proportional(taille), couleur);
                }
            }
        }
        ui.ctx().request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------------
// 23 · Stop motion
// ---------------------------------------------------------------------------------------------------

/// Webcams disponibles : (nom affiché, argument d'entrée ffmpeg).
pub fn webcams() -> Vec<(String, Vec<String>)> {
    #[cfg(windows)]
    {
        let Ok(mut c) = crate::createur::ffmpeg() else { return Vec::new() };
        let o = c.args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]).output();
        let texte = o.map(|o| String::from_utf8_lossy(&o.stderr).to_string()).unwrap_or_default();
        texte
            .lines()
            .filter(|l| l.contains("(video)"))
            .filter_map(|l| {
                let n = l.split('"').nth(1)?.to_string();
                Some((n.clone(), vec!["-f".into(), "dshow".into(), "-i".into(), format!("video={n}")]))
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut v: Vec<(String, Vec<String>)> = std::fs::read_dir("/dev")
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("video")))
            .map(|p| (p.display().to_string(), vec!["-f".into(), "v4l2".into(), "-i".into(), p.display().to_string()]))
            .collect();
        v.sort();
        v
    }
}

/// Sépare les images JPEG d'un flux MJPEG (renvoie les images complètes, garde le reste).
pub fn decouper_jpeg(tampon: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    loop {
        let Some(d) = tampon.windows(2).position(|w| w == [0xFF, 0xD8]) else {
            tampon.clear();
            break;
        };
        let Some(f) = tampon[d + 2..].windows(2).position(|w| w == [0xFF, 0xD9]) else {
            tampon.drain(..d);
            break;
        };
        let fin = d + 2 + f + 2;
        v.push(tampon[d..fin].to_vec());
        tampon.drain(..fin);
    }
    v
}

fn dossier_videos() -> PathBuf {
    let maison = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    let p = PathBuf::from(maison);
    if p.join("Vidéos").is_dir() { p.join("Vidéos") } else { p.join("Videos") }
}

pub struct StopMotion {
    cameras: Vec<(String, Vec<String>)>,
    camera: usize,
    direct: Arc<Mutex<Option<RgbaImage>>>,
    stop: Arc<AtomicBool>,
    images: Vec<RgbaImage>,
    vignettes: Vec<egui::TextureHandle>,
    tex_direct: Option<egui::TextureHandle>,
    pelure: bool,
    fps: u32,
    lecture: Option<Instant>,
    message: Arc<Mutex<String>>,
    cherche: bool,
}

impl StopMotion {
    pub fn new() -> Self {
        StopMotion { cameras: Vec::new(), camera: 0, direct: Arc::default(), stop: Arc::new(AtomicBool::new(true)), images: Vec::new(), vignettes: Vec::new(), tex_direct: None, pelure: true, fps: 8, lecture: None, message: Arc::default(), cherche: false }
    }

    fn allumer(&mut self) {
        self.stop.store(true, Relaxed);
        let Some((_, entree)) = self.cameras.get(self.camera).cloned() else { return };
        self.stop = Arc::new(AtomicBool::new(false));
        let (st, d, m) = (self.stop.clone(), self.direct.clone(), self.message.clone());
        std::thread::spawn(move || {
            let mut c = match crate::createur::ffmpeg() {
                Ok(c) => c,
                Err(e) => {
                    *m.lock().unwrap() = e;
                    return;
                }
            };
            c.args(["-v", "error"]).args(&entree).args(["-vf", "scale=960:-2", "-r", "15", "-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "4", "-"]);
            let Ok(mut enfant) = c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn() else { return };
            let Some(mut sortie) = enfant.stdout.take() else { return };
            let mut tampon = Vec::new();
            let mut b = vec![0u8; 65536];
            while !st.load(Relaxed) {
                let Ok(n) = sortie.read(&mut b) else { break };
                if n == 0 {
                    *m.lock().unwrap() = "La webcam ne répond pas (déjà utilisée par une autre appli ?)".into();
                    break;
                }
                tampon.extend_from_slice(&b[..n]);
                if let Some(j) = decouper_jpeg(&mut tampon).pop() {
                    if let Ok(i) = image::load_from_memory(&j) {
                        *d.lock().unwrap() = Some(i.to_rgba8());
                    }
                }
            }
            let _ = enfant.kill();
        });
    }

    fn ajouter(&mut self, ctx: &egui::Context, i: RgbaImage) {
        let v = image::imageops::thumbnail(&i, 120, (120 * i.height() / i.width().max(1)).max(1));
        let c = egui::ColorImage::from_rgba_unmultiplied([v.width() as usize, v.height() as usize], v.as_raw());
        self.vignettes.push(ctx.load_texture(format!("sm{}", self.vignettes.len()), c, Default::default()));
        self.images.push(i);
    }

    fn exporter(&self, gif: bool) {
        let ext = if gif { "gif" } else { "mp4" };
        let Some(dest) = rfd::FileDialog::new().set_directory(dossier_videos()).add_filter(ext, &[ext]).set_file_name(format!("stop_motion.{ext}")).save_file() else { return };
        let (images, fps, m) = (self.images.clone(), self.fps, self.message.clone());
        *m.lock().unwrap() = "Création de la vidéo…".into();
        std::thread::spawn(move || {
            let r = (|| -> Result<(), String> {
                let t = std::env::temp_dir().join("outils_stop_motion");
                let _ = std::fs::remove_dir_all(&t);
                std::fs::create_dir_all(&t).map_err(|e| e.to_string())?;
                for (k, i) in images.iter().enumerate() {
                    image::DynamicImage::ImageRgba8(i.clone()).to_rgb8().save(t.join(format!("{:04}.jpg", k + 1))).map_err(|e| e.to_string())?;
                }
                let mut c = crate::createur::ffmpeg()?;
                c.args(["-y", "-v", "error", "-framerate", &fps.to_string(), "-i"]).arg(t.join("%04d.jpg"));
                if gif {
                    c.args(["-vf", &format!("fps={fps},scale=480:-1:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse")]);
                } else {
                    c.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"]);
                }
                let o = c.arg(&dest).output().map_err(|e| e.to_string())?;
                let _ = std::fs::remove_dir_all(&t);
                if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("échec").to_string()) }
            })();
            *m.lock().unwrap() = match r {
                Ok(()) => format!("✔ {}", dest.display()),
                Err(e) => format!("✖ {e}"),
            };
        });
    }
}

impl Drop for StopMotion {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

fn melanger_pelure(direct: &RgbaImage, avant: &RgbaImage) -> RgbaImage {
    let a = if avant.dimensions() == direct.dimensions() { avant.clone() } else { image::imageops::resize(avant, direct.width(), direct.height(), image::imageops::FilterType::Triangle) };
    let mut r = direct.clone();
    for (p, q) in r.pixels_mut().zip(a.pixels()) {
        for k in 0..3 {
            p[k] = (p[k] as f32 * 0.65 + q[k] as f32 * 0.35) as u8;
        }
    }
    r
}

impl Outil for StopMotion {
    fn nom(&self) -> &'static str {
        "🎬 Stop motion"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Stop motion");
        ui.weak("Place tes figurines ou ta pâte à modeler, prends une photo, bouge un peu, reprends… L'image d'avant s'affiche en transparence pour t'aider.");
        if !self.cherche {
            self.cherche = true;
            let m = self.message.clone();
            *m.lock().unwrap() = "Recherche des webcams…".into();
            self.cameras = webcams();
            *m.lock().unwrap() = if self.cameras.is_empty() { "Aucune webcam trouvée : tu peux aussi importer des photos.".into() } else { String::new() };
            if !self.cameras.is_empty() {
                self.allumer();
            }
        }
        ui.horizontal_wrapped(|ui| {
            if !self.cameras.is_empty() {
                let avant = self.camera;
                egui::ComboBox::from_id_salt("webcam_sm").selected_text(self.cameras[self.camera].0.clone()).show_ui(ui, |ui| {
                    for (k, (n, _)) in self.cameras.iter().enumerate() {
                        ui.selectable_value(&mut self.camera, k, n);
                    }
                });
                if avant != self.camera {
                    self.allumer();
                }
            }
            if ui.button("🖼 Importer des photos…").clicked() {
                if let Some(v) = rfd::FileDialog::new().add_filter("Images", &["jpg", "jpeg", "png", "webp"]).pick_files() {
                    let mut v = v;
                    v.sort();
                    for p in v {
                        if let Ok(i) = image::open(&p) {
                            self.ajouter(ui.ctx(), i.to_rgba8());
                        }
                    }
                }
            }
            ui.checkbox(&mut self.pelure, "Image d'avant en transparence");
            ui.label("Vitesse");
            ui.add(egui::Slider::new(&mut self.fps, 2..=24).suffix(" img/s"));
        });
        let direct = self.direct.lock().unwrap().clone();
        let lecture = self.lecture.filter(|_| !self.images.is_empty());
        let affiche = if let Some(t) = lecture {
            let k = (t.elapsed().as_secs_f32() * self.fps as f32) as usize;
            if k >= self.images.len() {
                self.lecture = None;
            }
            self.images.get(k.min(self.images.len() - 1)).cloned()
        } else {
            direct.as_ref().map(|d| match (self.pelure, self.images.last()) {
                (true, Some(a)) => melanger_pelure(d, a),
                _ => d.clone(),
            })
        };
        if let Some(i) = affiche {
            let c = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
            match &mut self.tex_direct {
                Some(t) => t.set(c, Default::default()),
                None => self.tex_direct = Some(ui.ctx().load_texture("sm_direct", c, Default::default())),
            }
        }
        if let Some(t) = &self.tex_direct {
            let l = ui.available_width().min(720.0);
            let s = t.size_vec2();
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(l, l * s.y / s.x.max(1.0))));
        }
        ui.horizontal_wrapped(|ui| {
            let espace = ui.input(|i| i.key_pressed(egui::Key::Space));
            if ui.add_enabled(direct.is_some(), egui::Button::new(RichText::new("📸 Photo (Espace)").size(18.0))).clicked() || (espace && direct.is_some()) {
                if let Some(d) = direct.clone() {
                    self.ajouter(ui.ctx(), d);
                }
            }
            if ui.add_enabled(!self.images.is_empty(), egui::Button::new("▶ Revoir")).clicked() {
                self.lecture = Some(Instant::now());
            }
            if ui.add_enabled(!self.images.is_empty(), egui::Button::new("↶ Enlever la dernière")).clicked() {
                self.images.pop();
                self.vignettes.pop();
            }
            if ui.add_enabled(!self.images.is_empty(), egui::Button::new("🎞 Exporter en MP4…")).clicked() {
                self.exporter(false);
            }
            if ui.add_enabled(!self.images.is_empty(), egui::Button::new("🎞 Exporter en GIF…")).clicked() {
                self.exporter(true);
            }
            ui.label(format!("{} image(s) · {:.1} s", self.images.len(), self.images.len() as f32 / self.fps as f32));
        });
        ui.label(self.message.lock().unwrap().clone());
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut retirer = None;
                for (k, v) in self.vignettes.iter().enumerate() {
                    let r = ui.add(egui::Image::new(v).fit_to_exact_size(egui::vec2(90.0, 68.0)).sense(egui::Sense::click())).on_hover_text("Clic droit : supprimer");
                    r.context_menu(|ui| {
                        if ui.button("🗑 Supprimer cette image").clicked() {
                            retirer = Some(k);
                            ui.close_menu();
                        }
                    });
                }
                if let Some(k) = retirer {
                    self.images.remove(k);
                    let _ = self.vignettes.remove(k);
                }
            });
        });
        ui.ctx().request_repaint_after(Duration::from_millis(60));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_et_video() {
        // une sinusoïde de 1 kHz tombe dans la bonne bande
        let e: Vec<f32> = (0..2048).map(|k| (TAU * 1000.0 * k as f32 / 44100.0).sin()).collect();
        let b = bandes(&e, 44100, 32);
        let max = b.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let attendu = ((1000f32 / 40.0).ln() / (16000f32 / 40.0).ln() * 32.0) as usize;
        assert!((max as i32 - attendu as i32).abs() <= 1, "{max} {attendu}");
        // hauteur d'une note de 220 Hz
        let la: Vec<f32> = (0..1024).map(|k| (TAU * 220.0 * k as f32 / 8000.0).sin() * 0.5).collect();
        let h = hauteur(&la, 8000).unwrap();
        assert!((h - 220.0).abs() < 5.0, "{h}");
        assert!(hauteur(&vec![0.0; 1024], 8000).is_none());
        assert!(ecart_notes(440.0, 220.0) < 0.01);
        assert!((ecart_notes(440.0, 466.16) - 1.0).abs() < 0.05);
        let l = lire_lrc("[00:12.50]Bonjour\n[01:02.00][01:30.00]Refrain\nsans temps");
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], (12.5, "Bonjour".to_string()));
        assert_eq!(l[2].0, 90.0);
        let mut t = vec![1, 2, 0xFF, 0xD8, 9, 9, 0xFF, 0xD9, 0xFF, 0xD8, 7];
        let j = decouper_jpeg(&mut t);
        assert_eq!(j, vec![vec![0xFF, 0xD8, 9, 9, 0xFF, 0xD9]]);
        assert_eq!(t, vec![0xFF, 0xD8, 7]);
    }
}
