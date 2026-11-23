//! Macros clavier / souris : enregistre une suite de touches et de clics (avec les délais), la rejoue
//! avec un raccourci Ctrl+Alt+F1…F11, en boucle ou N fois. Ctrl+Alt+F12 arrête l'enregistrement ;
//! Échap arrête une macro en cours. Macros dans %APPDATA%\Outils\macros.json.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Ev {
    Touche { vk: u16, haut: bool },
    Clic { bouton: u8, haut: bool }, // 0 gauche, 1 droit, 2 milieu
    Souris { x: i32, y: i32 },       // position absolue à l'écran
    Molette { delta: i32 },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Macro {
    pub nom: String,
    pub evenements: Vec<(u32, Ev)>, // (délai en ms avant l'événement, événement)
    pub touche: u8,                 // 0 = aucune, 1..=11 = Ctrl+Alt+F1..F11
    pub repetitions: u32,           // 0 = en boucle jusqu'à Échap
    pub vitesse: f32,
}

impl Default for Macro {
    fn default() -> Self {
        Macro { nom: "Nouvelle macro".into(), evenements: vec![], touche: 0, repetitions: 1, vitesse: 1.0 }
    }
}

fn fichier() -> PathBuf {
    crate::outil::base().join("macros.json")
}

const MODIFS: [u16; 6] = [0x11, 0xA2, 0xA3, 0x12, 0xA4, 0xA5]; // Ctrl, Alt (et gauche / droite)

/// Retire la fin de l'enregistrement : appuis sur Ctrl / Alt du raccourci d'arrêt (sans relâchement).
pub fn nettoyer_fin(ev: &mut Vec<(u32, Ev)>) {
    while let Some((_, Ev::Touche { vk, haut: false })) = ev.last() {
        if MODIFS.contains(vk) {
            ev.pop();
        } else {
            break;
        }
    }
}

pub fn decrire(e: &Ev) -> String {
    match e {
        Ev::Touche { vk, haut } => format!("{} {}", if *haut { "↑" } else { "↓" }, nom_touche(*vk)),
        Ev::Clic { bouton, haut } => format!("{} clic {}", if *haut { "↑" } else { "↓" }, ["gauche", "droit", "milieu"].get(*bouton as usize).unwrap_or(&"?")),
        Ev::Souris { x, y } => format!("souris ({x}, {y})"),
        Ev::Molette { delta } => format!("molette {}", if *delta > 0 { "↑" } else { "↓" }),
    }
}

pub fn nom_touche(vk: u16) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x08 => "Retour".into(),
        0x09 => "Tab".into(),
        0x0D => "Entrée".into(),
        0x10 | 0xA0 | 0xA1 => "Maj".into(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".into(),
        0x12 | 0xA4 | 0xA5 => "Alt".into(),
        0x1B => "Échap".into(),
        0x20 => "Espace".into(),
        0x25 => "←".into(),
        0x26 => "↑".into(),
        0x27 => "→".into(),
        0x28 => "↓".into(),
        _ => format!("touche {vk:#04X}"),
    }
}

mod win {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    struct Enreg {
        ev: Vec<(u32, Ev)>,
        dernier: Instant,
        dernier_mouvement: Instant,
        souris: bool,
        fini: bool,
    }

    static ENREG: Mutex<Option<Enreg>> = Mutex::new(None);

    fn ajouter(e: Ev) {
        if let Some(x) = ENREG.lock().unwrap().as_mut() {
            let d = x.dernier.elapsed().as_millis().min(60_000) as u32;
            x.dernier = Instant::now();
            x.ev.push((d, e));
        }
    }

