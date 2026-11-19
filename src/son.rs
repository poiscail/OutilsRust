//! Son : enregistreur de ce qui sort des haut-parleurs (145, capture « loopback » WASAPI)
//! et égaliseur (146, via Equalizer APO, le seul moyen fiable d'égaliser tout le son de Windows).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// En-tête WAV PCM 16 bits.
pub fn entete_wav(canaux: u16, frequence: u32, octets_donnees: u32) -> Vec<u8> {
    let mut h = vec![];
    h.extend(b"RIFF");
    h.extend((36 + octets_donnees).to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes());
    h.extend(canaux.to_le_bytes());
    h.extend(frequence.to_le_bytes());
    h.extend((frequence * canaux as u32 * 2).to_le_bytes());
    h.extend((canaux * 2).to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend(octets_donnees.to_le_bytes());
    h
}

pub fn f32_vers_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0) as i16
}

fn dossier_sons() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Music").join("Enregistrements")
}

/// Enregistre le son du PC jusqu'à `stop` ; écrit un WAV 16 bits. Renvoie la durée en secondes.
/// `micro` : enregistre le micro par défaut au lieu de ce qui sort des haut-parleurs.
pub fn enregistrer_boucle(dest: &Path, stop: &AtomicBool, niveau: &Mutex<f32>, micro: bool) -> Result<f64, String> {
    use windows::Win32::Media::Audio::*;
    use windows::Win32::System::Com::*;
    let e = |x: windows::core::Error| x.message();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
        let dev = en.GetDefaultAudioEndpoint(if micro { eCapture } else { eRender }, eConsole).map_err(e)?;
        let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(e)?;
        let fmt = client.GetMixFormat().map_err(e)?;
        let (canaux, freq, bits, bloc) = ((*fmt).nChannels, (*fmt).nSamplesPerSec, (*fmt).wBitsPerSample, (*fmt).nBlockAlign as usize);
        let flottant = bits == 32 && ((*fmt).wFormatTag == 3 || (*fmt).wFormatTag == 0xFFFE);
        client.Initialize(AUDCLNT_SHAREMODE_SHARED, if micro { 0 } else { AUDCLNT_STREAMFLAGS_LOOPBACK }, 10_000_000, 0, fmt, None).map_err(e)?;
        let capture: IAudioCaptureClient = client.GetService().map_err(e)?;
        let mut f = std::io::BufWriter::new(std::fs::File::create(dest).map_err(|x| x.to_string())?);
        f.write_all(&entete_wav(canaux, freq, 0)).map_err(|x| x.to_string())?;
        client.Start().map_err(e)?;
        let debut = Instant::now();
        let mut trames: u64 = 0;
        let ecrire = |echantillons: &mut dyn Iterator<Item = i16>, f: &mut std::io::BufWriter<std::fs::File>| -> u64 {
            let mut n = 0;
            for s in echantillons {
                let _ = f.write_all(&s.to_le_bytes());
                n += 1;
            }
            n
        };
        while !stop.load(Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
            let mut paquet = capture.GetNextPacketSize().map_err(e)?;
            if paquet == 0 {
                // rien ne joue : Windows n'envoie rien, on complète avec du silence pour garder la durée réelle
                let attendu = (debut.elapsed().as_secs_f64() * freq as f64) as u64;
                if attendu > trames + freq as u64 / 5 {
                    let manque = attendu - trames;
                    ecrire(&mut std::iter::repeat_n(0i16, (manque * canaux as u64) as usize), &mut f);
                    trames += manque;
                }
                *niveau.lock().unwrap() = 0.0;
            }
            while paquet > 0 {
                let (mut data, mut n, mut drapeaux) = (std::ptr::null_mut(), 0u32, 0u32);
                capture.GetBuffer(&mut data, &mut n, &mut drapeaux, None, None).map_err(e)?;
                let octets = std::slice::from_raw_parts(data, n as usize * bloc);
                let silence = drapeaux & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                let mut crete = 0f32;
                if silence {
                    ecrire(&mut std::iter::repeat_n(0i16, n as usize * canaux as usize), &mut f);
                } else if flottant {
                    let mut it = octets.chunks_exact(4).map(|c| {
                        let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                        crete = crete.max(v.abs());
                        f32_vers_i16(v)
                    });
                    ecrire(&mut it, &mut f);
                } else if bits == 16 {
                    let mut it = octets.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]));
                    ecrire(&mut it, &mut f);
                }
                *niveau.lock().unwrap() = crete;
                trames += n as u64;
                capture.ReleaseBuffer(n).map_err(e)?;
                paquet = capture.GetNextPacketSize().map_err(e)?;
            }
        }
        let _ = client.Stop();
        CoTaskMemFree(Some(fmt as *const _));
        drop(f);
        // taille réelle dans l'en-tête
        let taille = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(44) as u32 - 44;
        let mut g = std::fs::OpenOptions::new().write(true).open(dest).map_err(|x| x.to_string())?;
        g.write_all(&entete_wav(canaux, freq, taille)).map_err(|x| x.to_string())?;
        Ok(trames as f64 / freq as f64)
    }
}

