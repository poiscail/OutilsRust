//! « Partager vers Outils » : ce que tu partages depuis la galerie, WhatsApp, Chrome… arrive ici
//! (le petit écran Java `Partage` le copie dans le dossier privé de l'appli). On choisit ensuite :
//! envoyer au PC, mettre au coffre, compresser, ou garder dans Téléchargements.
use crate::commun::*;
use crate::outil::{charger, Cat, Outil};
use eframe::egui::{self, RichText};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const TEXTE: &str = "partage_texte.txt";

pub fn dossier() -> PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.join("partage_entrant")).unwrap_or_else(|| PathBuf::from("partage_entrant"))
}

/// Fichiers reçus (le texte partagé à part).
pub fn recus() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dossier()).map(|l| l.flatten().map(|e| e.path()).filter(|p| p.is_file() && !p.ends_with(TEXTE)).collect()).unwrap_or_default();
    v.sort();
    v
}

pub fn texte_recu() -> Option<String> {
    std::fs::read_to_string(dossier().join(TEXTE)).ok().filter(|t| !t.trim().is_empty())
}

/// Nombre d'éléments en attente (pour ouvrir l'outil tout seul quand quelque chose arrive).
pub fn en_attente() -> usize {
    recus().len() + texte_recu().is_some() as usize
}

/// Déplace un fichier (copie puis efface si le renommage traverse deux stockages).
fn deplacer(de: &Path, vers_dossier: &Path) -> Result<PathBuf, String> {
    let nom = de.file_name().unwrap_or_default().to_string_lossy().to_string();
    let dest = libre(vers_dossier, &nom);
    if std::fs::rename(de, &dest).is_err() {
        std::fs::copy(de, &dest).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(de);
    }
    Ok(dest)
}

pub struct PartageRecu {
    e: Etat,
}

impl PartageRecu {
    pub fn new() -> Self {
        PartageRecu { e: Arc::default() }
    }
}

fn ouvrir_outil(nom: &str) {
    *crate::outil::OUVRIR.lock().unwrap() = Some(nom.to_string());
}