    unsafe extern "system" fn clavier(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            let k = &*(lp as *const KBDLLHOOKSTRUCT);
            if k.flags & LLKHF_INJECTED == 0 {
                let haut = matches!(wp as u32, WM_KEYUP | WM_SYSKEYUP);
                let ctrl_alt = GetAsyncKeyState(VK_CONTROL as i32) < 0 && GetAsyncKeyState(VK_MENU as i32) < 0;
                if k.vkCode == VK_F12 as u32 && ctrl_alt {
                    if let Some(x) = ENREG.lock().unwrap().as_mut() {
                        x.fini = true;
                    }
                    return 1; // on avale le F12
                }
                let repete = !haut && ENREG.lock().unwrap().as_ref().and_then(|x| x.ev.iter().rev().find_map(|(_, e)| match e {
                    Ev::Touche { vk, haut } if *vk as u32 == k.vkCode => Some(!*haut),
                    _ => None,
                })).unwrap_or(false);
                if !repete {
                    ajouter(Ev::Touche { vk: k.vkCode as u16, haut });
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }

    unsafe extern "system" fn souris(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            let m = &*(lp as *const MSLLHOOKSTRUCT);
            let (actif, deplacer) = ENREG.lock().unwrap().as_ref().map(|x| (x.souris, x.dernier_mouvement.elapsed() >= Duration::from_millis(15))).unwrap_or((false, false));
            if m.flags & LLMHF_INJECTED == 0 && actif {
                let e = match wp as u32 {
                    WM_LBUTTONDOWN => Some(Ev::Clic { bouton: 0, haut: false }),
                    WM_LBUTTONUP => Some(Ev::Clic { bouton: 0, haut: true }),
                    WM_RBUTTONDOWN => Some(Ev::Clic { bouton: 1, haut: false }),
                    WM_RBUTTONUP => Some(Ev::Clic { bouton: 1, haut: true }),
                    WM_MBUTTONDOWN => Some(Ev::Clic { bouton: 2, haut: false }),
                    WM_MBUTTONUP => Some(Ev::Clic { bouton: 2, haut: true }),
                    WM_MOUSEWHEEL => Some(Ev::Molette { delta: (m.mouseData >> 16) as i16 as i32 }),
                    WM_MOUSEMOVE if deplacer => {
                        if let Some(x) = ENREG.lock().unwrap().as_mut() {
                            x.dernier_mouvement = Instant::now();
                        }
                        Some(Ev::Souris { x: m.pt.x, y: m.pt.y })
                    }
                    _ => None,
                };
                if let Some(e) = e {
                    if !matches!(e, Ev::Souris { .. }) {
                        // position juste avant un clic, pour qu'il tombe au bon endroit
                        ajouter(Ev::Souris { x: m.pt.x, y: m.pt.y });
                    }
                    ajouter(e);
                }
            }
        }
        CallNextHookEx(null_mut(), code, wp, lp)
    }

    /// Enregistre jusqu'à Ctrl+Alt+F12 (bloquant, dans un fil à part).
    pub fn enregistrer(avec_souris: bool) -> Vec<(u32, Ev)> {
        unsafe {
            *ENREG.lock().unwrap() = Some(Enreg { ev: vec![], dernier: Instant::now(), dernier_mouvement: Instant::now(), souris: avec_souris, fini: false });
            let hk = SetWindowsHookExW(WH_KEYBOARD_LL, Some(clavier), null_mut(), 0);
            let hs = SetWindowsHookExW(WH_MOUSE_LL, Some(souris), null_mut(), 0);
            let mut msg: MSG = std::mem::zeroed();
            loop {
                // les crochets « bas niveau » ont besoin d'une boucle de messages dans ce fil
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if ENREG.lock().unwrap().as_ref().is_none_or(|x| x.fini) {
                    break;
                }
                MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 50, QS_ALLINPUT);
            }
            UnhookWindowsHookEx(hk);
            UnhookWindowsHookEx(hs);
            let mut ev = ENREG.lock().unwrap().take().map(|x| x.ev).unwrap_or_default();
            nettoyer_fin(&mut ev);
            ev
        }
    }

