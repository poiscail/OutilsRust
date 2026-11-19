//! Minecraft, en plus de l'onglet Serveur : installateur (72), plugins et mods Modrinth (73), réglages du serveur
//! (74), opérateurs (75), restauration à une date (76), prégénération du monde (77), alertes de performance (78).
use crate::minecraft::Pilote;
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).user_agent("poiscail/OutilsRust (outil personnel)").build()
}

fn json(url: &str) -> Result<serde_json::Value, String> {
    agent().get(url).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())
}

fn telecharger(url: &str, dest: &Path) -> Result<(), String> {
    let mut r = agent().get(url).call().map_err(|e| e.to_string())?.into_reader();
    let tmp = dest.with_extension("part");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    std::io::copy(&mut r, &mut f).map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
}

type Etat = Arc<Mutex<(bool, String)>>;

fn en_fond(e: &Etat, f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    let e = e.clone();
    *e.lock().unwrap() = (true, "En cours…".into());
    std::thread::spawn(move || {
        let r = f();
        *e.lock().unwrap() = (false, r.unwrap_or_else(|m| format!("✖ {m}")));
    });
}

fn etat_ui(ui: &mut egui::Ui, e: &Etat) -> bool {
    let (o, m) = e.lock().unwrap().clone();
    if o {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(&m);
        });
        ui.ctx().request_repaint_after(Duration::from_millis(300));
    } else if !m.is_empty() {
        ui.label(m);
    }
    o
}

fn sans_serveur(ui: &mut egui::Ui, mc: &Pilote) -> Option<PathBuf> {
    let d = mc.dossier();
    if d.is_none() {
        ui.weak("Aucun serveur configuré : installe-en un (onglet « Installer un serveur ») ou choisis son .jar dans l'onglet Serveur Minecraft.");
    }
    d
}

/// Version de Minecraft devinée d'après le nom du .jar (« paper-1.21.4-123.jar » → 1.21.4).
pub fn version_du_jar(jar: &str) -> Option<String> {
    let re = regex::Regex::new(r"1\.\d+(?:\.\d+)?").unwrap();
    re.find(jar).map(|m| m.as_str().to_string())
}

/// Type de serveur d'après le nom du .jar.
pub fn chargeur_du_jar(jar: &str) -> &'static str {
    let j = jar.to_lowercase();
    if j.contains("fabric") {
        "fabric"
    } else if j.contains("forge") || j.contains("neoforge") {
        "forge"
    } else if j.contains("purpur") || j.contains("paper") || j.contains("spigot") {
        "paper"
    } else {
        "vanilla"
    }
}

// ---------------------------------------------------------------------------------------------------
// 72 · Installateur de serveur
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Genre {
    Paper,
    Fabric,
    Vanilla,
}

fn versions(g: Genre) -> Result<Vec<String>, String> {
    match g {
        Genre::Paper => {
            let j = json("https://api.papermc.io/v2/projects/paper")?;
            let mut v: Vec<String> = j["versions"].as_array().ok_or("réponse Paper inattendue")?.iter().filter_map(|x| x.as_str().map(String::from)).filter(|x| !x.contains('-')).collect();
            v.reverse();
            Ok(v)
        }
        Genre::Fabric => Ok(json("https://meta.fabricmc.net/v2/versions/game")?.as_array().ok_or("réponse Fabric inattendue")?.iter()
            .filter(|x| x["stable"] == true).filter_map(|x| x["version"].as_str().map(String::from)).collect()),
        Genre::Vanilla => Ok(json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?["versions"].as_array().ok_or("réponse Mojang inattendue")?.iter()
            .filter(|x| x["type"] == "release").filter_map(|x| x["id"].as_str().map(String::from)).collect()),
    }
}

