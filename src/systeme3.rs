//! Système, suite : test de stabilité (42), grille de fenêtres (44), toujours au-dessus (45),
//! points de restauration (46), analyse antivirus (47), pare-feu par appli (48), qui utilise la webcam
//! et le micro (49), rappel de redémarrage Windows Update (50), démarrage retardé (51).
use crate::optimiseur::{est_admin, ps, reg_b, reg_lire, reg_noms, reg_sous_cles, reg_texte, relancer_admin};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};

const SANS_FENETRE: u32 = 0x0800_0000;

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

fn admin_requis(ui: &mut egui::Ui) -> bool {
    let a = est_admin();
    if !a {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "⚠ Il faut les droits administrateur pour cet outil.");
            if ui.small_button("Relancer en admin").clicked() && relancer_admin() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
    a
}

fn cmd(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(prog).args(args).creation_flags(SANS_FENETRE).output().map_err(|e| e.to_string())?;
    let s = crate::systeme2::texte_console(&o.stdout) + &crate::systeme2::texte_console(&o.stderr);
    if o.status.success() { Ok(s) } else { Err(s.trim().to_string()) }
}

fn ps_echappe(s: &str) -> String {
    s.replace('\'', "''")
}

// ---------------------------------------------------------------------------------------------------
// 42 · Test de stabilité (stress test)
// ---------------------------------------------------------------------------------------------------

#[derive(Default)]
struct Stress {
    debut: Option<Instant>,
    duree: u64,
    points: Vec<(f32, f32, f32)>, // (secondes, cpu %, temp °C)
    fini: Option<String>,
}

/// Verdict d'après la température maximale et l'usage moyen.
pub fn verdict_stress(max_t: f32, cpu_moy: f32) -> (&'static str, Color32) {
    if max_t <= 0.0 {
        ("Température non lue (lance Outils en admin pour la voir). Aucun plantage pendant le test : bon signe.", Color32::from_rgb(250, 210, 90))
    } else if max_t >= 95.0 {
        ("🔥 Le processeur chauffe beaucoup trop : nettoie la poussière, vérifie le ventirad et la pâte thermique.", Color32::LIGHT_RED)
    } else if max_t >= 85.0 {
        ("♨ Chaud mais encore acceptable. Un petit dépoussiérage ferait du bien.", Color32::from_rgb(250, 210, 90))
    } else if cpu_moy < 80.0 {
        ("Le processeur n'a pas pu être chargé à fond (autre programme ? mode économie d'énergie ?).", Color32::from_rgb(250, 210, 90))
    } else {
        ("✔ Stable et bien refroidi.", Color32::LIGHT_GREEN)
    }
}

pub struct TestStabilite {
    mesures: crate::moniteur::Lecteur,
    s: Arc<Mutex<Stress>>,
    arret: Arc<AtomicBool>,
    minutes: u64,
}

impl TestStabilite {
    pub fn new(mesures: crate::moniteur::Lecteur) -> Self {
        TestStabilite { mesures, s: Arc::default(), arret: Arc::new(AtomicBool::new(true)), minutes: 5 }
    }

    fn lancer(&mut self) {
        self.arret.store(false, Relaxed);
        *self.s.lock().unwrap() = Stress { debut: Some(Instant::now()), duree: self.minutes * 60, ..Default::default() };
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        for k in 0..n {
            let a = self.arret.clone();
            std::thread::spawn(move || {
                let mut x = 1.000_1f64 + k as f64;
                let mut v = vec![0u64; 1 << 16]; // un peu de mémoire cache aussi
                let mut i = 0usize;
                while !a.load(Relaxed) {
                    for _ in 0..200_000 {
                        x = (x * 1.000_000_1 + 0.5).sqrt() + 1.0;
                        i = (i * 1_103_515_245 + 12_345) & 0xFFFF;
                        v[i] = v[i].wrapping_add(x.to_bits());
                    }
                    std::hint::black_box(&v);
                }
            });
        }
        let (a, s, m) = (self.arret.clone(), self.s.clone(), self.mesures.clone());
        std::thread::spawn(move || {
            let t0 = Instant::now();
            while !a.load(Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                let (x, _) = m(true);
                let mut st = s.lock().unwrap();
                let t = t0.elapsed().as_secs_f32();
                st.points.push((t, x.cpu, x.cpu_t));
                if t as u64 >= st.duree {
                    a.store(true, Relaxed);
                }
            }
            let mut st = s.lock().unwrap();
            let max_t = st.points.iter().map(|p| p.2).fold(0.0, f32::max);
            let moy = st.points.iter().skip(5).map(|p| p.1).sum::<f32>() / st.points.len().saturating_sub(5).max(1) as f32;
            st.fini = Some(format!("{} — max {max_t:.0} °C, CPU moyen {moy:.0} %", verdict_stress(max_t, moy).0));
            crate::coeur::noter("Test de stabilité", st.fini.as_deref().unwrap_or(""));
        });
    }
}

