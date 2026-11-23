//! Serveur Minecraft sous Linux : installation (Vanilla ou Paper), démarrage, console, joueurs,
//! sauvegardes du monde. Pilotable aussi depuis le téléphone.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Reglages {
    pub dossier: PathBuf,
    pub ram_go: u32,
    pub eula: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { dossier: PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("ServeurMinecraft"), ram_go: 2, eula: false }
    }
}

#[derive(Default)]
pub struct Etat {
    enfant: Option<Child>,
    entree: Option<ChildStdin>,
    pub console: Vec<String>,
    pub joueurs: Vec<String>,
    pub pret: bool,
    pub message: String,
}

pub fn etat() -> &'static Arc<Mutex<Etat>> {
    static E: OnceLock<Arc<Mutex<Etat>>> = OnceLock::new();
    E.get_or_init(Arc::default)
}

/// Joueur qui arrive ou part d'après une ligne de la console : (pseudo, arrive ?).
pub fn evenement_joueur(l: &str) -> Option<(String, bool)> {
    let t = l.rsplit("]: ").next()?;
    if let Some(n) = t.strip_suffix(" joined the game") {
        return Some((n.trim().to_string(), true));
    }
    t.strip_suffix(" left the game").map(|n| (n.trim().to_string(), false))
}

pub fn en_marche() -> bool {
    let mut g = etat().lock().unwrap();
    match g.enfant.as_mut().map(|c| c.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            g.enfant = None;
            g.entree = None;
            g.pret = false;
            g.joueurs.clear();
            false
        }
        None => false,
    }
}

/// None : arrêté ; Some((prêt, joueurs)).
pub fn statut() -> Option<(bool, Vec<String>)> {
    if !en_marche() {
        return None;
    }
    let g = etat().lock().unwrap();
    Some((g.pret, g.joueurs.clone()))
}

pub fn commande(c: &str) -> bool {
    let mut g = etat().lock().unwrap();
    let ok = g.entree.as_mut().is_some_and(|e| writeln!(e, "{c}").is_ok());
    if ok {
        g.console.push(format!("> {c}"));
    }
    ok
}

pub fn demarrer() -> Result<(), String> {
    if en_marche() {
        return Ok(());
    }
    let r: Reglages = charger("minecraft_linux");
    let jar = r.dossier.join("server.jar");
    if !jar.is_file() {
        return Err("serveur pas installé".into());
    }
    if !r.eula {
        return Err("accepte d'abord le contrat de Minecraft (EULA)".into());
    }
    let _ = std::fs::write(r.dossier.join("eula.txt"), "eula=true\n");
    let mut c = Command::new("java")
        .args([format!("-Xmx{}G", r.ram_go.max(1)), format!("-Xms{}G", (r.ram_go / 2).max(1)), "-jar".into(), "server.jar".into(), "nogui".into()])
        .current_dir(&r.dossier)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Java introuvable ({e}) : installe-le (ex. sudo apt install openjdk-21-jre-headless)"))?;
    let sortie = c.stdout.take();
    let erreurs = c.stderr.take();
    {
        let mut g = etat().lock().unwrap();
        g.entree = c.stdin.take();
        g.enfant = Some(c);
        g.console.clear();
        g.pret = false;
        g.joueurs.clear();
    }
    for flux in [sortie.map(|s| Box::new(s) as Box<dyn Read + Send>), erreurs.map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        std::thread::spawn(move || {
            for l in BufReader::new(flux).lines().map_while(Result::ok) {
                let mut g = etat().lock().unwrap();
                if l.contains("Done (") {
                    g.pret = true;
                    drop(g);
                    crate::systeme::notifier("Serveur Minecraft", "Le serveur est prêt !");
                    g = etat().lock().unwrap();
                }
                if let Some((n, arrive)) = evenement_joueur(&l) {
                    if arrive {
                        g.joueurs.push(n);
                    } else {
                        g.joueurs.retain(|x| *x != n);
                    }
                }
                g.console.push(l);
                let n = g.console.len();
                if n > 600 {
                    g.console.drain(..n - 500);
                }
            }
        });
    }
    Ok(())
}

pub fn arreter() {
    commande("stop");
}

