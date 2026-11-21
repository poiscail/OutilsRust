//! Jeux, suite : temps de chargement (8), comparateur de prix (9), actus des jeux (10),
//! galerie des captures (11), amis Steam (13), générateur de pseudos (15).
use crate::lanceur::{scan_epic, scan_steam, steam_racine, Jeu};
use crate::outil::{charger, enregistrer, hasard, ouvrir, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).user_agent("Outils").build()
}

fn json(url: &str) -> Result<serde_json::Value, String> {
    agent().get(url).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())
}

pub(crate) fn jeux_installes() -> Vec<Jeu> {
    let var = |v: &str| PathBuf::from(std::env::var(v).unwrap_or_default());
    let mut j = scan_steam(steam_racine().as_deref());
    j.extend(scan_epic(&var("ProgramData").join(r"Epic\EpicGamesLauncher\Data\Manifests")));
    j.sort_by_key(|g| g.nom.to_lowercase());
    j
}

fn date_courte(t: i64) -> String {
    let (a, m, j) = crate::quotidien2::date_de_jours(t.div_euclid(86_400));
    format!("{j:02}/{m:02}/{a}")
}

fn maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------------
// 8 · Temps de chargement
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct Mesure {
    pub date: String,
    pub fenetre: f32,
    pub jouable: Option<f32>,
}

/// Une fenêtre visible (assez grande) d'un processus lancé depuis `dossier` existe-t-elle ?
fn fenetre_du_jeu(dossier: &str) -> bool {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    struct Ctx {
        prefixe: String,
        trouve: bool,
    }
    unsafe extern "system" fn rappel(h: HWND, l: LPARAM) -> BOOL {
        let c = &mut *(l as *mut Ctx);
        if IsWindowVisible(h) == 0 {
            return 1;
        }
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(h, &mut r);
        if r.right - r.left < 640 || r.bottom - r.top < 400 {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(h, &mut pid);
        if crate::lanceur::chemin_processus(pid).is_some_and(|p| p.to_lowercase().starts_with(&c.prefixe)) {
            c.trouve = true;
            return 0;
        }
        1
    }
    let prefixe = dossier.replace('/', "\\").trim_end_matches('\\').to_lowercase() + "\\";
    let mut c = Ctx { prefixe, trouve: false };
    unsafe { EnumWindows(Some(rappel), &mut c as *mut Ctx as LPARAM) };
    c.trouve
}

#[derive(Default)]
struct EnCours {
    jeu: String,
    debut: Option<Instant>,
    fenetre: Option<f32>,
    fini: bool,
    message: String,
}

pub struct Chargements {
    jeux: Vec<Jeu>,
    choisi: usize,
    mesures: BTreeMap<String, Vec<Mesure>>,
    e: Arc<Mutex<EnCours>>,
    jouable: Arc<AtomicBool>,
}

impl Chargements {
    pub fn new() -> Self {
        let jouable: Arc<AtomicBool> = Arc::default();
        let j = jouable.clone();
        // Ctrl+Alt+J : « je peux jouer » (fin du vrai chargement)
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            if RegisterHotKey(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'J' as u32) == 0 {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    j.store(true, Relaxed);
                }
            }
        });
        Chargements { jeux: vec![], choisi: 0, mesures: charger("chargements"), e: Arc::default(), jouable }
    }

    fn mesurer(&self, g: Jeu) {
        self.jouable.store(false, Relaxed);
        *self.e.lock().unwrap() = EnCours { jeu: g.nom.clone(), debut: Some(Instant::now()), ..Default::default() };
        let (e, jouable) = (self.e.clone(), self.jouable.clone());
        std::thread::spawn(move || {
            use windows_sys::Win32::UI::Shell::ShellExecuteW;
            let w = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
            unsafe { ShellExecuteW(std::ptr::null_mut(), w("open").as_ptr(), w(&g.cmd).as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
            let t0 = Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(250));
                let t = t0.elapsed().as_secs_f32();
                let mut x = e.lock().unwrap();
                if x.fini {
                    return;
                }
                if x.fenetre.is_none() && fenetre_du_jeu(&g.dir) {
                    x.fenetre = Some(t);
                    x.message = format!("Fenêtre ouverte après {t:.1} s. Appuie sur Ctrl+Alt+J dès que tu peux jouer.");
                }
                if jouable.swap(false, Relaxed) && x.fenetre.is_some() {
                    x.fini = true;
                    x.message = format!("Jouable après {t:.1} s.");
                    let mut m: BTreeMap<String, Vec<Mesure>> = charger("chargements");
                    m.entry(g.nom.clone()).or_default().push(Mesure { date: date_courte(maintenant()), fenetre: x.fenetre.unwrap_or(t), jouable: Some(t) });
                    enregistrer("chargements", &m);
                    crate::systeme::notifier("Temps de chargement", &format!("{} : {t:.1} s", g.nom));
                    return;
                }
                if t > 600.0 {
                    x.fini = true;
                    if let Some(f) = x.fenetre {
                        let mut m: BTreeMap<String, Vec<Mesure>> = charger("chargements");
                        m.entry(g.nom.clone()).or_default().push(Mesure { date: date_courte(maintenant()), fenetre: f, jouable: None });
                        enregistrer("chargements", &m);
                    }
                    x.message = "Mesure arrêtée (10 min).".into();
                    return;
                }
            }
        });
    }
}

