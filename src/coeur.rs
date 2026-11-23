//! Le cœur d'Outils : rapports de plantage (1), sauvegarde et restauration de tous les réglages (2),
//! synchronisation entre PC par un dossier synchronisé (3), journal de tout ce que font les outils (8),
//! mode portable (9).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

fn appdata() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into()))
}

fn horodatage() -> String {
    crate::minecraft::horodatage()
}

// ---------------------------------------------------------------------------------------------------
// 9 · Mode portable
// ---------------------------------------------------------------------------------------------------

/// Dossier des données à côté de l'exe (mode portable).
pub fn dossier_portable() -> Option<PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("Outils-donnees"))
}

/// À appeler tout au début : si le dossier « Outils-donnees » existe à côté de l'exe, toutes les données
/// d'Outils (et de ses outils) y sont lues et écrites au lieu de %APPDATA%.
pub fn activer_portable() -> bool {
    match dossier_portable() {
        Some(d) if d.is_dir() => {
            std::env::set_var("APPDATA", &d);
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------
// 6 · Palette de commandes : Ctrl+K dans Outils, Ctrl+Alt+K de partout
// ---------------------------------------------------------------------------------------------------

pub static PALETTE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn raccourci_palette() {
    std::thread::spawn(|| unsafe {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
        if crate::outil::raccourci(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'K' as u32) == 0 {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                PALETTE.store(true, std::sync::atomic::Ordering::Relaxed);
                crate::systeme::montrer();
            }
        }
    });
}

/// Correspondance souple : toutes les lettres de la recherche, dans l'ordre (sans accents ni casse).
pub fn correspond(texte: &str, q: &str) -> Option<usize> {
    let norm = |s: &str| -> Vec<char> {
        s.to_lowercase().chars().map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'à' | 'â' => 'a',
            'î' | 'ï' => 'i',
            'ô' => 'o',
            'û' | 'ù' => 'u',
            'ç' => 'c',
            x => x,
        }).filter(|c| c.is_alphanumeric() || *c == ' ').collect()
    };
    let (t, q) = (norm(texte), norm(q));
    if q.is_empty() {
        return Some(0);
    }
    let ts: String = t.iter().collect();
    let qs: String = q.iter().filter(|c| **c != ' ').collect();
    if let Some(p) = ts.find(&q.iter().collect::<String>()) {
        return Some(p); // bout de texte trouvé tel quel : en premier
    }
    let mut i = 0;
    let mut ecart = 0;
    for c in qs.chars() {
        let p = t[i..].iter().position(|x| *x == c)?;
        ecart += p;
        i += p + 1;
    }
    Some(1000 + ecart)
}

// ---------------------------------------------------------------------------------------------------
// 8 · Journal
// ---------------------------------------------------------------------------------------------------

static RECENT: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn dossier_journal() -> PathBuf {
    appdata().join("Outils").join("journal")
}

fn jour() -> String {
    crate::quotidien::date_locale(crate::pressepapiers::maintenant_utc(), crate::pressepapiers::decalage())
}

/// Note une ligne dans le journal du jour (appelé notamment par chaque notification).
pub fn noter(source: &str, texte: &str) {
    let off = crate::pressepapiers::decalage();
    let m = (crate::pressepapiers::maintenant_utc() + off).rem_euclid(86400);
    let l = format!("{:02}:{:02} · {source} · {}", m / 3600, m / 60 % 60, texte.replace('\n', " "));
    {
        let mut r = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        r.push(l.clone());
        let n = r.len();
        if n > 500 {
            r.drain(..n - 500);
        }
    }
    let d = dossier_journal();
    let _ = std::fs::create_dir_all(&d);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join(format!("{}.txt", jour()))) {
        use std::io::Write;
        let _ = writeln!(f, "{l}");
    }
}

// ---------------------------------------------------------------------------------------------------
// 1 · Plantages
// ---------------------------------------------------------------------------------------------------

/// Onglet affiché, pour le rapport.
pub static ONGLET: Mutex<String> = Mutex::new(String::new());

fn dossier_plantages() -> PathBuf {
    appdata().join("Outils").join("plantages")
}

