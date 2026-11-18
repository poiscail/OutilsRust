//! Bureau et matériel : banc d'essai des disques (98), écrans (103 : dispositions enregistrées, dupliquer /
//! étendre), transparence des fenêtres (106, Ctrl+Alt+Page↑/Page↓/Origine), bureaux virtuels (107).
use crate::outil::{charger, enregistrer, Cat, Outil};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------------------------------------------------------------------------------------------------
// 98 · Banc d'essai disque
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Default, Debug)]
pub struct Mesure {
    pub ecriture: f64, // Mo/s
    pub lecture: f64,
    pub iops_4k: f64,
}

/// Tampon aligné sur 4096 octets (exigé par la lecture / écriture sans cache).
struct Aligne {
    p: *mut u8,
    l: std::alloc::Layout,
}

impl Aligne {
    fn new(n: usize) -> Aligne {
        let l = std::alloc::Layout::from_size_align(n, 4096).unwrap();
        let p = unsafe { std::alloc::alloc(l) };
        for i in (0..n).step_by(4096) {
            unsafe { *p.add(i) = (i / 4096) as u8 }
        }
        Aligne { p, l }
    }
}

impl Drop for Aligne {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.p, self.l) }
    }
}

fn banc(fichier: &std::path::Path, taille_mo: u64, progres: &Mutex<String>) -> Result<Mesure, String> {
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::*;
    const BLOC: usize = 4 << 20;
    let buf = Aligne::new(BLOC);
    let nom = w(&fichier.to_string_lossy());
    let n_blocs = (taille_mo << 20) as usize / BLOC;
    let mut m = Mesure::default();
    unsafe {
        let ouvrir = |acces: u32, creation: u32| CreateFileW(nom.as_ptr(), acces, 0, std::ptr::null(), creation, FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH, std::ptr::null_mut());
        // écriture séquentielle
        let h = ouvrir(GENERIC_WRITE, CREATE_ALWAYS);
        if h == INVALID_HANDLE_VALUE {
            return Err("impossible de créer le fichier de test (droits ?)".into());
        }
        *progres.lock().unwrap() = "Écriture…".into();
        let t = Instant::now();
        for _ in 0..n_blocs {
            let mut ecrit = 0;
            if WriteFile(h, buf.p, BLOC as u32, &mut ecrit, std::ptr::null_mut()) == 0 {
                CloseHandle(h);
                let _ = std::fs::remove_file(fichier);
                return Err("écriture impossible (disque plein ?)".into());
            }
        }
        m.ecriture = (n_blocs * BLOC) as f64 / 1_048_576.0 / t.elapsed().as_secs_f64();
        CloseHandle(h);
        // lecture séquentielle
        *progres.lock().unwrap() = "Lecture…".into();
        let h = ouvrir(GENERIC_READ, OPEN_EXISTING);
        let t = Instant::now();
        for _ in 0..n_blocs {
            let mut lu = 0;
            ReadFile(h, buf.p, BLOC as u32, &mut lu, std::ptr::null_mut());
        }
        m.lecture = (n_blocs * BLOC) as f64 / 1_048_576.0 / t.elapsed().as_secs_f64();
        // lecture aléatoire de 4 Kio pendant 5 s
        *progres.lock().unwrap() = "Accès aléatoires 4K…".into();
        let t = Instant::now();
        let mut ops = 0u64;
        let blocs_4k = (n_blocs * BLOC / 4096) as u64;
        while t.elapsed() < Duration::from_secs(5) {
            let pos = (crate::outil::hasard(blocs_4k) * 4096) as i64;
            SetFilePointerEx(h, pos, std::ptr::null_mut(), FILE_BEGIN);
            let mut lu = 0;
            ReadFile(h, buf.p, 4096, &mut lu, std::ptr::null_mut());
            ops += 1;
        }
        m.iops_4k = ops as f64 / t.elapsed().as_secs_f64();
        CloseHandle(h);
    }
    let _ = std::fs::remove_file(fichier);
    Ok(m)
}