    fn envoyer(e: &Ev) {
        unsafe {
            let mut i: INPUT = std::mem::zeroed();
            match *e {
                Ev::Touche { vk, haut } => {
                    i.r#type = INPUT_KEYBOARD;
                    let etendue = matches!(vk, 0x21..=0x2E | 0x5B | 0x5C | 0xA3 | 0xA5);
                    i.Anonymous.ki = KEYBDINPUT {
                        wVk: vk, wScan: MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16,
                        dwFlags: (if haut { KEYEVENTF_KEYUP } else { 0 }) | if etendue { KEYEVENTF_EXTENDEDKEY } else { 0 }, time: 0, dwExtraInfo: 0,
                    };
                }
                Ev::Clic { bouton, haut } => {
                    i.r#type = INPUT_MOUSE;
                    let f = match (bouton, haut) {
                        (0, false) => MOUSEEVENTF_LEFTDOWN,
                        (0, true) => MOUSEEVENTF_LEFTUP,
                        (1, false) => MOUSEEVENTF_RIGHTDOWN,
                        (1, true) => MOUSEEVENTF_RIGHTUP,
                        (_, false) => MOUSEEVENTF_MIDDLEDOWN,
                        (_, true) => MOUSEEVENTF_MIDDLEUP,
                    };
                    i.Anonymous.mi = MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: f, time: 0, dwExtraInfo: 0 };
                }
                Ev::Souris { x, y } => {
                    SetCursorPos(x, y);
                    return;
                }
                Ev::Molette { delta } => {
                    i.r#type = INPUT_MOUSE;
                    i.Anonymous.mi = MOUSEINPUT { dx: 0, dy: 0, mouseData: delta as u32, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: 0 };
                }
            }
            SendInput(1, &i, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn echap_appuye() -> bool {
        unsafe { GetAsyncKeyState(VK_ESCAPE as i32) < 0 }
    }

    /// Rejoue la macro ; s'arrête si `stop` passe à vrai ou si Échap est enfoncé.
    pub fn jouer(m: &Macro, stop: &AtomicBool) {
        let v = m.vitesse.clamp(0.1, 10.0);
        let mut n = 0;
        let mut enfoncees: Vec<Ev> = vec![];
        'tours: while m.repetitions == 0 || n < m.repetitions {
            for (d, e) in &m.evenements {
                let fin = Instant::now() + Duration::from_millis((*d as f32 / v) as u64);
                while Instant::now() < fin {
                    if stop.load(Relaxed) || echap_appuye() {
                        break 'tours;
                    }
                    std::thread::sleep((fin - Instant::now()).min(Duration::from_millis(10)));
                }
                envoyer(e);
                match e {
                    Ev::Touche { vk, haut: false } => enfoncees.push(Ev::Touche { vk: *vk, haut: true }),
                    Ev::Clic { bouton, haut: false } => enfoncees.push(Ev::Clic { bouton: *bouton, haut: true }),
                    Ev::Touche { haut: true, .. } | Ev::Clic { haut: true, .. } => enfoncees.retain(|x| x != e),
                    _ => {}
                }
            }
            n += 1;
            if m.repetitions == 0 && m.evenements.iter().all(|(d, _)| *d == 0) {
                std::thread::sleep(Duration::from_millis(10)); // évite une boucle folle
            }
        }
        // ne laisse jamais une touche ou un bouton enfoncé
        for e in enfoncees {
            envoyer(&e);
        }
    }

    pub const WM_RECHARGER: u32 = WM_APP + 21;

    /// Fil des raccourcis Ctrl+Alt+F1..F11 : `action(n)` reçoit le numéro de la touche F.
    pub fn raccourcis(touches: Arc<Mutex<Vec<u8>>>, id_fil: Arc<AtomicU32>, mut action: impl FnMut(u8) + Send + 'static) {
        std::thread::spawn(move || unsafe {
            id_fil.store(windows_sys::Win32::System::Threading::GetCurrentThreadId(), Relaxed);
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
            let regler = || {
                for f in 1..=11u8 {
                    UnregisterHotKey(null_mut(), f as i32);
                }
                for f in touches.lock().unwrap().iter().filter(|f| (1..=11).contains(*f)) {
                    crate::outil::raccourci(null_mut(), *f as i32, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, VK_F1 as u32 + *f as u32 - 1);
                }
            };
            regler();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_HOTKEY => action(msg.wParam as u8),
                    WM_RECHARGER => regler(),
                    _ => {}
                }
            }
        });
    }

    pub fn recharger(id: u32) {
        if id != 0 {
            unsafe { PostThreadMessageW(id, WM_RECHARGER, 0, 0) };
        }
    }
}

