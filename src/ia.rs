//! IA locale, sans internet une fois installée : assistant (27, llama.cpp + Qwen), recherche de photos
//! par leur contenu (29, petit modèle de vision), voix réaliste (30, Piper).
//! Programmes et modèles sont téléchargés une seule fois dans le dossier commun d'Outils.
use crate::createur::{chercher, telecharger};
use crate::outil::{charger, enregistrer, Cat, Outil};
use base64::Engine;
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn dossier() -> PathBuf {
    crate::outil::racine().join("ia")
}

fn sans_console(c: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = c;
}

/// Extrait une archive .zip ou .tar.gz dans `dest`.
fn extraire(archive: &Path, dest: &Path) -> Result<(), String> {
    let nom = archive.to_string_lossy().to_lowercase();
    if nom.ends_with(".zip") {
        let mut a = zip::ZipArchive::new(std::fs::File::open(archive).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        a.extract(dest).map_err(|e| e.to_string())?;
    } else {
        #[cfg(not(windows))]
        {
            let f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
            tar::Archive::new(flate2::read::GzDecoder::new(f)).unpack(dest).map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        return Err("archive .tar.gz inattendue".into());
    }
    #[cfg(not(windows))]
    {
        // les programmes extraits doivent être exécutables
        use std::os::unix::fs::PermissionsExt;
        let mut pile = vec![dest.to_path_buf()];
        while let Some(p) = pile.pop() {
            for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                let q = e.path();
                if q.is_dir() {
                    pile.push(q);
                } else if q.extension().is_none() || q.to_string_lossy().contains(".so") {
                    let _ = std::fs::set_permissions(&q, std::fs::Permissions::from_mode(0o755));
                }
            }
        }
    }
    Ok(())
}

/// Télécharge (une fois) un fichier dans le dossier de l'IA.
pub fn fichier(url: &str, nom: &str, msg: &Arc<Mutex<String>>, quoi: &str) -> Result<PathBuf, String> {
    let d = dossier();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let p = d.join(nom);
    if !p.is_file() {
        telecharger(url, &p, msg, quoi)?;
    }
    Ok(p)
}

/// Programme llama-server (llama.cpp), téléchargé depuis GitHub la première fois.
fn llama_server(msg: &Arc<Mutex<String>>) -> Result<PathBuf, String> {
    let d = dossier().join("llama");
    let exe = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    if let Some(p) = chercher(&d, &[exe]) {
        return Ok(p);
    }
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    *msg.lock().unwrap() = "Recherche de llama.cpp…".into();
    let j: serde_json::Value = ureq::get("https://api.github.com/repos/ggml-org/llama.cpp/releases/latest").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let voulus: &[&str] = if cfg!(windows) { &["bin-win-cpu-x64.zip", "bin-win-avx2-x64.zip", "bin-win-noavx-x64.zip"] } else { &["bin-ubuntu-x64.zip", "bin-ubuntu-x64.tar.gz"] };
    let assets: Vec<(String, String)> = j["assets"].as_array().into_iter().flatten().filter_map(|a| Some((a["name"].as_str()?.to_lowercase(), a["browser_download_url"].as_str()?.to_string()))).collect();
    let (nom, url) = voulus.iter().find_map(|v| assets.iter().find(|(n, _)| n.ends_with(v)).cloned()).ok_or("llama.cpp introuvable sur GitHub")?;
    let archive = d.join(&nom);
    telecharger(&url, &archive, msg, "llama.cpp")?;
    extraire(&archive, &d)?;
    let _ = std::fs::remove_file(&archive);
    chercher(&d, &[exe]).ok_or_else(|| "llama-server absent de l'archive".into())
}

/// Un serveur llama.cpp local (API compatible OpenAI sur 127.0.0.1).
pub struct Llama {
    enfant: Option<Child>,
    port: u16,
    modele: PathBuf,
}

impl Drop for Llama {
    fn drop(&mut self) {
        if let Some(c) = &mut self.enfant {
            let _ = c.kill();
        }
    }
}

impl Llama {
    pub fn demarrer(modele: &Path, mmproj: Option<&Path>, port: u16, msg: &Arc<Mutex<String>>) -> Result<Llama, String> {
        let exe = llama_server(msg)?;
        *msg.lock().unwrap() = "Démarrage de l'IA…".into();
        let fils = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(4);
        let mut c = Command::new(&exe);
        c.arg("-m").arg(modele).args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", "4096", "-t", &fils.to_string()]);
        if let Some(m) = mmproj {
            c.arg("--mmproj").arg(m);
        }
        if let Some(d) = exe.parent() {
            c.current_dir(d);
            #[cfg(not(windows))]
            c.env("LD_LIBRARY_PATH", d);
        }
        c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        sans_console(&mut c);
        let enfant = c.spawn().map_err(|e| format!("llama-server ne démarre pas : {e}"))?;
        let mut l = Llama { enfant: Some(enfant), port, modele: modele.to_path_buf() };
        let debut = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if ureq::get(&format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).call().is_ok() {
                *msg.lock().unwrap() = "✔ IA prête".into();
                return Ok(l);
            }
            if let Some(c) = &mut l.enfant {
                if let Ok(Some(s)) = c.try_wait() {
                    return Err(format!("llama-server s'est arrêté ({s}) : pas assez de mémoire ?"));
                }
            }
            if debut.elapsed() > Duration::from_secs(240) {
                return Err("l'IA met trop de temps à démarrer".into());
            }
        }
    }

    pub fn modele(&self) -> &Path {
        &self.modele
    }

    /// Envoie la conversation ; chaque morceau de réponse passe par `morceau`. Renvoie la réponse entière.
    pub fn discuter(&self, messages: serde_json::Value, max: u32, stop: &AtomicBool, mut morceau: impl FnMut(&str)) -> Result<String, String> {
        let corps = serde_json::json!({ "messages": messages, "stream": true, "temperature": 0.6, "max_tokens": max });
        let r = ureq::post(&format!("http://127.0.0.1:{}/v1/chat/completions", self.port)).timeout(Duration::from_secs(600)).send_json(corps).map_err(|e| e.to_string())?;
        let mut tout = String::new();
        for l in std::io::BufReader::new(r.into_reader()).lines() {
            if stop.load(Relaxed) {
                break;
            }
            let l = l.map_err(|e| e.to_string())?;
            let Some(d) = l.strip_prefix("data: ") else { continue };
            if d.trim() == "[DONE]" {
                break;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(d) {
                if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
                    tout.push_str(t);
                    morceau(t);
                }
            }
        }
        Ok(tout)
    }
}

