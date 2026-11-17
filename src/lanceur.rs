//! Lanceur de jeux : Steam, Epic, FiveM et jeux ajoutés à la main, avec temps de jeu.
//! Réécriture de LanceurJeux/lanceur.py, même fichier de données (%APPDATA%\LanceurJeux\data.json).
use eframe::egui::{self, vec2, Color32, FontId, Rect, Sense};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const IGNORER: &[&str] = &["Steamworks Common", "Steam Linux Runtime", "Proton"];
const JAQUETTES: &[&str] = &["library_600x900", "library_capsule", "library_header"]; // par ordre de préférence
const TRIS: &[&str] = &["Nom", "Plus joués", "Récents"];
const CW: f32 = 160.0;
const CH: f32 = 240.0;
const ATTENTE: Duration = Duration::from_secs(180); // démarrage max d'un jeu
const PAS: Duration = Duration::from_secs(5); // pas de comptage du temps
const CARTE: Color32 = Color32::from_rgb(0x18, 0x1b, 0x22);
const SURVOL: Color32 = Color32::from_rgb(0x23, 0x28, 0x36);
const TEXTE: Color32 = Color32::from_rgb(0xe8, 0xea, 0xf0);
const GRIS: Color32 = Color32::from_rgb(0x8a, 0x93, 0xa6);
const BLEU: Color32 = Color32::from_rgb(0x4c, 0x8d, 0xff);
const VERT: Color32 = Color32::from_rgb(0x3d, 0xdc, 0x84);

#[derive(Clone, Debug)]
pub struct Jeu {
    pub id: String,
    pub nom: String,
    pub cmd: String,
    pub dir: String,
    pub cover: Option<PathBuf>,
    pub steam: bool, // a besoin de Steam (FiveM)
}

#[derive(Serialize, Deserialize, Default)]
pub struct Sauvegarde {
    #[serde(default)]
    pub manual: Vec<String>,
    #[serde(default)]
    pub time: HashMap<String, f64>,
    #[serde(default)]
    pub last: HashMap<String, f64>,
    #[serde(default)]
    pub steam_first: HashMap<String, bool>,
    #[serde(flatten)]
    autres: serde_json::Map<String, serde_json::Value>,
}

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn maintenant() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Chemin comparable : antislashs, minuscules, sans antislash final.
fn norm(p: &str) -> String {
    p.replace('/', "\\").replace("\\\\", "\\").trim_end_matches('\\').to_lowercase()
}

pub fn duree(sec: f64) -> String {
    let min = sec as i64 / 60;
    let (h, m) = (min / 60, min % 60);
    if h > 0 {
        format!("{h} h {m:02} min")
    } else if m > 0 {
        format!("{m} min")
    } else if sec > 0.0 {
        "< 1 min".into()
    } else {
        "Jamais lancé".into()
    }
}

pub fn trier<'a>(jeux: &'a [Jeu], mode: &str, s: &Sauvegarde) -> Vec<&'a Jeu> {
    let cle = |g: &Jeu| -> f64 {
        match mode {
            "Plus joués" => -s.time.get(&g.id).copied().unwrap_or(0.0),
            "Récents" => -s.last.get(&g.id).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    };
    let mut v: Vec<&Jeu> = jeux.iter().collect();
    v.sort_by(|a, b| cle(a).total_cmp(&cle(b)).then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase())));
    v
}

/// Toutes les valeurs de `"cle"   "valeur"` dans un fichier VDF/ACF de Steam.
fn kv_tous(texte: &str, cle: &str) -> Vec<String> {
    let motif = format!("\"{cle}\"");
    let mut out = vec![];
    let mut reste = texte;
    while let Some(i) = reste.find(&motif) {
        reste = &reste[i + motif.len()..];
        let apres = reste.trim_start_matches([' ', '\t', '\r', '\n']);
        if apres.len() < reste.len() && apres.starts_with('"') {
            if let Some(fin) = apres[1..].find('"') {
                out.push(apres[1..1 + fin].to_string());
            }
        }
    }
    out
}

