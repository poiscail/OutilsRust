//! Intégration Windows du hub : notifications, démarrage avec Windows, icône dans la zone de notification,
//! raccourci global Ctrl+Alt+O, fond d'écran du bureau, disques et mise à jour depuis GitHub.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use eframe::egui;
use std::io::Read as _;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DEPOT: &str = "https://api.github.com/repos/poiscail/OutilsRust/releases/latest";

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------- notifications ----------

pub static NOTIFS: AtomicBool = AtomicBool::new(false);

fn script_toast(titre: &str, texte: &str) -> String {
    // XML dans une chaîne PowerShell entre apostrophes
    let x = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "''");
    format!(
        concat!(
            "[Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]|Out-Null;",
            "[Windows.Data.Xml.Dom.XmlDocument,Windows.Data.Xml.Dom,ContentType=WindowsRuntime]|Out-Null;",
            "$x=New-Object Windows.Data.Xml.Dom.XmlDocument;",
            "$x.LoadXml('<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>');",
            "[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($x))"
        ),
        x(titre),
        x(texte)
    )
}

/// Notification Windows, si l'option est cochée. Ne bloque pas.
pub fn notifier(titre: &str, texte: &str) {
    crate::telephone::pousser_notif(titre, texte);
    crate::coeur::noter(titre, texte);
    crate::interface::pousser(titre, texte);
    if NOTIFS.load(Relaxed) {
        envoyer_toast(titre, texte);
    }
}

/// Notification Windows même si l'option est décochée (rappels demandés explicitement).
pub fn notifier_toujours(titre: &str, texte: &str) {
    crate::telephone::pousser_notif(titre, texte);
    crate::coeur::noter(titre, texte);
    crate::interface::pousser(titre, texte);
    envoyer_toast(titre, texte);
}

fn envoyer_toast(titre: &str, texte: &str) {
    let utf16: Vec<u8> = script_toast(titre, texte).encode_utf16().flat_map(u16::to_le_bytes).collect();
    let _ = Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &B64.encode(utf16)])
        .creation_flags(CREATE_NO_WINDOW).spawn();
}

// ---------- démarrage avec Windows, fond d'écran, disques ----------

pub fn demarrage(actif: bool) {
    const RUN: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let mut c = Command::new("reg.exe");
    if actif {
        let exe = std::env::current_exe().unwrap_or_default();
        c.args(["add", RUN, "/v", "Outils", "/t", "REG_SZ", "/f", "/d"]).arg(format!("\"{}\" --demarrage", exe.display()));
    } else {
        c.args(["delete", RUN, "/v", "Outils", "/f"]);
    }
    let _ = c.creation_flags(CREATE_NO_WINDOW).status();
}

pub fn fond_bureau(p: &Path) {
    let mut v: Vec<u16> = p.as_os_str().encode_wide().chain([0]).collect();
    std::thread::spawn(move || unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, v.as_mut_ptr().cast(), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) });
}

/// (lecteur, libre, total) des disques fixes.
pub fn disques() -> Vec<(String, u64, u64)> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
    let m = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| m & (1 << i) != 0)
        .filter_map(|i| {
            let d = format!("{}:\\", (b'A' + i) as char);
            let dw = w(&d);
            if unsafe { GetDriveTypeW(dw.as_ptr()) } != 3 {
                return None; // DRIVE_FIXED seulement
            }
            let (mut a, mut t, mut l) = (0u64, 0u64, 0u64);
            (unsafe { GetDiskFreeSpaceExW(dw.as_ptr(), &mut a, &mut t, &mut l) } != 0).then_some((d, l, t))
        })
        .collect()
}

// ---------- zone de notification + raccourci global (fil à part, sa propre boucle de messages) ----------

static FENETRE: AtomicIsize = AtomicIsize::new(0);
static ZONE: AtomicBool = AtomicBool::new(false);
static RACCOURCI: AtomicBool = AtomicBool::new(false);
static ICONE_MISE: AtomicBool = AtomicBool::new(false);
static RACC_MIS: AtomicBool = AtomicBool::new(false);
pub static QUITTER: AtomicBool = AtomicBool::new(false);
static CTX: OnceLock<egui::Context> = OnceLock::new();
const WM_ICONE: u32 = WM_APP + 1;
const WM_SYNC: u32 = WM_APP + 2;

