//! Capture d'écran (zone, fenêtre, écran entier) avec annotation, pipette, règle à l'écran,
//! enregistrement vidéo et « replay » des dernières secondes (ffmpeg).
//! Raccourcis globaux : Ctrl+Alt+S zone, Ctrl+Alt+W fenêtre, Ctrl+Alt+P écran entier,
//! Ctrl+Alt+C pipette, Ctrl+Alt+M règle, Ctrl+Alt+R sauver le replay.
//! Réglages dans %APPDATA%\Outils\capture.json.
use crate::systeme;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};


#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub raccourcis: bool,
    pub dossier: PathBuf,
    pub auto_enregistrer: bool,
    pub auto_copier: bool,
    pub ouvrir_editeur: bool,
    pub fps: u32,
    pub replay_secondes: u32,
    pub replay_auto_jeu: bool, // replay lancé tout seul pendant un jeu
    pub couleurs: Vec<String>,  // historique de la pipette
}

impl Default for Reglages {
    fn default() -> Self {
        let images = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Pictures").join("Captures");
        Reglages {
            raccourcis: true, dossier: images, auto_enregistrer: true, auto_copier: true, ouvrir_editeur: true, fps: 30, replay_secondes: 30,
            replay_auto_jeu: false, couleurs: vec![],
        }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("capture.json")
}

fn horodatage() -> String {
    crate::minecraft::horodatage() // AAAAMMJJ_HHMMSS
}

// ===================================================================================================
// Annotations (pures : testables)
// ===================================================================================================

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Outil {
    Rectangle,
    Fleche,
    Surligneur,
    Flou,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Op {
    pub outil: Outil,
    pub a: (f32, f32),
    pub b: (f32, f32),
    pub couleur: [u8; 3],
    pub epaisseur: f32,
}

fn melanger(img: &mut RgbaImage, x: i32, y: i32, c: [u8; 3], alpha: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    for k in 0..3 {
        p[k] = (p[k] as f32 * (1.0 - alpha) + c[k] as f32 * alpha).round() as u8;
    }
}

fn disque(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
    let r2 = r * r;
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            let d = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
            if d <= r2 {
                melanger(img, x, y, c, 1.0);
            }
        }
    }
}

fn ligne(img: &mut RgbaImage, a: (f32, f32), b: (f32, f32), ep: f32, c: [u8; 3]) {
    let long = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let pas = (long / 0.7).ceil().max(1.0) as usize;
    for i in 0..=pas {
        let t = i as f32 / pas as f32;
        disque(img, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, ep / 2.0, c);
    }
}

fn bornes(op: &Op, img: &RgbaImage) -> (u32, u32, u32, u32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let x0 = op.a.0.min(op.b.0).clamp(0.0, w) as u32;
    let x1 = op.a.0.max(op.b.0).clamp(0.0, w) as u32;
    let y0 = op.a.1.min(op.b.1).clamp(0.0, h) as u32;
    let y1 = op.a.1.max(op.b.1).clamp(0.0, h) as u32;
    (x0, y0, x1, y1)
}

/// Pointes de la flèche (deux segments à ±28° depuis la pointe).
pub fn pointes(a: (f32, f32), b: (f32, f32), ep: f32) -> [(f32, f32); 2] {
    let ang = (b.1 - a.1).atan2(b.0 - a.0);
    let l = 4.0 * ep + 10.0;
    let d = 28f32.to_radians();
    [(b.0 - l * (ang - d).cos(), b.1 - l * (ang - d).sin()), (b.0 - l * (ang + d).cos(), b.1 - l * (ang + d).sin())]
}

pub fn appliquer(img: &mut RgbaImage, op: &Op) {
    match op.outil {
        Outil::Rectangle => {
            let (x0, y0, x1, y1) = (op.a.0.min(op.b.0), op.a.1.min(op.b.1), op.a.0.max(op.b.0), op.a.1.max(op.b.1));
            for (p, q) in [((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))] {
                ligne(img, p, q, op.epaisseur, op.couleur);
            }
        }
        Outil::Fleche => {
            ligne(img, op.a, op.b, op.epaisseur, op.couleur);
            for p in pointes(op.a, op.b, op.epaisseur) {
                ligne(img, op.b, p, op.epaisseur, op.couleur);
            }
        }
        Outil::Surligneur => {
            let (x0, y0, x1, y1) = bornes(op, img);
            for y in y0..y1 {
                for x in x0..x1 {
                    melanger(img, x as i32, y as i32, op.couleur, 0.38);
                }
            }
        }
        Outil::Flou => {
            // pixellisation : illisible et impossible à « déflouter »
            let (x0, y0, x1, y1) = bornes(op, img);
            let bloc = 12u32;
            let mut by = y0;
            while by < y1 {
                let mut bx = x0;
                while bx < x1 {
                    let (ex, ey) = ((bx + bloc).min(x1), (by + bloc).min(y1));
                    let mut s = [0u64; 3];
                    let n = ((ex - bx) * (ey - by)) as u64;
                    for y in by..ey {
                        for x in bx..ex {
                            let p = img.get_pixel(x, y);
                            for k in 0..3 {
                                s[k] += p[k] as u64;
                            }
                        }
                    }
                    for y in by..ey {
                        for x in bx..ex {
                            let p = img.get_pixel_mut(x, y);
                            for k in 0..3 {
                                p[k] = (s[k] / n.max(1)) as u8;
                            }
                        }
                    }
                    bx = ex;
                }
                by = (by + bloc).min(y1);
            }
        }
    }
}

pub fn rendre(base: &RgbaImage, ops: &[Op]) -> RgbaImage {
    let mut img = base.clone();
    for op in ops {
        appliquer(&mut img, op);
    }
    img
}

/// Rectangle (x, y, l, h) normalisé à partir de deux coins, rogné à l'image.
pub fn rect_normalise(a: (i32, i32), b: (i32, i32), w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
    let x0 = a.0.min(b.0).clamp(0, w);
    let y0 = a.1.min(b.1).clamp(0, h);
    let x1 = a.0.max(b.0).clamp(0, w);
    let y1 = a.1.max(b.1).clamp(0, h);
    (x1 - x0 >= 3 && y1 - y0 >= 3).then_some((x0, y0, x1 - x0, y1 - y0))
}

pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

// ===================================================================================================
// Windows : écran, fenêtre, presse-papiers, surimpression (zone / règle / pipette)
// ===================================================================================================

mod win {
    use super::*;
    use std::cell::RefCell;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    /// Bureau virtuel entier (tous les écrans) : origine et pixels BGRA de haut en bas.
    pub struct Ecran {
        pub x: i32,
        pub y: i32,
        pub w: i32,
        pub h: i32,
        pub bgra: Vec<u8>,
    }

    impl Ecran {
        pub fn rgba(&self, x: i32, y: i32, l: i32, h: i32) -> RgbaImage {
            let mut img = RgbaImage::new(l.max(1) as u32, h.max(1) as u32);
            for j in 0..h {
                for i in 0..l {
                    let (sx, sy) = (x + i, y + j);
                    if sx < 0 || sy < 0 || sx >= self.w || sy >= self.h {
                        continue;
                    }
                    let o = ((sy * self.w + sx) * 4) as usize;
                    img.put_pixel(i as u32, j as u32, image::Rgba([self.bgra[o + 2], self.bgra[o + 1], self.bgra[o], 255]));
                }
            }
            img
        }

        pub fn couleur(&self, x: i32, y: i32) -> [u8; 3] {
            if x < 0 || y < 0 || x >= self.w || y >= self.h {
                return [0; 3];
            }
            let o = ((y * self.w + x) * 4) as usize;
            [self.bgra[o + 2], self.bgra[o + 1], self.bgra[o]]
        }
    }

    fn info(l: i32, h: i32) -> BITMAPINFO {
        let mut bi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = l;
        bi.bmiHeader.biHeight = -h; // de haut en bas
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        bi
    }

    pub fn capturer() -> Option<Ecran> {
        unsafe {
            let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (l, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            if l <= 0 || h <= 0 {
                return None;
            }
            let ecran = GetDC(null_mut());
            let mem = CreateCompatibleDC(ecran);
            let bm = CreateCompatibleBitmap(ecran, l, h);
            let vieux = SelectObject(mem, bm);
            BitBlt(mem, 0, 0, l, h, ecran, x, y, SRCCOPY | CAPTUREBLT);
            SelectObject(mem, vieux);
            let mut bi = info(l, h);
            let mut bgra = vec![0u8; (l * h * 4) as usize];
            let n = GetDIBits(mem, bm, 0, h as u32, bgra.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
            DeleteObject(bm);
            DeleteDC(mem);
            ReleaseDC(null_mut(), ecran);
            (n > 0).then_some(Ecran { x, y, w: l, h, bgra })
        }
    }

    /// Rectangle de la fenêtre au premier plan, en coordonnées de l'image `e`.
    pub fn fenetre_active(e: &Ecran) -> Option<(i32, i32, i32, i32)> {
        unsafe {
            let h = GetForegroundWindow();
            if h.is_null() {
                return None;
            }
            let mut r: RECT = std::mem::zeroed();
            // cadre visible (sans l'ombre invisible de Windows 10/11)
            if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS as u32, (&mut r as *mut RECT).cast(), std::mem::size_of::<RECT>() as u32) != 0 {
                GetWindowRect(h, &mut r);
            }
            rect_normalise((r.left - e.x, r.top - e.y), (r.right - e.x, r.bottom - e.y), e.w, e.h)
        }
    }

    pub fn copier_image(img: &RgbaImage) -> bool {
        crate::pressepapiers::win::ecrire_dib(&crate::pressepapiers::rgba_dib(img.width(), img.height(), img.as_raw()))
    }

    pub fn copier_texte(t: &str) -> bool {
        crate::pressepapiers::win::ecrire_texte(t, None, None)
    }

    #[derive(Clone, Copy, PartialEq)]
    pub enum Mode {
        Zone,
        Regle,
        Pipette,
    }

    pub enum Retour {
        Zone(i32, i32, i32, i32),
        Couleur([u8; 3]),
        Rien,
    }

    struct Surimpression {
        mode: Mode,
        ecran: Ecran,
        dc_clair: HDC,
        dc_sombre: HDC,
        objets: Vec<HGDIOBJ>,
        debut: Option<(i32, i32)>,
        fige: Option<((i32, i32), (i32, i32))>, // dernière mesure de la règle
        souris: (i32, i32),
        retour: Option<Retour>,
    }

    thread_local! {
        static ETAT: RefCell<Option<Surimpression>> = const { RefCell::new(None) };
    }

    fn rgb(c: [u8; 3]) -> COLORREF {
        c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
    }

    unsafe fn texte(dc: HDC, x: i32, y: i32, t: &str) {
        let u: Vec<u16> = t.encode_utf16().collect();
        let mut s: SIZE = std::mem::zeroed();
        GetTextExtentPoint32W(dc, u.as_ptr(), u.len() as i32, &mut s);
        let fond = RECT { left: x - 6, top: y - 4, right: x + s.cx + 6, bottom: y + s.cy + 4 };
        let b = CreateSolidBrush(rgb([20, 20, 28]));
        FillRect(dc, &fond, b);
        DeleteObject(b);
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, rgb([255, 255, 255]));
        TextOutW(dc, x, y, u.as_ptr(), u.len() as i32);
    }

    unsafe fn cadre(dc: HDC, a: (i32, i32), b: (i32, i32), couleur: [u8; 3]) {
        let stylo = CreatePen(PS_SOLID, 2, rgb(couleur));
        let v1 = SelectObject(dc, stylo);
        let v2 = SelectObject(dc, GetStockObject(NULL_BRUSH));
        Rectangle(dc, a.0.min(b.0), a.1.min(b.1), a.0.max(b.0) + 1, a.1.max(b.1) + 1);
        SelectObject(dc, v1);
        SelectObject(dc, v2);
        DeleteObject(stylo);
    }

