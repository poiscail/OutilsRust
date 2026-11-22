//! Sauvegarde automatique de dossiers vers une clé USB ou un disque : seulement ce qui a changé,
//! et les anciennes versions (fichiers modifiés ou supprimés) sont gardées quelques semaines.
//! Le disque est reconnu par son numéro de série : la lettre peut changer.
use crate::fichiers3::{a_copier, lecteurs};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const VERSIONS: &str = ".versions";

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Tache {
    pub source: String,
    pub serie: u32,
    pub disque: String,
    /// toutes les … heures quand le disque est là (0 = seulement en le branchant)
    pub toutes_h: u32,
    pub garder_jours: u32,
    pub derniere: u64,
    pub bilan: String,
}

fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Fichiers d'un dossier : chemin relatif → (taille, date).
fn lister(racine: &Path) -> HashMap<PathBuf, (u64, SystemTime)> {
    let mut v = HashMap::new();
    let mut pile = vec![racine.to_path_buf()];
    while let Some(d) = pile.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if p.file_name().is_some_and(|n| n == VERSIONS) {
                    continue;
                }
                pile.push(p);
            } else if let (Ok(r), Ok(t)) = (p.strip_prefix(racine), m.modified()) {
                v.insert(r.to_path_buf(), (m.len(), t));
            }
        }
    }
    v
}

/// Ce qu'il faut faire : (à copier, à retirer car supprimés de la source).
pub fn plan(source: &HashMap<PathBuf, (u64, SystemTime)>, dest: &HashMap<PathBuf, (u64, SystemTime)>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut copier: Vec<PathBuf> = source.iter().filter(|(r, m)| a_copier(**m, dest.get(*r).copied())).map(|(r, _)| r.clone()).collect();
    let mut retirer: Vec<PathBuf> = dest.keys().filter(|r| !source.contains_key(*r)).cloned().collect();
    copier.sort();
    retirer.sort();
    (copier, retirer)
}

fn archiver(dest: &Path, versions: &Path, r: &Path) -> std::io::Result<()> {
    let de = dest.join(r);
    let vers = versions.join(r);
    if let Some(p) = vers.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(&de, &vers).or_else(|_| std::fs::copy(&de, &vers).and_then(|_| std::fs::remove_file(&de)))
}

/// Une sauvegarde complète d'une tâche vers la lettre de disque donnée.
pub fn sauvegarder(t: &Tache, lettre: &str, etat: &Mutex<String>) -> Result<String, String> {
    let src = Path::new(&t.source);
    if !src.is_dir() {
        return Err(format!("{} est introuvable", t.source));
    }
    let nom = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Dossier".into());
    let dest = Path::new(lettre).join("Sauvegardes Outils").join(&nom);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    *etat.lock().unwrap() = format!("Comparaison de {nom}…");
    let (s, d) = (lister(src), lister(&dest));
    let (copier, retirer) = plan(&s, &d);
    let versions = dest.join(VERSIONS).join(crate::minecraft::horodatage());
    let (mut ok, mut err) = (0, 0);
    for (k, r) in copier.iter().enumerate() {
        if k % 20 == 0 {
            *etat.lock().unwrap() = format!("{nom} : {}/{}", k + 1, copier.len());
        }
        if d.contains_key(r) && archiver(&dest, &versions, r).is_err() {
            err += 1;
            continue;
        }
        let cible = dest.join(r);
        if let Some(p) = cible.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        match std::fs::copy(src.join(r), &cible) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }
    for r in &retirer {
        let _ = archiver(&dest, &versions, r);
    }
    // anciennes versions trop vieilles
    let limite = Duration::from_secs(t.garder_jours.max(1) as u64 * 86400);
    for e in std::fs::read_dir(dest.join(VERSIONS)).into_iter().flatten().flatten() {
        let vieux = e.metadata().and_then(|m| m.modified()).map(|m| m.elapsed().unwrap_or_default() > limite).unwrap_or(false);
        if vieux {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
    Ok(format!("{ok} fichier(s) copié(s), {} retiré(s) (gardés dans les versions){}", retirer.len(), if err > 0 { format!(", {err} erreur(s)") } else { String::new() }))
}

pub struct SauvegardeAuto {
    taches: Arc<Mutex<Vec<Tache>>>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    lance: bool,
    nouvelle: Tache,
}

/// Sauvegarde les tâches de `forcer` (disque qu'on vient de brancher, bouton), ou sinon celles dont c'est l'heure.
fn tourner(taches: Arc<Mutex<Vec<Tache>>>, etat: Arc<Mutex<String>>, occupe: Arc<AtomicBool>, forcer: Vec<usize>) {
    if occupe.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let disques = lecteurs();
        let liste = taches.lock().unwrap().clone();
        let mut faits = vec![];
        for (k, t) in liste.iter().enumerate() {
            let Some((lettre, ..)) = disques.iter().find(|d| d.1 == t.serie) else { continue };
            let du = forcer.contains(&k) || (forcer.is_empty() && t.toutes_h > 0 && maintenant().saturating_sub(t.derniere) >= t.toutes_h as u64 * 3600);
            if !du {
                continue;
            }
            let b = sauvegarder(t, lettre, &etat).unwrap_or_else(|e| format!("✖ {e}"));
            faits.push((k, b));
        }
        let mut l = taches.lock().unwrap();
        for (k, b) in &faits {
            if let Some(t) = l.get_mut(*k) {
                t.derniere = maintenant();
                t.bilan = format!("{} · {b}", crate::minecraft::horodatage().replace('_', " "));
            }
        }
        if !faits.is_empty() {
            enregistrer("sauvegarde_auto", &*l);
            crate::systeme::notifier("Sauvegarde automatique", &faits.iter().map(|f| f.1.clone()).collect::<Vec<_>>().join(" · "));
        }
        *etat.lock().unwrap() = String::new();
        occupe.store(false, Relaxed);
    });
}

