//! Jeu : overlay en jeu (10), statistiques de jeu (11), batterie de la manette (15), compteur de clics et de
//! touches (17), test d'écran (18), mode stream (20).
use crate::moniteur::Lecteur;
use crate::outil::{charger, enregistrer, jeu_actif, Cat, Outil};
use crate::pressepapiers::{decalage, maintenant_utc};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------------------------------
// 10 · Overlay en jeu (fenêtre native : marche même Outils rangé)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglagesOverlay {
    pub actif: bool,
    pub seulement_en_jeu: bool,
    pub coin: u8, // 0 haut-gauche, 1 haut-droite, 2 bas-gauche, 3 bas-droite
    pub taille: f32,
    pub fond: f32,
    pub fps: bool,
    pub cpu: bool,
    pub gpu: bool,
    pub ram: bool,
    pub heure: bool,
    pub duree: bool,
    pub note: String,
}

impl Default for ReglagesOverlay {
    fn default() -> Self {
        ReglagesOverlay { actif: false, seulement_en_jeu: true, coin: 0, taille: 18.0, fond: 0.45, fps: true, cpu: true, gpu: true, ram: false, heure: true, duree: true, note: String::new() }
    }
}

/// Lignes à afficher (texte, couleur).
pub fn lignes_overlay(r: &ReglagesOverlay, m: &crate::moniteur::Instantane, heure: &str, duree: Option<u64>) -> Vec<(String, [u8; 3])> {
    let deg = |t: f32| if t > 0.0 { format!(" {}°C", t as i32) } else { String::new() };
    let mut v = vec![];
    if r.fps && m.fps >= 0.0 {
        let c = if m.fps >= 60.0 { [80, 230, 150] } else if m.fps >= 30.0 { [250, 200, 60] } else { [250, 80, 100] };
        v.push((format!("{} FPS", m.fps as i32), c));
    }
    if r.cpu {
        v.push((format!("CPU {}%{}", m.cpu as i32, deg(m.cpu_t)), [235, 235, 245]));
    }
    if r.gpu && m.gpu >= 0.0 {
        v.push((format!("GPU {}%{}", m.gpu as i32, deg(m.gpu_t)), [235, 235, 245]));
    }
    if r.ram {
        v.push((format!("RAM {}%", m.ram as i32), [235, 235, 245]));
    }
    if r.heure {
        v.push((heure.to_string(), [255, 180, 215]));
    }
    if let (true, Some(d)) = (r.duree, duree) {
        v.push((format!("en jeu depuis {} h {:02}", d / 3600, d / 60 % 60), [200, 200, 220]));
    }
    if !r.note.trim().is_empty() {
        v.push((r.note.trim().to_string(), [255, 230, 150]));
    }
    v
}

mod fenetre {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn proc_(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        DefWindowProcW(h, m, wp, lp)
    }

    pub fn creer() -> HWND {
        unsafe {
            let inst = GetModuleHandleW(null());
            let classe: Vec<u16> = "OutilsOverlay".encode_utf16().chain([0]).collect();
            let wc = WNDCLASSW { lpfnWndProc: Some(proc_), hInstance: inst, lpszClassName: classe.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, classe.as_ptr(), classe.as_ptr(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), inst, null())
        }
    }

