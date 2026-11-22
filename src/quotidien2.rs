//! Quotidien, suite : raccourcis de texte (52), météo (53), agenda (54), dictée vocale (56),
//! eau et calories (64). (61 · presse-papiers vers le téléphone : dans l'onglet Téléphone.)
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------- dates (calendrier grégorien, sans dépendance) ----------

/// Jours depuis le 1970-01-01 (algorithme de H. Hinnant).
pub fn jours_depuis_1970(a: i32, m: u32, j: u32) -> i64 {
    let a = if m <= 2 { a - 1 } else { a } as i64;
    let ere = a.div_euclid(400);
    let ya = a - ere * 400;
    let mp = (m as i64 + 9) % 12;
    let jda = (153 * mp + 2) / 5 + j as i64 - 1;
    let jde = ya * 365 + ya / 4 - ya / 100 + jda;
    ere * 146_097 + jde - 719_468
}

pub fn date_de_jours(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let ere = z.div_euclid(146_097);
    let jde = z - ere * 146_097;
    let ya = (jde - jde / 1460 + jde / 36_524 - jde / 146_096) / 365;
    let jda = jde - (365 * ya + ya / 4 - ya / 100);
    let mp = (5 * jda + 2) / 153;
    let j = (jda - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((ya + ere * 400 + (m <= 2) as i64) as i32, m, j)
}

/// 0 = lundi … 6 = dimanche.
pub fn jour_semaine(a: i32, m: u32, j: u32) -> u32 {
    (jours_depuis_1970(a, m, j) + 3).rem_euclid(7) as u32
}

pub fn jours_dans_mois(a: i32, m: u32) -> u32 {
    let (a2, m2) = if m == 12 { (a + 1, 1) } else { (a, m + 1) };
    (jours_depuis_1970(a2, m2, 1) - jours_depuis_1970(a, m, 1)) as u32
}

pub fn texte_date(a: i32, m: u32, j: u32) -> String {
    format!("{a:04}-{m:02}-{j:02}")
}

pub fn lire_date(s: &str) -> Option<(i32, u32, u32)> {
    let mut p = s.splitn(3, '-');
    Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.get(..2)?.parse().ok()?))
}

/// (date AAAA-MM-JJ, minutes depuis minuit) maintenant, heure locale.
fn maintenant() -> (String, u32) {
    let h = crate::minecraft::horodatage(); // AAAA-MM-JJ_HH-MM-SS
    let n = |a: usize| h.get(a..a + 2).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    (h.get(..10).unwrap_or("").to_string(), n(11) * 60 + n(14))
}

const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];

// ---------------------------------------------------------------------------------------------------
// 52 · Raccourcis de texte
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Raccourci {
    pub abrev: String,
    pub texte: String,
}

/// Texte final : {date}, {heure} remplacés.
pub fn developper(t: &str, date: &str, heure: &str) -> String {
    t.replace("{date}", date).replace("{heure}", heure)
}

/// Abréviation qui termine le tampon (la plus longue).
pub fn trouver<'a>(tampon: &str, l: &'a [Raccourci]) -> Option<&'a Raccourci> {
    l.iter().filter(|r| r.abrev.chars().count() >= 2 && tampon.ends_with(&r.abrev)).max_by_key(|r| r.abrev.len())
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
mod crochet {
    use super::*;
    use std::ptr::null_mut;
    use std::sync::mpsc::{channel, Sender};
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const MARQUE: usize = 0x0075_7452;
    pub static LISTE: Mutex<Vec<Raccourci>> = Mutex::new(Vec::new());
    pub static ACTIF: AtomicBool = AtomicBool::new(true);
    static TAMPON: Mutex<String> = Mutex::new(String::new());
    static ENVOI: Mutex<Option<Sender<(usize, String)>>> = Mutex::new(None);