/// Fenêtre principale du hub (titre "Outils", notre processus).
fn hub() -> HWND {
    let h = unsafe { FindWindowW(null(), w("Outils").as_ptr()) };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, &mut pid) };
    if pid == std::process::id() { h } else { null_mut() }
}

pub fn montrer() {
    let h = hub();
    if h.is_null() {
        return;
    }
    unsafe {
        ShowWindow(h, SW_SHOW);
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h);
    }
    if let Some(c) = CTX.get() {
        c.request_repaint();
    }
}

pub fn cacher() {
    unsafe { ShowWindow(hub(), SW_HIDE) };
}

pub fn zone_active() -> bool {
    ICONE_MISE.load(Relaxed)
}

/// Vrai si le raccourci est voulu mais pris par une autre appli.
pub fn raccourci_pris() -> bool {
    RACCOURCI.load(Relaxed) && !RACC_MIS.load(Relaxed) && FENETRE.load(Relaxed) != 0
}

pub fn regler(zone: bool, raccourci: bool) {
    ZONE.store(zone, Relaxed);
    RACCOURCI.store(raccourci, Relaxed);
    let h = FENETRE.load(Relaxed);
    if h != 0 {
        unsafe { PostMessageW(h as HWND, WM_SYNC, 0, 0) };
    }
}

fn donnees_icone(h: HWND) -> NOTIFYICONDATAW {
    let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    d.hWnd = h;
    d.uID = 1;
    d
}

unsafe fn synchroniser(h: HWND) {
    let zone = ZONE.load(Relaxed);
    if zone != ICONE_MISE.load(Relaxed) {
        let mut d = donnees_icone(h);
        d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        d.uCallbackMessage = WM_ICONE;
        d.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        for (i, c) in "Outils".encode_utf16().enumerate() {
            d.szTip[i] = c;
        }
        if Shell_NotifyIconW(if zone { NIM_ADD } else { NIM_DELETE }, &d) != 0 {
            ICONE_MISE.store(zone, Relaxed);
        }
    }
    let r = RACCOURCI.load(Relaxed);
    if r != RACC_MIS.load(Relaxed) {
        let ok = if r { crate::outil::raccourci(h, 1, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'O' as u32) } else { UnregisterHotKey(h, 1) };
        if ok != 0 {
            RACC_MIS.store(r, Relaxed);
        }
    }
}

/// Raccourci : montre le hub, ou le range s'il est déjà devant.
unsafe fn basculer() {
    let h = hub();
    if !h.is_null() && GetForegroundWindow() == h && IsWindowVisible(h) != 0 {
        if zone_active() {
            cacher();
        } else {
            ShowWindow(h, SW_MINIMIZE);
        }
    } else {
        montrer();
    }
}

unsafe fn menu(h: HWND) {
    let m = CreatePopupMenu();
    AppendMenuW(m, MF_STRING, 1, w("Ouvrir Outils").as_ptr());
    AppendMenuW(m, MF_STRING, 2, w("Quitter").as_ptr());
    let mut p = POINT { x: 0, y: 0 };
    GetCursorPos(&mut p);
    SetForegroundWindow(h); // sinon le menu ne se ferme pas en cliquant ailleurs
    let choix = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, h, null());
    DestroyMenu(m);
    match choix {
        1 => montrer(),
        2 => {
            QUITTER.store(true, Relaxed);
            montrer(); // le hub caché ne repeint pas : on le montre pour qu'il se ferme
        }
        _ => {}
    }
}

unsafe extern "system" fn procedure(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_SYNC => synchroniser(h),
        WM_HOTKEY => basculer(),
        WM_ICONE => match lp as u32 & 0xFFFF {
            WM_LBUTTONUP => montrer(),
            WM_RBUTTONUP => menu(h),
            _ => {}
        },
        _ => return DefWindowProcW(h, m, wp, lp),
    }
    0
}

