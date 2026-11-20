//! Minecraft, suite : plusieurs serveurs (25), carte web BlueMap (26), liste blanche et bannissements (27),
//! statistiques des joueurs (28), messages de bienvenue (29), carte des régions (31), ouverture des ports par
//! UPnP (32), test de NAT (33). (30 · sauvegarde avant mise à jour : dans l'installateur.)
use crate::minecraft::Pilote;
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        ui.weak("Aucun serveur configuré (onglet « Installer un serveur » ou « Serveur Minecraft »).");
    }
    d
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Outils").build()
}

// ---------------------------------------------------------------------------------------------------
// 25 · Plusieurs serveurs
// ---------------------------------------------------------------------------------------------------

pub struct Serveurs {
    mc: Pilote,
    liste: Vec<(String, String)>, // (nom, jar)
    message: String,
}

impl Serveurs {
    pub fn new(mc: Pilote) -> Self {
        Serveurs { mc, liste: charger("serveurs_mc"), message: String::new() }
    }
}

impl Outil for Serveurs {
    fn nom(&self) -> &'static str {
        "🗃 Mes serveurs"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mes serveurs");
        ui.weak("Garde plusieurs serveurs (survie, créatif, moddé…) et passe de l'un à l'autre. Chacun a son dossier, son monde et ses réglages.");
        let actuel = self.mc.jar();
        if !actuel.is_empty() && !self.liste.iter().any(|s| s.1 == actuel) {
            let nom = Path::new(&actuel).parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("Serveur".into());
            self.liste.push((nom, actuel.clone()));
            enregistrer("serveurs_mc", &self.liste);
        }
        let avant = self.liste.clone();
        let en_ligne = self.mc.statut().is_some();
        let mut retirer = None;
        for (i, (n, j)) in self.liste.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let courant = *j == actuel;
                ui.label(if courant { "▶" } else { "  " });
                ui.add(egui::TextEdit::singleline(n).desired_width(160.0));
                ui.weak(Path::new(j).parent().map(|p| p.display().to_string()).unwrap_or_default());
                if !courant {
                    if ui.add_enabled(!en_ligne, egui::Button::new("Utiliser")).on_disabled_hover_text("Arrête d'abord le serveur en cours").clicked() {
                        self.mc.definir_jar(Path::new(j));
                        self.message = format!("« {n} » est maintenant le serveur de l'onglet Serveur Minecraft.");
                    }
                    if ui.small_button("🗑").on_hover_text("Retirer de la liste (les fichiers restent)").clicked() {
                        retirer = Some(i);
                    }
                }
                if !Path::new(j).exists() {
                    ui.colored_label(Color32::LIGHT_RED, "introuvable");
                }
            });
        }
        if let Some(i) = retirer {
            self.liste.remove(i);
        }
        if ui.button("➕ Ajouter un serveur existant (.jar)…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Serveur", &["jar"]).pick_file() {
                let nom = p.parent().and_then(|x| x.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or("Serveur".into());
                self.liste.push((nom, p.display().to_string()));
            }
        }
        ui.weak("Pour en créer un nouveau : onglet « Installer un serveur » avec un autre dossier.");
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if self.liste != avant {
            enregistrer("serveurs_mc", &self.liste);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 26 · Carte web (BlueMap)
// ---------------------------------------------------------------------------------------------------

/// Active « accept-download » dans la config de BlueMap (sans ça, la carte ne se génère pas).
pub fn accepter_bluemap(t: &str) -> String {
    let re = regex::Regex::new(r"(?m)^(\s*accept-download\s*:\s*)false").unwrap();
    re.replace(t, "${1}true").into_owned()
}

pub struct CarteWeb {
    mc: Pilote,
    e: Etat,
}

impl CarteWeb {
    pub fn new(mc: Pilote) -> Self {
        CarteWeb { mc, e: Arc::default() }
    }
}

impl Outil for CarteWeb {
    fn nom(&self) -> &'static str {
        "🗺 Carte web du monde"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Carte web du monde (BlueMap)");
        ui.weak("Une carte 3D du monde, à ouvrir dans le navigateur, depuis le PC ou un téléphone sur le même Wi-Fi.");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let jar = self.mc.jar();
        let chargeur = crate::mcplus::chargeur_du_jar(&jar);
        let sous = if chargeur == "paper" { "plugins" } else { "mods" };
        if chargeur == "vanilla" || chargeur == "forge" {
            ui.weak("BlueMap s'installe ici sur un serveur Paper ou Fabric.");
            return;
        }
        let installe = std::fs::read_dir(d.join(sous)).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().contains("bluemap"));
        let occupe = etat_ui(ui, &self.e);
        if !installe {
            if ui.add_enabled(!occupe, egui::Button::new("📥 Installer BlueMap")).clicked() {
                let (c, v, dd) = (chargeur.to_string(), crate::mcplus::version_du_jar(&jar).unwrap_or_default(), d.join(sous));
                en_fond(&self.e, move || crate::mcplus::installer_projet("bluemap", &c, &v, &dd).map(|n| format!("✔ {n} installé : démarre le serveur une fois, puis reviens ici.")));
            }
            return;
        }
        let conf = [d.join("plugins/BlueMap/core.conf"), d.join("config/bluemap/core.conf")].into_iter().find(|p| p.exists());
        match conf {
            None => {
                ui.label("BlueMap est installé : démarre le serveur une fois pour qu'il crée sa configuration, puis reviens ici.");
            }
            Some(c) => {
                let t = std::fs::read_to_string(&c).unwrap_or_default();
                if t.contains("accept-download: false") || regex::Regex::new(r"accept-download\s*:\s*false").unwrap().is_match(&t) {
                    ui.label("BlueMap doit télécharger les textures de Minecraft (fichier officiel de Mojang).");
                    if ui.button("✔ Accepter et activer la carte").clicked() {
                        *self.e.lock().unwrap() = (false, std::fs::write(&c, accepter_bluemap(&t)).map(|_| "✔ Activé : redémarre le serveur.".to_string()).unwrap_or_else(|e| e.to_string()));
                    }
                } else {
                    let ip = crate::telephone::ip_locale().unwrap_or("localhost".into());
                    ui.colored_label(Color32::LIGHT_GREEN, "✔ Carte active (elle se remplit au fil des minutes quand le serveur tourne).");
                    for u in ["http://localhost:8100".to_string(), format!("http://{ip}:8100")] {
                        ui.horizontal(|ui| {
                            if ui.link(&u).clicked() {
                                let _ = std::process::Command::new("explorer").arg(&u).spawn();
                            }
                            if ui.small_button("📋").clicked() {
                                ui.ctx().copy_text(u.clone());
                            }
                        });
                    }
                    ui.weak("Le second lien marche depuis un téléphone ou un autre PC de la maison.");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 27 · Liste blanche et bannissements
// ---------------------------------------------------------------------------------------------------

fn uuid_de(nom: &str) -> Result<(String, String), String> {
    let j: serde_json::Value = agent().get(&format!("https://api.mojang.com/users/profiles/minecraft/{nom}")).call().map_err(|_| format!("pseudo « {nom} » introuvable"))?
        .into_json().map_err(|e| e.to_string())?;
    Ok((crate::mcplus::uuid_tirets(j["id"].as_str().ok_or("uuid ?")?), j["name"].as_str().unwrap_or(nom).to_string()))
}

fn lire_liste(f: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub struct ListeBlanche {
    mc: Pilote,
    nom: String,
    tetes: Arc<Mutex<HashMap<String, Option<image::RgbaImage>>>>,
    textures: HashMap<String, egui::TextureHandle>,
    e: Etat,
}

impl ListeBlanche {
    pub fn new(mc: Pilote) -> Self {
        ListeBlanche { mc, nom: String::new(), tetes: Arc::default(), textures: HashMap::new(), e: Arc::default() }
    }

    fn tete(&mut self, ui: &mut egui::Ui, nom: &str) {
        if let Some(t) = self.textures.get(nom) {
            ui.add(egui::Image::new(t).fit_to_exact_size(egui::vec2(24.0, 24.0)));
            return;
        }
        let mut x = self.tetes.lock().unwrap();
        match x.get(nom) {
            None => {
                x.insert(nom.to_string(), None);
                let (t, n) = (self.tetes.clone(), nom.to_string());
                std::thread::spawn(move || {
                    let img = agent().get(&format!("https://mc-heads.net/avatar/{n}/32")).call().ok().and_then(|r| {
                        let mut b = vec![];
                        std::io::Read::read_to_end(&mut r.into_reader(), &mut b).ok()?;
                        image::load_from_memory(&b).ok().map(|i| i.to_rgba8())
                    });
                    t.lock().unwrap().insert(n, img);
                });
            }
            Some(Some(img)) => {
                let tex = ui.ctx().load_texture(format!("tete_{nom}"), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::NEAREST);
                drop(x);
                ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(24.0, 24.0)));
                self.textures.insert(nom.to_string(), tex);
                return;
            }
            Some(None) => {}
        }
        ui.label("👤");
    }
}

impl Outil for ListeBlanche {
    fn nom(&self) -> &'static str {
        "📋 Liste blanche et bannis"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Liste blanche et bannissements");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let en_ligne = self.mc.statut().is_some();
        let blanche = lire_liste(&d.join("whitelist.json"));
        let bannis = lire_liste(&d.join("banned-players.json"));
        let active = crate::mcplus::propriete(&std::fs::read_to_string(d.join("server.properties")).unwrap_or_default(), "white-list").is_some_and(|v| v.trim() == "true");
        ui.horizontal(|ui| {
            ui.label(if active { "🔒 Liste blanche activée : seuls ces joueurs peuvent entrer." } else { "🔓 Liste blanche désactivée (tout le monde peut entrer)." });
            if en_ligne && ui.small_button(if active { "Désactiver" } else { "Activer" }).clicked() {
                self.mc.commande(if active { "whitelist off" } else { "whitelist on" });
            }
        });
        if !en_ligne {
            ui.weak("Serveur arrêté : les changements sont écrits dans les fichiers et pris en compte au démarrage. (Activer / désactiver la liste : onglet Réglages du serveur.)");
        }
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("pseudo").desired_width(160.0));
            for (titre, fichier, cmd) in [("➕ Autoriser", "whitelist.json", "whitelist add"), ("🚫 Bannir", "banned-players.json", "ban")] {
                if ui.add_enabled(!occupe && !self.nom.trim().is_empty(), egui::Button::new(titre)).clicked() {
                    let (n, mc, f) = (self.nom.trim().to_string(), self.mc.clone(), d.join(fichier));
                    self.nom.clear();
                    en_fond(&self.e, move || {
                        if en_ligne {
                            mc.commande(&format!("{cmd} {n}"));
                            return Ok(format!("✔ {n} : fait."));
                        }
                        let (id, nom) = uuid_de(&n)?;
                        let mut l = lire_liste(&f);
                        l.retain(|x| x["uuid"] != id.as_str());
                        let mut e = serde_json::json!({ "uuid": id, "name": nom });
                        if cmd == "ban" {
                            e["created"] = "2026-01-01 00:00:00 +0000".into();
                            e["source"] = "Outils".into();
                            e["expires"] = "forever".into();
                            e["reason"] = "Banni depuis Outils".into();
                        }
                        l.push(e);
                        std::fs::write(&f, serde_json::to_string_pretty(&l).unwrap()).map_err(|e| e.to_string())?;
                        Ok(format!("✔ {nom} : fait (au prochain démarrage)."))
                    });
                }
            }
        });
        ui.columns(2, |c| {
            for (k, (titre, liste, retrait)) in [("✅ Autorisés", &blanche, "whitelist remove"), ("🚫 Bannis", &bannis, "pardon")].into_iter().enumerate() {
                let ui = &mut c[k];
                ui.strong(format!("{titre} ({})", liste.len()));
                for j in liste {
                    let n = j["name"].as_str().unwrap_or("?").to_string();
                    ui.horizontal(|ui| {
                        self.tete(ui, &n);
                        ui.label(&n);
                        if ui.small_button("✖").clicked() {
                            if en_ligne {
                                self.mc.commande(&format!("{retrait} {n}"));
                            } else {
                                let f = d.join(if k == 0 { "whitelist.json" } else { "banned-players.json" });
                                let reste: Vec<&serde_json::Value> = liste.iter().filter(|x| x["name"] != n.as_str()).collect();
                                let _ = std::fs::write(f, serde_json::to_string_pretty(&reste).unwrap());
                            }
                        }
                    });
                }
            }
        });
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 28 · Statistiques des joueurs
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatJoueur {
    pub nom: String,
    pub minutes: u64,
    pub morts: u64,
    pub tues: u64,
    pub blocs: u64,
    pub km: f64,
}

/// Lit un fichier stats/<uuid>.json du monde.
pub fn lire_stats(j: &serde_json::Value) -> StatJoueur {
    let s = &j["stats"];
    let c = &s["minecraft:custom"];
    let n = |k: &str| c[k].as_u64().unwrap_or(0);
    let ticks = n("minecraft:play_time").max(n("minecraft:play_one_minute"));
    let cm: u64 = ["minecraft:walk_one_cm", "minecraft:sprint_one_cm", "minecraft:fly_one_cm", "minecraft:swim_one_cm", "minecraft:boat_one_cm", "minecraft:horse_one_cm"].iter().map(|k| n(k)).sum();
    StatJoueur {
        nom: String::new(),
        minutes: ticks / 20 / 60,
        morts: n("minecraft:deaths"),
        tues: n("minecraft:mob_kills"),
        blocs: s["minecraft:mined"].as_object().map(|o| o.values().filter_map(|v| v.as_u64()).sum()).unwrap_or(0),
        km: cm as f64 / 100_000.0,
    }
}

pub struct StatsJoueurs {
    mc: Pilote,
}

impl StatsJoueurs {
    pub fn new(mc: Pilote) -> Self {
        StatsJoueurs { mc }
    }
}

impl Outil for StatsJoueurs {
    fn nom(&self) -> &'static str {
        "🏅 Statistiques des joueurs"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Statistiques des joueurs");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let monde = d.join(crate::minecraft::nom_monde(&d));
        let cache: Vec<serde_json::Value> = std::fs::read_to_string(d.join("usercache.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let noms: HashMap<String, (String, String)> = cache.iter().filter_map(|u| Some((u["uuid"].as_str()?.to_string(), (u["name"].as_str()?.to_string(), u["expiresOn"].as_str().unwrap_or("").to_string())))).collect();
        let mut v: Vec<(StatJoueur, String)> = std::fs::read_dir(monde.join("stats")).into_iter().flatten().flatten().filter_map(|e| {
            let uuid = e.path().file_stem()?.to_string_lossy().into_owned();
            let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            let mut s = lire_stats(&j);
            let (n, exp) = noms.get(&uuid).cloned().unwrap_or((uuid[..8.min(uuid.len())].to_string(), String::new()));
            s.nom = n;
            Some((s, exp))
        }).collect();
        v.sort_by(|a, b| b.0.minutes.cmp(&a.0.minutes));
        if v.is_empty() {
            ui.weak("Pas encore de statistiques (elles s'écrivent quand les joueurs se déconnectent ou à l'arrêt du serveur).");
            return;
        }
        egui::Grid::new("stats_mc").num_columns(7).striped(true).spacing([18.0, 6.0]).show(ui, |ui| {
            for t in ["Joueur", "Temps de jeu", "Morts", "Monstres tués", "Blocs cassés", "Distance", "Dernière venue"] {
                ui.strong(t);
            }
            ui.end_row();
            for (s, exp) in &v {
                ui.label(RichText::new(&s.nom).strong());
                ui.label(format!("{} h {:02}", s.minutes / 60, s.minutes % 60));
                ui.label(s.morts.to_string());
                ui.label(s.tues.to_string());
                ui.label(s.blocs.to_string());
                ui.label(format!("{:.1} km", s.km));
                // usercache : expiration = dernière connexion + 1 mois
                ui.weak(exp.get(..10).map(|d| format!("≈ un mois avant le {}/{}/{}", &d[8..10], &d[5..7], &d[..4])).unwrap_or_default());
                ui.end_row();
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 29 · Messages de bienvenue
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Bienvenue {
    pub actif: bool,
    pub message: String,
    pub premiere_fois: String,
    pub connus: BTreeSet<String>,
}

impl Default for Bienvenue {
    fn default() -> Self {
        Bienvenue { actif: false, message: "Bon retour {joueur} !".into(), premiere_fois: "Bienvenue sur le serveur {joueur}, amuse-toi bien !".into(), connus: BTreeSet::new() }
    }
}

/// Pseudo dans une ligne « X joined the game ».
pub fn arrivee(l: &str) -> Option<String> {
    let re = regex::Regex::new(r"\]: (\w{2,16}) joined the game").unwrap();
    re.captures(l).map(|c| c[1].to_string())
}

/// Commande tellraw (texte rose, JSON échappé).
pub fn tellraw(texte: &str) -> String {
    format!("tellraw @a {}", serde_json::json!({ "text": texte, "color": "light_purple" }))
}

pub struct MessagesBienvenue {
    b: Arc<Mutex<Bienvenue>>,
}

impl MessagesBienvenue {
    pub fn new(mc: Pilote) -> Self {
        let b: Arc<Mutex<Bienvenue>> = Arc::new(Mutex::new(charger("bienvenue")));
        let b2 = b.clone();
        std::thread::spawn(move || {
            let mut vues: BTreeSet<String> = mc.dernieres().into_iter().collect();
            loop {
                std::thread::sleep(Duration::from_secs(2));
                if mc.statut().is_none() {
                    vues.clear();
                    continue;
                }
                let lignes = mc.dernieres();
                for l in &lignes {
                    if vues.insert(l.clone()) {
                        if let Some(j) = arrivee(l) {
                            let mut x = b2.lock().unwrap();
                            if !x.actif {
                                continue;
                            }
                            let nouveau = x.connus.insert(j.clone());
                            let modele = if nouveau && !x.premiere_fois.is_empty() { x.premiere_fois.clone() } else { x.message.clone() };
                            enregistrer("bienvenue", &*x);
                            drop(x);
                            if !modele.is_empty() {
                                std::thread::sleep(Duration::from_secs(1));
                                mc.commande(&tellraw(&modele.replace("{joueur}", &j)));
                            }
                        }
                    }
                }
                if vues.len() > 2000 {
                    vues = lignes.into_iter().collect();
                }
            }
        });
        MessagesBienvenue { b }
    }
}

impl Outil for MessagesBienvenue {
    fn nom(&self) -> &'static str {
        "👋 Messages de bienvenue"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Messages de bienvenue");
        let avant = self.b.lock().unwrap().clone();
        let mut b = avant.clone();
        ui.checkbox(&mut b.actif, "Accueillir les joueurs qui se connectent");
        ui.label("Première visite");
        ui.add(egui::TextEdit::singleline(&mut b.premiere_fois).desired_width(f32::INFINITY));
        ui.label("Les fois suivantes");
        ui.add(egui::TextEdit::singleline(&mut b.message).desired_width(f32::INFINITY));
        ui.weak("{joueur} est remplacé par le pseudo. Laisse vide pour ne rien dire.");
        ui.label(format!("{} joueur(s) déjà venus", b.connus.len()));
        if b != avant {
            enregistrer("bienvenue", &b);
            *self.b.lock().unwrap() = b;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 31 · Carte des régions
// ---------------------------------------------------------------------------------------------------

/// « r.-1.3.mca » → (-1, 3)
pub fn coord_region(n: &str) -> Option<(i32, i32)> {
    let p: Vec<&str> = n.strip_prefix("r.")?.strip_suffix(".mca")?.split('.').collect();
    Some((p.first()?.parse().ok()?, p.get(1)?.parse().ok()?))
}

pub struct Regions {
    mc: Pilote,
    dim: usize,
    choisies: BTreeSet<(i32, i32)>,
    e: Etat,
}

impl Regions {
    pub fn new(mc: Pilote) -> Self {
        Regions { mc, dim: 0, choisies: BTreeSet::new(), e: Arc::default() }
    }
}

impl Outil for Regions {
    fn nom(&self) -> &'static str {
        "🧩 Carte des régions"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Carte des régions du monde");
        ui.weak("Chaque case = une région de 512 × 512 blocs ; plus elle est rose, plus elle pèse. Supprimer une région la fait régénérer à neuf (utile pour les zones explorées une fois et jamais revues).");
        let Some(d) = sans_serveur(ui, &self.mc) else { return };
        let monde = d.join(crate::minecraft::nom_monde(&d));
        let dims = [("Surface", monde.join("region")), ("Nether", monde.join("DIM-1/region")), ("End", monde.join("DIM1/region"))];
        ui.horizontal(|ui| {
            for (k, (n, _)) in dims.iter().enumerate() {
                if ui.selectable_label(self.dim == k, *n).clicked() {
                    self.dim = k;
                    self.choisies.clear();
                }
            }
        });
        let dossier = &dims[self.dim].1;
        let regions: Vec<((i32, i32), u64)> = std::fs::read_dir(dossier).into_iter().flatten().flatten()
            .filter_map(|e| Some((coord_region(&e.file_name().to_string_lossy())?, e.metadata().ok()?.len()))).collect();
        if regions.is_empty() {
            ui.weak("Aucune région.");
            return;
        }
        let total: u64 = regions.iter().map(|r| r.1).sum();
        let max = regions.iter().map(|r| r.1).max().unwrap_or(1).max(1);
        let (x0, x1) = (regions.iter().map(|r| r.0 .0).min().unwrap(), regions.iter().map(|r| r.0 .0).max().unwrap());
        let (z0, z1) = (regions.iter().map(|r| r.0 .1).min().unwrap(), regions.iter().map(|r| r.0 .1).max().unwrap());
        ui.label(format!("{} région(s) · {}", regions.len(), crate::disque::taille_lisible(total)));
        let c = (520.0 / ((x1 - x0 + 1).max(z1 - z0 + 1) as f32)).clamp(6.0, 40.0);
        let (zone, _) = ui.allocate_exact_size(egui::vec2((x1 - x0 + 1) as f32 * c, (z1 - z0 + 1) as f32 * c), egui::Sense::hover());
        for ((x, z), t) in &regions {
            let r = egui::Rect::from_min_size(zone.min + egui::vec2((x - x0) as f32 * c, (z - z0) as f32 * c), egui::vec2(c, c)).shrink(0.5);
            let f = (*t as f32 / max as f32).sqrt();
            let col = Color32::from_rgb((60.0 + 195.0 * f) as u8, (60.0 + 35.0 * f) as u8, (90.0 + 72.0 * f) as u8);
            let rep = ui.interact(r, ui.id().with(("reg", x, z)), egui::Sense::click()).on_hover_text(format!("Région {x}, {z} (blocs {} à {}, {} à {}) · {}", x * 512, x * 512 + 511, z * 512, z * 512 + 511, crate::disque::taille_lisible(*t)));
            ui.painter().rect_filled(r, 1.0, col);
            if (*x, *z) == (0, 0) || (*x, *z) == (-1, -1) {
                ui.painter().circle_filled(r.center(), 2.0, Color32::WHITE); // près du centre du monde
            }
            if self.choisies.contains(&(*x, *z)) {
                ui.painter().rect_stroke(r, 1.0, egui::Stroke::new(2.0f32, Color32::YELLOW), egui::StrokeKind::Inside);
            }
            if rep.clicked() && !self.choisies.remove(&(*x, *z)) {
                self.choisies.insert((*x, *z));
            }
        }
        let occupe = etat_ui(ui, &self.e);
        let en_ligne = self.mc.statut().is_some();
        if !self.choisies.is_empty() {
            ui.horizontal(|ui| {
                ui.label(format!("{} région(s) choisie(s)", self.choisies.len()));
                if ui.add_enabled(!occupe && !en_ligne, egui::Button::new("🗑 Régénérer (supprimer)")).on_disabled_hover_text("Arrête d'abord le serveur").clicked() {
                    let (mc, d2, l) = (self.mc.clone(), dossier.clone(), std::mem::take(&mut self.choisies));
                    en_fond(&self.e, move || {
                        mc.sauvegarder()?; // filet de sécurité
                        for (x, z) in &l {
                            let _ = std::fs::remove_file(d2.join(format!("r.{x}.{z}.mca")));
                        }
                        Ok(format!("✔ {} région(s) supprimée(s) (monde sauvegardé avant).", l.len()))
                    });
                }
                if ui.small_button("Annuler la sélection").clicked() {
                    self.choisies.clear();
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 32 · UPnP / 33 · Test de NAT
// ---------------------------------------------------------------------------------------------------

/// Valeur de l'en-tête LOCATION d'une réponse SSDP.
pub fn location_ssdp(r: &str) -> Option<String> {
    r.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case("location").then(|| v.trim().to_string())
    })
}

/// (type de service, URL de contrôle) de la connexion WAN dans la description de la box.
pub fn service_wan(xml: &str, base: &str) -> Option<(String, String)> {
    let re = regex::Regex::new(r"(?s)<service>(.*?)</service>").unwrap();
    for c in re.captures_iter(xml) {
        let s = &c[1];
        let typ = regex::Regex::new(r"<serviceType>(.*?)</serviceType>").unwrap().captures(s)?[1].trim().to_string();
        if typ.contains("WANIPConnection") || typ.contains("WANPPPConnection") {
            let ctrl = regex::Regex::new(r"<controlURL>(.*?)</controlURL>").unwrap().captures(s)?[1].trim().to_string();
            let url = if ctrl.starts_with("http") { ctrl } else { format!("{}/{}", base.trim_end_matches('/'), ctrl.trim_start_matches('/')) };
            return Some((typ, url));
        }
    }
    None
}

fn trouver_box() -> Result<(String, String), String> {
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok();
    let req = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
    s.send_to(req.as_bytes(), "239.255.255.250:1900").map_err(|e| e.to_string())?;
    let mut b = [0u8; 2048];
    let (n, _) = s.recv_from(&mut b).map_err(|_| "aucune box ne répond en UPnP (option désactivée dans la box ?)".to_string())?;
    let loc = location_ssdp(&String::from_utf8_lossy(&b[..n])).ok_or("réponse UPnP sans adresse")?;
    let xml = agent().get(&loc).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    let base = loc.split('/').take(3).collect::<Vec<_>>().join("/");
    service_wan(&xml, &base).ok_or_else(|| "la box ne propose pas l'ouverture de ports".into())
}

fn soap(service: &(String, String), action: &str, args: &[(&str, String)]) -> Result<String, String> {
    let corps: String = args.iter().map(|(k, v)| format!("<{k}>{v}</{k}>")).collect();
    let xml = format!(r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{}">{corps}</u:{action}></s:Body></s:Envelope>"#, service.0);
    agent().post(&service.1).set("Content-Type", "text/xml; charset=\"utf-8\"").set("SOAPAction", &format!("\"{}#{action}\"", service.0)).send_string(&xml)
        .map_err(|e| match e {
            ureq::Error::Status(c, r) => format!("la box a refusé ({c}) : {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
            x => x.to_string(),
        })?.into_string().map_err(|e| e.to_string())
}

/// Requête STUN « binding » (RFC 5389).
pub fn requete_stun(id: [u8; 12]) -> Vec<u8> {
    let mut v = vec![0x00, 0x01, 0x00, 0x00, 0x21, 0x12, 0xA4, 0x42];
    v.extend(id);
    v
}

/// Adresse publique vue par le serveur STUN (XOR-MAPPED-ADDRESS, ou MAPPED-ADDRESS).
pub fn reponse_stun(b: &[u8]) -> Option<SocketAddr> {
    if b.len() < 20 || b[0..2] != [0x01, 0x01] {
        return None;
    }
    let mut i = 20;
    while i + 4 <= b.len() {
        let t = u16::from_be_bytes([b[i], b[i + 1]]);
        let l = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let v = b.get(i + 4..i + 4 + l)?;
        if (t == 0x0020 || t == 0x0001) && l >= 8 && v[1] == 0x01 {
            let mut port = u16::from_be_bytes([v[2], v[3]]);
            let mut ip = [v[4], v[5], v[6], v[7]];
            if t == 0x0020 {
                port ^= 0x2112;
                for (k, x) in ip.iter_mut().enumerate() {
                    *x ^= [0x21, 0x12, 0xA4, 0x42][k];
                }
            }
            return Some(SocketAddr::from((Ipv4Addr::from(ip), port)));
        }
        i += 4 + l.div_ceil(4) * 4;
    }
    None
}

/// Résultat du test : (type, explication).
fn tester_nat() -> Result<(String, String), String> {
    use std::net::ToSocketAddrs;
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let mut vues = vec![];
    for serveur in ["stun.l.google.com:19302", "stun1.l.google.com:19302", "stun.cloudflare.com:3478"] {
        let Some(a) = serveur.to_socket_addrs().ok().and_then(|mut x| x.find(|a| a.is_ipv4())) else { continue };
        let mut id = [0u8; 12];
        let _ = getrandom::getrandom(&mut id);
        for _ in 0..2 {
            let _ = s.send_to(&requete_stun(id), a);
            let mut b = [0u8; 512];
            if let Ok((n, _)) = s.recv_from(&mut b) {
                if let Some(x) = reponse_stun(&b[..n]) {
                    vues.push(x);
                    break;
                }
            }
        }
    }
    if vues.len() < 2 {
        return Err("les serveurs de test ne répondent pas (UDP bloqué ?)".into());
    }
    let local = crate::telephone::ip_locale().unwrap_or_default();
    Ok(if vues[0].ip().to_string() == local {
        ("Ouvert".into(), "Ton PC est directement sur Internet : aucun souci pour héberger ou rejoindre.".into())
    } else if vues.iter().all(|v| v.port() == vues[0].port()) {
        ("Modéré (ou ouvert)".into(), format!("Ta box garde le même port pour tout le monde : le jeu en ligne marche bien. Pour héberger (serveur Minecraft…), ouvre le port (UPnP ci-dessus) ou utilise playit. IP publique : {}", vues[0].ip()))
    } else {
        ("Strict".into(), "Ta box change de port selon la destination : certains jeux auront du mal à te connecter à d'autres joueurs. Active l'UPnP dans la box, ou essaie la « DMZ » / un port fixe.".into())
    })
}

pub struct Ports {
    mc: Pilote,
    e: Etat,
    nat: Arc<Mutex<Option<Result<(String, String), String>>>>,
}

impl Ports {
    pub fn new(mc: Pilote) -> Self {
        Ports { mc, e: Arc::default(), nat: Arc::default() }
    }
}

impl Outil for Ports {
    fn nom(&self) -> &'static str {
        "🔓 Ouvrir les ports (UPnP) et test NAT"
    }
    fn cat(&self) -> Cat {
        Cat::Minecraft
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ouvrir les ports et tester le NAT");
        let port = self.mc.dossier().map(|d| crate::minecraft::port(&d)).unwrap_or(25565);
        ui.strong(format!("🔓 Port du serveur Minecraft : {port}"));
        ui.weak("Demande à la box (UPnP) de laisser entrer les amis sur ce port, sans aller dans son interface. L'UPnP doit être activé dans la box (c'est souvent le cas).");
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            for (titre, ouvrir) in [("🔓 Ouvrir le port", true), ("🔒 Refermer", false)] {
                if ui.add_enabled(!occupe, egui::Button::new(titre)).clicked() {
                    en_fond(&self.e, move || {
                        let s = trouver_box()?;
                        let local = crate::telephone::ip_locale().ok_or("adresse locale inconnue")?;
                        for proto in ["TCP", "UDP"] {
                            if ouvrir {
                                soap(&s, "AddPortMapping", &[("NewRemoteHost", String::new()), ("NewExternalPort", port.to_string()), ("NewProtocol", proto.into()),
                                    ("NewInternalPort", port.to_string()), ("NewInternalClient", local.clone()), ("NewEnabled", "1".into()),
                                    ("NewPortMappingDescription", "Outils Minecraft".into()), ("NewLeaseDuration", "0".into())])?;
                            } else {
                                let _ = soap(&s, "DeletePortMapping", &[("NewRemoteHost", String::new()), ("NewExternalPort", port.to_string()), ("NewProtocol", proto.into())]);
                            }
                        }
                        let ip = soap(&s, "GetExternalIPAddress", &[]).ok().and_then(|x| regex::Regex::new(r"<NewExternalIPAddress>(.*?)</NewExternalIPAddress>").unwrap().captures(&x).map(|c| c[1].to_string())).unwrap_or_default();
                        Ok(if ouvrir { format!("✔ Port {port} ouvert vers ce PC. Adresse à donner aux amis : {ip}:{port}") } else { "✔ Port refermé.".into() })
                    });
                }
            }
        });
        ui.separator();
        ui.strong("🌐 Test de NAT");
        ui.weak("Dit si ta connexion est ouverte, modérée ou stricte pour le jeu en ligne.");
        if ui.button("Lancer le test").clicked() {
            let n = self.nat.clone();
            *n.lock().unwrap() = Some(Err("test en cours…".into()));
            std::thread::spawn(move || *n.lock().unwrap() = Some(tester_nat()));
        }
        match self.nat.lock().unwrap().clone() {
            Some(Ok((t, e))) => {
                let c = match t.as_str() {
                    "Strict" => Color32::LIGHT_RED,
                    "Ouvert" => Color32::LIGHT_GREEN,
                    _ => Color32::from_rgb(250, 210, 90),
                };
                ui.colored_label(c, RichText::new(format!("NAT : {t}")).size(22.0));
                ui.label(e);
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures() {
        assert_eq!(accepter_bluemap("# x\naccept-download: false\nrenderThreadCount: 1\n"), "# x\naccept-download: true\nrenderThreadCount: 1\n");
        let j: serde_json::Value = serde_json::from_str(r#"{"stats":{"minecraft:custom":{"minecraft:play_time":144000,"minecraft:deaths":3,"minecraft:walk_one_cm":250000},"minecraft:mined":{"minecraft:stone":40,"minecraft:dirt":2}}}"#).unwrap();
        let s = lire_stats(&j);
        assert_eq!((s.minutes, s.morts, s.blocs), (120, 3, 42));
        assert!((s.km - 2.5).abs() < 1e-9);
        assert_eq!(arrivee("[12:00:01] [Server thread/INFO]: Bob_42 joined the game"), Some("Bob_42".into()));
        assert_eq!(arrivee("[12:00:01] [Server thread/INFO]: <Bob> joined the game lol"), None);
        assert_eq!(tellraw("Salut \"Bob\""), r#"tellraw @a {"color":"light_purple","text":"Salut \"Bob\""}"#);
        assert_eq!(coord_region("r.-1.3.mca"), Some((-1, 3)));
        assert_eq!(coord_region("level.dat"), None);
    }

    #[test]
    fn upnp_et_stun() {
        assert_eq!(location_ssdp("HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.1:5000/desc.xml\r\n"), Some("http://192.168.1.1:5000/desc.xml".into()));
        let xml = "<root><service><serviceType>urn:schemas-upnp-org:service:Layer3:1</serviceType><controlURL>/l3</controlURL></service><service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>/ctl/IPConn</controlURL></service></root>";
        assert_eq!(service_wan(xml, "http://192.168.1.1:5000"), Some(("urn:schemas-upnp-org:service:WANIPConnection:1".into(), "http://192.168.1.1:5000/ctl/IPConn".into())));
        let id = [7u8; 12];
        assert_eq!(&requete_stun(id)[..4], &[0, 1, 0, 0]);
        // réponse avec XOR-MAPPED-ADDRESS 203.0.113.5:54321
        let ip = [203u8, 0, 113, 5];
        let port: u16 = 54321;
        let mut r = vec![0x01, 0x01, 0x00, 0x0C, 0x21, 0x12, 0xA4, 0x42];
        r.extend(id);
        r.extend([0x00, 0x20, 0x00, 0x08, 0x00, 0x01]);
        r.extend((port ^ 0x2112).to_be_bytes());
        r.extend(ip.iter().zip([0x21, 0x12, 0xA4, 0x42]).map(|(a, b)| a ^ b));
        assert_eq!(reponse_stun(&r), Some("203.0.113.5:54321".parse().unwrap()));
    }
}