fn installer(g: Genre, v: &str, dossier: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    let (url, nom) = match g {
        Genre::Paper => {
            let b = json(&format!("https://api.papermc.io/v2/projects/paper/versions/{v}/builds"))?;
            let dernier = b["builds"].as_array().and_then(|a| a.iter().rev().find(|x| x["channel"] == "default").or(a.last())).ok_or("aucune version Paper")?.clone();
            let n = dernier["build"].as_u64().ok_or("build ?")?;
            let f = dernier["downloads"]["application"]["name"].as_str().ok_or("fichier ?")?.to_string();
            (format!("https://api.papermc.io/v2/projects/paper/versions/{v}/builds/{n}/downloads/{f}"), f)
        }
        Genre::Fabric => {
            let stable = |u: &str| -> Result<String, String> {
                json(u)?.as_array().and_then(|a| a.iter().find(|x| x["stable"] == true).and_then(|x| x["version"].as_str().map(String::from))).ok_or_else(|| "version Fabric ?".into())
            };
            let (l, i) = (stable("https://meta.fabricmc.net/v2/versions/loader")?, stable("https://meta.fabricmc.net/v2/versions/installer")?);
            (format!("https://meta.fabricmc.net/v2/versions/loader/{v}/{l}/{i}/server/jar"), format!("fabric-server-{v}.jar"))
        }
        Genre::Vanilla => {
            let m = json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")?;
            let u = m["versions"].as_array().and_then(|a| a.iter().find(|x| x["id"] == v)).and_then(|x| x["url"].as_str()).ok_or("version introuvable")?.to_string();
            let d = json(&u)?;
            (d["downloads"]["server"]["url"].as_str().ok_or("pas de serveur pour cette version")?.to_string(), format!("minecraft-server-{v}.jar"))
        }
    };
    let jar = dossier.join(nom);
    telecharger(&url, &jar)?;
    std::fs::write(dossier.join("eula.txt"), "# Acceptée dans Outils (https://aka.ms/MinecraftEULA)\neula=true\n").map_err(|e| e.to_string())?;
    Ok(jar)
}

pub struct Installateur {
    mc: Pilote,
    genre: Genre,
    liste: Arc<Mutex<Option<Result<Vec<String>, String>>>>,
    version: String,
    dossier: PathBuf,
    eula: bool,
    e: Etat,
    jar: Arc<Mutex<Option<PathBuf>>>,
}

impl Installateur {
    pub fn new(mc: Pilote) -> Self {
        let dossier = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Serveur Minecraft");
        Installateur { mc, genre: Genre::Paper, liste: Arc::default(), version: String::new(), dossier, eula: false, e: Arc::default(), jar: Arc::default() }
    }

    fn charger(&mut self) {
        let (l, g) = (self.liste.clone(), self.genre);
        *l.lock().unwrap() = Some(Err("chargement des versions…".into()));
        self.version.clear();
        std::thread::spawn(move || *l.lock().unwrap() = Some(versions(g)));
    }
}

impl Outil for Installateur {
    fn nom(&self) -> &'static str {
        "📥 Installer un serveur"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Installer un serveur Minecraft");
        if let Some(j) = self.jar.lock().unwrap().take() {
            self.mc.definir_jar(&j); // l'onglet Serveur Minecraft le prend tout de suite
        }
        ui.horizontal(|ui| {
            for (g, n, d) in [(Genre::Paper, "Paper", "plugins, le plus performant"), (Genre::Fabric, "Fabric", "mods"), (Genre::Vanilla, "Vanilla", "officiel, sans rien")] {
                if ui.selectable_label(self.genre == g, n).on_hover_text(d).clicked() && self.genre != g {
                    self.genre = g;
                    self.charger();
                }
            }
        });
        if self.liste.lock().unwrap().is_none() && self.version.is_empty() {
            let l = self.liste.clone();
            let g = self.genre;
            *l.lock().unwrap() = Some(Err("chargement des versions…".into()));
            std::thread::spawn(move || *l.lock().unwrap() = Some(versions(g)));
        }
        let liste = self.liste.lock().unwrap().clone();
        match liste {
            Some(Ok(v)) => {
                if self.version.is_empty() {
                    self.version = v.first().cloned().unwrap_or_default();
                }
                egui::ComboBox::from_label("version").selected_text(&self.version).height(300.0).show_ui(ui, |ui| {
                    for x in v.iter().take(60) {
                        ui.selectable_value(&mut self.version, x.clone(), x);
                    }
                });
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(400));
            }
            None => {
                ui.spinner();
            }
        }
        ui.horizontal(|ui| {
            ui.label("Dossier");
            ui.monospace(self.dossier.display().to_string());
            if ui.button("Changer…").clicked() {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    self.dossier = d;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.eula, "J'accepte le contrat de licence de Minecraft (EULA)");
            ui.hyperlink_to("le lire", "https://aka.ms/MinecraftEULA");
        });
        let occupe = etat_ui(ui, &self.e);
        if ui.add_enabled(!occupe && self.eula && !self.version.is_empty(), egui::Button::new(RichText::new("📥 Installer").size(16.0))).clicked() {
            let (g, v, d, j) = (self.genre, self.version.clone(), self.dossier.clone(), self.jar.clone());
            en_fond(&self.e, move || {
                let jar = installer(g, &v, &d)?;
                *j.lock().unwrap() = Some(jar.clone());
                Ok(format!("✔ Installé : {} · réglé dans l'onglet Serveur Minecraft, prêt à démarrer.", jar.display()))
            });
        }
        ui.weak("Java 21 est nécessaire pour les versions récentes (winget install EclipseAdoptium.Temurin.21.JRE).");
    }
}

