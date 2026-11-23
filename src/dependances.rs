//! Programmes externes téléchargés tout seuls, en version portable (aucune installation) dans
//! %APPDATA%\Outils\bin : ffmpeg / ffprobe (vidéo, son) et yt-dlp (téléchargement de vidéos).
//! Les outils appellent `commande("ffmpeg")` : s'il manque, il est récupéré la première fois.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const SANS_FENETRE: u32 = 0x0800_0000;
const FFMPEG_ZIP: &[&str] = &[
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip",
    "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
];
const YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";

pub fn dossier() -> PathBuf {
    crate::outil::racine().join("bin")
}

fn local(nom: &str) -> PathBuf {
    dossier().join(format!("{nom}.exe"))
}

/// Vrai si le programme est disponible (copie d'Outils ou déjà installé sur le PC).
pub fn present(nom: &str) -> bool {
    if local(nom).exists() {
        return true;
    }
    static DANS_PATH: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = DANS_PATH.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap().get(nom) {
        return *v;
    }
    let v = Command::new("where").arg(nom).creation_flags(SANS_FENETRE).output().is_ok_and(|o| o.status.success());
    cache.lock().unwrap().insert(nom.to_string(), v);
    v
}

#[derive(Clone, Default)]
pub struct Progres {
    pub fait: u64,
    pub total: u64,
    pub erreur: Option<String>,
    pub en_cours: bool,
}

fn etats() -> &'static Mutex<HashMap<&'static str, Progres>> {
    static E: OnceLock<Mutex<HashMap<&'static str, Progres>>> = OnceLock::new();
    E.get_or_init(Default::default)
}

pub fn progres(nom: &str) -> Progres {
    etats().lock().unwrap().get(nom).cloned().unwrap_or_default()
}

/// Nom du paquet qui fournit ce programme (ffprobe vient avec ffmpeg).
fn paquet(nom: &str) -> &'static str {
    if nom == "yt-dlp" { "yt-dlp" } else { "ffmpeg" }
}

fn telecharger(url: &str, dest: &std::path::Path, cle: &'static str) -> Result<(), String> {
    use std::io::{Read, Write};
    let r = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(20)).user_agent("Outils").build().get(url).call().map_err(|e| e.to_string())?;
    let total = r.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut lecteur = r.into_reader();
    let tmp = dest.with_extension("part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let mut fait = 0u64;
    loop {
        let n = lecteur.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        fait += n as u64;
        if let Some(p) = etats().lock().unwrap().get_mut(cle) {
            p.fait = fait;
            p.total = total;
        }
    }
    drop(f);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

/// Extrait `bin/ffmpeg.exe` et `bin/ffprobe.exe` d'une archive de build Windows.
pub fn extraire_ffmpeg(zip: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::fs::File::open(zip).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut trouves = 0;
    for i in 0..a.len() {
        let mut e = a.by_index(i).map_err(|e| e.to_string())?;
        let nom = e.name().replace('\\', "/");
        for exe in ["ffmpeg.exe", "ffprobe.exe"] {
            if nom.ends_with(&format!("/bin/{exe}")) {
                let mut f = std::fs::File::create(dest.join(exe)).map_err(|e| e.to_string())?;
                std::io::copy(&mut e, &mut f).map_err(|e| e.to_string())?;
                trouves += 1;
            }
        }
    }
    if trouves == 2 { Ok(()) } else { Err("archive ffmpeg inattendue".into()) }
}

/// Télécharge le paquet (bloquant). Sans effet s'il est déjà là ou déjà en cours ailleurs (on attend alors).
pub fn installer(nom: &str) -> Result<(), String> {
    let cle = paquet(nom);
    loop {
        let mut e = etats().lock().unwrap();
        let p = e.entry(cle).or_default();
        if !p.en_cours {
            if local(nom).exists() {
                return Ok(());
            }
            *p = Progres { en_cours: true, ..Default::default() };
            break;
        }
        drop(e);
        std::thread::sleep(Duration::from_millis(300)); // un autre outil télécharge déjà
    }
    let d = dossier();
    let r = (|| {
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        if cle == "yt-dlp" {
            return telecharger(YTDLP, &local("yt-dlp"), cle);
        }
        let zip = d.join("ffmpeg_telechargement.zip");
        let mut derniere = String::new();
        for u in FFMPEG_ZIP {
            match telecharger(u, &zip, cle).and_then(|_| extraire_ffmpeg(&zip, &d)) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&zip);
                    return Ok(());
                }
                Err(e) => derniere = e,
            }
        }
        let _ = std::fs::remove_file(&zip);
        Err(derniere)
    })();
    let mut e = etats().lock().unwrap();
    let p = e.entry(cle).or_default();
    p.en_cours = false;
    p.erreur = r.as_ref().err().cloned();
    r
}