    fn touche(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: MARQUE } } }
    }

    fn remplacer(effacer: usize, texte: &str) {
        let mut seq = vec![];
        for _ in 0..effacer {
            seq.extend([touche(VK_BACK, 0, 0), touche(VK_BACK, 0, KEYEVENTF_KEYUP)]);
        }
        for c in texte.replace("\r\n", "\n").encode_utf16() {
            if c == '\n' as u16 {
                seq.extend([touche(VK_RETURN, 0, 0), touche(VK_RETURN, 0, KEYEVENTF_KEYUP)]);
            } else {
                seq.extend([touche(0, c, KEYEVENTF_UNICODE), touche(0, c, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)]);
            }
        }
        unsafe { SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    }

    unsafe extern "system" fn clavier(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 && matches!(wp as u32, WM_KEYDOWN | WM_SYSKEYDOWN) && ACTIF.load(Relaxed) {
            let k = &*(lp as *const KBDLLHOOKSTRUCT);
            if k.dwExtraInfo != MARQUE {
                let mut t = TAMPON.lock().unwrap();
                let ctrl = GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_RMENU as i32) >= 0;
                match k.vkCode as u16 {
                    VK_BACK => {
                        t.pop();
                    }
                    VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CAPITAL | VK_RMENU | VK_LMENU | VK_MENU | VK_CONTROL | VK_LCONTROL | VK_RCONTROL => {}
                    _ if ctrl => t.clear(),
                    vk => {
                        let mut etat = [0u8; 256];
                        for v in [VK_SHIFT, VK_CONTROL, VK_MENU, VK_RMENU, VK_LCONTROL] {
                            if GetAsyncKeyState(v as i32) < 0 {
                                etat[v as usize] = 0x80;
                            }
                        }
                        etat[VK_CAPITAL as usize] = (GetKeyState(VK_CAPITAL as i32) & 1) as u8;
                        let mut b = [0u16; 8];
                        let fen = GetForegroundWindow();
                        let disp = GetKeyboardLayout(GetWindowThreadProcessId(fen, null_mut()));
                        // drapeau 4 : ne touche pas à l'état des touches mortes
                        let n = ToUnicodeEx(vk as u32, k.scanCode, etat.as_ptr(), b.as_mut_ptr(), 8, 4, disp);
                        if n > 0 {
                            let s = String::from_utf16_lossy(&b[..n as usize]);
                            if s.chars().any(|c| c.is_control()) {
                                t.clear();
                            } else {
                                t.push_str(&s);
                                let l = LISTE.lock().unwrap();
                                if let Some(r) = trouver(&t, &l) {
                                    if let Some(e) = ENVOI.lock().unwrap().as_ref() {
                                        let _ = e.send((r.abrev.chars().count() - 1, r.texte.clone()));
                                    }
                                    t.clear();
                                    // le dernier caractère de l'abréviation n'est pas tapé : on l'avale
                                    return 1;
                                }
                                let nb = t.chars().count();
                                if nb > 40 {
                                    *t = t.chars().skip(nb - 40).collect();
                                }
                            }
                        } else {
                            t.clear();
                        }
                    }
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }

    pub fn lancer() {
        let (tx, rx) = channel::<(usize, String)>();
        *ENVOI.lock().unwrap() = Some(tx);
        std::thread::spawn(move || {
            for (n, t) in rx {
                let (d, m) = maintenant();
                let date = lire_date(&d).map(|(a, mo, j)| format!("{j:02}/{mo:02}/{a}")).unwrap_or(d);
                std::thread::sleep(Duration::from_millis(20));
                remplacer(n, &developper(&t, &date, &format!("{}:{:02}", m / 60, m % 60)));
            }
        });
        std::thread::spawn(|| unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(clavier), null_mut(), 0);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
        });
    }
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
pub struct Raccourcis {
    liste: Vec<Raccourci>,
    abrev: String,
    texte: String,
    actif: bool,
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
impl Raccourcis {
    pub fn new() -> Self {
        let mut liste: Vec<Raccourci> = charger("raccourcis_texte");
        if liste.is_empty() {
            liste = vec![
                Raccourci { abrev: ";date".into(), texte: "{date}".into() },
                Raccourci { abrev: ";bjr".into(), texte: "Bonjour,\n\n".into() },
                Raccourci { abrev: ";cdt".into(), texte: "Cordialement,".into() },
            ];
        }
        let actif = charger::<Option<bool>>("raccourcis_texte_actif").unwrap_or(true);
        crochet::ACTIF.store(actif, Relaxed);
        *crochet::LISTE.lock().unwrap() = liste.clone();
        crochet::lancer();
        Raccourcis { liste, abrev: ";".into(), texte: String::new(), actif }
    }
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
impl Outil for Raccourcis {
    fn nom(&self) -> &'static str {
        "✍ Raccourcis de texte"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Raccourcis de texte");
        ui.weak("Tape une abréviation n'importe où (Discord, navigateur, Word…) et elle est remplacée par le texte complet. Astuce : commence-les par « ; » pour ne jamais les taper par erreur.");
        if ui.checkbox(&mut self.actif, "Actif").changed() {
            crochet::ACTIF.store(self.actif, Relaxed);
            enregistrer("raccourcis_texte_actif", &Some(self.actif));
        }
        let avant = self.liste.clone();
        let mut retirer = None;
        egui::Grid::new("raccourcis").striped(true).num_columns(3).show(ui, |ui| {
            for (i, r) in self.liste.iter_mut().enumerate() {
                ui.add(egui::TextEdit::singleline(&mut r.abrev).desired_width(90.0).font(egui::TextStyle::Monospace));
                ui.add(egui::TextEdit::multiline(&mut r.texte).desired_rows(1).desired_width(380.0));
                if ui.small_button("🗑").clicked() {
                    retirer = Some(i);
                }
                ui.end_row();
            }
        });
        if let Some(i) = retirer {
            self.liste.remove(i);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.abrev).desired_width(90.0).hint_text(";abc").font(egui::TextStyle::Monospace));
            ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(1).desired_width(380.0).hint_text("texte complet"));
            if ui.button("➕").clicked() && self.abrev.chars().count() >= 2 && !self.texte.is_empty() {
                self.liste.push(Raccourci { abrev: std::mem::replace(&mut self.abrev, ";".into()), texte: std::mem::take(&mut self.texte) });
            }
        });
        ui.weak("{date} et {heure} sont remplacés par la date et l'heure du moment. Les retours à la ligne sont tapés aussi.");
        if self.liste != avant {
            enregistrer("raccourcis_texte", &self.liste);
            *crochet::LISTE.lock().unwrap() = self.liste.clone();
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 53 · Météo
// ---------------------------------------------------------------------------------------------------

/// (emoji, description) d'un code météo WMO.
pub fn code_meteo(c: u32) -> (&'static str, &'static str) {
    match c {
        0 => ("☀", "ciel dégagé"),
        1 => ("🌤", "plutôt dégagé"),
        2 => ("⛅", "partiellement nuageux"),
        3 => ("☁", "couvert"),
        45 | 48 => ("🌫", "brouillard"),
        51 | 53 | 55 | 56 | 57 => ("🌦", "bruine"),
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => ("🌧", "pluie"),
        71 | 73 | 75 | 77 | 85 | 86 => ("❄", "neige"),
        95 | 96 | 99 => ("⛈", "orage"),
        _ => ("🌡", "?"),
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Ville {
    pub nom: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Clone, Default)]
struct Previsions {
    temp: f64,
    ressenti: f64,
    vent: f64,
    code: u32,
    jours: Vec<(String, u32, f64, f64, u32)>, // date, code, min, max, % pluie
    heures: Vec<(String, f64, u32)>,          // heure, temp, % pluie
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("Outils").build()
}

fn previsions(v: &Ville) -> Result<Previsions, String> {
    let j: serde_json::Value = agent().get("https://api.open-meteo.com/v1/forecast")
        .query("latitude", &v.lat.to_string()).query("longitude", &v.lon.to_string())
        .query("current", "temperature_2m,apparent_temperature,weather_code,wind_speed_10m")
        .query("hourly", "temperature_2m,precipitation_probability")
        .query("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max")
        .query("timezone", "auto").query("forecast_days", "5")
        .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let c = &j["current"];
    let d = &j["daily"];
    let f = |x: &serde_json::Value| x.as_f64().unwrap_or(0.0);
    let jours = (0..d["time"].as_array().map_or(0, |a| a.len())).map(|i| {
        (d["time"][i].as_str().unwrap_or("").to_string(), d["weather_code"][i].as_u64().unwrap_or(0) as u32, f(&d["temperature_2m_min"][i]), f(&d["temperature_2m_max"][i]), d["precipitation_probability_max"][i].as_u64().unwrap_or(0) as u32)
    }).collect();
    let h = &j["hourly"];
    let maint = c["time"].as_str().unwrap_or("").get(..13).unwrap_or("").to_string();
    let debut = h["time"].as_array().and_then(|a| a.iter().position(|t| t.as_str().is_some_and(|t| t.starts_with(&maint)))).unwrap_or(0);
    let heures = (debut..debut + 12).filter_map(|i| Some((h["time"][i].as_str()?.get(11..13)?.to_string() + "h", h["temperature_2m"][i].as_f64()?, h["precipitation_probability"][i].as_u64().unwrap_or(0) as u32))).collect();
    Ok(Previsions { temp: f(&c["temperature_2m"]), ressenti: f(&c["apparent_temperature"]), vent: f(&c["wind_speed_10m"]), code: c["weather_code"].as_u64().unwrap_or(0) as u32, jours, heures })
}

fn chercher_villes(q: &str) -> Result<Vec<Ville>, String> {
    let j: serde_json::Value = agent().get("https://geocoding-api.open-meteo.com/v1/search").query("name", q).query("count", "6").query("language", "fr")
        .call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(j["results"].as_array().into_iter().flatten().map(|r| Ville {
        nom: format!("{}{}", r["name"].as_str().unwrap_or("?"), r["admin1"].as_str().or(r["country"].as_str()).map(|x| format!(", {x}")).unwrap_or_default()),
        lat: r["latitude"].as_f64().unwrap_or(0.0),
        lon: r["longitude"].as_f64().unwrap_or(0.0),
    }).collect())
}

pub struct Meteo {
    ville: Arc<Mutex<Ville>>,
    p: Arc<Mutex<Option<Result<Previsions, String>>>>,
    recherche: String,
    trouvees: Arc<Mutex<Vec<Ville>>>,
    rafraichir: Arc<AtomicBool>,
}

impl Meteo {
    pub fn new() -> Self {
        let ville: Arc<Mutex<Ville>> = Arc::new(Mutex::new(charger("meteo_ville")));
        let p: Arc<Mutex<Option<Result<Previsions, String>>>> = Arc::default();
        let rafraichir = Arc::new(AtomicBool::new(true));
        let (v, pp, r) = (ville.clone(), p.clone(), rafraichir.clone());
        std::thread::spawn(move || {
            let mut t = 0u32;
            loop {
                let vv = v.lock().unwrap().clone();
                if !vv.nom.is_empty() && (r.swap(false, Relaxed) || t >= 1800) {
                    t = 0;
                    let res = previsions(&vv);
                    let ok = res.is_ok();
                    if ok || pp.lock().unwrap().is_none() {
                        *pp.lock().unwrap() = Some(res);
                    }
                    if !ok {
                        t = 1500; // nouvel essai dans 5 min
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
                t += 2;
            }
        });
        Meteo { ville, p, recherche: String::new(), trouvees: Arc::default(), rafraichir }
    }
}

impl Outil for Meteo {
    fn nom(&self) -> &'static str {
        "🌤 Météo"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let v = self.ville.lock().unwrap().nom.clone();
        match self.p.lock().unwrap().as_ref()? {
            Ok(p) => {
                let (e, d) = code_meteo(p.code);
                let pluie = p.jours.first().map(|j| j.4).unwrap_or(0);
                Some(format!("{e} {:.0} °C, {d} à {} · pluie {pluie} % aujourd'hui", p.temp, v.split(',').next().unwrap_or(&v)))
            }
            Err(_) => None,
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Météo");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("ville").desired_width(200.0));
            if (ui.button("🔍").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.recherche.trim().is_empty() {
                let (q, t) = (self.recherche.trim().to_string(), self.trouvees.clone());
                std::thread::spawn(move || *t.lock().unwrap() = chercher_villes(&q).unwrap_or_default());
            }
            if ui.small_button("⟳").clicked() {
                self.rafraichir.store(true, Relaxed);
            }
        });
        let t = self.trouvees.lock().unwrap().clone();
        if !t.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for v in t {
                    if ui.button(&v.nom).clicked() {
                        enregistrer("meteo_ville", &v);
                        *self.ville.lock().unwrap() = v;
                        *self.p.lock().unwrap() = None;
                        self.trouvees.lock().unwrap().clear();
                        self.recherche.clear();
                        self.rafraichir.store(true, Relaxed);
                    }
                }
            });
        }
        let v = self.ville.lock().unwrap().clone();
        if v.nom.is_empty() {
            ui.label("Cherche ta ville pour afficher la météo (elle apparaîtra aussi sur l'Accueil).");
            return;
        }
        ui.label(RichText::new(&v.nom).strong());
        match self.p.lock().unwrap().clone() {
            None => {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::LIGHT_RED, format!("Météo indisponible : {e}"));
            }
            Some(Ok(p)) => {
                let (e, d) = code_meteo(p.code);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(e).size(48.0));
                    ui.vertical(|ui| {
                        ui.label(RichText::new(format!("{:.0} °C", p.temp)).size(32.0).strong());
                        ui.label(format!("{d} · ressenti {:.0} °C · vent {:.0} km/h", p.ressenti, p.vent));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for (h, t, pl) in &p.heures {
                        ui.vertical(|ui| {
                            ui.weak(h);
                            ui.label(format!("{t:.0}°"));
                            if *pl >= 30 {
                                ui.colored_label(Color32::from_rgb(120, 170, 255), format!("{pl}%"));
                            }
                        });
                    }
                });
                ui.separator();
                egui::Grid::new("jours_meteo").spacing([18.0, 6.0]).show(ui, |ui| {
                    for (k, (date, c, mi, ma, pl)) in p.jours.iter().enumerate() {
                        let nom = if k == 0 { "Aujourd'hui".into() } else { lire_date(date).map(|(a, m, j)| format!("{} {j}", JOURS[jour_semaine(a, m, j) as usize])).unwrap_or(date.clone()) };
                        ui.label(nom);
                        ui.label(RichText::new(code_meteo(*c).0).size(20.0));
                        ui.label(format!("{mi:.0}° / {ma:.0}°"));
                        ui.label(format!("☔ {pl} %"));
                        ui.end_row();
                    }
                });
                ui.weak("Données Open-Meteo, mises à jour toutes les 30 minutes.");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 54 · Agenda
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
pub enum Repete {
    #[default]
    Jamais,
    Semaine,
    Mois,
    Annee,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Evenement {
    pub id: u64,
    pub date: String,
    pub heure: Option<u32>, // minutes depuis minuit
    pub titre: String,
    pub rappel: u32,        // minutes avant
    pub repete: Repete,
    pub prevenu: String,    // date de la dernière occurrence notifiée
}

/// L'événement a-t-il lieu ce jour-là ?
pub fn a_lieu(e: &Evenement, date: &str) -> bool {
    let (Some(d0), Some(d)) = (lire_date(&e.date), lire_date(date)) else { return false };
    let (n0, n) = (jours_depuis_1970(d0.0, d0.1, d0.2), jours_depuis_1970(d.0, d.1, d.2));
    if n < n0 {
        return false;
    }
    match e.repete {
        Repete::Jamais => n == n0,
        Repete::Semaine => (n - n0) % 7 == 0,
        Repete::Mois => d.2 == d0.2,
        Repete::Annee => d.1 == d0.1 && d.2 == d0.2,
    }
}

fn texte_heure(h: Option<u32>) -> String {
    h.map(|m| format!("{}h{:02}", m / 60, m % 60)).unwrap_or_else(|| "journée".into())
}

pub struct Agenda {
    ev: Arc<Mutex<Vec<Evenement>>>,
    mois: (i32, u32),
    choisi: String,
    nouveau: Evenement,
    heure_txt: String,
}

impl Agenda {
    pub fn new() -> Self {
        let ev: Arc<Mutex<Vec<Evenement>>> = Arc::new(Mutex::new(charger("agenda")));
        let e2 = ev.clone();
        std::thread::spawn(move || loop {
            let (d, m) = maintenant();
            let mut l = e2.lock().unwrap();
            let mut change = false;
            for e in l.iter_mut() {
                let quand = e.heure.unwrap_or(8 * 60); // événement « journée » : rappel à 8 h
                if e.prevenu != d && a_lieu(e, &d) && m + e.rappel >= quand && m <= quand + 60 {
                    let t = if e.rappel > 0 && m < quand { format!("Dans {} min ({})", quand - m, texte_heure(e.heure)) } else { texte_heure(e.heure) };
                    crate::systeme::notifier_toujours(&format!("📅 {}", e.titre), &t);
                    e.prevenu = d.clone();
                    change = true;
                }
            }
            if change {
                enregistrer("agenda", &*l);
            }
            drop(l);
            std::thread::sleep(Duration::from_secs(30));
        });
        let (d, _) = maintenant();
        let (a, m, _) = lire_date(&d).unwrap_or((2026, 1, 1));
        Agenda { ev, mois: (a, m), choisi: d, nouveau: Evenement { rappel: 15, ..Default::default() }, heure_txt: String::new() }
    }
}

impl Outil for Agenda {
    fn nom(&self) -> &'static str {
        "📅 Agenda"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let (d, m) = maintenant();
        let l = self.ev.lock().unwrap();
        let mut v: Vec<&Evenement> = l.iter().filter(|e| a_lieu(e, &d) && e.heure.map_or(true, |h| h + 60 >= m)).collect();
        v.sort_by_key(|e| e.heure);
        (!v.is_empty()).then(|| format!("Aujourd'hui : {}", v.iter().map(|e| format!("{} {}", texte_heure(e.heure), e.titre)).collect::<Vec<_>>().join(" · ")))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Agenda");
        let (auj, _) = maintenant();
        let (a, m) = self.mois;
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                self.mois = if m == 1 { (a - 1, 12) } else { (a, m - 1) };
            }
            ui.label(RichText::new(format!("{} {a}", MOIS[m as usize - 1])).size(18.0).strong());
            if ui.button("▶").clicked() {
                self.mois = if m == 12 { (a + 1, 1) } else { (a, m + 1) };
            }
            if ui.small_button("Aujourd'hui").clicked() {
                if let Some((a, m, _)) = lire_date(&auj) {
                    self.mois = (a, m);
                }
                self.choisi = auj.clone();
            }
        });
        let ev = self.ev.lock().unwrap().clone();
        let premier = jour_semaine(a, m, 1);
        let nb = jours_dans_mois(a, m);
        egui::Grid::new("calendrier").spacing([4.0, 4.0]).show(ui, |ui| {
            for j in ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"] {
                ui.weak(j);
            }
            ui.end_row();
            for _ in 0..premier {
                ui.label("");
            }
            for j in 1..=nb {
                let d = texte_date(a, m, j);
                let n = ev.iter().filter(|e| a_lieu(e, &d)).count();
                let mut t = RichText::new(format!("{j:>2}{}", if n > 0 { " •" } else { "  " })).monospace();
                if d == auj {
                    t = t.color(Color32::from_rgb(255, 120, 170)).strong();
                }
                if ui.add(egui::Button::new(t).selected(d == self.choisi).min_size(egui::vec2(52.0, 34.0))).clicked() {
                    self.choisi = d.clone();
                }
                if (premier + j) % 7 == 0 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
        let titre = lire_date(&self.choisi).map(|(a, m, j)| format!("{} {j} {} {a}", JOURS[jour_semaine(a, m, j) as usize], MOIS[m as usize - 1])).unwrap_or_default();
        ui.strong(titre);
        let mut du_jour: Vec<&Evenement> = ev.iter().filter(|e| a_lieu(e, &self.choisi)).collect();
        du_jour.sort_by_key(|e| e.heure);
        let mut retirer = None;
        for e in &du_jour {
            ui.horizontal(|ui| {
                ui.label(RichText::new(texte_heure(e.heure)).monospace());
                ui.label(&e.titre);
                if e.repete != Repete::Jamais {
                    ui.weak("🔁");
                }
                if ui.small_button("🗑").clicked() {
                    retirer = Some(e.id);
                }
            });
        }
        if du_jour.is_empty() {
            ui.weak("Rien de prévu.");
        }
        if let Some(id) = retirer {
            let mut l = self.ev.lock().unwrap();
            l.retain(|e| e.id != id);
            enregistrer("agenda", &*l);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.titre).hint_text("nouvel événement").desired_width(200.0));
            ui.add(egui::TextEdit::singleline(&mut self.heure_txt).hint_text("14h30").desired_width(60.0));
            egui::ComboBox::from_id_salt("rappel").selected_text(match self.nouveau.rappel { 0 => "rappel à l'heure".to_string(), r => format!("rappel {r} min avant") }).show_ui(ui, |ui| {
                for r in [0, 5, 15, 30, 60, 120, 240] {
                    ui.selectable_value(&mut self.nouveau.rappel, r, if r == 0 { "à l'heure".to_string() } else { format!("{r} min avant") });
                }
            });
            egui::ComboBox::from_id_salt("repete").selected_text(match self.nouveau.repete { Repete::Jamais => "une fois", Repete::Semaine => "chaque semaine", Repete::Mois => "chaque mois", Repete::Annee => "chaque année" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.nouveau.repete, Repete::Jamais, "une fois");
                ui.selectable_value(&mut self.nouveau.repete, Repete::Semaine, "chaque semaine");
                ui.selectable_value(&mut self.nouveau.repete, Repete::Mois, "chaque mois");
                ui.selectable_value(&mut self.nouveau.repete, Repete::Annee, "chaque année (anniversaire)");
            });
            if ui.button("➕ Ajouter").clicked() && !self.nouveau.titre.trim().is_empty() {
                let mut e = std::mem::replace(&mut self.nouveau, Evenement { rappel: 15, ..Default::default() });
                e.heure = crate::quotidien2::lire_heure(&self.heure_txt);
                e.date = self.choisi.clone();
                let mut l = self.ev.lock().unwrap();
                e.id = l.iter().map(|x| x.id).max().unwrap_or(0) + 1;
                l.push(e);
                enregistrer("agenda", &*l);
                self.heure_txt.clear();
            }
        });
        ui.weak("Les rappels arrivent en notification Windows, même quand Outils est réduit.");
    }
}