pub fn verdict(m: &Mesure) -> &'static str {
    match m.lecture as u64 {
        0..=200 => "disque dur classique (HDD) ou clé USB",
        201..=600 => "SSD SATA",
        601..=2500 => "SSD NVMe",
        _ => "SSD NVMe rapide (PCIe 4 ou 5)",
    }
}

pub struct BancDisque {
    disque: String,
    taille: u64,
    etat: Arc<Mutex<String>>,
    resultats: Arc<Mutex<Vec<(String, Result<Mesure, String>)>>>,
    occupe: Arc<std::sync::atomic::AtomicBool>,
}

impl BancDisque {
    pub fn new() -> Self {
        BancDisque { disque: "C:\\".into(), taille: 1024, etat: Arc::default(), resultats: Arc::default(), occupe: Arc::default() }
    }
}

impl Outil for BancDisque {
    fn nom(&self) -> &'static str {
        "🏎 Vitesse des disques"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        use std::sync::atomic::Ordering::Relaxed;
        ui.heading("Vitesse des disques");
        let occupe = self.occupe.load(Relaxed);
        ui.horizontal(|ui| {
            for (d, _, _) in crate::systeme::disques() {
                ui.selectable_value(&mut self.disque, d.clone(), &d);
            }
            ui.label("Taille du test");
            for t in [256, 1024, 4096] {
                ui.selectable_value(&mut self.taille, t, format!("{} Go", t as f32 / 1024.0));
            }
            if ui.add_enabled(!occupe, egui::Button::new("▶ Tester")).clicked() {
                let f = if self.disque.to_uppercase().starts_with("C:") { std::env::temp_dir().join("outils_banc.tmp") } else { PathBuf::from(&self.disque).join("outils_banc.tmp") };
                let (e, r, o, t, d) = (self.etat.clone(), self.resultats.clone(), self.occupe.clone(), self.taille, self.disque.clone());
                o.store(true, Relaxed);
                std::thread::spawn(move || {
                    let m = banc(&f, t, &e);
                    r.lock().unwrap().insert(0, (d, m));
                    e.lock().unwrap().clear();
                    o.store(false, Relaxed);
                });
            }
        });
        if occupe {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.etat.lock().unwrap().clone());
            });
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        for (d, r) in self.resultats.lock().unwrap().iter() {
            egui::Frame::group(ui.style()).show(ui, |ui| match r {
                Ok(m) => {
                    ui.horizontal(|ui| {
                        ui.strong(d);
                        ui.label(RichText::new(format!("lecture {:.0} Mo/s", m.lecture)).size(18.0));
                        ui.label(RichText::new(format!("écriture {:.0} Mo/s", m.ecriture)).size(18.0));
                        ui.label(format!("4K aléatoire {:.0} IOPS", m.iops_4k));
                    });
                    ui.weak(format!("Ça ressemble à : {}", verdict(m)));
                }
                Err(e) => {
                    ui.colored_label(Color32::LIGHT_RED, format!("{d} : {e}"));
                }
            });
        }
        ui.weak("Écrit un fichier temporaire (supprimé ensuite) en contournant le cache de Windows, comme CrystalDiskMark.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 103 · Écrans
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
struct Disposition {
    nom: String,
    chemins: String, // base64 des structures Windows
    modes: String,
    nb: (u32, u32),
}

fn ecrans() -> Vec<String> {
    use windows_sys::Win32::Graphics::Gdi::*;
    let mut v = vec![];
    unsafe {
        for i in 0..16 {
            let mut d: DISPLAY_DEVICEW = std::mem::zeroed();
            d.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(std::ptr::null(), i, &mut d, 0) == 0 {
                break;
            }
            if d.StateFlags & DISPLAY_DEVICE_ACTIVE == 0 {
                continue;
            }
            let mut m: DEVMODEW = std::mem::zeroed();
            m.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
            EnumDisplaySettingsW(d.DeviceName.as_ptr(), ENUM_CURRENT_SETTINGS, &mut m);
            let mut ecran: DISPLAY_DEVICEW = std::mem::zeroed();
            ecran.cb = d.cb;
            EnumDisplayDevicesW(d.DeviceName.as_ptr(), 0, &mut ecran, 0);
            let txt = |t: &[u16]| String::from_utf16_lossy(&t[..t.iter().position(|&c| c == 0).unwrap_or(t.len())]);
            let principal = if d.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 { " (principal)" } else { "" };
            v.push(format!("{}{principal} : {}×{} à {} Hz", txt(&ecran.DeviceString), m.dmPelsWidth, m.dmPelsHeight, m.dmDisplayFrequency));
        }
    }
    v
}

fn lire_disposition(nom: &str) -> Option<Disposition> {
    use windows_sys::Win32::Devices::Display::*;
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != 0 {
            return None;
        }
        let mut p: Vec<DISPLAYCONFIG_PATH_INFO> = vec![std::mem::zeroed(); np as usize];
        let mut m: Vec<DISPLAYCONFIG_MODE_INFO> = vec![std::mem::zeroed(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, p.as_mut_ptr(), &mut nm, m.as_mut_ptr(), std::ptr::null_mut()) != 0 {
            return None;
        }
        let octets = |ptr: *const u8, n: usize| std::slice::from_raw_parts(ptr, n).to_vec();
        Some(Disposition {
            nom: nom.into(),
            chemins: B64.encode(octets(p.as_ptr().cast(), np as usize * std::mem::size_of::<DISPLAYCONFIG_PATH_INFO>())),
            modes: B64.encode(octets(m.as_ptr().cast(), nm as usize * std::mem::size_of::<DISPLAYCONFIG_MODE_INFO>())),
            nb: (np, nm),
        })
    }
}

fn appliquer_disposition(d: &Disposition) -> Result<(), String> {
    use windows_sys::Win32::Devices::Display::*;
    let p = B64.decode(&d.chemins).map_err(|e| e.to_string())?;
    let m = B64.decode(&d.modes).map_err(|e| e.to_string())?;
    if p.len() != d.nb.0 as usize * std::mem::size_of::<DISPLAYCONFIG_PATH_INFO>() || m.len() != d.nb.1 as usize * std::mem::size_of::<DISPLAYCONFIG_MODE_INFO>() {
        return Err("disposition abîmée".into());
    }
    let r = unsafe {
        SetDisplayConfig(d.nb.0, p.as_ptr().cast(), d.nb.1, m.as_ptr().cast(), SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_SAVE_TO_DATABASE | SDC_ALLOW_CHANGES)
    };
    if r == 0 { Ok(()) } else { Err(format!("Windows a refusé cette disposition (écran débranché ?) : code {r}")) }
}

fn topologie(t: u32) {
    use windows_sys::Win32::Devices::Display::{SetDisplayConfig, SDC_APPLY};
    unsafe { SetDisplayConfig(0, std::ptr::null(), 0, std::ptr::null(), t | SDC_APPLY) };
}

pub struct Ecrans {
    liste: Vec<String>,
    dispositions: Vec<Disposition>,
    nouveau: String,
    message: String,
}

impl Ecrans {
    pub fn new() -> Self {
        Ecrans { liste: vec![], dispositions: charger("ecrans"), nouveau: String::new(), message: String::new() }
    }
}

impl Outil for Ecrans {
    fn nom(&self) -> &'static str {
        "🖥 Écrans"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        use windows_sys::Win32::Devices::Display::*;
        ui.heading("Écrans");
        if self.liste.is_empty() {
            self.liste = ecrans();
        }
        for e in &self.liste {
            ui.label(format!("🖥 {e}"));
        }
        ui.horizontal(|ui| {
            for (n, t) in [("Écran du PC seul", SDC_TOPOLOGY_INTERNAL), ("Dupliquer", SDC_TOPOLOGY_CLONE), ("Étendre", SDC_TOPOLOGY_EXTEND), ("Second écran seul", SDC_TOPOLOGY_EXTERNAL)] {
                if ui.button(n).clicked() {
                    topologie(t);
                    self.liste.clear();
                }
            }
            if ui.button("⟳").clicked() {
                self.liste.clear();
            }
        });
        ui.separator();
        ui.strong("Dispositions enregistrées");
        ui.weak("Place tes écrans comme tu veux dans les paramètres Windows, puis enregistre la disposition pour la retrouver en un clic.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau).hint_text("nom (ex. Jeu, Travail)").desired_width(180.0));
            if ui.button("💾 Enregistrer la disposition actuelle").clicked() && !self.nouveau.trim().is_empty() {
                match lire_disposition(self.nouveau.trim()) {
                    Some(d) => {
                        self.dispositions.retain(|x| x.nom != d.nom);
                        self.dispositions.push(d);
                        enregistrer("ecrans", &self.dispositions);
                        self.nouveau.clear();
                    }
                    None => self.message = "Lecture de la disposition impossible.".into(),
                }
            }
        });
        let mut retirer = None;
        for (i, d) in self.dispositions.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(format!("▶ {}", d.nom)).clicked() {
                    self.message = appliquer_disposition(d).map(|_| format!("« {} » appliquée.", d.nom)).unwrap_or_else(|e| e);
                    self.liste.clear();
                }
                if ui.small_button("🗑").clicked() {
                    retirer = Some(i);
                }
            });
        }
        if let Some(i) = retirer {
            self.dispositions.remove(i);
            enregistrer("ecrans", &self.dispositions);
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 106 · Transparence des fenêtres
// ---------------------------------------------------------------------------------------------------

pub fn alpha_suivant(a: u8, plus: bool) -> u8 {
    if plus { a.saturating_add(25) } else { a.saturating_sub(25).max(40) }
}

pub struct Transparence {
    actif: Arc<std::sync::atomic::AtomicBool>,
    pris: Arc<std::sync::atomic::AtomicBool>,
}

impl Transparence {
    pub fn new() -> Self {
        use std::sync::atomic::Ordering::Relaxed;
        let r: bool = charger::<Option<bool>>("transparence").unwrap_or(true);
        let t = Transparence { actif: Arc::new(std::sync::atomic::AtomicBool::new(r)), pris: Arc::default() };
        let (a, p) = (t.actif.clone(), t.pris.clone());
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            let mut ok = true;
            for (id, vk) in [(1, VK_PRIOR), (2, VK_NEXT), (3, VK_HOME)] {
                ok &= RegisterHotKey(std::ptr::null_mut(), id, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, vk as u32) != 0;
            }
            p.store(!ok, Relaxed);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message != WM_HOTKEY || !a.load(Relaxed) {
                    continue;
                }
                let h = GetForegroundWindow();
                if h.is_null() {
                    continue;
                }
                let style = GetWindowLongW(h, GWL_EXSTYLE) as u32;
                let (mut cle, mut alpha, mut drapeaux) = (0u32, 255u8, 0u32);
                if style & WS_EX_LAYERED == 0 || GetLayeredWindowAttributes(h, &mut cle, &mut alpha, &mut drapeaux) == 0 || drapeaux & LWA_ALPHA == 0 {
                    alpha = 255;
                }
                let nouveau = match msg.wParam {
                    1 => alpha_suivant(alpha, true),
                    2 => alpha_suivant(alpha, false),
                    _ => 255,
                };
                if nouveau == 255 {
                    SetLayeredWindowAttributes(h, 0, 255, LWA_ALPHA);
                    if msg.wParam == 3 {
                        SetWindowLongW(h, GWL_EXSTYLE, (style & !WS_EX_LAYERED) as i32);
                    }
                } else {
                    SetWindowLongW(h, GWL_EXSTYLE, (style | WS_EX_LAYERED) as i32);
                    SetLayeredWindowAttributes(h, 0, nouveau, LWA_ALPHA);
                }
            }
        });
        t
    }
}

