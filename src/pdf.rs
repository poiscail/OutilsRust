//! Outil PDF (81) : fusionner, extraire ou supprimer des pages, pivoter, compresser.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32};
use lopdf::{Document, Object, ObjectId};
use std::path::{Path, PathBuf};

/// « 1-3, 5, 8- » → pages (1-indexées) parmi `n`.
pub fn lire_pages(t: &str, n: u32) -> Result<Vec<u32>, String> {
    let mut v = vec![];
    for morceau in t.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let (a, b) = match morceau.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (morceau, morceau),
        };
        let a: u32 = if a.is_empty() { 1 } else { a.parse().map_err(|_| format!("« {morceau} » n'est pas une page"))? };
        let b: u32 = if b.is_empty() { n } else { b.parse().map_err(|_| format!("« {morceau} » n'est pas une page"))? };
        if a == 0 || b > n || a > b {
            return Err(format!("« {morceau} » : pages de 1 à {n}"));
        }
        v.extend(a..=b);
    }
    v.dedup();
    if v.is_empty() {
        return Err("aucune page".into());
    }
    Ok(v)
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Fusionne les PDF dans l'ordre donné (d'après l'exemple de lopdf, en gardant l'ordre des pages).
pub fn fusionner(fichiers: &[PathBuf], sortie: &Path) -> Result<(), String> {
    let mut max_id = 1;
    let mut pages: Vec<(ObjectId, Object)> = vec![];
    let mut objets = std::collections::BTreeMap::new();
    for f in fichiers {
        let mut d = Document::load(f).map_err(|e| format!("{} : {e}", f.display()))?;
        d.renumber_objects_with(max_id);
        max_id = d.max_id + 1;
        for (_, id) in d.get_pages() {
            pages.push((id, d.get_object(id).map_err(err)?.clone()));
        }
        objets.extend(d.objects);
    }
    let mut doc = Document::with_version("1.5");
    let (mut catalogue, mut racine): (Option<(ObjectId, Object)>, Option<(ObjectId, Object)>) = (None, None);
    for (id, o) in objets {
        match o.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalogue.get_or_insert((id, o));
            }
            b"Pages" => {
                racine.get_or_insert((id, o));
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                doc.objects.insert(id, o);
            }
        }
    }
    let (racine_id, racine_o) = racine.ok_or("arbre des pages introuvable")?;
    let (cat_id, cat_o) = catalogue.ok_or("catalogue introuvable")?;
    for (id, o) in &pages {
        let mut d = o.as_dict().map_err(err)?.clone();
        d.set("Parent", racine_id);
        doc.objects.insert(*id, Object::Dictionary(d));
    }
    let mut r = racine_o.as_dict().map_err(err)?.clone();
    r.set("Count", pages.len() as u32);
    r.set("Kids", pages.iter().map(|(id, _)| Object::Reference(*id)).collect::<Vec<_>>());
    doc.objects.insert(racine_id, Object::Dictionary(r));
    let mut c = cat_o.as_dict().map_err(err)?.clone();
    c.set("Pages", racine_id);
    c.remove(b"Outlines");
    doc.objects.insert(cat_id, Object::Dictionary(c));
    doc.trailer.set("Root", cat_id);
    doc.max_id = doc.objects.len() as u32;
    doc.renumber_objects();
    doc.compress();
    doc.save(sortie).map_err(err)?;
    Ok(())
}

/// Garde seulement `pages` (ou les supprime si `supprimer`).
pub fn garder(entree: &Path, pages: &[u32], supprimer: bool, sortie: &Path) -> Result<(), String> {
    let mut d = Document::load(entree).map_err(err)?;
    let n = d.get_pages().len() as u32;
    let a_retirer: Vec<u32> = if supprimer { pages.to_vec() } else { (1..=n).filter(|p| !pages.contains(p)).collect() };
    if a_retirer.len() as u32 >= n {
        return Err("il ne resterait aucune page".into());
    }
    d.delete_pages(&a_retirer);
    d.prune_objects();
    d.compress();
    d.save(sortie).map_err(err)?;
    Ok(())
}

