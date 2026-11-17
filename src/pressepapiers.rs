//! Presse-papiers : historique des copies (texte et images), même dossier que l'ancienne version C#
//! (%APPDATA%\GestionnairePressePapiers : historique.json, parametres.json, images\<sha256>.png).
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const TEXTE: u8 = 0;
pub const IMAGE: u8 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Item {
    pub kind: u8,
    pub text: String,
    pub key: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

impl Item {
    pub fn texte(t: &str) -> Item {
        Item { kind: TEXTE, text: t.into(), key: t.into(), pinned: false, time: maintenant(), source: String::new(), rtf: None, html: None }
    }

    /// Une seule URL http/https, sans espace.
    pub fn lien(&self) -> Option<&str> {
        let t = self.text.trim();
        let ok = self.kind == TEXTE && !t.chars().any(char::is_whitespace)
            && ["http://", "https://"].iter().any(|p| t.len() > p.len() && t[..p.len()].eq_ignore_ascii_case(p));
        ok.then_some(t)
    }

    pub fn apercu(&self) -> String {
        let t = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.chars().count() > 120 { format!("{}…", t.chars().take(120).collect::<String>()) } else { t }
    }
}

// ---------- dates (ISO 8601 comme .NET : 2026-11-16T21:34:26.9741181+01:00) ----------

pub(crate) fn jours(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}

fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// Décalage local actuel en secondes (heure locale - UTC).
fn decalage() -> i64 {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime};
    let s = |t: &SYSTEMTIME| jours(t.wYear as i64, t.wMonth as i64, t.wDay as i64) * 86400 + t.wHour as i64 * 3600 + t.wMinute as i64 * 60;
    let (mut l, mut u): (SYSTEMTIME, SYSTEMTIME) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    unsafe {
        GetLocalTime(&mut l);
        GetSystemTime(&mut u);
    }
    ((s(&l) - s(&u)) as f64 / 900.0).round() as i64 * 900 // au quart d'heure près
}

fn maintenant_utc() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn format_iso(utc: i64, off: i64) -> String {
    let l = utc + off;
    let (y, m, d) = civil(l.div_euclid(86400));
    let s = l.rem_euclid(86400);
    let (signe, o) = if off < 0 { ('-', -off) } else { ('+', off) };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.0000000{signe}{:02}:{:02}", s / 3600, s / 60 % 60, s % 60, o / 3600, o / 60 % 60)
}

pub fn maintenant() -> String {
    format_iso(maintenant_utc(), decalage())
}

/// Secondes UTC depuis 1970 ; sans fuseau = heure locale.
fn secondes(t: &str) -> Option<i64> {
    let n = |a: usize, b: usize| t.get(a..b)?.parse::<i64>().ok();
    let base = jours(n(0, 4)?, n(5, 7)?, n(8, 10)?) * 86400 + n(11, 13)? * 3600 + n(14, 16)? * 60 + n(17, 19)?;
    let reste = t.get(19..)?.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    Some(match reste.chars().next() {
        Some('Z') => base,
        Some(c @ ('+' | '-')) => {
            let o = reste.get(1..3)?.parse::<i64>().ok()? * 3600 + reste.get(4..6)?.parse::<i64>().ok()? * 60;
            if c == '+' { base - o } else { base + o }
        }
        _ => base - decalage(),
    })
}

// ---------- historique ----------

pub struct Historique {
    pub items: Vec<Item>,
    pub max: usize,
    pub max_jours: i64, // 0 = jamais d'effacement par l'âge
    fichier: PathBuf,
}