impl Outil for Chargements {
    fn nom(&self) -> &'static str {
        "⏳ Temps de chargement"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Temps de chargement des jeux");
        ui.weak("Lance un jeu d'ici : Outils chronomètre l'ouverture de sa fenêtre, puis appuie sur Ctrl+Alt+J quand tu peux vraiment jouer. Pratique pour voir si un SSD, un mod ou une mise à jour change quelque chose.");
        if self.jeux.is_empty() {
            self.jeux = jeux_installes();
        }
        let x = self.e.lock().unwrap();
        let en_cours = x.debut.is_some() && !x.fini;
        drop(x);
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("jeu_chargement").width(280.0).selected_text(self.jeux.get(self.choisi).map(|g| g.nom.clone()).unwrap_or_default()).show_ui(ui, |ui| {
                for (k, g) in self.jeux.iter().enumerate() {
                    ui.selectable_value(&mut self.choisi, k, &g.nom);
                }
            });
            if en_cours {
                if ui.button("⏹ Annuler").clicked() {
                    let mut x = self.e.lock().unwrap();
                    x.fini = true;
                    x.message = "Annulé.".into();
                }
            } else if ui.add_enabled(!self.jeux.is_empty(), egui::Button::new("▶ Lancer et chronométrer")).clicked() {
                self.mesurer(self.jeux[self.choisi].clone());
            }
        });
        let x = self.e.lock().unwrap();
        if let (Some(d), false) = (x.debut, x.fini) {
            ui.label(RichText::new(format!("⏱ {:.1} s — {}", d.elapsed().as_secs_f32(), x.jeu)).size(22.0));
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if !x.message.is_empty() {
            ui.label(&x.message);
        }
        let fini = x.fini;
        drop(x);
        if fini {
            self.mesures = charger("chargements");
        }
        ui.separator();
        if self.mesures.is_empty() {
            ui.weak("Pas encore de mesure.");
            return;
        }
        let max = self.mesures.values().flatten().map(|m| m.jouable.unwrap_or(m.fenetre)).fold(1.0f32, f32::max);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (jeu, l) in &self.mesures {
                let der = l.last().unwrap();
                let moy = l.iter().map(|m| m.jouable.unwrap_or(m.fenetre)).sum::<f32>() / l.len() as f32;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(jeu).strong());
                    ui.weak(format!("{} mesure(s) · moyenne {moy:.1} s", l.len()));
                });
                let (r, _) = ui.allocate_exact_size(egui::vec2(420.0, 18.0), egui::Sense::hover());
                let lf = der.fenetre / max * r.width();
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(lf, 16.0)), 3.0, Color32::from_rgb(120, 170, 255));
                if let Some(j) = der.jouable {
                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(lf, 0.0), egui::vec2((j - der.fenetre).max(0.0) / max * r.width(), 16.0)), 3.0, Color32::from_rgb(255, 120, 170));
                }
                ui.weak(format!("dernière ({}) : fenêtre {:.1} s{}", der.date, der.fenetre, der.jouable.map(|j| format!(", jouable {j:.1} s")).unwrap_or_default()));
                ui.add_space(4.0);
            }
        });
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(120, 170, 255), "■ jusqu'à la fenêtre");
            ui.colored_label(Color32::from_rgb(255, 120, 170), "■ jusqu'à jouable");
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 9 · Comparateur de prix (CheapShark)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Offre {
    boutique: String,
    prix: f64,
    normal: f64,
    lien: String,
}

#[derive(Clone)]
struct Resultat {
    titre: String,
    image: String,
    plus_bas: Option<(f64, String)>,
    offres: Vec<Offre>,
    steam_eur: Option<String>,
}

fn boutiques() -> HashMap<String, String> {
    json("https://www.cheapshark.com/api/1.0/stores").ok().and_then(|j| j.as_array().cloned()).unwrap_or_default().iter()
        .filter_map(|s| Some((s["storeID"].as_str()?.to_string(), s["storeName"].as_str()?.to_string()))).collect()
}

