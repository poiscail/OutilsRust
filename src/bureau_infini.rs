//! Bureau infini (5) : une immense table de travail où l'on pose notes, listes, fichiers, liens et
//! (sur Windows) les fenêtres ouvertes, rangés en « coins » (école, jeu, projet…). On dézoome pour
//! tout voir, et « Ouvrir le coin » ramène toutes ses fenêtres devant et range celles des autres coins.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Carte {
    Note { texte: String, couleur: [u8; 3] },
    Liste { titre: String, items: Vec<(bool, String)> },
    Fichier { chemin: PathBuf },
    Lien { url: String, titre: String },
    Fenetre { titre: String, exe: PathBuf },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Element {
    pub id: u64,
    pub pos: [f32; 2],
    pub taille: [f32; 2],
    pub carte: Carte,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Coin {
    pub id: u64,
    pub nom: String,
    pub rect: [f32; 4],
    pub couleur: [u8; 3],
}

impl Coin {
    pub fn r(&self) -> Rect {
        Rect::from_min_size(Pos2::new(self.rect[0], self.rect[1]), Vec2::new(self.rect[2], self.rect[3]))
    }
}

impl Element {
    pub fn r(&self) -> Rect {
        Rect::from_min_size(Pos2::new(self.pos[0], self.pos[1]), Vec2::new(self.taille[0], self.taille[1]))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Table {
    pub elements: Vec<Element>,
    pub coins: Vec<Coin>,
    pub vue: [f32; 4],
    pub prochain: u64,
}

impl Table {
    fn nouvel_id(&mut self) -> u64 {
        self.prochain += 1;
        self.prochain
    }
    /// Éléments dont le centre est dans le coin.
    pub fn dans(&self, c: &Coin) -> Vec<usize> {
        let r = c.r();
        self.elements.iter().enumerate().filter(|(_, e)| r.contains(e.r().center())).map(|(k, _)| k).collect()
    }
    /// Rectangle qui contient tout.
    pub fn tout(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for x in self.elements.iter().map(|e| e.r()).chain(self.coins.iter().map(|c| c.r())) {
            r = Some(r.map_or(x, |a| a.union(x)));
        }
        r
    }
    fn exemple() -> Table {
        let mut t = Table::default();
        t.coins.push(Coin { id: 1, nom: "📚 École".into(), rect: [0.0, 0.0, 700.0, 460.0], couleur: [80, 140, 230] });
        t.coins.push(Coin { id: 2, nom: "🎮 Jeux".into(), rect: [760.0, 0.0, 560.0, 460.0], couleur: [200, 90, 200] });
        t.elements.push(Element { id: 3, pos: [30.0, 60.0], taille: [260.0, 170.0], carte: Carte::Note { texte: "Bienvenue sur ton bureau infini !\n\nGlisse le fond pour te déplacer, Ctrl + molette pour zoomer. Glisse des fichiers ici depuis l'explorateur.".into(), couleur: [255, 236, 150] } });
        t.elements.push(Element { id: 4, pos: [320.0, 60.0], taille: [240.0, 170.0], carte: Carte::Liste { titre: "À faire".into(), items: vec![(false, "Exposé d'histoire".into()), (true, "Exercices de maths".into())] } });
        t.elements.push(Element { id: 5, pos: [790.0, 60.0], taille: [240.0, 110.0], carte: Carte::Note { texte: "Mets ici les fenêtres de tes jeux, Discord…".into(), couleur: [200, 230, 255] } });
        t.prochain = 10;
        t
    }
}

// ---------------------------------------------------------------------------------------------------
// Fenêtres de Windows
// ---------------------------------------------------------------------------------------------------

#[cfg(windows)]
mod fenetres {
    use image::RgbaImage;
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn rappel(h: HWND, l: LPARAM) -> BOOL {
        let v = &mut *(l as *mut Vec<(isize, String, PathBuf)>);
        if IsWindowVisible(h) == 0 || !GetWindow(h, GW_OWNER).is_null() || GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        let mut cache = 0i32;
        windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute(h, 14, &mut cache as *mut i32 as _, 4);
        if cache != 0 {
            return 1;
        }
        let mut b = [0u16; 256];
        let n = GetWindowTextW(h, b.as_mut_ptr(), 256);
        if n > 0 {
            let t = String::from_utf16_lossy(&b[..n as usize]);
            if t != "Program Manager" && t != "Outils" {
                v.push((h as isize, t, exe(h)));
            }
        }
        1
    }

    fn exe(h: HWND) -> PathBuf {
        use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() {
                return PathBuf::new();
            }
            let mut b = [0u16; 1024];
            let mut n = 1024u32;
            let ok = QueryFullProcessImageNameW(p, 0, b.as_mut_ptr(), &mut n) != 0;
            windows_sys::Win32::Foundation::CloseHandle(p);
            if ok { PathBuf::from(String::from_utf16_lossy(&b[..n as usize])) } else { PathBuf::new() }
        }
    }

    /// Fenêtres ouvertes : (handle, titre, programme).
    pub fn liste() -> Vec<(isize, String, PathBuf)> {
        let mut v = vec![];
        unsafe { EnumWindows(Some(rappel), &mut v as *mut _ as LPARAM) };
        v
    }

    pub fn existe(h: isize) -> bool {
        unsafe { IsWindow(h as HWND) != 0 }
    }

    pub fn devant(h: isize) {
        unsafe {
            let h = h as HWND;
            if IsIconic(h) != 0 {
                ShowWindow(h, SW_RESTORE);
            }
            // Windows n'autorise le premier plan qu'après une « touche » : Alt simulé
            windows_sys::Win32::UI::Input::KeyboardAndMouse::keybd_event(0x12, 0, 0, 0);
            SetForegroundWindow(h);
            windows_sys::Win32::UI::Input::KeyboardAndMouse::keybd_event(0x12, 0, 2, 0);
        }
    }

    pub fn reduire(h: isize) {
        unsafe { ShowWindow(h as HWND, SW_MINIMIZE) };
    }

    /// Aperçu de la fenêtre (même cachée derrière d'autres).
    pub fn apercu(h: isize, largeur: u32) -> Option<RgbaImage> {
        use windows_sys::Win32::Graphics::Gdi::*;
        unsafe {
            let h = h as HWND;
            if IsIconic(h) != 0 {
                return None;
            }
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(h, &mut r);
            let (l, ht) = (r.right - r.left, r.bottom - r.top);
            if l <= 0 || ht <= 0 || l > 10000 || ht > 10000 {
                return None;
            }
            let ecran = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(ecran);
            let bm = CreateCompatibleBitmap(ecran, l, ht);
            let vieux = SelectObject(mem, bm);
            let ok = windows_sys::Win32::Storage::Xps::PrintWindow(h, mem, 2 /* PW_RENDERFULLCONTENT */) != 0;
            SelectObject(mem, vieux);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = l;
            bi.bmiHeader.biHeight = -ht;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            let mut px = vec![0u8; (l * ht * 4) as usize];
            let n = GetDIBits(mem, bm, 0, ht as u32, px.as_mut_ptr().cast(), &mut bi, DIB_RGB_COLORS);
            DeleteObject(bm);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), ecran);
            if !ok || n == 0 {
                return None;
            }
            for p in px.chunks_exact_mut(4) {
                p.swap(0, 2);
                p[3] = 255;
            }
            let img = RgbaImage::from_raw(l as u32, ht as u32, px)?;
            let hauteur = (largeur as f32 * ht as f32 / l as f32) as u32;
            Some(image::imageops::thumbnail(&img, largeur, hauteur.max(1)))
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

const COULEURS_NOTES: [[u8; 3]; 5] = [[255, 236, 150], [200, 230, 255], [210, 245, 200], [255, 210, 220], [230, 215, 255]];
const COULEURS_COINS: [[u8; 3]; 6] = [[80, 140, 230], [200, 90, 200], [80, 190, 120], [230, 150, 60], [220, 80, 80], [140, 140, 150]];

pub struct Bureau {
    t: Table,
    vue: Rect,
    modifie: Option<Instant>,
    images: HashMap<PathBuf, Option<egui::TextureHandle>>,
    /// fenêtres liées aux cartes : id → (handle, aperçu, date de l'aperçu)
    liees: HashMap<u64, (isize, Option<egui::TextureHandle>, Instant)>,
    choix_fenetre: bool,
    lien: String,
    edition: Option<u64>,
    message: String,
}

impl Bureau {
    pub fn new() -> Self {
        let mut t: Table = charger("bureau_infini");
        if t.elements.is_empty() && t.coins.is_empty() {
            t = Table::exemple();
        }
        let vue = if t.vue[2] > 0.0 { Rect::from_min_size(Pos2::new(t.vue[0], t.vue[1]), Vec2::new(t.vue[2], t.vue[3])) } else { t.tout().map(|r| r.expand(60.0)).unwrap_or(Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0))) };
        Bureau { t, vue, modifie: None, images: HashMap::new(), liees: HashMap::new(), choix_fenetre: false, lien: String::new(), edition: None, message: String::new() }
    }

    fn ajouter(&mut self, carte: Carte, taille: [f32; 2], pos: Option<Pos2>) -> u64 {
        let id = self.t.nouvel_id();
        let c = pos.unwrap_or(self.vue.center() + Vec2::new((self.t.elements.len() % 5) as f32 * 24.0, (self.t.elements.len() % 5) as f32 * 24.0));
        self.t.elements.push(Element { id, pos: [c.x - taille[0] / 2.0, c.y - taille[1] / 2.0], taille, carte });
        self.modifie = Some(Instant::now());
        id
    }

    /// Relie les cartes « fenêtre » aux fenêtres ouvertes (après un redémarrage).
    #[cfg(windows)]
    fn relier(&mut self) {
        let ouvertes = fenetres::liste();
        for e in &self.t.elements {
            if let Carte::Fenetre { titre, exe } = &e.carte {
                let ok = self.liees.get(&e.id).is_some_and(|x| fenetres::existe(x.0));
                if !ok {
                    let pris: Vec<isize> = self.liees.values().map(|x| x.0).collect();
                    let trouve = ouvertes.iter().find(|(h, t, x)| !pris.contains(h) && x == exe && t == titre).or_else(|| ouvertes.iter().find(|(h, _, x)| !pris.contains(h) && x == exe && !exe.as_os_str().is_empty()));
                    match trouve {
                        Some((h, _, _)) => {
                            self.liees.insert(e.id, (*h, None, Instant::now() - std::time::Duration::from_secs(60)));
                        }
                        None => {
                            self.liees.remove(&e.id);
                        }
                    }
                }
            }
        }
    }

    /// « Ouvrir le coin » : ses fenêtres devant, celles des autres coins réduites, ses fichiers ouverts.
    fn ouvrir_coin(&mut self, k: usize) {
        let ids: Vec<usize> = self.t.dans(&self.t.coins[k]);
        #[cfg(windows)]
        {
            self.relier();
            for (j, c) in self.t.coins.iter().enumerate() {
                if j == k {
                    continue;
                }
                for i in self.t.dans(c) {
                    if let Some((h, _, _)) = self.liees.get(&self.t.elements[i].id) {
                        fenetres::reduire(*h);
                    }
                }
            }
        }
        let mut n = 0;
        for i in ids {
            let e = self.t.elements[i].clone();
            match &e.carte {
                Carte::Fenetre { exe, .. } => {
                    #[cfg(windows)]
                    match self.liees.get(&e.id) {
                        Some((h, _, _)) => fenetres::devant(*h),
                        None if exe.is_file() => {
                            let _ = std::process::Command::new(exe).spawn();
                        }
                        None => {}
                    }
                    #[cfg(not(windows))]
                    let _ = exe;
                    n += 1;
                }
                Carte::Fichier { chemin } => {
                    crate::outil::ouvrir(chemin);
                    n += 1;
                }
                _ => {}
            }
        }
        self.message = format!("🚀 « {} » ouvert ({n} élément(s))", self.t.coins[k].nom);
    }

    fn texture(&mut self, ctx: &egui::Context, p: &PathBuf) -> Option<egui::TextureHandle> {
        self.images
            .entry(p.clone())
            .or_insert_with(|| {
                image::open(p).ok().map(|i| {
                    let t = i.thumbnail(320, 320).to_rgba8();
                    ctx.load_texture(p.to_string_lossy(), egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default())
                })
            })
            .clone()
    }
}

fn col(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

impl Outil for Bureau {
    fn nom(&self) -> &'static str {
        "🪟 Bureau infini"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // barre d'outils
        ui.horizontal_wrapped(|ui| {
            ui.heading("🪟 Bureau infini");
            ui.separator();
            if ui.button("📝 Note").clicked() {
                let c = COULEURS_NOTES[self.t.elements.len() % COULEURS_NOTES.len()];
                let id = self.ajouter(Carte::Note { texte: String::new(), couleur: c }, [220.0, 150.0], None);
                self.edition = Some(id);
            }
            if ui.button("☑ Liste").clicked() {
                self.ajouter(Carte::Liste { titre: "Liste".into(), items: vec![(false, String::new())] }, [230.0, 170.0], None);
            }
            if ui.button("📄 Fichier").clicked() {
                if let Some(v) = rfd::FileDialog::new().pick_files() {
                    for p in v {
                        let img = crate::sans_dossiers::Genre::de(&p) == crate::sans_dossiers::Genre::Photo;
                        self.ajouter(Carte::Fichier { chemin: p }, if img { [220.0, 190.0] } else { [200.0, 70.0] }, None);
                    }
                }
            }
            ui.menu_button("🔗 Lien", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.lien).hint_text("https://…"));
                if ui.button("Ajouter").clicked() && !self.lien.trim().is_empty() {
                    let u = self.lien.trim().to_string();
                    let u = if u.contains("://") { u } else { format!("https://{u}") };
                    let titre = u.split("://").nth(1).unwrap_or(&u).split('/').next().unwrap_or("").trim_start_matches("www.").to_string();
                    self.ajouter(Carte::Lien { url: u, titre }, [220.0, 64.0], None);
                    self.lien.clear();
                    ui.close_menu();
                }
            });
            #[cfg(windows)]
            if ui.button("🪟 Fenêtre ouverte").clicked() {
                self.choix_fenetre = !self.choix_fenetre;
            }
            if ui.button("🟦 Coin").on_hover_text("Une zone pour regrouper (école, jeux, un projet…)").clicked() {
                let id = self.t.nouvel_id();
                let c = self.vue.center();
                let couleur = COULEURS_COINS[self.t.coins.len() % COULEURS_COINS.len()];
                self.t.coins.push(Coin { id, nom: format!("Coin {}", self.t.coins.len() + 1), rect: [c.x - 300.0, c.y - 200.0, 600.0, 400.0], couleur });
                self.modifie = Some(Instant::now());
            }
            ui.separator();
            if ui.button("🔭 Tout voir").clicked() {
                if let Some(r) = self.t.tout() {
                    self.vue = r.expand(60.0);
                }
            }
        });
        // aller vers un coin / l'ouvrir
        let mut ouvrir = None;
        if !self.t.coins.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.weak("Coins :");
                for (k, c) in self.t.coins.iter().enumerate() {
                    if ui.button(RichText::new(format!("🎯 {}", c.nom)).color(col(c.couleur))).on_hover_text("Aller voir").clicked() {
                        self.vue = c.r().expand(40.0);
                    }
                    if ui.small_button("🚀").on_hover_text("Ouvrir le coin : ses fenêtres et fichiers devant, les fenêtres des autres coins réduites").clicked() {
                        ouvrir = Some(k);
                    }
                }
            });
        }
        if let Some(k) = ouvrir {
            self.ouvrir_coin(k);
        }
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
        #[cfg(windows)]
        if self.choix_fenetre {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label("Clique sur une fenêtre pour la poser sur le bureau :");
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for (h, titre, exe) in fenetres::liste() {
                        if ui.selectable_label(false, format!("🪟 {titre}")).clicked() {
                            let id = self.ajouter(Carte::Fenetre { titre, exe }, [300.0, 210.0], None);
                            self.liees.insert(id, (h, None, Instant::now() - std::time::Duration::from_secs(60)));
                            self.choix_fenetre = false;
                        }
                    }
                });
            });
        }
        // fichiers glissés depuis l'explorateur
        let deposes: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        for p in deposes {
            let img = crate::sans_dossiers::Genre::de(&p) == crate::sans_dossiers::Genre::Photo;
            self.ajouter(Carte::Fichier { chemin: p }, if img { [220.0, 190.0] } else { [200.0, 70.0] }, None);
        }
        // aperçus des fenêtres : un par image, toutes les 2 secondes
        #[cfg(windows)]
        {
            if ctx.cumulative_pass_nr() % 120 == 0 {
                self.relier();
            }
            if let Some((id, h)) = self.liees.iter().filter(|x| x.1 .2.elapsed().as_secs_f32() > 2.0).min_by_key(|x| x.1 .2).map(|(id, x)| (*id, x.0)) {
                let tex = fenetres::apercu(h, 360).map(|i| ctx.load_texture(format!("fenetre{id}"), egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()), Default::default()));
                if let Some(x) = self.liees.get_mut(&id) {
                    x.2 = Instant::now();
                    if tex.is_some() {
                        x.1 = tex;
                    }
                }
            }
            if !self.liees.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
        ui.weak("Glisse le fond pour te déplacer · Ctrl + molette pour zoomer · double-clic sur le fond : nouvelle note · glisse des fichiers ici");
        // la table
        let mut vue = self.vue;
        let mut a_supprimer: Option<u64> = None;
        let mut a_supprimer_coin: Option<u64> = None;
        let mut change = false;
        let mut nouvelle_note: Option<Pos2> = None;
        let mut images: Vec<(u64, PathBuf)> = vec![];
        let liees: HashMap<u64, (bool, Option<egui::TextureHandle>, isize)> = self.liees.iter().map(|(k, v)| (*k, (true, v.1.clone(), v.0))).collect();
        let mut devant: Option<isize> = None;
        let mut edition = self.edition;
        let scene = egui::Scene::new().zoom_range(0.08..=2.0);
        let rep = egui::Frame::canvas(ui.style()).show(ui, |ui| {
            scene.show(ui, &mut vue, |ui| {
                // quadrillage léger
                let clip = ui.clip_rect();
                let pas = 100.0;
                let p = ui.painter();
                let trait_ = Stroke::new(1.0_f32, ui.visuals().faint_bg_color);
                let mut x = (clip.left() / pas).floor() * pas;
                while x < clip.right() {
                    p.line_segment([Pos2::new(x, clip.top()), Pos2::new(x, clip.bottom())], trait_);
                    x += pas;
                }
                let mut y = (clip.top() / pas).floor() * pas;
                while y < clip.bottom() {
                    p.line_segment([Pos2::new(clip.left(), y), Pos2::new(clip.right(), y)], trait_);
                    y += pas;
                }
                // coins (dessous)
                let n_coins = self.t.coins.len();
                for k in 0..n_coins {
                    let c = self.t.coins[k].clone();
                    let r = c.r();
                    ui.painter().rect_filled(r, 14.0, col(c.couleur).gamma_multiply(0.12));
                    ui.painter().rect_stroke(r, 14.0, Stroke::new(3.0_f32, col(c.couleur).gamma_multiply(0.7)), egui::StrokeKind::Inside);
                    let entete = Rect::from_min_size(r.min, Vec2::new(r.width(), 40.0));
                    let rep = ui.interact(entete, ui.id().with(("coin", c.id)), Sense::click_and_drag());
                    if rep.dragged() {
                        let d = rep.drag_delta();
                        for i in self.t.dans(&c) {
                            self.t.elements[i].pos[0] += d.x;
                            self.t.elements[i].pos[1] += d.y;
                        }
                        self.t.coins[k].rect[0] += d.x;
                        self.t.coins[k].rect[1] += d.y;
                        change = true;
                    }
                    let mut nom = c.nom.clone();
                    let champ = Rect::from_min_size(r.min + Vec2::new(14.0, 6.0), Vec2::new((r.width() - 60.0).max(80.0), 28.0));
                    if ui.put(champ, egui::TextEdit::singleline(&mut nom).frame(false).font(FontId::proportional(22.0)).text_color(col(c.couleur))).changed() {
                        self.t.coins[k].nom = nom;
                        change = true;
                    }
                    let x = Rect::from_min_size(Pos2::new(r.right() - 34.0, r.top() + 8.0), Vec2::splat(24.0));
                    if ui.put(x, egui::Button::new("✖").frame(false)).on_hover_text("Enlever ce coin (les cartes restent)").clicked() {
                        a_supprimer_coin = Some(c.id);
                    }
                    // poignée de taille
                    let poignee = Rect::from_center_size(r.right_bottom() - Vec2::splat(10.0), Vec2::splat(20.0));
                    let rp = ui.interact(poignee, ui.id().with(("coin_taille", c.id)), Sense::drag());
                    ui.painter().text(poignee.center(), Align2::CENTER_CENTER, "◢", FontId::proportional(16.0), col(c.couleur));
                    if rp.dragged() {
                        self.t.coins[k].rect[2] = (self.t.coins[k].rect[2] + rp.drag_delta().x).max(200.0);
                        self.t.coins[k].rect[3] = (self.t.coins[k].rect[3] + rp.drag_delta().y).max(150.0);
                        change = true;
                    }
                }
                // cartes
                for k in 0..self.t.elements.len() {
                    let id = self.t.elements[k].id;
                    let r = self.t.elements[k].r();
                    let (fond, titre) = match &self.t.elements[k].carte {
                        Carte::Note { couleur, .. } => (col(*couleur), "📝".to_string()),
                        Carte::Liste { titre, .. } => (Color32::from_rgb(250, 250, 252), format!("☑ {titre}")),
                        Carte::Fichier { chemin } => (Color32::from_rgb(245, 245, 248), format!("{} {}", crate::sans_dossiers::Genre::de(chemin).icone(), chemin.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
                        Carte::Lien { titre, .. } => (Color32::from_rgb(232, 240, 255), format!("🔗 {titre}")),
                        Carte::Fenetre { titre, .. } => (Color32::from_rgb(40, 44, 52), format!("🪟 {titre}")),
                    };
                    let sombre = matches!(self.t.elements[k].carte, Carte::Fenetre { .. });
                    let encre = if sombre { Color32::from_gray(230) } else { Color32::from_gray(30) };
                    ui.painter().rect_filled(r.translate(Vec2::new(3.0, 4.0)), 8.0, Color32::from_black_alpha(50));
                    ui.painter().rect_filled(r, 8.0, fond);
                    // en-tête : on la prend pour déplacer
                    let entete = Rect::from_min_size(r.min, Vec2::new(r.width(), 24.0));
                    let rep = ui.interact(entete, ui.id().with(("carte", id)), Sense::click_and_drag());
                    if rep.dragged() {
                        let d = rep.drag_delta();
                        self.t.elements[k].pos[0] += d.x;
                        self.t.elements[k].pos[1] += d.y;
                        change = true;
                    }
                    ui.painter().with_clip_rect(entete.shrink2(Vec2::new(26.0, 0.0))).text(entete.left_center() + Vec2::new(8.0, 0.0), Align2::LEFT_CENTER, &titre, FontId::proportional(13.0), encre.gamma_multiply(0.8));
                    let x = Rect::from_min_size(Pos2::new(r.right() - 24.0, r.top() + 2.0), Vec2::splat(20.0));
                    if ui.put(x, egui::Button::new(RichText::new("✖").color(encre.gamma_multiply(0.6))).frame(false)).clicked() {
                        a_supprimer = Some(id);
                    }
                    let corps = Rect::from_min_max(r.min + Vec2::new(8.0, 26.0), r.max - Vec2::new(8.0, 8.0));
                    let mut ouvrir_ce: Option<String> = None;
                    match &mut self.t.elements[k].carte {
                        Carte::Note { texte, .. } => {
                            if edition == Some(id) {
                                let r = ui.put(corps, egui::TextEdit::multiline(texte).frame(false).text_color(encre).desired_width(corps.width()));
                                if r.changed() {
                                    change = true;
                                }
                                if r.lost_focus() {
                                    edition = None;
                                }
                                if !r.has_focus() && edition == Some(id) && !r.lost_focus() {
                                    r.request_focus();
                                }
                            } else {
                                let rc = ui.interact(corps, ui.id().with(("note", id)), Sense::click());
                                let g = ui.painter().layout(if texte.is_empty() { "(double-clic pour écrire)".into() } else { texte.clone() }, FontId::proportional(14.0), encre, corps.width());
                                ui.painter().with_clip_rect(corps).galley(corps.min, g, encre);
                                if rc.double_clicked() {
                                    edition = Some(id);
                                }
                            }
                        }
                        Carte::Liste { items, .. } => {
                            let mut y = corps.top();
                            let mut retirer = None;
                            for (i, (ok, t)) in items.iter_mut().enumerate() {
                                if y + 22.0 > corps.bottom() {
                                    break;
                                }
                                let case = Rect::from_min_size(Pos2::new(corps.left(), y), Vec2::new(22.0, 22.0));
                                if ui.put(case, egui::Checkbox::without_text(ok)).changed() {
                                    change = true;
                                }
                                let champ = Rect::from_min_size(Pos2::new(corps.left() + 24.0, y), Vec2::new(corps.width() - 46.0, 22.0));
                                let mut f = RichText::new("").color(encre);
                                if *ok {
                                    f = f.strikethrough();
                                }
                                let _ = f;
                                if ui.put(champ, egui::TextEdit::singleline(t).frame(false).text_color(if *ok { encre.gamma_multiply(0.45) } else { encre })).changed() {
                                    change = true;
                                }
                                if ui.put(Rect::from_min_size(Pos2::new(corps.right() - 20.0, y), Vec2::splat(20.0)), egui::Button::new(RichText::new("–").color(encre.gamma_multiply(0.5))).frame(false)).clicked() {
                                    retirer = Some(i);
                                }
                                y += 24.0;
                            }
                            if let Some(i) = retirer {
                                items.remove(i);
                                change = true;
                            }
                            if y + 22.0 <= corps.bottom() && ui.put(Rect::from_min_size(Pos2::new(corps.left(), y), Vec2::new(90.0, 20.0)), egui::Button::new(RichText::new("➕ ajouter").color(encre.gamma_multiply(0.6))).frame(false)).clicked() {
                                items.push((false, String::new()));
                                change = true;
                            }
                        }
                        Carte::Fichier { chemin } => {
                            let rc = ui.interact(corps, ui.id().with(("fichier", id)), Sense::click());
                            if crate::sans_dossiers::Genre::de(chemin) == crate::sans_dossiers::Genre::Photo {
                                images.push((id, chemin.clone()));
                            } else {
                                let dossier = chemin.parent().map(|p| p.display().to_string()).unwrap_or_default();
                                ui.painter().with_clip_rect(corps).text(corps.left_top(), Align2::LEFT_TOP, dossier, FontId::proportional(11.0), encre.gamma_multiply(0.55));
                            }
                            if !chemin.exists() {
                                ui.painter().text(corps.left_bottom(), Align2::LEFT_BOTTOM, "⚠ introuvable", FontId::proportional(11.0), Color32::from_rgb(220, 90, 60));
                            }
                            if rc.double_clicked() {
                                ouvrir_ce = Some(chemin.display().to_string());
                            }
                            rc.on_hover_text("Double-clic pour ouvrir");
                        }
                        Carte::Lien { url, .. } => {
                            let rc = ui.interact(corps, ui.id().with(("lien", id)), Sense::click());
                            ui.painter().with_clip_rect(corps).text(corps.left_top(), Align2::LEFT_TOP, url.as_str(), FontId::proportional(11.0), Color32::from_rgb(60, 100, 200));
                            if rc.double_clicked() {
                                ouvrir_ce = Some(url.clone());
                            }
                        }
                        Carte::Fenetre { .. } => {
                            let rc = ui.interact(corps, ui.id().with(("fenetre", id)), Sense::click());
                            match liees.get(&id) {
                                Some((_, Some(t), h)) => {
                                    let s = t.size_vec2();
                                    let e = (corps.width() / s.x).min(corps.height() / s.y);
                                    egui::Image::new(t).corner_radius(4.0).paint_at(ui, Rect::from_center_size(corps.center(), s * e));
                                    if rc.clicked() {
                                        devant = Some(*h);
                                    }
                                }
                                Some((_, None, h)) => {
                                    ui.painter().text(corps.center(), Align2::CENTER_CENTER, "(fenêtre réduite)\nclic pour l'afficher", FontId::proportional(12.0), encre.gamma_multiply(0.6));
                                    if rc.clicked() {
                                        devant = Some(*h);
                                    }
                                }
                                None => {
                                    ui.painter().text(corps.center(), Align2::CENTER_CENTER, "fermée — « 🚀 » sur son coin la relance", FontId::proportional(12.0), encre.gamma_multiply(0.6));
                                }
                            }
                        }
                    }
                    if let Some(o) = ouvrir_ce {
                        if o.contains("://") {
                            #[cfg(windows)]
                            let _ = std::process::Command::new("explorer").arg(&o).spawn();
                            #[cfg(not(windows))]
                            let _ = std::process::Command::new("xdg-open").arg(&o).spawn();
                        } else {
                            crate::outil::ouvrir(std::path::Path::new(&o));
                        }
                    }
                    // poignée de taille
                    let poignee = Rect::from_center_size(r.right_bottom() - Vec2::splat(7.0), Vec2::splat(14.0));
                    let rp = ui.interact(poignee, ui.id().with(("taille", id)), Sense::drag());
                    ui.painter().text(poignee.center(), Align2::CENTER_CENTER, "◢", FontId::proportional(11.0), encre.gamma_multiply(0.35));
                    if rp.dragged() {
                        let e = &mut self.t.elements[k];
                        e.taille[0] = (e.taille[0] + rp.drag_delta().x).max(120.0);
                        e.taille[1] = (e.taille[1] + rp.drag_delta().y).max(50.0);
                        change = true;
                    }
                }
                for (id, p) in images {
                    if let (Some(t), Some(e)) = (self.texture(&ctx, &p), self.t.elements.iter().find(|e| e.id == id)) {
                        let corps = Rect::from_min_max(e.r().min + Vec2::new(8.0, 26.0), e.r().max - Vec2::new(8.0, 8.0));
                        let s = t.size_vec2();
                        let z = (corps.width() / s.x).min(corps.height() / s.y);
                        egui::Image::new(&t).corner_radius(4.0).paint_at(ui, Rect::from_center_size(corps.center(), s * z));
                    }
                }
            })
        });
        // double-clic sur le fond : nouvelle note à cet endroit
        if rep.inner.response.double_clicked() {
            if let Some(p) = rep.inner.response.interact_pointer_pos() {
                let fond = rep.response.rect;
                let t = vue.size() / fond.size();
                nouvelle_note = Some(vue.min + (p - fond.min) * t.x);
            }
        }
        if let Some(p) = nouvelle_note {
            let c = COULEURS_NOTES[self.t.elements.len() % COULEURS_NOTES.len()];
            let id = self.ajouter(Carte::Note { texte: String::new(), couleur: c }, [220.0, 150.0], Some(p));
            edition = Some(id);
        }
        #[cfg(windows)]
        if let Some(h) = devant {
            fenetres::devant(h);
        }
        #[cfg(not(windows))]
        let _ = devant;
        self.edition = edition;
        if let Some(id) = a_supprimer {
            self.t.elements.retain(|e| e.id != id);
            self.liees.remove(&id);
            change = true;
        }
        if let Some(id) = a_supprimer_coin {
            self.t.coins.retain(|c| c.id != id);
            change = true;
        }
        if vue != self.vue {
            self.vue = vue;
            change = true;
        }
        if change {
            self.modifie.get_or_insert_with(Instant::now);
        }
        // enregistrement (pas à chaque image pendant qu'on glisse)
        if self.modifie.is_some_and(|t| t.elapsed().as_secs_f32() > 0.8) && !ui.input(|i| i.pointer.any_down()) {
            self.t.vue = [self.vue.min.x, self.vue.min.y, self.vue.width(), self.vue.height()];
            enregistrer("bureau_infini", &self.t);
            self.modifie = None;
        } else if self.modifie.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(900));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let t = Table::exemple();
        assert_eq!(t.dans(&t.coins[0]).len(), 2);
        assert_eq!(t.dans(&t.coins[1]).len(), 1);
        let tout = t.tout().unwrap();
        assert!(tout.contains(Pos2::new(1300.0, 400.0)));
        let j = serde_json::to_string(&t).unwrap();
        let u: Table = serde_json::from_str(&j).unwrap();
        assert!(u == t);
    }
}
