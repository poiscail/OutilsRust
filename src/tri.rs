//! Tri des téléchargements : range chaque nouveau fichier dans un sous-dossier selon son extension.
//! Réécriture de TriTelechargements/tri.py, mêmes règles (`regles.json`).
use eframe::egui::{self, Color32, RichText};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAUT: &str = include_str!("../regles.json");
const VERT: Color32 = Color32::from_rgb(0x3e, 0xcf, 0x8e);
const ROUGE: Color32 = Color32::from_rgb(0xef, 0x5f, 0x5f);

#[derive(Deserialize, Clone)]
pub struct Regles {
    pub dossier: String,
    pub ignorer: Vec<String>,
    pub en_cours: Vec<String>,
    pub categories: HashMap<String, Vec<String>>,
}

impl Regles {
    /// `regles.json` à côté de l'exe, sinon celui de l'ancien outil, sinon la copie embarquée.
    pub fn charger(racine: &Path) -> Result<(Regles, PathBuf), String> {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        let candidats = [exe_dir.map(|d| d.join("regles.json")), Some(racine.join("TriTelechargements").join("regles.json"))];
        for c in candidats.into_iter().flatten() {
            if c.is_file() {
                let texte = std::fs::read_to_string(&c).map_err(|e| format!("{}\n\n{e}", c.display()))?;
                return serde_json::from_str(&texte).map(|r| (r, c.clone())).map_err(|e| format!("{}\n\n{e}", c.display()));
            }
        }
        Ok((serde_json::from_str(DEFAUT).expect("regles.json embarqué"), PathBuf::from("(règles par défaut)")))
    }

    fn ext_vers_dossier(&self) -> HashMap<String, String> {
        self.categories.iter().flat_map(|(nom, exts)| exts.iter().map(move |e| (e.to_lowercase(), nom.clone()))).collect()
    }
}

/// "%USERPROFILE%/Downloads" -> "C:\Users\...\Downloads"
pub fn developper(s: &str) -> PathBuf {
    let mut out = String::new();
    let mut reste = s;
    while let Some(i) = reste.find('%') {
        out.push_str(&reste[..i]);
        match reste[i + 1..].find('%') {
            Some(j) => {
                let nom = &reste[i + 1..i + 1 + j];
                out.push_str(&std::env::var(nom).unwrap_or_else(|_| format!("%{nom}%")));
                reste = &reste[i + 2 + j..];
            }
            None => {
                reste = &reste[i..];
                break;
            }
        }
    }
    out.push_str(reste);
    PathBuf::from(out)
}

/// Jamais écraser : "photo.jpg" -> "photo (1).jpg" ; "x.tar.gz" -> "x (1).tar.gz".
pub fn chemin_libre(cible: &Path) -> PathBuf {
    let nom = cible.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = match nom.rfind('.') {
        Some(i) if i > 0 => {
            if nom[..i].ends_with(".tar") && i > 4 { &nom[i - 4..] } else { &nom[i..] }
        }
        _ => "",
    };
    let base = &nom[..nom.len() - ext.len()];
    let mut c = cible.to_path_buf();
    let mut n = 1;
    while c.exists() {
        c = cible.with_file_name(format!("{base} ({n}){ext}"));
        n += 1;
    }
    c
}

