//! Tests du matériel : souris (fréquence d'envoi réelle, doubles clics parasites) (65) et clavier
//! (touches qui répondent, nombre de touches simultanées) (66). Lecture « raw input » dans un fil dédié,
//! qui reçoit les entrées même quand la fenêtre n'a pas le focus.
use crate::outil::{Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Brut {
    pub mouvements: VecDeque<Instant>,   // derniers mouvements bruts (fenêtre d'une seconde)
    pub max_hz: u32,
    pub clics: [VecDeque<Instant>; 3],  // appuis récents par bouton
    pub doubles: [u32; 3],               // appuis trop rapprochés (< 60 ms) : rebond du bouton
    pub total_clics: [u32; 3],
    pub enfoncees: BTreeSet<u16>,        // touches enfoncées (vk)
    pub deja: BTreeSet<u16>,             // touches qui ont répondu au moins une fois
    pub max_simultanees: usize,
}

static BRUT: OnceLock<Arc<Mutex<Brut>>> = OnceLock::new();

/// Démarre (une seule fois) le fil d'écoute brute.
pub fn brut() -> Arc<Mutex<Brut>> {
    BRUT.get_or_init(|| {
        let b: Arc<Mutex<Brut>> = Arc::default();
        win::lancer(b.clone());
        b
    })
    .clone()
}

/// Appuis rapprochés de moins de `seuil` : signe d'un bouton qui « rebondit ».
pub fn est_rebond(precedent: Option<Instant>, maintenant: Instant, seuil: Duration) -> bool {
    precedent.is_some_and(|p| maintenant.duration_since(p) < seuil)
}

mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    static CIBLE: OnceLock<Arc<Mutex<Brut>>> = OnceLock::new();

    unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if m == WM_INPUT {
            let mut taille = 0u32;
            GetRawInputData(lp as HRAWINPUT, RID_INPUT, null_mut(), &mut taille, std::mem::size_of::<RAWINPUTHEADER>() as u32);
            let mut buf = vec![0u8; taille as usize];
            if taille > 0 && GetRawInputData(lp as HRAWINPUT, RID_INPUT, buf.as_mut_ptr().cast(), &mut taille, std::mem::size_of::<RAWINPUTHEADER>() as u32) == taille {
                let r = &*(buf.as_ptr() as *const RAWINPUT);
                if let Some(b) = CIBLE.get() {
                    let mut b = b.lock().unwrap();
                    let maintenant = Instant::now();
                    if r.header.dwType == RIM_TYPEMOUSE {
                        let s = r.data.mouse;
                        if s.lLastX != 0 || s.lLastY != 0 {
                            b.mouvements.push_back(maintenant);
                            while b.mouvements.front().is_some_and(|t| maintenant.duration_since(*t) > Duration::from_secs(1)) {
                                b.mouvements.pop_front();
                            }
                            let hz = b.mouvements.len() as u32;
                            b.max_hz = b.max_hz.max(hz);
                        }
                        let boutons = s.Anonymous.Anonymous.usButtonFlags as u32;
                        for (k, drapeau) in [(0, RI_MOUSE_LEFT_BUTTON_DOWN), (1, RI_MOUSE_RIGHT_BUTTON_DOWN), (2, RI_MOUSE_MIDDLE_BUTTON_DOWN)] {
                            if boutons & drapeau as u32 != 0 {
                                let prec = b.clics[k].back().copied();
                                if est_rebond(prec, maintenant, Duration::from_millis(60)) {
                                    b.doubles[k] += 1;
                                }
                                b.total_clics[k] += 1;
                                b.clics[k].push_back(maintenant);
                                if b.clics[k].len() > 20 {
                                    b.clics[k].pop_front();
                                }
                            }
                        }
                    } else if r.header.dwType == RIM_TYPEKEYBOARD {
                        let k = r.data.keyboard;
                        let haut = k.Flags as u32 & RI_KEY_BREAK != 0;
                        if haut {
                            b.enfoncees.remove(&k.VKey);
                        } else {
                            b.enfoncees.insert(k.VKey);
                            b.deja.insert(k.VKey);
                            let n = b.enfoncees.len();
                            b.max_simultanees = b.max_simultanees.max(n);
                        }
                    }
                }
            }
            return DefWindowProcW(h, m, wp, lp);
        }
        DefWindowProcW(h, m, wp, lp)
    }

    pub fn lancer(b: Arc<Mutex<Brut>>) {
        let _ = CIBLE.set(b);
        std::thread::spawn(|| unsafe {
            let inst = GetModuleHandleW(null());
            let classe: Vec<u16> = "OutilsBrut".encode_utf16().chain([0]).collect();
            let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(0, classe.as_ptr(), classe.as_ptr(), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), inst, null());
            if h.is_null() {
                return;
            }
            let dev = [
                RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_INPUTSINK, hwndTarget: h }, // souris
                RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_INPUTSINK, hwndTarget: h }, // clavier
            ];
            RegisterRawInputDevices(dev.as_ptr(), 2, std::mem::size_of::<RAWINPUTDEVICE>() as u32);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        });
    }
}