impl Historique {
    /// JSON abîmé : copié en .bak et on repart vide.
    pub fn charger(fichier: &Path, max: usize, max_jours: i64) -> Historique {
        let items = match std::fs::read_to_string(fichier) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                let _ = std::fs::copy(fichier, fichier.with_extension("json.bak"));
                vec![]
            }),
            Err(_) => vec![],
        };
        Historique { items, max, max_jours, fichier: fichier.into() }
    }

    pub fn sauver(&self) -> std::io::Result<()> {
        if let Some(d) = self.fichier.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.fichier.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(&self.items)?)?;
        std::fs::rename(tmp, &self.fichier)
    }

    /// Ajoute en tête ; un doublon garde son épingle, sa source et ses formats riches. Retourne les éléments retirés.
    pub fn ajouter(&mut self, mut item: Item) -> Vec<Item> {
        if let Some(i) = self.items.iter().position(|x| x.kind == item.kind && x.key == item.key) {
            let ancien = self.items.remove(i);
            item.pinned = ancien.pinned;
            if item.source.is_empty() {
                item.source = ancien.source;
            }
            item.rtf = item.rtf.or(ancien.rtf);
            item.html = item.html.or(ancien.html);
        }
        self.items.insert(0, item);
        self.tailler()
    }

    /// Retire les non épinglés plus vieux que max_jours, puis les plus vieux au-delà de max (le plus récent reste).
    pub fn tailler(&mut self) -> Vec<Item> {
        let mut out = vec![];
        if self.max_jours > 0 {
            let limite = maintenant_utc() - self.max_jours * 86400;
            for j in (1..self.items.len()).rev() {
                if !self.items[j].pinned && secondes(&self.items[j].time).is_some_and(|t| t < limite) {
                    out.push(self.items.remove(j));
                }
            }
        }
        let mut j = self.items.len();
        while j > 1 && self.items.len() > self.max {
            j -= 1;
            if !self.items[j].pinned {
                out.push(self.items.remove(j));
            }
        }
        out
    }

    pub fn effacer_non_epingles(&mut self) -> Vec<Item> {
        let (garder, out) = std::mem::take(&mut self.items).into_iter().partition(|x| x.pinned);
        self.items = garder;
        out
    }

    /// Indices filtrés (0 tout, 1 texte, 2 images, 3 liens), épinglés d'abord.
    pub fn vue(&self, requete: &str, filtre: u8) -> Vec<usize> {
        let q = requete.to_lowercase();
        let mut v: Vec<usize> = (0..self.items.len())
            .filter(|&i| {
                let x = &self.items[i];
                let f = match filtre {
                    1 => x.kind == TEXTE,
                    2 => x.kind == IMAGE,
                    3 => x.lien().is_some(),
                    _ => true,
                };
                f && (q.is_empty() || x.text.to_lowercase().contains(&q))
            })
            .collect();
        v.sort_by_key(|&i| !self.items[i].pinned); // tri stable
        v
    }
}

// ---------- Windows : presse-papiers, raccourci, collage ----------