fn zipper(src: &Path, dest: &Path) -> Result<(), String> {
    let mut z = zip::ZipWriter::new(std::fs::File::create(dest).map_err(|e| e.to_string())?);
    let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    let base = src.parent().unwrap_or(src);
    let mut pile = vec![src.to_path_buf()];
    while let Some(d) = pile.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.is_dir() {
                pile.push(p);
                continue;
            }
            let Ok(b) = std::fs::read(&p) else { continue };
            let rel = p.strip_prefix(base).map_err(|e| e.to_string())?.to_string_lossy().to_string();
            z.start_file(rel, opt).map_err(|e| e.to_string())?;
            z.write_all(&b).map_err(|e| e.to_string())?;
        }
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn sauvegarder() -> Result<PathBuf, String> {
    let r: Reglages = charger("minecraft_linux");
    let monde = std::fs::read_to_string(r.dossier.join("server.properties")).ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("level-name=").map(|x| x.trim().to_string()))).unwrap_or_else(|| "world".into());
    let src = r.dossier.join(&monde);
    if !src.is_dir() {
        return Err("aucun monde à sauvegarder (lance le serveur une fois)".into());
    }
    if en_marche() {
        commande("save-all flush");
        commande("save-off");
        std::thread::sleep(Duration::from_secs(3));
    }
    let d = r.dossier.join("sauvegardes");
    let _ = std::fs::create_dir_all(&d);
    let dest = d.join(format!("{monde}_{}.zip", crate::minecraft::horodatage()));
    let res = zipper(&src, &dest);
    if en_marche() {
        commande("save-on");
    }
    res.map(|_| dest)
}

/// Télécharge server.jar : (genre « vanilla » ou « paper », version obtenue).
fn installer(dossier: &Path, paper: bool) -> Result<String, String> {
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    let a = ureq::AgentBuilder::new().timeout(Duration::from_secs(120)).user_agent("Outils").build();
    let get = |u: &str| a.get(u).call().map_err(|e| e.to_string())?.into_json::<serde_json::Value>().map_err(|e| e.to_string());
    let (url, version) = if paper {
        let p = get("https://api.papermc.io/v2/projects/paper")?;
        let v = p["versions"].as_array().and_then(|x| x.last()).and_then(|x| x.as_str()).ok_or("versions de Paper illisibles")?.to_string();
        let b = get(&format!("https://api.papermc.io/v2/projects/paper/versions/{v}/builds"))?;
        let last = b["builds"].as_array().and_then(|x| x.last()).ok_or("builds illisibles")?;
        let n = last["build"].as_u64().unwrap_or(0);
        let nom = last["downloads"]["application"]["name"].as_str().unwrap_or("").to_string();
        (format!("https://api.papermc.io/v2/projects/paper/versions/{v}/builds/{n}/downloads/{nom}"), format!("Paper {v}"))
    } else {
        let m = get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
        let v = m["latest"]["release"].as_str().ok_or("version illisible")?.to_string();
        let u = m["versions"].as_array().and_then(|l| l.iter().find(|x| x["id"] == v.as_str())).and_then(|x| x["url"].as_str()).ok_or("version introuvable")?.to_string();
        let d = get(&u)?;
        (d["downloads"]["server"]["url"].as_str().ok_or("pas de serveur pour cette version")?.to_string(), format!("Vanilla {v}"))
    };
    let r = a.get(&url).call().map_err(|e| e.to_string())?;
    let mut b = vec![];
    r.into_reader().read_to_end(&mut b).map_err(|e| e.to_string())?;
    if b.len() < 100_000 {
        return Err("téléchargement incomplet".into());
    }
    std::fs::write(dossier.join("server.jar"), b).map_err(|e| e.to_string())?;
    Ok(version)
}

pub struct ServeurMc {
    r: Reglages,
    commande: String,
    occupe: Arc<Mutex<Option<String>>>,
}

impl ServeurMc {
    pub fn new() -> Self {
        ServeurMc { r: charger("minecraft_linux"), commande: String::new(), occupe: Arc::default() }
    }
}