impl Outil for TestStabilite {
    fn nom(&self) -> &'static str {
        "🔥 Test de stabilité (stress)"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test de stabilité du processeur");
        ui.weak("Fait tourner tous les cœurs à fond et surveille la température. Utile après un nettoyage, un changement de pâte thermique ou en cas de plantages en jeu.");
        let en_cours = !self.arret.load(Relaxed);
        ui.horizontal(|ui| {
            ui.add_enabled(!en_cours, egui::DragValue::new(&mut self.minutes).range(1..=60).suffix(" min"));
            if en_cours {
                if ui.button("⏹ Arrêter").clicked() {
                    self.arret.store(true, Relaxed);
                }
            } else if ui.button("▶ Lancer le test").clicked() {
                self.lancer();
            }
        });
        let s = self.s.lock().unwrap();
        if let (Some(d), true) = (s.debut, en_cours) {
            let e = d.elapsed().as_secs();
            ui.add(egui::ProgressBar::new(e as f32 / s.duree.max(1) as f32).text(format!("{}:{:02} / {} min", e / 60, e % 60, s.duree / 60)));
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        if let Some(p) = s.points.last() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("CPU {:.0} %", p.1)).size(20.0));
                if p.2 > 0.0 {
                    let c = if p.2 >= 95.0 { Color32::LIGHT_RED } else if p.2 >= 85.0 { Color32::from_rgb(250, 210, 90) } else { Color32::LIGHT_GREEN };
                    ui.label(RichText::new(format!("{:.0} °C", p.2)).size(20.0).color(c));
                }
            });
            // courbe de température
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(600.0), 140.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 4.0, Color32::from_black_alpha(60));
            let tmax = s.points.last().map(|p| p.0).unwrap_or(1.0).max(10.0);
            let pt = |t: f32, v: f32| egui::pos2(r.left() + t / tmax * r.width(), r.bottom() - (v / 110.0).clamp(0.0, 1.0) * r.height());
            let courbe = |f: fn(&(f32, f32, f32)) -> f32| s.points.iter().map(|p| pt(p.0, f(p))).collect::<Vec<_>>();
            ui.painter().add(egui::Shape::line(courbe(|p| p.1), egui::Stroke::new(1.5_f32, Color32::from_rgb(120, 170, 255))));
            if s.points.iter().any(|p| p.2 > 0.0) {
                ui.painter().add(egui::Shape::line(courbe(|p| p.2), egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 120, 170))));
            }
            ui.painter().hline(r.x_range(), pt(0.0, 95.0).y, egui::Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 80, 80, 120)));
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(120, 170, 255), "— CPU %");
                ui.colored_label(Color32::from_rgb(255, 120, 170), "— température");
                ui.colored_label(Color32::LIGHT_RED, "— 95 °C");
            });
        }
        if let Some(f) = &s.fini {
            let max_t = s.points.iter().map(|p| p.2).fold(0.0, f32::max);
            let moy = s.points.iter().skip(5).map(|p| p.1).sum::<f32>() / s.points.len().saturating_sub(5).max(1) as f32;
            ui.colored_label(verdict_stress(max_t, moy).1, RichText::new(f).strong());
        }
        if self.mesures.as_ref()(false).0.cpu_t <= 0.0 && !est_admin() {
            ui.weak("La température du processeur n'est lisible qu'avec Outils lancé en administrateur.");
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 44 · Grille de fenêtres / 45 · Toujours au-dessus
// ---------------------------------------------------------------------------------------------------

/// Zone (x, y, l, h) dans l'espace de travail pour une touche du pavé numérique (disposition du pavé).
pub fn zone(touche: u32, t: (i32, i32, i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let (x, y, l, h) = t;
    let (l2, h2) = (l / 2, h / 2);
    Some(match touche {
        4 => (x, y, l2, h),
        6 => (x + l2, y, l - l2, h),
        8 => (x, y, l, h2),
        2 => (x, y + h2, l, h - h2),
        7 => (x, y, l2, h2),
        9 => (x + l2, y, l - l2, h2),
        1 => (x, y + h2, l2, h - h2),
        3 => (x + l2, y + h2, l - l2, h - h2),
        5 => (x + l / 10, y + h / 10, l * 8 / 10, h * 8 / 10),
        // 0 : tiers gauche / . : deux tiers droite (pratique en écran large)
        0 => (x, y, l / 3, h),
        10 => (x + l / 3, y, l - l / 3, h),
        _ => return None,
    })
}

/// Grille pour n fenêtres : (colonnes, lignes).
pub fn grille(n: usize) -> (usize, usize) {
    let c = (n as f64).sqrt().ceil() as usize;
    let c = c.max(1);
    (c, n.div_ceil(c).max(1))
}

unsafe extern "system" fn rappel(h: HWND, l: LPARAM) -> BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let v = &mut *(l as *mut Vec<(isize, String)>);
    if IsWindowVisible(h) == 0 || IsIconic(h) != 0 || !GetWindow(h, GW_OWNER).is_null() {
        return 1;
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW != 0 {
        return 1;
    }
    let mut cache = 0i32;
    windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, 14 /* DWMWA_CLOAKED */, &mut cache as *mut i32 as _, 4);
    if cache != 0 {
        return 1;
    }
    let mut b = [0u16; 256];
    let n = GetWindowTextW(h, b.as_mut_ptr(), 256);
    if n > 0 {
        let t = String::from_utf16_lossy(&b[..n as usize]);
        if t != "Program Manager" && t != "Outils" {
            v.push((h as isize, t));
        }
    }
    1
}

/// Fenêtres visibles de la barre des tâches : (handle, titre).
pub fn fenetres() -> Vec<(isize, String)> {
    let mut v: Vec<(isize, String)> = vec![];
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows(Some(rappel), &mut v as *mut _ as LPARAM) };
    v
}

fn travail(h: HWND) -> (i32, i32, i32, i32) {
    use windows_sys::Win32::Graphics::Gdi::*;
    unsafe {
        let m = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(m, &mut mi);
        let r = mi.rcWork;
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }
}