fn chercher_prix(q: &str) -> Result<Vec<Resultat>, String> {
    let liste = agent().get("https://www.cheapshark.com/api/1.0/games").query("title", q).query("limit", "5").call().map_err(|e| e.to_string())?
        .into_json::<serde_json::Value>().map_err(|e| e.to_string())?;
    let noms = boutiques();
    let mut v = vec![];
    for g in liste.as_array().into_iter().flatten().take(5) {
        let Some(id) = g["gameID"].as_str() else { continue };
        let d = json(&format!("https://www.cheapshark.com/api/1.0/games?id={id}"))?;
        let f = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
        let mut offres: Vec<Offre> = d["deals"].as_array().into_iter().flatten().map(|o| Offre {
            boutique: noms.get(o["storeID"].as_str().unwrap_or("")).cloned().unwrap_or_else(|| format!("boutique {}", o["storeID"].as_str().unwrap_or("?"))),
            prix: f(&o["price"]),
            normal: f(&o["retailPrice"]),
            lien: format!("https://www.cheapshark.com/redirect?dealID={}", o["dealID"].as_str().unwrap_or("")),
        }).collect();
        offres.sort_by(|a, b| a.prix.total_cmp(&b.prix));
        let steam_eur = g["steamAppID"].as_str().and_then(|a| {
            let j = json(&format!("https://store.steampowered.com/api/appdetails?appids={a}&cc=fr&filters=price_overview")).ok()?;
            j[a]["data"]["price_overview"]["final_formatted"].as_str().map(String::from)
        });
        v.push(Resultat {
            titre: d["info"]["title"].as_str().unwrap_or("?").to_string(),
            image: d["info"]["thumb"].as_str().unwrap_or("").to_string(),
            plus_bas: d["cheapestPriceEver"]["price"].as_str().and_then(|p| p.parse().ok()).map(|p| (p, d["cheapestPriceEver"]["date"].as_i64().map(date_courte).unwrap_or_default())),
            offres,
            steam_eur,
        });
    }
    Ok(v)
}

pub struct Prix {
    q: String,
    r: Arc<Mutex<Option<Result<Vec<Resultat>, String>>>>,
}

impl Prix {
    pub fn new() -> Self {
        Prix { q: String::new(), r: Arc::default() }
    }
}

