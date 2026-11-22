//! Traduction instantanée : sélectionne un texte n'importe où, Ctrl+Alt+T, la traduction apparaît dans une
//! bulle près de la souris (clic : copier et fermer, Échap : fermer). Google Traduction, langue détectée.
//! Réglages dans %APPDATA%\Outils\traduction_instantanee.json.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub langue: String,
    pub seconde: String, // si le texte est déjà dans la langue voulue
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: true, langue: "fr".into(), seconde: "en".into() }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("traduction_instantanee.json")
}

pub const LANGUES: [(&str, &str); 10] = [
    ("fr", "Français"), ("en", "Anglais"), ("es", "Espagnol"), ("de", "Allemand"), ("it", "Italien"), ("pt", "Portugais"), ("ja", "Japonais"),
    ("ko", "Coréen"), ("zh-CN", "Chinois"), ("ru", "Russe"),
];

/// Compare sans casse ni espaces : la traduction n'a rien changé = texte déjà dans la langue.
pub fn inchange(a: &str, b: &str) -> bool {
    let n = |s: &str| s.chars().filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation()).flat_map(char::to_lowercase).collect::<String>();
    n(a) == n(b)
}

pub(crate) mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    pub fn lire_presse_papiers() -> Option<String> {
        unsafe {
            if OpenClipboard(null_mut()) == 0 {
                return None;
            }
            let h = GetClipboardData(13); // CF_UNICODETEXT
            let mut r = None;
            if !h.is_null() {
                let p = GlobalLock(h) as *const u16;
                if !p.is_null() {
                    let mut n = 0;
                    while *p.add(n) != 0 && n < 200_000 {
                        n += 1;
                    }
                    r = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                    GlobalUnlock(h);
                }
            }
            CloseClipboard();
            r
        }
    }

    fn touche(vk: u16, haut: bool) -> INPUT {
        let mut i: INPUT = unsafe { std::mem::zeroed() };
        i.r#type = INPUT_KEYBOARD;
        i.Anonymous.ki = KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if haut { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 };
        i
    }

    /// Copie la sélection de l'appli au premier plan (Ctrl+C) et la lit.
    pub fn copier_selection() -> Option<String> {
        unsafe {
            // attend que Ctrl / Alt / T du raccourci soient relâchés, sinon Ctrl+C devient Ctrl+Alt+C
            let fin = Instant::now() + Duration::from_millis(1500);
            while Instant::now() < fin && [VK_CONTROL, VK_MENU, b'T' as u16].iter().any(|k| GetAsyncKeyState(*k as i32) < 0) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let avant = GetClipboardSequenceNumber();
            let seq = [touche(VK_CONTROL, false), touche(b'C' as u16, false), touche(b'C' as u16, true), touche(VK_CONTROL, true)];
            SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32);
            let fin = Instant::now() + Duration::from_millis(800);
            while Instant::now() < fin && GetClipboardSequenceNumber() == avant {
                std::thread::sleep(Duration::from_millis(20));
            }
            if GetClipboardSequenceNumber() == avant {
                return None; // rien de sélectionné
            }
            std::thread::sleep(Duration::from_millis(30));
            lire_presse_papiers().filter(|t| !t.trim().is_empty())
        }
    }

    struct Bulle {
        texte: Vec<u16>,
        police: HFONT,
        copier: Option<String>,
        fini: bool,
    }

    thread_local! {
        static BULLE: std::cell::RefCell<Option<Bulle>> = const { std::cell::RefCell::new(None) };
    }

    const MARGE: i32 = 14;

    unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match m {
            WM_PAINT => {
                BULLE.with(|b| {
                    if let Some(b) = b.borrow().as_ref() {
                        let mut ps: PAINTSTRUCT = std::mem::zeroed();
                        let dc = BeginPaint(h, &mut ps);
                        let mut r: RECT = std::mem::zeroed();
                        GetClientRect(h, &mut r);
                        let fond = CreateSolidBrush(0x00_1A_10_1E); // BGR : violet très sombre
                        FillRect(dc, &r, fond);
                        DeleteObject(fond);
                        let bord = CreateSolidBrush(0x00_A2_5F_FF); // rose
                        FrameRect(dc, &r, bord);
                        DeleteObject(bord);
                        SelectObject(dc, b.police);
                        SetBkMode(dc, TRANSPARENT as i32);
                        SetTextColor(dc, 0x00_E8_DC_F5);
                        let mut t = RECT { left: MARGE, top: MARGE, right: r.right - MARGE, bottom: r.bottom - MARGE };
                        DrawTextW(dc, b.texte.as_ptr(), b.texte.len() as i32, &mut t, DT_WORDBREAK | DT_NOPREFIX);
                        EndPaint(h, &ps);
                    }
                });
                0
            }
            WM_LBUTTONUP => {
                BULLE.with(|b| {
                    if let Some(b) = b.borrow_mut().as_mut() {
                        if let Some(t) = b.copier.take() {
                            crate::pressepapiers::win::ecrire_texte(&t, None, None);
                        }
                        b.fini = true;
                    }
                });
                0
            }
            WM_KEYDOWN | WM_RBUTTONUP | WM_TIMER => {
                if m != WM_KEYDOWN || wp == VK_ESCAPE as usize {
                    BULLE.with(|b| b.borrow_mut().as_mut().map(|b| b.fini = true));
                }
                0
            }
            WM_ACTIVATE if (wp & 0xFFFF) as u32 == WA_INACTIVE => {
                BULLE.with(|b| b.borrow_mut().as_mut().map(|b| b.fini = true));
                0
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }

    /// Affiche la bulle près de la souris (bloquant jusqu'à sa fermeture).
    pub fn bulle(texte: &str, copier: Option<String>) {
        unsafe {
            let inst = GetModuleHandleW(null());
            let classe = w("OutilsTraduction");
            let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), hCursor: LoadCursorW(null_mut(), IDC_HAND), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let police = CreateFontW(-17, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, w("Segoe UI").as_ptr());
            let u: Vec<u16> = texte.encode_utf16().collect();
            // mesure du texte pour une largeur de 460 px
            let dc = GetDC(null_mut());
            let vieux = SelectObject(dc, police);
            let mut r = RECT { left: 0, top: 0, right: 460, bottom: 0 };
            DrawTextW(dc, u.as_ptr(), u.len() as i32, &mut r, DT_WORDBREAK | DT_CALCRECT | DT_NOPREFIX);
            SelectObject(dc, vieux);
            ReleaseDC(null_mut(), dc);
            let (l, ht) = (r.right + 2 * MARGE, (r.bottom + 2 * MARGE).min(600));
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            let (el, eh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let x = (p.x + 16).min(el - l - 8).max(0);
            let y = if p.y + 20 + ht > eh { (p.y - ht - 10).max(0) } else { p.y + 20 };
            BULLE.with(|b| *b.borrow_mut() = Some(Bulle { texte: u, police, copier, fini: false }));
            let h = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, classe.as_ptr(), classe.as_ptr(), WS_POPUP | WS_VISIBLE, x, y, l, ht, null_mut(), null_mut(), inst, null());
            if !h.is_null() {
                SetForegroundWindow(h);
                SetTimer(h, 1, 60_000, None);
                let mut msg: MSG = std::mem::zeroed();
                while !BULLE.with(|b| b.borrow().as_ref().is_none_or(|b| b.fini)) && GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                DestroyWindow(h);
            }
            BULLE.with(|b| b.borrow_mut().take());
            DeleteObject(police);
        }
    }

    pub fn raccourci(actif: Arc<AtomicBool>, pris: Arc<AtomicBool>, mut action: impl FnMut() + Send + 'static) {
        std::thread::spawn(move || unsafe {
            let ok = crate::outil::raccourci(null_mut(), 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'T' as u32) != 0;
            pris.store(!ok, Relaxed);
            if !ok {
                return;
            }
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY && actif.load(Relaxed) {
                    action();
                }
            }
        });
    }
}