fn extension(f: &Path) -> String {
    f.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

/// Sous-dossier où ranger ce fichier, ou None s'il ne faut pas y toucher.
pub fn categorie(f: &Path, regles: &Regles, map: &HashMap<String, String>) -> Option<String> {
    let nom = f.file_name()?.to_string_lossy().into_owned();
    // "~$x.docx" : verrou d'Office, ".~lock" : LibreOffice
    if !f.is_file() || regles.ignorer.contains(&nom) || nom.starts_with("~$") || nom.starts_with(".~lock") {
        return None;
    }
    let ext = extension(f);
    if regles.en_cours.contains(&ext) {
        return None;
    }
    let cat = map.get(&ext)?;
    // Firefox crée "film.mp4" vide à côté de "film.mp4.part" : attendre la fin.
    if regles.en_cours.iter().any(|e| f.with_file_name(format!("{nom}{e}")).exists()) {
        return None;
    }
    Some(cat.clone())
}

/// Déplacement qui échoue (sans rien perdre) si la cible existe ou si le fichier est encore ouvert.
fn deplacer(de: &Path, vers: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let w = |p: &Path| p.as_os_str().encode_wide().chain([0]).collect::<Vec<u16>>();
    let (a, b) = (w(de), w(vers));
    // flags 0 : pas de MOVEFILE_REPLACE_EXISTING (std::fs::rename écraserait)
    unsafe { windows_sys::Win32::Storage::FileSystem::MoveFileExW(a.as_ptr(), b.as_ptr(), 0) != 0 }
}

/// Déplace un fichier ; renvoie la destination, ou None s'il faut l'ignorer / réessayer.
pub fn ranger(dossier: &Path, f: &Path, regles: &Regles, map: &HashMap<String, String>) -> Option<PathBuf> {
    let cat = categorie(f, regles, map)?;
    let dest = dossier.join(cat);
    if std::fs::create_dir(&dest).is_err() && !dest.is_dir() {
        return None;
    }
    let cible = chemin_libre(&dest.join(f.file_name()?));
    deplacer(f, &cible).then_some(cible)
}

/// Simulation : ce qui serait rangé dans le dossier. Ne déplace rien.
pub fn apercu(dossier: &Path, regles: &Regles) -> Vec<(PathBuf, String)> {
    let map = regles.ext_vers_dossier();
    let Ok(lecture) = std::fs::read_dir(dossier) else { return vec![] };
    lecture.flatten().filter_map(|e| categorie(&e.path(), regles, &map).map(|c| (e.path(), c))).collect()
}

fn noms(dossier: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dossier).map(|l| l.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

/// Boucle jusqu'à `arret`. Les fichiers présents au lancement ne sont jamais touchés.
fn surveiller(dossier: PathBuf, regles: Regles, arret: Arc<AtomicBool>, tx: mpsc::Sender<(String, PathBuf)>) {
    let map = regles.ext_vers_dossier();
    let mut deja_la: Vec<PathBuf> = noms(&dossier);
    let mut tailles: HashMap<PathBuf, u64> = HashMap::new();
    while !arret.load(Ordering::Relaxed) {
        let presents = noms(&dossier);
        deja_la.retain(|p| presents.contains(p)); // supprimé puis re-téléchargé : redevient "nouveau"
        tailles.retain(|p, _| presents.contains(p));
        for p in &presents {
            if deja_la.contains(p) {
                continue;
            }
            let Ok(meta) = p.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if tailles.get(p) != Some(&meta.len()) {
                tailles.insert(p.clone(), meta.len()); // attendre que la taille ne bouge plus
                continue;
            }
            if let Some(cible) = ranger(&dossier, p, &regles, &map) {
                tailles.remove(p);
                let _ = tx.send((p.file_name().unwrap_or_default().to_string_lossy().into_owned(), cible));
            }
        }
        for _ in 0..20 {
            if arret.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

fn heure() -> String {
    let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    format!("{:02}:{:02}", t.wHour, t.wMinute)
}

pub struct Tri {
    regles: Result<Regles, String>,
    dossier: PathBuf,
    arret: Arc<AtomicBool>,
    fil: Option<JoinHandle<()>>,
    tx: mpsc::Sender<(String, PathBuf)>,
    rx: mpsc::Receiver<(String, PathBuf)>,
    ranges: Vec<(String, String, PathBuf)>, // heure, nom, destination
    confirmer: Option<Vec<(PathBuf, String)>>,
    info: Option<String>,
}

impl Tri {
    pub fn new(racine: &Path) -> Self {
        let (tx, rx) = mpsc::channel();
        let regles = Regles::charger(racine).map(|(r, _)| r);
        let dossier = regles.as_ref().map(|r| developper(&r.dossier)).unwrap_or_default();
        let mut t = Tri { regles, dossier, arret: Arc::new(AtomicBool::new(true)), fil: None, tx, rx,
                          ranges: vec![], confirmer: None, info: None };
        t.demarrer();
        t
    }

    fn actif(&self) -> bool {
        !self.arret.load(Ordering::Relaxed)
    }

    fn demarrer(&mut self) {
        let Ok(regles) = self.regles.clone() else { return };
        if !self.dossier.is_dir() {
            return;
        }
        self.arret = Arc::new(AtomicBool::new(false));
        let (d, a, tx) = (self.dossier.clone(), self.arret.clone(), self.tx.clone());
        self.fil = Some(thread::spawn(move || surveiller(d, regles, a, tx)));
    }

    fn arreter(&mut self) {
        self.arret.store(true, Ordering::Relaxed);
    }

    /// À appeler à chaque image, même quand l'onglet n'est pas affiché.
    pub fn relever(&mut self) {
        while let Ok((nom, cible)) = self.rx.try_recv() {
            self.ranges.insert(0, (heure(), nom, cible));
            self.ranges.truncate(200);
        }
    }

    fn plante(&self) -> bool {
        self.actif() && self.fil.as_ref().is_some_and(|f| f.is_finished())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tri des téléchargements");
        let regles = match &self.regles {
            Ok(r) => r.clone(),
            Err(e) => {
                ui.colored_label(ROUGE, format!("regles.json illisible :\n{e}"));
                return;
            }
        };
        ui.label(RichText::new(self.dossier.display().to_string()).color(Color32::GRAY));
        if !self.dossier.is_dir() {
            ui.colored_label(ROUGE, "Dossier introuvable : vérifie \"dossier\" dans regles.json.");
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let (couleur, texte) = if self.plante() {
                (ROUGE, "Erreur : surveillance arrêtée")
            } else if self.actif() {
                (VERT, "Surveillance active")
            } else {
                (Color32::GRAY, "En pause")
            };
            // pastille dessinée : la police d'egui n'a pas le glyphe "●"
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 5.0, couleur);
            ui.colored_label(couleur, texte);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.actif() && !self.plante() {
                    if ui.add(egui::Button::new(RichText::new("Arrêter").color(Color32::BLACK)).fill(ROUGE)).clicked() {
                        self.arreter();
                    }
                } else if ui.add(egui::Button::new(RichText::new("Démarrer").color(Color32::BLACK)).fill(VERT)).clicked() {
                    self.demarrer();
                }
                if ui.button("Ouvrir le dossier").clicked() {
                    let _ = Command::new("explorer").arg(&self.dossier).spawn();
                }
                if ui.button("Ranger l'existant…").clicked() {
                    let prevu = apercu(&self.dossier, &regles);
                    if prevu.is_empty() {
                        self.info = Some("Rien à ranger.".into());
                    } else {
                        self.confirmer = Some(prevu);
                    }
                }
            });
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Derniers fichiers rangés (double-clic : montrer dans l'Explorateur)").color(Color32::GRAY));
        egui::Frame::group(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                if self.ranges.is_empty() {
                    ui.colored_label(Color32::GRAY, "Aucun fichier rangé pour l'instant.");
                }
                for (h, nom, cible) in &self.ranges {
                    let dossier = cible.parent().and_then(|p| p.strip_prefix(&self.dossier).ok()).unwrap_or(Path::new(""));
                    let texte = format!("{h}   {nom}   →  {}", dossier.display());
                    if ui.add(egui::Label::new(texte).sense(egui::Sense::click())).double_clicked() && cible.exists() {
                        let _ = Command::new("explorer").arg(format!("/select,{}", cible.display())).spawn();
                    }
                }
            });
        });
        self.fenetres(ui.ctx(), &regles);
    }

    fn fenetres(&mut self, ctx: &egui::Context, regles: &Regles) {
        if let Some(texte) = self.info.clone() {
            egui::Window::new("Ranger l'existant").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(texte);
                    if ui.button("OK").clicked() {
                        self.info = None;
                    }
                });
        }
        let Some(prevu) = self.confirmer.clone() else { return };
        // Simulation d'abord, puis confirmation : rien ne bouge sans accord.
        let mut compte: Vec<(String, usize)> = vec![];
        for (_, c) in &prevu {
            match compte.iter_mut().find(|(n, _)| n == c) {
                Some((_, k)) => *k += 1,
                None => compte.push((c.clone(), 1)),
            }
        }
        compte.sort_by(|a, b| b.1.cmp(&a.1));
        egui::Window::new("Ranger l'existant ?").collapsible(false).resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label(format!("{} fichier(s) seront déplacés :", prevu.len()));
                for (c, n) in &compte {
                    ui.label(format!("   {n} → {c}"));
                }
                ui.label("Rien n'est supprimé ni écrasé.");
                ui.horizontal(|ui| {
                    if ui.button("Continuer").clicked() {
                        let map = regles.ext_vers_dossier();
                        for (f, _) in &prevu {
                            if let Some(cible) = ranger(&self.dossier, f, regles, &map) {
                                let nom = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
                                self.ranges.insert(0, (heure(), nom, cible));
                            }
                        }
                        self.confirmer = None;
                    }
                    if ui.button("Annuler").clicked() {
                        self.confirmer = None;
                    }
                });
            });
    }
}