pub fn pivoter(entree: &Path, pages: &[u32], angle: i64, sortie: &Path) -> Result<(), String> {
    let mut d = Document::load(entree).map_err(err)?;
    for (num, id) in d.get_pages() {
        if pages.contains(&num) {
            let p = d.get_object_mut(id).and_then(|o| o.as_dict_mut()).map_err(err)?;
            let r = p.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
            p.set("Rotate", (r + angle).rem_euclid(360));
        }
    }
    d.save(sortie).map_err(err)?;
    Ok(())
}

/// Compresse : avec Ghostscript s'il est installé (images réduites), sinon recompression des flux seulement.
pub fn compresser(entree: &Path, sortie: &Path, qualite: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    for gs in ["gswin64c", "gswin32c", "gs"] {
        let r = std::process::Command::new(gs).args(["-sDEVICE=pdfwrite", "-dCompatibilityLevel=1.5", &format!("-dPDFSETTINGS=/{qualite}"), "-dNOPAUSE", "-dQUIET", "-dBATCH"])
            .arg(format!("-sOutputFile={}", sortie.display())).arg(entree).creation_flags(0x0800_0000).status();
        if let Ok(s) = r {
            return if s.success() { Ok("compressé avec Ghostscript".into()) } else { Err("Ghostscript a échoué".into()) };
        }
    }
    let mut d = Document::load(entree).map_err(err)?;
    d.prune_objects();
    d.compress();
    d.save(sortie).map_err(err)?;
    Ok("flux recompressés (installe Ghostscript pour réduire aussi les images : winget install ArtifexSoftware.GhostScript)".into())
}

fn suffixe(p: &Path, s: &str) -> PathBuf {
    p.with_file_name(format!("{} {s}.pdf", p.file_stem().unwrap_or_default().to_string_lossy()))
}

fn nb_pages(p: &Path) -> Option<u32> {
    Document::load(p).ok().map(|d| d.get_pages().len() as u32)
}

pub struct OutilPdf {
    fichiers: Vec<(PathBuf, Option<u32>)>,
    pages: String,
    angle: i64,
    qualite: usize,
    message: String,
}

impl OutilPdf {
    pub fn new() -> Self {
        OutilPdf { fichiers: vec![], pages: "1".into(), angle: 90, qualite: 1, message: String::new() }
    }
}