impl Outil for Prix {
    fn nom(&self) -> &'static str {
        "💸 Comparateur de prix"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comparateur de prix des jeux");
        ui.weak("Le prix d'un jeu dans les boutiques officielles (Steam, GOG, Epic, Humble, Fanatical, GamesPlanet…), avec le plus bas jamais vu.");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("nom du jeu").desired_width(260.0));
            if (ui.button("🔍 Comparer").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.q.trim().is_empty() {
                let (q, r) = (self.q.trim().to_string(), self.r.clone());
                *r.lock().unwrap() = None;
                std::thread::spawn(move || *r.lock().unwrap() = Some(chercher_prix(&q)));
                *self.r.lock().unwrap() = Some(Err("recherche…".into()));
            }
        });
        match self.r.lock().unwrap().clone() {
            None => {}
            Some(Err(e)) => {
                if e == "recherche…" {
                    ui.spinner();
                    ui.ctx().request_repaint_after(Duration::from_millis(300));
                } else {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label("Aucun jeu trouvé.");
            }
            Some(Ok(v)) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for g in v {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if !g.image.is_empty() {
                                    ui.add(egui::Image::new(g.image.as_str()).max_height(60.0));
                                }
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&g.titre).size(18.0).strong());
                                    if let Some(s) = &g.steam_eur {
                                        ui.label(format!("Steam France : {s}"));
                                    }
                                    if let Some((p, d)) = &g.plus_bas {
                                        ui.weak(format!("Plus bas jamais vu : {p:.2} $ ({d})"));
                                    }
                                });
                            });
                            egui::Grid::new(("offres", &g.titre)).striped(true).show(ui, |ui| {
                                for (k, o) in g.offres.iter().enumerate() {
                                    let remise = if o.normal > 0.0 { (100.0 * (1.0 - o.prix / o.normal)).round() } else { 0.0 };
                                    ui.label(if k == 0 { RichText::new(&o.boutique).strong().color(Color32::LIGHT_GREEN) } else { RichText::new(&o.boutique) });
                                    ui.label(format!("{:.2} $", o.prix));
                                    ui.weak(if remise > 0.0 { format!("-{remise:.0} %") } else { String::new() });
                                    if ui.small_button("🛒 Voir").clicked() {
                                        let _ = std::process::Command::new("explorer").arg(&o.lien).spawn();
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    ui.weak("Prix en dollars (données CheapShark), le prix Steam est en euros.");
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 10 · Actus et patchs de mes jeux (Steam)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Serialize, Deserialize)]
pub struct Actu {
    pub jeu: String,
    pub titre: String,
    pub texte: String,
    pub lien: String,
    pub date: i64,
    pub patch: bool,
}

/// Retire le BBCode / HTML des actus Steam.
pub fn texte_propre(s: &str) -> String {
    let s = regex::Regex::new(r"\[/?[a-zA-Z0-9*]+(=[^\]]*)?\]|<[^>]+>|\{STEAM_CLAN_IMAGE\}[^\s]*").unwrap().replace_all(s, " ");
    let s = s.replace("&quot;", "\"").replace("&amp;", "&").replace("&#39;", "'");
    regex::Regex::new(r"\s+").unwrap().replace_all(&s, " ").trim().to_string()
}

pub struct Actus {
    l: Arc<Mutex<(bool, Vec<Actu>)>>,
    patchs: bool,
    lance: bool,
}

impl Actus {
    pub fn new() -> Self {
        Actus { l: Arc::new(Mutex::new((false, charger("actus_jeux")))), patchs: false, lance: false }
    }

    fn charger(&mut self) {
        self.lance = true;
        let l = self.l.clone();
        l.lock().unwrap().0 = true;
        std::thread::spawn(move || {
            let jeux: Vec<(String, String)> = scan_steam(steam_racine().as_deref()).into_iter().filter_map(|g| Some((g.id.strip_prefix("steam:")?.to_string(), g.nom))).collect();
            let mut v = vec![];
            for (appid, nom) in jeux {
                let Ok(j) = json(&format!("https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={appid}&count=4&maxlength=400&format=json")) else { continue };
                for n in j["appnews"]["newsitems"].as_array().into_iter().flatten() {
                    let tags = n["tags"].as_array().map(|t| t.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default();
                    let titre = n["title"].as_str().unwrap_or("").to_string();
                    let bas = titre.to_lowercase();
                    v.push(Actu {
                        jeu: nom.clone(),
                        patch: tags.contains("patchnotes") || ["patch", "update", "mise à jour", "hotfix"].iter().any(|m| bas.contains(m)),
                        titre,
                        texte: texte_propre(n["contents"].as_str().unwrap_or("")),
                        lien: n["url"].as_str().unwrap_or("").to_string(),
                        date: n["date"].as_i64().unwrap_or(0),
                    });
                }
            }
            v.sort_by(|a, b| b.date.cmp(&a.date));
            v.truncate(300);
            enregistrer("actus_jeux", &v);
            *l.lock().unwrap() = (false, v);
        });
    }
}

impl Outil for Actus {
    fn nom(&self) -> &'static str {
        "📰 Actus de mes jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn resume(&self) -> Option<String> {
        let l = self.l.lock().unwrap();
        let semaine = maintenant() - 7 * 86_400;
        let n = l.1.iter().filter(|a| a.patch && a.date > semaine).count();
        (n > 0).then(|| format!("📰 {n} mise(s) à jour de tes jeux cette semaine"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Actus et mises à jour de mes jeux");
        if !self.lance {
            self.charger();
        }
        let (charge, l) = self.l.lock().unwrap().clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.patchs, "Seulement les mises à jour / patchs");
            if charge {
                ui.spinner();
                ui.weak("chargement des actus de tes jeux Steam…");
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else if ui.small_button("⟳").clicked() {
                self.charger();
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            for a in l.iter().filter(|a| !self.patchs || a.patch).take(150) {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&a.jeu).color(Color32::from_rgb(255, 120, 170)).strong());
                        ui.weak(date_courte(a.date));
                        if a.patch {
                            ui.label(RichText::new("🔧 patch").small());
                        }
                    });
                    if ui.link(RichText::new(&a.titre).strong()).clicked() {
                        let _ = std::process::Command::new("explorer").arg(&a.lien).spawn();
                    }
                    ui.label(a.texte.chars().take(300).collect::<String>());
                });
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 11 · Galerie des captures de jeux
// ---------------------------------------------------------------------------------------------------

/// Nom du jeu d'après le fichier : « Elden Ring 2024-03-02 21-14-33.png » (Xbox Game Bar) → « Elden Ring ».
pub fn jeu_du_fichier(n: &str) -> Option<String> {
    let c = regex::Regex::new(r"^(.+?)\s+\d{4}[-.]\d{2}[-.]\d{2}").unwrap().captures(n)?;
    Some(c[1].trim().trim_end_matches(" Screenshot").to_string())
}

pub(crate) fn captures() -> BTreeMap<String, Vec<PathBuf>> {
    let mut m: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let images = |d: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| ["jpg", "jpeg", "png", "webp"].contains(&x.to_string_lossy().to_lowercase().as_str()))).collect()
    };
    // Steam : userdata/<compte>/760/remote/<appid>/screenshots
    if let Some(r) = steam_racine() {
        let noms: HashMap<String, String> = scan_steam(Some(&r)).into_iter().filter_map(|g| Some((g.id.strip_prefix("steam:")?.to_string(), g.nom))).collect();
        for u in std::fs::read_dir(r.join("userdata")).into_iter().flatten().flatten() {
            for a in std::fs::read_dir(u.path().join("760").join("remote")).into_iter().flatten().flatten() {
                let id = a.file_name().to_string_lossy().into_owned();
                let v = images(&a.path().join("screenshots"));
                if !v.is_empty() {
                    m.entry(noms.get(&id).cloned().unwrap_or(format!("Jeu Steam {id}"))).or_default().extend(v);
                }
            }
        }
    }
    // Xbox Game Bar, Outils, NVIDIA
    let u = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default());
    for d in [u.join("Videos").join("Captures"), u.join("Pictures").join("Captures"), u.join("Pictures").join("Screenshots")] {
        for p in images(&d) {
            let n = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            m.entry(jeu_du_fichier(&n).unwrap_or_else(|| "Autres captures".into())).or_default().push(p);
        }
    }
    for d in std::fs::read_dir(u.join("Videos")).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
        let n = d.file_name().to_string_lossy().into_owned();
        if n != "Captures" {
            let v = images(&d.path());
            if !v.is_empty() {
                m.entry(n).or_default().extend(v);
            }
        }
    }
    for v in m.values_mut() {
        v.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|x| x.modified()).ok()));
    }
    m
}