// ---------------------------------------------------------------------------------------------------
// 27 · Assistant
// ---------------------------------------------------------------------------------------------------

/// Modèles : nom, fichier, adresse, taille affichée.
pub const MODELES: [(&str, &str, &str, &str); 3] = [
    ("Léger (rapide)", "qwen2.5-0.5b-instruct-q4_k_m.gguf", "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf", "490 Mo"),
    ("Normal", "qwen2.5-1.5b-instruct-q4_k_m.gguf", "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf", "1,1 Go"),
    ("Précis (plus lent)", "qwen2.5-3b-instruct-q4_k_m.gguf", "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf", "2,1 Go"),
];

pub const MODES: [(&str, &str); 4] = [
    ("💬 Discuter", "Tu es l'assistant d'Outils, sur l'ordinateur de l'utilisateur. Réponds en français, simplement et clairement, en quelques phrases. Si tu ne sais pas, dis-le."),
    ("📚 Aide aux devoirs", "Tu es un professeur patient. Réponds en français. Ne donne pas directement la réponse finale : explique la méthode étape par étape, donne un exemple, puis pose une question pour vérifier que l'élève a compris."),
    ("📝 Résumer", "Résume le texte donné en français, en 5 points courts maximum, avec les idées importantes. N'ajoute rien qui n'est pas dans le texte."),
    ("✔ Corriger", "Corrige l'orthographe, la grammaire et la ponctuation du texte donné, en français. Donne d'abord le texte corrigé, puis la liste des fautes corrigées avec une courte explication."),
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
struct ReglagesAssistant {
    modele: usize,
}

pub struct Assistant {
    r: ReglagesAssistant,
    mode: usize,
    messages: Arc<Mutex<Vec<(bool, String)>>>,
    saisie: String,
    llama: Arc<Mutex<Option<Llama>>>,
    etat: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Assistant {
    pub fn new() -> Self {
        Assistant { r: charger("assistant_ia"), mode: 0, messages: Arc::default(), saisie: String::new(), llama: Arc::default(), etat: Arc::default(), occupe: Arc::default(), stop: Arc::default() }
    }

    fn envoyer(&mut self) {
        let texte = std::mem::take(&mut self.saisie).trim().to_string();
        if texte.is_empty() || self.occupe.swap(true, Relaxed) {
            return;
        }
        self.messages.lock().unwrap().push((true, texte));
        self.messages.lock().unwrap().push((false, String::new()));
        let (ms, ll, et, oc, st) = (self.messages.clone(), self.llama.clone(), self.etat.clone(), self.occupe.clone(), self.stop.clone());
        st.store(false, Relaxed);
        let (choix, systeme) = (MODELES[self.r.modele.min(2)], MODES[self.mode].1);
        std::thread::spawn(move || {
            let r = (|| -> Result<(), String> {
                let modele = fichier(choix.2, choix.1, &et, &format!("l'IA « {} » ({}, une seule fois)", choix.0, choix.3))?;
                {
                    let mut l = ll.lock().unwrap();
                    if l.as_ref().is_none_or(|x| x.modele() != modele) {
                        *l = None;
                        *l = Some(Llama::demarrer(&modele, None, 8765, &et)?);
                    }
                }
                let mut conv = vec![serde_json::json!({"role": "system", "content": systeme})];
                let historique: Vec<(bool, String)> = ms.lock().unwrap().iter().filter(|m| !m.1.is_empty()).cloned().collect();
                for (u, t) in historique.iter().skip(historique.len().saturating_sub(12)) {
                    conv.push(serde_json::json!({"role": if *u { "user" } else { "assistant" }, "content": t}));
                }
                *et.lock().unwrap() = "L'IA écrit…".into();
                let g = ll.lock().unwrap();
                let l = g.as_ref().ok_or("IA arrêtée")?;
                l.discuter(serde_json::Value::Array(conv), 1024, &st, |m| {
                    if let Some(d) = ms.lock().unwrap().last_mut() {
                        d.1.push_str(m);
                    }
                })?;
                *et.lock().unwrap() = String::new();
                Ok(())
            })();
            if let Err(e) = r {
                *et.lock().unwrap() = format!("✖ {e}");
                let mut m = ms.lock().unwrap();
                if m.last().is_some_and(|x| !x.0 && x.1.is_empty()) {
                    m.pop();
                }
            }
            oc.store(false, Relaxed);
        });
    }
}

impl Outil for Assistant {
    fn nom(&self) -> &'static str {
        "🤖 Assistant IA (hors ligne)"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Assistant IA");
        ui.weak("Une IA qui tourne sur ton PC : tes questions ne partent pas sur internet. Elle peut se tromper : vérifie ce qui compte.");
        let avant = self.r.clone();
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal_wrapped(|ui| {
            for (k, (n, _)) in MODES.iter().enumerate() {
                ui.selectable_value(&mut self.mode, k, *n);
            }
            ui.separator();
            ui.add_enabled_ui(!occupe, |ui| {
                egui::ComboBox::from_id_salt("modele_ia").selected_text(MODELES[self.r.modele.min(2)].0).show_ui(ui, |ui| {
                    for (k, m) in MODELES.iter().enumerate() {
                        let deja = dossier().join(m.1).is_file();
                        ui.selectable_value(&mut self.r.modele, k, format!("{} — {}{}", m.0, m.3, if deja { " ✔" } else { "" }));
                    }
                });
            });
            if ui.small_button("🧹 Nouvelle conversation").clicked() && !occupe {
                self.messages.lock().unwrap().clear();
            }
        });
        let etat = self.etat.lock().unwrap().clone();
        if !etat.is_empty() {
            ui.horizontal(|ui| {
                if occupe {
                    ui.spinner();
                }
                ui.label(etat);
            });
        }
        let hauteur = ui.available_height() - 110.0;
        egui::ScrollArea::vertical().max_height(hauteur.max(160.0)).stick_to_bottom(true).show(ui, |ui| {
            for (u, t) in self.messages.lock().unwrap().iter() {
                let (fond, aligne) = if *u { (Color32::from_rgb(40, 70, 120), egui::Align::Max) } else { (Color32::from_rgb(45, 45, 52), egui::Align::Min) };
                ui.with_layout(egui::Layout::top_down(aligne), |ui| {
                    egui::Frame::new().fill(fond).corner_radius(10.0).inner_margin(10.0).show(ui, |ui| {
                        ui.set_max_width(ui.available_width() * 0.8);
                        ui.label(if t.is_empty() { "…" } else { t.as_str() });
                    });
                });
                ui.add_space(4.0);
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::multiline(&mut self.saisie).desired_rows(2).desired_width(ui.available_width() - 110.0).hint_text(if self.mode >= 2 { "Colle ton texte ici…" } else { "Écris ta question… (Entrée pour envoyer, Maj+Entrée pour aller à la ligne)" }));
            let entree = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
            if entree {
                self.saisie = self.saisie.trim_end_matches('\n').to_string();
            }
            ui.vertical(|ui| {
                if occupe {
                    if ui.button("⏹ Stop").clicked() {
                        self.stop.store(true, Relaxed);
                    }
                } else if ui.button(RichText::new("➤ Envoyer").strong()).clicked() || entree {
                    self.envoyer();
                }
                if let Some((false, t)) = self.messages.lock().unwrap().last().cloned() {
                    if ui.small_button("📋 Copier").clicked() {
                        ui.ctx().copy_text(t);
                    }
                }
            });
        });
        if occupe {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if self.r != avant {
            enregistrer("assistant_ia", &self.r);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 29 · Recherche de photos par leur contenu
// ---------------------------------------------------------------------------------------------------

const VISION: (&str, &str, &str, &str) = (
    "SmolVLM-500M-Instruct-Q8_0.gguf",
    "https://huggingface.co/ggml-org/SmolVLM-500M-Instruct-GGUF/resolve/main/SmolVLM-500M-Instruct-Q8_0.gguf",
    "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
    "https://huggingface.co/ggml-org/SmolVLM-500M-Instruct-GGUF/resolve/main/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf",
);

/// Petit dictionnaire français → anglais pour chercher dans les descriptions (écrites en anglais).
pub const DICO: &[(&str, &str)] = &[
    ("chien", "dog"), ("chat", "cat"), ("chiot", "puppy"), ("chaton", "kitten"), ("oiseau", "bird"), ("cheval", "horse"), ("vache", "cow"), ("poisson", "fish"), ("lapin", "rabbit"), ("animal", "animal"),
    ("plage", "beach"), ("mer", "sea"), ("océan", "ocean"), ("montagne", "mountain"), ("neige", "snow"), ("forêt", "forest"), ("arbre", "tree"), ("fleur", "flower"), ("jardin", "garden"), ("lac", "lake"),
    ("rivière", "river"), ("ciel", "sky"), ("nuage", "cloud"), ("soleil", "sun"), ("coucher", "sunset"), ("nuit", "night"), ("ville", "city"), ("rue", "street"), ("route", "road"), ("pont", "bridge"),
    ("maison", "house"), ("bâtiment", "building"), ("église", "church"), ("château", "castle"), ("voiture", "car"), ("vélo", "bicycle"), ("moto", "motorcycle"), ("bus", "bus"), ("train", "train"), ("avion", "airplane"),
    ("bateau", "boat"), ("personne", "person"), ("homme", "man"), ("femme", "woman"), ("enfant", "child"), ("bébé", "baby"), ("garçon", "boy"), ("fille", "girl"), ("famille", "family"), ("groupe", "group"),
    ("visage", "face"), ("sourire", "smiling"), ("selfie", "selfie"), ("gâteau", "cake"), ("anniversaire", "birthday"), ("repas", "meal"), ("nourriture", "food"), ("pizza", "pizza"), ("table", "table"), ("cuisine", "kitchen"),
    ("chambre", "bedroom"), ("lit", "bed"), ("canapé", "couch"), ("salon", "living room"), ("bureau", "desk"), ("ordinateur", "computer"), ("écran", "screen"), ("téléphone", "phone"), ("jeu", "game"), ("jeux", "game"),
    ("capture", "screenshot"), ("texte", "text"), ("document", "document"), ("papier", "paper"), ("livre", "book"), ("dessin", "drawing"), ("carte", "map"), ("sport", "sport"), ("football", "soccer"), ("ballon", "ball"),
    ("piscine", "pool"), ("eau", "water"), ("herbe", "grass"), ("champ", "field"), ("noël", "christmas"), ("sapin", "christmas tree"), ("fête", "party"), ("concert", "concert"), ("musique", "music"), ("guitare", "guitar"),
    ("rouge", "red"), ("bleu", "blue"), ("vert", "green"), ("jaune", "yellow"), ("noir", "black"), ("blanc", "white"), ("rose", "pink"), ("orange", "orange"), ("violet", "purple"), ("gris", "gray"),
    ("vêtement", "clothes"), ("chaussure", "shoe"), ("lunettes", "glasses"), ("chapeau", "hat"), ("magasin", "store"), ("restaurant", "restaurant"), ("école", "school"), ("classe", "classroom"), ("tableau", "board"), ("bébé", "infant"),
    ("minecraft", "minecraft"), ("voiture", "vehicle"), ("chien", "puppy"), ("jouet", "toy"), ("peluche", "stuffed animal"), ("intérieur", "indoor"), ("extérieur", "outdoor"), ("vacances", "vacation"), ("camping", "tent"), ("feu", "fire"),
];

/// Mots cherchés (avec leur traduction anglaise).
pub fn mots_recherche(q: &str) -> Vec<String> {
    let mut v = Vec::new();
    for m in q.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|m| m.len() > 1) {
        let s = m.trim_end_matches('s').trim_end_matches('x');
        v.push(m.to_string());
        for (fr, en) in DICO {
            if *fr == m || *fr == s {
                v.push(en.to_string());
            }
        }
    }
    v.sort();
    v.dedup();
    v
}

/// Score d'une description pour une recherche.
pub fn score(description: &str, mots: &[String]) -> usize {
    let d = description.to_lowercase();
    mots.iter().filter(|m| d.contains(m.as_str()) || d.contains(m.trim_end_matches('s'))).count()
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Index {
    dossier: Option<PathBuf>,
    photos: Vec<(PathBuf, u64, String)>, // chemin, date de modification, description
}

fn photos_du_dossier(d: &Path) -> Vec<(PathBuf, u64)> {
    let mut v = Vec::new();
    let mut pile = vec![d.to_path_buf()];
    while let Some(p) = pile.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let q = e.path();
            if q.is_dir() {
                pile.push(q);
            } else if q.extension().and_then(|e| e.to_str()).is_some_and(|e| ["jpg", "jpeg", "png", "webp", "bmp"].contains(&e.to_lowercase().as_str())) {
                let t = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
                v.push((q, t));
            }
        }
    }
    v
}

pub struct RecherchePhotos {
    index: Arc<Mutex<Index>>,
    recherche: String,
    etat: Arc<Mutex<String>>,
    en_cours: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    vignettes: Vec<(PathBuf, egui::TextureHandle)>,
    resultats: Vec<PathBuf>,
}

impl RecherchePhotos {
    pub fn new() -> Self {
        let mut i: Index = charger("index_photos_ia");
        if i.dossier.is_none() {
            let m = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
            let p = PathBuf::from(m);
            i.dossier = [p.join("Pictures"), p.join("Images")].into_iter().find(|d| d.is_dir());
        }
        RecherchePhotos { index: Arc::new(Mutex::new(i)), recherche: String::new(), etat: Arc::default(), en_cours: Arc::default(), stop: Arc::default(), vignettes: Vec::new(), resultats: Vec::new() }
    }

    fn indexer(&self) {
        let Some(d) = self.index.lock().unwrap().dossier.clone() else { return };
        if self.en_cours.swap(true, Relaxed) {
            return;
        }
        self.stop.store(false, Relaxed);
        let (ix, et, ec, st) = (self.index.clone(), self.etat.clone(), self.en_cours.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let r = (|| -> Result<(), String> {
                let toutes = photos_du_dossier(&d);
                let a_faire: Vec<(PathBuf, u64)> = {
                    let i = ix.lock().unwrap();
                    toutes.into_iter().filter(|(p, t)| !i.photos.iter().any(|(q, u, _)| q == p && u == t)).collect()
                };
                if a_faire.is_empty() {
                    *et.lock().unwrap() = "✔ Toutes les photos sont déjà analysées.".into();
                    return Ok(());
                }
                let modele = fichier(VISION.1, VISION.0, &et, "le modèle de vision (une seule fois)")?;
                let mmproj = fichier(VISION.3, VISION.2, &et, "le modèle de vision (2/2)")?;
                let l = Llama::demarrer(&modele, Some(&mmproj), 8766, &et)?;
                let debut = Instant::now();
                for (k, (p, t)) in a_faire.iter().enumerate() {
                    if st.load(Relaxed) {
                        break;
                    }
                    let reste = if k > 0 { debut.elapsed().as_secs_f32() / k as f32 * (a_faire.len() - k) as f32 } else { 0.0 };
                    *et.lock().unwrap() = format!("Analyse des photos : {}/{} (encore ~{} min)", k + 1, a_faire.len(), (reste / 60.0).ceil());
                    let Ok(img) = image::open(p) else { continue };
                    let petite = img.thumbnail(448, 448).to_rgb8();
                    let mut jpg = Vec::new();
                    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 85).encode_image(&petite).is_err() {
                        continue;
                    }
                    let url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(&jpg));
                    let msg = serde_json::json!([{"role": "user", "content": [
                        {"type": "image_url", "image_url": {"url": url}},
                        {"type": "text", "text": "Describe this picture in one sentence, then list 10 keywords (objects, place, colors, people, animals)."}
                    ]}]);
                    let desc = l.discuter(msg, 90, &st, |_| {}).unwrap_or_default();
                    let mut i = ix.lock().unwrap();
                    i.photos.retain(|(q, _, _)| q != p);
                    i.photos.push((p.clone(), *t, desc.trim().to_string()));
                    if k % 10 == 9 {
                        enregistrer("index_photos_ia", &*i);
                    }
                }
                enregistrer("index_photos_ia", &*ix.lock().unwrap());
                *et.lock().unwrap() = format!("✔ {} photos analysées.", ix.lock().unwrap().photos.len());
                Ok(())
            })();
            if let Err(e) = r {
                *et.lock().unwrap() = format!("✖ {e}");
            }
            ec.store(false, Relaxed);
        });
    }

    fn chercher(&mut self, ctx: &egui::Context) {
        let mots = mots_recherche(&self.recherche);
        let i = self.index.lock().unwrap();
        let mut v: Vec<(usize, &PathBuf)> = i.photos.iter().map(|(p, _, d)| (score(d, &mots), p)).filter(|(s, _)| *s > 0).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        self.resultats = v.into_iter().take(60).map(|(_, p)| p.clone()).collect();
        drop(i);
        self.vignettes.retain(|(p, _)| self.resultats.contains(p));
        for p in self.resultats.clone() {
            if self.vignettes.iter().any(|(q, _)| *q == p) {
                continue;
            }
            if let Ok(img) = image::open(&p) {
                let t = img.thumbnail(160, 160).to_rgba8();
                let c = egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw());
                self.vignettes.push((p.clone(), ctx.load_texture(p.to_string_lossy(), c, Default::default())));
            }
        }
    }
}