pub struct EnregistreurSon {
    stop: Arc<AtomicBool>,
    actif: Arc<AtomicBool>,
    niveau: Arc<Mutex<f32>>,
    debut: Option<Instant>,
    message: Arc<Mutex<String>>,
    mp3: bool,
}

impl EnregistreurSon {
    pub fn new() -> Self {
        EnregistreurSon { stop: Arc::default(), actif: Arc::default(), niveau: Arc::default(), debut: None, message: Arc::default(), mp3: true }
    }
}

impl Outil for EnregistreurSon {
    fn nom(&self) -> &'static str {
        "🎙 Enregistrer le son du PC"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Enregistrer le son du PC");
        ui.weak("Capture tout ce qui sort des haut-parleurs / du casque (musique, appel, jeu, vidéo). Uniquement pour un usage personnel.");
        let actif = self.actif.load(Relaxed);
        ui.horizontal(|ui| {
            if actif {
                let t = self.debut.map(|d| d.elapsed().as_secs()).unwrap_or(0);
                ui.colored_label(Color32::RED, RichText::new(format!("⏺ {:02}:{:02}", t / 60, t % 60)).size(22.0));
                let n = *self.niveau.lock().unwrap();
                ui.add(egui::ProgressBar::new(n.min(1.0)).desired_width(160.0));
                if ui.button("⏹ Arrêter").clicked() {
                    self.stop.store(true, Relaxed);
                }
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            } else if ui.button(RichText::new("⏺ Enregistrer").size(18.0)).clicked() {
                let d = dossier_sons();
                let _ = std::fs::create_dir_all(&d);
                let wav = d.join(format!("Son_{}.wav", crate::minecraft::horodatage()));
                let (stop, act, niv, msg, mp3) = (self.stop.clone(), self.actif.clone(), self.niveau.clone(), self.message.clone(), self.mp3);
                stop.store(false, Relaxed);
                act.store(true, Relaxed);
                self.debut = Some(Instant::now());
                std::thread::spawn(move || {
                    let r = enregistrer_boucle(&wav, &stop, &niv, false);
                    let mut fin = wav.clone();
                    if r.is_ok() && mp3 {
                        let m = wav.with_extension("mp3");
                        let ok = crate::dependances::commande("ffmpeg").is_ok_and(|mut c| c.args(["-y", "-v", "error", "-i"]).arg(&wav).args(["-b:a", "192k"]).arg(&m).status().is_ok_and(|s| s.success()));
                        if ok {
                            let _ = std::fs::remove_file(&wav);
                            fin = m;
                        }
                    }
                    *msg.lock().unwrap() = match r {
                        Ok(s) => format!("✔ {:.0} s enregistrées : {}", s, fin.display()),
                        Err(e) => format!("✖ {e}"),
                    };
                    act.store(false, Relaxed);
                });
            }
            ui.checkbox(&mut self.mp3, "convertir en MP3 (ffmpeg)");
            if ui.button("📂").clicked() {
                let _ = std::fs::create_dir_all(dossier_sons());
                crate::outil::ouvrir(&dossier_sons());
            }
        });
        ui.label(self.message.lock().unwrap().clone());
    }
}

// ---------------------------------------------------------------------------------------------------
// 146 · Égaliseur (Equalizer APO)
// ---------------------------------------------------------------------------------------------------

pub const BANDES: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

