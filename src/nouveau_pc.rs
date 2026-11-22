//! Assistant « nouveau PC » : sur l'ancien PC, Outils prépare un dossier de déménagement (liste des
//! programmes, réglages d'Outils, sauvegardes de jeux, temps de jeu) sur une clé ou un disque ;
//! sur le nouveau, il réinstalle les programmes, remet les réglages et les sauvegardes, puis guide.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

const MANIFESTE: &str = "demenagement.json";

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Manifeste {
    pub date: String,
    pub pc: String,
    pub profil: String,
    pub jeux: Vec<(String, String, String)>, // (nom, dossier d'origine, archive)
}

fn appdata(s: &str) -> PathBuf {
    PathBuf::from(std::env::var(s).unwrap_or_default())
}

/// Chemin d'origine adapté au nouveau PC (le dossier utilisateur peut avoir un autre nom).
pub fn adapter(chemin: &str, ancien_profil: &str, nouveau_profil: &str) -> String {
    if !ancien_profil.is_empty() && chemin.to_lowercase().starts_with(&ancien_profil.to_lowercase()) {
        format!("{nouveau_profil}{}", &chemin[ancien_profil.len()..])
    } else {
        chemin.to_string()
    }
}

/// Zippe un dossier en sautant les fichiers énormes et les caches (re-téléchargeables).
fn zipper_filtre(src: &Path, dest: &Path, ignorer: &[&str]) -> Result<usize, String> {
    let mut z = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| e.to_string())?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let mut n = 0;
    let mut pile = vec![src.to_path_buf()];
    while let Some(d) = pile.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let nom = e.file_name().to_string_lossy().to_lowercase();
            if ignorer.iter().any(|i| nom == *i) {
                continue;
            }
            if p.is_dir() {
                pile.push(p);
                continue;
            }
            if e.metadata().map(|m| m.len() > 200 << 20).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&p) else { continue };
            let rel = p.strip_prefix(src).map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
            z.start_file(rel, opt).map_err(|e| e.to_string())?;
            z.write_all(&data).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(n)
}

fn dezipper(archive: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    z.extract(dest).map_err(|e| e.to_string())
}

pub fn preparer(dossier: &Path, etat: &Mutex<String>) -> Result<String, String> {
    let d = dossier.join(format!("Outils - déménagement {}", crate::minecraft::horodatage().get(..10).unwrap_or("")));
    std::fs::create_dir_all(d.join("jeux")).map_err(|e| e.to_string())?;
    let mut bilan = vec![];
    *etat.lock().unwrap() = "Liste des programmes (winget)…".into();
    match crate::majlogiciels::winget(&["export", "-o", &format!("\"{}\"", d.join("programmes.json").display()), "--accept-source-agreements", "--disable-interactivity"]) {
        Ok(_) if d.join("programmes.json").is_file() => bilan.push("liste des programmes".to_string()),
        _ => bilan.push("⚠ liste des programmes impossible (winget absent ?)".into()),
    }
    *etat.lock().unwrap() = "Réglages d'Outils…".into();
    let n = zipper_filtre(&appdata("APPDATA").join("Outils"), &d.join("reglages_outils.zip"), &["jaquettes", "abimes", "index_fichiers", "cache"])?;
    bilan.push(format!("{n} réglages d'Outils"));
    let lanceur = appdata("APPDATA").join("LanceurJeux");
    if lanceur.is_dir() {
        let _ = zipper_filtre(&lanceur, &d.join("lanceur_jeux.zip"), &[]);
        bilan.push("temps de jeu".into());
    }
    // sauvegardes de jeux : celles suivies dans « Sauvegardes jeux » et celles trouvées automatiquement
    let r: crate::sauvegardes::Reglages = crate::outil::charger("sauvegardes");
    let mut jeux: Vec<(String, PathBuf)> = r.jeux.iter().map(|e| (e.nom.clone(), e.dossier.clone())).collect();
    for (n, p) in crate::sauvegardes::suggestions() {
        if !jeux.iter().any(|j| j.1 == p) {
            jeux.push((n, p));
        }
    }
    let mut m = Manifeste { date: crate::minecraft::horodatage(), pc: std::env::var("COMPUTERNAME").unwrap_or_default(), profil: std::env::var("USERPROFILE").unwrap_or_default(), jeux: vec![] };
    for (k, (nom, p)) in jeux.iter().enumerate() {
        if !p.is_dir() {
            continue;
        }
        *etat.lock().unwrap() = format!("Sauvegardes de jeux {}/{} : {nom}", k + 1, jeux.len());
        let a = format!("{}.zip", crate::sauvegardes::nom_sur(nom));
        if zipper_filtre(p, &d.join("jeux").join(&a), &[]).is_ok() {
            m.jeux.push((nom.clone(), p.display().to_string(), a));
        }
    }
    bilan.push(format!("{} sauvegarde(s) de jeux", m.jeux.len()));
    std::fs::write(d.join(MANIFESTE), serde_json::to_string_pretty(&m).unwrap_or_default()).map_err(|e| e.to_string())?;
    let _ = std::fs::write(
        d.join("LISEZMOI.txt"),
        "Dossier de déménagement d'Outils.\r\nSur le nouveau PC : installe Outils, ouvre Système › Nouveau PC, choisis « J'arrive sur ce PC » et sélectionne ce dossier.\r\n",
    );
    Ok(format!("✔ Prêt dans {} : {}", d.display(), bilan.join(", ")))
}