impl Outil for RecherchePhotos {
    fn nom(&self) -> &'static str {
        "🔎 Photos par leur contenu"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chercher une photo par ce qu'elle montre");
        ui.weak("Une IA regarde tes photos (sur ton PC, sans internet) et écrit ce qu'elle voit. Ensuite, cherche « chien plage », « gâteau », « neige »…");
        let (dossier, n) = {
            let i = self.index.lock().unwrap();
            (i.dossier.clone(), i.photos.len())
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("📁 {}", dossier.as_ref().map(|d| d.display().to_string()).unwrap_or("aucun dossier".into())));
            if ui.small_button("Changer…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.index.lock().unwrap().dossier = Some(d);
                }
            }
            ui.label(format!("· {n} photo(s) analysée(s)"));
            if self.en_cours.load(Relaxed) {
                if ui.button("⏸ Arrêter l'analyse").clicked() {
                    self.stop.store(true, Relaxed);
                }
            } else if ui.button("🧠 Analyser les nouvelles photos").clicked() {
                self.indexer();
            }
        });
        let e = self.etat.lock().unwrap().clone();
        if !e.is_empty() {
            ui.label(e);
        }
        if self.en_cours.load(Relaxed) {
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔎 chien, plage, anniversaire, voiture rouge…").desired_width(360.0).font(egui::TextStyle::Heading));
            if ui.button("Chercher").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.chercher(ui.ctx());
            }
        });
        if !self.recherche.is_empty() && self.resultats.is_empty() {
            ui.weak("Rien trouvé (ou pas encore analysé). Astuce : tu peux aussi chercher en anglais.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for p in &self.resultats {
                    if let Some((_, t)) = self.vignettes.iter().find(|(q, _)| q == p) {
                        let desc = self.index.lock().unwrap().photos.iter().find(|x| x.0 == *p).map(|x| x.2.clone()).unwrap_or_default();
                        let r = ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(150.0, 150.0)).sense(egui::Sense::click())).on_hover_text(format!("{}\n\n{desc}", p.display()));
                        if r.clicked() {
                            crate::outil::ouvrir(p);
                        }
                    }
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 30 · Voix réaliste (Piper)
// ---------------------------------------------------------------------------------------------------