fn kv(texte: &str, cle: &str) -> String {
    kv_tous(texte, cle).into_iter().next().unwrap_or_default()
}

fn reg(cle: &str, valeur: &str, dword: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Registry::*;
    let mut buf = vec![0u8; 2048];
    let mut taille = buf.len() as u32;
    let genre = if dword { RRF_RT_REG_DWORD } else { RRF_RT_REG_SZ };
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, w(cle).as_ptr(), w(valeur).as_ptr(), genre, std::ptr::null_mut(),
                     buf.as_mut_ptr().cast(), &mut taille)
    };
    (r == 0).then(|| {
        buf.truncate(taille as usize);
        buf
    })
}

fn reg_texte(cle: &str, valeur: &str) -> Option<String> {
    let b = reg(cle, valeur, false)?;
    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
    Some(String::from_utf16_lossy(&u))
}

fn reg_dword(cle: &str, valeur: &str) -> Option<u32> {
    let b = reg(cle, valeur, true)?;
    Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?))
}

pub fn steam_racine() -> Option<PathBuf> {
    reg_texte(r"Software\Valve\Steam", "SteamPath").map(|s| PathBuf::from(s.replace('/', "\\")))
}

fn fichiers(dossier: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dossier).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            fichiers(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Jaquette portrait de préférence, y compris dans les sous-dossiers hachés du cache Steam.
pub fn trouver_jaquette(racine: &Path, appid: &str) -> Option<PathBuf> {
    let mut tous = vec![];
    fichiers(&racine.join("appcache").join("librarycache").join(appid), &mut tous);
    JAQUETTES.iter().find_map(|pre| {
        tous.iter().find(|p| {
            let n = p.file_name().unwrap_or_default().to_string_lossy();
            n.starts_with(pre) && n.ends_with(".jpg")
        }).cloned()
    })
}

pub fn scan_steam(racine: Option<&Path>) -> Vec<Jeu> {
    let Some(racine) = racine else { return vec![] };
    let mut libs: Vec<String> = vec![racine.display().to_string()];
    if let Ok(vdf) = std::fs::read_to_string(racine.join("steamapps").join("libraryfolders.vdf")) {
        libs.extend(kv_tous(&vdf, "path").into_iter().map(|p| p.replace("\\\\", "\\")));
    }
    let mut vus = HashSet::new();
    libs.retain(|l| vus.insert(norm(l)));
    let mut jeux = vec![];
    for lib in libs {
        let apps = Path::new(&lib).join("steamapps");
        for e in std::fs::read_dir(&apps).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let Ok(octets) = std::fs::read(e.path()) else { continue };
            let t = String::from_utf8_lossy(&octets);
            let (nom, appid, d) = (kv(&t, "name"), kv(&t, "appid"), kv(&t, "installdir"));
            if nom.is_empty() || d.is_empty() || IGNORER.iter().any(|i| nom.starts_with(i)) {
                continue;
            }
            jeux.push(Jeu {
                id: format!("steam:{appid}"),
                cmd: format!("steam://rungameid/{appid}"),
                dir: apps.join("common").join(&d).display().to_string(),
                cover: trouver_jaquette(racine, &appid),
                nom,
                steam: false,
            });
        }
    }
    jeux
}

