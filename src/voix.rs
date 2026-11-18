//! Voix : mémos vocaux (136, micro via ffmpeg), lecture à voix haute (137, voix de Windows, Ctrl+Alt+L
//! lit la sélection), sous-titres en direct (138, « Sous-titres en direct » de Windows 11).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const SANS_FENETRE: u32 = 0x0800_0000;

/// Micros listés par `ffmpeg -list_devices true -f dshow -i dummy` (sortie d'erreur).
pub fn lire_micros(sortie: &str) -> Vec<String> {
    sortie.lines().filter(|l| l.contains("(audio)")).filter_map(|l| {
        let a = l.find('"')?;
        let b = l[a + 1..].find('"')? + a + 1;
        Some(l[a + 1..b].to_string())
    }).collect()
}

fn micros() -> Vec<String> {
    Command::new("ffmpeg").args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]).creation_flags(SANS_FENETRE).output()
        .map(|o| lire_micros(&String::from_utf8_lossy(&o.stderr))).unwrap_or_default()
}

fn dossier_memos() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Documents").join("Mémos vocaux")
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Reglages {
    micro: String,
    voix: String,
    vitesse: i32, // -10..10
    raccourci: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { micro: String::new(), voix: String::new(), vitesse: 0, raccourci: true }
    }
}

fn parler(texte: &str, voix: &str, vitesse: i32) -> Option<Child> {
    let script = format!(
        "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Rate = {vitesse}; {} $s.Speak([Console]::In.ReadToEnd())",
        if voix.is_empty() { String::new() } else { format!("$s.SelectVoice('{}');", voix.replace('\'', "''")) }
    );
    let mut c = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]).stdin(Stdio::piped()).creation_flags(SANS_FENETRE).spawn().ok()?;
    if let Some(mut e) = c.stdin.take() {
        let _ = e.write_all(texte.as_bytes());
    }
    Some(c)
}

fn voix_installees() -> Vec<String> {
    Command::new("powershell.exe").args(["-NoProfile", "-Command", "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | % { $_.VoiceInfo.Name }"])
        .creation_flags(SANS_FENETRE).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

pub struct Voix {
    r: Arc<Mutex<Reglages>>,
    micros: Option<Vec<String>>,
    voix: Option<Vec<String>>,
    enreg: Option<(Child, PathBuf, Instant)>,
    lecture: Arc<Mutex<Option<Child>>>,
    texte: String,
    message: String,
    actif: Arc<AtomicBool>,
}

impl Voix {
    pub fn new() -> Self {
        let r: Reglages = charger("voix");
        let actif = Arc::new(AtomicBool::new(r.raccourci));
        let r = Arc::new(Mutex::new(r));
        let lecture: Arc<Mutex<Option<Child>>> = Arc::default();
        let (r2, l2, a2) = (r.clone(), lecture.clone(), actif.clone());
        // Ctrl+Alt+L : lit le texte sélectionné (ou arrête la lecture en cours)
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
            if RegisterHotKey(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'L' as u32) == 0 {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message != WM_HOTKEY || !a2.load(Relaxed) {
                    continue;
                }
                if let Some(mut c) = l2.lock().unwrap().take() {
                    if matches!(c.try_wait(), Ok(None)) {
                        let _ = c.kill();
                        continue;
                    }
                }
                if let Some(t) = crate::tradinstant::win::copier_selection() {
                    let reg = r2.lock().unwrap().clone();
                    *l2.lock().unwrap() = parler(&t, &reg.voix, reg.vitesse);
                }
            }
        });
        Voix { r, micros: None, voix: None, enreg: None, lecture, texte: String::new(), message: String::new(), actif }
    }
}

impl Outil for Voix {
    fn nom(&self) -> &'static str {
        "🗣 Voix (mémos, lecture, sous-titres)"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Voix");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();