impl Outil for ServeurMc {
    fn nom(&self) -> &'static str {
        "⛏ Serveur Minecraft"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Serveur Minecraft");
        let installe = self.r.dossier.join("server.jar").is_file();
        let marche = en_marche();
        let avant = self.r.clone();
        ui.horizontal_wrapped(|ui| {
            ui.label("Dossier :");
            ui.monospace(self.r.dossier.display().to_string());
            if !marche && ui.small_button("Changer…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.r.dossier = d;
                }
            }
            ui.label("Mémoire :");
            ui.add(egui::DragValue::new(&mut self.r.ram_go).range(1..=32).suffix(" Go"));
        });
        if ui.checkbox(&mut self.r.eula, "J'accepte le contrat d'utilisation de Minecraft (EULA)").changed() || self.r.dossier != avant.dossier || self.r.ram_go != avant.ram_go {
            enregistrer("minecraft_linux", &self.r);
        }
        ui.hyperlink_to("Lire le contrat (EULA)", "https://aka.ms/MinecraftEULA");
        let occupe = self.occupe.lock().unwrap().clone();
        if let Some(m) = &occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(m);
            });
            ui.ctx().request_repaint_after(Duration::from_millis(400));
        }
        ui.horizontal_wrapped(|ui| {
            for (paper, t) in [(false, "📥 Installer Vanilla (officiel)"), (true, "📥 Installer Paper (plugins, plus rapide)")] {
                if ui.add_enabled(!marche && occupe.is_none(), egui::Button::new(if installe { t.replace("Installer", "Mettre à jour") } else { t.to_string() })).clicked() {
                    let (o, d) = (self.occupe.clone(), self.r.dossier.clone());
                    *o.lock().unwrap() = Some("Téléchargement du serveur…".into());
                    std::thread::spawn(move || {
                        let m = match installer(&d, paper) {
                            Ok(v) => format!("✔ {v} installé"),
                            Err(e) => format!("✖ {e}"),
                        };
                        etat().lock().unwrap().message = m;
                        *o.lock().unwrap() = None;
                    });
                }
            }
        });
        ui.separator();
        let (pret, joueurs, message) = {
            let g = etat().lock().unwrap();
            (g.pret, g.joueurs.clone(), g.message.clone())
        };
        ui.horizontal(|ui| {
            if !marche {
                ui.label(RichText::new("● Arrêté").color(Color32::GRAY).strong());
                if ui.add_enabled(installe, egui::Button::new("▶ Démarrer")).clicked() {
                    etat().lock().unwrap().message = match demarrer() {
                        Ok(()) => "Démarrage…".into(),
                        Err(e) => format!("✖ {e}"),
                    };
                }
            } else {
                ui.label(RichText::new(if pret { "● En ligne" } else { "● Démarrage…" }).color(if pret { Color32::LIGHT_GREEN } else { Color32::YELLOW }).strong());
                if ui.button("⏹ Arrêter").clicked() {
                    arreter();
                }
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            if ui.button("💾 Sauvegarder le monde").clicked() {
                etat().lock().unwrap().message = match sauvegarder() {
                    Ok(p) => format!("✔ {}", p.display()),
                    Err(e) => format!("✖ {e}"),
                };
            }
            if ui.button("📂 Ouvrir le dossier").clicked() {
                crate::outil::ouvrir(&self.r.dossier);
            }
        });
        ui.label(message);
        if let Some(ip) = crate::telephone::ip_locale() {
            ui.weak(format!("Tes amis sur le même réseau se connectent à {ip} (port 25565). Pour jouer avec des amis ailleurs, ouvre le port 25565 sur ta box."));
        }
        if marche {
            ui.label(format!("👥 {} joueur(s) : {}", joueurs.len(), joueurs.join(", ")));
            let lignes = etat().lock().unwrap().console.clone();
            egui::ScrollArea::vertical().max_height(300.0).stick_to_bottom(true).show(ui, |ui| {
                for l in lignes.iter().rev().take(300).rev() {
                    ui.monospace(l);
                }
            });
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.commande).hint_text("commande (ex. op Pseudo, say Bonjour)").desired_width(320.0));
                if (ui.button("Envoyer").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.commande.is_empty() {
                    commande(self.commande.trim_start_matches('/'));
                    self.commande.clear();
                }
            });
        }
    }
    fn resume(&self) -> Option<String> {
        statut().map(|(p, j)| if p { format!("En ligne · {} joueur(s)", j.len()) } else { "Démarrage…".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joueurs() {
        assert_eq!(evenement_joueur("[12:00:00] [Server thread/INFO]: Steve joined the game"), Some(("Steve".into(), true)));
        assert_eq!(evenement_joueur("[12:01:00] [Server thread/INFO]: Alex left the game"), Some(("Alex".into(), false)));
        assert_eq!(evenement_joueur("[12:00:00] [Server thread/INFO]: Done (3.2s)!"), None);
    }
}