/// Place une fenêtre en compensant ses bordures invisibles (Windows 10/11).
fn placer(h: HWND, (x, y, l, ht): (i32, i32, i32, i32)) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        if IsZoomed(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        let (mut r, mut f): (RECT, RECT) = (std::mem::zeroed(), std::mem::zeroed());
        GetWindowRect(h, &mut r);
        let ok = windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, 9 /* DWMWA_EXTENDED_FRAME_BOUNDS */, &mut f as *mut RECT as _, std::mem::size_of::<RECT>() as u32) == 0;
        let (g, hh, d, b) = if ok { (f.left - r.left, f.top - r.top, r.right - f.right, r.bottom - f.bottom) } else { (0, 0, 0, 0) };
        SetWindowPos(h, std::ptr::null_mut(), x - g, y - hh, l + g + d, ht + hh + b, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn est_au_dessus(h: HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe { GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 }
}

fn au_dessus(h: HWND, oui: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe { SetWindowPos(h, if oui { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
}

fn arranger_tout() -> usize {
    let v = fenetres();
    let Some(&(premiere, _)) = v.first() else { return 0 };
    let t = travail(premiere as HWND);
    let (c, l) = grille(v.len());
    let (lc, hl) = (t.2 / c as i32, t.3 / l as i32);
    for (i, (h, _)) in v.iter().enumerate() {
        placer(*h as HWND, (t.0 + (i % c) as i32 * lc, t.1 + (i / c) as i32 * hl, lc, hl));
    }
    v.len()
}

pub struct Fenetres {
    actif: Arc<AtomicBool>,
    pris: Arc<AtomicBool>,
    liste: Vec<(isize, String)>,
}

impl Fenetres {
    pub fn new() -> Self {
        let actif = Arc::new(AtomicBool::new(charger::<Option<bool>>("grille_fenetres").unwrap_or(true)));
        let pris: Arc<AtomicBool> = Arc::default();
        let (a, p) = (actif.clone(), pris.clone());
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let mut ok = true;
            for k in 0..=9u32 {
                ok &= RegisterHotKey(std::ptr::null_mut(), 100 + k as i32, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_NUMPAD0 as u32 + k) != 0;
            }
            ok &= RegisterHotKey(std::ptr::null_mut(), 110, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_DECIMAL as u32) != 0;
            ok &= RegisterHotKey(std::ptr::null_mut(), 120, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'T' as u32) != 0;
            ok &= RegisterHotKey(std::ptr::null_mut(), 121, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_MULTIPLY as u32) != 0;
            p.store(!ok, Relaxed);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message != WM_HOTKEY || !a.load(Relaxed) {
                    continue;
                }
                let h = GetForegroundWindow();
                match msg.wParam {
                    120 if !h.is_null() => au_dessus(h, !est_au_dessus(h)),
                    121 => {
                        arranger_tout();
                    }
                    k @ 100..=110 if !h.is_null() => {
                        if let Some(z) = zone(k as u32 - 100, travail(h)) {
                            placer(h, z);
                        }
                    }
                    _ => {}
                }
            }
        });
        Fenetres { actif, pris, liste: vec![] }
    }
}

impl Outil for Fenetres {
    fn nom(&self) -> &'static str {
        "🪟 Fenêtres : grille et au-dessus"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ranger les fenêtres");
        let mut a = self.actif.load(Relaxed);
        if ui.checkbox(&mut a, "Raccourcis actifs").changed() {
            self.actif.store(a, Relaxed);
            enregistrer("grille_fenetres", &Some(a));
        }
        if self.pris.load(Relaxed) {
            ui.colored_label(Color32::LIGHT_RED, "Un des raccourcis est déjà pris par une autre appli.");
        }
        ui.label("Sur la fenêtre au premier plan, Ctrl+Alt + pavé numérique :");
        egui::Grid::new("pave").show(ui, |ui| {
            for ligne in [[("7", "↖ quart"), ("8", "⬆ moitié haute"), ("9", "↗ quart")], [("4", "⬅ moitié gauche"), ("5", "◻ centrée"), ("6", "➡ moitié droite")], [("1", "↙ quart"), ("2", "⬇ moitié basse"), ("3", "↘ quart")]] {
                for (k, t) in ligne {
                    ui.label(RichText::new(format!("{k}  {t}")).monospace());
                }
                ui.end_row();
            }
        });
        ui.label("  0 : tiers gauche · « . » : deux tiers droite · * : ranger toutes les fenêtres en grille");
        ui.label(RichText::new("  Ctrl+Alt+T : garder la fenêtre toujours au-dessus (ou l'enlever)").strong());
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("▦ Ranger toutes les fenêtres en grille").clicked() {
                arranger_tout();
            }
            if ui.button("⟳ Liste des fenêtres").clicked() || self.liste.is_empty() {
                self.liste = fenetres();
            }
        });
        ui.strong("📌 Toujours au-dessus");
        egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
            for (h, t) in &self.liste {
                let mut oui = est_au_dessus(*h as HWND);
                if ui.checkbox(&mut oui, t.chars().take(80).collect::<String>()).changed() {
                    au_dessus(*h as HWND, oui);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 46 · Points de restauration
// ---------------------------------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct Point {
    #[serde(rename = "SequenceNumber")]
    numero: u32,
    #[serde(rename = "Description")]
    description: String,
    #[serde(rename = "D")]
    date: String,
}

const PS_POINTS: &str = r#"$p = @(Get-ComputerRestorePoint -ErrorAction SilentlyContinue | ForEach-Object { [pscustomobject]@{ SequenceNumber = $_.SequenceNumber; Description = $_.Description; D = ([Management.ManagementDateTimeConverter]::ToDateTime($_.CreationTime)).ToString('dd/MM/yyyy HH:mm') } })
ConvertTo-Json -Compress -InputObject $p"#;

pub struct Restauration {
    points: Arc<Mutex<Option<Result<Vec<Point>, String>>>>,
    nom: String,
    e: Etat,
}

impl Restauration {
    pub fn new() -> Self {
        Restauration { points: Arc::default(), nom: "Avant installation".into(), e: Arc::default() }
    }

    fn charger(&self) {
        let p = self.points.clone();
        std::thread::spawn(move || {
            let r = ps(PS_POINTS).and_then(|s| {
                let s = s.trim();
                if s.is_empty() || s == "null" { Ok(vec![]) } else { serde_json::from_str::<Vec<Point>>(s).map_err(|e| e.to_string()) }
            });
            *p.lock().unwrap() = Some(r.map(|mut v| {
                v.reverse();
                v
            }));
        });
    }
}

impl Outil for Restauration {
    fn nom(&self) -> &'static str {
        "🛟 Points de restauration"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Points de restauration de Windows");
        ui.weak("Une « photo » de Windows (pilotes, registre, programmes) à laquelle revenir si une installation casse quelque chose. Tes fichiers personnels ne sont pas touchés.");
        let admin = admin_requis(ui);
        let occupe = etat_ui(ui, &self.e);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nom).desired_width(220.0));
            if ui.add_enabled(admin && !occupe, egui::Button::new("📸 Créer un point maintenant")).clicked() {
                let n = ps_echappe(&self.nom);
                let p = self.points.clone();
                en_fond(&self.e, move || {
                    // Windows limite à un point par 24 h sans ce réglage
                    let _ = crate::optimiseur::reg_d(r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore", "SystemRestorePointCreationFrequency", 0);
                    ps(&format!("Enable-ComputerRestore -Drive \"$env:SystemDrive\\\" -ErrorAction SilentlyContinue; Checkpoint-Computer -Description '{n}' -RestorePointType MODIFY_SETTINGS -ErrorAction Stop; 'ok'"))?;
                    *p.lock().unwrap() = None;
                    Ok("✔ Point de restauration créé.".into())
                });
            }
        });
        ui.horizontal(|ui| {
            if ui.button("⏪ Revenir à un point… (assistant Windows)").clicked() {
                let _ = std::process::Command::new("rstrui.exe").spawn();
            }
            if ui.button("⟳").clicked() {
                *self.points.lock().unwrap() = None;
            }
        });
        let l = self.points.lock().unwrap().clone();
        match l {
            None => {
                *self.points.lock().unwrap() = Some(Err("lecture…".into()));
                self.charger();
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.label(if admin { "Aucun point de restauration (la protection système est peut-être désactivée : créer un point l'active)." } else { "Aucun point visible (il faut les droits admin pour les lister)." });
            }
            Some(Ok(v)) => {
                egui::Grid::new("points").striped(true).show(ui, |ui| {
                    for p in &v {
                        ui.label(&p.date);
                        ui.label(&p.description);
                        ui.weak(format!("n° {}", p.numero));
                        ui.end_row();
                    }
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 47 · Analyse antivirus (Microsoft Defender)
// ---------------------------------------------------------------------------------------------------

fn mpcmdrun() -> std::path::PathBuf {
    // la version la plus récente est dans ProgramData\…\Platform\<version>
    let plat = std::path::PathBuf::from(std::env::var("ProgramData").unwrap_or(r"C:\ProgramData".into())).join(r"Microsoft\Windows Defender\Platform");
    let mut v: Vec<std::path::PathBuf> = std::fs::read_dir(&plat).into_iter().flatten().flatten().map(|e| e.path().join("MpCmdRun.exe")).filter(|p| p.exists()).collect();
    v.sort();
    v.pop().unwrap_or_else(|| std::path::PathBuf::from(std::env::var("ProgramFiles").unwrap_or(r"C:\Program Files".into())).join(r"Windows Defender\MpCmdRun.exe"))
}

/// Résumé d'une sortie de MpCmdRun.
pub fn resume_analyse(s: &str) -> String {
    if s.contains("found no threats") || s.contains("aucune menace") {
        "✔ Aucune menace trouvée.".into()
    } else if let Some(l) = s.lines().find(|l| l.contains("Threat") || l.contains("Menace")) {
        format!("⚠ {}", l.trim())
    } else {
        s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("Terminé.").trim().to_string()
    }
}

const PS_DEFENDER: &str = r#"$s = Get-MpComputerStatus -ErrorAction Stop
$m = @(Get-MpThreatDetection -ErrorAction SilentlyContinue | Select-Object -First 10 | ForEach-Object { [pscustomobject]@{ n = (Get-MpThreat -ThreatID $_.ThreatID -ErrorAction SilentlyContinue).ThreatName; f = ($_.Resources -join ', '); d = $_.InitialDetectionTime.ToString('dd/MM/yyyy HH:mm') } })
ConvertTo-Json -Compress -InputObject ([pscustomobject]@{ rt = $s.RealTimeProtectionEnabled; sig = $s.AntivirusSignatureLastUpdated.ToString('dd/MM/yyyy HH:mm'); rapide = $s.QuickScanAge; menaces = $m })"#;

pub struct Antivirus {
    statut: Arc<Mutex<Option<Result<serde_json::Value, String>>>>,
    e: Etat,
}

impl Antivirus {
    pub fn new() -> Self {
        Antivirus { statut: Arc::default(), e: Arc::default() }
    }

    fn analyser(&self, args: Vec<String>, quoi: &'static str) {
        let st = self.statut.clone();
        en_fond(&self.e, move || {
            let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let o = std::process::Command::new(mpcmdrun()).args(&a).creation_flags(SANS_FENETRE).output().map_err(|e| format!("Defender introuvable : {e}"))?;
            let s = crate::systeme2::texte_console(&o.stdout);
            *st.lock().unwrap() = None;
            let r = resume_analyse(&s);
            crate::systeme::notifier(&format!("Analyse {quoi}"), &r);
            Ok(format!("{quoi} : {r}"))
        });
    }
}

impl Outil for Antivirus {
    fn nom(&self) -> &'static str {
        "🛡 Analyse antivirus"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Analyse antivirus (Microsoft Defender)");
        ui.weak("L'antivirus intégré à Windows, piloté d'ici : rapide, complet, ou juste un fichier téléchargé.");
        let occupe = etat_ui(ui, &self.e);
        if let Some(f) = ui.ctx().input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone())) {
            if !occupe {
                self.analyser(vec!["-Scan".into(), "-ScanType".into(), "3".into(), "-File".into(), f.display().to_string(), "-DisableRemediation".into()], "du fichier");
            }
        }
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!occupe, |ui| {
                if ui.button("⚡ Analyse rapide").clicked() {
                    self.analyser(vec!["-Scan".into(), "-ScanType".into(), "1".into()], "rapide");
                }
                if ui.button("🔬 Analyse complète (long)").clicked() {
                    self.analyser(vec!["-Scan".into(), "-ScanType".into(), "2".into()], "complète");
                }
                if ui.button("📄 Analyser un fichier / dossier…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_file() {
                        self.analyser(vec!["-Scan".into(), "-ScanType".into(), "3".into(), "-File".into(), p.display().to_string()], "du fichier");
                    }
                }
                if ui.button("⬇ Mettre à jour les définitions").clicked() {
                    self.analyser(vec!["-SignatureUpdate".into()], "mise à jour");
                }
            });
        });
        ui.weak("(tu peux aussi glisser un fichier sur la fenêtre)");
        ui.separator();
        let s = self.statut.lock().unwrap().clone();
        match s {
            None => {
                *self.statut.lock().unwrap() = Some(Err("lecture de l'état…".into()));
                let st = self.statut.clone();
                std::thread::spawn(move || *st.lock().unwrap() = Some(ps(PS_DEFENDER).and_then(|s| serde_json::from_str(s.trim()).map_err(|e| e.to_string()))));
            }
            Some(Err(e)) => {
                ui.weak(if e.contains("lecture") { e } else { "État de Defender illisible (un autre antivirus est peut-être installé).".into() });
            }
            Some(Ok(j)) => {
                if j["rt"].as_bool() == Some(true) {
                    ui.colored_label(Color32::LIGHT_GREEN, "✔ Protection en temps réel active");
                } else {
                    ui.colored_label(Color32::LIGHT_RED, "✖ Protection en temps réel désactivée");
                }
                ui.label(format!("Définitions du {} · dernière analyse rapide il y a {} jour(s)", j["sig"].as_str().unwrap_or("?"), j["rapide"].as_u64().map(|x| x.to_string()).unwrap_or("?".into())));
                let m = j["menaces"].as_array().cloned().unwrap_or_default();
                if !m.is_empty() {
                    ui.strong("Menaces détectées récemment");
                    for x in m {
                        ui.label(format!("⚠ {} — {} — {}", x["d"].as_str().unwrap_or(""), x["n"].as_str().unwrap_or("?"), x["f"].as_str().unwrap_or("").chars().take(100).collect::<String>()));
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 48 · Pare-feu par application
// ---------------------------------------------------------------------------------------------------

pub fn nom_regle(exe: &str) -> String {
    let n = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    format!("Outils - bloquer {n}")
}

/// Programmes en cours d'exécution : chemins complets, sans doublons, triés.
fn programmes_lances() -> Vec<String> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::*;
    let mut v = std::collections::BTreeSet::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
            if !h.is_null() {
                let mut b = [0u16; 1024];
                let mut n = 1024u32;
                if QueryFullProcessImageNameW(h, 0, b.as_mut_ptr(), &mut n) != 0 {
                    let p = String::from_utf16_lossy(&b[..n as usize]);
                    if !p.to_lowercase().starts_with(r"c:\windows") {
                        v.insert(p);
                    }
                }
                windows_sys::Win32::Foundation::CloseHandle(h);
            }
            ok = Process32NextW(snap, &mut e);
        }
        windows_sys::Win32::Foundation::CloseHandle(snap);
    }
    v.into_iter().collect()
}

pub struct PareFeu {
    bloques: Vec<String>,
    lances: Vec<String>,
    filtre: String,
    message: String,
}

impl PareFeu {
    pub fn new() -> Self {
        PareFeu { bloques: charger("pare_feu"), lances: vec![], filtre: String::new(), message: String::new() }
    }

    fn bloquer(&mut self, exe: &str) {
        let n = nom_regle(exe);
        let _ = cmd("netsh", &["advfirewall", "firewall", "delete", "rule", &format!("name={n}")]);
        let mut ok = true;
        for dir in ["out", "in"] {
            ok &= cmd("netsh", &["advfirewall", "firewall", "add", "rule", &format!("name={n}"), &format!("dir={dir}"), "action=block", &format!("program={exe}"), "enable=yes"]).is_ok();
        }
        if ok {
            if !self.bloques.iter().any(|x| x == exe) {
                self.bloques.push(exe.to_string());
            }
            enregistrer("pare_feu", &self.bloques);
            self.message = format!("🚫 {} n'a plus accès à internet.", nom_regle(exe).trim_start_matches("Outils - bloquer "));
        } else {
            self.message = "✖ Impossible de créer la règle (droits administrateur ?).".into();
        }
    }
}

impl Outil for PareFeu {
    fn nom(&self) -> &'static str {
        "🧱 Couper internet à une appli"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Couper internet à une application");
        ui.weak("Bloque un programme dans le pare-feu Windows : il marche toujours, mais sans internet (jeu solo qui se met à jour tout seul, logiciel trop bavard…).");
        let admin = admin_requis(ui);
        ui.add_enabled_ui(admin, |ui| {
            if ui.button("📄 Choisir un programme (.exe)…").clicked() {
                if let Some(p) = rfd::FileDialog::new().add_filter("Programme", &["exe"]).pick_file() {
                    self.bloquer(&p.display().to_string());
                }
            }
            ui.strong("Programmes bloqués");
            if self.bloques.is_empty() {
                ui.weak("Aucun.");
            }
            let mut retirer = None;
            for (i, b) in self.bloques.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("🚫 {b}"));
                    if ui.small_button("✔ Rendre internet").clicked() {
                        let _ = cmd("netsh", &["advfirewall", "firewall", "delete", "rule", &format!("name={}", nom_regle(b))]);
                        retirer = Some(i);
                    }
                });
            }
            if let Some(i) = retirer {
                self.bloques.remove(i);
                enregistrer("pare_feu", &self.bloques);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.strong("Programmes ouverts en ce moment");
                if ui.small_button("⟳").clicked() || self.lances.is_empty() {
                    self.lances = programmes_lances();
                }
                ui.add(egui::TextEdit::singleline(&mut self.filtre).hint_text("filtrer").desired_width(140.0));
            });
            let f = self.filtre.to_lowercase();
            let mut choisi = None;
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for p in self.lances.iter().filter(|p| p.to_lowercase().contains(&f)) {
                    ui.horizontal(|ui| {
                        if !self.bloques.contains(p) && ui.small_button("🚫").on_hover_text("Bloquer").clicked() {
                            choisi = Some(p.clone());
                        }
                        ui.label(p);
                    });
                }
            });
            if let Some(p) = choisi {
                self.bloquer(&p);
            }
        });
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 49 · Qui utilise la webcam, le micro, la position
// ---------------------------------------------------------------------------------------------------

