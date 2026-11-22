//! Viseur à l'écran : croix / point / cercle toujours au centre de l'écran principal, par-dessus les jeux
//! (en plein écran fenêtré), transparent aux clics. Ctrl+Alt+X pour l'afficher / le cacher.
//! Réglages dans %APPDATA%\Outils\viseur.json.
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Forme {
    Croix,
    Point,
    Cercle,
    CroixPoint,
    T,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    pub forme: Forme,
    pub couleur: [u8; 3],
    pub taille: u32,
    pub epaisseur: u32,
    pub ecart: u32,
    pub contour: bool,
    pub opacite: f32,
    pub decalage: (i32, i32),
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: false, forme: Forme::CroixPoint, couleur: [0, 255, 120], taille: 8, epaisseur: 2, ecart: 4, contour: true, opacite: 1.0, decalage: (0, 0) }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("viseur.json")
}

/// Image carrée du viseur : (côté, pixels BGRA prémultipliés). Le centre est le pixel (côté/2, côté/2).
pub fn dessiner(r: &Reglages) -> (i32, Vec<u8>) {
    let ep = r.epaisseur.clamp(1, 20) as i32;
    let rayon = (r.taille + r.ecart) as i32 + ep + 3;
    let cote = 2 * rayon + 1;
    let c = rayon;
    let mut masque = vec![0u8; (cote * cote) as usize]; // 0 rien, 1 contour, 2 couleur
    fn remplir(m: &mut [u8], cote: i32, (x0, y0, x1, y1): (i32, i32, i32, i32), v: u8) {
        for y in y0.max(0)..=y1.min(cote - 1) {
            for x in x0.max(0)..=x1.min(cote - 1) {
                let i = (y * cote + x) as usize;
                m[i] = m[i].max(v);
            }
        }
    }
    macro_rules! plein {
        ($a:expr, $b:expr, $x:expr, $y:expr, $v:expr) => {
            remplir(&mut masque, cote, ($a, $b, $x, $y), $v)
        };
    }
    let (t, e, demi) = (r.taille as i32, r.ecart as i32, ep / 2);
    let mut formes: Vec<(i32, i32, i32, i32)> = vec![];
    let croix = |f: &mut Vec<(i32, i32, i32, i32)>, haut: bool| {
        f.push((c - e - t, c - demi, c - e - 1, c - demi + ep - 1)); // gauche
        f.push((c + e + 1, c - demi, c + e + t, c - demi + ep - 1)); // droite
        f.push((c - demi, c + e + 1, c - demi + ep - 1, c + e + t)); // bas
        if haut {
            f.push((c - demi, c - e - t, c - demi + ep - 1, c - e - 1));
        }
    };
    let point = |f: &mut Vec<(i32, i32, i32, i32)>| f.push((c - demi, c - demi, c - demi + ep - 1, c - demi + ep - 1));
    match r.forme {
        Forme::Croix => croix(&mut formes, true),
        Forme::T => croix(&mut formes, false),
        Forme::Point => point(&mut formes),
        Forme::CroixPoint => {
            croix(&mut formes, true);
            point(&mut formes);
        }
        Forme::Cercle => {}
    }
    if r.contour {
        for &(a, b, x, y) in &formes {
            plein!(a - 1, b - 1, x + 1, y + 1, 1);
        }
    }
    for &(a, b, x, y) in &formes {
        plein!(a, b, x, y, 2);
    }
    if r.forme == Forme::Cercle {
        let rr = (t + e) as f32;
        for y in 0..cote {
            for x in 0..cote {
                let d = (((x - c) * (x - c) + (y - c) * (y - c)) as f32).sqrt();
                let i = (y * cote + x) as usize;
                if (d - rr).abs() <= ep as f32 / 2.0 {
                    masque[i] = 2;
                } else if r.contour && (d - rr).abs() <= ep as f32 / 2.0 + 1.0 {
                    masque[i] = masque[i].max(1);
                }
            }
        }
        if r.contour {
            plein!(c - 1, c - 1, c + 1, c + 1, 1);
        }
        plein!(c, c, c, c, 2);
    }
    let a = (r.opacite.clamp(0.05, 1.0) * 255.0) as u32;
    let mut px = vec![0u8; masque.len() * 4];
    for (i, m) in masque.iter().enumerate() {
        let (rgb, al) = match m {
            2 => (r.couleur, a),
            1 => ([0, 0, 0], a * 3 / 4),
            _ => continue,
        };
        // BGRA prémultiplié pour UpdateLayeredWindow
        px[i * 4] = (rgb[2] as u32 * al / 255) as u8;
        px[i * 4 + 1] = (rgb[1] as u32 * al / 255) as u8;
        px[i * 4 + 2] = (rgb[0] as u32 * al / 255) as u8;
        px[i * 4 + 3] = al as u8;
    }
    (cote, px)
}

mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub const WM_MAJ: u32 = WM_APP + 11;

    unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        DefWindowProcW(h, m, wp, lp)
    }

    unsafe fn afficher(h: HWND, r: &Reglages) {
        if !r.actif {
            ShowWindow(h, SW_HIDE);
            return;
        }
        let (cote, px) = dessiner(r);
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.bmiHeader.biWidth = cote;
        bi.bmiHeader.biHeight = -cote;
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
        let (l, ht) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let pos = POINT { x: l / 2 - cote / 2 + r.decalage.0, y: ht / 2 - cote / 2 + r.decalage.1 };
        let taille = SIZE { cx: cote, cy: cote };
        let source = POINT { x: 0, y: 0 };
        let melange = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(h, ecran, &pos, &taille, mem, &source, 0, &melange, ULW_ALPHA);
        SelectObject(mem, vieux);
        DeleteObject(bm);
        DeleteDC(mem);
        ReleaseDC(null_mut(), ecran);
        ShowWindow(h, SW_SHOWNOACTIVATE);
        SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }

    /// Fil de la fenêtre du viseur (+ raccourci Ctrl+Alt+X).
    pub fn lancer(r: Arc<Mutex<Reglages>>, fenetre: Arc<AtomicIsize>, pris: Arc<AtomicBool>, change: Arc<AtomicBool>) {
        std::thread::spawn(move || unsafe {
            let inst = GetModuleHandleW(null());
            let classe: Vec<u16> = "OutilsViseur".encode_utf16().chain([0]).collect();
            let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), ..std::mem::zeroed() };
            RegisterClassW(&wc);
            let h = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, classe.as_ptr(), classe.as_ptr(), WS_POPUP,
                0, 0, 1, 1, null_mut(), null_mut(), inst, null());
            if h.is_null() {
                return;
            }
            fenetre.store(h as isize, Relaxed);
            pris.store(crate::outil::raccourci(h, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, b'X' as u32) == 0, Relaxed);
            afficher(h, &r.lock().unwrap().clone());
            // certains jeux repassent devant : on réaffirme « au premier plan » régulièrement
            SetTimer(h, 1, 2000, None);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_HOTKEY => {
                        let mut x = r.lock().unwrap();
                        x.actif = !x.actif;
                        let copie = x.clone();
                        drop(x);
                        change.store(true, Relaxed);
                        afficher(h, &copie);
                    }
                    WM_TIMER => {
                        if r.lock().unwrap().actif {
                            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                        }
                    }
                    _ if msg.hwnd == h && msg.message == WM_MAJ => afficher(h, &r.lock().unwrap().clone()),
                    _ => {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        });
    }

    pub fn mettre_a_jour(h: isize) {
        if h != 0 {
            unsafe { PostMessageW(h as HWND, WM_MAJ, 0, 0) };
        }
    }
}

pub struct Viseur {
    r: Arc<Mutex<Reglages>>,
    fenetre: Arc<AtomicIsize>,
    pris: Arc<AtomicBool>,
    change: Arc<AtomicBool>,
    apercu: Option<(Reglages, egui::TextureHandle)>,
}

