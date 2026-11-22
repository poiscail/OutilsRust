//! Fichiers du téléphone : espace occupé, doublons, rangement, renommage en lot, fichiers et applis
//! louches, corbeille de sécurité (rien n'est supprimé pour de bon sans le demander).
use crate::commun::*;
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub fn racine() -> PathBuf {
    PathBuf::from(crate::android::STOCKAGE)
}

#[derive(Clone, Debug)]
pub struct Fichier {
    pub chemin: PathBuf,
    pub taille: u64,
    pub date: u64,
}

/// Tous les fichiers sous `d` (sans Android/data, inaccessibles, ni la corbeille d'Outils).
pub fn lister_tout(d: &Path, max: usize) -> Vec<Fichier> {
    let mut v = vec![];
    let mut pile = vec![d.to_path_buf()];
    while let Some(x) = pile.pop() {
        let Ok(rd) = std::fs::read_dir(&x) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let n = e.file_name().to_string_lossy().to_string();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if n == ".Outils-corbeille" || (n == "Android" && x == d) {
                    continue;
                }
                pile.push(p);
            } else if m.is_file() {
                let date = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |t| t.as_secs());
                v.push(Fichier { chemin: p, taille: m.len(), date });
                if v.len() >= max {
                    return v;
                }
            }
        }
    }
    v
}

// ---------------------------------------------------------------------------------------------------
// Navigateur (choisir des fichiers)
// ---------------------------------------------------------------------------------------------------

pub struct Navigateur {
    pub dossier: PathBuf,
    pub choisis: Vec<PathBuf>,
    entrees: Vec<(PathBuf, bool, u64, u64)>,
    charge: Option<PathBuf>,
    photos: bool,
}

impl Navigateur {
    pub fn new() -> Self {
        Navigateur { dossier: racine(), choisis: vec![], entrees: vec![], charge: None, photos: false }
    }

    fn charger(&mut self) {
        self.charge = Some(self.dossier.clone());
        let mut v: Vec<(PathBuf, bool, u64, u64)> = std::fs::read_dir(&self.dossier).into_iter().flatten().flatten().filter_map(|e| {
            let m = e.metadata().ok()?;
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('.') {
                return None;
            }
            Some((e.path(), m.is_dir(), m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |t| t.as_secs())))
        }).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.file_name().cmp(&b.0.file_name())));
        self.entrees = v;
    }

    fn dernieres_photos(&mut self) {
        self.photos = true;
        self.charge = None;
        let r = racine();
        let mut v: Vec<Fichier> = ["DCIM", "Pictures"].iter().flat_map(|d| lister_tout(&r.join(d), 20_000)).filter(|f| matches!(famille(&f.chemin.to_string_lossy()), "Images" | "Vidéos")).collect();
        v.sort_by(|a, b| b.date.cmp(&a.date));
        self.entrees = v.into_iter().take(80).map(|f| (f.chemin, false, f.taille, f.date)).collect();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if !self.photos && self.charge.as_ref() != Some(&self.dossier) {
            self.charger();
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("📸 Dernières photos").clicked() {
                self.dernieres_photos();
            }
            for (n, d) in [("📥 Téléchargements", "Download"), ("🏠 Tout", "")] {
                if ui.button(n).clicked() {
                    self.photos = false;
                    self.dossier = if d.is_empty() { racine() } else { racine().join(d) };
                }
            }
            if !self.photos && self.dossier != racine() && ui.button("⬆ Remonter").clicked() {
                if let Some(p) = self.dossier.parent() {
                    self.dossier = p.to_path_buf();
                }
            }
        });
        ui.weak(if self.photos { "Photos et vidéos les plus récentes".to_string() } else { self.dossier.display().to_string().replace(crate::android::STOCKAGE, "Téléphone") });
        let mut entrer = None;
        for (p, dossier, t, d) in &self.entrees {
            let nom = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            ui.horizontal(|ui| {
                if *dossier {
                    if ui.add(egui::Button::new(RichText::new(format!("📁 {nom}")).size(16.0)).frame(false)).clicked() {
                        entrer = Some(p.clone());
                    }
                } else {
                    let mut c = self.choisis.contains(p);
                    if ui.checkbox(&mut c, RichText::new(format!("{} {nom}", icone(&nom, false))).size(16.0)).changed() {
                        if c {
                            self.choisis.push(p.clone());
                        } else {
                            self.choisis.retain(|x| x != p);
                        }
                    }
                    ui.weak(format!("{} · {}", taille(*t), date(*d)));
                }
            });
        }
        if let Some(p) = entrer {
            self.dossier = p;
        }
        if self.entrees.is_empty() {
            ui.weak("Dossier vide.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// Doublons, rangement, renommage : calculs (testés)
// ---------------------------------------------------------------------------------------------------

/// Empreinte (SHA-1) du contenu ; `debut` = seulement les 64 premiers Ko (tri rapide).
pub fn empreinte(p: &Path, debut: bool) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = sha1_smol::Sha1::new();
    let mut b = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut b).ok()?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
        if debut {
            break;
        }
    }
    Some(h.digest().to_string())
}