const CONSENT: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

/// Nom lisible d'une clé : « C:#Program Files#Discord#Discord.exe » → « Discord.exe », sinon le nom du paquet.
pub fn nom_app(cle: &str) -> String {
    if cle.contains('#') {
        cle.rsplit('#').next().unwrap_or(cle).to_string()
    } else {
        cle.split('_').next().unwrap_or(cle).rsplit('.').next().unwrap_or(cle).to_string()
    }
}

pub fn filetime_unix(ft: u64) -> i64 {
    (ft / 10_000_000) as i64 - 11_644_473_600
}

fn qword(p: &str, n: &str) -> u64 {
    reg_lire(p, n).filter(|(_, b)| b.len() >= 8).map(|(_, b)| u64::from_le_bytes(b[..8].try_into().unwrap())).unwrap_or(0)
}

#[derive(Clone)]
pub struct Usage {
    pub appareil: &'static str,
    pub app: String,
    pub en_cours: bool,
    pub dernier: i64,
}

fn usages() -> Vec<Usage> {
    let mut v = vec![];
    for (cap, nom) in [("webcam", "📷 Webcam"), ("microphone", "🎙 Micro"), ("location", "📍 Position")] {
        let base = format!(r"{CONSENT}\{cap}");
        let mut cles: Vec<String> = reg_sous_cles(&base).into_iter().filter(|c| c != "NonPackaged").map(|c| format!(r"{base}\{c}")).collect();
        cles.extend(reg_sous_cles(&format!(r"{base}\NonPackaged")).into_iter().map(|c| format!(r"{base}\NonPackaged\{c}")));
        for k in cles {
            let (debut, fin) = (qword(&k, "LastUsedTimeStart"), qword(&k, "LastUsedTimeStop"));
            if debut == 0 {
                continue;
            }
            v.push(Usage { appareil: nom, app: nom_app(k.rsplit('\\').next().unwrap_or("")), en_cours: fin == 0 || fin < debut, dernier: filetime_unix(debut) });
        }
    }
    v.sort_by(|a, b| b.en_cours.cmp(&a.en_cours).then(b.dernier.cmp(&a.dernier)));
    v
}