pub const VOIX: [(&str, &str); 2] = [("Siwis (femme)", "fr/fr_FR/siwis/medium/fr_FR-siwis-medium"), ("Tom (homme)", "fr/fr_FR/tom/medium/fr_FR-tom-medium")];

fn piper(msg: &Arc<Mutex<String>>) -> Result<PathBuf, String> {
    let d = dossier().join("piper");
    let exe = if cfg!(windows) { "piper.exe" } else { "piper" };
    if let Some(p) = chercher(&d, &[exe]) {
        return Ok(p);
    }
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let (url, nom) = if cfg!(windows) {
        ("https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip", "piper.zip")
    } else {
        ("https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz", "piper.tar.gz")
    };
    let a = d.join(nom);
    telecharger(url, &a, msg, "Piper (voix)")?;
    extraire(&a, &d)?;
    let _ = std::fs::remove_file(&a);
    chercher(&d, &[exe]).ok_or_else(|| "piper absent de l'archive".into())
}

/// Fabrique un WAV avec la voix choisie.
pub fn synthese(texte: &str, voix: usize, vitesse: f32, sortie: &Path, msg: &Arc<Mutex<String>>) -> Result<(), String> {
    let exe = piper(msg)?;
    let chemin = VOIX[voix.min(VOIX.len() - 1)].1;
    let nom = chemin.rsplit('/').next().unwrap_or("voix");
    let base = "https://huggingface.co/rhasspy/piper-voices/resolve/main/";
    let modele = fichier(&format!("{base}{chemin}.onnx"), &format!("{nom}.onnx"), msg, "la voix (une seule fois)")?;
    fichier(&format!("{base}{chemin}.onnx.json"), &format!("{nom}.onnx.json"), msg, "la voix")?;
    *msg.lock().unwrap() = "Lecture du texte…".into();
    let mut c = Command::new(&exe);
    c.arg("--model").arg(&modele).arg("--output_file").arg(sortie).args(["--length_scale", &format!("{:.2}", 1.0 / vitesse.clamp(0.5, 2.0))]);
    if let Some(d) = exe.parent() {
        c.current_dir(d);
        #[cfg(not(windows))]
        c.env("LD_LIBRARY_PATH", d);
    }
    c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    sans_console(&mut c);
    let mut e = c.spawn().map_err(|e| e.to_string())?;
    if let Some(mut s) = e.stdin.take() {
        use std::io::Write;
        let _ = s.write_all(texte.as_bytes());
    }
    let mut err = String::new();
    if let Some(mut s) = e.stderr.take() {
        let _ = s.read_to_string(&mut err);
    }
    let st = e.wait().map_err(|e| e.to_string())?;
    if st.success() && sortie.is_file() {
        Ok(())
    } else {
        Err(err.lines().last().unwrap_or("la voix n'a pas fonctionné").to_string())
    }
}