pub struct TradInstant {
    r: Arc<Mutex<Reglages>>,
    actif: Arc<AtomicBool>,
    pris: Arc<AtomicBool>,
    historique: Arc<Mutex<Vec<(String, String)>>>,
    essai: String,
}

fn traduire(r: &Reglages, texte: &str) -> Result<String, String> {
    let t = crate::traducteur::traduire_google(texte, &r.langue)?;
    if inchange(&t, texte) && r.seconde != r.langue {
        return crate::traducteur::traduire_google(texte, &r.seconde);
    }
    Ok(t)
}

impl TradInstant {
    pub fn new() -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let actif = Arc::new(AtomicBool::new(r.actif));
        let r = Arc::new(Mutex::new(r));
        let (pris, historique): (Arc<AtomicBool>, Arc<Mutex<Vec<(String, String)>>>) = (Arc::default(), Arc::default());
        let (r2, h2) = (r.clone(), historique.clone());
        win::raccourci(actif.clone(), pris.clone(), move || {
            let Some(texte) = win::copier_selection() else {
                win::bulle("Sélectionne d'abord un texte, puis Ctrl+Alt+T.", None);
                return;
            };
            let texte: String = texte.chars().take(4000).collect();
            let reg = r2.lock().unwrap().clone();
            match traduire(&reg, &texte) {
                Ok(t) => {
                    {
                        let mut h = h2.lock().unwrap();
                        h.insert(0, (texte.clone(), t.clone()));
                        h.truncate(30);
                    }
                    win::bulle(&format!("{t}\n\n— clic : copier la traduction · Échap : fermer"), Some(t));
                }
                Err(e) => win::bulle(&format!("Traduction impossible : {e}"), None),
            }
        });
        TradInstant { r, actif, pris, historique, essai: String::new() }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&*self.r.lock().unwrap()).unwrap_or_default());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Traduction instantanée");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Raccourci Ctrl+Alt+T actif");
            if self.pris.load(Relaxed) {
                ui.colored_label(Color32::LIGHT_RED, "(déjà pris par une autre appli)");
            }
        });
        ui.weak("Sélectionne un texte dans n'importe quelle appli (navigateur, jeu fenêtré, Discord…) puis Ctrl+Alt+T.");
        let choix = |ui: &mut egui::Ui, id: &str, v: &mut String| {
            egui::ComboBox::from_id_salt(id).selected_text(LANGUES.iter().find(|(c, _)| c == v).map(|x| x.1).unwrap_or(v.as_str()).to_string()).show_ui(ui, |ui| {
                for (c, n) in LANGUES {
                    ui.selectable_value(v, c.to_string(), n);
                }
            });
        };
        ui.horizontal(|ui| {
            ui.label("Traduire vers");
            choix(ui, "langue", &mut r.langue);
            ui.label("et si c'est déjà dans cette langue, vers");
            choix(ui, "seconde", &mut r.seconde);
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.essai).hint_text("ou tape un texte ici").desired_width(360.0));
            if ui.button("Traduire").clicked() && !self.essai.trim().is_empty() {
                let (t, reg, h) = (self.essai.clone(), r.clone(), self.historique.clone());
                std::thread::spawn(move || {
                    let res = traduire(&reg, &t).unwrap_or_else(|e| format!("Erreur : {e}"));
                    let mut x = h.lock().unwrap();
                    x.insert(0, (t, res));
                    x.truncate(30);
                });
            }
        });
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        egui::ScrollArea::vertical().id_salt("histo_trad").show(ui, |ui| {
            for (src, t) in self.historique.lock().unwrap().iter() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.weak(src.chars().take(300).collect::<String>());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(t);
                        if ui.small_button("📋").clicked() {
                            ui.ctx().copy_text(t.clone());
                        }
                    });
                });
            }
        });
        if r != avant {
            self.actif.store(r.actif, Relaxed);
            *self.r.lock().unwrap() = r;
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparaison() {
        assert!(inchange("Bonjour, le monde !", "bonjour le monde"));
        assert!(!inchange("Hello world", "Bonjour le monde"));
    }
}