fn il_y_a(t: i64) -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) - t;
    match d {
        ..=59 => "à l'instant".into(),
        60..=3599 => format!("il y a {} min", d / 60),
        3600..=86_399 => format!("il y a {} h", d / 3600),
        _ => format!("il y a {} j", d / 86_400),
    }
}

pub struct Espion {
    alerte: Arc<AtomicBool>,
    actifs: Arc<Mutex<Vec<Usage>>>,
}

impl Espion {
    pub fn new() -> Self {
        let alerte = Arc::new(AtomicBool::new(charger::<Option<bool>>("alerte_webcam").unwrap_or(true)));
        let actifs: Arc<Mutex<Vec<Usage>>> = Arc::default();
        let (a, ac) = (alerte.clone(), actifs.clone());
        std::thread::spawn(move || {
            let mut avant: Vec<(String, &str)> = usages().into_iter().filter(|u| u.en_cours).map(|u| (u.app, u.appareil)).collect();
            loop {
                std::thread::sleep(Duration::from_secs(3));
                let l: Vec<Usage> = usages().into_iter().filter(|u| u.en_cours).collect();
                for u in &l {
                    if !avant.iter().any(|(x, y)| *x == u.app && *y == u.appareil) && a.load(Relaxed) && u.appareil != "📍 Position" {
                        crate::systeme::notifier_toujours(&format!("{} utilisé(e)", u.appareil), &format!("{} vient de l'allumer.", u.app));
                    }
                }
                avant = l.iter().map(|u| (u.app.clone(), u.appareil)).collect();
                *ac.lock().unwrap() = l;
            }
        });
        Espion { alerte, actifs }
    }
}