impl Outil for OutilPdf {
    fn nom(&self) -> &'static str {
        "📄 PDF"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Outil PDF");
        let deposes: Vec<PathBuf> = ui.ctx().input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in deposes {
            if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                let n = nb_pages(&p);
                self.fichiers.push((p, n));
            }
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Ajouter des PDF…").clicked() {
                for p in rfd::FileDialog::new().add_filter("PDF", &["pdf"]).pick_files().unwrap_or_default() {
                    let n = nb_pages(&p);
                    self.fichiers.push((p, n));
                }
            }
            if ui.button("Vider").clicked() {
                self.fichiers.clear();
            }
            ui.weak("ou glisse-les ici · l'ordre de la liste est celui de la fusion");
        });
        let mut monter = None;
        let mut retirer = None;
        for (i, (p, n)) in self.fichiers.iter().enumerate() {
            ui.horizontal(|ui| {
                if i > 0 && ui.small_button("⬆").clicked() {
                    monter = Some(i);
                }
                if ui.small_button("✖").clicked() {
                    retirer = Some(i);
                }
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                match n {
                    Some(n) => ui.weak(format!("{n} page(s)")),
                    None => ui.colored_label(Color32::LIGHT_RED, "illisible ou protégé"),
                };
            });
        }
        if let Some(i) = monter {
            self.fichiers.swap(i, i - 1);
        }
        if let Some(i) = retirer {
            self.fichiers.remove(i);
        }
        ui.separator();
        let premier = self.fichiers.first().cloned();
        ui.horizontal(|ui| {
            if ui.add_enabled(self.fichiers.len() >= 2, egui::Button::new("🔗 Fusionner")).clicked() {
                if let Some(s) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name("Fusion.pdf").save_file() {
                    let liste: Vec<PathBuf> = self.fichiers.iter().map(|f| f.0.clone()).collect();
                    self.message = match fusionner(&liste, &s) {
                        Ok(()) => format!("✔ {}", s.display()),
                        Err(e) => format!("✖ {e}"),
                    };
                }
            }
            ui.weak("(premier fichier de la liste pour les actions ci-dessous)");
        });
        ui.horizontal(|ui| {
            ui.label("Pages");
            ui.add(egui::TextEdit::singleline(&mut self.pages).hint_text("ex. 1-3, 5, 8-").desired_width(140.0));
            let ok = premier.as_ref().is_some_and(|f| f.1.is_some());
            let n = premier.as_ref().and_then(|f| f.1).unwrap_or(0);
            let mut faire = |ui: &mut egui::Ui, texte: &str, f: &dyn Fn(&Path, &[u32]) -> Result<PathBuf, String>| {
                if ui.add_enabled(ok, egui::Button::new(texte)).clicked() {
                    let p = premier.as_ref().unwrap().0.clone();
                    self.message = match lire_pages(&self.pages, n).and_then(|pg| f(&p, &pg)) {
                        Ok(s) => format!("✔ {}", s.display()),
                        Err(e) => format!("✖ {e}"),
                    };
                }
            };
            faire(ui, "✂ Extraire ces pages", &|p, pg| {
                let s = suffixe(p, "(extrait)");
                garder(p, pg, false, &s).map(|_| s)
            });
            faire(ui, "🗑 Supprimer ces pages", &|p, pg| {
                let s = suffixe(p, "(pages retirées)");
                garder(p, pg, true, &s).map(|_| s)
            });
        });
        ui.horizontal(|ui| {
            for a in [90, 180, 270] {
                ui.selectable_value(&mut self.angle, a, format!("{a}°"));
            }
            if ui.add_enabled(premier.as_ref().is_some_and(|f| f.1.is_some()), egui::Button::new("⟳ Pivoter ces pages")).clicked() {
                let (p, n) = premier.clone().unwrap();
                let s = suffixe(&p, "(pivoté)");
                self.message = match lire_pages(&self.pages, n.unwrap_or(0)).and_then(|pg| pivoter(&p, &pg, self.angle, &s)) {
                    Ok(()) => format!("✔ {}", s.display()),
                    Err(e) => format!("✖ {e}"),
                };
            }
        });
        ui.horizontal(|ui| {
            let niveaux = [("screen", "petit (écran)"), ("ebook", "moyen"), ("printer", "bonne qualité")];
            for (i, (_, n)) in niveaux.iter().enumerate() {
                ui.selectable_value(&mut self.qualite, i, *n);
            }
            if ui.add_enabled(premier.is_some(), egui::Button::new("🗜 Compresser")).clicked() {
                let p = premier.clone().unwrap().0;
                let s = suffixe(&p, "(compressé)");
                self.message = match compresser(&p, &s, niveaux[self.qualite].0) {
                    Ok(m) => {
                        let (a, b) = (p.metadata().map(|m| m.len()).unwrap_or(0), s.metadata().map(|m| m.len()).unwrap_or(0));
                        format!("✔ {} → {} ({m})", crate::disque::taille_lisible(a), crate::disque::taille_lisible(b))
                    }
                    Err(e) => format!("✖ {e}"),
                };
            }
        });
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plages() {
        assert_eq!(lire_pages("1-3, 5, 8-", 9).unwrap(), vec![1, 2, 3, 5, 8, 9]);
        assert_eq!(lire_pages("2", 3).unwrap(), vec![2]);
        assert!(lire_pages("4", 3).is_err());
        assert!(lire_pages("x", 3).is_err());
        assert!(lire_pages("", 3).is_err());
    }
}