type Miniatures = Arc<Mutex<HashMap<PathBuf, Option<egui::ColorImage>>>>;

pub struct Galerie {
    liste: Arc<Mutex<Option<BTreeMap<String, Vec<PathBuf>>>>>,
    jeu: Option<String>,
    mini: Miniatures,
    textures: HashMap<PathBuf, egui::TextureHandle>,
    grande: Option<(PathBuf, egui::TextureHandle)>,
}

impl Galerie {
    pub fn new() -> Self {
        Galerie { liste: Arc::default(), jeu: None, mini: Arc::default(), textures: HashMap::new(), grande: None }
    }
}

impl Outil for Galerie {
    fn nom(&self) -> &'static str {
        "🖼 Galerie des captures de jeux"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Galerie des captures de jeux");
        ui.weak("Captures Steam (F12), Xbox Game Bar (Win+Alt+Impr), NVIDIA et Outils, rangées par jeu.");
        if self.liste.lock().unwrap().is_none() {
            let l = self.liste.clone();
            *l.lock().unwrap() = Some(BTreeMap::new());
            std::thread::spawn(move || *l.lock().unwrap() = Some(captures()));
        }
        if let Some((p, t)) = self.grande.clone() {
            let mut retour = false;
            ui.horizontal(|ui| {
                retour = ui.button("⬅ Retour").clicked();
                if ui.button("📂 Ouvrir").clicked() {
                    ouvrir(&p);
                }
                if ui.button("📋 Copier l'image").clicked() {
                    if let Ok(i) = image::open(&p) {
                        crate::capture::win::copier_image(&i.to_rgba8());
                    }
                }
                ui.weak(p.display().to_string());
            });
            if retour {
                self.grande = None;
            } else {
                let l = ui.available_width();
                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(l, l * t.size()[1] as f32 / t.size()[0] as f32)));
            }
            return;
        }
        let liste = self.liste.lock().unwrap().clone().unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.jeu.is_none(), format!("Tous ({})", liste.values().map(|v| v.len()).sum::<usize>())).clicked() {
                self.jeu = None;
            }
            for (j, v) in &liste {
                if ui.selectable_label(self.jeu.as_ref() == Some(j), format!("{j} ({})", v.len())).clicked() {
                    self.jeu = Some(j.clone());
                }
            }
            if ui.small_button("⟳").clicked() {
                *self.liste.lock().unwrap() = None;
            }
        });
        let mut fichiers: Vec<&PathBuf> = match &self.jeu {
            Some(j) => liste.get(j).map(|v| v.iter().collect()).unwrap_or_default(),
            None => liste.values().flatten().collect(),
        };
        fichiers.truncate(400);
        if fichiers.is_empty() {
            ui.weak("Aucune capture trouvée.");
        }
        let mut agrandir = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for p in fichiers {
                    let (r, rep) = ui.allocate_exact_size(egui::vec2(192.0, 108.0), egui::Sense::click());
                    if !ui.is_rect_visible(r) {
                        continue;
                    }
                    match self.textures.get(p) {
                        Some(t) => {
                            ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            ui.painter().rect_filled(r, 4.0, Color32::from_rgb(40, 30, 50));
                            let mut m = self.mini.lock().unwrap();
                            match m.get_mut(p) {
                                None => {
                                    m.insert(p.clone(), None);
                                    let (pp, mm, ctx) = (p.clone(), self.mini.clone(), ui.ctx().clone());
                                    std::thread::spawn(move || {
                                        let i = image::open(&pp).ok().map(|i| {
                                            let t = i.resize_to_fill(384, 216, image::imageops::FilterType::Triangle).to_rgba8();
                                            egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw())
                                        });
                                        mm.lock().unwrap().insert(pp, i);
                                        ctx.request_repaint();
                                    });
                                }
                                Some(Some(_)) => {
                                    if let Some(Some(img)) = m.remove(p) {
                                        let t = ui.ctx().load_texture(p.display().to_string(), img, Default::default());
                                        self.textures.insert(p.clone(), t);
                                    }
                                }
                                Some(None) => {}
                            }
                        }
                    }
                    if rep.on_hover_text(p.file_name().unwrap_or_default().to_string_lossy()).clicked() {
                        agrandir = Some(p.clone());
                    }
                }
            });
        });
        if let Some(p) = agrandir {
            if let Ok(i) = image::open(&p) {
                let i = i.thumbnail(1920, 1080).to_rgba8();
                let t = ui.ctx().load_texture("capture_grande", egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()), Default::default());
                self.grande = Some((p, t));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 13 · Amis Steam
// ---------------------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Ami {
    pub nom: String,
    pub etat: u64, // 0 hors ligne, 1 en ligne, 2 occupé, 3 absent…
    pub jeu: Option<String>,
    pub avatar: String,
    pub vu: i64,
}