impl Outil for Espion {
    fn nom(&self) -> &'static str {
        "👁 Webcam et micro : qui écoute ?"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn resume(&self) -> Option<String> {
        let a = self.actifs.lock().unwrap();
        let v: Vec<String> = a.iter().filter(|u| u.appareil != "📍 Position").map(|u| format!("{} {}", u.appareil, u.app)).collect();
        (!v.is_empty()).then(|| format!("En cours : {}", v.join(", ")))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Qui utilise la webcam, le micro, la position ?");
        let mut a = self.alerte.load(Relaxed);
        if ui.checkbox(&mut a, "Me prévenir dès qu'une appli allume la webcam ou le micro").changed() {
            self.alerte.store(a, Relaxed);
            enregistrer("alerte_webcam", &Some(a));
        }
        let l = usages();
        if l.is_empty() {
            ui.weak("Aucune utilisation enregistrée par Windows.");
        }
        egui::Grid::new("usages").striped(true).show(ui, |ui| {
            for u in l.iter().take(60) {
                ui.label(u.appareil);
                ui.label(RichText::new(&u.app).strong());
                if u.en_cours {
                    ui.colored_label(Color32::LIGHT_RED, "● EN COURS");
                } else {
                    ui.weak(il_y_a(u.dernier));
                }
                ui.end_row();
            }
        });
        if ui.button("⚙ Réglages de confidentialité Windows").clicked() {
            let _ = std::process::Command::new("explorer").arg("ms-settings:privacy-webcam").spawn();
        }
        ui.ctx().request_repaint_after(Duration::from_secs(2));
    }
}