pub struct VoixRealiste {
    texte: String,
    voix: usize,
    vitesse: f32,
    msg: Arc<Mutex<String>>,
    occupe: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl VoixRealiste {
    pub fn new() -> Self {
        VoixRealiste { texte: "Bonjour ! Je suis la voix d'Outils. Je peux lire tes textes, tes exposés ou les dialogues de tes vidéos.".into(), voix: 0, vitesse: 1.0, msg: Arc::default(), occupe: Arc::default(), stop: Arc::new(AtomicBool::new(false)) }
    }

    fn lancer(&mut self, dest: Option<PathBuf>) {
        if self.occupe.swap(true, Relaxed) {
            return;
        }
        self.stop.store(true, Relaxed);
        self.stop = Arc::new(AtomicBool::new(false));
        let (t, v, vi, m, o, st) = (self.texte.clone(), self.voix, self.vitesse, self.msg.clone(), self.occupe.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let wav = std::env::temp_dir().join("outils_voix.wav");
            let r = synthese(&t, v, vi, &wav, &m).and_then(|_| match &dest {
                None => {
                    *m.lock().unwrap() = "🔊 Lecture…".into();
                    crate::scene::jouer_wav(wav.clone(), st);
                    Ok(String::new())
                }
                Some(d) if d.extension().is_some_and(|e| e == "mp3") => {
                    let o = crate::createur::ffmpeg()?.args(["-y", "-v", "error", "-i"]).arg(&wav).args(["-b:a", "160k"]).arg(d).output().map_err(|e| e.to_string())?;
                    if o.status.success() { Ok(format!("✔ {}", d.display())) } else { Err(String::from_utf8_lossy(&o.stderr).trim().to_string()) }
                }
                Some(d) => std::fs::copy(&wav, d).map(|_| format!("✔ {}", d.display())).map_err(|e| e.to_string()),
            });
            *m.lock().unwrap() = r.unwrap_or_else(|e| format!("✖ {e}"));
            o.store(false, Relaxed);
        });
    }
}