pub struct Macros {
    liste: Arc<Mutex<Vec<Macro>>>,
    touches: Arc<Mutex<Vec<u8>>>,
    id_fil: Arc<AtomicU32>,
    joue: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    enregistre: Arc<AtomicBool>,
    nouvel: Arc<Mutex<Option<Vec<(u32, Ev)>>>>,
    choisie: Option<usize>,
    avec_souris: bool,
}

fn jouer_fond(m: Macro, joue: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    if joue.swap(true, Relaxed) {
        stop.store(true, Relaxed); // le raccourci pendant la lecture l'arrête
        return;
    }
    stop.store(false, Relaxed);
    std::thread::spawn(move || {
        win::jouer(&m, &stop);
        joue.store(false, Relaxed);
    });
}

impl Macros {
    pub fn new() -> Self {
        let liste: Vec<Macro> = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let touches = Arc::new(Mutex::new(liste.iter().map(|m| m.touche).collect()));
        let liste = Arc::new(Mutex::new(liste));
        let (joue, stop) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let id_fil = Arc::new(AtomicU32::new(0));
        let (l2, j2, s2) = (liste.clone(), joue.clone(), stop.clone());
        win::raccourcis(touches.clone(), id_fil.clone(), move |f| {
            let m = l2.lock().unwrap().iter().find(|m| m.touche == f).cloned();
            if let Some(m) = m {
                std::thread::sleep(Duration::from_millis(150)); // laisse relâcher Ctrl+Alt
                jouer_fond(m, j2.clone(), s2.clone());
            }
        });
        Macros { liste, touches, id_fil, joue, stop, enregistre: Arc::default(), nouvel: Arc::default(), choisie: None, avec_souris: true }
    }