pub struct TestSouris;

impl Outil for TestSouris {
    fn nom(&self) -> &'static str {
        "🖱 Test de souris"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test de souris");
        let b = brut();
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        let mut x = b.lock().unwrap();
        let maintenant = Instant::now();
        while x.mouvements.front().is_some_and(|t| maintenant.duration_since(*t) > Duration::from_secs(1)) {
            x.mouvements.pop_front();
        }
        ui.weak("Bouge la souris vite et en rond pendant quelques secondes : on compte les mouvements reçus par seconde.");
        ui.horizontal(|ui| {
            ui.label("Fréquence actuelle :");
            ui.label(RichText::new(format!("{} Hz", x.mouvements.len())).size(26.0).strong());
            ui.separator();
            ui.label("Maximum :");
            ui.label(RichText::new(format!("{} Hz", x.max_hz)).size(26.0).strong().color(Color32::from_rgb(255, 120, 180)));
            if ui.button("Remettre à zéro").clicked() {
                x.max_hz = 0;
                x.doubles = [0; 3];
                x.total_clics = [0; 3];
            }
        });
        ui.weak("Une souris réglée à 1000 Hz monte vers 900-1000 ; 125 Hz est la valeur des souris de bureau.");
        ui.separator();
        ui.strong("Doubles clics parasites");
        ui.weak("Clique normalement, un clic à la fois : un appui enregistré moins de 60 ms après le précédent indique un bouton usé.");
        egui::Grid::new("clics").num_columns(3).spacing([24.0, 6.0]).show(ui, |ui| {
            for (k, n) in ["Gauche", "Droit", "Milieu"].iter().enumerate() {
                ui.label(*n);
                ui.label(format!("{} clic(s)", x.total_clics[k]));
                if x.doubles[k] > 0 {
                    ui.colored_label(Color32::from_rgb(255, 170, 80), format!("⚠ {} double(s) suspect(s)", x.doubles[k]));
                } else {
                    ui.colored_label(Color32::LIGHT_GREEN, "✔ rien de suspect");
                }
                ui.end_row();
            }
        });
    }
}

/// Disposition AZERTY simplifiée : rangées de (vk, libellé, largeur).
const RANGEES: &[&[(u16, &str, f32)]] = &[
    &[(0x1B, "Échap", 1.2), (0x70, "F1", 1.0), (0x71, "F2", 1.0), (0x72, "F3", 1.0), (0x73, "F4", 1.0), (0x74, "F5", 1.0), (0x75, "F6", 1.0), (0x76, "F7", 1.0),
      (0x77, "F8", 1.0), (0x78, "F9", 1.0), (0x79, "F10", 1.0), (0x7A, "F11", 1.0), (0x7B, "F12", 1.0)],
    &[(0xDE, "²", 1.0), (0x31, "1", 1.0), (0x32, "2", 1.0), (0x33, "3", 1.0), (0x34, "4", 1.0), (0x35, "5", 1.0), (0x36, "6", 1.0), (0x37, "7", 1.0),
      (0x38, "8", 1.0), (0x39, "9", 1.0), (0x30, "0", 1.0), (0xDB, ")", 1.0), (0xBB, "=", 1.0), (0x08, "⌫", 2.0)],
    &[(0x09, "Tab", 1.5), (0x41, "A", 1.0), (0x5A, "Z", 1.0), (0x45, "E", 1.0), (0x52, "R", 1.0), (0x54, "T", 1.0), (0x59, "Y", 1.0), (0x55, "U", 1.0),
      (0x49, "I", 1.0), (0x4F, "O", 1.0), (0x50, "P", 1.0), (0xDD, "^", 1.0), (0xBA, "$", 1.0), (0x0D, "Entrée", 1.5)],
    &[(0x14, "Verr.", 1.8), (0x51, "Q", 1.0), (0x53, "S", 1.0), (0x44, "D", 1.0), (0x46, "F", 1.0), (0x47, "G", 1.0), (0x48, "H", 1.0), (0x4A, "J", 1.0),
      (0x4B, "K", 1.0), (0x4C, "L", 1.0), (0x4D, "M", 1.0), (0xC0, "ù", 1.0), (0xDC, "*", 1.0)],
    &[(0x10, "Maj", 1.3), (0xE2, "<", 1.0), (0x57, "W", 1.0), (0x58, "X", 1.0), (0x43, "C", 1.0), (0x56, "V", 1.0), (0x42, "B", 1.0), (0x4E, "N", 1.0),
      (0xBC, ",", 1.0), (0xBE, ";", 1.0), (0xBF, ":", 1.0), (0xDF, "!", 1.0), (0x26, "↑", 1.0)],
    &[(0x11, "Ctrl", 1.5), (0x5B, "Win", 1.2), (0x12, "Alt", 1.2), (0x20, "Espace", 5.5), (0x25, "←", 1.0), (0x28, "↓", 1.0), (0x27, "→", 1.0)],
];

