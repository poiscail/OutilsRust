//! Lanceur de jeux Linux : Steam (natif, Flatpak), Heroic (Epic, GOG, Amazon), Lutris et jeux ajoutés
//! à la main, avec jaquettes et temps de jeu.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub struct Jeu {
    pub id: String,
    pub nom: String,
    /// commande à lancer (programme puis arguments)
    pub cmd: Vec<String>,
    pub dir: String,
    pub jaquette: Option<PathBuf>,
    pub magasin: &'static str,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Valeur d'une clé dans un fichier VDF/ACF de Steam (« "name"  "Portal 2" »).
pub fn vdf(t: &str, cle: &str) -> Option<String> {
    let motif = format!("\"{cle}\"");
    t.lines().find_map(|l| {
        let l = l.trim();
        let r = l.strip_prefix(&motif)?;
        Some(r.trim().trim_matches('"').replace("\\\\", "\\"))
    })
}

fn vdf_tous(t: &str, cle: &str) -> Vec<String> {
    let motif = format!("\"{cle}\"");
    t.lines().filter_map(|l| l.trim().strip_prefix(&motif).map(|r| r.trim().trim_matches('"').to_string())).collect()
}

const IGNORER: &[&str] = &["Proton", "Steam Linux Runtime", "Steamworks Common", "SteamVR", "Steam Audio"];

pub fn racines_steam() -> Vec<PathBuf> {
    let h = home();
    let mut v: Vec<PathBuf> = [h.join(".steam/steam"), h.join(".local/share/Steam"), h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")].into_iter().filter(|p| p.join("steamapps").is_dir()).collect();
    v.dedup_by(|a, b| a.canonicalize().ok() == b.canonicalize().ok());
    v
}

fn scan_steam() -> Vec<Jeu> {
    let mut jeux = vec![];
    for racine in racines_steam() {
        let flatpak = racine.to_string_lossy().contains("com.valvesoftware.Steam");
        let mut libs = vec![racine.clone()];
        if let Ok(t) = std::fs::read_to_string(racine.join("steamapps/libraryfolders.vdf")) {
            libs.extend(vdf_tous(&t, "path").into_iter().map(PathBuf::from));
        }
        libs.dedup();
        for lib in libs {
            for e in std::fs::read_dir(lib.join("steamapps")).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                    continue;
                }
                let t = std::fs::read_to_string(e.path()).unwrap_or_default();
                let (Some(nom), Some(id), Some(d)) = (vdf(&t, "name"), vdf(&t, "appid"), vdf(&t, "installdir")) else { continue };
                if IGNORER.iter().any(|i| nom.starts_with(i)) || jeux.iter().any(|j: &Jeu| j.id == format!("steam:{id}")) {
                    continue;
                }
                let cache = racine.join("appcache/librarycache");
                let jaquette = [cache.join(format!("{id}_library_600x900.jpg")), cache.join(&id).join("library_600x900.jpg")].into_iter().find(|p| p.is_file());
                let cmd = if flatpak { vec!["flatpak".into(), "run".into(), "com.valvesoftware.Steam".into(), format!("steam://rungameid/{id}")] } else { vec!["steam".into(), format!("steam://rungameid/{id}")] };
                jeux.push(Jeu { id: format!("steam:{id}"), nom, cmd, dir: lib.join("steamapps/common").join(d).display().to_string(), jaquette, magasin: "Steam" });
            }
        }
    }
    jeux
}

fn scan_heroic() -> Vec<Jeu> {
    let h = home();
    let mut v = vec![];
    for base in [h.join(".config/heroic"), h.join(".var/app/com.heroicgameslauncher.hgl/config/heroic")] {
        if let Some(j) = std::fs::read_to_string(base.join("legendaryConfig/legendary/installed.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            for (app, x) in j.as_object().into_iter().flatten() {
                v.push(Jeu { id: format!("epic:{app}"), nom: x["title"].as_str().unwrap_or(app).to_string(), cmd: vec!["xdg-open".into(), format!("heroic://launch/legendary/{app}")], dir: x["install_path"].as_str().unwrap_or("").to_string(), jaquette: None, magasin: "Epic" });
            }
        }
        if let Some(j) = std::fs::read_to_string(base.join("gog_store/installed.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
            for x in j["installed"].as_array().into_iter().flatten() {
                let app = x["appName"].as_str().unwrap_or("").to_string();
                let dir = x["install_path"].as_str().unwrap_or("").to_string();
                let nom = Path::new(&dir).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| app.clone());
                v.push(Jeu { id: format!("gog:{app}"), nom, cmd: vec!["xdg-open".into(), format!("heroic://launch/gog/{app}")], dir, jaquette: None, magasin: "GOG" });
            }
        }
    }
    v
}