impl SauvegardeAuto {
    pub fn new() -> Self {
        SauvegardeAuto { taches: Arc::new(Mutex::new(charger("sauvegarde_auto"))), etat: Arc::default(), occupe: Arc::default(), lance: false, nouvelle: Tache { toutes_h: 24, garder_jours: 30, ..Default::default() } }
    }
}

impl Outil for SauvegardeAuto {
    fn nom(&self) -> &'static str {
        "🗄 Sauvegarde automatique"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if self.lance || self.taches.lock().unwrap().is_empty() {
            return;
        }
        self.lance = true;
        // surveillance : disque branché (sauvegarde tout de suite) et sauvegardes régulières
        let (t, e, o) = (self.taches.clone(), self.etat.clone(), self.occupe.clone());
        std::thread::spawn(move || {
            let mut presents: Vec<u32> = lecteurs().iter().map(|d| d.1).collect();
            loop {
                std::thread::sleep(Duration::from_secs(30));
                if crate::outil::DISCRET.load(Relaxed) {
                    continue;
                }
                let maintenant_la: Vec<u32> = lecteurs().iter().map(|d| d.1).collect();
                let branches: Vec<u32> = maintenant_la.iter().filter(|s| !presents.contains(s)).copied().collect();
                presents = maintenant_la;
                let liste = t.lock().unwrap().clone();
                let a_faire: Vec<usize> = liste.iter().enumerate().filter(|(_, x)| branches.contains(&x.serie)).map(|(k, _)| k).collect();
                tourner(t.clone(), e.clone(), o.clone(), a_faire);
            }
        });
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sauvegarde automatique");
        ui.weak("Choisis des dossiers et une clé USB ou un disque : Outils y recopie ce qui a changé, tout seul, dès que le disque est branché et régulièrement. Les fichiers modifiés ou supprimés gardent leur ancienne version quelques semaines (dossier « .versions »).");
        let disques = lecteurs();
        let occupe = self.occupe.load(Relaxed);
        if occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
            });
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let mut supprimer = None;
        let mut lancer = None;
        {
            let mut l = self.taches.lock().unwrap();
            let avant = l.clone();
            for (k, t) in l.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let la = disques.iter().find(|d| d.1 == t.serie);
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("📁 {}", t.source));
                        ui.label("→");
                        ui.label(RichText::new(format!("💾 {}", if t.disque.is_empty() { "disque" } else { &t.disque })).color(if la.is_some() { Color32::LIGHT_GREEN } else { Color32::GRAY }));
                        ui.weak(if la.is_some() { "(branché)" } else { "(pas branché)" });
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Toutes les");
                        ui.add(egui::DragValue::new(&mut t.toutes_h).range(0..=168).suffix(" h"));
                        ui.weak("(0 = seulement en branchant)");
                        ui.label("· garder les versions");
                        ui.add(egui::DragValue::new(&mut t.garder_jours).range(1..=365).suffix(" jours"));
                    });
                    if !t.bilan.is_empty() {
                        ui.weak(format!("Dernière : {}", t.bilan));
                    }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(la.is_some() && !occupe, egui::Button::new("▶ Sauvegarder maintenant")).clicked() {
                            lancer = Some(k);
                        }
                        if let Some((lettre, ..)) = la {
                            if ui.button("📂 Ouvrir la sauvegarde").clicked() {
                                let nom = Path::new(&t.source).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                                let _ = std::process::Command::new("explorer").arg(Path::new(lettre).join("Sauvegardes Outils").join(nom)).spawn();
                            }
                        }
                        if ui.small_button("🗑").on_hover_text("Retirer cette sauvegarde (rien n'est effacé sur le disque)").clicked() {
                            supprimer = Some(k);
                        }
                    });
                });
            }
            if let Some(k) = supprimer {
                l.remove(k);
            }
            if *l != avant {
                enregistrer("sauvegarde_auto", &*l);
            }
        }
        if let Some(k) = lancer {
            tourner(self.taches.clone(), self.etat.clone(), self.occupe.clone(), vec![k]);
        }
        ui.separator();
        ui.strong("➕ Nouvelle sauvegarde");
        ui.horizontal_wrapped(|ui| {
            if ui.button(if self.nouvelle.source.is_empty() { "Choisir le dossier…".to_string() } else { format!("📁 {}", self.nouvelle.source) }).clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.nouvelle.source = d.display().to_string();
                }
            }
            ui.label("vers");
            let choisi = disques.iter().find(|d| d.1 == self.nouvelle.serie).map(|d| format!("{} {}", d.0, d.2)).unwrap_or_else(|| "choisir le disque".into());
            egui::ComboBox::from_id_salt("disque_sauv").selected_text(choisi).show_ui(ui, |ui| {
                for (l, s, n) in &disques {
                    if ui.selectable_label(self.nouvelle.serie == *s, format!("{l} {n}")).clicked() {
                        self.nouvelle.serie = *s;
                        self.nouvelle.disque = if n.is_empty() { l.clone() } else { n.clone() };
                    }
                }
            });
            let pret = !self.nouvelle.source.is_empty() && self.nouvelle.serie != 0;
            if ui.add_enabled(pret, egui::Button::new("Ajouter")).clicked() {
                let t = std::mem::replace(&mut self.nouvelle, Tache { toutes_h: 24, garder_jours: 30, ..Default::default() });
                let mut l = self.taches.lock().unwrap();
                l.push(t);
                enregistrer("sauvegarde_auto", &*l);
                drop(l); // la surveillance démarre au prochain tic si c'était la première
            }
        });
        if disques.is_empty() {
            ui.weak("Branche une clé USB ou un disque externe (les disques autres que C: apparaissent ici).");
        }
    }
    fn resume(&self) -> Option<String> {
        let l = self.taches.lock().ok()?;
        (!l.is_empty()).then(|| format!("{} dossier(s) sauvegardé(s) automatiquement", l.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_de_copie() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let t1 = t0 + Duration::from_secs(100);
        let s: HashMap<PathBuf, (u64, SystemTime)> = [("a.txt", (5, t1)), ("b.txt", (3, t0)), ("c/d.txt", (1, t0))].iter().map(|(p, m)| (PathBuf::from(p), *m)).collect();
        let d: HashMap<PathBuf, (u64, SystemTime)> = [("a.txt", (5, t0)), ("b.txt", (3, t0)), ("vieux.txt", (9, t0))].iter().map(|(p, m)| (PathBuf::from(p), *m)).collect();
        let (c, r) = plan(&s, &d);
        assert_eq!(c, vec![PathBuf::from("a.txt"), PathBuf::from("c/d.txt")]);
        assert_eq!(r, vec![PathBuf::from("vieux.txt")]);
    }

    #[test]
    fn sauvegarde_reelle() {
        let base = std::env::temp_dir().join(format!("outils_sauv_{}", std::process::id()));
        let src = base.join("Docs");
        std::fs::create_dir_all(src.join("sous")).unwrap();
        std::fs::write(src.join("a.txt"), "1").unwrap();
        std::fs::write(src.join("sous/b.txt"), "2").unwrap();
        let t = Tache { source: src.display().to_string(), garder_jours: 30, ..Default::default() };
        let e = Mutex::new(String::new());
        let lettre = base.join("cle");
        sauvegarder(&t, &lettre.display().to_string(), &e).unwrap();
        let dest = lettre.join("Sauvegardes Outils").join("Docs");
        assert_eq!(std::fs::read_to_string(dest.join("sous/b.txt")).unwrap(), "2");
        std::fs::write(src.join("a.txt"), "nouveau").unwrap();
        std::fs::remove_file(src.join("sous/b.txt")).unwrap();
        sauvegarder(&t, &lettre.display().to_string(), &e).unwrap();
        assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "nouveau");
        assert!(!dest.join("sous/b.txt").exists());
        let versions: Vec<_> = std::fs::read_dir(dest.join(VERSIONS)).unwrap().flatten().collect();
        assert_eq!(versions.len(), 1);
        assert_eq!(std::fs::read_to_string(versions[0].path().join("a.txt")).unwrap(), "1");
        let _ = std::fs::remove_dir_all(&base);
    }
}
