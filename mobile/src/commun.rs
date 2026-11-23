//! Petits outils communs aux écrans du téléphone.
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const ROSE: Color32 = Color32::from_rgb(255, 95, 162);
pub const VERT: Color32 = Color32::from_rgb(110, 220, 130);
pub const ORANGE: Color32 = Color32::from_rgb(250, 170, 60);
pub const ROUGE: Color32 = Color32::from_rgb(255, 90, 90);

/// (en cours, message) d'une tâche lancée en arrière-plan.
pub type Etat = Arc<Mutex<(bool, String)>>;

pub fn en_fond(e: &Etat, attente: &str, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, attente.to_string());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

/// Affiche l'état ; vrai si une tâche tourne.
pub fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    } else if !m.is_empty() {
        ui.label(RichText::new(m).strong());
    }
    o
}

pub fn taille(o: u64) -> String {
    let u = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = o as f64;
    let mut k = 0;
    while v >= 1024.0 && k < 4 {
        v /= 1024.0;
        k += 1;
    }
    if k == 0 { format!("{o} o") } else { format!("{v:.1} {}", u[k]) }
}

/// « 22/11/2026 » à partir de secondes Unix.
pub fn date(t: u64) -> String {
    if t == 0 {
        return String::new();
    }
    let (a, m, j) = crate::quotidien2::date_de_jours(t as i64 / 86_400);
    format!("{j:02}/{m:02}/{a}")
}

pub fn maintenant() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn date_fichier(p: &Path) -> u64 {
    p.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

/// Grand bouton pour le doigt.
pub fn gros_bouton(ui: &mut egui::Ui, texte: &str) -> bool {
    ui.add(egui::Button::new(RichText::new(texte).size(19.0)).min_size(egui::vec2(ui.available_width().min(380.0), 50.0))).clicked()
}

/// Onglets en haut d'un outil.
pub fn onglets(ui: &mut egui::Ui, choisi: &mut usize, noms: &[&str]) {
    ui.horizontal_wrapped(|ui| {
        for (k, n) in noms.iter().enumerate() {
            ui.selectable_value(choisi, k, RichText::new(*n).size(16.0));
        }
    });
    ui.separator();
}

/// Dossier de l'appli dans le stockage partagé (fichiers reçus du PC…), visible dans « Fichiers ».
pub fn dossier_outils() -> PathBuf {
    let d = PathBuf::from(crate::android::STOCKAGE).join("Download").join("Outils");
    if std::fs::create_dir_all(&d).is_ok() {
        return d;
    }
    // sans l'accès aux fichiers : dossier privé de l'appli
    let p = crate::outil::chemin("x").parent().map(|p| p.join("recus")).unwrap_or_else(|| PathBuf::from("recus"));
    let _ = std::fs::create_dir_all(&p);
    p
}

/// Corbeille de l'appli : rien n'est supprimé directement, tout peut être restauré pendant 30 jours.
pub fn corbeille() -> PathBuf {
    PathBuf::from(crate::android::STOCKAGE).join(".Outils-corbeille")
}

pub fn mettre_a_la_corbeille(p: &Path) -> Result<(), String> {
    let c = corbeille();
    std::fs::create_dir_all(&c).map_err(|e| e.to_string())?;
    let nom = format!("{}__{}", maintenant(), p.file_name().unwrap_or_default().to_string_lossy());
    let dest = c.join(nom);
    // l'origine est notée pour pouvoir restaurer
    let _ = std::fs::write(dest.with_extension("origine"), p.display().to_string());
    std::fs::rename(p, &dest).or_else(|_| std::fs::copy(p, &dest).and_then(|_| std::fs::remove_file(p))).map_err(|e| e.to_string())?;
    crate::android::signaler_media(&[p.display().to_string()]);
    Ok(())
}

/// Fichier libre (« nom (2).ext ») dans un dossier.
pub fn libre(dossier: &Path, nom: &str) -> PathBuf {
    let p = dossier.join(nom);
    if !p.exists() {
        return p;
    }
    let (base, ext) = match nom.rsplit_once('.') {
        Some((b, e)) if !b.is_empty() => (b.to_string(), format!(".{e}")),
        _ => (nom.to_string(), String::new()),
    };
    (2..).map(|i| dossier.join(format!("{base} ({i}){ext}"))).find(|p| !p.exists()).unwrap()
}

/// Bandeau demandant l'accès aux fichiers. Renvoie vrai si l'accès est donné.
pub fn acces_fichiers_ui(ui: &mut egui::Ui) -> bool {
    if crate::android::acces_fichiers() {
        return true;
    }
    let photos = crate::android::acces_photos();
    if photos {
        ui.horizontal_wrapped(|ui| {
            ui.weak("Accès aux photos seulement. Pour supprimer, ranger ou voir tous les fichiers :");
            if ui.small_button("📂 Accès complet").clicked() {
                crate::android::demander_acces_fichiers();
            }
        });
        return true;
    }
    egui::Frame::group(ui.style()).show(ui, |ui| {
        {
            ui.label(RichText::new("Outils a besoin d'accéder à tes fichiers").strong());
            ui.weak("1. Touche le premier bouton : Android demande « Autoriser l'accès aux photos et vidéos » → Autoriser.");
            if gros_bouton(ui, "🖼 Autoriser l'accès aux photos") {
                crate::android::demander_acces_photos();
            }
            ui.weak("2. Pour supprimer, ranger ou voir tous les fichiers :");
        }
        if gros_bouton(ui, "📂 Accès complet aux fichiers") {
            crate::android::demander_acces_fichiers();
        }
        ui.weak("Dans l'écran qui s'ouvre, trouve Outils et active « Autoriser l'accès pour gérer tous les fichiers », puis reviens avec ←. Si l'écran des réglages de l'appli s'ouvre : Autorisations › Fichiers et contenus multimédias › Autoriser la gestion de tous les fichiers.");
    });
    ui.ctx().request_repaint_after(Duration::from_secs(1));
    // les outils de photos marchent déjà en lecture avec l'accès aux photos
    photos
}

/// Catégorie d'un fichier d'après son extension.
pub fn famille(nom: &str) -> &'static str {
    let e = nom.rsplit('.').next().unwrap_or("").to_lowercase();
    match e.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "bmp" | "dng" => "Images",
        "mp4" | "mkv" | "mov" | "3gp" | "webm" | "avi" => "Vidéos",
        "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "flac" | "amr" => "Musique",
        "pdf" | "doc" | "docx" | "odt" | "txt" | "xls" | "xlsx" | "ods" | "ppt" | "pptx" | "csv" | "rtf" | "epub" => "Documents",
        "apk" | "apks" | "xapk" | "apkm" => "Applis (APK)",
        "zip" | "rar" | "7z" | "tar" | "gz" => "Archives",
        _ => "Autres",
    }
}

pub fn icone(nom: &str, dossier: bool) -> &'static str {
    if dossier {
        return "📁";
    }
    match famille(nom) {
        "Images" => "🖼",
        "Vidéos" => "🎬",
        "Musique" => "🎵",
        "Documents" => "📄",
        "Applis (APK)" => "📦",
        "Archives" => "🗜",
        _ => "📃",
    }
}