pub struct TestClavier;

impl Outil for TestClavier {
    fn nom(&self) -> &'static str {
        "⌨ Test de clavier"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test de clavier");
        ui.weak("Appuie sur les touches : elles s'allument quand elles sont enfoncées, restent marquées une fois testées. Maintiens plusieurs touches pour tester les appuis simultanés.");
        let b = brut();
        ui.ctx().request_repaint_after(Duration::from_millis(50));
        let mut x = b.lock().unwrap();
        ui.horizontal(|ui| {
            ui.label(format!("Enfoncées maintenant : {}", x.enfoncees.len()));
            ui.separator();
            ui.label(RichText::new(format!("Maximum simultané : {}", x.max_simultanees)).strong());
            if ui.button("Remettre à zéro").clicked() {
                x.deja.clear();
                x.max_simultanees = 0;
            }
        });
        ui.weak("Un clavier de jeu accepte en général 6 touches ou plus (« anti-ghosting ») ; un clavier de bureau parfois seulement 2 ou 3.");
        ui.add_space(8.0);
        let u = 38.0;
        for rangee in RANGEES {
            ui.horizontal(|ui| {
                for (vk, nom, l) in *rangee {
                    // Maj / Ctrl / Alt : le clavier envoie la version gauche ou droite
                    let variantes: &[u16] = match vk {
                        0x10 => &[0x10, 0xA0, 0xA1],
                        0x11 => &[0x11, 0xA2, 0xA3],
                        0x12 => &[0x12, 0xA4, 0xA5],
                        _ => std::slice::from_ref(vk),
                    };
                    let bas = variantes.iter().any(|v| x.enfoncees.contains(v));
                    let teste = variantes.iter().any(|v| x.deja.contains(v));
                    let fond = if bas { Color32::from_rgb(255, 95, 162) } else if teste { Color32::from_rgb(60, 130, 90) } else { Color32::from_gray(55) };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(u * l, u), egui::Sense::hover());
                    ui.painter().rect_filled(r.shrink(2.0), 5.0, fond);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, *nom, egui::FontId::proportional(13.0), Color32::WHITE);
                }
            });
        }
        let connues: BTreeSet<u16> = RANGEES.iter().flat_map(|r| r.iter().map(|k| k.0)).collect();
        let autres: Vec<String> = x.deja.iter().filter(|v| !connues.contains(v) && ![0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5].contains(*v)).map(|v| format!("{v:#04X}")).collect();
        if !autres.is_empty() {
            ui.weak(format!("Autres touches testées : {}", autres.join(" ")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebonds() {
        let t = Instant::now();
        assert!(est_rebond(Some(t), t + Duration::from_millis(30), Duration::from_millis(60)));
        assert!(!est_rebond(Some(t), t + Duration::from_millis(200), Duration::from_millis(60)));
        assert!(!est_rebond(None, t, Duration::from_millis(60)));
    }
}