pub fn installer(d: &Path, programmes: bool, etat: &Mutex<String>) -> Result<String, String> {
    let m: Manifeste = serde_json::from_str(&std::fs::read_to_string(d.join(MANIFESTE)).map_err(|_| "ce dossier n'est pas un dossier de déménagement d'Outils".to_string())?).map_err(|e| e.to_string())?;
    let mut bilan = vec![];
    let profil = std::env::var("USERPROFILE").unwrap_or_default();
    *etat.lock().unwrap() = "Sauvegardes de jeux…".into();
    let mut n = 0;
    for (nom, origine, a) in &m.jeux {
        let cible = PathBuf::from(adapter(origine, &m.profil, &profil));
        if cible.is_dir() && std::fs::read_dir(&cible).map(|mut r| r.next().is_some()).unwrap_or(false) {
            // déjà des sauvegardes ici : on garde une copie avant de remplacer
            let _ = zipper_filtre(&cible, &cible.with_extension("avant-demenagement.zip"), &[]);
        }
        *etat.lock().unwrap() = format!("Sauvegardes : {nom}");
        if dezipper(&d.join("jeux").join(a), &cible).is_ok() {
            n += 1;
        }
    }
    bilan.push(format!("{n} sauvegarde(s) de jeux remises"));
    if d.join("lanceur_jeux.zip").is_file() && dezipper(&d.join("lanceur_jeux.zip"), &appdata("APPDATA").join("LanceurJeux")).is_ok() {
        bilan.push("temps de jeu".into());
    }
    *etat.lock().unwrap() = "Réglages d'Outils…".into();
    let reglages = appdata("APPDATA").join("Outils");
    if d.join("reglages_outils.zip").is_file() {
        let _ = zipper_filtre(&reglages, &d.join(format!("reglages_avant_{}.zip", std::env::var("COMPUTERNAME").unwrap_or_default())), &["jaquettes"]);
        dezipper(&d.join("reglages_outils.zip"), &reglages)?;
        bilan.push("réglages d'Outils (relance Outils pour les voir)".into());
    }
    if programmes && d.join("programmes.json").is_file() {
        *etat.lock().unwrap() = "Installation des programmes (winget) : ça peut prendre longtemps…".into();
        match crate::majlogiciels::winget(&["import", "-i", &format!("\"{}\"", d.join("programmes.json").display()), "--accept-package-agreements", "--accept-source-agreements", "--ignore-unavailable", "--disable-interactivity"]) {
            Ok(s) => bilan.push(format!("programmes : {} installé(s)", s.lines().filter(|l| l.contains("Installé") || l.contains("Successfully installed")).count())),
            Err(e) => bilan.push(format!("⚠ programmes : {e}")),
        }
    }
    Ok(format!("✔ {}", bilan.join(", ")))
}

pub struct NouveauPc {
    etat: Arc<Mutex<String>>,
    fini: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    programmes: bool,
    coches: Vec<bool>,
}

const ETAPES: &[(&str, Option<&str>)] = &[
    ("Faire les mises à jour Windows (Paramètres › Windows Update), puis redémarrer", None),
    ("Installer les pilotes de la carte graphique", Some("🆙 Pilotes à jour")),
    ("Se connecter à Steam / Epic / les autres lanceurs ; si tu as gardé le disque de jeux, Steam › Paramètres › Stockage › ajouter le dossier", Some("🎮 Lanceur Jeux")),
    ("Régler les performances (plan d'alimentation, démarrage)", Some("🚀 Optimiseur")),
    ("Associer à nouveau ton téléphone", Some("📱 Téléphone")),
    ("Vérifier que tout va bien", Some("💯 Bilan du PC")),
];