/// Groupes de fichiers identiques (même taille, même contenu), du plus gros gaspillage au plus petit.
pub fn doublons(fichiers: &[Fichier], hacher: &dyn Fn(&Path, bool) -> Option<String>) -> Vec<Vec<Fichier>> {
    let mut par_taille: HashMap<u64, Vec<&Fichier>> = HashMap::new();
    for f in fichiers.iter().filter(|f| f.taille >= 4096) {
        par_taille.entry(f.taille).or_default().push(f);
    }
    let mut groupes = vec![];
    for (_, l) in par_taille.into_iter().filter(|(_, l)| l.len() > 1) {
        let mut par_debut: HashMap<String, Vec<&Fichier>> = HashMap::new();
        for f in l {
            if let Some(h) = hacher(&f.chemin, true) {
                par_debut.entry(h).or_default().push(f);
            }
        }
        for (_, l) in par_debut.into_iter().filter(|(_, l)| l.len() > 1) {
            let mut complet: HashMap<String, Vec<Fichier>> = HashMap::new();
            for f in l {
                let h = if f.taille <= 64 * 1024 { hacher(&f.chemin, true) } else { hacher(&f.chemin, false) };
                if let Some(h) = h {
                    complet.entry(h).or_default().push(f.clone());
                }
            }
            for (_, mut g) in complet.into_iter().filter(|(_, g)| g.len() > 1) {
                // on garde en premier le plus ancien (souvent l'original), et ceux de DCIM avant le reste
                g.sort_by_key(|f| (!f.chemin.to_string_lossy().contains("/DCIM/"), f.date));
                groupes.push(g);
            }
        }
    }
    groupes.sort_by_key(|g| std::cmp::Reverse(g[0].taille * (g.len() as u64 - 1)));
    groupes
}

/// Rangement par type : (fichier, nouveau chemin) pour les fichiers directement dans `dossier`.
pub fn plan_par_type(dossier: &Path, fichiers: &[PathBuf]) -> Vec<(PathBuf, PathBuf)> {
    fichiers.iter().filter(|p| p.parent() == Some(dossier)).map(|p| {
        let n = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        (p.clone(), dossier.join(famille(&n)).join(&n))
    }).collect()
}