// ---------------------------------------------------------------------------------------------------
// 50 · Rappel de redémarrage (Windows Update)
// ---------------------------------------------------------------------------------------------------

fn redemarrage_attendu() -> bool {
    let cle_existe = |p: &str| !reg_sous_cles(p).is_empty() || !reg_noms(p).is_empty() || reg_lire(p, "").is_some();
    cle_existe(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired")
        || cle_existe(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending")
}

/// Secondes jusqu'à la prochaine heure h:00 (locale), depuis (heure, minute, seconde) actuelles.
pub fn secondes_jusqua(h: u32, maintenant: (u32, u32, u32)) -> u32 {
    let (hh, mm, ss) = maintenant;
    let now = hh * 3600 + mm * 60 + ss;
    let cible = h * 3600;
    if cible > now { cible - now } else { cible + 86_400 - now }
}

fn heure_locale() -> (u32, u32, u32) {
    let h = crate::minecraft::horodatage(); // AAAA-MM-JJ_HH-MM-SS
    let n = |a: usize| h.get(a..a + 2).and_then(|x| x.parse().ok()).unwrap_or(0);
    (n(11), n(14), n(17))
}

pub struct RappelMaj {
    attendu: Arc<AtomicBool>,
    heure: u32,
    actif: Arc<AtomicBool>,
    message: String,
}

impl RappelMaj {
    pub fn new() -> Self {
        let attendu: Arc<AtomicBool> = Arc::default();
        let actif = Arc::new(AtomicBool::new(charger::<Option<bool>>("rappel_maj").unwrap_or(true)));
        let (at, ac) = (attendu.clone(), actif.clone());
        std::thread::spawn(move || {
            let mut dernier: Option<Instant> = None;
            loop {
                let r = redemarrage_attendu();
                at.store(r, Relaxed);
                // rappel toutes les 4 h, jamais en pleine partie
                if r && ac.load(Relaxed) && crate::outil::jeu_actif().is_none() && dernier.map_or(true, |d| d.elapsed() > Duration::from_secs(4 * 3600)) {
                    crate::systeme::notifier_toujours("Redémarrage en attente", "Windows a installé des mises à jour : redémarre quand ça t'arrange (onglet « Rappel de redémarrage » pour le programmer).");
                    dernier = Some(Instant::now());
                }
                std::thread::sleep(Duration::from_secs(30 * 60));
            }
        });
        RappelMaj { attendu, heure: 3, actif, message: String::new() }
    }
}

impl Outil for RappelMaj {
    fn nom(&self) -> &'static str {
        "🔄 Rappel de redémarrage"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn resume(&self) -> Option<String> {
        self.attendu.load(Relaxed).then(|| "🔄 Redémarrage en attente pour les mises à jour Windows".into())
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mises à jour Windows : rappel de redémarrage");
        let mut a = self.actif.load(Relaxed);
        if ui.checkbox(&mut a, "Me le rappeler (toutes les 4 h, jamais pendant un jeu)").changed() {
            self.actif.store(a, Relaxed);
            enregistrer("rappel_maj", &Some(a));
        }
        if ui.small_button("⟳ Vérifier").clicked() {
            self.attendu.store(redemarrage_attendu(), Relaxed);
        }
        if self.attendu.load(Relaxed) {
            ui.colored_label(Color32::from_rgb(250, 210, 90), RichText::new("🔄 Un redémarrage est nécessaire pour finir les mises à jour.").size(18.0));
        } else {
            ui.colored_label(Color32::LIGHT_GREEN, "✔ Aucun redémarrage en attente.");
        }
        ui.horizontal(|ui| {
            if ui.button("Redémarrer dans 1 min").clicked() {
                self.message = cmd("shutdown", &["/r", "/t", "60", "/c", "Redémarrage demandé depuis Outils"]).map(|_| "Redémarrage dans 1 minute (Annuler pour l'empêcher).".into()).unwrap_or_else(|e| e);
            }
            ui.label("ou à");
            ui.add(egui::DragValue::new(&mut self.heure).range(0..=23).suffix(" h"));
            if ui.button("Programmer").clicked() {
                let s = secondes_jusqua(self.heure, heure_locale());
                self.message = cmd("shutdown", &["/r", "/t", &s.to_string(), "/c", "Redémarrage programmé depuis Outils"])
                    .map(|_| format!("Redémarrage programmé à {} h (dans {} h {:02}). Pense à enregistrer ton travail.", self.heure, s / 3600, s % 3600 / 60)).unwrap_or_else(|e| e);
            }
            if ui.button("Annuler").clicked() {
                self.message = cmd("shutdown", &["/a"]).map(|_| "Redémarrage annulé.".into()).unwrap_or_else(|_| "Aucun redémarrage n'était programmé.".into());
            }
        });
        if ui.button("⚙ Ouvrir Windows Update").clicked() {
            let _ = std::process::Command::new("explorer").arg("ms-settings:windowsupdate").spawn();
        }
        ui.label(&self.message);
    }
}

// ---------------------------------------------------------------------------------------------------
// 51 · Démarrage retardé
// ---------------------------------------------------------------------------------------------------

const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const APPROUVE_RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Retarde {
    pub nom: String,
    pub commande: String,
    pub delai: u32,
}

/// Sépare « "C:\a b\x.exe" --min » en (programme, arguments).
pub fn separer_commande(c: &str) -> (String, String) {
    let c = c.trim();
    if let Some(r) = c.strip_prefix('"') {
        if let Some((p, a)) = r.split_once('"') {
            return (p.to_string(), a.trim().to_string());
        }
    }
    let bas = c.to_lowercase();
    if let Some(i) = bas.find(".exe") {
        return (c[..i + 4].to_string(), c[i + 4..].trim().to_string());
    }
    match c.split_once(' ') {
        Some((p, a)) => (p.to_string(), a.trim().to_string()),
        None => (c.to_string(), String::new()),
    }
}

/// Lance les programmes retardés si Windows vient de démarrer (moins de 10 min).
pub fn lancer_retardes() {
    let l: Vec<Retarde> = charger("demarrage_retarde");
    let allume = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() } / 1000;
    if l.is_empty() || allume > 600 {
        return;
    }
    for r in l {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs((r.delai as u64).saturating_sub(allume)));
            let (p, a) = separer_commande(&crate::optimiseur::exp(&r.commande));
            let mut c = std::process::Command::new(&p);
            c.raw_arg(&a);
            if let Err(e) = c.spawn() {
                crate::coeur::noter("Démarrage retardé", &format!("{} : {e}", r.nom));
            }
        });
    }
}