    unsafe fn peindre(h: HWND, s: &Surimpression) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(h, &mut ps);
        let (l, ht) = (s.ecran.w, s.ecran.h);
        let tampon = CreateCompatibleDC(dc);
        let bm = CreateCompatibleBitmap(dc, l, ht);
        let vieux = SelectObject(tampon, bm);
        SelectObject(tampon, GetStockObject(DEFAULT_GUI_FONT));
        let rose = [255, 80, 170];
        match s.mode {
            Mode::Pipette => {
                BitBlt(tampon, 0, 0, l, ht, s.dc_clair, 0, 0, SRCCOPY);
                let (mx, my) = s.souris;
                let c = s.ecran.couleur(mx, my);
                // loupe 15×15 px agrandie 8 fois, à côté du curseur
                let (lx, ly) = (if mx + 160 > l { mx - 150 } else { mx + 24 }, if my + 170 > ht { my - 160 } else { my + 24 });
                SetStretchBltMode(tampon, COLORONCOLOR);
                StretchBlt(tampon, lx, ly, 120, 120, s.dc_clair, mx - 7, my - 7, 15, 15, SRCCOPY);
                cadre(tampon, (lx, ly), (lx + 120, ly + 120), [255, 255, 255]);
                cadre(tampon, (lx + 56, ly + 56), (lx + 64, ly + 64), [255, 0, 0]);
                let b = CreateSolidBrush(rgb(c));
                FillRect(tampon, &RECT { left: lx, top: ly + 124, right: lx + 24, bottom: ly + 144 }, b);
                DeleteObject(b);
                texte(tampon, lx + 32, ly + 126, &format!("{}  rgb({}, {}, {})", hex(c), c[0], c[1], c[2]));
            }
            _ => {
                BitBlt(tampon, 0, 0, l, ht, s.dc_sombre, 0, 0, SRCCOPY);
                let mesure = match (s.debut, s.fige) {
                    (Some(d), _) => Some((d, s.souris)),
                    (None, Some(f)) => Some(f),
                    _ => None,
                };
                if let Some((a, b)) = mesure {
                    let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
                    BitBlt(tampon, x0, y0, x1 - x0 + 1, y1 - y0 + 1, s.dc_clair, x0, y0, SRCCOPY);
                    cadre(tampon, (x0, y0), (x1, y1), rose);
                    let (dx, dy) = (x1 - x0, y1 - y0);
                    let t = if s.mode == Mode::Regle {
                        let stylo = CreatePen(PS_DOT, 1, rgb([255, 255, 255]));
                        let v = SelectObject(tampon, stylo);
                        MoveToEx(tampon, a.0, a.1, null_mut());
                        LineTo(tampon, b.0, b.1);
                        SelectObject(tampon, v);
                        DeleteObject(stylo);
                        format!("{dx} × {dy} px  ·  diagonale {:.0} px", ((dx * dx + dy * dy) as f64).sqrt())
                    } else {
                        format!("{dx} × {dy}")
                    };
                    texte(tampon, x0, (y0 - 26).max(4), &t);
                } else {
                    let aide = if s.mode == Mode::Regle { "Règle : glisse pour mesurer · Échap pour fermer" } else { "Glisse pour choisir la zone · Échap pour annuler" };
                    texte(tampon, s.souris.0 + 18, s.souris.1 + 18, aide);
                }
            }
        }
        BitBlt(dc, 0, 0, l, ht, tampon, 0, 0, SRCCOPY);
        SelectObject(tampon, vieux);
        DeleteObject(bm);
        DeleteDC(tampon);
        EndPaint(h, &ps);
    }

    fn pos(lp: LPARAM) -> (i32, i32) {
        ((lp & 0xFFFF) as i16 as i32, ((lp >> 16) & 0xFFFF) as i16 as i32)
    }

    unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let gere = ETAT.with(|e| {
            let mut e = e.borrow_mut();
            let Some(s) = e.as_mut() else { return false };
            match m {
                WM_PAINT => peindre(h, s),
                WM_ERASEBKGND => {}
                WM_SETCURSOR => {
                    SetCursor(LoadCursorW(null_mut(), IDC_CROSS));
                }
                WM_MOUSEMOVE => {
                    s.souris = pos(lp);
                    InvalidateRect(h, null(), 0);
                }
                WM_LBUTTONDOWN => {
                    let p = pos(lp);
                    if s.mode == Mode::Pipette {
                        s.retour = Some(Retour::Couleur(s.ecran.couleur(p.0, p.1)));
                    } else {
                        s.debut = Some(p);
                        s.fige = None;
                        SetCapture(h);
                    }
                }
                WM_LBUTTONUP => {
                    ReleaseCapture();
                    if let Some(d) = s.debut.take() {
                        let p = pos(lp);
                        match s.mode {
                            Mode::Zone => {
                                if let Some((x, y, l, ht)) = rect_normalise(d, p, s.ecran.w, s.ecran.h) {
                                    s.retour = Some(Retour::Zone(x, y, l, ht));
                                }
                            }
                            _ => s.fige = Some((d, p)),
                        }
                    }
                    InvalidateRect(h, null(), 0);
                }
                WM_RBUTTONUP => s.retour = Some(Retour::Rien),
                WM_KEYDOWN if wp == VK_ESCAPE as usize => s.retour = Some(Retour::Rien),
                _ => return false,
            }
            true
        });
        if gere {
            if m == WM_ERASEBKGND || m == WM_SETCURSOR {
                return 1;
            }
            0
        } else {
            DefWindowProcW(h, m, wp, lp)
        }
    }

    unsafe fn dc_de(dc_ecran: HDC, e: &Ecran, facteur: f32, objets: &mut Vec<HGDIOBJ>) -> HDC {
        let bi = info(e.w, e.h);
        let mut bits: *mut core::ffi::c_void = null_mut();
        let bm = CreateDIBSection(dc_ecran, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if !bits.is_null() {
            let dest = std::slice::from_raw_parts_mut(bits as *mut u8, e.bgra.len());
            if facteur >= 1.0 {
                dest.copy_from_slice(&e.bgra);
            } else {
                for (d, s) in dest.iter_mut().zip(&e.bgra) {
                    *d = (*s as f32 * facteur) as u8;
                }
            }
        }
        let dc = CreateCompatibleDC(dc_ecran);
        SelectObject(dc, bm);
        objets.push(bm);
        dc
    }

    /// Affiche la surimpression plein écran (bloquant, dans le fil appelant) et rend le choix.
    pub fn surimpression(mode: Mode, ecran: Ecran) -> (Retour, Ecran) {
        unsafe {
            let inst = GetModuleHandleW(null());
            let classe = w("OutilsCapture");
            let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc); // déjà enregistrée : sans effet
            let dc_ecran = GetDC(null_mut());
            let mut objets = vec![];
            let dc_clair = dc_de(dc_ecran, &ecran, 1.0, &mut objets);
            let dc_sombre = dc_de(dc_ecran, &ecran, 0.55, &mut objets);
            ReleaseDC(null_mut(), dc_ecran);
            let (x, y, l, ht) = (ecran.x, ecran.y, ecran.w, ecran.h);
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            ETAT.with(|e| {
                *e.borrow_mut() = Some(Surimpression {
                    mode, ecran, dc_clair, dc_sombre, objets, debut: None, fige: None, souris: (p.x - x, p.y - y), retour: None,
                })
            });
            let h = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, classe.as_ptr(), classe.as_ptr(), WS_POPUP | WS_VISIBLE, x, y, l, ht, null_mut(), null_mut(), inst, null());
            if !h.is_null() {
                SetForegroundWindow(h);
                SetFocus(h);
                let mut msg: MSG = std::mem::zeroed();
                loop {
                    let fini = ETAT.with(|e| e.borrow().as_ref().is_none_or(|s| s.retour.is_some()));
                    if fini || GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
                        break;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                DestroyWindow(h);
            }
            let s = ETAT.with(|e| e.borrow_mut().take()).unwrap();
            DeleteDC(s.dc_clair);
            DeleteDC(s.dc_sombre);
            for o in &s.objets {
                DeleteObject(*o);
            }
            (s.retour.unwrap_or(Retour::Rien), s.ecran)
        }
    }

    pub const RACCOURCIS: [(i32, u16, &str); 6] = [
        (1, b'S' as u16, "Zone"),
        (2, b'W' as u16, "Fenêtre active"),
        (3, b'P' as u16, "Écran entier"),
        (4, b'C' as u16, "Pipette"),
        (5, b'M' as u16, "Règle"),
        (6, b'R' as u16, "Sauver le replay"),
    ];
    pub const WM_RECHARGER: u32 = WM_APP + 7;

    /// Fil des raccourcis : Ctrl+Alt+lettre. `action(id)` est appelé dans ce fil.
    pub fn fil_raccourcis(actif: Arc<AtomicBool>, id_fil: Arc<AtomicU32>, pris: Arc<Mutex<Vec<&'static str>>>, mut action: impl FnMut(i32) + Send + 'static) {
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
        std::thread::spawn(move || unsafe {
            id_fil.store(GetCurrentThreadId(), Relaxed);
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE); // crée la file de messages du fil
            let regler = |pris: &Mutex<Vec<&'static str>>| {
                let mut p = pris.lock().unwrap();
                p.clear();
                for (id, vk, nom) in RACCOURCIS {
                    UnregisterHotKey(null_mut(), id);
                    if actif.load(Relaxed) && RegisterHotKey(null_mut(), id, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, vk as u32) == 0 {
                        p.push(nom);
                    }
                }
            };
            regler(&pris);
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_HOTKEY => action(msg.wParam as i32),
                    WM_RECHARGER => regler(&pris),
                    _ => {}
                }
            }
        });
    }

    pub fn recharger(id_fil: u32) {
        if id_fil != 0 {
            unsafe { PostThreadMessageW(id_fil, WM_RECHARGER, 0, 0) };
        }
    }

    /// Taille de l'écran principal (pour ffmpeg).
    pub fn ecran_principal() -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }
}