// ---------------------------------------------------------------------------------------------------
// 73 · Plugins et mods (Modrinth)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Projet {
    pub id: String,
    pub titre: String,
    pub description: String,
    pub telechargements: u64,
}

pub struct Plugins {
    mc: Pilote,
    recherche: String,
    res: Arc<Mutex<Option<Result<Vec<Projet>, String>>>>,
    e: Etat,
}

impl Plugins {
    pub fn new(mc: Pilote) -> Self {
        Plugins { mc, recherche: String::new(), res: Arc::default(), e: Arc::default() }
    }
}

/// Télécharge la dernière version compatible d'un projet Modrinth dans `dossier`.
pub fn installer_projet(id: &str, chargeur: &str, version: &str, dossier: &Path) -> Result<String, String> {
    let v: serde_json::Value = agent().get(&format!("https://api.modrinth.com/v2/project/{id}/version"))
        .query("loaders", &format!("[\"{chargeur}\"]")).query("game_versions", &format!("[\"{version}\"]"))
        .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let f = v.as_array().and_then(|a| a.first()).and_then(|x| x["files"].as_array()?.iter().find(|f| f["primary"] == true).or(x["files"].as_array()?.first()).cloned())
        .ok_or(format!("pas de version pour {chargeur} {version}"))?;
    let (u, n) = (f["url"].as_str().ok_or("lien ?")?, f["filename"].as_str().ok_or("nom ?")?);
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    telecharger(u, &dossier.join(n))?;
    Ok(n.to_string())
}