pub struct DemarrageRetarde {
    liste: Vec<Retarde>,
    message: String,
}

impl DemarrageRetarde {
    pub fn new() -> Self {
        DemarrageRetarde { liste: charger("demarrage_retarde"), message: String::new() }
    }

    fn sauver(&self) {
        enregistrer("demarrage_retarde", &self.liste);
    }
}

impl Outil for DemarrageRetarde {
    fn nom(&self) -> &'static str {
        "🐢 Démarrage retardé"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Démarrage retardé");
        ui.weak("Windows lance tout en même temps à l'allumage, ce qui rame. Ici, les programmes non urgents (Discord, Steam, launchers…) démarrent un peu plus tard, l'un après l'autre.");
        if reg_texte(RUN, "Outils").is_none() {
            ui.colored_label(Color32::from_rgb(250, 210, 90), "⚠ Outils doit démarrer avec Windows pour lancer les programmes retardés (onglet Personnaliser).");
        }
        ui.strong("Programmes retardés");
        let mut remettre = None;
        let mut change = false;
        for (i, r) in self.liste.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&r.nom).strong());
                ui.label("après");
                change |= ui.add(egui::DragValue::new(&mut r.delai).range(10..=900).suffix(" s")).changed();
                if ui.small_button("↩ Remettre au démarrage normal").clicked() {
                    remettre = Some(i);
                }
            });
        }
        if self.liste.is_empty() {
            ui.weak("Aucun.");
        }
        if let Some(i) = remettre {
            let r = self.liste.remove(i);
            let mut b = [0u8; 12];
            b[0] = 2; // réactivé
            let _ = reg_b(APPROUVE_RUN, &r.nom, &b);
            change = true;
            self.message = format!("{} redémarrera normalement avec Windows.", r.nom);
        }
        ui.separator();
        ui.strong("Au démarrage de Windows (ton compte)");
        let noms = reg_noms(RUN);
        let deja: Vec<String> = self.liste.iter().map(|r| r.nom.clone()).collect();
        for n in noms.iter().filter(|n| !n.is_empty() && *n != "Outils" && !deja.contains(n)) {
            let c = reg_texte(RUN, n).unwrap_or_default();
            let off = reg_lire(APPROUVE_RUN, n).is_some_and(|(_, b)| b.first().is_some_and(|x| x & 1 == 1));
            ui.horizontal(|ui| {
                if ui.add_enabled(!off, egui::Button::new("🐢 Retarder")).on_disabled_hover_text("Désactivé au démarrage").clicked() {
                    let mut b = [0u8; 12];
                    b[0] = 3; // désactivé pour Windows, c'est Outils qui le lancera
                    if reg_b(APPROUVE_RUN, n, &b).is_ok() {
                        let delai = 30 + 20 * self.liste.len() as u32;
                        self.liste.push(Retarde { nom: n.clone(), commande: c.clone(), delai });
                        change = true;
                    }
                }
                ui.label(n);
                ui.weak(c.chars().take(70).collect::<String>());
            });
        }
        if change {
            self.sauver();
        }
        ui.label(&self.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grilles_et_zones() {
        let t = (0, 0, 1920, 1040);
        assert_eq!(zone(4, t), Some((0, 0, 960, 1040)));
        assert_eq!(zone(3, t), Some((960, 520, 960, 520)));
        assert_eq!(zone(0, t), Some((0, 0, 640, 1040)));
        assert_eq!(zone(11, t), None);
        assert_eq!(grille(1), (1, 1));
        assert_eq!(grille(3), (2, 2));
        assert_eq!(grille(5), (3, 2));
        assert_eq!(grille(9), (3, 3));
    }

    #[test]
    fn divers() {
        assert_eq!(nom_app(r"C:#Program Files#Discord#Discord.exe"), "Discord.exe");
        assert_eq!(nom_app("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "WindowsCamera");
        assert_eq!(filetime_unix(116_444_736_000_000_000), 0);
        assert_eq!(secondes_jusqua(3, (23, 0, 0)), 4 * 3600);
        assert_eq!(secondes_jusqua(3, (2, 59, 0)), 60);
        assert_eq!(separer_commande(r#""C:\Program Files\Steam\steam.exe" -silent"#), (r"C:\Program Files\Steam\steam.exe".into(), "-silent".into()));
        assert_eq!(separer_commande(r"C:\Discord\Update.exe --processStart Discord.exe"), (r"C:\Discord\Update.exe".into(), "--processStart Discord.exe".into()));
        assert_eq!(nom_regle(r"C:\Jeux\x.exe"), "Outils - bloquer x.exe");
        assert!(resume_analyse("Scan starting...\nScan finished.\nScanning C: found no threats.").starts_with('✔'));
        assert_eq!(verdict_stress(98.0, 100.0).1, Color32::LIGHT_RED);
    }
}