impl Viseur {
    pub fn new() -> Self {
        let r: Reglages = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let v = Viseur { r: Arc::new(Mutex::new(r)), fenetre: Arc::default(), pris: Arc::default(), change: Arc::default(), apercu: None };
        win::lancer(v.r.clone(), v.fenetre.clone(), v.pris.clone(), v.change.clone());
        v
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, serde_json::to_string_pretty(&*self.r.lock().unwrap()).unwrap_or_default());
    }

    /// Enregistre si le raccourci a basculé le viseur.
    pub fn tic(&mut self) {
        if self.change.swap(false, Relaxed) {
            self.enregistrer();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Viseur");
        let avant = self.r.lock().unwrap().clone();
        let mut r = avant.clone();
        ui.horizontal(|ui| {
            ui.checkbox(&mut r.actif, "Afficher le viseur");
            ui.weak("· Ctrl+Alt+X pour basculer");
            if self.pris.load(Relaxed) {
                ui.colored_label(Color32::LIGHT_RED, "(raccourci déjà pris)");
            }
        });
        ui.weak("Visible par-dessus les jeux en « plein écran fenêtré » / « sans bordure » (pas en plein écran exclusif). Attention : certains jeux en ligne interdisent les viseurs externes.");
        ui.separator();
        ui.columns(2, |c| {
            let ui = &mut c[0];
            ui.horizontal_wrapped(|ui| {
                for (f, n) in [(Forme::CroixPoint, "Croix + point"), (Forme::Croix, "Croix"), (Forme::T, "T"), (Forme::Point, "Point"), (Forme::Cercle, "Cercle")] {
                    ui.selectable_value(&mut r.forme, f, n);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Couleur");
                ui.color_edit_button_srgb(&mut r.couleur);
                for c in [[0, 255, 120], [255, 60, 60], [0, 230, 255], [255, 255, 0], [255, 95, 162], [255, 255, 255]] {
                    let (re, rep) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
                    ui.painter().circle_filled(re.center(), 7.0, Color32::from_rgb(c[0], c[1], c[2]));
                    if rep.clicked() {
                        r.couleur = c;
                    }
                }
            });
            ui.add(egui::Slider::new(&mut r.taille, 0..=40).text("longueur"));
            ui.add(egui::Slider::new(&mut r.epaisseur, 1..=10).text("épaisseur"));
            ui.add(egui::Slider::new(&mut r.ecart, 0..=20).text("écart au centre"));
            ui.add(egui::Slider::new(&mut r.opacite, 0.1..=1.0).text("opacité"));
            ui.checkbox(&mut r.contour, "Contour noir");
            ui.horizontal(|ui| {
                ui.label("Décalage");
                ui.add(egui::DragValue::new(&mut r.decalage.0).range(-200..=200).prefix("x "));
                ui.add(egui::DragValue::new(&mut r.decalage.1).range(-200..=200).prefix("y "));
                if ui.small_button("0").clicked() {
                    r.decalage = (0, 0);
                }
            });
            // aperçu agrandi
            let ui = &mut c[1];
            if self.apercu.as_ref().is_none_or(|(x, _)| x != &r) {
                let (cote, px) = dessiner(&r);
                let mut rgba = vec![0u8; px.len()];
                for i in 0..px.len() / 4 {
                    let a = px[i * 4 + 3];
                    if a > 0 {
                        // dé-prémultiplie pour egui
                        let f = |v: u8| ((v as u32 * 255) / a as u32).min(255) as u8;
                        rgba[i * 4..i * 4 + 4].copy_from_slice(&[f(px[i * 4 + 2]), f(px[i * 4 + 1]), f(px[i * 4]), a]);
                    }
                }
                let img = egui::ColorImage::from_rgba_unmultiplied([cote as usize, cote as usize], &rgba);
                self.apercu = Some((r.clone(), ui.ctx().load_texture("viseur", img, egui::TextureOptions::NEAREST)));
            }
            if let Some((_, t)) = &self.apercu {
                let (zone, _) = ui.allocate_exact_size(egui::vec2(220.0, 220.0), egui::Sense::hover());
                ui.painter().rect_filled(zone, 6.0, Color32::from_rgb(70, 90, 110));
                let s = t.size_vec2() * (200.0 / t.size_vec2().x.max(1.0)).min(6.0);
                ui.painter().image(t.id(), egui::Rect::from_center_size(zone.center(), s), egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)), Color32::WHITE);
                ui.weak("Aperçu agrandi");
            }
        });
        if r != avant {
            *self.r.lock().unwrap() = r;
            self.enregistrer();
            win::mettre_a_jour(self.fenetre.load(Relaxed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dessin() {
        let r = Reglages { forme: Forme::Croix, taille: 5, epaisseur: 1, ecart: 2, contour: false, opacite: 1.0, couleur: [255, 0, 0], ..Default::default() };
        let (cote, px) = dessiner(&r);
        let c = cote / 2;
        let alpha = |x: i32, y: i32| px[((y * cote + x) * 4 + 3) as usize];
        assert_eq!(alpha(c, c), 0); // centre vide (écart)
        assert_eq!(alpha(c + 3, c), 255); // bras droit
        assert_eq!(alpha(c, c - 7), 255); // bout du bras haut
        assert_eq!(alpha(c + 1, c + 1), 0);
        assert_eq!(px[((c * cote + c + 3) * 4 + 2) as usize], 255); // rouge en BGRA
        let p = Reglages { forme: Forme::Point, contour: false, ..r };
        let (cote, px) = dessiner(&p);
        assert_eq!(px[(((cote / 2) * cote + cote / 2) * 4 + 3) as usize], 255);
    }
}
