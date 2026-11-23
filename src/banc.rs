//! Comparateur de performances : mesure le processeur (1 cœur et tous), la mémoire et le disque,
//! repère la carte graphique, garde l'historique, et dit quelle pièce changer en premier.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Default, Debug, PartialEq)]
#[serde(default)]
pub struct Resultat {
    pub date: String,
    pub cpu_nom: String,
    pub fils: u32,
    pub gpu_nom: String,
    pub ram_go: f32,
    pub un_coeur: f32,  // Mo/s compressés sur 1 fil
    pub tous: f32,      // Mo/s sur tous les fils
    pub memoire: f32,   // Go/s copiés
    pub ecriture: f32,  // Mo/s
    pub lecture: f32,   // Mo/s
}

/// Niveau approximatif d'une carte graphique (100 ≈ RTX 4090), d'après son nom.
pub fn niveau_gpu(nom: &str) -> Option<u32> {
    crate::pratique::niveau_gpu(nom)
}

/// Pièce à changer en premier (titre, explication), d'après le matériel et les mesures.
pub fn conseil(r: &Resultat) -> (String, String) {
    let gpu = niveau_gpu(&r.gpu_nom);
    if r.ecriture > 0.0 && r.ecriture < 150.0 {
        return ("💽 Le disque".into(), format!("Ton disque écrit à {:.0} Mo/s : c'est sans doute un disque dur (ou un SSD très lent). Un SSD (NVMe si possible) rend Windows et les chargements de jeux bien plus rapides, pour pas cher.", r.ecriture));
    }
    if r.ram_go > 0.0 && r.ram_go < 15.0 {
        return ("🧠 La mémoire (RAM)".into(), format!("{:.0} Go de mémoire, c'est juste pour jouer aujourd'hui : passe à 16 Go (32 Go si tu streames ou fais des mods).", r.ram_go));
    }
    if gpu.is_some_and(|g| g < 25) {
        return ("🎮 La carte graphique".into(), format!("Ta carte graphique ({}) est la pièce la plus faible pour les jeux récents. C'est elle qui fait le plus de FPS.", r.gpu_nom));
    }
    if r.fils > 0 && r.fils < 8 {
        return ("🧮 Le processeur".into(), format!("{} fils d'exécution seulement : les jeux récents et le multitâche (Discord, navigateur, stream) en demandent plus.", r.fils));
    }
    if gpu.is_some_and(|g| g < 45) {
        return ("🎮 La carte graphique".into(), "Le reste suit bien : une carte graphique plus récente donnera le plus de FPS en plus.".into());
    }
    ("✔ Rien d'urgent".into(), "Ton PC est équilibré. Pour gagner encore : refroidissement, mémoire plus rapide, ou un écran à plus haute fréquence.".into())
}

fn donnees_test() -> Vec<u8> {
    // texte un peu répétitif : compressible comme de vrais fichiers
    let mut v = Vec::with_capacity(4 << 20);
    let mut x: u32 = 12345;
    while v.len() < 4 << 20 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mot = ["outils", "jeu", "minecraft", "fichier", "données", "réseau", "pc", "test", "vitesse"][(x % 9) as usize];
        v.extend_from_slice(mot.as_bytes());
        v.push(if x % 7 == 0 { b'\n' } else { b' ' });
        v.extend_from_slice(&(x % 1000).to_string().into_bytes());
    }
    v
}

fn compresser_pendant(d: &[u8], duree: Duration) -> f32 {
    let t = Instant::now();
    let mut octets = 0usize;
    while t.elapsed() < duree {
        let mut e = flate2::write::DeflateEncoder::new(Vec::with_capacity(d.len() / 3), flate2::Compression::new(6));
        let _ = e.write_all(d);
        let _ = e.finish();
        octets += d.len();
    }
    octets as f32 / 1048576.0 / t.elapsed().as_secs_f32()
}

fn info_materiel() -> (String, u32, String, f32) {
    let cpu = crate::optimiseur::reg_texte(r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString").unwrap_or_default().trim().to_string();
    let fils = std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1);
    let gpu = crate::optimiseur::ps("(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notmatch 'Basic|Virtual|Parsec|Remote' } | Sort-Object AdapterRAM -Descending | Select-Object -First 1).Name").unwrap_or_default().trim().to_string();
    let mut m: windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of_val(&m) as u32;
    unsafe { windows_sys::Win32::System::SystemInformation::GlobalMemoryStatusEx(&mut m) };
    (cpu, fils, gpu, m.ullTotalPhys as f32 / (1u64 << 30) as f32)
}