pub fn scan_epic(manifestes: &Path) -> Vec<Jeu> {
    let mut jeux = vec![];
    for e in std::fs::read_dir(manifestes).into_iter().flatten().flatten() {
        if e.path().extension().is_none_or(|x| x != "item") {
            continue;
        }
        let Ok(t) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(j) = serde_json::from_str::<serde_json::Value>(&t) else { continue };
        let s = |k: &str| j.get(k).and_then(|v| v.as_str()).map(str::to_string);
        if let (Some(ns), Some(ci), Some(app), Some(nom), Some(dir)) =
            (s("CatalogNamespace"), s("CatalogItemId"), s("AppName"), s("DisplayName"), s("InstallLocation"))
        {
            jeux.push(Jeu {
                id: format!("epic:{app}"),
                cmd: format!("com.epicgames.launcher://apps/{ns}%3A{ci}%3A{app}?action=launch&silent=true"),
                nom, dir, cover: None, steam: false,
            });
        }
    }
    jeux
}

pub fn jeu_manuel(chemin: &str) -> Jeu {
    let p = Path::new(chemin);
    Jeu {
        id: format!("man:{chemin}"),
        nom: p.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
        cmd: chemin.into(),
        dir: p.parent().map(|d| d.display().to_string()).unwrap_or_default(),
        cover: None,
        steam: false,
    }
}

/// FiveM n'a pas de manifeste : on le cherche à son emplacement habituel. Il a besoin de Steam (GTA V).
pub fn scan_fivem(chemin: &Path) -> Vec<Jeu> {
    if !chemin.is_file() {
        return vec![];
    }
    let g = jeu_manuel(&chemin.display().to_string());
    vec![Jeu { id: "fivem".into(), nom: "FiveM".into(), steam: true, ..g }]
}

fn chemin_processus(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::*;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut taille = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut taille);
        CloseHandle(h);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..taille as usize]))
    }
}

/// Vrai si un processus tourne depuis ce dossier.
pub fn tourne_dans(dossier: &str) -> bool {
    if dossier.is_empty() {
        return false;
    }
    let prefixe = norm(dossier) + "\\";
    let mut pids = vec![0u32; 4096];
    let mut n = 0u32;
    unsafe {
        windows_sys::Win32::System::ProcessStatus::K32EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut n);
    }
    pids[..n as usize / 4].iter().any(|&pid| chemin_processus(pid).is_some_and(|c| c.to_lowercase().starts_with(&prefixe)))
}

/// Attend que le jeu démarre (max `attente`), puis appelle `ajouter(secondes)` tant qu'il tourne.
pub fn surveiller(dossier: &str, attente: Duration, pas: Duration, mut demarre: impl FnMut(), mut ajouter: impl FnMut(f64)) {
    let t0 = Instant::now();
    while !tourne_dans(dossier) {
        if t0.elapsed() > attente {
            return;
        }
        thread::sleep(Duration::from_secs(1));
    }
    demarre();
    let mut dernier = Instant::now();
    while tourne_dans(dossier) {
        thread::sleep(pas);
        ajouter(dernier.elapsed().as_secs_f64());
        dernier = Instant::now();
    }
}

/// Steam est lancé ET connecté : le pid enregistré est bien celui de steam.exe.
pub fn steam_pret() -> bool {
    let cle = r"Software\Valve\Steam\ActiveProcess";
    let (Some(pid), Some(user)) = (reg_dword(cle, "pid"), reg_dword(cle, "ActiveUser")) else { return false };
    user != 0 && chemin_processus(pid).is_some_and(|c| c.to_lowercase().ends_with("\\steam.exe"))
}

/// Lance Steam s'il n'est pas prêt et attend son chargement complet. Faux si trop long.
pub fn assurer_steam(pret: impl Fn() -> bool, lancer: impl FnOnce(), delai: Duration, pas: Duration) -> bool {
    if pret() {
        return true;
    }
    lancer();
    let t0 = Instant::now();
    while !pret() {
        if t0.elapsed() > delai {
            return false;
        }
        thread::sleep(pas);
    }
    true
}

fn ouvrir(cible: &str, dossier: Option<&str>) -> bool {
    let d = dossier.map(w);
    let r = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), w("open").as_ptr(), w(cible).as_ptr(),
            std::ptr::null(), d.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), 1)
    };
    r as isize > 32
}