/// « 14h30 », « 14:30 », « 9h », « 14 » → minutes depuis minuit.
pub fn lire_heure(s: &str) -> Option<u32> {
    let s = s.trim().to_lowercase().replace(':', "h");
    if s.is_empty() {
        return None;
    }
    let (h, m) = s.split_once('h').unwrap_or((&s, ""));
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = if m.trim().is_empty() { 0 } else { m.trim().parse().ok()? };
    (h < 24 && m < 60).then_some(h * 60 + m)
}

// ---------------------------------------------------------------------------------------------------
// 56 · Dictée vocale
// ---------------------------------------------------------------------------------------------------

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
fn win_h() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let t = |vk: u16, f: u32| INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: f, time: 0, dwExtraInfo: 0 } } };
    let seq = [t(VK_LWIN, 0), t(b'H' as u16, 0), t(b'H' as u16, KEYEVENTF_KEYUP), t(VK_LWIN, KEYEVENTF_KEYUP)];
    unsafe { SendInput(4, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
pub struct Dictee {
    texte: String,
    focus: bool,
    message: String,
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
impl Dictee {
    pub fn new() -> Self {
        Dictee { texte: charger::<Option<String>>("dictee").unwrap_or_default(), focus: false, message: String::new() }
    }
}

#[cfg(windows)] // propre au PC (pas dans l'appli téléphone)
impl Outil for Dictee {
    fn nom(&self) -> &'static str {
        "🎙 Dictée vocale"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dictée vocale");
        ui.weak("Parle, Windows écrit (ponctuation comprise : dis « virgule », « point », « à la ligne »). Fonctionne aussi partout ailleurs avec Win+H.");
        ui.horizontal(|ui| {
            if ui.button(RichText::new("🎙 Dicter").size(18.0)).clicked() {
                self.focus = true;
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(350));
                    win_h();
                });
            }
            if ui.button("📋 Copier").clicked() {
                ui.ctx().copy_text(self.texte.clone());
                self.message = "Copié.".into();
            }
            if ui.button("💾 Enregistrer en .txt…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Texte", &["txt"]).set_file_name("dictee.txt").save_file() {
                    self.message = std::fs::write(&p, &self.texte).map(|_| format!("Enregistré : {}", p.display())).unwrap_or_else(|e| e.to_string());
                }
            }
            if ui.button("🧹 Effacer").clicked() {
                self.texte.clear();
            }
        });
        let r = ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(14).desired_width(f32::INFINITY));
        if self.focus {
            r.request_focus();
            self.focus = false;
        }
        if r.changed() {
            enregistrer("dictee", &Some(self.texte.clone()));
        }
        ui.label(format!("{} mots", self.texte.split_whitespace().count()));
        ui.weak("Si rien ne se passe : Paramètres Windows → Confidentialité → Voix → activer la reconnaissance vocale en ligne.");
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 64 · Eau et calories
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Jour {
    pub eau: u32,
    pub repas: Vec<(String, u32)>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Sante {
    pub jours: BTreeMap<String, Jour>,
    pub objectif_eau: u32,
    pub objectif_kcal: u32,
    pub rappel_eau: bool,
    pub intervalle: u32,
}

impl Default for Sante {
    fn default() -> Self {
        Sante { jours: BTreeMap::new(), objectif_eau: 8, objectif_kcal: 2200, rappel_eau: false, intervalle: 60 }
    }
}

const ALIMENTS: [(&str, u32); 20] = [
    ("Pomme", 80), ("Banane", 105), ("Yaourt", 90), ("Œuf", 78), ("Pain (tranche)", 80), ("Croissant", 230), ("Café sucré", 25),
    ("Pâtes (assiette)", 350), ("Riz (assiette)", 260), ("Salade composée", 300), ("Sandwich jambon", 350), ("Burger", 550),
    ("Frites (portion)", 365), ("Pizza (part)", 285), ("Kebab", 750), ("Soda 33 cl", 139), ("Chips (45 g)", 240),
    ("Barre chocolatée", 250), ("Steak + légumes", 450), ("Céréales + lait", 250),
];

pub struct EauCalories {
    s: Arc<Mutex<Sante>>,
    nom: String,
    kcal: u32,
}

impl EauCalories {
    pub fn new() -> Self {
        let s: Arc<Mutex<Sante>> = Arc::new(Mutex::new(charger("sante")));
        let s2 = s.clone();
        std::thread::spawn(move || {
            let mut depuis = 0u32;
            loop {
                std::thread::sleep(Duration::from_secs(60));
                depuis += 1;
                let x = s2.lock().unwrap().clone();
                let (d, m) = maintenant();
                let bu = x.jours.get(&d).map(|j| j.eau).unwrap_or(0);
                if x.rappel_eau && depuis >= x.intervalle && (9 * 60..22 * 60).contains(&m) && bu < x.objectif_eau && crate::outil::jeu_actif().is_none() {
                    crate::systeme::notifier_toujours("💧 Pense à boire", &format!("{bu} / {} verres aujourd'hui", x.objectif_eau));
                    depuis = 0;
                }
            }
        });
        EauCalories { s, nom: String::new(), kcal: 0 }
    }
}

impl Outil for EauCalories {
    fn nom(&self) -> &'static str {
        "💧 Eau et calories"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let s = self.s.lock().unwrap();
        let j = s.jours.get(&maintenant().0)?;
        let k: u32 = j.repas.iter().map(|r| r.1).sum();
        Some(format!("💧 {} / {} verres · 🍽 {k} / {} kcal", j.eau, s.objectif_eau, s.objectif_kcal))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Eau et calories");
        let (d, _) = maintenant();
        let avant = self.s.lock().unwrap().clone();
        let mut s = avant.clone();
        let (oe, ok) = (s.objectif_eau, s.objectif_kcal);
        let j = s.jours.entry(d.clone()).or_default();
        ui.strong("💧 Eau");
        ui.horizontal(|ui| {
            for i in 0..oe.max(j.eau) {
                ui.label(RichText::new(if i < j.eau { "🥛" } else { "◻" }).size(24.0));
            }
        });
        ui.horizontal(|ui| {
            if ui.button(RichText::new("➕ Un verre").size(16.0)).clicked() {
                j.eau += 1;
            }
            if ui.small_button("−").clicked() {
                j.eau = j.eau.saturating_sub(1);
            }
            ui.label(format!("{} / {oe} verres (≈ {:.1} L)", j.eau, j.eau as f32 * 0.25));
        });
        ui.separator();
        let total: u32 = j.repas.iter().map(|r| r.1).sum();
        ui.strong(format!("🍽 Calories : {total} / {ok} kcal"));
        ui.add(egui::ProgressBar::new(total as f32 / ok.max(1) as f32).desired_width(400.0));
        let mut retirer = None;
        for (i, (n, k)) in j.repas.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("{n} — {k} kcal"));
                if ui.small_button("✖").clicked() {
                    retirer = Some(i);
                }
            });
        }
        if let Some(i) = retirer {
            j.repas.remove(i);
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("aliment").desired_width(160.0));
            ui.add(egui::DragValue::new(&mut self.kcal).range(0..=5000).suffix(" kcal"));
            if ui.button("➕").clicked() && !self.nom.trim().is_empty() {
                j.repas.push((std::mem::take(&mut self.nom), self.kcal));
                self.kcal = 0;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.weak("Ajout rapide :");
            for (n, k) in ALIMENTS {
                if ui.small_button(format!("{n} {k}")).clicked() {
                    j.repas.push((n.to_string(), k));
                }
            }
        });
        ui.separator();
        ui.strong("7 derniers jours");
        let (a, m, jj) = lire_date(&d).unwrap_or((2026, 1, 1));
        let n0 = jours_depuis_1970(a, m, jj);
        let (r, _) = ui.allocate_exact_size(egui::vec2(420.0, 110.0), egui::Sense::hover());
        for k in 0..7 {
            let (ya, ym, yj) = date_de_jours(n0 - 6 + k);
            let x = s.jours.get(&texte_date(ya, ym, yj)).cloned().unwrap_or_default();
            let kc: u32 = x.repas.iter().map(|r| r.1).sum();
            let l = r.width() / 7.0;
            let base = egui::pos2(r.left() + k as f32 * l, r.bottom() - 16.0);
            let h1 = (kc as f32 / ok.max(1) as f32).min(1.3) * 70.0;
            let h2 = (x.eau as f32 / oe.max(1) as f32).min(1.3) * 70.0;
            ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(base.x + 6.0, base.y - h1), egui::pos2(base.x + l / 2.0, base.y)), 2.0, Color32::from_rgb(255, 120, 170));
            ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(base.x + l / 2.0, base.y - h2), egui::pos2(base.x + l - 6.0, base.y)), 2.0, Color32::from_rgb(120, 170, 255));
            ui.painter().text(egui::pos2(base.x + l / 2.0, r.bottom()), egui::Align2::CENTER_BOTTOM, &JOURS[jour_semaine(ya, ym, yj) as usize][..3], egui::FontId::proportional(11.0), Color32::GRAY);
        }
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(255, 120, 170), "■ calories");
            ui.colored_label(Color32::from_rgb(120, 170, 255), "■ eau");
        });
        ui.separator();
        ui.collapsing("⚙ Réglages", |ui| {
            ui.horizontal(|ui| {
                ui.label("Objectif eau");
                ui.add(egui::DragValue::new(&mut s.objectif_eau).range(1..=20).suffix(" verres"));
                ui.label("Objectif calories");
                ui.add(egui::DragValue::new(&mut s.objectif_kcal).range(800..=6000).suffix(" kcal"));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.rappel_eau, "Me rappeler de boire toutes les");
                ui.add(egui::DragValue::new(&mut s.intervalle).range(15..=240).suffix(" min"));
                ui.weak("(de 9 h à 22 h, pas pendant un jeu)");
            });
        });
        // on ne garde que 90 jours
        while s.jours.len() > 90 {
            let k = s.jours.keys().next().cloned().unwrap();
            s.jours.remove(&k);
        }
        if s != avant {
            enregistrer("sante", &s);
            *self.s.lock().unwrap() = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(jours_depuis_1970(1970, 1, 1), 0);
        assert_eq!(jours_depuis_1970(2000, 3, 1), 11_017);
        for n in [-1000i64, 0, 11_017, 20_000, 20_777] {
            let (a, m, j) = date_de_jours(n);
            assert_eq!(jours_depuis_1970(a, m, j), n);
        }
        assert_eq!(jour_semaine(2026, 11, 20), 4); // vendredi
        assert_eq!(jours_dans_mois(2024, 2), 29);
        assert_eq!(jours_dans_mois(2026, 2), 28);
        assert_eq!(lire_date("2026-11-20"), Some((2026, 11, 20)));
        assert_eq!(lire_heure("14h30"), Some(870));
        assert_eq!(lire_heure("9:05"), Some(545));
        assert_eq!(lire_heure("9h"), Some(540));
        assert_eq!(lire_heure("25h"), None);
    }

    #[test]
    fn agenda_et_raccourcis() {
        let e = Evenement { date: "2026-11-20".into(), repete: Repete::Semaine, ..Default::default() };
        assert!(a_lieu(&e, "2026-11-27"));
        assert!(!a_lieu(&e, "2026-11-26"));
        assert!(!a_lieu(&e, "2026-11-13"));
        let e = Evenement { date: "2000-05-03".into(), repete: Repete::Annee, ..Default::default() };
        assert!(a_lieu(&e, "2026-05-03"));
        let l = vec![Raccourci { abrev: ";a".into(), texte: "x".into() }, Raccourci { abrev: ";ab".into(), texte: "y".into() }];
        assert_eq!(trouver("salut ;ab", &l).map(|r| r.texte.as_str()), Some("y"));
        assert!(trouver("salut", &l).is_none());
        assert_eq!(developper("le {date} à {heure}", "20/11/2026", "9:05"), "le 20/11/2026 à 9:05");
        assert_eq!(code_meteo(0).0, "☀");
    }
}