// ===================================================================================================
// Vidéo : enregistrement et replay (ffmpeg gdigrab)
// ===================================================================================================

fn dossier_replay() -> PathBuf {
    std::env::temp_dir().join("OutilsReplay")
}

const SEGMENT: u32 = 5;

/// Arguments ffmpeg communs : capture de l'écran principal.
fn args_capture(fps: u32) -> Vec<String> {
    let (l, h) = win::ecran_principal();
    [
        "-y", "-v", "error", "-f", "gdigrab", "-framerate", &fps.clamp(10, 60).to_string(), "-offset_x", "0", "-offset_y", "0",
        "-video_size", &format!("{l}x{h}"), "-draw_mouse", "1", "-i", "desktop",
        "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-c:v", "libx264", "-preset", "ultrafast", "-crf", "23", "-pix_fmt", "yuv420p",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

pub fn segments_utiles(mut segs: Vec<(PathBuf, std::time::SystemTime)>, secondes: u32) -> Vec<PathBuf> {
    segs.sort_by_key(|s| s.1);
    let n = (secondes.div_ceil(SEGMENT) + 1) as usize;
    let debut = segs.len().saturating_sub(n);
    segs[debut..].iter().map(|s| s.0.clone()).collect()
}

enum Video {
    Rien,
    Enregistre(Child, PathBuf, Instant),
    Replay(Child),
}

fn lancer_ffmpeg(args: &[String]) -> Result<Child, String> {
    crate::dependances::commande_ou_telecharger("ffmpeg")?.args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .map_err(|e| e.to_string())
}

/// Arrêt propre (« q ») pour que la vidéo soit lisible, sinon on tue.
fn arreter(mut c: Child) {
    if let Some(mut e) = c.stdin.take() {
        let _ = e.write_all(b"q\n");
    }
    for _ in 0..50 {
        if matches!(c.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = c.kill();
    let _ = c.wait();
}

// ===================================================================================================
// Outil
// ===================================================================================================

#[derive(Default)]
struct Partage {
    nouvelle: Option<RgbaImage>, // capture à reprendre dans l'éditeur
    ouvrir: bool,
    message: String,
}

pub struct Capture {
    r: Arc<Mutex<Reglages>>,
    p: Arc<Mutex<Partage>>,
    actif: Arc<AtomicBool>,
    id_fil: Arc<AtomicU32>,
    pris: Arc<Mutex<Vec<&'static str>>>,
    video: Arc<Mutex<Video>>,
    // éditeur
    base: Option<RgbaImage>,
    ops: Vec<Op>,
    texture: Option<egui::TextureHandle>,
    a_rendre: bool,
    outil: Outil,
    couleur: [u8; 3],
    epaisseur: f32,
    trace: Option<(f32, f32)>,
    replay_par_jeu: bool, // le replay en cours a été lancé tout seul pour un jeu
}

impl Capture {
    pub fn new() -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let actif = Arc::new(AtomicBool::new(r.raccourcis));
        let r = Arc::new(Mutex::new(r));
        let p: Arc<Mutex<Partage>> = Arc::default();
        let video = Arc::new(Mutex::new(Video::Rien));
        let (id_fil, pris) = (Arc::new(AtomicU32::new(0)), Arc::default());
        let (r2, p2, v2) = (r.clone(), p.clone(), video.clone());
        win::fil_raccourcis(actif.clone(), id_fil.clone(), Arc::clone(&pris), move |id| match id {
            1..=3 => {
                std::thread::sleep(Duration::from_millis(120)); // laisse les touches se relâcher
                let Some(e) = win::capturer() else { return };
                let img = match id {
                    1 => match win::surimpression(win::Mode::Zone, e) {
                        (win::Retour::Zone(x, y, l, h), e) => Some(e.rgba(x, y, l, h)),
                        _ => None,
                    },
                    2 => win::fenetre_active(&e).map(|(x, y, l, h)| e.rgba(x, y, l, h)),
                    _ => Some(e.rgba(0, 0, e.w, e.h)),
                };
                if let Some(img) = img {
                    livrer(&r2, &p2, img);
                }
            }
            4 => {
                let Some(e) = win::capturer() else { return };
                if let (win::Retour::Couleur(c), _) = win::surimpression(win::Mode::Pipette, e) {
                    let h = hex(c);
                    win::copier_texte(&h);
                    let mut r = r2.lock().unwrap();
                    r.couleurs.retain(|x| x != &h);
                    r.couleurs.insert(0, h.clone());
                    r.couleurs.truncate(24);
                    enregistrer(&r);
                    p2.lock().unwrap().message = format!("Couleur {h} copiée.");
                    systeme::notifier("Pipette", &format!("{h} copiée dans le presse-papiers."));
                }
            }
            5 => {
                if let Some(e) = win::capturer() {
                    win::surimpression(win::Mode::Regle, e);
                }
            }
            6 => {
                let reg = r2.lock().unwrap().clone();
                let m = sauver_replay(&v2, &reg);
                systeme::notifier("Replay", &m);
                p2.lock().unwrap().message = m;
            }
            _ => {}
        });
        Capture {
            r, p, actif, id_fil, pris, video, base: None, ops: vec![], texture: None, a_rendre: false, outil: Outil::Fleche, couleur: [255, 60, 120],
            epaisseur: 4.0, trace: None, replay_par_jeu: false,
        }
    }

    /// À chaque image : reprend une capture faite au raccourci, gère le replay automatique pendant les jeux.
    /// Renvoie true s'il faut montrer l'onglet.
    pub fn tic(&mut self, jeu_en_cours: bool) -> bool {
        let (nouvelle, ouvrir) = {
            let mut p = self.p.lock().unwrap();
            (p.nouvelle.take(), std::mem::take(&mut p.ouvrir))
        };
        if let Some(img) = nouvelle {
            self.base = Some(img);
            self.ops.clear();
            self.a_rendre = true;
        }
        let auto = self.r.lock().unwrap().replay_auto_jeu;
        let replay = matches!(*self.video.lock().unwrap(), Video::Replay(..));
        if auto && jeu_en_cours && !replay && matches!(*self.video.lock().unwrap(), Video::Rien) {
            if self.demarrer_replay().is_ok() {
                self.replay_par_jeu = true;
            }
        } else if replay && self.replay_par_jeu && (!jeu_en_cours || !auto) {
            self.arreter_video();
        }
        ouvrir
    }

    fn demarrer_replay(&mut self) -> Result<(), String> {
        let fps = self.r.lock().unwrap().fps;
        let secondes = self.r.lock().unwrap().replay_secondes;
        let d = dossier_replay();
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
        let garder = secondes.div_ceil(SEGMENT) + 3;
        let mut a = args_capture(fps);
        a.extend(
            ["-g", &(fps * 2).to_string(), "-f", "segment", "-segment_time", &SEGMENT.to_string(), "-segment_wrap", &garder.to_string(), "-reset_timestamps", "1"]
                .iter().map(|s| s.to_string()),
        );
        a.push(d.join("seg%03d.mkv").to_string_lossy().into_owned());
        let c = lancer_ffmpeg(&a)?;
        *self.video.lock().unwrap() = Video::Replay(c);
        Ok(())
    }

    fn demarrer_video(&mut self) -> Result<(), String> {
        let r = self.r.lock().unwrap().clone();
        std::fs::create_dir_all(&r.dossier).map_err(|e| e.to_string())?;
        let chemin = r.dossier.join(format!("Video_{}.mp4", horodatage()));
        let mut a = args_capture(r.fps);
        a.extend(["-movflags", "+faststart"].iter().map(|s| s.to_string()));
        a.push(chemin.to_string_lossy().into_owned());
        let c = lancer_ffmpeg(&a)?;
        *self.video.lock().unwrap() = Video::Enregistre(c, chemin, Instant::now());
        Ok(())
    }

    fn arreter_video(&mut self) {
        let v = std::mem::replace(&mut *self.video.lock().unwrap(), Video::Rien);
        self.replay_par_jeu = false;
        match v {
            Video::Enregistre(c, chemin, _) => {
                let p = self.p.clone();
                std::thread::spawn(move || {
                    arreter(c);
                    p.lock().unwrap().message = format!("Vidéo enregistrée : {}", chemin.display());
                });
            }
            Video::Replay(c) => {
                std::thread::spawn(move || {
                    arreter(c);
                    let _ = std::fs::remove_dir_all(dossier_replay());
                });
            }
            Video::Rien => {}
        }
    }

    pub fn resume(&self) -> String {
        match &*self.video.lock().unwrap() {
            Video::Enregistre(_, _, t) => format!("⏺ Enregistrement en cours ({} s)", t.elapsed().as_secs()),
            Video::Replay(..) => "Replay actif (Ctrl+Alt+R pour sauver)".into(),
            Video::Rien => if self.actif.load(Relaxed) { "Raccourcis actifs (Ctrl+Alt+S…)".into() } else { "Raccourcis désactivés".into() },
        }
    }

    fn enregistrer_image(&self, img: &RgbaImage, chemin: Option<PathBuf>) {
        let r = self.r.lock().unwrap().clone();
        let chemin = chemin.unwrap_or_else(|| r.dossier.join(format!("Capture_{}.png", horodatage())));
        let _ = std::fs::create_dir_all(chemin.parent().unwrap_or(Path::new(".")));
        self.p.lock().unwrap().message = match img.save(&chemin) {
            Ok(_) => format!("Enregistrée : {}", chemin.display()),
            Err(e) => format!("Échec : {e}"),
        };
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Capture d'écran");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        // ---- barre d'actions ----
        ui.horizontal_wrapped(|ui| {
            let fond = |ui: &egui::Ui| {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            };
            if ui.button("✂ Zone").on_hover_text("Ctrl+Alt+S").clicked() {
                fond(ui);
                self.declencher(1);
            }
            if ui.button("🖥 Écran entier").on_hover_text("Ctrl+Alt+P").clicked() {
                fond(ui);
                self.declencher(3);
            }
            if ui.button("💧 Pipette").on_hover_text("Ctrl+Alt+C").clicked() {
                fond(ui);
                self.declencher(4);
            }
            if ui.button("📏 Règle").on_hover_text("Ctrl+Alt+M").clicked() {
                fond(ui);
                self.declencher(5);
            }
            ui.separator();
            let etat = match &*self.video.lock().unwrap() {
                Video::Rien => 0,
                Video::Enregistre(_, _, t) => 1 + t.elapsed().as_secs() as i64,
                Video::Replay(..) => -1,
            };
            match etat {
                0 => {
                    if ui.button("⏺ Enregistrer l'écran").clicked() {
                        if let Err(e) = self.demarrer_video() {
                            self.p.lock().unwrap().message = e;
                        }
                    }
                    if ui.button("⟲ Activer le replay").on_hover_text(format!("Garde les {} dernières secondes ; Ctrl+Alt+R pour les sauver", r.replay_secondes)).clicked() {
                        if let Err(e) = self.demarrer_replay() {
                            self.p.lock().unwrap().message = e;
                        }
                    }
                }
                -1 => {
                    if ui.button("💾 Sauver le replay").clicked() {
                        self.declencher(6);
                    }
                    if ui.button("⏹ Couper le replay").clicked() {
                        self.arreter_video();
                    }
                    ui.colored_label(Color32::from_rgb(255, 120, 170), "replay actif");
                }
                s => {
                    if ui.button("⏹ Arrêter").clicked() {
                        self.arreter_video();
                    }
                    ui.colored_label(Color32::RED, format!("⏺ {:02}:{:02}", (s - 1) / 60, (s - 1) % 60));
                }
            }
            if etat != 0 {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
        let message = self.p.lock().unwrap().message.clone();
        if !message.is_empty() {
            ui.label(message);
        }
        egui::CollapsingHeader::new("Réglages et raccourcis").id_salt("capt_reg").show(ui, |ui| {
            ui.checkbox(&mut r.raccourcis, "Raccourcis globaux Ctrl+Alt + S (zone), W (fenêtre), P (écran), C (pipette), M (règle), R (replay)");
            let pris = self.pris.lock().unwrap().clone();
            if !pris.is_empty() {
                ui.colored_label(Color32::LIGHT_RED, format!("Déjà pris par une autre appli : {}", pris.join(", ")));
            }
            ui.horizontal(|ui| {
                ui.label("Dossier");
                ui.monospace(r.dossier.display().to_string());
                if ui.button("Changer…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                        r.dossier = d;
                    }
                }
                if ui.button("📂").clicked() {
                    let _ = std::fs::create_dir_all(&r.dossier);
                    let _ = Command::new("explorer").arg(&r.dossier).spawn();
                }
            });
            ui.checkbox(&mut r.auto_enregistrer, "Enregistrer chaque capture automatiquement (PNG)");
            ui.checkbox(&mut r.auto_copier, "Copier chaque capture dans le presse-papiers");
            ui.checkbox(&mut r.ouvrir_editeur, "Ouvrir l'éditeur après une capture");
            ui.horizontal(|ui| {
                ui.label("Vidéo :");
                ui.add(egui::DragValue::new(&mut r.fps).range(10..=60).suffix(" images/s"));
                ui.label("replay de");
                ui.add(egui::DragValue::new(&mut r.replay_secondes).range(10..=300).suffix(" s"));
            });
            ui.checkbox(&mut r.replay_auto_jeu, "Activer le replay tout seul pendant un jeu (détecté via le Lanceur)");
            ui.weak("Vidéo de l'écran principal, sans le son, via ffmpeg (téléchargé tout seul au premier usage). Le replay consomme un peu de CPU en continu.");
        });
        if !r.couleurs.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak("Couleurs :");
                for h in r.couleurs.clone() {
                    let c = u32::from_str_radix(h.trim_start_matches('#'), 16).unwrap_or(0);
                    let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                    if ui.add(egui::Button::new(egui::RichText::new("■").color(col).size(18.0))).on_hover_text(format!("{h} : clic pour copier")).clicked() {
                        win::copier_texte(&h);
                        self.p.lock().unwrap().message = format!("{h} copiée.");
                    }
                }
                if ui.small_button("Effacer").clicked() {
                    r.couleurs.clear();
                }
            });
        }
        if r != avant {
            if r.raccourcis != avant.raccourcis {
                self.actif.store(r.raccourcis, Relaxed);
                win::recharger(self.id_fil.load(Relaxed));
            }
            enregistrer(&r);
            *self.r.lock().unwrap() = r;
        }
        ui.separator();
        self.editeur(ui);
    }

    /// Lance une action de capture comme le ferait le raccourci (dans un fil, la fenêtre se réduit d'abord).
    pub fn declencher(&self, id: i32) {
        let (r, p, v) = (self.r.clone(), self.p.clone(), self.video.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(350)); // laisse la fenêtre se réduire
            match id {
                1 | 3 => {
                    let Some(e) = win::capturer() else { return };
                    let img = if id == 1 {
                        match win::surimpression(win::Mode::Zone, e) {
                            (win::Retour::Zone(x, y, l, h), e) => Some(e.rgba(x, y, l, h)),
                            _ => None,
                        }
                    } else {
                        Some(e.rgba(0, 0, e.w, e.h))
                    };
                    match img {
                        Some(img) => livrer(&r, &p, img),
                        None => systeme::montrer(),
                    }
                }
                4 => {
                    if let Some(e) = win::capturer() {
                        if let (win::Retour::Couleur(c), _) = win::surimpression(win::Mode::Pipette, e) {
                            let h = hex(c);
                            win::copier_texte(&h);
                            let mut reg = r.lock().unwrap();
                            reg.couleurs.retain(|x| x != &h);
                            reg.couleurs.insert(0, h.clone());
                            reg.couleurs.truncate(24);
                            enregistrer(&reg);
                            p.lock().unwrap().message = format!("Couleur {h} copiée.");
                        }
                    }
                    systeme::montrer();
                }
                5 => {
                    if let Some(e) = win::capturer() {
                        win::surimpression(win::Mode::Regle, e);
                    }
                    systeme::montrer();
                }
                6 => {
                    let reg = r.lock().unwrap().clone();
                    p.lock().unwrap().message = sauver_replay(&v, &reg);
                }
                _ => {}
            }
        });
    }

    fn editeur(&mut self, ui: &mut egui::Ui) {
        let Some(base) = &self.base else {
            ui.weak("Fais une capture (bouton ou raccourci) : elle s'ouvre ici pour l'annoter (flèches, cadres, surligneur, flou).");
            return;
        };
        if self.a_rendre || self.texture.is_none() {
            let img = rendre(base, &self.ops);
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.texture = Some(ui.ctx().load_texture("capture", ci, egui::TextureOptions::LINEAR));
            self.a_rendre = false;
        }
        let (bw, bh) = (base.width() as f32, base.height() as f32);
        ui.horizontal_wrapped(|ui| {
            for (o, n) in [(Outil::Fleche, "➚ Flèche"), (Outil::Rectangle, "▭ Cadre"), (Outil::Surligneur, "🖍 Surligneur"), (Outil::Flou, "▦ Flou")] {
                ui.selectable_value(&mut self.outil, o, n);
            }
            ui.separator();
            for c in [[255, 60, 120], [255, 60, 60], [255, 200, 40], [60, 200, 120], [70, 150, 255], [255, 255, 255], [20, 20, 20]] {
                let (r, rep) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::click());
                ui.painter().circle_filled(r.center(), 8.0, Color32::from_rgb(c[0], c[1], c[2]));
                if self.couleur == c {
                    ui.painter().circle_stroke(r.center(), 9.5, Stroke::new(2.0f32, Color32::WHITE));
                }
                if rep.clicked() {
                    self.couleur = c;
                }
            }
            ui.add(egui::Slider::new(&mut self.epaisseur, 1.0..=16.0).text("épaisseur"));
        });
        let mut action = 0;
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.ops.is_empty(), egui::Button::new("↶ Annuler")).clicked() {
                action = 1;
            }
            if ui.button("💾 Enregistrer").clicked() {
                action = 2;
            }
            if ui.button("Enregistrer sous…").clicked() {
                action = 3;
            }
            if ui.button("📋 Copier").clicked() {
                action = 4;
            }
            if ui.button("✖ Fermer").clicked() {
                action = 5;
            }
            ui.weak(format!("{} × {} px", bw as u32, bh as u32));
        });
        // image ajustée à la place disponible
        let dispo = ui.available_size();
        let echelle = (dispo.x / bw).min(dispo.y / bh).min(1.0).max(0.05);
        let (zone, rep) = ui.allocate_exact_size(egui::vec2(bw * echelle, bh * echelle), Sense::drag());
        if let Some(t) = &self.texture {
            ui.painter().image(t.id(), zone, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        let vers_image = |p: Pos2| (((p.x - zone.left()) / echelle).clamp(0.0, bw), ((p.y - zone.top()) / echelle).clamp(0.0, bh));
        let vers_ecran = |p: (f32, f32)| Pos2::new(zone.left() + p.0 * echelle, zone.top() + p.1 * echelle);
        if rep.drag_started() {
            self.trace = rep.interact_pointer_pos().map(vers_image);
        }
        if let (Some(a), Some(p)) = (self.trace, rep.interact_pointer_pos()) {
            let b = vers_image(p);
            let op = Op { outil: self.outil, a, b, couleur: self.couleur, epaisseur: self.epaisseur };
            // aperçu à l'écran
            let c = Color32::from_rgb(op.couleur[0], op.couleur[1], op.couleur[2]);
            let st = Stroke::new(op.epaisseur * echelle, c);
            let r = egui::Rect::from_two_pos(vers_ecran(a), vers_ecran(b));
            match op.outil {
                Outil::Rectangle => {
                    ui.painter().rect_stroke(r, 0.0, st, egui::StrokeKind::Middle);
                }
                Outil::Fleche => {
                    ui.painter().line_segment([vers_ecran(a), vers_ecran(b)], st);
                    for q in pointes(a, b, op.epaisseur) {
                        ui.painter().line_segment([vers_ecran(b), vers_ecran(q)], st);
                    }
                }
                Outil::Surligneur => {
                    ui.painter().rect_filled(r, 0.0, c.gamma_multiply(0.38));
                }
                Outil::Flou => {
                    ui.painter().rect_filled(r, 0.0, Color32::from_gray(128).gamma_multiply(0.7));
                }
            }
            if rep.drag_stopped() {
                let utile = (a.0 - b.0).abs() > 2.0 || (a.1 - b.1).abs() > 2.0;
                if utile {
                    self.ops.push(op);
                    self.a_rendre = true;
                }
                self.trace = None;
            }
        }
        match action {
            1 => {
                self.ops.pop();
                self.a_rendre = true;
            }
            2 | 3 | 4 => {
                let img = rendre(base, &self.ops);
                match action {
                    2 => self.enregistrer_image(&img, None),
                    3 => {
                        if let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).add_filter("JPEG", &["jpg"]).set_file_name(format!("Capture_{}.png", horodatage())).save_file() {
                            self.enregistrer_image(&img, Some(p));
                        }
                    }
                    _ => {
                        let ok = win::copier_image(&img);
                        self.p.lock().unwrap().message = if ok { "Copiée dans le presse-papiers.".into() } else { "Copie impossible.".into() };
                    }
                }
            }
            5 => {
                self.base = None;
                self.texture = None;
                self.ops.clear();
            }
            _ => {}
        }
    }
}