impl Drop for Tri {
    fn drop(&mut self) {
        self.arreter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dossier_test(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tri_test_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn regles() -> Regles {
        serde_json::from_str(DEFAUT).unwrap()
    }

    #[test]
    fn chemin_libre_ne_ecrase_pas() {
        let d = dossier_test("libre");
        fs::write(d.join("photo.jpg"), "x").unwrap();
        fs::write(d.join("x.tar.gz"), "x").unwrap();
        assert_eq!(chemin_libre(&d.join("photo.jpg")), d.join("photo (1).jpg"));
        assert_eq!(chemin_libre(&d.join("x.tar.gz")), d.join("x (1).tar.gz"));
        assert_eq!(chemin_libre(&d.join("neuf.txt")), d.join("neuf.txt"));
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn categories_et_exclusions() {
        let d = dossier_test("cat");
        let r = regles();
        let map = r.ext_vers_dossier();
        for f in ["a.JPG", "b.pdf", "c.inconnu", "~$d.docx", "film.mp4", "film.mp4.part", "Projet"] {
            fs::write(d.join(f), "x").unwrap();
        }
        assert_eq!(categorie(&d.join("a.JPG"), &r, &map).as_deref(), Some("Images"));
        assert_eq!(categorie(&d.join("b.pdf"), &r, &map).as_deref(), Some("Documents"));
        for f in ["c.inconnu", "~$d.docx", "film.mp4", "film.mp4.part", "Projet"] {
            assert_eq!(categorie(&d.join(f), &r, &map), None, "{f}");
        }
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn ranger_deplace_sans_ecraser() {
        let d = dossier_test("ranger");
        let r = regles();
        let map = r.ext_vers_dossier();
        fs::create_dir(d.join("Images")).unwrap();
        fs::write(d.join("Images").join("p.png"), "ancien").unwrap();
        fs::write(d.join("p.png"), "nouveau").unwrap();
        let cible = ranger(&d, &d.join("p.png"), &r, &map).unwrap();
        assert_eq!(cible, d.join("Images").join("p (1).png"));
        assert_eq!(fs::read_to_string(d.join("Images").join("p.png")).unwrap(), "ancien");
        assert_eq!(fs::read_to_string(cible).unwrap(), "nouveau");
        assert!(apercu(&d, &r).is_empty());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn variables_d_environnement() {
        std::env::set_var("TRI_TEST_VAR", "C:\\X");
        assert_eq!(developper("%TRI_TEST_VAR%/Downloads"), PathBuf::from("C:\\X/Downloads"));
        assert_eq!(developper("sans"), PathBuf::from("sans"));
    }
}