/// Rangement par mois : « 2026-05 ».
pub fn plan_par_mois(dossier: &Path, fichiers: &[Fichier]) -> Vec<(PathBuf, PathBuf)> {
    fichiers.iter().filter(|f| f.chemin.parent() == Some(dossier) && f.date > 0).map(|f| {
        let (a, m, _) = crate::quotidien2::date_de_jours(f.date as i64 / 86_400);
        (f.chemin.clone(), dossier.join(format!("{a}-{m:02}")).join(f.chemin.file_name().unwrap_or_default()))
    }).collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Modele {
    Date,
    Numero,
    Nettoyer,
}

/// Nouveaux noms : par date (« 2026-05-14 18.32.07.jpg »), par numéro (« Vacances 001.jpg »),
/// ou nettoyés. Jamais deux fois le même nom.
pub fn plan_renommage(fichiers: &[Fichier], modele: Modele, prefixe: &str) -> Vec<(PathBuf, PathBuf)> {
    let mut l: Vec<&Fichier> = fichiers.iter().collect();
    l.sort_by_key(|f| (f.date, f.chemin.clone()));
    let mut pris: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let largeur = l.len().to_string().len().max(3);
    let mut v = vec![];
    for (k, f) in l.iter().enumerate() {
        let dossier = f.chemin.parent().unwrap_or(Path::new("")).to_path_buf();
        let nom = f.chemin.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (base, ext) = match nom.rsplit_once('.') {
            Some((b, e)) if !b.is_empty() => (b.to_string(), format!(".{}", e.to_lowercase())),
            _ => (nom.clone(), String::new()),
        };
        let nouveau = match modele {
            Modele::Date => {
                let (a, m, j) = crate::quotidien2::date_de_jours(f.date as i64 / 86_400);
                let s = f.date % 86_400;
                format!("{a}-{m:02}-{j:02} {:02}.{:02}.{:02}", s / 3600, s / 60 % 60, s % 60)
            }
            Modele::Numero => format!("{} {:0largeur$}", if prefixe.trim().is_empty() { "Fichier" } else { prefixe.trim() }, k + 1),
            Modele::Nettoyer => {
                let b = base.replace(['_', '+'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
                let b = b.trim_start_matches("IMG ").trim_start_matches("VID ").to_string();
                if b.is_empty() { base.clone() } else { b }
            }
        };
        let mut cible = dossier.join(format!("{nouveau}{ext}"));
        let mut i = 2;
        while pris.contains(&cible) || (cible != f.chemin && cible.exists()) {
            cible = dossier.join(format!("{nouveau} ({i}){ext}"));
            i += 1;
        }
        pris.insert(cible.clone());
        if cible != f.chemin {
            v.push((f.chemin.clone(), cible));
        }
    }
    v
}

/// Raisons pour lesquelles un fichier est louche (vide = rien à signaler). Note : 1 à 3.
pub fn fichier_louche(chemin: &str) -> Vec<(u8, &'static str)> {
    let nom = chemin.rsplit('/').next().unwrap_or(chemin).to_lowercase();
    let mut v = vec![];
    let parts: Vec<&str> = nom.split('.').collect();
    let ext = if parts.len() > 1 { *parts.last().unwrap() } else { "" };
    let exe = ["apk", "xapk", "apks", "exe", "msi", "bat", "cmd", "scr", "vbs", "js", "jar", "sh", "ps1", "com", "pif"];
    if parts.len() > 2 {
        let avant = parts[parts.len() - 2];
        let leurres = ["pdf", "jpg", "jpeg", "png", "mp4", "mp3", "doc", "docx", "txt", "zip"];
        if leurres.contains(&avant) && exe.contains(&ext) {
            v.push((3, "double extension : se fait passer pour un document ou une photo"));
        }
    }
    if matches!(ext, "apk" | "xapk" | "apks") {
        v.push((if chemin.contains("/Download/") { 2 } else { 1 }, "appli à installer (APK) : à n'installer que si elle vient d'un site sûr"));
    } else if matches!(ext, "exe" | "msi" | "bat" | "cmd" | "scr" | "vbs" | "js" | "ps1" | "com" | "pif") {
        v.push((2, "programme ou script pour PC : inutile sur un téléphone, souvent piégé"));
    }
    let mots = ["crack", "keygen", "hack", "cheat", "mod menu", "modmenu", "free robux", "freerobux", "vbucks", "generator", "unlimited", "patch", "premium free", "spy"];
    if let Some(m) = mots.iter().find(|m| nom.contains(*m)) {
        let _ = m;
        v.push((2, "nom typique des faux « hacks » ou logiciels piratés"));
    }
    if nom.starts_with('.') && chemin.contains("/Download/") {
        v.push((1, "fichier caché dans les Téléchargements"));
    }
    v
}

pub const MAGASINS: [&str; 8] = ["com.android.vending", "com.sec.android.app.samsungapps", "com.huawei.appmarket", "com.xiaomi.market", "com.xiaomi.mipicks", "com.oppo.market", "com.heytap.market", "com.amazon.venezia"];

/// Note de méfiance d'une appli et ses raisons.
pub fn appli_louche(a: &crate::android::Appli, accessibilite: &str) -> (u32, Vec<String>) {
    if a.systeme {
        return (0, vec![]);
    }
    let mut n = 0;
    let mut r = vec![];
    let hors_magasin = a.installateur.as_deref().is_none_or(|i| !MAGASINS.contains(&i));
    if hors_magasin {
        n += 2;
        r.push(format!("installée hors du Play Store ({})", a.installateur.as_deref().unwrap_or("fichier APK")));
    }
    let a_perm = |p: &str| a.permissions.iter().any(|x| x.ends_with(p));
    for (p, pts, texte) in [
        ("READ_SMS", 3, "peut lire tes SMS (codes de validation…)"),
        ("RECEIVE_SMS", 2, "reçoit tes SMS"),
        ("SEND_SMS", 2, "peut envoyer des SMS (payants ?)"),
        ("READ_CALL_LOG", 2, "lit ton journal d'appels"),
        ("BIND_DEVICE_ADMIN", 2, "veut être administrateur du téléphone"),
        ("REQUEST_INSTALL_PACKAGES", 1, "peut installer d'autres applis"),
        ("SYSTEM_ALERT_WINDOW", 1, "peut s'afficher par-dessus les autres applis"),
        ("PACKAGE_USAGE_STATS", 1, "voit quelles applis tu utilises"),
        ("READ_CONTACTS", 1, "lit tes contacts"),
    ] {
        if a_perm(p) {
            n += pts;
            r.push(texte.to_string());
        }
    }
    if a_perm("RECORD_AUDIO") && a_perm("CAMERA") && hors_magasin {
        n += 1;
        r.push("micro + appareil photo".into());
    }
    if accessibilite.contains(&format!("{}/", a.paquet)) {
        n += 3;
        r.push("service d'accessibilité actif : peut lire et contrôler l'écran".into());
    }
    let nom = a.nom.to_lowercase();
    if ["cleaner", "booster", "battery saver", "free vpn", "hack", "mod", "cheat"].iter().any(|m| nom.contains(m)) {
        n += 1;
        r.push("type d'appli souvent piégée (nettoyeur, booster, VPN gratuit, hack…)".into());
    }
    (n, r)
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Analyse {
    familles: Vec<(String, u64, usize)>,
    dossiers: Vec<(PathBuf, u64)>,
    gros: Vec<Fichier>,
    total: u64,
}

pub struct FichiersTel {
    onglet: usize,
    e: Etat,
    analyse: Arc<Mutex<Option<Analyse>>>,
    groupes: Arc<Mutex<Option<Vec<Vec<Fichier>>>>>,
    louches: Arc<Mutex<Option<(Vec<(PathBuf, Vec<(u8, &'static str)>)>, Vec<(crate::android::Appli, u32, Vec<String>)>)>>>,
    choisis: Vec<PathBuf>,
    plan: Vec<(PathBuf, PathBuf)>,
    nav: Navigateur,
    modele: Modele,
    prefixe: String,
}

impl FichiersTel {
    pub fn new() -> Self {
        FichiersTel { onglet: 0, e: Arc::default(), analyse: Arc::default(), groupes: Arc::default(), louches: Arc::default(), choisis: vec![], plan: vec![], nav: Navigateur::new(), modele: Modele::Date, prefixe: String::new() }
    }

    fn corbeille_choisis(&mut self) {
        let l = std::mem::take(&mut self.choisis);
        let (g, a) = (self.groupes.clone(), self.analyse.clone());
        en_fond(&self.e, "Mise à la corbeille…", move || {
            let mut n = 0;
            let mut gagne = 0;
            for p in &l {
                let t = p.metadata().map(|m| m.len()).unwrap_or(0);
                if mettre_a_la_corbeille(p).is_ok() {
                    n += 1;
                    gagne += t;
                }
            }
            if let Some(gr) = g.lock().unwrap().as_mut() {
                for x in gr.iter_mut() {
                    x.retain(|f| !l.contains(&f.chemin));
                }
                gr.retain(|x| x.len() > 1);
            }
            if let Some(an) = a.lock().unwrap().as_mut() {
                an.gros.retain(|f| !l.contains(&f.chemin));
            }
            Ok(format!("✔ {n} fichier(s) à la corbeille d'Outils ({} libérés après l'avoir vidée)", taille(gagne)))
        });
    }

    fn appliquer_plan(&mut self) {
        let plan = std::mem::take(&mut self.plan);
        en_fond(&self.e, "Déplacement…", move || {
            let mut n = 0;
            let mut signaler = vec![];
            for (de, vers) in &plan {
                if let Some(d) = vers.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let vers = if vers.exists() { libre(vers.parent().unwrap(), &vers.file_name().unwrap_or_default().to_string_lossy()) } else { vers.clone() };
                if std::fs::rename(de, &vers).is_ok() {
                    n += 1;
                    signaler.push(de.display().to_string());
                    signaler.push(vers.display().to_string());
                }
            }
            crate::android::signaler_media(&signaler);
            Ok(format!("✔ {n} fichier(s) déplacé(s) ou renommé(s)"))
        });
    }

    fn apercu_plan(&mut self, ui: &mut egui::Ui) {
        if self.plan.is_empty() {
            return;
        }
        ui.label(RichText::new(format!("{} changement(s) :", self.plan.len())).strong());
        for (de, vers) in self.plan.iter().take(25) {
            ui.label(format!("{}  →  {}", de.file_name().unwrap_or_default().to_string_lossy(), vers.display().to_string().replace(crate::android::STOCKAGE, "")));
        }
        if self.plan.len() > 25 {
            ui.weak(format!("… et {} autres", self.plan.len() - 25));
        }
        ui.horizontal(|ui| {
            if ui.button(RichText::new("✔ Appliquer").size(18.0).strong()).clicked() {
                self.appliquer_plan();
            }
            if ui.button("Annuler").clicked() {
                self.plan.clear();
            }
        });
    }
}

impl Outil for FichiersTel {
    fn nom(&self) -> &'static str {
        "📂 Fichiers du téléphone"
    }
    fn cat(&self) -> Cat {
        Cat::Fichiers
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if !acces_fichiers_ui(ui) {
            return;
        }
        onglets(ui, &mut self.onglet, &["📊 Espace", "👯 Doublons", "🗂 Ranger", "✏ Renommer", "🕵 Louche", "🗑 Corbeille"]);
        let occupe = etat_ui(ui, &self.e);
        match self.onglet {
            0 => {
                if gros_bouton(ui, "📊 Analyser le stockage") && !occupe {
                    let a = self.analyse.clone();
                    en_fond(&self.e, "Analyse de tous les fichiers…", move || {
                        let f = lister_tout(&racine(), 500_000);
                        let mut fam: HashMap<&str, (u64, usize)> = HashMap::new();
                        let mut dos: HashMap<PathBuf, u64> = HashMap::new();
                        let r = racine();
                        for x in &f {
                            let e = fam.entry(famille(&x.chemin.to_string_lossy())).or_default();
                            e.0 += x.taille;
                            e.1 += 1;
                            // dossiers de premier et deuxième niveau
                            if let Ok(rel) = x.chemin.strip_prefix(&r) {
                                let c: Vec<_> = rel.components().collect();
                                if c.len() > 1 {
                                    *dos.entry(r.join(c[0])).or_default() += x.taille;
                                }
                                if c.len() > 2 {
                                    *dos.entry(r.join(c[0]).join(c[1])).or_default() += x.taille;
                                }
                            }
                        }
                        let total = f.iter().map(|x| x.taille).sum();
                        let mut familles: Vec<(String, u64, usize)> = fam.into_iter().map(|(k, v)| (k.to_string(), v.0, v.1)).collect();
                        familles.sort_by(|a, b| b.1.cmp(&a.1));
                        let mut dossiers: Vec<(PathBuf, u64)> = dos.into_iter().collect();
                        dossiers.sort_by(|a, b| b.1.cmp(&a.1));
                        dossiers.truncate(15);
                        let mut gros = f;
                        gros.sort_by(|a, b| b.taille.cmp(&a.taille));
                        gros.truncate(40);
                        *a.lock().unwrap() = Some(Analyse { familles, dossiers, gros, total });
                        Ok(format!("{} utilisés par tes fichiers", taille(total)))
                    });
                }
                let a = self.analyse.lock().unwrap();
                if let Some(a) = a.as_ref() {
                    ui.label(RichText::new("Par type").strong());
                    for (f, t, n) in &a.familles {
                        ui.horizontal(|ui| {
                            ui.add_sized([120.0, 22.0], egui::Label::new(format!("{} {f}", icone(&format!("x.{}", match f.as_str() { "Images" => "jpg", "Vidéos" => "mp4", "Musique" => "mp3", "Documents" => "pdf", "Applis (APK)" => "apk", "Archives" => "zip", _ => "x" }), false))));
                            ui.add(egui::ProgressBar::new(*t as f32 / a.total.max(1) as f32).desired_width(ui.available_width() - 110.0).fill(ROSE).text(taille(*t)));
                            ui.weak(format!("{n}"));
                        });
                    }
                    ui.label(RichText::new("Plus gros dossiers").strong());
                    for (d, t) in &a.dossiers {
                        ui.horizontal(|ui| {
                            ui.label(d.display().to_string().replace(crate::android::STOCKAGE, ""));
                            ui.weak(taille(*t));
                        });
                    }
                    ui.label(RichText::new("Plus gros fichiers").strong());
                    for f in &a.gros {
                        let mut c = self.choisis.contains(&f.chemin);
                        ui.horizontal(|ui| {
                            let n = f.chemin.file_name().unwrap_or_default().to_string_lossy().to_string();
                            if ui.checkbox(&mut c, format!("{} {n}", icone(&n, false))).changed() {
                                if c { self.choisis.push(f.chemin.clone()) } else { self.choisis.retain(|x| x != &f.chemin) }
                            }
                            ui.weak(taille(f.taille));
                        });
                    }
                }
                drop(a);
                if !self.choisis.is_empty() && gros_bouton(ui, &format!("🗑 Mettre {} fichier(s) à la corbeille", self.choisis.len())) {
                    self.corbeille_choisis();
                }
            }
            1 => {
                if gros_bouton(ui, "👯 Chercher les doublons") && !occupe {
                    let g = self.groupes.clone();
                    en_fond(&self.e, "Recherche des fichiers identiques…", move || {
                        let f = lister_tout(&racine(), 500_000);
                        let gr = doublons(&f, &empreinte);
                        let perdu: u64 = gr.iter().map(|x| x[0].taille * (x.len() as u64 - 1)).sum();
                        let n = gr.len();
                        *g.lock().unwrap() = Some(gr);
                        Ok(format!("{n} groupe(s) de doublons · {} récupérables", taille(perdu)))
                    });
                }
                let gr = self.groupes.lock().unwrap().clone();
                if let Some(gr) = gr {
                    if !gr.is_empty() && ui.button("Cocher toutes les copies (garde l'original)").clicked() {
                        self.choisis = gr.iter().flat_map(|g| g[1..].iter().map(|f| f.chemin.clone())).collect();
                    }
                    for g in gr.iter().take(100) {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.weak(format!("{} × {}", g.len(), taille(g[0].taille)));
                            for (k, f) in g.iter().enumerate() {
                                let mut c = self.choisis.contains(&f.chemin);
                                let t = format!("{}{}", if k == 0 { "⭐ " } else { "" }, f.chemin.display().to_string().replace(crate::android::STOCKAGE, ""));
                                if ui.checkbox(&mut c, t).changed() {
                                    if c { self.choisis.push(f.chemin.clone()) } else { self.choisis.retain(|x| x != &f.chemin) }
                                }
                            }
                        });
                    }
                }
                if !self.choisis.is_empty() && gros_bouton(ui, &format!("🗑 Mettre {} copie(s) à la corbeille", self.choisis.len())) {
                    self.corbeille_choisis();
                }
                ui.weak("⭐ = celui qu'on garde (le plus ancien, ou celui de l'appareil photo).");
            }
            2 => {
                ui.weak("Range en sous-dossiers. Tu vois la liste avant que rien ne bouge.");
                let r = racine();
                if gros_bouton(ui, "📥 Ranger les Téléchargements par type") {
                    let d = r.join("Download");
                    let f: Vec<PathBuf> = lister_tout(&d, 100_000).into_iter().map(|f| f.chemin).collect();
                    self.plan = plan_par_type(&d, &f);
                }
                for (titre, d) in [("📸 Ranger les photos de l'appareil par mois", "DCIM/Camera"), ("🖼 Ranger les captures d'écran par mois", "Pictures/Screenshots"), ("💬 Ranger les images WhatsApp par mois", "Android/media/com.whatsapp/WhatsApp/Media/WhatsApp Images")] {
                    if gros_bouton(ui, titre) {
                        let d = r.join(d);
                        let f = lister_tout(&d, 100_000);
                        self.plan = plan_par_mois(&d, &f);
                        if self.plan.is_empty() {
                            *self.e.lock().unwrap() = (false, "Rien à ranger ici.".into());
                        }
                    }
                }
                self.apercu_plan(ui);
            }
            3 => {
                ui.weak("Choisis un dossier, puis un modèle de nom : les fichiers du dossier (pas les sous-dossiers) sont renommés.");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.modele, Modele::Date, "📅 Par date");
                    ui.selectable_value(&mut self.modele, Modele::Numero, "🔢 Nom + numéro");
                    ui.selectable_value(&mut self.modele, Modele::Nettoyer, "🧹 Nettoyer les noms");
                });
                if self.modele == Modele::Numero {
                    ui.add(egui::TextEdit::singleline(&mut self.prefixe).hint_text("ex. Vacances Bretagne").desired_width(f32::INFINITY));
                }
                if gros_bouton(ui, &format!("👀 Voir les nouveaux noms ({})", self.nav.dossier.file_name().unwrap_or_default().to_string_lossy())) {
                    let f: Vec<Fichier> = lister_tout(&self.nav.dossier, 50_000).into_iter().filter(|f| f.chemin.parent() == Some(self.nav.dossier.as_path())).collect();
                    self.plan = plan_renommage(&f, self.modele, &self.prefixe);
                    if self.plan.is_empty() {
                        *self.e.lock().unwrap() = (false, "Rien à renommer.".into());
                    }
                }
                self.apercu_plan(ui);
                ui.separator();
                self.nav.ui(ui);
            }
            4 => {
                if gros_bouton(ui, "🕵 Chercher les fichiers et applis louches") && !occupe {
                    let l = self.louches.clone();
                    en_fond(&self.e, "Inspection…", move || {
                        let f = lister_tout(&racine(), 500_000);
                        let mut fl: Vec<(PathBuf, Vec<(u8, &'static str)>)> = f.into_iter().filter_map(|x| {
                            let r = fichier_louche(&x.chemin.to_string_lossy());
                            (!r.is_empty()).then_some((x.chemin, r))
                        }).collect();
                        fl.sort_by_key(|x| std::cmp::Reverse(x.1.iter().map(|r| r.0 as u32).sum::<u32>()));
                        let acc = crate::android::accessibilite_active();
                        let mut ap: Vec<(crate::android::Appli, u32, Vec<String>)> = crate::android::applis().into_iter().filter_map(|a| {
                            let (n, r) = appli_louche(&a, &acc);
                            (n >= 3).then_some((a, n, r))
                        }).collect();
                        ap.sort_by_key(|x| std::cmp::Reverse(x.1));
                        let bilan = format!("{} fichier(s) et {} appli(s) à vérifier", fl.len(), ap.len());
                        *l.lock().unwrap() = Some((fl, ap));
                        Ok(bilan)
                    });
                }
                let l = self.louches.lock().unwrap().clone();
                if let Some((fl, ap)) = l {
                    ui.label(RichText::new("📱 Applis").size(18.0).strong());
                    if ap.is_empty() {
                        ui.colored_label(VERT, "✔ Aucune appli inquiétante.");
                    }
                    for (a, n, r) in &ap {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&a.nom).strong().size(17.0));
                                ui.colored_label(if *n >= 6 { ROUGE } else { ORANGE }, if *n >= 6 { "très louche" } else { "à vérifier" });
                            });
                            ui.weak(&a.paquet);
                            for x in r {
                                ui.label(format!("• {x}"));
                            }
                            ui.horizontal(|ui| {
                                if ui.button("ℹ Fiche").clicked() {
                                    crate::android::fiche_appli(&a.paquet);
                                }
                                if ui.button(RichText::new("🗑 Désinstaller").color(ROUGE)).clicked() {
                                    crate::android::desinstaller(&a.paquet);
                                }
                            });
                        });
                    }
                    ui.label(RichText::new("📄 Fichiers").size(18.0).strong());
                    if fl.is_empty() {
                        ui.colored_label(VERT, "✔ Aucun fichier inquiétant.");
                    }
                    for (p, r) in fl.iter().take(150) {
                        let mut c = self.choisis.contains(p);
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut c, p.display().to_string().replace(crate::android::STOCKAGE, "")).changed() {
                                if c { self.choisis.push(p.clone()) } else { self.choisis.retain(|x| x != p) }
                            }
                        });
                        for (n, t) in r {
                            ui.colored_label(if *n >= 3 { ROUGE } else if *n == 2 { ORANGE } else { Color32::GRAY }, format!("    {t}"));
                        }
                    }
                    if !self.choisis.is_empty() && gros_bouton(ui, &format!("🗑 Mettre {} fichier(s) à la corbeille", self.choisis.len())) {
                        self.corbeille_choisis();
                    }
                    ui.weak("Une appli « à vérifier » n'est pas forcément dangereuse : demande-toi si tu sais d'où elle vient et si elle a besoin de ces accès.");
                }
            }
            _ => {
                let c = corbeille();
                let mut v: Vec<(PathBuf, String, u64)> = std::fs::read_dir(&c).into_iter().flatten().flatten().filter_map(|e| {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "origine") {
                        return None;
                    }
                    let origine = std::fs::read_to_string(p.with_extension("origine")).unwrap_or_default();
                    Some((p, origine, e.metadata().map(|m| m.len()).unwrap_or(0)))
                }).collect();
                v.sort_by(|a, b| b.0.cmp(&a.0));
                let total: u64 = v.iter().map(|x| x.2).sum();
                ui.label(RichText::new(format!("{} élément(s) · {}", v.len(), taille(total))).strong());
                ui.weak("Les fichiers restent ici 30 jours, puis sont supprimés.");
                // purge automatique
                let limite = maintenant().saturating_sub(30 * 86_400);
                for (p, _, _) in &v {
                    let t: u64 = p.file_name().and_then(|n| n.to_string_lossy().split("__").next().and_then(|x| x.parse().ok())).unwrap_or(u64::MAX);
                    if t < limite {
                        let _ = std::fs::remove_file(p);
                        let _ = std::fs::remove_file(p.with_extension("origine"));
                    }
                }
                if !v.is_empty() && ui.button(RichText::new("🔥 Vider la corbeille").color(ROUGE)).clicked() {
                    for (p, _, _) in &v {
                        let _ = std::fs::remove_file(p);
                        let _ = std::fs::remove_file(p.with_extension("origine"));
                    }
                }
                for (p, origine, t) in v.iter().take(200) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} ({})", origine.replace(crate::android::STOCKAGE, ""), taille(*t)));
                        if ui.small_button("↩ Restaurer").clicked() && !origine.is_empty() {
                            let o = PathBuf::from(origine);
                            if let Some(d) = o.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let dest = if o.exists() { libre(o.parent().unwrap(), &o.file_name().unwrap_or_default().to_string_lossy()) } else { o };
                            if std::fs::rename(p, &dest).is_ok() {
                                let _ = std::fs::remove_file(p.with_extension("origine"));
                                crate::android::signaler_media(&[dest.display().to_string()]);
                            }
                        }
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn louches_et_plans() {
        assert!(fichier_louche("/storage/emulated/0/Download/facture.pdf.apk").iter().any(|r| r.0 == 3));
        assert!(fichier_louche("/storage/emulated/0/Download/FreeRobux_generator.apk").len() >= 2);
        assert!(fichier_louche("/storage/emulated/0/DCIM/Camera/IMG_1.jpg").is_empty());
        let a = crate::android::Appli { paquet: "x.y".into(), nom: "Super Cleaner".into(), systeme: false, installateur: None, permissions: vec!["android.permission.READ_SMS".into()], installee: 0 };
        let (n, r) = appli_louche(&a, "x.y/.S");
        assert!(n >= 8 && r.len() >= 4);
        let ok = crate::android::Appli { installateur: Some("com.android.vending".into()), permissions: vec![], nom: "Météo".into(), ..a.clone() };
        assert_eq!(appli_louche(&ok, "").0, 0);
        let d = PathBuf::from("/s/Download");
        let p = plan_par_type(&d, &[d.join("a.pdf"), d.join("b.apk"), d.join("sous/c.jpg")]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].1, d.join("Applis (APK)").join("b.apk"));
        let f = |n: &str, t: u64| Fichier { chemin: PathBuf::from(format!("/s/DCIM/{n}")), taille: 10, date: t };
        let r = plan_renommage(&[f("IMG_2.jpg", 1_700_000_000), f("IMG_1.JPG", 1_700_000_000)], Modele::Date, "");
        assert_eq!(r.len(), 2);
        assert_ne!(r[0].1, r[1].1); // même seconde : pas deux fois le même nom
        assert!(r.iter().all(|x| x.1.to_string_lossy().ends_with(".jpg")));
        let r = plan_renommage(&[f("a.jpg", 2), f("b.jpg", 1)], Modele::Numero, "Vacances");
        assert_eq!(r[0].1, PathBuf::from("/s/DCIM/Vacances 001.jpg"));
        let m = plan_par_mois(&PathBuf::from("/s/DCIM"), &[f("x.jpg", 1_747_000_000)]);
        assert_eq!(m[0].1, PathBuf::from("/s/DCIM/2025-05/x.jpg"));
    }

    #[test]
    fn doublons_identiques() {
        let f = |n: &str, t: u64, d: u64| Fichier { chemin: PathBuf::from(n), taille: t, date: d };
        let l = vec![f("/a/1.jpg", 5000, 3), f("/DCIM/2.jpg", 5000, 9), f("/a/3.jpg", 5000, 1), f("/a/4.jpg", 9000, 1)];
        // 1 et 2 identiques, 3 a la même taille mais un autre contenu
        let h = |p: &Path, _: bool| Some(if p.ends_with("3.jpg") { "B".to_string() } else { "A".to_string() });
        let g = doublons(&l, &h);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].len(), 2);
        assert_eq!(g[0][0].chemin, PathBuf::from("/DCIM/2.jpg")); // on garde celui de DCIM
    }
}