/// Commande prête à lancer ; télécharge le programme s'il manque (bloquant : à appeler dans un fil).
pub fn commande(nom: &str) -> Result<Command, String> {
    if !present(nom) {
        installer(nom).map_err(|e| format!("{nom} n'a pas pu être téléchargé : {e}"))?;
    }
    let p = local(nom);
    let mut c = Command::new(if p.exists() { p } else { PathBuf::from(nom) });
    c.creation_flags(SANS_FENETRE);
    Ok(c)
}

/// Version non bloquante (interface) : lance le téléchargement en arrière-plan et prévient.
pub fn commande_ou_telecharger(nom: &str) -> Result<Command, String> {
    if present(nom) {
        return commande(nom);
    }
    let n = nom.to_string();
    if !progres(paquet(nom)).en_cours {
        std::thread::spawn(move || {
            let _ = installer(&n);
        });
    }
    Err(format!("Téléchargement de {} en cours (une seule fois, ~115 Mo pour ffmpeg) : réessaie dans un moment.", paquet(nom)))
}

// ---------------------------------------------------------------------------------------------------
// Onglet
// ---------------------------------------------------------------------------------------------------

pub struct Programmes {
    message: String,
}

impl Programmes {
    pub fn new() -> Self {
        Programmes { message: String::new() }
    }
}

impl Outil for Programmes {
    fn nom(&self) -> &'static str {
        "🧰 Programmes externes"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programmes externes");
        ui.label("Rien à installer : quand un outil en a besoin, Outils télécharge la version portable officielle ici :");
        ui.monospace(dossier().display().to_string());
        ui.add_space(6.0);
        for (nom, role, taille) in [("ffmpeg", "vidéo, GIF, son, enregistrement d'écran, replay", "~115 Mo"), ("yt-dlp", "téléchargement de vidéos", "~18 Mo")] {
            let p = progres(paquet(nom));
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(nom).strong());
                    ui.weak(format!("{role} · {taille}"));
                });
                ui.horizontal(|ui| {
                    if p.en_cours {
                        let f = if p.total > 0 { p.fait as f32 / p.total as f32 } else { 0.0 };
                        ui.add(egui::ProgressBar::new(f).desired_width(260.0).text(format!("{} / {}", crate::disque::taille_lisible(p.fait), crate::disque::taille_lisible(p.total))));
                        ui.ctx().request_repaint_after(Duration::from_millis(300));
                    } else if local(nom).exists() {
                        ui.colored_label(Color32::LIGHT_GREEN, "✔ prêt (copie d'Outils)");
                        if ui.small_button("Mettre à jour").clicked() {
                            let _ = std::fs::remove_file(local(nom));
                            if nom == "ffmpeg" {
                                let _ = std::fs::remove_file(local("ffprobe"));
                            }
                            let n = nom.to_string();
                            std::thread::spawn(move || installer(&n));
                        }
                    } else if present(nom) {
                        ui.colored_label(Color32::LIGHT_GREEN, "✔ déjà installé sur le PC");
                    } else {
                        ui.weak("pas encore téléchargé (le sera au premier besoin)");
                        if ui.button("📥 Télécharger maintenant").clicked() {
                            let n = nom.to_string();
                            std::thread::spawn(move || installer(&n));
                        }
                    }
                    if let Some(e) = &p.erreur {
                        ui.colored_label(Color32::LIGHT_RED, e);
                    }
                });
            });
        }
        ui.add_space(6.0);
        ui.weak("Plus besoin de 7-Zip (7z intégré, rar via Windows), de Ghostscript (compression PDF intégrée) ni de ffmpeg pour les mémos vocaux (micro lu directement).");
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_ffmpeg() {
        let d = std::env::temp_dir().join(format!("outils_dep_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let z = d.join("f.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&z).unwrap());
        for n in ["ffmpeg-7.1-essentials/bin/ffmpeg.exe", "ffmpeg-7.1-essentials/bin/ffprobe.exe", "ffmpeg-7.1-essentials/doc/x.html"] {
            w.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, n.as_bytes()).unwrap();
        }
        w.finish().unwrap();
        extraire_ffmpeg(&z, &d).unwrap();
        assert!(d.join("ffmpeg.exe").exists() && d.join("ffprobe.exe").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