mod win {
    use std::path::Path;
    use std::ptr::null_mut;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, GlobalFree};
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows_sys::Win32::System::DataExchange::*;
    use windows_sys::Win32::System::Memory::*;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const CF_UNICODETEXT: u32 = 13;
    const CF_DIB: u32 = 8;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn format(nom: &str) -> u32 {
        unsafe { RegisterClipboardFormatW(w(nom).as_ptr()) }
    }

    fn ouvrir() -> bool {
        for _ in 0..20 {
            if unsafe { OpenClipboard(null_mut()) } != 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    unsafe fn octets(fmt: u32) -> Option<Vec<u8>> {
        let h = GetClipboardData(fmt);
        if h.is_null() {
            return None;
        }
        let p = GlobalLock(h) as *const u8;
        if p.is_null() {
            return None;
        }
        let v = std::slice::from_raw_parts(p, GlobalSize(h)).to_vec();
        GlobalUnlock(h);
        Some(v)
    }

    unsafe fn poser(fmt: u32, data: &[u8]) -> bool {
        let h = GlobalAlloc(GMEM_MOVEABLE, data.len());
        if h.is_null() {
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), GlobalLock(h) as *mut u8, data.len());
        GlobalUnlock(h);
        if SetClipboardData(fmt, h).is_null() {
            GlobalFree(h);
            return false;
        }
        true
    }

    fn jusqu_au_nul(v: &[u8]) -> &[u8] {
        &v[..v.iter().position(|&b| b == 0).unwrap_or(v.len())]
    }

    pub enum Contenu {
        Texte { texte: String, rtf: Option<String>, html: Option<String> },
        Image(Vec<u8>), // DIB
    }

    /// Lit le presse-papiers ; None si vide, exclu (ex. mot de passe du Coffre) ou illisible.
    pub fn lire(images: bool) -> Option<Contenu> {
        if !ouvrir() {
            return None;
        }
        let r = unsafe {
            if IsClipboardFormatAvailable(format("ExcludeClipboardContentFromMonitorProcessing")) != 0 {
                None
            } else if IsClipboardFormatAvailable(CF_UNICODETEXT) != 0 {
                octets(CF_UNICODETEXT).and_then(|b| {
                    let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                    let texte = String::from_utf16_lossy(&u);
                    let riche = |nom: &str| octets(format(nom)).filter(|v| v.len() <= 1 << 20).map(|v| String::from_utf8_lossy(jusqu_au_nul(&v)).into_owned());
                    (!texte.trim().is_empty()).then(|| Contenu::Texte { rtf: riche("Rich Text Format"), html: riche("HTML Format"), texte })
                })
            } else if images && IsClipboardFormatAvailable(CF_DIB) != 0 {
                octets(CF_DIB).map(Contenu::Image)
            } else {
                None
            }
        };
        unsafe { CloseClipboard() };
        r
    }

    pub fn ecrire_texte(texte: &str, rtf: Option<&str>, html: Option<&str>) -> bool {
        if !ouvrir() {
            return false;
        }
        let ok = unsafe {
            EmptyClipboard();
            let data: Vec<u8> = texte.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
            let mut ok = poser(CF_UNICODETEXT, &data);
            for (nom, v) in [("Rich Text Format", rtf), ("HTML Format", html)] {
                if let Some(v) = v {
                    ok &= poser(format(nom), &[v.as_bytes(), &[0]].concat());
                }
            }
            ok
        };
        unsafe { CloseClipboard() };
        ok
    }

    pub fn ecrire_dib(dib: &[u8]) -> bool {
        if !ouvrir() {
            return false;
        }
        let ok = unsafe {
            EmptyClipboard();
            poser(CF_DIB, dib)
        };
        unsafe { CloseClipboard() };
        ok
    }

    fn description(exe: &Path) -> Option<String> {
        let p = w(&exe.to_string_lossy());
        unsafe {
            let n = GetFileVersionInfoSizeW(p.as_ptr(), null_mut());
            if n == 0 {
                return None;
            }
            let mut buf = vec![0u8; n as usize];
            if GetFileVersionInfoW(p.as_ptr(), 0, n, buf.as_mut_ptr() as _) == 0 {
                return None;
            }
            let (mut ptr, mut len) = (null_mut(), 0u32);
            if VerQueryValueW(buf.as_ptr() as _, w(r"\VarFileInfo\Translation").as_ptr(), &mut ptr, &mut len) == 0 || len < 4 {
                return None;
            }
            let t = std::slice::from_raw_parts(ptr as *const u16, 2);
            let cle = format!(r"\StringFileInfo\{:04x}{:04x}\FileDescription", t[0], t[1]);
            if VerQueryValueW(buf.as_ptr() as _, w(&cle).as_ptr(), &mut ptr, &mut len) == 0 || len == 0 {
                return None;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr as *const u16, len as usize));
            Some(s.trim_end_matches('\0').trim().to_string()).filter(|s| !s.is_empty())
        }
    }

    /// Appli qui a rempli le presse-papiers (description de l'exe, sinon son nom) ; "" si c'est nous.
    pub fn source() -> String {
        unsafe {
            let mut h = GetClipboardOwner();
            if h.is_null() {
                h = GetForegroundWindow();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, &mut pid);
            if pid == 0 || pid == std::process::id() {
                return String::new();
            }
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() {
                return String::new();
            }
            let mut buf = [0u16; 1024];
            let mut n = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n) != 0;
            CloseHandle(p);
            if !ok {
                return String::new();
            }
            let exe = std::path::PathBuf::from(String::from_utf16_lossy(&buf[..n as usize]));
            description(&exe).unwrap_or_else(|| exe.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
        }
    }

    pub fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    pub fn premier_plan() -> isize {
        unsafe { GetForegroundWindow() as isize }
    }

    /// Fenêtre principale du hub (titre "Outils", notre processus).
    pub fn hub() -> isize {
        let h = unsafe { FindWindowW(null_mut(), w("Outils").as_ptr()) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, &mut pid) };
        if pid == std::process::id() { h as isize } else { 0 }
    }

    pub fn montrer(h: isize) {
        unsafe {
            if IsIconic(h as _) != 0 {
                ShowWindow(h as _, SW_RESTORE);
            }
            SetForegroundWindow(h as _);
        }
    }

    fn touche(vk: u16, haut: bool) -> INPUT {
        let flags = if haut { KEYEVENTF_KEYUP } else { 0 };
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    /// Réduit le hub, rend la main à la fenêtre d'avant et y envoie Ctrl+V.
    pub fn coller_dans(cible: isize, hub: isize) {
        unsafe {
            if hub != 0 {
                ShowWindow(hub as _, SW_MINIMIZE);
            }
            SetForegroundWindow(cible as _);
        }
        std::thread::sleep(Duration::from_millis(150));
        // Maj relâchée d'abord : sinon un Maj+clic deviendrait Ctrl+Maj+V
        let seq = [touche(0x10, true), touche(0x11, false), touche(0x56, false), touche(0x56, true), touche(0x11, true)];
        unsafe { SendInput(seq.len() as u32, seq.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    }

    /// Enregistre le raccourci global (touches WinForms) et appelle `f` à chaque appui. Bloque : à lancer dans un fil.
    pub fn raccourci(touches: u32, mut f: impl FnMut()) -> Result<(), String> {
        let mut mods = MOD_NOREPEAT;
        if touches & 0x10000 != 0 {
            mods |= MOD_SHIFT;
        }
        if touches & 0x20000 != 0 {
            mods |= MOD_CONTROL;
        }
        if touches & 0x40000 != 0 {
            mods |= MOD_ALT;
        }
        if unsafe { RegisterHotKey(null_mut(), 1, mods, touches & 0xFFFF) } == 0 {
            return Err("raccourci déjà pris par une autre appli (ancienne version ouverte ?)".into());
        }
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {
            if msg.message == WM_HOTKEY {
                f();
            }
        }
        Ok(())
    }
}

pub fn nom_raccourci(t: u32) -> String {
    let mut s = String::new();
    for (bit, nom) in [(0x20000, "Ctrl+"), (0x10000, "Maj+"), (0x40000, "Alt+")] {
        if t & bit != 0 {
            s += nom;
        }
    }
    let vk = t & 0xFFFF;
    match char::from_u32(vk) {
        Some(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => s.push(c),
        _ => s += &format!("touche {vk:#X}"),
    }
    s
}

// ---------- images : DIB <-> PNG ----------

/// DIB 24/32 bits (BI_RGB ou BI_BITFIELDS) en RGBA.
pub fn dib_rgba(d: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let u32a = |o: usize| Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?));
    let taille = u32a(0)? as usize;
    let (larg, haut) = (u32a(4)? as i32, u32a(8)? as i32);
    let bpp = u16::from_le_bytes(d.get(14..16)?.try_into().ok()?) as usize;
    let comp = u32a(16)?;
    if larg <= 0 || haut == 0 || !(bpp == 24 || bpp == 32) || !(comp == 0 || comp == 3) {
        return None;
    }
    let off = taille + if comp == 3 && taille == 40 { 12 } else { 0 } + u32a(32)? as usize * 4;
    let (w, h) = (larg as usize, haut.unsigned_abs() as usize);
    let ligne = (w * bpp / 8 + 3) & !3;
    if d.len() < off + ligne * h {
        return None;
    }
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src = off + if haut > 0 { h - 1 - y } else { y } * ligne;
        for x in 0..w {
            let p = &d[src + x * bpp / 8..];
            let o = (y * w + x) * 4;
            out[o..o + 4].copy_from_slice(&[p[2], p[1], p[0], if bpp == 32 { p[3] } else { 255 }]);
        }
    }
    if out.chunks(4).all(|p| p[3] == 0) {
        out.chunks_mut(4).for_each(|p| p[3] = 255); // beaucoup d'applis laissent l'alpha à 0
    }
    Some((w as u32, h as u32, out))
}