fn amis(cle: &str, sid: &str) -> Result<Vec<Ami>, String> {
    let l = json(&format!("https://api.steampowered.com/ISteamUser/GetFriendList/v1/?key={cle}&steamid={sid}&relationship=friend"))
        .map_err(|e| if e.contains("401") || e.contains("403") { "clé refusée, ou liste d'amis privée".to_string() } else { e })?;
    let ids: Vec<String> = l["friendslist"]["friends"].as_array().into_iter().flatten().filter_map(|f| f["steamid"].as_str().map(String::from)).collect();
    let mut v = vec![];
    for paquet in ids.chunks(100) {
        let j = json(&format!("https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v2/?key={cle}&steamids={}", paquet.join(",")))?;
        for p in j["response"]["players"].as_array().into_iter().flatten() {
            v.push(Ami {
                nom: p["personaname"].as_str().unwrap_or("?").to_string(),
                etat: p["personastate"].as_u64().unwrap_or(0),
                jeu: p["gameextrainfo"].as_str().map(String::from),
                avatar: p["avatarmedium"].as_str().unwrap_or("").to_string(),
                vu: p["lastlogoff"].as_i64().unwrap_or(0),
            });
        }
    }
    v.sort_by(|a, b| b.jeu.is_some().cmp(&a.jeu.is_some()).then((b.etat > 0).cmp(&(a.etat > 0))).then(a.nom.to_lowercase().cmp(&b.nom.to_lowercase())));
    Ok(v)
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(default)]
struct ReglageAmis {
    cle: String,
    prevenir: bool,
}

pub struct AmisSteam {
    r: Arc<Mutex<ReglageAmis>>,
    l: Arc<Mutex<Option<Result<Vec<Ami>, String>>>>,
    voir_cle: bool,
}

impl AmisSteam {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageAmis>> = Arc::new(Mutex::new(charger("amis_steam")));
        let l: Arc<Mutex<Option<Result<Vec<Ami>, String>>>> = Arc::default();
        let (r2, l2) = (r.clone(), l.clone());
        std::thread::spawn(move || {
            let mut jouaient: HashMap<String, String> = HashMap::new();
            let mut premier = true;
            loop {
                let c = r2.lock().unwrap().clone();
                if let (false, Some(sid)) = (c.cle.trim().is_empty(), crate::promos::steamid_local()) {
                    let res = amis(c.cle.trim(), &sid);
                    if let Ok(v) = &res {
                        for a in v {
                            if let Some(j) = &a.jeu {
                                if c.prevenir && !premier && jouaient.get(&a.nom) != Some(j) {
                                    crate::systeme::notifier("Ami Steam", &format!("{} joue à {j}", a.nom));
                                }
                            }
                        }
                        jouaient = v.iter().filter_map(|a| Some((a.nom.clone(), a.jeu.clone()?))).collect();
                        premier = false;
                    }
                    *l2.lock().unwrap() = Some(res);
                }
                std::thread::sleep(Duration::from_secs(90));
            }
        });
        AmisSteam { r, l, voir_cle: false }
    }
}

