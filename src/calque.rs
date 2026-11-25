//! Calque : petite fenêtre transparente toujours au premier plan, qui affiche une image RGBA
//! (animal de bureau, guide en jeu). Optionnellement traversée par les clics et liée à un raccourci.
use image::RgbaImage;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_MAJ: u32 = WM_APP + 21;

/// Clics reçus par chaque calque (fenêtre → compteur).
static CLICS: Mutex<Vec<(isize, Arc<AtomicU32>)>> = Mutex::new(Vec::new());

unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if m == WM_LBUTTONDOWN {
        if let Some((_, c)) = CLICS.lock().unwrap().iter().find(|(x, _)| *x == h as isize) {
            c.fetch_add(1, Relaxed);
        }
        return 0;
    }
    if m == WM_MOUSEACTIVATE {
        return MA_NOACTIVATE as LRESULT;
    }
    DefWindowProcW(h, m, wp, lp)
}

pub struct Calque {
    h: Arc<AtomicIsize>,
    image: Arc<Mutex<Option<(RgbaImage, i32, i32)>>>,
    clics: Arc<AtomicU32>,
    /// appuis sur chaque raccourci clavier (dans l'ordre donné à `new`)
    appuis: Arc<Vec<AtomicU32>>,
    pub raccourci_pris: Arc<AtomicBool>,
}

impl Calque {
    /// `raccourcis` : (modificateurs, touche) comptés par `appuis(k)`.
    pub fn new(classe: &str, traversant: bool, raccourcis: &[(u32, u32)]) -> Self {
        let c = Calque { h: Arc::default(), image: Arc::default(), clics: Arc::default(), appuis: Arc::new(raccourcis.iter().map(|_| AtomicU32::new(0)).collect()), raccourci_pris: Arc::default() };
        let (h, image, clics, appuis, pris) = (c.h.clone(), c.image.clone(), c.clics.clone(), c.appuis.clone(), c.raccourci_pris.clone());
        let raccourcis = raccourcis.to_vec();
        let classe: Vec<u16> = classe.encode_utf16().chain([0]).collect();
        std::thread::spawn(move || unsafe {
            let inst = GetModuleHandleW(null());
            let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), hCursor: LoadCursorW(null_mut(), IDC_HAND), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let mut ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
            if traversant {
                ex |= WS_EX_TRANSPARENT;
            }
            let w = CreateWindowExW(ex, classe.as_ptr(), classe.as_ptr(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), inst, null());
            if w.is_null() {
                return;
            }
            h.store(w as isize, Relaxed);
            CLICS.lock().unwrap().push((w as isize, clics));
            for (k, (m, t)) in raccourcis.iter().enumerate() {
                if crate::outil::raccourci(w, k as i32 + 1, *m, *t) == 0 {
                    pris.store(true, Relaxed);
                }
            }
            SetTimer(w, 1, 2000, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_HOTKEY => {
                        if let Some(a) = appuis.get((msg.wParam as usize).wrapping_sub(1)) {
                            a.fetch_add(1, Relaxed);
                        }
                    }
                    WM_TIMER if IsWindowVisible(w) != 0 => {
                        // certains jeux repassent devant
                        SetWindowPos(w, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    }
                    _ if msg.hwnd == w && msg.message == WM_MAJ => {
                        let i = image.lock().unwrap().take();
                        match i {
                            Some((img, x, y)) => afficher(w, &img, x, y),
                            None => {
                                ShowWindow(w, SW_HIDE);
                            }
                        }
                    }
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        });
        c
    }

    /// Affiche l'image (coin haut-gauche en x, y, en pixels d'écran).
    pub fn montrer(&self, img: RgbaImage, x: i32, y: i32) {
        *self.image.lock().unwrap() = Some((img, x, y));
        self.poster();
    }

    pub fn cacher(&self) {
        *self.image.lock().unwrap() = None;
        self.poster();
    }

    fn poster(&self) {
        let h = self.h.load(Relaxed);
        if h != 0 {
            unsafe { PostMessageW(h as HWND, WM_MAJ, 0, 0) };
        }
    }

    /// Appuis sur le raccourci `k` depuis le dernier appel.
    pub fn appuis(&self, k: usize) -> u32 {
        self.appuis.get(k).map(|a| a.swap(0, Relaxed)).unwrap_or(0)
    }

    /// Nombre de clics depuis le dernier appel.
    pub fn clics(&self) -> u32 {
        self.clics.swap(0, Relaxed)
    }
}

unsafe fn afficher(h: HWND, img: &RgbaImage, x: i32, y: i32) {
    let (l, ht) = (img.width() as i32, img.height() as i32);
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
        // BGRA prémultiplié
        let d = std::slice::from_raw_parts_mut(bits as *mut u8, (l * ht * 4) as usize);
        for (o, p) in d.chunks_exact_mut(4).zip(img.pixels()) {
            let a = p[3] as u32;
            o[0] = (p[2] as u32 * a / 255) as u8;
            o[1] = (p[1] as u32 * a / 255) as u8;
            o[2] = (p[0] as u32 * a / 255) as u8;
            o[3] = p[3];
        }
    }
    let vieux = SelectObject(mem, bm);
    let pos = POINT { x, y };
    let taille = SIZE { cx: l, cy: ht };
    let source = POINT { x: 0, y: 0 };
    let melange = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    UpdateLayeredWindow(h, ecran, &pos, &taille, mem, &source, 0, &melange, ULW_ALPHA);
    SelectObject(mem, vieux);
    DeleteObject(bm);
    DeleteDC(mem);
    ReleaseDC(null_mut(), ecran);
    if IsWindowVisible(h) == 0 {
        ShowWindow(h, SW_SHOWNOACTIVATE);
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Zone de travail de l'écran principal (sans la barre des tâches) : (gauche, haut, droite, bas).
pub fn zone_travail() -> (i32, i32, i32, i32) {
    unsafe {
        let mut r: windows_sys::Win32::Foundation::RECT = std::mem::zeroed();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut _ as *mut core::ffi::c_void, 0) != 0 {
            (r.left, r.top, r.right, r.bottom)
        } else {
            (0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
        }
    }
}

/// Position de la souris à l'écran.
pub fn souris() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

/// Bouton gauche enfoncé (n'importe où).
pub fn bouton_gauche() -> bool {
    unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) as u16 & 0x8000 != 0 }
}