    /// Affiche une image RGBA (non prémultipliée) au coin choisi de l'écran principal.
    pub fn afficher(h: HWND, img: &image::RgbaImage, coin: u8) {
        unsafe {
            let (l, ht) = (img.width() as i32, img.height() as i32);
            let mut px = vec![0u8; img.as_raw().len()];
            for (i, p) in img.pixels().enumerate() {
                let a = p[3] as u32;
                px[i * 4] = (p[2] as u32 * a / 255) as u8;
                px[i * 4 + 1] = (p[1] as u32 * a / 255) as u8;
                px[i * 4 + 2] = (p[0] as u32 * a / 255) as u8;
                px[i * 4 + 3] = p[3];
            }
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = l;
            bi.bmiHeader.biHeight = -ht;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            let ecran = GetDC(null_mut());
            let mem = CreateCompatibleDC(ecran);
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bm = CreateDIBSection(ecran, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
            }
            let vieux = SelectObject(mem, bm);
            let (el, eh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let m = 12;
            let pos = POINT { x: if coin % 2 == 0 { m } else { el - l - m }, y: if coin < 2 { m } else { eh - ht - m } };
            let taille = SIZE { cx: l, cy: ht };
            let src = POINT { x: 0, y: 0 };
            let bf = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(h, ecran, &pos, &taille, mem, &src, 0, &bf, ULW_ALPHA);
            SelectObject(mem, vieux);
            DeleteObject(bm);
            DeleteDC(mem);
            ReleaseDC(null_mut(), ecran);
            ShowWindow(h, SW_SHOWNOACTIVATE);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn cacher(h: HWND) {
        unsafe { ShowWindow(h, SW_HIDE) };
    }

    pub fn boucle_messages_courte() {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

pub struct Overlay {
    r: Arc<Mutex<ReglagesOverlay>>,
}

impl Overlay {
    pub fn new(lire: Lecteur) -> Self {
        let r = Arc::new(Mutex::new(charger::<ReglagesOverlay>("overlay")));
        let r2 = r.clone();
        // Ctrl+Alt+H : afficher / cacher
        let r3 = r.clone();
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
            if RegisterHotKey(std::ptr::null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'H' as u32) == 0 {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    let mut x = r3.lock().unwrap();
                    x.actif = !x.actif;
                    enregistrer("overlay", &*x);
                }
            }
        });
        std::thread::spawn(move || {
            let h = fenetre::creer();
            let police = crate::creatif::police(&["segoeuib.ttf", "arialbd.ttf", "arial.ttf"]);
            let mut debut_jeu: Option<(String, Instant)> = None;
            loop {
                fenetre::boucle_messages_courte();
                std::thread::sleep(Duration::from_millis(500));
                let reg = r2.lock().unwrap().clone();
                let jeu = jeu_actif();
                match (&jeu, &debut_jeu) {
                    (Some(j), Some((d, _))) if j == d => {}
                    (Some(j), _) => debut_jeu = Some((j.clone(), Instant::now())),
                    (None, _) => debut_jeu = None,
                }
                if !reg.actif || (reg.seulement_en_jeu && jeu.is_none()) || police.is_none() {
                    fenetre::cacher(h);
                    continue;
                }
                let (m, _) = lire(true);
                let l = (maintenant_utc() + decalage()).rem_euclid(86400);
                let heure = format!("{:02}:{:02}", l / 3600, l / 60 % 60);
                let lignes = lignes_overlay(&reg, &m, &heure, debut_jeu.as_ref().map(|d| d.1.elapsed().as_secs()));
                if lignes.is_empty() {
                    fenetre::cacher(h);
                    continue;
                }
                let f = police.as_ref().unwrap();
                let t = reg.taille.clamp(10.0, 48.0);
                let hl = (t * 1.3) as u32;
                let larg = lignes.iter().map(|(s, _)| s.chars().count()).max().unwrap_or(4) as f32 * t * 0.62 + 20.0;
                let mut img = image::RgbaImage::from_pixel(larg as u32, hl * lignes.len() as u32 + 12, image::Rgba([12, 10, 18, (reg.fond.clamp(0.0, 1.0) * 255.0) as u8]));
                for (i, (s, c)) in lignes.iter().enumerate() {
                    crate::creatif::ecrire(&mut img, f, t, 10.0, 6.0 + (i as u32 * hl) as f32, s, *c, Some(([0, 0, 0], 1)));
                }
                fenetre::afficher(h, &img, reg.coin);
            }
        });
        Overlay { r }
    }
}

impl Outil for Overlay {
    fn nom(&self) -> &'static str {
        "🔳 Overlay en jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Overlay en jeu");
        ui.weak("Infos par-dessus le jeu (en plein écran fenêtré / sans bordure), transparentes aux clics. Ctrl+Alt+H pour l'afficher ou le cacher. Le FPS vient du Moniteur Jeu (Outils lancé en administrateur).");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Afficher l'overlay");
            ui.checkbox(&mut r.seulement_en_jeu, "seulement pendant un jeu");
        });
        ui.horizontal(|ui| {
            ui.label("Coin");
            for (k, n) in ["↖", "↗", "↙", "↘"].iter().enumerate() {
                ui.selectable_value(&mut r.coin, k as u8, *n);
            }
            ui.add(egui::Slider::new(&mut r.taille, 10.0..=40.0).text("taille"));
            ui.add(egui::Slider::new(&mut r.fond, 0.0..=1.0).text("fond"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut r.fps, "FPS");
            ui.checkbox(&mut r.cpu, "Processeur");
            ui.checkbox(&mut r.gpu, "Carte graphique");
            ui.checkbox(&mut r.ram, "Mémoire");
            ui.checkbox(&mut r.heure, "Heure");
            ui.checkbox(&mut r.duree, "Temps de jeu");
        });
        ui.horizontal(|ui| {
            ui.label("Note");
            ui.add(egui::TextEdit::singleline(&mut r.note).hint_text("ex. code du coffre : 4521").desired_width(300.0));
        });
        if r != avant {
            enregistrer("overlay", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 11 · Statistiques de jeu
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Session {
    pub jeu: String,
    pub debut: i64,
    pub fin: i64,
}

/// Jour local (depuis 1970) d'un instant UTC.
fn jour_de(t: i64, off: i64) -> i64 {
    (t + off).div_euclid(86400)
}

/// Série de jours consécutifs avec au moins une session, jusqu'à `aujourdhui` (ou hier).
pub fn serie(jours: &std::collections::BTreeSet<i64>, aujourdhui: i64) -> u32 {
    let mut j = if jours.contains(&aujourdhui) { aujourdhui } else { aujourdhui - 1 };
    let mut n = 0;
    while jours.contains(&j) {
        n += 1;
        j -= 1;
    }
    n
}

pub struct StatsJeu {
    sessions: Vec<Session>,
    en_cours: Option<(String, i64)>,
    vue: u8,
}

impl StatsJeu {
    pub fn new() -> Self {
        StatsJeu { sessions: charger("stats_jeu"), en_cours: None, vue: 0 }
    }
}

impl Outil for StatsJeu {
    fn nom(&self) -> &'static str {
        "📊 Statistiques de jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _: &egui::Context) {
        let jeu = jeu_actif();
        let m = maintenant_utc();
        match (&jeu, &self.en_cours) {
            (Some(j), Some((e, _))) if j == e => {}
            _ => {
                if let Some((e, d)) = self.en_cours.take() {
                    if m - d >= 60 {
                        self.sessions.push(Session { jeu: e, debut: d, fin: m });
                        enregistrer("stats_jeu", &self.sessions);
                    }
                }
                self.en_cours = jeu.map(|j| (j, m));
            }
        }
    }
    fn resume(&self) -> Option<String> {
        let off = decalage();
        let auj = jour_de(maintenant_utc(), off);
        let s: i64 = self.sessions.iter().filter(|x| jour_de(x.debut, off) > auj - 7).map(|x| x.fin - x.debut).sum();
        (!self.sessions.is_empty()).then(|| format!("{} de jeu ces 7 derniers jours", crate::quotidien::duree(s as u64)))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Statistiques de jeu");
        ui.weak("Chaque partie détectée par le Lanceur (lancée d'ici ou non) est notée, à partir d'une minute.");
        let off = decalage();
        let m = maintenant_utc();
        let auj = jour_de(m, off);
        let mut toutes = self.sessions.clone();
        if let Some((e, d)) = &self.en_cours {
            toutes.push(Session { jeu: e.clone(), debut: *d, fin: m });
            ui.colored_label(Color32::LIGHT_GREEN, format!("🎮 En jeu : {e} depuis {}", crate::quotidien::duree((m - d) as u64)));
            ui.ctx().request_repaint_after(Duration::from_secs(30));
        }
        let jours: std::collections::BTreeSet<i64> = toutes.iter().map(|s| jour_de(s.debut, off)).collect();
        let total: i64 = toutes.iter().map(|s| s.fin - s.debut).sum();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Total : {}", crate::quotidien::duree(total as u64))).strong());
            ui.label(format!("· {} parties", toutes.len()));
            ui.label(format!("· série : {} jour(s) d'affilée", serie(&jours, auj)));
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.vue, 0, "Par semaine");
            ui.selectable_value(&mut self.vue, 1, "Par jeu (30 jours)");
        });
        if self.vue == 0 {
            // 8 dernières semaines
            let semaines: Vec<(String, i64)> = (0..8).rev().map(|k| {
                let fin = auj - 7 * k;
                let t: i64 = toutes.iter().filter(|s| { let j = jour_de(s.debut, off); j > fin - 7 && j <= fin }).map(|s| s.fin - s.debut).sum();
                let (_, mo, d) = crate::pressepapiers::civil(fin - 6);
                (format!("{d:02}/{mo:02}"), t)
            }).collect();
            let max = semaines.iter().map(|s| s.1).max().unwrap_or(1).max(1);
            let (zone, _) = ui.allocate_exact_size(egui::vec2(480.0, 150.0), egui::Sense::hover());
            let l = zone.width() / 8.0;
            for (i, (n, t)) in semaines.iter().enumerate() {
                let h = (zone.height() - 20.0) * *t as f32 / max as f32;
                let r = egui::Rect::from_min_max(egui::pos2(zone.left() + i as f32 * l + 6.0, zone.bottom() - 20.0 - h), egui::pos2(zone.left() + (i + 1) as f32 * l - 6.0, zone.bottom() - 20.0));
                ui.painter().rect_filled(r, 3.0, Color32::from_rgb(255, 95, 162));
                ui.painter().text(egui::pos2(r.center().x, zone.bottom()), egui::Align2::CENTER_BOTTOM, n, egui::FontId::proportional(11.0), Color32::GRAY);
                if *t > 0 {
                    ui.painter().text(egui::pos2(r.center().x, r.top() - 2.0), egui::Align2::CENTER_BOTTOM, crate::quotidien::duree(*t as u64), egui::FontId::proportional(10.0), Color32::WHITE);
                }
            }
        } else {
            let mut par: BTreeMap<String, i64> = BTreeMap::new();
            for s in toutes.iter().filter(|s| jour_de(s.debut, off) > auj - 30) {
                *par.entry(s.jeu.clone()).or_default() += s.fin - s.debut;
            }
            let mut v: Vec<(String, i64)> = par.into_iter().collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            let max = v.first().map(|x| x.1).unwrap_or(1).max(1);
            for (j, t) in v {
                ui.horizontal(|ui| {
                    ui.add(egui::ProgressBar::new(t as f32 / max as f32).desired_width(220.0).text(crate::quotidien::duree(t as u64)));
                    ui.label(j);
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 15 · Batterie de la manette (manettes Xbox / XInput)
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum EtatManette {
    Absente,
    Filaire,
    Batterie(u8), // 0 vide, 1 faible, 2 moyenne, 3 pleine
}

fn manettes() -> [EtatManette; 4] {
    use windows_sys::Win32::UI::Input::XboxController::*;
    std::array::from_fn(|i| unsafe {
        let mut s: XINPUT_STATE = std::mem::zeroed();
        if XInputGetState(i as u32, &mut s) != 0 {
            return EtatManette::Absente;
        }
        let mut b: XINPUT_BATTERY_INFORMATION = std::mem::zeroed();
        if XInputGetBatteryInformation(i as u32, BATTERY_DEVTYPE_GAMEPAD, &mut b) != 0 {
            return EtatManette::Filaire;
        }
        match b.BatteryType {
            BATTERY_TYPE_WIRED | BATTERY_TYPE_DISCONNECTED => EtatManette::Filaire,
            _ => EtatManette::Batterie(b.BatteryLevel),
        }
    })
}

pub struct Manette {
    etat: Arc<Mutex<[EtatManette; 4]>>,
}

impl Manette {
    pub fn new() -> Self {
        let etat: Arc<Mutex<[EtatManette; 4]>> = Arc::new(Mutex::new(std::array::from_fn(|_| EtatManette::Absente)));
        let e2 = etat.clone();
        std::thread::spawn(move || {
            let mut prevenu = [false; 4];
            loop {
                let m = manettes();
                for (i, x) in m.iter().enumerate() {
                    let faible = matches!(x, EtatManette::Batterie(0 | 1));
                    if faible && !prevenu[i] {
                        crate::systeme::notifier_toujours("🎮 Manette", &format!("Batterie faible sur la manette {} : pense à la recharger.", i + 1));
                    }
                    prevenu[i] = faible;
                }
                *e2.lock().unwrap() = m;
                std::thread::sleep(Duration::from_secs(60));
            }
        });
        Manette { etat }
    }
}

impl Outil for Manette {
    fn nom(&self) -> &'static str {
        "🎮 Batterie de la manette"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn resume(&self) -> Option<String> {
        let e = self.etat.lock().unwrap();
        e.iter().find_map(|x| match x {
            EtatManette::Batterie(n) => Some(format!("Manette : batterie {}", ["vide", "faible", "moyenne", "pleine"][(*n as usize).min(3)])),
            _ => None,
        })
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Batterie de la manette");
        ui.weak("Manettes Xbox et compatibles XInput. Une notification prévient quand la batterie devient faible. (Les manettes PlayStation passent par Steam et ne sont pas visibles ici.)");
        if ui.button("⟳ Actualiser").clicked() {
            *self.etat.lock().unwrap() = manettes();
        }
        let e = self.etat.lock().unwrap().clone();
        let mut aucune = true;
        for (i, x) in e.iter().enumerate() {
            let t = match x {
                EtatManette::Absente => continue,
                EtatManette::Filaire => "branchée en câble".to_string(),
                EtatManette::Batterie(n) => {
                    let n = (*n as usize).min(3);
                    format!("{} {}", ["▱▱▱", "▰▱▱", "▰▰▱", "▰▰▰"][n], ["vide", "faible", "moyenne", "pleine"][n])
                }
            };
            aucune = false;
            ui.label(RichText::new(format!("🎮 Manette {} : {t}", i + 1)).size(20.0));
        }
        if aucune {
            ui.weak("Aucune manette XInput détectée.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 17 · Compteur de clics et de touches
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Compte {
    touches: u64,
    clics: u64,
    metres: f64,
}

pub struct Compteur {
    actif: bool,
    jours: BTreeMap<String, Compte>,
    base: (u64, u64, f64), // valeurs du compteur brut déjà ajoutées
    dernier: Instant,
}

impl Compteur {
    pub fn new() -> Self {
        let actif: bool = charger::<Option<bool>>("compteur_actif").unwrap_or(false);
        if actif {
            crate::tests_materiel::brut();
        }
        Compteur { actif, jours: charger("compteur"), base: (0, 0, 0.0), dernier: Instant::now() }
    }

    fn relever(&mut self) {
        let b = crate::tests_materiel::brut();
        let x = b.lock().unwrap();
        let (t, c, m) = (x.touches, x.nb_clics, x.mickeys);
        drop(x);
        let j = crate::quotidien::date_locale(maintenant_utc(), decalage());
        let e = self.jours.entry(j).or_default();
        e.touches += t - self.base.0.min(t);
        e.clics += c - self.base.1.min(c);
        e.metres += (m - self.base.2).max(0.0) / 800.0 * 0.0254; // souris ~800 DPI
        self.base = (t, c, m);
    }
}

impl Outil for Compteur {
    fn nom(&self) -> &'static str {
        "🔢 Compteur clics et touches"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.actif && self.dernier.elapsed() > Duration::from_secs(30) {
            self.dernier = Instant::now();
            self.relever();
            enregistrer("compteur", &self.jours);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Compteur de clics et de touches");
        if ui.checkbox(&mut self.actif, "Compter (seulement des nombres : aucune touche n'est enregistrée)").changed() {
            enregistrer("compteur_actif", &Some(self.actif));
            if self.actif {
                crate::tests_materiel::brut();
            }
        }
        if self.actif {
            self.relever();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let auj = crate::quotidien::date_locale(maintenant_utc(), decalage());
        let a = self.jours.get(&auj).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            for (n, v) in [("⌨ touches", a.touches.to_string()), ("🖱 clics", a.clics.to_string()), ("📏 souris", format!("{:.0} m", a.metres))] {
                ui.vertical(|ui| {
                    ui.label(RichText::new(v).size(30.0).strong());
                    ui.weak(format!("{n} aujourd'hui"));
                });
                ui.add_space(20.0);
            }
        });
        let total = self.jours.values().fold(Compte::default(), |a, b| Compte { touches: a.touches + b.touches, clics: a.clics + b.clics, metres: a.metres + b.metres });
        ui.label(format!("Depuis le début : {} touches, {} clics, {:.1} km de souris", total.touches, total.clics, total.metres / 1000.0));
        ui.separator();
        for (j, c) in self.jours.iter().rev().take(10) {
            ui.weak(format!("{j} : {} touches · {} clics · {:.0} m", c.touches, c.clics, c.metres));
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 18 · Test d'écran
// ---------------------------------------------------------------------------------------------------

pub struct TestEcran {
    actif: bool,
    etape: usize,
    debut: Instant,
}

const ETAPES: [&str; 9] = ["Noir", "Blanc", "Rouge", "Vert", "Bleu", "Gris moyen", "Dégradé", "Grille", "Mouvement (ghosting)"];

impl TestEcran {
    pub fn new() -> Self {
        TestEcran { actif: false, etape: 0, debut: Instant::now() }
    }
}

impl Outil for TestEcran {
    fn nom(&self) -> &'static str {
        "🖥 Test d'écran"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, ctx: &egui::Context) {
        if !self.actif {
            return;
        }
        let (etape, t) = (self.etape, self.debut.elapsed().as_secs_f32());
        let mut suivant = false;
        let mut fin = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("test_ecran"), egui::ViewportBuilder::default().with_fullscreen(true).with_decorations(false).with_title("Test d'écran"), |ctx, _| {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx, |ui| {
                let z = ui.max_rect();
                let p = ui.painter();
                match etape {
                    0..=5 => {
                        let c = [Color32::BLACK, Color32::WHITE, Color32::from_rgb(255, 0, 0), Color32::from_rgb(0, 255, 0), Color32::from_rgb(0, 0, 255), Color32::from_gray(128)][etape];
                        p.rect_filled(z, 0.0, c);
                    }
                    6 => {
                        let n = 64;
                        for k in 0..n {
                            let g = (k * 255 / (n - 1)) as u8;
                            let w = z.width() / n as f32;
                            p.rect_filled(egui::Rect::from_min_size(egui::pos2(z.left() + k as f32 * w, z.top()), egui::vec2(w + 1.0, z.height())), 0.0, Color32::from_gray(g));
                        }
                    }
                    7 => {
                        p.rect_filled(z, 0.0, Color32::BLACK);
                        let mut x = z.left();
                        while x < z.right() {
                            p.line_segment([egui::pos2(x, z.top()), egui::pos2(x, z.bottom())], egui::Stroke::new(1.0f32, Color32::WHITE));
                            x += 40.0;
                        }
                        let mut y = z.top();
                        while y < z.bottom() {
                            p.line_segment([egui::pos2(z.left(), y), egui::pos2(z.right(), y)], egui::Stroke::new(1.0f32, Color32::WHITE));
                            y += 40.0;
                        }
                    }
                    _ => {
                        p.rect_filled(z, 0.0, Color32::from_gray(90));
                        let x = (t * 960.0) % (z.width() + 200.0) - 100.0;
                        for (k, c) in [Color32::WHITE, Color32::from_rgb(0, 200, 255), Color32::from_rgb(255, 95, 162)].iter().enumerate() {
                            p.rect_filled(egui::Rect::from_center_size(egui::pos2(x, z.top() + z.height() * (k as f32 + 1.0) / 4.0), egui::vec2(90.0, 90.0)), 8.0, *c);
                        }
                        ctx.request_repaint();
                    }
                }
                p.text(z.left_bottom() + egui::vec2(16.0, -16.0), egui::Align2::LEFT_BOTTOM, format!("{} ({}/{}) · clic ou Espace : suivant · Échap : quitter", ETAPES[etape], etape + 1, ETAPES.len()),
                    egui::FontId::proportional(14.0), Color32::from_gray(140));
            });
            ctx.input(|i| {
                suivant = i.pointer.any_click() || i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::ArrowRight);
                fin = i.key_pressed(egui::Key::Escape) || i.viewport().close_requested();
            });
        });
        if suivant {
            self.etape += 1;
            if self.etape >= ETAPES.len() {
                fin = true;
            }
        }
        if fin {
            self.actif = false;
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test d'écran");
        ui.label("Couleurs pleines pour repérer les pixels morts ou bloqués, dégradé pour les bandes, grille pour la géométrie, carrés qui défilent pour le flou de mouvement.");
        if ui.button(RichText::new("▶ Lancer le test plein écran").size(18.0)).clicked() {
            self.actif = true;
            self.etape = 0;
            self.debut = Instant::now();
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 20 · Mode stream
// ---------------------------------------------------------------------------------------------------

fn processus_present(noms: &[&str]) -> bool {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let s = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(s, &mut e) != 0;
        let mut trouve = false;
        while ok && !trouve {
            let n = String::from_utf16_lossy(&e.szExeFile[..e.szExeFile.iter().position(|&c| c == 0).unwrap_or(0)]).to_lowercase();
            trouve = noms.iter().any(|x| n == *x);
            ok = Process32NextW(s, &mut e) != 0;
        }
        windows_sys::Win32::Foundation::CloseHandle(s);
        trouve
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct ReglagesStream {
    auto: bool,
    manuel: bool,
    couper_notifs: bool,
}

impl Default for ReglagesStream {
    fn default() -> Self {
        ReglagesStream { auto: true, manuel: false, couper_notifs: true }
    }
}

pub struct ModeStream {
    r: ReglagesStream,
    obs: Arc<AtomicBool>,
    notifs_avant: Option<bool>,
}

impl ModeStream {
    pub fn new() -> Self {
        let obs: Arc<AtomicBool> = Arc::default();
        let o2 = obs.clone();
        std::thread::spawn(move || loop {
            o2.store(processus_present(&["obs64.exe", "obs32.exe", "streamlabs obs.exe", "twitchstudio.exe"]), Relaxed);
            std::thread::sleep(Duration::from_secs(10));
        });
        ModeStream { r: charger("stream"), obs, notifs_avant: None }
    }

    fn voulu(&self) -> bool {
        self.r.manuel || (self.r.auto && self.obs.load(Relaxed))
    }
}

impl Outil for ModeStream {
    fn nom(&self) -> &'static str {
        "🎥 Mode stream"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _: &egui::Context) {
        let v = self.voulu();
        if v != crate::outil::STREAM.load(Relaxed) {
            crate::outil::STREAM.store(v, Relaxed);
            if self.r.couper_notifs {
                if v {
                    self.notifs_avant = Some(crate::systeme::NOTIFS.swap(false, Relaxed));
                } else if let Some(n) = self.notifs_avant.take() {
                    crate::systeme::NOTIFS.store(n, Relaxed);
                }
            }
        }
    }
    fn resume(&self) -> Option<String> {
        crate::outil::STREAM.load(Relaxed).then(|| "Actif : onglets sensibles masqués".into())
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mode stream");
        let avant = self.r.clone();
        let actif = crate::outil::STREAM.load(Relaxed);
        if actif {
            ui.colored_label(Color32::from_rgb(255, 120, 170), RichText::new("🔴 Mode stream actif").size(18.0));
        }
        ui.checkbox(&mut self.r.auto, "S'active tout seul quand OBS, Streamlabs ou Twitch Studio tourne");
        ui.checkbox(&mut self.r.manuel, "Forcer maintenant");
        ui.checkbox(&mut self.r.couper_notifs, "Couper les notifications d'Outils (pas d'infos perso à l'écran)");
        ui.label("Pendant le stream, le Coffre, le presse-papiers, les mots de passe Wi-Fi et le Coffre de fichiers sont masqués s'ils sont affichés.");
        if self.r != avant {
            enregistrer("stream", &self.r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_et_series() {
        let r = ReglagesOverlay { ram: true, duree: true, note: "salut".into(), ..Default::default() };
        let m = crate::moniteur::Instantane { fps: 144.0, cpu: 30.0, cpu_t: 65.0, gpu: -1.0, gpu_t: -1.0, ram: 50.0 };
        let l = lignes_overlay(&r, &m, "21:05", Some(3700));
        assert_eq!(l[0].0, "144 FPS");
        assert_eq!(l[1].0, "CPU 30% 65°C");
        assert!(l.iter().any(|x| x.0 == "RAM 50%") && !l.iter().any(|x| x.0.starts_with("GPU")));
        assert!(l.iter().any(|x| x.0 == "en jeu depuis 1 h 01"));
        let j: std::collections::BTreeSet<i64> = [10, 11, 12, 14].into();
        assert_eq!(serie(&j, 12), 3);
        assert_eq!(serie(&j, 13), 3); // pas encore joué aujourd'hui : la série d'hier compte
        assert_eq!(serie(&j, 16), 0);
    }
}