impl Outil for AmisSteam {
    fn nom(&self) -> &'static str {
        "👥 Amis Steam"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn resume(&self) -> Option<String> {
        let l = self.l.lock().unwrap();
        let v = l.as_ref()?.as_ref().ok()?;
        let n = v.iter().filter(|a| a.jeu.is_some()).count();
        (n > 0).then(|| format!("👥 {n} ami(s) en jeu : {}", v.iter().filter(|a| a.jeu.is_some()).take(3).map(|a| a.nom.as_str()).collect::<Vec<_>>().join(", ")))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Amis Steam");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        if r.cle.trim().is_empty() || self.voir_cle {
            ui.label("Il faut une clé Steam (gratuite, 1 minute) : connecte-toi sur la page ci-dessous, mets « localhost » comme nom de domaine, puis colle la clé ici.");
            if ui.link("steamcommunity.com/dev/apikey").clicked() {
                let _ = std::process::Command::new("explorer").arg("https://steamcommunity.com/dev/apikey").spawn();
            }
            ui.add(egui::TextEdit::singleline(&mut r.cle).password(true).hint_text("clé Steam").desired_width(320.0));
            ui.weak("La clé reste sur ton PC. Ta liste d'amis doit être publique ou visible par toi.");
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.prevenir, "Me prévenir quand un ami lance un jeu");
            if !r.cle.is_empty() && ui.small_button(if self.voir_cle { "Masquer la clé" } else { "Changer la clé" }).clicked() {
                self.voir_cle = !self.voir_cle;
            }
        });
        if r != avant {
            enregistrer("amis_steam", &r);
            *self.r.lock().unwrap() = r.clone();
        }
        if r.cle.trim().is_empty() {
            return;
        }
        if crate::promos::steamid_local().is_none() {
            ui.colored_label(Color32::LIGHT_RED, "Steam n'est pas installé ou aucun compte n'y est connecté.");
            return;
        }
        match self.l.lock().unwrap().clone() {
            None => {
                ui.spinner();
                ui.weak("chargement (toutes les 90 s)…");
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            Some(Ok(v)) => {
                ui.label(format!("{} en jeu · {} en ligne · {} amis", v.iter().filter(|a| a.jeu.is_some()).count(), v.iter().filter(|a| a.etat > 0).count(), v.len()));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for a in &v {
                        ui.horizontal(|ui| {
                            if !a.avatar.is_empty() {
                                ui.add(egui::Image::new(a.avatar.as_str()).fit_to_exact_size(egui::vec2(32.0, 32.0)));
                            }
                            let (c, t) = match (&a.jeu, a.etat) {
                                (Some(j), _) => (Color32::LIGHT_GREEN, format!("🎮 {j}")),
                                (None, 0) => (Color32::GRAY, format!("hors ligne{}", if a.vu > 0 { format!(" · vu le {}", date_courte(a.vu)) } else { String::new() })),
                                (None, 3 | 4) => (Color32::from_rgb(250, 210, 90), "absent".into()),
                                (None, 2) => (Color32::LIGHT_RED, "occupé".into()),
                                _ => (Color32::from_rgb(120, 170, 255), "en ligne".into()),
                            };
                            ui.label(RichText::new(&a.nom).strong());
                            ui.colored_label(c, t);
                        });
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 15 · Générateur de pseudos
// ---------------------------------------------------------------------------------------------------

const ADJ: &[&str] = &["Shadow", "Crimson", "Silent", "Neon", "Frost", "Lunar", "Wild", "Dark", "Swift", "Iron", "Mystic", "Toxic", "Cosmic", "Savage", "Golden", "Pixel", "Turbo", "Cyber", "Rogue", "Sakura", "Velvet", "Storm", "Ghost", "Atomic", "Lucky"];
const NOMS: &[&str] = &["Wolf", "Fox", "Ninja", "Dragon", "Phoenix", "Raven", "Tiger", "Viper", "Samurai", "Panda", "Falcon", "Reaper", "Knight", "Kitsune", "Blade", "Comet", "Hunter", "Sniper", "Wizard", "Golem", "Spectre", "Koi", "Onyx", "Yeti", "Kraken"];
const FR: &[&str] = &["Lune", "Brume", "Renard", "Loup", "Éclair", "Ombre", "Flamme", "Givre", "Tempête", "Sabre", "Nuage", "Étoile", "Panthère", "Corbeau", "Comète"];
const JP: &[&str] = &["Kage", "Tora", "Ryu", "Yuki", "Hana", "Kaze", "Sora", "Hoshi", "Kitsu", "Neko", "Raijin", "Oni", "Tsuki", "Kumo", "Akuma"];

#[derive(Clone, Copy, PartialEq)]
pub enum Style {
    Classique,
    Francais,
    Japonais,
    Deco,
    Leet,
}

pub fn leet(s: &str) -> String {
    s.chars().map(|c| match c.to_ascii_lowercase() {
        'a' => '4',
        'e' => '3',
        'i' => '1',
        'o' => '0',
        's' => '5',
        't' => '7',
        _ => c,
    }).collect()
}

pub fn pseudo(style: Style, base: &str, r: &mut impl FnMut(usize) -> usize) -> String {
    let p = |l: &[&str], r: &mut dyn FnMut(usize) -> usize| l[r(l.len())].to_string();
    let base = base.trim();
    let mot = |r: &mut dyn FnMut(usize) -> usize| if base.is_empty() { p(NOMS, r) } else { base.to_string() };
    match style {
        Style::Classique => format!("{}{}{}", p(ADJ, r), mot(r), if r(2) == 0 { (r(99) + 1).to_string() } else { String::new() }),
        Style::Francais => format!("{}{}", p(FR, r), if base.is_empty() { p(&["Noire", "Sauvage", "Rapide", "Rose", "Dorée", "Silencieuse", "Céleste"], r) } else { base.to_string() }),
        Style::Japonais => format!("{}{}", p(JP, r), if base.is_empty() { p(JP, r).to_lowercase() } else { base.to_string() }),
        Style::Deco => {
            let (a, b) = [("xX_", "_Xx"), ("〖", "〗"), ("ツ", "ツ"), ("☆", "☆"), ("•", "•"), ("『", "』")][r(6)];
            format!("{a}{}{}{b}", p(ADJ, r), mot(r))
        }
        Style::Leet => leet(&format!("{}{}", p(ADJ, r), mot(r))),
    }
}

pub struct Pseudos {
    style: Style,
    base: String,
    max: usize,
    liste: Vec<String>,
    gardes: Vec<String>,
}

impl Pseudos {
    pub fn new() -> Self {
        Pseudos { style: Style::Classique, base: String::new(), max: 16, liste: vec![], gardes: charger("pseudos") }
    }

    fn generer(&mut self) {
        let mut r = |n: usize| hasard(n as u64) as usize;
        let mut v = vec![];
        for _ in 0..200 {
            let s = pseudo(self.style, &self.base, &mut r);
            if s.chars().count() <= self.max && !v.contains(&s) {
                v.push(s);
            }
            if v.len() >= 12 {
                break;
            }
        }
        self.liste = v;
    }
}

impl Outil for Pseudos {
    fn nom(&self) -> &'static str {
        "🏷 Générateur de pseudos"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Générateur de pseudos");
        ui.horizontal_wrapped(|ui| {
            for (s, n) in [(Style::Classique, "🎮 Classique"), (Style::Francais, "🇫🇷 Français"), (Style::Japonais, "🌸 Japonais"), (Style::Deco, "✨ Décoré"), (Style::Leet, "💻 L33t")] {
                if ui.selectable_value(&mut self.style, s, n).clicked() {
                    self.generer();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.base).hint_text("mot à garder (facultatif)").desired_width(180.0));
            ui.label("max");
            ui.add(egui::DragValue::new(&mut self.max).range(6..=32).suffix(" lettres"));
            if ui.button(RichText::new("🎲 Générer").size(16.0)).clicked() || self.liste.is_empty() {
                self.generer();
            }
        });
        ui.separator();
        let mut garder = None;
        egui::Grid::new("pseudos").num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
            for (k, p) in self.liste.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(p).size(17.0).monospace());
                    if ui.small_button("📋").clicked() {
                        ui.ctx().copy_text(p.clone());
                    }
                    if ui.small_button("⭐").clicked() {
                        garder = Some(p.clone());
                    }
                });
                if k % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        if let Some(p) = garder {
            if !self.gardes.contains(&p) {
                self.gardes.push(p);
                enregistrer("pseudos", &self.gardes);
            }
        }
        if !self.gardes.is_empty() {
            ui.separator();
            ui.strong("⭐ Gardés");
            let mut retirer = None;
            ui.horizontal_wrapped(|ui| {
                for (k, p) in self.gardes.iter().enumerate() {
                    let b = ui.button(p).on_hover_text("Clic : copier · clic droit : retirer");
                    if b.clicked() {
                        ui.ctx().copy_text(p.clone());
                    }
                    if b.secondary_clicked() {
                        retirer = Some(k);
                    }
                }
            });
            if let Some(k) = retirer {
                self.gardes.remove(k);
                enregistrer("pseudos", &self.gardes);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(jeu_du_fichier("Elden Ring 2024-03-02 21-14-33"), Some("Elden Ring".into()));
        assert_eq!(jeu_du_fichier("Screenshot"), None);
        assert_eq!(texte_propre("[h1]Patch 1.2[/h1] [list][*]Fix &amp; more[/list] <br>ok"), "Patch 1.2 Fix & more ok");
        assert_eq!(leet("Toxic Fox"), "70x1c F0x");
        let mut k = 0;
        let mut r = |n: usize| {
            k += 1;
            k % n
        };
        for s in [Style::Classique, Style::Francais, Style::Japonais, Style::Deco, Style::Leet] {
            assert!(!pseudo(s, "", &mut r).is_empty());
        }
        assert!(pseudo(Style::Classique, "Lea", &mut r).contains("Lea"));
    }
}