fn scan_lutris() -> Vec<Jeu> {
    let Ok(o) = std::process::Command::new("lutris").args(["--list-games", "--installed", "--json"]).output() else { return vec![] };
    let Ok(j) = serde_json::from_slice::<serde_json::Value>(&o.stdout) else { return vec![] };
    j.as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let slug = x["slug"].as_str()?;
            Some(Jeu { id: format!("lutris:{slug}"), nom: x["name"].as_str().unwrap_or(slug).to_string(), cmd: vec!["lutris".into(), format!("lutris:rungame/{slug}")], dir: x["directory"].as_str().unwrap_or("").to_string(), jaquette: None, magasin: "Lutris" })
        })
        .collect()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Donnees {
    manuels: Vec<String>,
    temps: BTreeMap<String, f64>,
    dernier: BTreeMap<String, u64>,
}

fn maintenant() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Tous les jeux installés (pour le lanceur et pour le téléphone).
pub fn jeux_installes() -> Vec<Jeu> {
    let d: Donnees = charger("lanceur_linux");
    let mut v = scan_steam();
    v.extend(scan_heroic());
    v.extend(scan_lutris());
    for m in d.manuels {
        let p = Path::new(&m);
        v.push(Jeu { id: format!("man:{m}"), nom: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), cmd: vec![m.clone()], dir: p.parent().map(|x| x.display().to_string()).unwrap_or_default(), jaquette: None, magasin: "" });
    }
    let mut vus = std::collections::HashSet::new();
    v.retain(|j| vus.insert(j.nom.to_lowercase()));
    for j in v.iter_mut().filter(|j| j.jaquette.is_none()) {
        j.jaquette = jaquette_en_cache(&j.nom);
    }
    v.sort_by(|a, b| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()));
    v
}

/// Un processus tourne-t-il depuis ce dossier ?
pub fn tourne_dans(dossier: &str) -> bool {
    if dossier.len() < 4 {
        return false;
    }
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        if let Ok(exe) = std::fs::read_link(e.path().join("exe")) {
            if exe.starts_with(dossier) {
                return true;
            }
        }
        // jeux Windows (Proton / Wine) : le chemin est dans la ligne de commande
        if let Ok(c) = std::fs::read(e.path().join("cmdline")) {
            if String::from_utf8_lossy(&c).contains(dossier) {
                return true;
            }
        }
    }
    false
}

pub fn lancer(j: &Jeu) -> bool {
    let mut c = std::process::Command::new(&j.cmd[0]);
    c.args(&j.cmd[1..]);
    if j.id.starts_with("man:") && !j.dir.is_empty() {
        c.current_dir(&j.dir);
    }
    let ok = c.spawn().is_ok();
    if ok {
        let mut d: Donnees = charger("lanceur_linux");
        d.dernier.insert(j.id.clone(), maintenant());
        enregistrer("lanceur_linux", &d);
        // compte le temps de jeu tant qu'un processus tourne depuis le dossier du jeu
        let (id, dir) = (j.id.clone(), j.dir.clone());
        std::thread::spawn(move || {
            let debut = Instant::now();
            let mut vu = false;
            loop {
                std::thread::sleep(Duration::from_secs(10));
                let t = tourne_dans(&dir);
                if t {
                    vu = true;
                    let mut d: Donnees = charger("lanceur_linux");
                    *d.temps.entry(id.clone()).or_default() += 10.0;
                    enregistrer("lanceur_linux", &d);
                    *JEU_EN_COURS.lock().unwrap() = Some(id.clone());
                } else if vu || debut.elapsed() > Duration::from_secs(300) {
                    break;
                }
            }
            *JEU_EN_COURS.lock().unwrap() = None;
        });
    }
    ok
}