impl Outil for VoixRealiste {
    fn nom(&self) -> &'static str {
        "🗣 Voix réaliste"
    }
    fn cat(&self) -> Cat {
        Cat::Creatif
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Voix réaliste");
        ui.weak("Une voix naturelle qui lit ton texte, sur ton PC (Piper). Pour tes exposés, tes vidéos, ou te faire lire un cours.");
        ui.horizontal_wrapped(|ui| {
            for (k, (n, _)) in VOIX.iter().enumerate() {
                ui.selectable_value(&mut self.voix, k, *n);
            }
            ui.label("Vitesse");
            ui.add(egui::Slider::new(&mut self.vitesse, 0.6..=1.6).show_value(false));
        });
        ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(8).desired_width(f32::INFINITY));
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!occupe, egui::Button::new(RichText::new("▶ Écouter").size(18.0))).clicked() {
                self.lancer(None);
            }
            if ui.button("⏹").clicked() {
                self.stop.store(true, Relaxed);
            }
            if ui.add_enabled(!occupe, egui::Button::new("💾 Enregistrer en MP3…")).clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("MP3", &["mp3"]).add_filter("WAV", &["wav"]).set_file_name("voix.mp3").save_file() {
                    self.lancer(Some(p));
                }
            }
            if occupe {
                ui.spinner();
            }
        });
        ui.label(self.msg.lock().unwrap().clone());
        if occupe {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recherche() {
        let m = mots_recherche("Chiens à la plage");
        assert!(m.contains(&"dog".to_string()) && m.contains(&"beach".to_string()));
        assert!(score("A brown dog running on a sandy beach. Keywords: dog, beach, sand", &m) >= 2);
        assert_eq!(score("A plate of pasta", &m), 0);
        assert_eq!(MODELES.len(), 3);
    }
}
