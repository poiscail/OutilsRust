//! Éditeur de musique (boîte à rythmes + piano roll) : des motifs de 16 pas joués en boucle,
//! enchaînés en morceau, avec des sons synthétisés (aucun fichier à télécharger). Export WAV / MP3.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText, Sense};
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub const TAUX: u32 = 44100;
pub const PAS: usize = 16;
pub const BATTERIE: [&str; 5] = ["Grosse caisse", "Caisse claire", "Charleston", "Clap", "Tom"];
/// Gamme (mineure pentatonique) sur deux octaves, de la plus aiguë à la plus grave pour l'affichage.
pub const NOTES: [(&str, f32); 10] = [("Mi5", 659.26), ("Ré5", 587.33), ("Do5", 523.25), ("La4", 440.0), ("Sol4", 392.0), ("Mi4", 329.63), ("Ré4", 293.66), ("Do4", 261.63), ("La3", 220.0), ("Sol3", 196.0)];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Motif {
    pub batterie: Vec<[bool; PAS]>,
    /// note de la basse à chaque pas (indice dans NOTES)
    pub basse: [Option<u8>; PAS],
    pub melodie: [Option<u8>; PAS],
}

impl Default for Motif {
    fn default() -> Self {
        Motif { batterie: vec![[false; PAS]; BATTERIE.len()], basse: [None; PAS], melodie: [None; PAS] }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum Timbre {
    Carre,
    Scie,
    Triangle,
    Douce,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Morceau {
    pub tempo: f32,
    pub motifs: Vec<Motif>,
    /// enchaînement des motifs, ex. « AABA »
    pub ordre: String,
    pub timbre: Timbre,
    pub volumes: [f32; 3], // batterie, basse, mélodie
    pub swing: f32,
}

impl Default for Morceau {
    fn default() -> Self {
        let mut a = Motif::default();
        for k in [0, 4, 8, 12] {
            a.batterie[0][k] = true;
        }
        for k in [4, 12] {
            a.batterie[1][k] = true;
        }
        for k in (0..PAS).step_by(2) {
            a.batterie[2][k] = true;
        }
        for (k, n) in [(0, 9), (3, 9), (6, 7), (8, 8), (11, 8), (14, 6)] {
            a.basse[k] = Some(n);
        }
        for (k, n) in [(0, 3), (2, 2), (4, 3), (7, 4), (10, 5), (12, 4)] {
            a.melodie[k] = Some(n);
        }
        Morceau { tempo: 110.0, motifs: vec![a, Motif::default()], ordre: "AAAB".into(), timbre: Timbre::Carre, volumes: [0.9, 0.7, 0.5], swing: 0.0 }
    }
}

/// Bruit blanc déterministe (xorshift).
struct Bruit(u32);
impl Bruit {
    fn suivant(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Son d'un instrument de batterie.
pub fn coup(instrument: usize) -> Vec<f32> {
    let t = TAUX as f32;
    let mut b = Bruit(0x9E37_79B9 + instrument as u32);
    let (duree, f): (f32, Box<dyn FnMut(f32) -> f32>) = match instrument {
        0 => (0.35, Box::new(|s: f32| {
            let phase = TAU * (45.0 * s + 110.0 * (1.0 - (-s * 30.0).exp()) / 30.0);
            phase.sin() * (-s * 9.0).exp()
        })),
        1 => (0.22, Box::new(move |s: f32| (0.6 * b.suivant() + 0.4 * (TAU * 185.0 * s).sin()) * (-s * 18.0).exp())),
        2 => {
            let mut avant = 0.0;
            (0.06, Box::new(move |s: f32| {
                let n = b.suivant();
                let h = n - avant; // passe-haut grossier
                avant = n;
                0.5 * h * (-s * 70.0).exp()
            }))
        }
        3 => (0.25, Box::new(move |s: f32| {
            let salves = if s < 0.03 { (-(s % 0.01) * 300.0).exp() } else { (-(s - 0.03) * 20.0).exp() };
            b.suivant() * salves * 0.7
        })),
        _ => (0.3, Box::new(|s: f32| (TAU * (120.0 * s + 60.0 * (1.0 - (-s * 12.0).exp()) / 12.0)).sin() * (-s * 10.0).exp())),
    };
    let mut f = f;
    (0..(duree * t) as usize).map(|i| f(i as f32 / t)).collect()
}

fn onde(timbre: Timbre, phase: f32) -> f32 {
    let p = phase.fract();
    match timbre {
        Timbre::Carre => if p < 0.5 { 0.6 } else { -0.6 },
        Timbre::Scie => 2.0 * p - 1.0,
        Timbre::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
        Timbre::Douce => (TAU * p).sin() * 0.8 + (TAU * 2.0 * p).sin() * 0.2,
    }
}

/// Note synthétisée (enveloppe attaque/déclin) d'une durée donnée.
pub fn note(freq: f32, duree: f32, timbre: Timbre) -> Vec<f32> {
    let t = TAUX as f32;
    let n = (duree * t) as usize;
    (0..n)
        .map(|i| {
            let s = i as f32 / t;
            let env = (s / 0.005).min(1.0) * (1.0 - s / duree).max(0.0).powf(1.5);
            onde(timbre, freq * s) * env
        })
        .collect()
}

fn mixer(dest: &mut Vec<f32>, debut: usize, son: &[f32], volume: f32) {
    if dest.len() < debut + son.len() {
        dest.resize(debut + son.len(), 0.0);
    }
    for (k, v) in son.iter().enumerate() {
        dest[debut + k] += v * volume;
    }
}

/// Durée d'un pas (double croche) en échantillons.
pub fn duree_pas(tempo: f32) -> f32 {
    60.0 / tempo.max(30.0) / 4.0 * TAUX as f32
}

/// Liste des motifs à jouer (indices), d'après l'ordre « AABA ».
pub fn enchainement(m: &Morceau) -> Vec<usize> {
    let v: Vec<usize> = m.ordre.to_uppercase().chars().filter_map(|c| c.is_ascii_uppercase().then(|| (c as u8 - b'A') as usize)).filter(|i| *i < m.motifs.len()).collect();
    if v.is_empty() { vec![0] } else { v }
}

/// Rend un morceau (ou un seul motif si `seul`) en échantillons mono.
pub fn rendre(m: &Morceau, seul: Option<usize>) -> Vec<f32> {
    let dp = duree_pas(m.tempo);
    let sons: Vec<Vec<f32>> = (0..BATTERIE.len()).map(coup).collect();
    let liste = match seul {
        Some(i) => vec![i],
        None => enchainement(m),
    };
    let mut sortie = vec![0.0f32; (dp * (PAS * liste.len()) as f32) as usize];
    for (n, &i) in liste.iter().enumerate() {
        let Some(mo) = m.motifs.get(i) else { continue };
        for pas in 0..PAS {
            let decal = if pas % 2 == 1 { m.swing * dp * 0.5 } else { 0.0 };
            let debut = (dp * (n * PAS + pas) as f32 + decal) as usize;
            for (k, ligne) in mo.batterie.iter().enumerate() {
                if ligne[pas] {
                    mixer(&mut sortie, debut, &sons[k.min(sons.len() - 1)], m.volumes[0] * 0.8);
                }
            }
            // une note dure jusqu'à la suivante (max 4 pas)
            let longueur = |piste: &[Option<u8>; PAS]| (1..=4).take_while(|d| pas + d < PAS && piste[pas + d].is_none()).count() + 1;
            if let Some(b) = mo.basse[pas] {
                mixer(&mut sortie, debut, &note(NOTES[b as usize % NOTES.len()].1 / 2.0, longueur(&mo.basse) as f32 * dp / TAUX as f32, Timbre::Scie), m.volumes[1] * 0.45);
            }
            if let Some(b) = mo.melodie[pas] {
                mixer(&mut sortie, debut, &note(NOTES[b as usize % NOTES.len()].1, longueur(&mo.melodie) as f32 * dp / TAUX as f32 * 0.9, m.timbre), m.volumes[2] * 0.4);
            }
        }
    }
    // un motif joué en boucle garde sa durée exacte (sinon le rythme dérive)
    if seul.is_some() {
        sortie.truncate((dp * PAS as f32) as usize);
    }
    // limiteur doux
    let pic = sortie.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    if pic > 0.95 {
        for v in &mut sortie {
            *v = (*v / pic) * 0.95;
        }
    }
    sortie
}

/// Fichier WAV 16 bits mono.
pub fn wav(e: &[f32]) -> Vec<u8> {
    let n = (e.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + n as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + n).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&TAUX.to_le_bytes());
    v.extend_from_slice(&(TAUX * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&n.to_le_bytes());
    for s in e {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    v
}

/// Lecture en arrière-plan ; s'arrête quand `stop` passe à vrai.
fn jouer(e: Vec<f32>, boucle: bool, stop: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        {
            let Ok((_flux, h)) = rodio::OutputStream::try_default() else { return };
            let Ok(sink) = rodio::Sink::try_new(&h) else { return };
            loop {
                sink.append(rodio::buffer::SamplesBuffer::new(1, TAUX, e.clone()));
                while !sink.empty() {
                    if stop.load(Relaxed) {
                        sink.stop();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                if !boucle || stop.load(Relaxed) {
                    return;
                }
            }
        }
        #[cfg(not(windows))]
        {
            let f = std::env::temp_dir().join("outils_musique.wav");
            if std::fs::write(&f, wav(&e)).is_err() {
                return;
            }
            loop {
                let Ok(mut c) = std::process::Command::new("paplay").arg(&f).spawn().or_else(|_| std::process::Command::new("aplay").arg("-q").arg(&f).spawn()) else { return };
                loop {
                    if stop.load(Relaxed) {
                        let _ = c.kill();
                        return;
                    }
                    if matches!(c.try_wait(), Ok(Some(_))) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                if !boucle {
                    return;
                }
            }
        }
    });
}

pub struct Musique {
    m: Morceau,
    motif: usize,
    stop: Arc<AtomicBool>,
    debut: Option<(Instant, usize, bool)>, // départ, nb de pas, boucle
    message: Arc<Mutex<String>>,
}

impl Musique {
    pub fn new() -> Self {
        Musique { m: charger("musique"), motif: 0, stop: Arc::new(AtomicBool::new(false)), debut: None, message: Arc::default() }
    }

    fn arreter(&mut self) {
        self.stop.store(true, Relaxed);
        self.debut = None;
    }

    fn lancer(&mut self, seul: Option<usize>, boucle: bool) {
        self.arreter();
        self.stop = Arc::new(AtomicBool::new(false));
        let e = rendre(&self.m, seul);
        let pas = if seul.is_some() { PAS } else { enchainement(&self.m).len() * PAS };
        self.debut = Some((Instant::now(), pas, boucle));
        jouer(e, boucle, self.stop.clone());
    }
}

fn grille(ui: &mut egui::Ui, nom: &str, actif: &mut [bool; PAS], couleur: Color32, curseur: Option<usize>) -> bool {
    let mut change = false;
    ui.horizontal(|ui| {
        ui.add_sized([110.0, 18.0], egui::Label::new(RichText::new(nom).small()));
        for k in 0..PAS {
            let (r, rep) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), Sense::click());
            let fond = if actif[k] { couleur } else if k % 4 == 0 { Color32::from_gray(70) } else { Color32::from_gray(50) };
            ui.painter().rect_filled(r.shrink(1.0), 3.0, fond);
            if curseur == Some(k) {
                ui.painter().rect_stroke(r.shrink(1.0), 3.0, egui::Stroke::new(2.0_f32, Color32::WHITE), egui::StrokeKind::Inside);
            }
            if rep.clicked() {
                actif[k] = !actif[k];
                change = true;
            }
        }
    });
    change
}

fn piano(ui: &mut egui::Ui, piste: &mut [Option<u8>; PAS], couleur: Color32, curseur: Option<usize>) -> bool {
    let mut change = false;
    for (n, (nom, _)) in NOTES.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_sized([110.0, 14.0], egui::Label::new(RichText::new(*nom).small().weak()));
            for k in 0..PAS {
                let (r, rep) = ui.allocate_exact_size(egui::vec2(22.0, 14.0), Sense::click());
                let on = piste[k] == Some(n as u8);
                let fond = if on { couleur } else if k % 4 == 0 { Color32::from_gray(62) } else { Color32::from_gray(45) };
                ui.painter().rect_filled(r.shrink(1.0), 2.0, fond);
                if curseur == Some(k) {
                    ui.painter().rect_stroke(r.shrink(0.5), 2.0, egui::Stroke::new(1.0_f32, Color32::from_white_alpha(120)), egui::StrokeKind::Inside);
                }
                if rep.clicked() {
                    piste[k] = if on { None } else { Some(n as u8) };
                    change = true;
                }
            }
        });
    }
    change
}

impl Outil for Musique {
    fn nom(&self) -> &'static str {
        "🎹 Éditeur de musique"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Éditeur de musique");
        ui.weak("Clique dans les cases pour placer les sons. Chaque motif fait 16 pas et tourne en boucle ; enchaîne-les pour faire un morceau.");
        let avant = self.m.clone();
        // curseur de lecture
        let mut curseur = None;
        if let Some((t, n, boucle)) = self.debut {
            let pas = (t.elapsed().as_secs_f32() * TAUX as f32 / duree_pas(self.m.tempo)) as usize;
            if pas >= n && !boucle {
                self.debut = None;
            } else {
                curseur = Some(pas % PAS);
            }
            ui.ctx().request_repaint();
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("▶ Motif en boucle").clicked() {
                self.lancer(Some(self.motif), true);
            }
            if ui.button("▶ Morceau").clicked() {
                self.lancer(None, false);
            }
            if ui.button("⏹ Stop").clicked() {
                self.arreter();
            }
            ui.label("Tempo");
            ui.add(egui::DragValue::new(&mut self.m.tempo).range(60.0..=200.0).suffix(" bpm"));
            ui.label("Swing");
            ui.add(egui::Slider::new(&mut self.m.swing, 0.0..=0.6).show_value(false));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Motif :");
            for i in 0..self.m.motifs.len() {
                ui.selectable_value(&mut self.motif, i, RichText::new(((b'A' + i as u8) as char).to_string()).strong());
            }
            if self.m.motifs.len() < 8 && ui.small_button("➕").on_hover_text("Nouveau motif (copie de celui-ci)").clicked() {
                let c = self.m.motifs[self.motif].clone();
                self.m.motifs.push(c);
                self.motif = self.m.motifs.len() - 1;
            }
            if ui.small_button("🧹").on_hover_text("Vider ce motif").clicked() {
                self.m.motifs[self.motif] = Motif::default();
            }
            ui.separator();
            ui.label("Morceau :");
            ui.add(egui::TextEdit::singleline(&mut self.m.ordre).desired_width(120.0).hint_text("AABA"));
        });
        let mo = &mut self.m.motifs[self.motif];
        mo.batterie.resize(BATTERIE.len(), [false; PAS]);
        ui.separator();
        ui.strong("🥁 Batterie");
        for (k, n) in BATTERIE.iter().enumerate() {
            grille(ui, n, &mut mo.batterie[k], Color32::from_rgb(240, 120, 60), curseur);
        }
        ui.separator();
        egui::CollapsingHeader::new(RichText::new("🎸 Basse").strong()).default_open(false).show(ui, |ui| {
            piano(ui, &mut mo.basse, Color32::from_rgb(90, 170, 255), curseur);
        });
        egui::CollapsingHeader::new(RichText::new("🎹 Mélodie").strong()).default_open(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Son :");
                for (t, n) in [(Timbre::Carre, "8-bit"), (Timbre::Scie, "Synthé"), (Timbre::Triangle, "Flûte"), (Timbre::Douce, "Doux")] {
                    ui.selectable_value(&mut self.m.timbre, t, n);
                }
            });
            let mo = &mut self.m.motifs[self.motif];
            piano(ui, &mut mo.melodie, Color32::from_rgb(140, 230, 120), curseur);
        });
        ui.horizontal_wrapped(|ui| {
            for (k, n) in ["Batterie", "Basse", "Mélodie"].iter().enumerate() {
                ui.label(*n);
                ui.add(egui::Slider::new(&mut self.m.volumes[k], 0.0..=1.0).show_value(false));
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("💾 Exporter en WAV…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("WAV", &["wav"]).set_file_name("morceau.wav").save_file() {
                    *self.message.lock().unwrap() = std::fs::write(&p, wav(&rendre(&self.m, None))).map(|_| format!("✔ {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("💾 Exporter en MP3…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("MP3", &["mp3"]).set_file_name("morceau.mp3").save_file() {
                    let (e, m) = (rendre(&self.m, None), self.message.clone());
                    *m.lock().unwrap() = "Conversion en MP3…".into();
                    std::thread::spawn(move || {
                        let t = std::env::temp_dir().join("outils_morceau.wav");
                        let r = std::fs::write(&t, wav(&e)).map_err(|e| e.to_string()).and_then(|_| {
                            let o = crate::createur::ffmpeg()?.args(["-y", "-v", "error", "-i"]).arg(&t).args(["-b:a", "192k"]).arg(&p).output().map_err(|e| e.to_string())?;
                            if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
                        });
                        let _ = std::fs::remove_file(&t);
                        *m.lock().unwrap() = match r {
                            Ok(()) => format!("✔ {}", p.display()),
                            Err(e) => format!("✖ {e}"),
                        };
                    });
                }
            }
            let s = enchainement(&self.m).len() as f32 * PAS as f32 * duree_pas(self.m.tempo) / TAUX as f32;
            ui.weak(format!("Durée : {:.0} s", s));
        });
        ui.label(self.message.lock().unwrap().clone());
        if self.m != avant {
            enregistrer("musique", &self.m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn musique() {
        let m = Morceau::default();
        assert_eq!(enchainement(&m), vec![0, 0, 0, 1]);
        assert_eq!(enchainement(&Morceau { ordre: "zz".into(), ..Morceau::default() }), vec![0]);
        let e = rendre(&m, Some(0));
        let attendu = duree_pas(110.0) * 16.0;
        assert_eq!(e.len(), attendu as usize);
        assert!(rendre(&m, None).len() >= 4 * attendu as usize);
        assert!(e.iter().all(|v| v.abs() <= 1.0) && e.iter().any(|v| v.abs() > 0.1));
        let w = wav(&e);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(w.len(), 44 + e.len() * 2);
        for i in 0..BATTERIE.len() {
            assert!(!coup(i).is_empty() && coup(i).iter().all(|v| v.is_finite()));
        }
    }
}