    fn enregistrer(&self) {
        let l = self.liste.lock().unwrap();
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&*l).unwrap_or_default());
        *self.touches.lock().unwrap() = l.iter().map(|m| m.touche).collect();
        win::recharger(self.id_fil.load(Relaxed));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Macros clavier / souris");
        ui.weak("Attention : les jeux en ligne avec anti-triche peuvent interdire les macros.");
        if let Some(ev) = self.nouvel.lock().unwrap().take() {
            if let Some(i) = self.choisie {
                if let Some(m) = self.liste.lock().unwrap().get_mut(i) {
                    m.evenements = ev;
                }
            }
            self.enregistrer();
            crate::systeme::montrer();
        }
        let avant = self.liste.lock().unwrap().clone();
        let mut liste = avant.clone();
        let enregistre = self.enregistre.load(Relaxed);
        let joue = self.joue.load(Relaxed);
        if enregistre || joue {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Nouvelle macro").clicked() {
                liste.push(Macro::default());
                self.choisie = Some(liste.len() - 1);
            }
            if enregistre {
                ui.colored_label(Color32::RED, "⏺ Enregistrement… Ctrl+Alt+F12 pour arrêter");
            }
            if joue {
                ui.colored_label(Color32::from_rgb(255, 120, 170), "▶ Lecture… Échap pour arrêter");
                if ui.button("⏹").clicked() {
                    self.stop.store(true, Relaxed);
                }
            }
        });
        ui.separator();
        let mut supprimer = None;
        let mut action = 0;
        ui.columns(2, |c| {
            for (i, m) in liste.iter().enumerate() {
                let t = if m.touche > 0 { format!(" · Ctrl+Alt+F{}", m.touche) } else { String::new() };
                if c[0].selectable_label(self.choisie == Some(i), format!("{}{t} · {} évén.", m.nom, m.evenements.len())).clicked() {
                    self.choisie = Some(i);
                }
            }
            if liste.is_empty() {
                c[0].weak("Aucune macro.");
            }
            let ui = &mut c[1];
            let prises: Vec<u8> = liste.iter().enumerate().filter(|(i, _)| Some(*i) != self.choisie).map(|(_, m)| m.touche).collect();
            let Some(m) = self.choisie.and_then(|i| liste.get_mut(i)) else {
                ui.weak("Choisis une macro.");
                return;
            };
            ui.add(egui::TextEdit::singleline(&mut m.nom).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                ui.label("Raccourci");
                egui::ComboBox::from_id_salt("touche_macro").selected_text(if m.touche == 0 { "aucun".into() } else { format!("Ctrl+Alt+F{}", m.touche) }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.touche, 0, "aucun");
                    for f in 1..=11u8 {
                        if !prises.contains(&f) {
                            ui.selectable_value(&mut m.touche, f, format!("Ctrl+Alt+F{f}"));
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Répéter");
                ui.add(egui::DragValue::new(&mut m.repetitions).range(0..=10_000));
                ui.weak("fois (0 = en boucle jusqu'à Échap)");
            });
            ui.add(egui::Slider::new(&mut m.vitesse, 0.25..=4.0).text("vitesse").logarithmic(true));
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.avec_souris, "Enregistrer la souris");
                if ui.add_enabled(!enregistre && !joue, egui::Button::new("⏺ Enregistrer")).clicked() {
                    action = 1;
                }
                if ui.add_enabled(!enregistre && !joue && !m.evenements.is_empty(), egui::Button::new("▶ Tester")).clicked() {
                    action = 2;
                }
                if ui.button("🗑").clicked() {
                    supprimer = self.choisie;
                }
            });
            let total: u32 = m.evenements.iter().map(|(d, _)| *d).sum();
            ui.weak(format!("{} événement(s) · {:.1} s", m.evenements.len(), total as f32 / 1000.0 / m.vitesse));
            egui::ScrollArea::vertical().id_salt("ev_macro").max_height(260.0).show(ui, |ui| {
                let mut retirer = None;
                for (k, (d, e)) in m.evenements.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(d).range(0..=60_000).suffix(" ms"));
                        ui.label(decrire(e));
                        if ui.small_button("✖").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
                if let Some(k) = retirer {
                    m.evenements.remove(k);
                }
            });
        });
        match action {
            1 => {
                let (en, nouvel, souris) = (self.enregistre.clone(), self.nouvel.clone(), self.avec_souris);
                en.store(true, Relaxed);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(400));
                    let ev = win::enregistrer(souris);
                    *nouvel.lock().unwrap() = Some(ev);
                    en.store(false, Relaxed);
                });
            }
            2 => {
                if let Some(m) = self.choisie.and_then(|i| liste.get(i)).cloned() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    let (j, s) = (self.joue.clone(), self.stop.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(600));
                        jouer_fond(m, j, s);
                    });
                }
            }
            _ => {}
        }
        if let Some(i) = supprimer {
            liste.remove(i);
            self.choisie = None;
        }
        if liste != avant {
            *self.liste.lock().unwrap() = liste;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fin_nettoyee() {
        let mut v = vec![
            (0, Ev::Touche { vk: 0x41, haut: false }),
            (50, Ev::Touche { vk: 0x41, haut: true }),
            (300, Ev::Touche { vk: 0xA2, haut: false }),
            (20, Ev::Touche { vk: 0xA4, haut: false }),
        ];
        nettoyer_fin(&mut v);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn noms() {
        assert_eq!(nom_touche(0x41), "A");
        assert_eq!(nom_touche(0x75), "F6");
        assert_eq!(decrire(&Ev::Clic { bouton: 1, haut: false }), "↓ clic droit");
    }
}