/// Identifiant du jeu lancé d'ici qui tourne encore.
pub static JEU_EN_COURS: Mutex<Option<String>> = Mutex::new(None);

// ---------- jaquettes (magasin Steam) ----------

fn slug(nom: &str) -> String {
    let s: String = nom.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
}

fn dossier_jaquettes() -> PathBuf {
    crate::outil::dossier().join("jaquettes")
}

fn jaquette_en_cache(nom: &str) -> Option<PathBuf> {
    let p = dossier_jaquettes().join(format!("{}.jpg", slug(nom)));
    p.metadata().map(|m| m.len() > 1000).unwrap_or(false).then_some(p)
}

static JAQ: AtomicBool = AtomicBool::new(false);
static NOUVELLES: AtomicBool = AtomicBool::new(false);

fn chercher_jaquettes(noms: Vec<String>) {
    if noms.is_empty() || JAQ.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let d = dossier_jaquettes();
        let _ = std::fs::create_dir_all(&d);
        let mut echecs: Vec<String> = charger("jaquettes_introuvables");
        for n in noms {
            let s = slug(&n);
            if echecs.contains(&s) {
                continue;
            }
            let id = ureq::get("https://store.steampowered.com/api/storesearch/").query("term", &n).query("l", "french").query("cc", "FR").timeout(Duration::from_secs(15)).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()).and_then(|j| {
                let items = j["items"].as_array()?.clone();
                items.iter().find(|i| slug(i["name"].as_str().unwrap_or("")) == s).or_else(|| items.first().filter(|i| {
                    let x = slug(i["name"].as_str().unwrap_or(""));
                    x.len() >= 4 && (x.starts_with(&s) || s.starts_with(&x))
                })).and_then(|i| i["id"].as_u64())
            });
            let mut ok = false;
            if let Some(id) = id {
                if let Ok(r) = ureq::get(&format!("https://cdn.akamai.steamstatic.com/steam/apps/{id}/library_600x900.jpg")).timeout(Duration::from_secs(15)).call() {
                    let mut b = vec![];
                    if std::io::Read::read_to_end(&mut r.into_reader(), &mut b).is_ok() && b.len() > 1000 {
                        ok = std::fs::write(d.join(format!("{s}.jpg")), &b).is_ok();
                    }
                }
            }
            if ok {
                NOUVELLES.store(true, Relaxed);
            } else {
                echecs.push(s);
            }
            std::thread::sleep(Duration::from_millis(1200));
        }
        enregistrer("jaquettes_introuvables", &echecs);
        JAQ.store(false, Relaxed);
    });
}

pub fn duree(s: f64) -> String {
    let m = (s / 60.0) as u64;
    if m < 60 { format!("{m} min") } else { format!("{} h {:02}", m / 60, m % 60) }
}

pub struct Lanceur {
    jeux: Vec<Jeu>,
    charge: bool,
    filtre: String,
    tri: usize,
    message: String,
}

impl Lanceur {
    pub fn new() -> Self {
        Lanceur { jeux: vec![], charge: false, filtre: String::new(), tri: 0, message: String::new() }
    }

    fn actualiser(&mut self) {
        self.jeux = jeux_installes();
        chercher_jaquettes(self.jeux.iter().filter(|j| j.jaquette.is_none()).map(|j| j.nom.clone()).collect());
        self.charge = true;
    }
}