struct Etat {
    save: Sauvegarde,
    statut: HashMap<String, String>,
    en_cours: HashSet<String>,
    fichier: PathBuf,
}

impl Etat {
    /// Écriture atomique : pas de fichier corrompu si on ferme en plein milieu.
    fn enregistrer(&self) {
        let tmp = self.fichier.with_extension("json.tmp");
        if let Some(d) = self.fichier.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(t) = serde_json::to_string(&self.save) {
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, &self.fichier);
            }
        }
    }
}

type Partage = Arc<Mutex<Etat>>;

fn statut(etat: &Partage, id: &str, texte: Option<&str>) {
    let mut e = etat.lock().unwrap();
    match texte {
        Some(t) => e.statut.insert(id.into(), t.into()),
        None => e.statut.remove(id),
    };
}

fn partie(etat: &Partage, g: &Jeu) {
    let flash = |t: &str| {
        statut(etat, &g.id, Some(t));
        thread::sleep(Duration::from_secs(4));
    };
    let steam_dabord = etat.lock().unwrap().save.steam_first.get(&g.id).copied().unwrap_or(g.steam);
    if steam_dabord {
        statut(etat, &g.id, Some("Démarrage de Steam…"));
        if !assurer_steam(steam_pret, || { ouvrir("steam://open/main", None); }, Duration::from_secs(240), Duration::from_secs(1)) {
            return flash("Steam n'a pas démarré");
        }
    }
    statut(etat, &g.id, Some("Lancement…"));
    let manuel = g.id.starts_with("man:") || g.id == "fivem";
    if !ouvrir(&g.cmd, manuel.then_some(g.dir.as_str())) {
        return flash("Lancement impossible");
    }
    surveiller(&g.dir, ATTENTE, PAS, || statut(etat, &g.id, None), |s| {
        let mut e = etat.lock().unwrap();
        *e.save.time.entry(g.id.clone()).or_insert(0.0) += s;
        e.enregistrer();
    });
}

enum Action {
    Lancer(Jeu),
    Dossier(String),
    SteamDabord(String, bool),
    Retirer(String),
    Ajouter,
    Actualiser,
}

pub struct Lanceur {
    jeux: Vec<Jeu>,
    etat: Partage,
    tri: usize,
    recherche: String,
}

impl Lanceur {
    pub fn new() -> Self {
        let fichier = PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("LanceurJeux").join("data.json");
        let save = std::fs::read_to_string(&fichier).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let etat = Etat { save, statut: HashMap::new(), en_cours: HashSet::new(), fichier };
        let mut l = Lanceur { jeux: vec![], etat: Arc::new(Mutex::new(etat)), tri: 0, recherche: String::new() };
        l.actualiser();
        l
    }

    fn actualiser(&mut self) {
        let var = |v: &str| PathBuf::from(std::env::var(v).unwrap_or_default());
        let mut jeux = scan_steam(steam_racine().as_deref());
        jeux.extend(scan_epic(&var("ProgramData").join(r"Epic\EpicGamesLauncher\Data\Manifests")));
        jeux.extend(scan_fivem(&var("LOCALAPPDATA").join(r"FiveM\FiveM.exe")));
        jeux.extend(self.etat.lock().unwrap().save.manual.iter().map(|p| jeu_manuel(p)));
        self.jeux = jeux;
    }

    fn lancer(&self, g: Jeu) {
        {
            let mut e = self.etat.lock().unwrap();
            if !e.en_cours.insert(g.id.clone()) {
                return; // évite de compter deux fois le même jeu
            }
            e.save.last.insert(g.id.clone(), maintenant());
            e.enregistrer();
        }
        let etat = self.etat.clone();
        thread::spawn(move || {
            partie(&etat, &g);
            let mut e = etat.lock().unwrap(); // quoi qu'il arrive, la carte redevient cliquable
            e.statut.remove(&g.id);
            e.en_cours.remove(&g.id);
        });
    }