impl NouveauPc {
    pub fn new() -> Self {
        NouveauPc { etat: Arc::default(), fini: Arc::default(), occupe: Arc::default(), programmes: true, coches: vec![false; ETAPES.len()] }
    }

    fn en_fond(&self, f: impl FnOnce(&Mutex<String>) -> Result<String, String> + Send + 'static) {
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        let (e, fini, o) = (self.etat.clone(), self.fini.clone(), self.occupe.clone());
        std::thread::spawn(move || {
            let r = f(&e).unwrap_or_else(|x| format!("✖ {x}"));
            *fini.lock().unwrap() = r;
            *e.lock().unwrap() = String::new();
            o.store(false, Relaxed);
        });
    }
}

impl Outil for NouveauPc {
    fn nom(&self) -> &'static str {
        "🚚 Nouveau PC"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Assistant « nouveau PC »");
        ui.weak("Tu changes de PC ? Outils emporte la liste de tes programmes, ses réglages, tes sauvegardes de jeux et ton temps de jeu, puis remet tout en place sur le nouveau.");
        let occupe = self.occupe.load(Relaxed);
        if occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
        }
        let fini = self.fini.lock().unwrap().clone();
        if !fini.is_empty() {
            ui.label(RichText::new(fini).color(Color32::LIGHT_GREEN));
        }
        ui.separator();
        ui.strong("1. Sur l'ancien PC : « Je pars »");
        ui.label("Choisis une clé USB ou un disque externe (quelques Go suffisent en général).");
        if ui.add_enabled(!occupe, egui::Button::new("📦 Préparer le déménagement…")).clicked() {
            if let Some(d) = rfd::FileDialog::new().set_title("Où mettre le dossier de déménagement ?").pick_folder() {
                self.en_fond(move |e| preparer(&d, e));
            }
        }
        ui.separator();
        ui.strong("2. Sur le nouveau PC : « J'arrive »");
        ui.checkbox(&mut self.programmes, "Réinstaller aussi les programmes (winget, peut prendre du temps)");
        if ui.add_enabled(!occupe, egui::Button::new("📥 Installer depuis un dossier de déménagement…")).clicked() {
            if let Some(d) = rfd::FileDialog::new().set_title("Dossier « Outils - déménagement … »").pick_folder() {
                let p = self.programmes;
                self.en_fond(move |e| installer(&d, p, e));
            }
        }
        ui.separator();
        ui.strong("3. Ensuite, à faire");
        let mut ouvrir = None;
        for (k, (t, outil)) in ETAPES.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.coches[k], *t);
                if let Some(o) = outil {
                    if ui.small_button("Ouvrir").clicked() {
                        ouvrir = Some(*o);
                    }
                }
            });
        }
        if let Some(o) = ouvrir {
            *crate::interface2::OUVRIR.lock().unwrap() = Some(o.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chemins() {
        assert_eq!(adapter(r"C:\Users\Ancien\Saved Games\Jeu", r"C:\Users\Ancien", r"C:\Users\Nouveau"), r"C:\Users\Nouveau\Saved Games\Jeu");
        assert_eq!(adapter(r"D:\Jeux\Saves", r"C:\Users\Ancien", r"C:\Users\Nouveau"), r"D:\Jeux\Saves");
        assert_eq!(adapter(r"c:\users\ancien\x", r"C:\Users\Ancien", r"C:\Users\N"), r"C:\Users\N\x");
    }

    #[test]
    fn aller_retour_zip() {
        let b = std::env::temp_dir().join(format!("outils_dem_{}", std::process::id()));
        std::fs::create_dir_all(b.join("src/cache")).unwrap();
        std::fs::write(b.join("src/a.json"), "{}").unwrap();
        std::fs::write(b.join("src/cache/gros.bin"), "x").unwrap();
        let n = zipper_filtre(&b.join("src"), &b.join("r.zip"), &["cache"]).unwrap();
        assert_eq!(n, 1);
        dezipper(&b.join("r.zip"), &b.join("dst")).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("dst/a.json")).unwrap(), "{}");
        assert!(!b.join("dst/cache").exists());
        let _ = std::fs::remove_dir_all(&b);
    }
}