impl Outil for Lanceur {
    fn nom(&self) -> &'static str {
        "🎮 Lanceur de jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.charge || NOUVELLES.swap(false, Relaxed) {
            self.actualiser();
        }
        let d: Donnees = charger("lanceur_linux");
        ui.horizontal_wrapped(|ui| {
            ui.heading("Mes jeux");
            ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("🔍 chercher").desired_width(180.0));
            for (k, t) in ["A → Z", "Récents", "Temps de jeu"].iter().enumerate() {
                ui.selectable_value(&mut self.tri, k, *t);
            }
            if ui.button("⟳").on_hover_text("Rechercher les jeux installés").clicked() {
                self.actualiser();
            }
            if ui.button("➕ Ajouter un jeu").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    let mut d: Donnees = charger("lanceur_linux");
                    d.manuels.push(p.display().to_string());
                    enregistrer("lanceur_linux", &d);
                    self.actualiser();
                }
            }
            ui.label(&self.message);
        });
        if self.jeux.is_empty() {
            ui.weak("Aucun jeu trouvé. Outils cherche dans Steam (y compris en Flatpak), Heroic (Epic, GOG) et Lutris. Tu peux aussi ajouter un jeu à la main.");
            return;
        }
        let f = self.filtre.to_lowercase();
        let mut l: Vec<&Jeu> = self.jeux.iter().filter(|j| f.is_empty() || j.nom.to_lowercase().contains(&f)).collect();
        match self.tri {
            1 => l.sort_by_key(|j| std::cmp::Reverse(d.dernier.get(&j.id).copied().unwrap_or(0))),
            2 => l.sort_by(|a, b| d.temps.get(&b.id).copied().unwrap_or(0.0).total_cmp(&d.temps.get(&a.id).copied().unwrap_or(0.0))),
            _ => {}
        }
        let en_cours = JEU_EN_COURS.lock().unwrap().clone();
        let mut a_lancer = None;
        ui.horizontal_wrapped(|ui| {
            for j in l {
                let (r, rep) = ui.allocate_exact_size(egui::vec2(150.0, 260.0), egui::Sense::click());
                let p = ui.painter_at(r);
                p.rect_filled(r, 6.0, if rep.hovered() { Color32::from_gray(60) } else { Color32::from_gray(38) });
                let img = egui::Rect::from_min_size(r.min + egui::vec2(6.0, 6.0), egui::vec2(138.0, 207.0));
                match &j.jaquette {
                    Some(c) => {
                        egui::Image::new(format!("file://{}", c.display())).paint_at(ui, img);
                    }
                    None => {
                        let t = j.nom.chars().map(|c| c as u32).sum::<u32>() % 360;
                        p.rect_filled(img, 4.0, Color32::from(egui::ecolor::Hsva::new(t as f32 / 360.0, 0.5, 0.45, 1.0)));
                        p.text(img.center(), egui::Align2::CENTER_CENTER, j.nom.chars().next().unwrap_or('?').to_uppercase().to_string(), egui::FontId::proportional(48.0), Color32::WHITE);
                    }
                }
                if !j.magasin.is_empty() {
                    p.text(img.right_top() + egui::vec2(-4.0, 4.0), egui::Align2::RIGHT_TOP, j.magasin, egui::FontId::proportional(10.5), Color32::WHITE);
                }
                let nom: String = j.nom.chars().take(20).collect();
                p.text(img.left_bottom() + egui::vec2(0.0, 6.0), egui::Align2::LEFT_TOP, nom, egui::FontId::proportional(13.0), Color32::WHITE);
                let t = d.temps.get(&j.id).copied().unwrap_or(0.0);
                let (texte, c) = if en_cours.as_ref() == Some(&j.id) { (format!("En cours · {}", duree(t)), Color32::LIGHT_GREEN) } else { (duree(t), Color32::GRAY) };
                p.text(r.left_bottom() + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, texte, egui::FontId::proportional(11.5), c);
                if rep.on_hover_text(format!("{} · {}", j.nom, j.dir)).clicked() {
                    a_lancer = Some(j.clone());
                }
            }
        });
        if let Some(j) = a_lancer {
            self.message = if lancer(&j) { format!("▶ {}", j.nom) } else { format!("✖ Impossible de lancer {}", j.nom) };
        }
        ui.add_space(6.0);
        ui.weak(RichText::new("Astuce : pour voir les FPS en jeu, installe MangoHud (Jeux › Moniteur de jeu)."));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"620\"\n\t\"name\"\t\t\"Portal 2\"\n\t\"installdir\"\t\t\"Portal 2\"\n}";
        assert_eq!(vdf(acf, "appid").as_deref(), Some("620"));
        assert_eq!(vdf(acf, "name").as_deref(), Some("Portal 2"));
        let lib = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/home/a/.local/share/Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"/mnt/jeux/SteamLibrary\"\n\t}\n}";
        assert_eq!(vdf_tous(lib, "path"), vec!["/home/a/.local/share/Steam", "/mnt/jeux/SteamLibrary"]);
        assert_eq!(duree(3700.0), "1 h 01");
        assert_eq!(slug("Baldur's Gate 3"), "baldur-s-gate-3");
    }
}