pub fn rgba_dib(w: u32, h: u32, px: &[u8]) -> Vec<u8> {
    let mut d = Vec::with_capacity(40 + px.len());
    for v in [40u32, w, h] {
        d.extend(v.to_le_bytes());
    }
    d.extend(1u16.to_le_bytes());
    d.extend(32u16.to_le_bytes());
    d.extend([0u8; 24]);
    for y in (0..h as usize).rev() {
        for p in px[y * w as usize * 4..(y + 1) * w as usize * 4].chunks(4) {
            d.extend([p[2], p[1], p[0], p[3]]);
        }
    }
    d
}

fn png(w: u32, h: u32, px: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(w, h, px)?;
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

fn hex_sha256(b: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(b).iter().map(|x| format!("{x:02X}")).collect()
}

// ---------- surveillance ----------

struct Partage {
    hist: Mutex<Historique>,
    dossier: PathBuf,
    cible: Mutex<Option<isize>>, // fenêtre où coller, posée par le raccourci
    appel: std::sync::atomic::AtomicBool,
    erreur: Mutex<String>,
}

impl Partage {
    fn images(&self) -> PathBuf {
        self.dossier.join("images")
    }

    fn oublier(&self, retires: &[Item]) {
        for x in retires.iter().filter(|x| x.kind == IMAGE) {
            let _ = std::fs::remove_file(self.images().join(format!("{}.png", x.key)));
        }
    }

    fn capturer(&self, c: win::Contenu, source: String) {
        let mut item = match c {
            win::Contenu::Texte { texte, rtf, html } => Item { rtf, html, ..Item::texte(&texte) },
            win::Contenu::Image(dib) => {
                let Some((w, h, px)) = dib_rgba(&dib) else { return };
                let Some(png) = png(w, h, px) else { return };
                let cle = hex_sha256(&png);
                let chemin = self.images().join(format!("{cle}.png"));
                if !chemin.exists() && (std::fs::create_dir_all(self.images()).is_err() || std::fs::write(&chemin, &png).is_err()) {
                    return;
                }
                Item { kind: IMAGE, text: format!("Image {w}×{h}"), key: cle, ..Item::texte("") }
            }
        };
        item.source = source;
        let mut h = self.hist.lock().unwrap();
        let retires = h.ajouter(item);
        let _ = h.sauver();
        drop(h);
        self.oublier(&retires);
    }
}

fn reglages(dossier: &Path) -> Value {
    std::fs::read_to_string(dossier.join("parametres.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
}

// ---------- interface ----------

enum Action {
    Coller(usize, bool),
    Epingler(usize),
    Supprimer(Vec<usize>),
    Modifier(usize),
    Enregistrer(usize),
    Ouvrir(String),
    CopierSelection,
    Vider,
}

pub struct PressePapiers {
    p: Arc<Partage>,
    demarre: bool,
    touches: u32,
    images: bool,
    coller_au_clic: bool,
    recherche: String,
    filtre: u8,
    selection: HashSet<(u8, String)>,
    focus: bool,
    vider: bool,
    edition: Option<(String, String)>, // (clé d'origine, texte)
    statut: String,
}

impl PressePapiers {
    pub fn new() -> Self {
        let dossier = PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("GestionnairePressePapiers");
        let r = reglages(&dossier);
        let max = r["Max"].as_u64().unwrap_or(200).max(1) as usize;
        let hist = Historique::charger(&dossier.join("historique.json"), max, r["MaxDays"].as_i64().unwrap_or(0));
        PressePapiers {
            p: Arc::new(Partage { hist: Mutex::new(hist), dossier, cible: Mutex::new(None), appel: Default::default(), erreur: Default::default() }),
            demarre: false,
            touches: r["Hotkey"].as_u64().unwrap_or(0x30056) as u32,
            images: r["CaptureImages"].as_bool().unwrap_or(true),
            coller_au_clic: r["PasteOnClick"].as_bool().unwrap_or(true),
            recherche: String::new(),
            filtre: 0,
            selection: HashSet::new(),
            focus: false,
            vider: false,
            edition: None,
            statut: String::new(),
        }
    }

    /// Lance la surveillance et le raccourci au premier appel ; vrai quand le raccourci demande l'onglet.
    pub fn tic(&mut self, ctx: &egui::Context) -> bool {
        if !self.demarre {
            self.demarre = true;
            let (p, c, images) = (self.p.clone(), ctx.clone(), self.images);
            // shortcut: sondage toutes les 300 ms au lieu de AddClipboardFormatListener (pas de fenêtre à nous côté winit)
            std::thread::spawn(move || {
                let mut seq = win::sequence();
                loop {
                    std::thread::sleep(Duration::from_millis(300));
                    let s = win::sequence();
                    if s == seq {
                        continue;
                    }
                    seq = s;
                    std::thread::sleep(Duration::from_millis(50)); // laisse l'appli finir d'écrire
                    if let Some(contenu) = win::lire(images) {
                        p.capturer(contenu, win::source());
                        c.request_repaint();
                    }
                }
            });
            let (p, c, t) = (self.p.clone(), ctx.clone(), self.touches);
            std::thread::spawn(move || {
                let p2 = p.clone();
                let r = win::raccourci(t, || {
                    let (avant, hub) = (win::premier_plan(), win::hub());
                    *p.cible.lock().unwrap() = (avant != hub && avant != 0).then_some(avant);
                    p.appel.store(true, std::sync::atomic::Ordering::SeqCst);
                    win::montrer(hub); // le hub réduit ne repeint pas : on le restaure d'ici
                    c.request_repaint();
                });
                if let Err(e) = r {
                    *p2.erreur.lock().unwrap() = e;
                }
            });
        }
        let appel = self.p.appel.swap(false, std::sync::atomic::Ordering::SeqCst);
        if appel {
            self.recherche.clear();
            self.focus = true;
        }
        appel
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let p = self.p.clone();
        let mut h = p.hist.lock().unwrap();
        let mut actions = vec![];

        ui.heading("Presse-papiers");
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("Rechercher…").desired_width(260.0));
            if std::mem::take(&mut self.focus) {
                r.request_focus();
            }
            for (i, n) in ["Tout", "Texte", "Images", "Liens"].iter().enumerate() {
                ui.selectable_value(&mut self.filtre, i as u8, *n);
            }
            ui.separator();
            if !self.selection.is_empty() && ui.button(format!("Copier la sélection ({})", self.selection.len())).clicked() {
                actions.push(Action::CopierSelection);
            }
            if self.vider {
                if ui.button("Confirmer").clicked() {
                    actions.push(Action::Vider);
                }
                if ui.button("Annuler").clicked() {
                    self.vider = false;
                }
            } else if ui.button("Effacer les non épinglés").clicked() {
                self.vider = true;
            }
        });
        let err = p.erreur.lock().unwrap().clone();
        let mut info = format!("{} éléments · raccourci {}", h.items.len(), nom_raccourci(self.touches));
        if !err.is_empty() {
            info += &format!(" : {err}");
        }
        ui.horizontal(|ui| {
            ui.weak(info);
            ui.weak("· clic : coller (Maj : sans mise en forme) · Ctrl+clic : sélection · Suppr · F2");
        });
        if !self.statut.is_empty() {
            ui.label(&self.statut);
        }
        ui.separator();

        let vue = h.vue(&self.recherche, self.filtre);
        let images = p.images();
        let (selection, items) = (&self.selection, &h.items);
        let mods = ui.input(|i| i.modifiers);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, 46.0, vue.len(), |ui, rangee| {
            for &i in &vue[rangee] {
                let x = &items[i];
                let choisi = selection.contains(&(x.kind, x.key.clone()));
                ui.horizontal(|ui| {
                    ui.set_height(42.0);
                    let pin = egui::RichText::new("📌").color(if x.pinned { egui::Color32::from_rgb(230, 180, 60) } else { egui::Color32::DARK_GRAY });
                    if ui.add(egui::Button::new(pin).frame(false)).on_hover_text("Épingler").clicked() {
                        actions.push(Action::Epingler(i));
                    }
                    if x.kind == IMAGE {
                        let uri = format!("file://{}", images.join(format!("{}.png", x.key)).display());
                        ui.add(egui::Image::new(uri).max_size(egui::vec2(64.0, 40.0)));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").on_hover_text("Supprimer").clicked() {
                            actions.push(Action::Supprimer(vec![i]));
                        }
                        if ui.small_button("💾").on_hover_text("Enregistrer sous").clicked() {
                            actions.push(Action::Enregistrer(i));
                        }
                        if x.kind == TEXTE && ui.small_button("✏").on_hover_text("Modifier (F2)").clicked() {
                            actions.push(Action::Modifier(i));
                        }
                        if let Some(u) = x.lien() {
                            if ui.small_button("🔗").on_hover_text("Ouvrir le lien").clicked() {
                                actions.push(Action::Ouvrir(u.into()));
                            }
                        }
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            let mut t = egui::RichText::new(x.apercu());
                            if choisi {
                                t = t.strong().color(ui.visuals().selection.stroke.color);
                            }
                            let r = ui.add(egui::Label::new(t).truncate().sense(egui::Sense::click()));
                            let quand = x.time.get(11..16).map(|hm| format!("{}/{} {hm}", &x.time[8..10], &x.time[5..7])).unwrap_or_default();
                            ui.weak(if x.source.is_empty() { quand } else { format!("{quand} · {}", x.source) });
                            if r.clicked() {
                                if mods.command {
                                    actions.push(Action::Epingler(usize::MAX - i)); // bascule la sélection
                                } else {
                                    actions.push(Action::Coller(i, mods.shift));
                                }
                            }
                        });
                    });
                });
            }
        });

        // touches
        let edition_ouverte = self.edition.is_some();
        if !edition_ouverte && !ui.ctx().wants_keyboard_input() {
            let choisis: Vec<usize> = (0..h.items.len()).filter(|&i| self.selection.contains(&(h.items[i].kind, h.items[i].key.clone()))).collect();
            if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !choisis.is_empty() {
                actions.push(Action::Supprimer(choisis.clone()));
            }
            if ui.input(|i| i.key_pressed(egui::Key::F2)) && choisis.len() == 1 && h.items[choisis[0]].kind == TEXTE {
                actions.push(Action::Modifier(choisis[0]));
            }
        }

        let mut change = false;
        for a in actions {
            match a {
                Action::Epingler(j) if j > usize::MAX / 2 => {
                    let x = &h.items[usize::MAX - j];
                    let k = (x.kind, x.key.clone());
                    if !self.selection.remove(&k) {
                        self.selection.insert(k);
                    }
                }
                Action::Epingler(i) => {
                    h.items[i].pinned ^= true;
                    change = true;
                }
                Action::Coller(i, brut) => {
                    let x = h.items[i].clone();
                    drop(h);
                    self.coller(&x, brut);
                    h = p.hist.lock().unwrap();
                }
                Action::Supprimer(mut v) => {
                    v.sort_unstable();
                    let retires: Vec<Item> = v.iter().rev().map(|&i| h.items.remove(i)).collect();
                    for x in &retires {
                        self.selection.remove(&(x.kind, x.key.clone()));
                    }
                    p.oublier(&retires);
                    change = true;
                }
                Action::Modifier(i) => self.edition = Some((h.items[i].key.clone(), h.items[i].text.clone())),
                Action::Enregistrer(i) => self.enregistrer(&h.items[i]),
                Action::Ouvrir(u) => {
                    let _ = std::process::Command::new("explorer").arg(u).spawn();
                }
                Action::CopierSelection => {
                    let t: Vec<&str> = h.items.iter().filter(|x| x.kind == TEXTE && self.selection.contains(&(x.kind, x.key.clone()))).map(|x| x.text.as_str()).collect();
                    self.statut = if win::ecrire_texte(&t.join("\n"), None, None) { format!("{} textes copiés", t.len()) } else { "Presse-papiers occupé".into() };
                }
                Action::Vider => {
                    let r = h.effacer_non_epingles();
                    p.oublier(&r);
                    self.selection.clear();
                    self.vider = false;
                    change = true;
                }
            }
        }

        if let Some((cle, texte)) = &mut self.edition {
            let mut fini = None;
            egui::Window::new("Modifier").collapsible(false).default_width(500.0).show(ui.ctx(), |ui| {
                egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(texte).desired_width(f32::INFINITY).desired_rows(10));
                });
                ui.horizontal(|ui| {
                    if ui.button("Enregistrer").clicked() {
                        fini = Some(true);
                    }
                    if ui.button("Annuler").clicked() {
                        fini = Some(false);
                    }
                });
            });
            match fini {
                Some(true) if !texte.trim().is_empty() => {
                    if let Some(i) = h.items.iter().position(|x| x.kind == TEXTE && x.key == *cle) {
                        let x = &mut h.items[i];
                        x.text = texte.clone();
                        x.key = texte.clone();
                        x.rtf = None;
                        x.html = None;
                        // une autre entrée identique au nouveau texte : on garde celle-ci
                        let k = texte.clone();
                        let mut vu = false;
                        h.items.retain(|y| !(y.kind == TEXTE && y.key == k && std::mem::replace(&mut vu, true)));
                        change = true;
                    }
                    self.edition = None;
                }
                Some(_) => self.edition = None,
                None => {}
            }
        }

        if change {
            if let Err(e) = h.sauver() {
                self.statut = format!("Enregistrement impossible : {e}");
            }
        }
    }

    fn coller(&mut self, x: &Item, brut: bool) {
        let ok = if x.kind == IMAGE {
            image::open(self.p.images().join(format!("{}.png", x.key))).map(|i| {
                let i = i.to_rgba8();
                win::ecrire_dib(&rgba_dib(i.width(), i.height(), &i))
            }).unwrap_or(false)
        } else if brut {
            win::ecrire_texte(&x.text, None, None)
        } else {
            win::ecrire_texte(&x.text, x.rtf.as_deref(), x.html.as_deref())
        };
        if !ok {
            self.statut = "Copie impossible".into();
            return;
        }
        let cible = self.p.cible.lock().unwrap().take();
        match cible {
            Some(c) if self.coller_au_clic => {
                let hub = win::hub();
                std::thread::spawn(move || win::coller_dans(c, hub));
                self.statut.clear();
            }
            _ => self.statut = "Copié".into(),
        }
    }

    fn enregistrer(&mut self, x: &Item) {
        let (nom, ext) = if x.kind == IMAGE { ("image.png", "png") } else { ("texte.txt", "txt") };
        let Some(dest) = rfd::FileDialog::new().set_file_name(nom).add_filter(ext, &[ext]).save_file() else { return };
        let r = if x.kind == IMAGE { std::fs::copy(self.p.images().join(format!("{}.png", x.key)), &dest).map(|_| ()) } else { std::fs::write(&dest, &x.text) };
        self.statut = match r {
            Ok(()) => format!("Enregistré : {}", dest.display()),
            Err(e) => format!("Erreur : {e}"),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hist(nom: &str, max: usize) -> Historique {
        let d = std::env::temp_dir().join("outils_pp_tests");
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join(format!("{nom}.json"));
        let _ = std::fs::remove_file(&f);
        Historique::charger(&f, max, 0)
    }

    #[test]
    fn ajout_doublon_et_limite() {
        let mut h = hist("a", 3);
        h.ajouter(Item::texte("a"));
        h.ajouter(Item::texte("b"));
        assert_eq!(h.items[0].text, "b"); // ajout en tête
        h.items[1].pinned = true;
        h.items[1].source = "Notepad".into();
        h.ajouter(Item::texte("a"));
        assert_eq!(h.items.len(), 2);
        assert!(h.items[0].pinned, "le doublon garde l'épingle");
        assert_eq!(h.items[0].source, "Notepad", "et l'ancienne source");
        for t in ["c", "d", "e"] {
            h.ajouter(Item::texte(t));
        }
        assert_eq!(h.items.len(), 3);
        assert!(h.items.iter().any(|x| x.text == "a"), "épinglé jamais retiré");
        assert!(!h.items.iter().any(|x| x.text == "b"), "le plus vieux non épinglé part");
        h.max = 1;
        h.tailler();
        assert_eq!(h.items.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["e", "a"], "baisser max taille");
    }

    #[test]
    fn vue_recherche_et_effacer() {
        let mut h = hist("b", 10);
        for t in ["Bonjour", "https://exemple.fr", "autre"] {
            h.ajouter(Item::texte(t));
        }
        h.items[2].pinned = true; // Bonjour
        assert_eq!(h.items[h.vue("", 0)[0]].text, "Bonjour", "épinglés d'abord");
        assert_eq!(h.vue("BONJ", 0).len(), 1, "recherche sans casse");
        assert_eq!(h.vue("", 3).len(), 1);
        h.effacer_non_epingles();
        assert_eq!(h.items.len(), 1);
        assert!(h.items[0].pinned);
    }

    #[test]
    fn liens() {
        assert!(Item::texte(" https://a.b/c ").lien().is_some());
        assert!(Item::texte("http://x").lien().is_some());
        assert!(Item::texte("voir https://a.b").lien().is_none());
        assert!(Item::texte("ftp://a.b").lien().is_none());
        assert!(Item::texte("https://").lien().is_none());
    }

    #[test]
    fn recopie_simple_garde_rtf() {
        let mut h = hist("c", 10);
        h.ajouter(Item { rtf: Some("{\\rtf1}".into()), ..Item::texte("x") });
        h.ajouter(Item::texte("x"));
        assert_eq!(h.items[0].rtf.as_deref(), Some("{\\rtf1}"));
    }

    #[test]
    fn aller_retour_et_fichier_abime() {
        let mut h = hist("d", 10);
        h.ajouter(Item { source: "Claude".into(), ..Item::texte("été ✓") });
        h.items[0].pinned = true;
        h.sauver().unwrap();
        let h2 = Historique::charger(&h.fichier, 10, 0);
        assert_eq!(h2.items, h.items);
        // format C# : Kind en entier, champs PascalCase, Rtf absent
        let brut = std::fs::read_to_string(&h.fichier).unwrap();
        assert!(brut.contains("\"Kind\":0") && brut.contains("\"Pinned\":true") && !brut.contains("Rtf"));
        std::fs::write(&h.fichier, "{abîmé").unwrap();
        let h3 = Historique::charger(&h.fichier, 10, 0);
        assert!(h3.items.is_empty());
        assert!(h.fichier.with_extension("json.bak").exists());
    }

    #[test]
    fn age_max_sauf_epingles() {
        let mut h = hist("e", 10);
        h.max_jours = 2;
        let vieux = format_iso(maintenant_utc() - 5 * 86400, 3600);
        h.items = vec![Item::texte("neuf"), Item { time: vieux.clone(), ..Item::texte("vieux") }, Item { time: vieux, pinned: true, ..Item::texte("vieux épinglé") }];
        let r = h.tailler();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "vieux");
        assert_eq!(h.items.len(), 2);
    }

    #[test]
    fn dates() {
        let t = secondes("2026-11-16T21:34:26.9741181+01:00").unwrap();
        assert_eq!(t, secondes("2026-11-16T20:34:26Z").unwrap());
        assert_eq!(format_iso(t, 3600), "2026-11-16T21:34:26.0000000+01:00");
        assert_eq!(civil(jours(2024, 2, 29)), (2024, 2, 29));
        assert!(secondes(&maintenant()).unwrap().abs_diff(maintenant_utc()) < 5);
    }

    #[test]
    fn dib_aller_retour() {
        let px: Vec<u8> = (0..3 * 2 * 4).map(|i| (i * 10) as u8).collect();
        let (w, h, out) = dib_rgba(&rgba_dib(3, 2, &px)).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(out, px);
        assert_eq!(nom_raccourci(196694), "Ctrl+Maj+V");
    }
}