impl Outil for PartageRecu {
    fn nom(&self) -> &'static str {
        "📥 Reçu par partage"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let occupe = etat_ui(ui, &self.e);
        let fichiers = recus();
        let texte = texte_recu();
        if fichiers.is_empty() && texte.is_none() {
            ui.label(RichText::new("Rien en attente").size(20.0).strong());
            ui.weak("Dans n'importe quelle appli (Galerie, WhatsApp, Chrome…), touche « Partager » puis « Outils » : les photos, vidéos, fichiers ou liens arrivent ici, et tu choisis quoi en faire.");
            return;
        }
        let pc: Option<crate::monpc::Pc> = charger("pc");
        if let Some(t) = &texte {
            ui.label(RichText::new("📝 Texte ou lien reçu").strong());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(t.chars().take(600).collect::<String>());
            });
            ui.horizontal_wrapped(|ui| {
                if let Some(p) = pc.clone() {
                    if ui.button("🖥 Presse-papiers du PC").clicked() && !occupe {
                        let t = t.clone();
                        en_fond(&self.e, "Envoi…", move || {
                            p.post("/presse", t.as_bytes())?;
                            let _ = std::fs::remove_file(dossier().join(TEXTE));
                            Ok("✔ Texte collé dans le presse-papiers du PC".into())
                        });
                    }
                }
                if ui.button("📋 Copier").clicked() {
                    ui.ctx().copy_text(t.clone());
                }
                if t.trim_start().starts_with("http") && ui.button("🌐 Ouvrir").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(t.trim()));
                }
                if ui.button("🗑 Ignorer").clicked() {
                    let _ = std::fs::remove_file(dossier().join(TEXTE));
                }
            });
            ui.separator();
        }
        if fichiers.is_empty() {
            return;
        }
        let total: u64 = fichiers.iter().map(|f| f.metadata().map(|m| m.len()).unwrap_or(0)).sum();
        ui.label(RichText::new(format!("📦 {} fichier(s) reçu(s) · {}", fichiers.len(), taille(total))).strong());
        for f in fichiers.iter().take(30) {
            let n = f.file_name().unwrap_or_default().to_string_lossy().to_string();
            ui.label(format!("{} {n} · {}", icone(&n, false), taille(f.metadata().map(|m| m.len()).unwrap_or(0))));
        }
        if fichiers.len() > 30 {
            ui.weak(format!("… et {} autre(s)", fichiers.len() - 30));
        }
        ui.add_space(6.0);
        let videos: Vec<PathBuf> = fichiers.iter().filter(|p| famille(&p.to_string_lossy()) == "Vidéos").cloned().collect();
        let images = fichiers.iter().filter(|p| matches!(famille(&p.to_string_lossy()), "Images" | "Vidéos")).count();
        if let Some(p) = pc {
            if gros_bouton(ui, "🖥 Envoyer au PC") && !occupe {
                let l = fichiers.clone();
                en_fond(&self.e, "Envoi au PC…", move || {
                    let mut n = 0;
                    for f in &l {
                        let nom = f.file_name().unwrap_or_default().to_string_lossy().to_string();
                        let fic = std::fs::File::open(f).map_err(|e| e.to_string())?;
                        let lon = fic.metadata().map(|m| m.len()).unwrap_or(0);
                        ureq::post(&p.url(&format!("/envoyer?nom={}", crate::monpc::encoder(&nom))))
                            .timeout(Duration::from_secs(3600))
                            .set("Content-Length", &lon.to_string())
                            .send(fic)
                            .map_err(|e| e.to_string())?;
                        let _ = std::fs::remove_file(f);
                        n += 1;
                    }
                    crate::android::vibrer(60);
                    Ok(format!("✔ {n} fichier(s) envoyé(s) : sur le PC, dans Téléchargements › Depuis le téléphone"))
                });
            }
        } else {
            ui.weak("Associe ton PC (PC et réseau › Mon PC) pour pouvoir lui envoyer ce que tu partages.");
        }
        if images > 0 && gros_bouton(ui, "🔒 Mettre au coffre-fort photos") && !occupe {
            *crate::photos_tel::A_RANGER.lock().unwrap() = fichiers.clone();
            ouvrir_outil("🔒 Coffre-fort photos");
        }
        if !videos.is_empty() && gros_bouton(ui, &format!("🎞 Compresser {} vidéo(s)", videos.len())) && !occupe {
            // la vidéo compressée doit être visible : on range d'abord l'originale dans Téléchargements › Outils
            let d = dossier_outils();
            let v: Vec<PathBuf> = videos.iter().filter_map(|f| deplacer(f, &d).ok()).collect();
            crate::android::signaler_media(&v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>());
            *crate::photos_tel::A_COMPRESSER.lock().unwrap() = v;
            ouvrir_outil("🎞 Compresser une vidéo");
        }
        if gros_bouton(ui, "💾 Garder dans Téléchargements › Outils") && !occupe {
            let d = dossier_outils();
            let v: Vec<String> = fichiers.iter().filter_map(|f| deplacer(f, &d).ok()).map(|p| p.display().to_string()).collect();
            crate::android::signaler_media(&v);
            *self.e.lock().unwrap() = (false, format!("✔ {} fichier(s) rangé(s) dans Téléchargements › Outils", v.len()));
        }
        if ui.button("🗑 Tout ignorer").clicked() && !occupe {
            for f in &fichiers {
                let _ = std::fs::remove_file(f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deplacement() {
        let d = std::env::temp_dir().join(format!("outils_partage_{}", std::process::id()));
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("photo.jpg"), b"x").unwrap();
        std::fs::write(b.join("photo.jpg"), b"y").unwrap();
        let p = deplacer(&a.join("photo.jpg"), &b).unwrap();
        assert!(!a.join("photo.jpg").exists());
        assert_ne!(p, b.join("photo.jpg")); // pas d'écrasement
        assert_eq!(std::fs::read(&p).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&d);
    }
}