impl Outil for Transparence {
    fn nom(&self) -> &'static str {
        "👻 Transparence des fenêtres"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        use std::sync::atomic::Ordering::Relaxed;
        ui.heading("Transparence des fenêtres");
        let mut a = self.actif.load(Relaxed);
        if ui.checkbox(&mut a, "Raccourcis actifs").changed() {
            self.actif.store(a, Relaxed);
            enregistrer("transparence", &Some(a));
        }
        if self.pris.load(Relaxed) {
            ui.colored_label(Color32::LIGHT_RED, "Un des raccourcis est déjà pris par une autre appli.");
        }
        ui.label("Sur la fenêtre au premier plan :");
        ui.label("  • Ctrl+Alt+Page↓ : plus transparente");
        ui.label("  • Ctrl+Alt+Page↑ : moins transparente");
        ui.label("  • Ctrl+Alt+Origine : redevient normale");
        ui.weak("Pratique pour garder un tuto ou une carte par-dessus un jeu fenêtré, ou surveiller une fenêtre en transparence.");
    }
}

// ---------------------------------------------------------------------------------------------------
// 107 · Bureaux virtuels
// ---------------------------------------------------------------------------------------------------

const CLE_BUREAUX: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops";

fn reg_binaire(sous_cle: &str, valeur: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Registry::*;
    unsafe {
        let mut n = 0u32;
        let (c, v) = (w(sous_cle), w(valeur));
        if RegGetValueW(HKEY_CURRENT_USER, c.as_ptr(), v.as_ptr(), RRF_RT_ANY, std::ptr::null_mut(), std::ptr::null_mut(), &mut n) != 0 {
            return None;
        }
        let mut b = vec![0u8; n as usize];
        if RegGetValueW(HKEY_CURRENT_USER, c.as_ptr(), v.as_ptr(), RRF_RT_ANY, std::ptr::null_mut(), b.as_mut_ptr().cast(), &mut n) != 0 {
            return None;
        }
        b.truncate(n as usize);
        Some(b)
    }
}