/// Écrit un rapport à chaque panique (dans n'importe quel fil), sans empêcher le reste de tourner.
pub fn installer_crochet() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let ou = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let fil = std::thread::current().name().unwrap_or("?").to_string();
        let onglet = ONGLET.lock().map(|o| o.clone()).unwrap_or_default();
        let trace = std::backtrace::Backtrace::force_capture();
        let rapport = format!(
            "Outils v{}\nDate : {}\nOnglet : {onglet}\nFil : {fil}\nErreur : {msg}\nÀ : {ou}\n\nTrace :\n{trace}\n",
            env!("CARGO_PKG_VERSION"), horodatage()
        );
        let d = dossier_plantages();
        let _ = std::fs::create_dir_all(&d);
        let _ = std::fs::write(d.join(format!("plantage_{}.txt", horodatage())), rapport);
        noter("Plantage", &format!("{onglet} : {msg} ({ou})"));
    }));
}

/// Rapports pas encore vus (pour l'avertissement au démarrage).
pub fn nouveaux_plantages() -> usize {
    let vus: Vec<String> = charger("plantages_vus");
    rapports().iter().filter(|p| !vus.contains(&p.file_name().unwrap_or_default().to_string_lossy().into_owned())).count()
}

fn rapports() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dossier_plantages()).into_iter().flatten().flatten().map(|e| e.path()).collect();
    v.sort();
    v.reverse();
    v
}

pub fn marquer_vus() {
    let tous: Vec<String> = rapports().iter().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
    enregistrer("plantages_vus", &tous);
}

pub struct Journal {
    jour: String,
    texte: (String, String),
    choisi: Option<PathBuf>,
}

impl Journal {
    pub fn new() -> Self {
        Journal { jour: String::new(), texte: (String::new(), String::new()), choisi: None }
    }
}

impl Outil for Journal {
    fn nom(&self) -> &'static str {
        "📜 Journal et plantages"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Journal et plantages");
        marquer_vus();
        let liste = rapports();
        if !liste.is_empty() {
            ui.strong(format!("💥 {} rapport(s) de plantage", liste.len()));
            ui.weak("Copie le rapport et envoie-le-moi : il dit exactement où et pourquoi ça a planté.");
            for p in liste.iter().take(10) {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.choisi.as_ref() == Some(p), p.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                        self.choisi = Some(p.clone());
                    }
                    if ui.small_button("📋 Copier").clicked() {
                        ui.ctx().copy_text(std::fs::read_to_string(p).unwrap_or_default());
                    }
                    if ui.small_button("🗑").clicked() {
                        let _ = std::fs::remove_file(p);
                    }
                });
            }
            if let Some(p) = &self.choisi {
                let t = std::fs::read_to_string(p).unwrap_or_default();
                egui::ScrollArea::vertical().id_salt("rapport").max_height(180.0).show(ui, |ui| {
                    ui.monospace(t.lines().take(40).collect::<Vec<_>>().join("\n"));
                });
            }
            ui.separator();
        }
        ui.strong("📜 Journal");
        ui.weak("Tout ce que font les outils (notifications, sauvegardes, alertes…), jour par jour.");
        let mut jours: Vec<String> = std::fs::read_dir(dossier_journal()).into_iter().flatten().flatten()
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned())).collect();
        jours.sort();
        jours.reverse();
        if self.jour.is_empty() {
            self.jour = jour();
        }
        ui.horizontal_wrapped(|ui| {
            for j in jours.iter().take(14) {
                ui.selectable_value(&mut self.jour, j.clone(), &j[5..]);
            }
        });
        if self.texte.0 != self.jour || self.jour == jour() {
            let t = std::fs::read_to_string(dossier_journal().join(format!("{}.txt", self.jour))).unwrap_or_default();
            self.texte = (self.jour.clone(), t);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        egui::ScrollArea::vertical().id_salt("journal_lignes").show(ui, |ui| {
            for l in self.texte.1.lines().rev() {
                ui.label(l);
            }
            if self.texte.1.is_empty() {
                ui.weak("Rien ce jour-là.");
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 2 · Sauvegarde des réglages / 3 · Synchronisation / 9 · portable
// ---------------------------------------------------------------------------------------------------

/// Données de tous les outils, relatives à %APPDATA%.
pub const DONNEES: &[&str] = &[
    "Outils", "LanceurJeux", "CoffreMotsDePasse", "GestionnairePressePapiers", "GestionServeurMinecraft.json", "GestionServeurMinecraft", "TraducteurSousTitres.json",
];

/// Faut-il ignorer ce chemin (relatif) dans une sauvegarde ? (gros programmes, caches, copie de l'économiseur)
pub fn ignorer(rel: &str) -> bool {
    let r = rel.replace('\\', "/").to_lowercase();
    r.starts_with("outils/bin/") || r.starts_with("outils/whisper/") || r.starts_with("outils/plantages/") || r.starts_with("outils/avant_restauration_") || r == "outils/outils.scr" || r.ends_with(".part")
}

pub fn exporter(base: &Path, dest: &Path) -> Result<usize, String> {
    let mut z = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| e.to_string())?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let mut n = 0;
    for d in DONNEES {
        let mut pile = vec![base.join(d)];
        while let Some(p) = pile.pop() {
            if p.is_dir() {
                pile.extend(std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()));
            } else if p.is_file() {
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                if ignorer(&rel) {
                    continue;
                }
                let Ok(mut f) = std::fs::File::open(&p) else { continue };
                z.start_file(rel, opt).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut z).map_err(|e| e.to_string())?;
                n += 1;
            }
        }
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(n)
}

/// Restaure une sauvegarde (l'état actuel est d'abord exporté à côté, par sécurité).
pub fn importer(base: &Path, zip: &Path) -> Result<PathBuf, String> {
    let securite = base.join("Outils").join(format!("avant_restauration_{}.zip", horodatage()));
    let _ = std::fs::create_dir_all(base.join("Outils"));
    exporter(base, &securite)?;
    let mut a = zip::ZipArchive::new(std::fs::File::open(zip).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    // seulement les dossiers connus : une archive ne peut rien écrire d'autre
    for i in 0..a.len() {
        let mut e = a.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = e.enclosed_name() else { continue };
        let premier = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        if !DONNEES.contains(&premier.as_str()) || e.is_dir() {
            continue;
        }
        let cible = base.join(&rel);
        let _ = std::fs::create_dir_all(cible.parent().unwrap_or(base));
        let mut f = std::fs::File::create(&cible).map_err(|e| e.to_string())?;
        std::io::copy(&mut e, &mut f).map_err(|e| e.to_string())?;
    }
    Ok(securite)
}

fn nom_pc() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
struct Synchro {
    dossier: Option<PathBuf>,
    auto: bool,
    derniere: i64,
}

pub struct Sauvegarde {
    s: Synchro,
    message: std::sync::Arc<Mutex<String>>,
}

impl Sauvegarde {
    pub fn new() -> Self {
        let s: Synchro = charger("synchro");
        let message: std::sync::Arc<Mutex<String>> = Default::default();
        // envoi automatique toutes les heures vers le dossier synchronisé
        if s.auto {
            if let Some(d) = s.dossier.clone() {
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_secs(600));
                    let _ = exporter(&appdata(), &d.join(format!("Outils-{}.zip", nom_pc())));
                    std::thread::sleep(Duration::from_secs(3000));
                });
            }
        }
        Sauvegarde { s, message }
    }
}