/// Lecture sans le cache de Windows (sinon on mesure la mémoire, pas le disque).
fn lire_sans_cache(p: &std::path::Path) -> std::io::Result<f32> {
    use std::os::windows::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(0x2000_0000).open(p)?; // FILE_FLAG_NO_BUFFERING
    let couche = std::alloc::Layout::from_size_align(4 << 20, 4096).map_err(|_| std::io::Error::other("alloc"))?;
    let ptr = unsafe { std::alloc::alloc(couche) };
    if ptr.is_null() {
        return Err(std::io::Error::other("alloc"));
    }
    let tampon = unsafe { std::slice::from_raw_parts_mut(ptr, 4 << 20) };
    let t = Instant::now();
    let mut total = 0usize;
    let r = loop {
        match f.read(tampon) {
            Ok(0) => break Ok(()),
            Ok(n) => total += n,
            Err(e) => break Err(e),
        }
    };
    unsafe { std::alloc::dealloc(ptr, couche) };
    r?;
    Ok(total as f32 / 1048576.0 / t.elapsed().as_secs_f32())
}

pub fn mesurer(etat: &Mutex<String>) -> Resultat {
    let (cpu_nom, fils, gpu_nom, ram_go) = info_materiel();
    let mut r = Resultat { date: crate::minecraft::date_lisible(), cpu_nom, fils, gpu_nom, ram_go, ..Default::default() };
    let d = Arc::new(donnees_test());
    *etat.lock().unwrap() = "Processeur, 1 cœur…".into();
    r.un_coeur = compresser_pendant(&d, Duration::from_secs(3));
    *etat.lock().unwrap() = format!("Processeur, {fils} fils…");
    let h: Vec<_> = (0..fils).map(|_| {
        let d = d.clone();
        std::thread::spawn(move || compresser_pendant(&d, Duration::from_secs(3)))
    }).collect();
    r.tous = h.into_iter().map(|x| x.join().unwrap_or(0.0)).sum();
    *etat.lock().unwrap() = "Mémoire…".into();
    let a = vec![7u8; 256 << 20];
    let mut b = vec![0u8; 256 << 20];
    let t = Instant::now();
    for _ in 0..8 {
        b.copy_from_slice(&a);
        std::hint::black_box(&b);
    }
    r.memoire = 8.0 * 0.25 / t.elapsed().as_secs_f32();
    drop((a, b));
    *etat.lock().unwrap() = "Disque (écriture de 1 Go)…".into();
    let f = std::env::temp_dir().join("outils_test_disque.bin");
    let bloc = vec![0x5Au8; 4 << 20];
    let t = Instant::now();
    let ecrit = (|| -> std::io::Result<()> {
        let mut o = std::fs::File::create(&f)?;
        for _ in 0..256 {
            o.write_all(&bloc)?;
        }
        o.sync_all()
    })();
    if ecrit.is_ok() {
        r.ecriture = 1024.0 / t.elapsed().as_secs_f32();
        *etat.lock().unwrap() = "Disque (lecture)…".into();
        r.lecture = lire_sans_cache(&f).unwrap_or(0.0);
    }
    let _ = std::fs::remove_file(&f);
    r
}

pub struct Banc {
    historique: Arc<Mutex<Vec<Resultat>>>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
}

impl Banc {
    pub fn new() -> Self {
        Banc { historique: Arc::new(Mutex::new(charger("banc_pc"))), etat: Arc::default(), occupe: Arc::default() }
    }
}

fn barre(ui: &mut egui::Ui, nom: &str, v: f32, unite: &str, repere: f32, avant: Option<f32>) {
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 18.0], egui::Label::new(nom));
        let f = (v / (repere * 1.5)).clamp(0.02, 1.0);
        let c = if v >= repere { Color32::from_rgb(90, 200, 120) } else if v >= repere * 0.6 { Color32::from_rgb(240, 200, 70) } else { Color32::from_rgb(230, 90, 90) };
        ui.add(egui::ProgressBar::new(f).desired_width(260.0).fill(c).text(format!("{v:.0} {unite}")));
        if let Some(a) = avant.filter(|a| *a > 0.0) {
            let d = (v - a) / a * 100.0;
            if d.abs() >= 5.0 {
                ui.colored_label(if d > 0.0 { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 150, 120) }, format!("{d:+.0} % depuis la dernière fois"));
            }
        }
    });
}