/// GUID (16 octets, ordre Windows) → « {xxxxxxxx-xxxx-…} ».
pub fn guid(b: &[u8]) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        u32::from_le_bytes(b[0..4].try_into().unwrap()), u16::from_le_bytes([b[4], b[5]]), u16::from_le_bytes([b[6], b[7]]),
        b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

/// (noms des bureaux, indice du bureau actuel)
fn bureaux() -> (Vec<(String, String)>, usize) {
    let ids = reg_binaire(CLE_BUREAUX, "VirtualDesktopIDs").unwrap_or_default();
    let actuel = reg_binaire(CLE_BUREAUX, "CurrentVirtualDesktop").unwrap_or_default();
    let mut v = vec![];
    let mut i_actuel = 0;
    for (k, c) in ids.chunks(16).filter(|c| c.len() == 16).enumerate() {
        if c == actuel.as_slice() {
            i_actuel = k;
        }
        let g = guid(c);
        let nom = reg_binaire(&format!(r"{CLE_BUREAUX}\Desktops\{g}"), "Name")
            .map(|b| String::from_utf16_lossy(&b.chunks(2).map(|x| u16::from_le_bytes([x[0], *x.get(1).unwrap_or(&0)])).collect::<Vec<_>>()).trim_end_matches('\0').to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Bureau {}", k + 1));
        v.push((g, nom));
    }
    if v.is_empty() {
        v.push((String::new(), "Bureau 1".into()));
    }
    (v, i_actuel)
}

fn touches(seq: &[(u16, bool)]) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    let v: Vec<INPUT> = seq.iter().map(|&(vk, haut)| {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 } | if matches!(vk, 0x25 | 0x27 | 0x5B) { KEYEVENTF_EXTENDEDKEY } else { 0 }, time: 0, dwExtraInfo: 0 };
        i
    }).collect();
    unsafe { SendInput(v.len() as u32, v.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

/// Win+Ctrl+touche
fn win_ctrl(vk: u16) {
    touches(&[(0x5B, false), (0x11, false), (vk, false), (vk, true), (0x11, true), (0x5B, true)]);
}

fn aller_a(cible: usize) {
    let (_, actuel) = bureaux();
    let (vk, n) = if cible > actuel { (0x27, cible - actuel) } else { (0x25, actuel - cible) };
    for _ in 0..n {
        win_ctrl(vk);
        std::thread::sleep(Duration::from_millis(120));
    }
}

pub struct Bureaux {
    renommer: Option<(String, String)>,
    message: String,
}

impl Bureaux {
    pub fn new() -> Self {
        // Ctrl+Alt+1…9 du pavé numérique : aller au bureau N
        std::thread::spawn(|| unsafe {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            use windows_sys::Win32::UI::WindowsAndMessaging::*;
            for k in 1..=9 {
                RegisterHotKey(std::ptr::null_mut(), k, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_NUMPAD0 as u32 + k as u32);
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY {
                    std::thread::sleep(Duration::from_millis(150));
                    aller_a(msg.wParam - 1);
                }
            }
        });
        Bureaux { renommer: None, message: String::new() }
    }
}

impl Outil for Bureaux {
    fn nom(&self) -> &'static str {
        "🗃 Bureaux virtuels"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Bureaux virtuels");
        let (liste, actuel) = bureaux();
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        ui.horizontal_wrapped(|ui| {
            for (i, (g, n)) in liste.iter().enumerate() {
                let rep = ui.add(egui::Button::new(RichText::new(format!("{} {n}", i + 1)).size(16.0)).selected(i == actuel).min_size(egui::vec2(140.0, 40.0)));
                if rep.clicked() {
                    std::thread::spawn(move || aller_a(i));
                }
                if rep.secondary_clicked() {
                    self.renommer = Some((g.clone(), n.clone()));
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("➕ Nouveau bureau").clicked() {
                win_ctrl(0x44); // D
            }
            if ui.button("✖ Fermer le bureau actuel").clicked() {
                win_ctrl(0x73); // F4
            }
        });
        if let Some((g, mut n)) = self.renommer.clone() {
            ui.horizontal(|ui| {
                ui.label("Nouveau nom");
                ui.add(egui::TextEdit::singleline(&mut n).desired_width(160.0));
                if ui.button("OK").clicked() && !g.is_empty() {
                    use std::os::windows::process::CommandExt;
                    let ok = std::process::Command::new("reg.exe").args(["add", &format!(r"HKCU\{CLE_BUREAUX}\Desktops\{g}"), "/v", "Name", "/t", "REG_SZ", "/d", &n, "/f"])
                        .creation_flags(0x0800_0000).status().is_ok_and(|s| s.success());
                    self.message = if ok { "Renommé (Windows affichera le nouveau nom après le prochain redémarrage de l'explorateur).".into() } else { "Échec du renommage.".into() };
                    self.renommer = None;
                    return;
                }
                self.renommer = Some((g, n));
            });
        }
        ui.weak("Clic : aller au bureau · clic droit : renommer · Ctrl+Alt+1…9 du pavé numérique : aller au bureau N, de partout.");
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(alpha_suivant(255, false), 230);
        assert_eq!(alpha_suivant(50, false), 40);
        assert_eq!(alpha_suivant(240, true), 255);
        let b = [0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0x78, 0x56, 1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(guid(&b), "{12345678-1234-5678-0102-030405060708}");
        assert_eq!(verdict(&Mesure { lecture: 3500.0, ..Default::default() }), "SSD NVMe rapide (PCIe 4 ou 5)");
    }
}