        // --- mémos vocaux ---
        ui.strong("🎙 Mémos vocaux");
        if self.micros.is_none() {
            self.micros = Some(micros());
        }
        let liste = self.micros.clone().unwrap_or_default();
        if r.micro.is_empty() {
            r.micro = liste.first().cloned().unwrap_or_default();
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("micro").selected_text(if r.micro.is_empty() { "aucun micro (ffmpeg installé ?)" } else { r.micro.as_str() }).width(260.0).show_ui(ui, |ui| {
                for m in &liste {
                    ui.selectable_value(&mut r.micro, m.clone(), m);
                }
            });
            match &mut self.enreg {
                None => {
                    if ui.add_enabled(!r.micro.is_empty(), egui::Button::new("⏺ Enregistrer")).clicked() {
                        let d = dossier_memos();
                        let _ = std::fs::create_dir_all(&d);
                        let f = d.join(format!("Memo_{}.m4a", crate::minecraft::horodatage()));
                        match Command::new("ffmpeg").args(["-y", "-v", "error", "-f", "dshow", "-i"]).arg(format!("audio={}", r.micro)).args(["-c:a", "aac", "-b:a", "128k"]).arg(&f)
                            .stdin(Stdio::piped()).creation_flags(SANS_FENETRE).spawn() {
                            Ok(c) => self.enreg = Some((c, f, Instant::now())),
                            Err(_) => self.message = "ffmpeg introuvable (winget install ffmpeg).".into(),
                        }
                    }
                }
                Some((_, _, t)) => {
                    ui.colored_label(Color32::RED, format!("⏺ {:02}:{:02}", t.elapsed().as_secs() / 60, t.elapsed().as_secs() % 60));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                    if ui.button("⏹ Arrêter").clicked() {
                        if let Some((mut c, f, _)) = self.enreg.take() {
                            if let Some(mut e) = c.stdin.take() {
                                let _ = e.write_all(b"q");
                            }
                            let _ = c.wait();
                            self.message = format!("Enregistré : {}", f.file_name().unwrap_or_default().to_string_lossy());
                        }
                    }
                }
            }
            if ui.button("📂").clicked() {
                let _ = std::fs::create_dir_all(dossier_memos());
                crate::outil::ouvrir(&dossier_memos());
            }
        });
        let mut memos: Vec<PathBuf> = std::fs::read_dir(dossier_memos()).into_iter().flatten().flatten().map(|e| e.path()).collect();
        memos.sort();
        memos.reverse();
        for m in memos.iter().take(6) {
            ui.horizontal(|ui| {
                if ui.small_button("▶").clicked() {
                    crate::outil::ouvrir(m);
                }
                ui.label(m.file_name().unwrap_or_default().to_string_lossy());
            });
        }
        ui.separator();

        // --- lecture à voix haute ---
        ui.strong("🔊 Lecture à voix haute");
        ui.checkbox(&mut r.raccourci, "Ctrl+Alt+L lit le texte sélectionné dans n'importe quelle appli (et l'arrête)");
        if self.voix.is_none() {
            self.voix = Some(voix_installees());
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("voix").selected_text(if r.voix.is_empty() { "voix par défaut" } else { r.voix.as_str() }).width(260.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut r.voix, String::new(), "voix par défaut");
                for v in self.voix.clone().unwrap_or_default() {
                    ui.selectable_value(&mut r.voix, v.clone(), v);
                }
            });
            ui.add(egui::Slider::new(&mut r.vitesse, -10..=10).text("vitesse"));
        });
        ui.add(egui::TextEdit::multiline(&mut self.texte).hint_text("Texte à lire…").desired_rows(4).desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            if ui.button("▶ Lire").clicked() && !self.texte.trim().is_empty() {
                *self.lecture.lock().unwrap() = parler(&self.texte, &r.voix, r.vitesse);
            }
            if ui.button("⏹ Stop").clicked() {
                if let Some(mut c) = self.lecture.lock().unwrap().take() {
                    let _ = c.kill();
                }
            }
        });
        ui.separator();

        // --- sous-titres en direct ---
        ui.strong("💬 Sous-titres en direct");
        let lc = PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join(r"System32\LiveCaptions.exe");
        if lc.exists() {
            ui.horizontal(|ui| {
                if ui.button("Ouvrir les sous-titres en direct de Windows").clicked() {
                    let _ = Command::new(&lc).spawn();
                }
                ui.weak("(aussi : Win+Ctrl+L) · transcrit tout ce qui sort du PC : vidéos, appels, jeux");
            });
        } else {
            ui.weak("Les sous-titres en direct sont une fonction de Windows 11 (22H2 ou plus récent) : introuvables sur ce PC.");
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if r != avant {
            self.actif.store(r.raccourci, Relaxed);
            enregistrer("voix", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liste_micros() {
        let s = "[dshow @ 0x1] \"HD Webcam\" (video)\n[dshow @ 0x1] \"Microphone (Realtek Audio)\" (audio)\n[dshow @ 0x1]   Alternative name \"@device_cm_{...}\"\n";
        assert_eq!(lire_micros(s), vec!["Microphone (Realtek Audio)".to_string()]);
    }
}