pub const PRESETS: &[(&str, [f32; 10])] = &[
    ("Plat", [0.0; 10]),
    ("Basses", [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Voix / podcast", [-4.0, -3.0, -1.0, 1.0, 3.0, 4.0, 4.0, 2.0, 0.0, -1.0]),
    ("Pas en jeu (FPS)", [-6.0, -5.0, -3.0, -1.0, 0.0, 2.0, 4.0, 5.0, 3.0, 1.0]),
    ("Musique (V)", [4.0, 3.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 3.0, 4.0]),
    ("Casque doux", [2.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0, -2.0, -3.0, -3.0]),
];

/// Ligne de configuration Equalizer APO.
pub fn config_apo(g: &[f32; 10], preamp: f32) -> String {
    let b: Vec<String> = BANDES.iter().zip(g).map(|(f, v)| format!("{f} {v:.1}")).collect();
    format!("# Écrit par Outils\nPreamp: {preamp:.1} dB\nGraphicEQ: {}\n", b.join("; "))
}

fn dossier_apo() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or("C:\\Program Files".into())).join(r"EqualizerAPO\config")
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Eq {
    gains: [f32; 10],
    actif: bool,
}

impl Default for Eq {
    fn default() -> Self {
        Eq { gains: [0.0; 10], actif: true }
    }
}

pub struct Egaliseur {
    e: Eq,
    message: String,
    installation: Arc<Mutex<String>>,
}

impl Egaliseur {
    pub fn new() -> Self {
        Egaliseur { e: charger("egaliseur"), message: String::new(), installation: Arc::default() }
    }

    fn appliquer(&mut self) {
        let d = dossier_apo();
        let max = self.e.gains.iter().cloned().fold(0.0f32, f32::max);
        let txt = if self.e.actif { config_apo(&self.e.gains, -max) } else { "# Égaliseur d'Outils désactivé\n".into() };
        let r = std::fs::write(d.join("outils.txt"), txt).and_then(|_| {
            let c = d.join("config.txt");
            let contenu = std::fs::read_to_string(&c).unwrap_or_default();
            if !contenu.lines().any(|l| l.trim() == "Include: outils.txt") {
                std::fs::write(&c, format!("{}\nInclude: outils.txt\n", contenu.trim_end()))?;
            }
            Ok(())
        });
        self.message = match r {
            Ok(()) => "✔ Appliqué (instantané).".into(),
            Err(e) => format!("✖ {e} (lance Outils en administrateur la première fois)"),
        };
        enregistrer("egaliseur", &self.e);
    }
}

impl Outil for Egaliseur {
    fn nom(&self) -> &'static str {
        "🎚 Égaliseur audio"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Égaliseur audio");
        if !dossier_apo().exists() {
            ui.label("Windows n'a pas d'égaliseur pour tout le son : il faut le module audio gratuit Equalizer APO, à installer une seule fois (c'est un pilote, Outils ne peut pas le remplacer).");
            let etat = self.installation.lock().unwrap().clone();
            if etat.is_empty() {
                if ui.button(RichText::new("📥 Installer Equalizer APO").size(16.0)).clicked() {
                    let e = self.installation.clone();
                    *e.lock().unwrap() = "Téléchargement (12 Mo)…".into();
                    std::thread::spawn(move || {
                        let f = std::env::temp_dir().join("EqualizerAPO-installation.exe");
                        let r = ureq::AgentBuilder::new().user_agent("Outils").build().get("https://sourceforge.net/projects/equalizerapo/files/latest/download").call()
                            .map_err(|x| x.to_string())
                            .and_then(|r| {
                                let mut out = std::fs::File::create(&f).map_err(|x| x.to_string())?;
                                std::io::copy(&mut r.into_reader(), &mut out).map_err(|x| x.to_string())
                            })
                            .and_then(|_| std::process::Command::new(&f).spawn().map_err(|x| x.to_string()));
                        *e.lock().unwrap() = match r {
                            Ok(_) => "Installateur lancé : accepte, coche ton casque / tes haut-parleurs dans la liste, puis redémarre le PC.".into(),
                            Err(x) => format!("✖ {x}"),
                        };
                    });
                }
            } else {
                ui.label(etat);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            ui.weak("Ensuite, cet onglet le pilote directement : réglages appliqués instantanément.");
            return;
        }
        let avant = self.e.clone();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.e.actif, "Actif");
            for (n, g) in PRESETS {
                if ui.button(*n).clicked() {
                    self.e.gains = *g;
                }
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (i, f) in BANDES.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.add(egui::Slider::new(&mut self.e.gains[i], -12.0..=12.0).vertical().fixed_decimals(0).suffix(" dB").show_value(true));
                    ui.label(if *f >= 1000 { format!("{}k", f / 1000) } else { f.to_string() });
                });
            }
        });
        ui.weak("Les basses à gauche, les aigus à droite. Le volume général est baissé automatiquement quand tu montes une bande, pour éviter la saturation.");
        if self.e != avant {
            self.appliquer();
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.weak("Equalizer APO s'applique à tout un périphérique (casque, haut-parleurs), pas appli par appli.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_et_eq() {
        let h = entete_wav(2, 48000, 1000);
        assert_eq!(h.len(), 44);
        assert_eq!(&h[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 1036);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 192_000);
        assert_eq!(f32_vers_i16(2.0), 32767);
        assert_eq!(f32_vers_i16(-0.5), -16383);
        let c = config_apo(&PRESETS[1].1, -6.0);
        assert!(c.contains("Preamp: -6.0 dB") && c.contains("GraphicEQ: 31 6.0; 62 5.0"));
    }
}