impl Outil for Banc {
    fn nom(&self) -> &'static str {
        "🏁 Comparateur de performances"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparateur de performances");
        ui.weak("Mesure ton processeur, ta mémoire et ton disque en une minute, compare avec tes mesures précédentes et un PC de jeu actuel, et te dit quelle pièce changer en premier. Ferme les jeux avant.");
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new("▶ Lancer la mesure")).clicked() {
                self.occupe.store(true, Relaxed);
                let (h, e, o) = (self.historique.clone(), self.etat.clone(), self.occupe.clone());
                std::thread::spawn(move || {
                    let r = mesurer(&e);
                    let mut v = h.lock().unwrap();
                    v.insert(0, r);
                    v.truncate(30);
                    enregistrer("banc_pc", &*v);
                    *e.lock().unwrap() = String::new();
                    o.store(false, Relaxed);
                });
            }
            if occupe {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
        });
        let h = self.historique.lock().unwrap().clone();
        let Some(r) = h.first() else {
            ui.weak("Pas encore de mesure.");
            return;
        };
        let avant = h.get(1);
        ui.separator();
        ui.label(format!("Mesure du {}", r.date));
        ui.label(format!("🧮 {} ({} fils) · 🎮 {} · 🧠 {:.0} Go", r.cpu_nom, r.fils, if r.gpu_nom.is_empty() { "carte graphique inconnue" } else { &r.gpu_nom }, r.ram_go));
        ui.add_space(4.0);
        ui.weak("Repère : la barre devient verte au niveau d'un PC de jeu correct d'aujourd'hui.");
        barre(ui, "Processeur, 1 cœur", r.un_coeur, "Mo/s", 60.0, avant.map(|a| a.un_coeur));
        barre(ui, "Processeur, tous", r.tous, "Mo/s", 450.0, avant.map(|a| a.tous));
        barre(ui, "Mémoire", r.memoire, "Go/s", 12.0, avant.map(|a| a.memoire));
        barre(ui, "Disque, écriture", r.ecriture, "Mo/s", 1000.0, avant.map(|a| a.ecriture));
        barre(ui, "Disque, lecture", r.lecture, "Mo/s", 1500.0, avant.map(|a| a.lecture));
        match niveau_gpu(&r.gpu_nom) {
            Some(g) => barre(ui, "Carte graphique", g as f32, "/ 100", 45.0, avant.and_then(|a| niveau_gpu(&a.gpu_nom)).map(|x| x as f32)),
            None => {
                ui.weak("Carte graphique : modèle pas dans la liste d'Outils (le conseil se base sur le reste).");
            }
        }
        ui.separator();
        let (titre, texte) = conseil(r);
        ui.label(RichText::new(format!("À changer en premier : {titre}")).size(17.0).strong());
        ui.label(texte);
        if h.len() > 1 {
            ui.separator();
            egui::CollapsingHeader::new(format!("Historique ({} mesures)", h.len())).show(ui, |ui| {
                egui::Grid::new("banc_hist").striped(true).show(ui, |ui| {
                    for t in ["Date", "1 cœur", "Tous", "Mémoire", "Écriture", "Lecture"] {
                        ui.strong(t);
                    }
                    ui.end_row();
                    for x in &h {
                        ui.label(&x.date);
                        ui.label(format!("{:.0}", x.un_coeur));
                        ui.label(format!("{:.0}", x.tous));
                        ui.label(format!("{:.1}", x.memoire));
                        ui.label(format!("{:.0}", x.ecriture));
                        ui.label(format!("{:.0}", x.lecture));
                        ui.end_row();
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn niveaux_et_conseils() {
        assert_eq!(niveau_gpu("NVIDIA GeForce RTX 4070 Ti SUPER"), Some(70));
        assert_eq!(niveau_gpu("NVIDIA GeForce RTX 4070"), Some(60));
        assert_eq!(niveau_gpu("AMD Radeon RX 7900 XTX"), Some(86));
        assert_eq!(niveau_gpu("Intel(R) UHD Graphics 620"), Some(4));
        assert_eq!(niveau_gpu("Carte inconnue"), None);
        let bon = Resultat { gpu_nom: "RTX 4070".into(), fils: 16, ram_go: 32.0, ecriture: 2000.0, ..Default::default() };
        assert!(conseil(&bon).0.contains("Rien"));
        assert!(conseil(&Resultat { ecriture: 110.0, ..bon.clone() }).0.contains("disque"));
        assert!(conseil(&Resultat { ram_go: 8.0, ..bon.clone() }).0.contains("mémoire"));
        assert!(conseil(&Resultat { gpu_nom: "GTX 1050".into(), ..bon.clone() }).0.contains("graphique"));
        assert!(conseil(&Resultat { fils: 4, ..bon.clone() }).0.contains("processeur"));
        let d = donnees_test();
        assert!(d.len() >= 4 << 20);
        assert!(compresser_pendant(&d[..100_000], Duration::from_millis(50)) > 0.0);
    }
}