impl Outil for Sauvegarde {
    fn nom(&self) -> &'static str {
        "💼 Sauvegarde, synchro, portable"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegarde, synchronisation et mode portable");
        let avant = self.s.clone();
        ui.strong("💾 Sauvegarder tous les réglages");
        ui.weak("Notes, tâches, coffre (toujours chiffré), jeux, serveur, raccourcis, thème… dans un seul fichier .zip, à remettre sur le nouveau PC.");
        ui.horizontal(|ui| {
            if ui.button("📤 Exporter…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Zip", &["zip"]).set_file_name(format!("Outils-reglages-{}.zip", horodatage())).save_file() {
                    *self.message.lock().unwrap() = exporter(&appdata(), &p).map(|n| format!("✔ {n} fichiers sauvegardés dans {}", p.display())).unwrap_or_else(|e| format!("✖ {e}"));
                }
            }
            if ui.button("📥 Restaurer…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Zip", &["zip"]).pick_file() {
                    *self.message.lock().unwrap() = importer(&appdata(), &p)
                        .map(|s| format!("✔ Restauré. Redémarre Outils pour tout recharger. (L'ancien état est gardé dans {})", s.display())).unwrap_or_else(|e| format!("✖ {e}"));
                }
            }
        });
        ui.separator();
        ui.strong("🔄 Synchroniser deux PC");
        ui.weak("Choisis un dossier synchronisé par OneDrive (déjà dans Windows) ou Google Drive : chaque PC y dépose ses réglages, et l'autre les récupère.");
        ui.horizontal(|ui| {
            match &self.s.dossier {
                Some(d) => ui.monospace(d.display().to_string()),
                None => ui.weak("aucun dossier"),
            };
            if ui.button("Choisir…").clicked() {
                let depart = std::env::var("OneDrive").map(PathBuf::from).ok();
                let mut f = rfd::FileDialog::new();
                if let Some(d) = depart {
                    f = f.set_directory(d);
                }
                if let Some(d) = f.pick_folder() {
                    self.s.dossier = Some(d.join("Outils-synchro"));
                }
            }
        });
        if let Some(d) = self.s.dossier.clone() {
            ui.horizontal(|ui| {
                if ui.button(format!("📤 Envoyer les réglages de ce PC ({})", nom_pc())).clicked() {
                    let _ = std::fs::create_dir_all(&d);
                    *self.message.lock().unwrap() = exporter(&appdata(), &d.join(format!("Outils-{}.zip", nom_pc()))).map(|n| format!("✔ {n} fichiers envoyés.")).unwrap_or_else(|e| format!("✖ {e}"));
                }
                ui.checkbox(&mut self.s.auto, "envoyer tout seul toutes les heures");
            });
            let autres: Vec<PathBuf> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "zip") && p.file_stem().is_some_and(|s| s.to_string_lossy() != format!("Outils-{}", nom_pc()))).collect();
            for p in autres {
                ui.horizontal(|ui| {
                    let date = p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| crate::notes::format_local(d.as_secs() as i64, crate::pressepapiers::decalage())).unwrap_or_default();
                    ui.label(format!("💻 {} · {date}", p.file_stem().unwrap_or_default().to_string_lossy().trim_start_matches("Outils-")));
                    if ui.button("📥 Récupérer ces réglages ici").clicked() {
                        *self.message.lock().unwrap() = importer(&appdata(), &p).map(|_| "✔ Récupéré : redémarre Outils.".to_string()).unwrap_or_else(|e| format!("✖ {e}"));
                    }
                });
            }
        }
        ui.separator();
        ui.strong("🔌 Mode portable (clé USB)");
        let portable = dossier_portable().is_some_and(|d| d.is_dir());
        if portable {
            ui.colored_label(Color32::LIGHT_GREEN, "✔ Outils est en mode portable : ses données sont dans « Outils-donnees » à côté de l'exe.");
        } else {
            ui.weak("Copie Outils.exe sur une clé, puis active le mode portable : toutes les données voyagent avec lui (rien n'est écrit sur le PC hôte, sauf les raccourcis Windows).");
            if ui.button("Activer le mode portable ici").clicked() {
                if let Some(d) = dossier_portable() {
                    let _ = std::fs::create_dir_all(&d);
                    let tmp = std::env::temp_dir().join("outils_portable.zip");
                    let r = exporter(&appdata(), &tmp).and_then(|_| {
                        let mut a = zip::ZipArchive::new(std::fs::File::open(&tmp).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                        a.extract(&d).map_err(|e| e.to_string())
                    });
                    let _ = std::fs::remove_file(tmp);
                    *self.message.lock().unwrap() = r.map(|_| "✔ Données copiées à côté de l'exe : redémarre Outils.".into()).unwrap_or_else(|e| format!("✖ {e}"));
                }
            }
        }
        let m = self.message.lock().unwrap().clone();
        if !m.is_empty() {
            ui.label(RichText::new(m));
        }
        if self.s != avant {
            enregistrer("synchro", &self.s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recherche_souple() {
        assert!(correspond("📸 Capture", "capt").is_some());
        assert!(correspond("🔌 Démarrage PC", "demarrage").is_some());
        assert!(correspond("Mixeur audio", "mxa").is_some()); // lettres dans l'ordre
        assert!(correspond("Mixeur audio", "zz").is_none());
        assert!(correspond("Capture", "cap").unwrap() < correspond("Calcul après", "cap").unwrap());
    }

    #[test]
    fn sauvegarde_aller_retour() {
        let b = std::env::temp_dir().join(format!("outils_coeur_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&b);
        std::fs::create_dir_all(b.join("Outils/bin")).unwrap();
        std::fs::create_dir_all(b.join("LanceurJeux")).unwrap();
        std::fs::create_dir_all(b.join("Autre")).unwrap();
        std::fs::write(b.join("Outils/notes.json"), "[1]").unwrap();
        std::fs::write(b.join("Outils/bin/ffmpeg.exe"), "gros").unwrap();
        std::fs::write(b.join("LanceurJeux/data.json"), "{}").unwrap();
        std::fs::write(b.join("Autre/x"), "pas à nous").unwrap();
        let z = b.join("s.zip");
        assert_eq!(exporter(&b, &z).unwrap(), 2); // ni bin/ ni Autre/
        std::fs::write(b.join("Outils/notes.json"), "[2]").unwrap();
        importer(&b, &z).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("Outils/notes.json")).unwrap(), "[1]");
        assert!(ignorer("Outils\\bin\\ffmpeg.exe") && !ignorer("Outils/notes.json"));
        let _ = std::fs::remove_dir_all(&b);
    }
}