    fn agir(&mut self, a: Action) {
        match a {
            Action::Lancer(g) => self.lancer(g),
            Action::Dossier(d) => {
                if Path::new(&d).is_dir() {
                    ouvrir(&d, None);
                }
            }
            Action::SteamDabord(id, on) => {
                let mut e = self.etat.lock().unwrap();
                e.save.steam_first.insert(id, on);
                e.enregistrer();
            }
            Action::Retirer(cmd) => {
                {
                    let mut e = self.etat.lock().unwrap();
                    e.save.manual.retain(|p| *p != cmd);
                    e.enregistrer();
                }
                self.actualiser();
            }
            Action::Ajouter => {
                let Some(p) = rfd::FileDialog::new().add_filter("Programme", &["exe"]).pick_file() else { return };
                let p = p.display().to_string();
                {
                    let mut e = self.etat.lock().unwrap();
                    if e.save.manual.contains(&p) {
                        return;
                    }
                    e.save.manual.push(p);
                    e.enregistrer();
                }
                self.actualiser();
            }
            Action::Actualiser => self.actualiser(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // mode plein écran / canapé
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            let plein = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!plein));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        }
        let mut actions = vec![];
        let etat = self.etat.clone();
        let e = etat.lock().unwrap();
        let q = self.recherche.to_lowercase();
        let affiches: Vec<&Jeu> = trier(&self.jeux, TRIS[self.tri], &e.save).into_iter()
            .filter(|g| g.nom.to_lowercase().contains(&q)).collect();

        ui.horizontal(|ui| {
            ui.heading("Mes jeux");
            let n = affiches.len();
            ui.colored_label(GRIS, format!("{n} jeu{}", if n > 1 { "x" } else { "" }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("Tri : {}", TRIS[self.tri])).clicked() {
                    self.tri = (self.tri + 1) % TRIS.len();
                }
                if ui.button("+ Ajouter").clicked() {
                    actions.push(Action::Ajouter);
                }
                if ui.button("Actualiser").clicked() {
                    actions.push(Action::Actualiser);
                }
                ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("Rechercher").desired_width(180.0));
            });
        });
        ui.add_space(8.0);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
            ui.horizontal_wrapped(|ui| {
                for g in &affiches {
                    carte(ui, g, &e, &mut actions);
                }
            });
        });
        drop(e);
        for a in actions {
            self.agir(a);
        }
    }
}

fn carte(ui: &mut egui::Ui, g: &Jeu, e: &Etat, actions: &mut Vec<Action>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(CW + 16.0, CH + 76.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6.0, if resp.hovered() { SURVOL } else { CARTE });
        let img = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(CW, CH));
        jaquette(ui, &p, img, g);
        let mut job = egui::text::LayoutJob::single_section(g.nom.clone(),
            egui::TextFormat::simple(FontId::proportional(14.0), TEXTE));
        job.wrap = egui::text::TextWrapping { max_width: CW, max_rows: 2, break_anywhere: false, overflow_character: Some('…') };
        let galley = ui.fonts(|f| f.layout_job(job));
        p.galley(img.left_bottom() + vec2(0.0, 4.0), galley, TEXTE);
        let temps = duree(e.save.time.get(&g.id).copied().unwrap_or(0.0));
        let (texte, couleur) = if let Some(s) = e.statut.get(&g.id) {
            (s.clone(), BLEU)
        } else if e.en_cours.contains(&g.id) {
            (format!("En cours · {temps}"), VERT)
        } else {
            (temps, GRIS)
        };
        p.text(rect.left_bottom() + vec2(8.0, -8.0), egui::Align2::LEFT_BOTTOM, texte, FontId::proportional(12.0), couleur);
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        actions.push(Action::Lancer(g.clone()));
    }
    resp.context_menu(|ui| {
        if ui.button("Ouvrir le dossier").clicked() {
            actions.push(Action::Dossier(g.dir.clone()));
            ui.close_menu();
        }
        if !g.id.starts_with("steam:") && !g.id.starts_with("epic:") {
            let mut on = e.save.steam_first.get(&g.id).copied().unwrap_or(g.steam);
            if ui.checkbox(&mut on, "Lancer Steam d'abord").changed() {
                actions.push(Action::SteamDabord(g.id.clone(), on));
                ui.close_menu();
            }
        }
        if g.id.starts_with("man:") && ui.button("Retirer de la liste").clicked() {
            actions.push(Action::Retirer(g.cmd.clone()));
            ui.close_menu();
        }
    });
}