pub fn lancer(ctx: egui::Context) {
    let _ = CTX.set(ctx);
    std::thread::spawn(|| unsafe {
        let classe = w("OutilsZone");
        let inst = GetModuleHandleW(null());
        let wc = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: inst, lpszClassName: classe.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let h = CreateWindowExW(0, classe.as_ptr(), classe.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), inst, null());
        if h.is_null() {
            return;
        }
        FENETRE.store(h as isize, Relaxed);
        synchroniser(h);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Retire l'icône avant de quitter (sinon elle reste jusqu'au survol).
pub fn fin() {
    let h = FENETRE.load(Relaxed);
    if h != 0 && ICONE_MISE.load(Relaxed) {
        unsafe { Shell_NotifyIconW(NIM_DELETE, &donnees_icone(h as HWND)) };
    }
}

// ---------- mise à jour depuis les releases GitHub ----------

#[derive(Default)]
pub struct Maj {
    pub dispo: Option<(String, String)>, // (version, lien de Outils.exe)
    pub etat: String,
    pub notes: String, // texte de la release GitHub
}
pub type MajPartagee = Arc<Mutex<Maj>>;

pub fn plus_recente(tag: &str, actuelle: &str) -> bool {
    let v = |s: &str| s.trim().trim_start_matches('v').split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    v(tag) > v(actuelle)
}

pub fn verifier(m: MajPartagee) {
    m.lock().unwrap().etat = "Recherche de mise à jour…".into();
    std::thread::spawn(move || {
        let j = ureq::get(DEPOT).set("User-Agent", "Outils").call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok());
        let mut m = m.lock().unwrap();
        let Some(j) = j else {
            m.etat = "Impossible de joindre GitHub.".into();
            return;
        };
        let tag = j["tag_name"].as_str().unwrap_or_default().to_string();
        let lien = j["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == "Outils.exe"))
            .and_then(|x| x["browser_download_url"].as_str()).map(String::from);
        match lien {
            Some(l) if plus_recente(&tag, env!("CARGO_PKG_VERSION")) => {
                m.etat = format!("Nouvelle version {tag} disponible.");
                m.notes = j["body"].as_str().unwrap_or_default().to_string();
                m.dispo = Some((tag, l));
            }
            _ => m.etat = format!("À jour (v{}).", env!("CARGO_PKG_VERSION")),
        }
    });
}

/// Télécharge la nouvelle version, remplace l'exe (l'ancien devient Outils.old.exe) et relance.
pub fn installer(m: MajPartagee) {
    let Some((_, lien)) = m.lock().unwrap().dispo.clone() else { return };
    m.lock().unwrap().etat = "Téléchargement…".into();
    std::thread::spawn(move || {
        let r = (|| -> Result<(), String> {
            let mut data = Vec::new();
            ureq::get(&lien).set("User-Agent", "Outils").call().map_err(|e| e.to_string())?
                .into_reader().read_to_end(&mut data).map_err(|e| e.to_string())?;
            if data.len() < 100_000 || !data.starts_with(b"MZ") {
                return Err("fichier reçu invalide".into());
            }
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let vieux = exe.with_extension("old.exe");
            let _ = std::fs::remove_file(&vieux);
            std::fs::rename(&exe, &vieux).map_err(|e| e.to_string())?; // un exe lancé peut être renommé, pas écrasé
            if let Err(e) = std::fs::write(&exe, &data) {
                let _ = std::fs::rename(&vieux, &exe);
                return Err(e.to_string());
            }
            Command::new(&exe).spawn().map_err(|e| e.to_string())?;
            fin();
            std::process::exit(0);
        })();
        if let Err(e) = r {
            m.lock().unwrap().etat = format!("Échec de la mise à jour : {e}");
        }
    });
}

pub fn nettoyer_ancienne() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old.exe"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(plus_recente("v0.2.0", "0.1.9"));
        assert!(plus_recente("v0.10.0", "0.9.0"));
        assert!(!plus_recente("v0.2.0", "0.2.0"));
        assert!(!plus_recente("", "0.1.0"));
    }

    #[test]
    fn toast_echappe() {
        let s = script_toast("Tri", "l'image <a&b>");
        assert!(s.contains("l''image &lt;a&amp;b&gt;"));
        assert!(s.contains("{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}"));
    }
}