impl Outil for Plugins {
    fn nom(&self) -> &'static str {
        "🧩 Plugins et mods du serveur"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Plugins et mods du serveur (Modrinth)");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let jar = self.mc.jar();
        let chargeur = chargeur_du_jar(&jar);
        let version = version_du_jar(&jar).unwrap_or_default();
        let (sous, genre) = if chargeur == "paper" { ("plugins", "plugin") } else { ("mods", "mod") };
        ui.weak(format!("Serveur {chargeur} {version} · dossier « {sous} »"));
        if chargeur == "vanilla" {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "Un serveur Vanilla n'accepte ni plugins ni mods : installe Paper ou Fabric.");
            return;
        }
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("chercher (ex. essentials, chunky, worldedit)").desired_width(320.0));
            if ui.button("🔍").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let (q, res, c, v) = (self.recherche.clone(), self.res.clone(), chargeur.to_string(), version.clone());
                *res.lock().unwrap() = None;
                std::thread::spawn(move || {
                    let facettes = format!("[[\"project_type:{genre}\"],[\"categories:{c}\"],[\"versions:{v}\"]]");
                    let r = agent().get("https://api.modrinth.com/v2/search").query("query", &q).query("facets", &facettes).query("limit", "20").call()
                        .map_err(|e| e.to_string()).and_then(|r| r.into_json::<serde_json::Value>().map_err(|e| e.to_string())).map(|j| {
                            j["hits"].as_array().into_iter().flatten().filter_map(|h| Some(Projet {
                                id: h["project_id"].as_str()?.into(), titre: h["title"].as_str()?.into(), description: h["description"].as_str().unwrap_or("").into(),
                                telechargements: h["downloads"].as_u64().unwrap_or(0),
                            })).collect()
                        });
                    *res.lock().unwrap() = Some(r);
                });
            }
        });
        let occupe = etat_ui(ui, &self.e);
        let res = self.res.lock().unwrap().clone();
        if let Some(r) = res {
            match r {
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
                Ok(l) => {
                    egui::ScrollArea::vertical().id_salt("modrinth").max_height(260.0).show(ui, |ui| {
                        for p in l {
                            ui.horizontal_wrapped(|ui| {
                                if ui.add_enabled(!occupe, egui::Button::new("➕")).clicked() {
                                    let (id, c, v, dd) = (p.id.clone(), chargeur.to_string(), version.clone(), d.join(sous));
                                    en_fond(&self.e, move || installer_projet(&id, &c, &v, &dd).map(|n| format!("✔ {n} installé (redémarre le serveur)")));
                                }
                                ui.strong(&p.titre);
                                ui.weak(format!("{} · {} téléchargements", p.description, p.telechargements));
                            });
                        }
                    });
                }
            }
        }
        ui.separator();
        ui.strong(format!("Installés ({sous})"));
        let mut liste: Vec<PathBuf> = std::fs::read_dir(d.join(sous)).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "jar")).collect();
        liste.sort();
        for p in liste {
            ui.horizontal(|ui| {
                if ui.small_button("🗑").clicked() {
                    let _ = crate::disque::corbeille(&p);
                }
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 74 · server.properties
// ---------------------------------------------------------------------------------------------------

/// Valeur d'une clé de server.properties.
pub fn propriete(t: &str, cle: &str) -> Option<String> {
    t.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (!l.trim_start().starts_with('#') && k.trim() == cle).then(|| v.to_string())
    })
}

/// Remplace (ou ajoute) une clé en gardant le reste du fichier tel quel.
pub fn avec_propriete(t: &str, cle: &str, valeur: &str) -> String {
    let mut trouve = false;
    let mut l: Vec<String> = t.lines().map(|l| {
        if !l.trim_start().starts_with('#') && l.split_once('=').is_some_and(|(k, _)| k.trim() == cle) {
            trouve = true;
            format!("{cle}={valeur}")
        } else {
            l.to_string()
        }
    }).collect();
    if !trouve {
        l.push(format!("{cle}={valeur}"));
    }
    l.join("\n") + "\n"
}

enum Champ {
    Texte,
    Nombre(i64, i64),
    Oui,
    Choix(&'static [(&'static str, &'static str)]),
}

const CHAMPS: &[(&str, &str, Champ)] = &[
    ("motd", "Message de la liste des serveurs", Champ::Texte),
    ("max-players", "Joueurs maximum", Champ::Nombre(1, 500)),
    ("difficulty", "Difficulté", Champ::Choix(&[("peaceful", "paisible"), ("easy", "facile"), ("normal", "normale"), ("hard", "difficile")])),
    ("gamemode", "Mode de jeu", Champ::Choix(&[("survival", "survie"), ("creative", "créatif"), ("adventure", "aventure"), ("spectator", "spectateur")])),
    ("pvp", "Combat entre joueurs", Champ::Oui),
    ("hardcore", "Hardcore", Champ::Oui),
    ("white-list", "Liste blanche (seuls les joueurs autorisés)", Champ::Oui),
    ("online-mode", "Vérifier les comptes Minecraft (désactiver = comptes « crackés » acceptés)", Champ::Oui),
    ("allow-flight", "Autoriser le vol (évite les expulsions avec certains mods)", Champ::Oui),
    ("view-distance", "Distance d'affichage (chunks)", Champ::Nombre(3, 32)),
    ("simulation-distance", "Distance de simulation (chunks)", Champ::Nombre(3, 32)),
    ("spawn-protection", "Protection du spawn (blocs)", Champ::Nombre(0, 64)),
    ("server-port", "Port", Champ::Nombre(1024, 65535)),
    ("level-seed", "Graine du monde (avant la création)", Champ::Texte),
];

pub struct Proprietes {
    mc: Pilote,
    texte: Option<(PathBuf, String)>,
    message: String,
}

impl Proprietes {
    pub fn new(mc: Pilote) -> Self {
        Proprietes { mc, texte: None, message: String::new() }
    }
}

impl Outil for Proprietes {
    fn nom(&self) -> &'static str {
        "⚙ Réglages du serveur"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Réglages du serveur (server.properties)");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let f = d.join("server.properties");
        if self.texte.as_ref().is_none_or(|(p, _)| *p != f) {
            self.texte = Some((f.clone(), std::fs::read_to_string(&f).unwrap_or_default()));
        }
        let (_, mut t) = self.texte.clone().unwrap();
        if t.is_empty() {
            ui.weak("Le fichier sera créé au premier démarrage du serveur ; tu peux déjà régler ici.");
        }
        let avant = t.clone();
        egui::Grid::new("props").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            for (cle, nom, champ) in CHAMPS {
                ui.label(*nom);
                let v = propriete(&t, cle).unwrap_or_default();
                let mut nouveau = v.clone();
                match champ {
                    Champ::Texte => {
                        ui.add(egui::TextEdit::singleline(&mut nouveau).desired_width(300.0));
                    }
                    Champ::Nombre(a, b) => {
                        let mut n: i64 = v.trim().parse().unwrap_or(*a);
                        if ui.add(egui::DragValue::new(&mut n).range(*a..=*b)).changed() {
                            nouveau = n.to_string();
                        }
                    }
                    Champ::Oui => {
                        let mut b = v.trim() == "true";
                        if ui.checkbox(&mut b, "").changed() {
                            nouveau = b.to_string();
                        }
                    }
                    Champ::Choix(c) => {
                        egui::ComboBox::from_id_salt(cle).selected_text(c.iter().find(|x| x.0 == v.trim()).map(|x| x.1).unwrap_or(v.as_str()).to_string()).show_ui(ui, |ui| {
                            for (k, n) in *c {
                                if ui.selectable_label(v.trim() == *k, *n).clicked() {
                                    nouveau = k.to_string();
                                }
                            }
                        });
                    }
                }
                if nouveau != v {
                    t = avec_propriete(&t, cle, &nouveau);
                }
                ui.end_row();
            }
        });
        if t != avant {
            self.message = match std::fs::write(&f, &t) {
                Ok(()) => "Enregistré : redémarre le serveur pour appliquer.".into(),
                Err(e) => e.to_string(),
            };
            self.texte = Some((f, t));
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 75 · Opérateurs
// ---------------------------------------------------------------------------------------------------

pub fn uuid_tirets(s: &str) -> String {
    if s.len() == 32 { format!("{}-{}-{}-{}-{}", &s[..8], &s[8..12], &s[12..16], &s[16..20], &s[20..]) } else { s.to_string() }
}

pub struct Operateurs {
    mc: Pilote,
    nom: String,
    e: Etat,
}

impl Operateurs {
    pub fn new(mc: Pilote) -> Self {
        Operateurs { mc, nom: String::new(), e: Arc::default() }
    }
}

impl Outil for Operateurs {
    fn nom(&self) -> &'static str {
        "👑 Opérateurs (admins)"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Opérateurs du serveur");
        ui.weak("Les opérateurs peuvent utiliser les commandes (gamemode, tp, kick…).");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let f = d.join("ops.json");
        let ops: Vec<serde_json::Value> = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let en_ligne = self.mc.statut().is_some();
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("pseudo Minecraft").desired_width(180.0));
            if ui.add_enabled(!occupe && !self.nom.trim().is_empty(), egui::Button::new("👑 Donner les droits")).clicked() {
                let (n, mc, f) = (self.nom.trim().to_string(), self.mc.clone(), f.clone());
                self.nom.clear();
                en_fond(&self.e, move || {
                    if en_ligne {
                        mc.commande(&format!("op {n}"));
                        return Ok(format!("✔ {n} est opérateur."));
                    }
                    let j = json(&format!("https://api.mojang.com/users/profiles/minecraft/{n}")).map_err(|_| format!("pseudo « {n} » introuvable"))?;
                    let (id, nom) = (uuid_tirets(j["id"].as_str().ok_or("uuid ?")?), j["name"].as_str().unwrap_or(&n).to_string());
                    let mut ops: Vec<serde_json::Value> = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                    ops.retain(|o| o["uuid"] != id.as_str());
                    ops.push(serde_json::json!({ "uuid": id, "name": nom, "level": 4, "bypassesPlayerLimit": false }));
                    std::fs::write(&f, serde_json::to_string_pretty(&ops).unwrap()).map_err(|e| e.to_string())?;
                    Ok(format!("✔ {nom} est opérateur (pris en compte au prochain démarrage)."))
                });
            }
        });
        for o in &ops {
            let n = o["name"].as_str().unwrap_or("?").to_string();
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("👑 {n}")).strong());
                ui.weak(format!("niveau {}", o["level"].as_u64().unwrap_or(4)));
                if ui.small_button("Retirer").clicked() {
                    if en_ligne {
                        self.mc.commande(&format!("deop {n}"));
                    } else {
                        let reste: Vec<&serde_json::Value> = ops.iter().filter(|x| x["name"] != n.as_str()).collect();
                        let _ = std::fs::write(&f, serde_json::to_string_pretty(&reste).unwrap());
                    }
                }
            });
        }
        if ops.is_empty() {
            ui.weak("Aucun opérateur.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 76 · Restauration à une date
// ---------------------------------------------------------------------------------------------------

/// « monde_20261118_193005.zip » → (« 18/11/2026 », « 19:30 »).
pub fn date_sauvegarde(nom: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"_(\d{4})(\d{2})(\d{2})_(\d{2})(\d{2})\d{2}\.zip$").unwrap();
    let c = re.captures(nom)?;
    Some((format!("{}/{}/{}", &c[3], &c[2], &c[1]), format!("{}:{}", &c[4], &c[5])))
}

pub struct Restauration {
    mc: Pilote,
    confirmer: Option<PathBuf>,
    e: Etat,
}

impl Restauration {
    pub fn new(mc: Pilote) -> Self {
        Restauration { mc, confirmer: None, e: Arc::default() }
    }
}

impl Outil for Restauration {
    fn nom(&self) -> &'static str {
        "⏪ Restaurer le monde"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Restaurer le monde à une date");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let occupe = etat_ui(ui, &self.e);
        let mut zips: Vec<PathBuf> = std::fs::read_dir(d.join("sauvegardes")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "zip")).collect();
        zips.sort();
        zips.reverse();
        if zips.is_empty() {
            ui.weak("Aucune sauvegarde : lance-en une depuis l'onglet Serveur Minecraft ou le Planificateur.");
            return;
        }
        let mut jour_prec = String::new();
        egui::ScrollArea::vertical().id_salt("restau").show(ui, |ui| {
            for z in &zips {
                let n = z.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let (j, h) = date_sauvegarde(&n).unwrap_or((String::new(), n.clone()));
                if j != jour_prec {
                    ui.label(RichText::new(format!("📅 {j}")).strong());
                    jour_prec = j;
                }
                ui.horizontal(|ui| {
                    ui.label(format!("   {h}"));
                    ui.weak(crate::disque::taille_lisible(z.metadata().map(|m| m.len()).unwrap_or(0)));
                    if ui.add_enabled(!occupe, egui::Button::new("⏪ Revenir à ce moment").small()).clicked() {
                        self.confirmer = Some(z.clone());
                    }
                });
            }
        });
        if let Some(z) = self.confirmer.clone() {
            egui::Window::new("Restaurer ?").collapsible(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
                ui.label(format!("Remplacer le monde actuel par {} ?", z.file_name().unwrap_or_default().to_string_lossy()));
                ui.weak("Le serveur est arrêté, le monde actuel est d'abord sauvegardé, puis la sauvegarde choisie le remplace.");
                ui.horizontal(|ui| {
                    if ui.button("⏪ Restaurer").clicked() {
                        let (mc, d) = (self.mc.clone(), d.clone());
                        en_fond(&self.e, move || {
                            if mc.statut().is_some() {
                                mc.arreter();
                            }
                            mc.sauvegarder()?;
                            let mut a = zip::ZipArchive::new(std::fs::File::open(&z).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                            let monde = a.file_names().next().and_then(|n| n.split('/').next()).ok_or("sauvegarde vide")?.to_string();
                            let _ = std::fs::remove_dir_all(d.join(&monde));
                            a.extract(&d).map_err(|e| e.to_string())?;
                            Ok(format!("✔ Monde « {monde} » restauré. Tu peux redémarrer le serveur."))
                        });
                        self.confirmer = None;
                    }
                    if ui.button("Annuler").clicked() {
                        self.confirmer = None;
                    }
                });
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 77 · Prégénération et 78 · Alertes de performance
// ---------------------------------------------------------------------------------------------------

/// TPS (1 min) dans une réponse de la commande « tps » de Paper.
pub fn lire_tps(l: &str) -> Option<f32> {
    let i = l.find("TPS from last")?;
    let apres = l[i..].split_once(':')?.1;
    let premier = apres.split(',').next()?;
    let chiffres: String = premier.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    chiffres.trim_start_matches('*').parse().ok()
}

/// Avancement de Chunky : « Processed: 1234 chunks (12.34%) ».
pub fn lire_chunky(l: &str) -> Option<f32> {
    let i = l.find("Processed:")?;
    let p = l[i..].split('(').nth(1)?.split('%').next()?;
    p.trim().parse().ok()
}

fn memoire(pid: u32) -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut m: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let ok = K32GetProcessMemoryInfo(h, &mut m, std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32) != 0;
        windows_sys::Win32::Foundation::CloseHandle(h);
        ok.then_some(m.WorkingSetSize as u64)
    }
}

pub struct Performance {
    mc: Pilote,
    tps: Arc<Mutex<(Option<f32>, Option<u64>)>>,
    rayon: u32,
    e: Etat,
}

impl Performance {
    pub fn new(mc: Pilote) -> Self {
        let tps: Arc<Mutex<(Option<f32>, Option<u64>)>> = Arc::default();
        let (m2, t2) = (mc.clone(), tps.clone());
        std::thread::spawn(move || {
            let mut prevenu = Instant::now() - Duration::from_secs(3600);
            loop {
                std::thread::sleep(Duration::from_secs(180)); // pas trop souvent : chaque mesure écrit une ligne dans la console
                if m2.statut().is_none_or(|s| !s.0) {
                    *t2.lock().unwrap() = (None, None);
                    continue;
                }
                if chargeur_du_jar(&m2.jar()) == "paper" {
                    m2.commande("tps");
                    std::thread::sleep(Duration::from_secs(2));
                }
                let tps = m2.dernieres().iter().rev().find_map(|l| lire_tps(l));
                let ram = m2.pid().and_then(memoire);
                *t2.lock().unwrap() = (tps, ram);
                let max = m2.ram() as u64 * 1024 * 1024;
                let mut pb = vec![];
                if let Some(t) = tps.filter(|t| *t < 15.0) {
                    pb.push(format!("TPS bas ({t:.1} / 20) : le serveur rame"));
                }
                if let Some(r) = ram.filter(|r| *r > max + max / 10) {
                    pb.push(format!("mémoire {} pour {} prévus", crate::disque::taille_lisible(r), crate::disque::taille_lisible(max)));
                }
                if !pb.is_empty() && prevenu.elapsed() > Duration::from_secs(900) {
                    crate::systeme::notifier("Serveur Minecraft", &pb.join(" · "));
                    prevenu = Instant::now();
                }
            }
        });
        Performance { mc, tps, rayon: 3000, e: Arc::default() }
    }
}

impl Outil for Performance {
    fn nom(&self) -> &'static str {
        "📈 Performance et prégénération"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Performance du serveur");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let jar = self.mc.jar();
        let (tps, ram) = *self.tps.lock().unwrap();
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        match self.mc.statut() {
            None => {
                ui.weak("Serveur arrêté.");
            }
            Some(_) => {
                ui.horizontal(|ui| {
                    match tps {
                        Some(t) => ui.label(RichText::new(format!("TPS {t:.1} / 20")).size(22.0).color(if t >= 18.0 { Color32::LIGHT_GREEN } else if t >= 15.0 { Color32::YELLOW } else { Color32::LIGHT_RED })),
                        None => ui.weak(if chargeur_du_jar(&jar) == "paper" { "TPS : mesure toutes les 3 minutes…" } else { "TPS : disponible avec Paper" }),
                    };
                    if let Some(r) = ram {
                        ui.label(RichText::new(format!("Mémoire {} / {} Mo prévus", crate::disque::taille_lisible(r), self.mc.ram())).size(18.0));
                    }
                });
            }
        }
        ui.weak("Alerte (notification) si les TPS passent sous 15 ou si la mémoire déborde. 20 TPS = le serveur tourne à pleine vitesse.");
        ui.separator();
        ui.strong("🗺 Prégénérer le monde (Chunky)");
        ui.weak("Génère les chunks à l'avance autour du spawn : plus de ralentissements quand les joueurs explorent. Long : à lancer quand personne ne joue.");
        let chargeur = chargeur_du_jar(&jar);
        let version = version_du_jar(&jar).unwrap_or_default();
        let sous = if chargeur == "paper" { "plugins" } else { "mods" };
        let installe = std::fs::read_dir(d.join(sous)).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().contains("chunky"));
        let occupe = etat_ui(ui, &self.e);
        if chargeur == "vanilla" || chargeur == "forge" {
            ui.weak("Chunky s'installe ici sur Paper ou Fabric.");
            return;
        }
        if !installe {
            if ui.add_enabled(!occupe, egui::Button::new("📥 Installer Chunky")).clicked() {
                let (c, v, dd) = (chargeur.to_string(), version, d.join(sous));
                en_fond(&self.e, move || installer_projet("chunky", &c, &v, &dd).map(|n| format!("✔ {n} installé : redémarre le serveur.")));
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Rayon");
            ui.add(egui::DragValue::new(&mut self.rayon).range(500..=20000).suffix(" blocs"));
            let ok = self.mc.statut().is_some_and(|s| s.0);
            if ui.add_enabled(ok, egui::Button::new("▶ Lancer")).clicked() {
                self.mc.commande(&format!("chunky radius {}", self.rayon));
                self.mc.commande("chunky start");
            }
            if ui.add_enabled(ok, egui::Button::new("⏸ Pause")).clicked() {
                self.mc.commande("chunky pause");
            }
            if ui.add_enabled(ok, egui::Button::new("▶ Reprendre")).clicked() {
                self.mc.commande("chunky continue");
            }
        });
        if let Some(p) = self.mc.dernieres().iter().rev().find_map(|l| lire_chunky(l)) {
            ui.add(egui::ProgressBar::new(p / 100.0).text(format!("{p:.1} %")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jars() {
        assert_eq!(version_du_jar("paper-1.21.4-123.jar"), Some("1.21.4".into()));
        assert_eq!(version_du_jar("fabric-server-1.20.jar"), Some("1.20".into()));
        assert_eq!(chargeur_du_jar(r"C:\srv\fabric-server-1.21.jar"), "fabric");
        assert_eq!(chargeur_du_jar("minecraft-server-1.21.jar"), "vanilla");
    }

    #[test]
    fn proprietes() {
        let t = "#Minecraft\nmotd=Salut\nmax-players=10\n";
        assert_eq!(propriete(t, "max-players").as_deref(), Some("10"));
        let n = avec_propriete(t, "max-players", "20");
        assert!(n.contains("max-players=20") && n.starts_with("#Minecraft\nmotd=Salut"));
        assert!(avec_propriete(t, "pvp", "false").ends_with("pvp=false\n"));
    }

    #[test]
    fn lectures() {
        assert_eq!(lire_tps("[12:00:00 INFO]: TPS from last 1m, 5m, 15m: *20.0, 19.97, 19.5"), Some(20.0));
        assert_eq!(lire_tps("[INFO]: TPS from last 1m, 5m, 15m: 14.2, 15.0, 16.0"), Some(14.2));
        assert_eq!(lire_chunky("[Chunky] Task running for minecraft:overworld. Processed: 1234 chunks (12.34%), ETA: 0:10:00"), Some(12.34));
        assert_eq!(uuid_tirets("069a79f444e94726a5befca90e38aaf5"), "069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(date_sauvegarde("monde_20261118_193005.zip"), Some(("18/11/2026".into(), "19:30".into())));
    }
}