/// Jaquette recadrée sans déformer, ou initiale sur fond coloré.
fn jaquette(ui: &egui::Ui, p: &egui::Painter, r: Rect, g: &Jeu) {
    if let Some(c) = &g.cover {
        let uri = format!("file://{}", c.display());
        if let Ok(egui::load::TexturePoll::Ready { texture }) =
            ui.ctx().try_load_texture(&uri, egui::TextureOptions::LINEAR, egui::SizeHint::default())
        {
            let (ri, rc) = (texture.size.x / texture.size.y, CW / CH);
            let uv = if ri > rc {
                let f = rc / ri;
                Rect::from_min_max(egui::pos2((1.0 - f) / 2.0, 0.0), egui::pos2((1.0 + f) / 2.0, 1.0))
            } else {
                let f = ri / rc;
                Rect::from_min_max(egui::pos2(0.0, (1.0 - f) / 2.0), egui::pos2(1.0, (1.0 + f) / 2.0))
            };
            p.image(texture.id, r, uv, Color32::WHITE);
            return;
        }
    }
    p.rect_filled(r, 0.0, Color32::from_rgb(0x20, 0x24, 0x2e));
    let teinte = g.nom.chars().map(|c| c as u32).sum::<u32>() % 360;
    let couleur = egui::ecolor::Hsva::new(teinte as f32 / 360.0, 0.54, 0.75, 1.0);
    let lettre = g.nom.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    p.text(r.center(), egui::Align2::CENTER_CENTER, lettre, FontId::proportional(72.0), Color32::from(couleur));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dossier_test(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lanceur_test_{nom}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn format_du_temps() {
        assert_eq!(duree(0.0), "Jamais lancé");
        assert_eq!(duree(30.0), "< 1 min");
        assert_eq!(duree(3900.0), "1 h 05 min");
        assert_eq!(duree(120.0), "2 min");
    }

    #[test]
    fn tri_des_jeux() {
        let j = |id: &str, nom: &str| Jeu { id: id.into(), nom: nom.into(), cmd: String::new(), dir: String::new(), cover: None, steam: false };
        let gs = [j("a", "Zed"), j("b", "Alpha"), j("c", "Beta")];
        let mut s = Sauvegarde::default();
        s.time = [("a".into(), 5.0), ("b".into(), 90.0)].into();
        s.last = [("a".into(), 200.0), ("b".into(), 100.0)].into();
        let ids = |m| trier(&gs, m, &s).iter().map(|g| g.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("Nom"), ["b", "c", "a"]);
        assert_eq!(ids("Plus joués"), ["b", "a", "c"]);
        assert_eq!(ids("Récents"), ["a", "b", "c"]);
    }

    #[test]
    fn epic_et_manuel_et_fivem() {
        let d = dossier_test("epic");
        fs::write(d.join("a.item"), r#"{"CatalogNamespace":"ns","CatalogItemId":"ci","AppName":"app","DisplayName":"Jeu","InstallLocation":"C:\\X"}"#).unwrap();
        let g = scan_epic(&d);
        assert_eq!(g[0].nom, "Jeu");
        assert!(g[0].cmd.contains("ns%3Aci%3Aapp"));
        let exe = d.join("faux.exe");
        fs::write(&exe, "x").unwrap();
        let m = jeu_manuel(&exe.display().to_string());
        assert_eq!((m.nom.as_str(), m.dir.clone()), ("faux", d.display().to_string()));
        let f = scan_fivem(&exe);
        assert!(f[0].steam && f[0].nom == "FiveM" && scan_fivem(&d.join("non.exe")).is_empty());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn jaquette_portrait_preferee() {
        let d = dossier_test("cover");
        let lc = d.join("appcache").join("librarycache").join("7");
        fs::create_dir_all(lc.join("aaa")).unwrap();
        fs::create_dir_all(lc.join("bbb")).unwrap();
        fs::write(lc.join("aaa").join("library_header.jpg"), "").unwrap();
        fs::write(lc.join("bbb").join("library_600x900.jpg"), "").unwrap();
        assert!(trouver_jaquette(&d, "7").unwrap().ends_with("library_600x900.jpg"));
        assert!(trouver_jaquette(&d, "8").is_none());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn vdf() {
        let t = "\"appid\"\t\t\"42\"\n\"name\"\t\t\"Mon Jeu\"\n\"path\"\t\t\"D:\\\\Steam\"";
        assert_eq!(kv(t, "name"), "Mon Jeu");
        assert_eq!(kv_tous(t, "path"), ["D:\\\\Steam"]);
        assert_eq!(kv(t, "absent"), "");
    }

    #[test]
    fn steam_de_ce_pc() {
        let steam = scan_steam(steam_racine().as_deref());
        assert!(!steam.iter().any(|g| IGNORER.iter().any(|i| g.nom.starts_with(i))));
        let ids: HashSet<_> = steam.iter().map(|g| &g.id).collect();
        assert_eq!(ids.len(), steam.len()); // pas de doublons
    }

    #[test]
    fn steam_dabord() {
        use std::cell::Cell;
        let lance = Cell::new(0);
        assert!(assurer_steam(|| true, || lance.set(lance.get() + 1), Duration::from_secs(1), Duration::from_millis(1)));
        assert_eq!(lance.get(), 0);
        let appels = Cell::new(0);
        assert!(assurer_steam(|| { appels.set(appels.get() + 1); appels.get() > 3 }, || lance.set(lance.get() + 1),
                              Duration::from_secs(5), Duration::from_millis(1)));
        assert_eq!(lance.get(), 1);
        assert!(!assurer_steam(|| false, || {}, Duration::from_millis(200), Duration::from_millis(50)));
        let _ = steam_pret();
    }

    #[test]
    fn vrai_jeu_compte_le_temps() {
        // un exe copié dans un dossier à lui : temps compté, jeu terminé -> arrêt
        let d = dossier_test("watch");
        let exe = d.join("faux_jeu.exe");
        fs::copy(r"C:\Windows\System32\PING.EXE", &exe).unwrap();
        let dir = d.display().to_string();
        assert!(!tourne_dans(&dir));
        let mut enfant = std::process::Command::new(&exe).args(["-n", "7", "127.0.0.1"])
            .stdout(std::process::Stdio::null()).spawn().unwrap();
        let (mut total, mut demarre) = (0.0, false);
        let t0 = Instant::now();
        surveiller(&dir, Duration::from_secs(10), Duration::from_secs(1), || demarre = true, |s| total += s);
        let dt = t0.elapsed().as_secs_f64();
        let _ = enfant.wait();
        assert!(demarre && total > 3.0 && total <= dt + 1.0, "{total} {dt}");
        assert!(!tourne_dans(&dir));
        // jamais démarré : abandon sans rien compter
        let t0 = Instant::now();
        surveiller(&dir, Duration::from_secs(2), Duration::from_secs(1), || panic!(), |_| panic!());
        assert!(t0.elapsed() < Duration::from_secs(5));
        fs::remove_dir_all(d).unwrap();
    }
}