fn enregistrer(r: &Reglages) {
    let f = fichier();
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, serde_json::to_string_pretty(r).unwrap_or_default());
}

/// Capture prise : enregistrement, copie, puis l'éditeur.
fn livrer(r: &Mutex<Reglages>, p: &Mutex<Partage>, img: RgbaImage) {
    let reg = r.lock().unwrap().clone();
    let mut msg = vec![];
    if reg.auto_enregistrer {
        let _ = std::fs::create_dir_all(&reg.dossier);
        let chemin = reg.dossier.join(format!("Capture_{}.png", horodatage()));
        if img.save(&chemin).is_ok() {
            msg.push(format!("enregistrée ({})", chemin.file_name().unwrap_or_default().to_string_lossy()));
        }
    }
    if reg.auto_copier && win::copier_image(&img) {
        msg.push("copiée".into());
    }
    let mut x = p.lock().unwrap();
    x.message = format!("Capture {} × {} {}", img.width(), img.height(), msg.join(", "));
    x.nouvelle = Some(img);
    if reg.ouvrir_editeur {
        x.ouvrir = true;
        drop(x);
        systeme::montrer();
    }
}

/// Assemble les derniers segments du replay en une vidéo.
fn sauver_replay(v: &Mutex<Video>, r: &Reglages) -> String {
    if !matches!(*v.lock().unwrap(), Video::Replay(..)) {
        return "Le replay n'est pas actif.".into();
    }
    let segs: Vec<(PathBuf, std::time::SystemTime)> = std::fs::read_dir(dossier_replay()).into_iter().flatten().flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "mkv"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    let liste = segments_utiles(segs, r.replay_secondes);
    if liste.is_empty() {
        return "Replay vide pour l'instant (attends quelques secondes).".into();
    }
    let texte: String = liste.iter().map(|p| format!("file '{}'\n", p.display().to_string().replace('\'', "'\\''"))).collect();
    let fichier_liste = dossier_replay().join("liste.txt");
    if std::fs::write(&fichier_liste, texte).is_err() {
        return "Impossible d'écrire la liste du replay.".into();
    }
    let _ = std::fs::create_dir_all(&r.dossier);
    let sortie = r.dossier.join(format!("Replay_{}.mp4", horodatage()));
    let ok = crate::dependances::commande("ffmpeg").is_ok_and(|mut c| c.args(["-y", "-v", "error", "-err_detect", "ignore_err", "-f", "concat", "-safe", "0", "-i"]).arg(&fichier_liste)
        .args(["-c", "copy", "-movflags", "+faststart"]).arg(&sortie).status().is_ok_and(|s| s.success()));
    if ok || sortie.exists() { format!("Replay enregistré : {}", sortie.display()) } else { "Échec de l'assemblage du replay.".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles() {
        assert_eq!(rect_normalise((50, 40), (10, 10), 100, 100), Some((10, 10, 40, 30)));
        assert_eq!(rect_normalise((-20, 5), (30, 500), 100, 100), Some((0, 5, 30, 95)));
        assert_eq!(rect_normalise((5, 5), (6, 6), 100, 100), None);
    }

    #[test]
    fn annotations() {
        let base = RgbaImage::from_pixel(100, 60, image::Rgba([10, 10, 10, 255]));
        let rouge = [255, 0, 0];
        let img = rendre(&base, &[Op { outil: Outil::Rectangle, a: (10.0, 10.0), b: (90.0, 50.0), couleur: rouge, epaisseur: 3.0 }]);
        assert_eq!(img.get_pixel(10, 30)[0], 255); // bord gauche
        assert_eq!(img.get_pixel(50, 30)[0], 10); // intérieur intact
        let img = rendre(&base, &[Op { outil: Outil::Surligneur, a: (0.0, 0.0), b: (20.0, 20.0), couleur: [255, 255, 0], epaisseur: 1.0 }]);
        assert!(img.get_pixel(5, 5)[0] > 80 && img.get_pixel(30, 30)[0] == 10);
        // flou : un bloc devient uniforme
        let mut damier = RgbaImage::new(24, 24);
        for (x, y, p) in damier.enumerate_pixels_mut() {
            *p = if (x + y) % 2 == 0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([200, 200, 200, 255]) };
        }
        let f = rendre(&damier, &[Op { outil: Outil::Flou, a: (0.0, 0.0), b: (12.0, 12.0), couleur: rouge, epaisseur: 1.0 }]);
        assert_eq!(f.get_pixel(0, 0), f.get_pixel(11, 11));
        assert_eq!(f.get_pixel(0, 0)[0], 100);
    }

    #[test]
    fn fleche() {
        let [p, q] = pointes((0.0, 0.0), (100.0, 0.0), 2.0);
        assert!(p.0 < 100.0 && q.0 < 100.0 && (p.1 + q.1).abs() < 0.01);
    }

    #[test]
    fn couleurs_hex() {
        assert_eq!(hex([255, 80, 0]), "#FF5000");
    }

    #[test]
    fn replay() {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = |s| UNIX_EPOCH + Duration::from_secs(s);
        let segs: Vec<(PathBuf, SystemTime)> = (0..10u64).map(|i| (PathBuf::from(format!("seg{:03}.mkv", (i + 3) % 10)), t(100 + i * 5))).collect();
        let l = segments_utiles(segs, 30);
        assert_eq!(l.len(), 7);
        assert_eq!(l.last().unwrap(), &PathBuf::from("seg002.mkv")); // le plus récent
    }
}
