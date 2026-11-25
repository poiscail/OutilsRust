//! Webcam virtuelle (32) : la caméra du téléphone devient une vraie webcam pour Windows
//! (« DirectShow Softcam ») que Discord, Zoom, Teams ou OBS peuvent choisir.
//! Utilise Softcam (MIT, tshino/softcam), téléchargé et enregistré une seule fois.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const LARGEUR: u32 = 1280;
pub const HAUTEUR: u32 = 720;

pub static ETAT: Mutex<String> = Mutex::new(String::new());
pub static ACTIVE: AtomicBool = AtomicBool::new(false);

fn dossier() -> PathBuf {
    crate::outil::racine().join("softcam")
}

/// softcam.dll en 64 bits (l'archive contient aussi une version 32 bits).
fn trouver_dll(d: &Path) -> Option<PathBuf> {
    let mut trouves = Vec::new();
    let mut pile = vec![d.to_path_buf()];
    while let Some(p) = pile.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let q = e.path();
            if q.is_dir() {
                pile.push(q);
            } else if q.file_name().is_some_and(|n| n.eq_ignore_ascii_case("softcam.dll")) {
                trouves.push(q);
            }
        }
    }
    trouves.sort_by_key(|p| !p.to_string_lossy().to_lowercase().contains("x64"));
    trouves.into_iter().next()
}

/// Télécharge Softcam et l'enregistre dans Windows (demande l'autorisation administrateur une fois).
pub fn installer(msg: &Arc<Mutex<String>>) -> Result<PathBuf, String> {
    let d = dossier();
    if let Some(p) = trouver_dll(&d).filter(|_| d.join("enregistre").exists()) {
        return Ok(p);
    }
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let dll = match trouver_dll(&d) {
        Some(p) => p,
        None => {
            *msg.lock().unwrap() = "Recherche de Softcam…".into();
            let j: serde_json::Value = ureq::get("https://api.github.com/repos/tshino/softcam/releases/latest").set("User-Agent", "Outils").call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
            let url = j["assets"].as_array().into_iter().flatten().filter_map(|a| Some((a["name"].as_str()?.to_lowercase(), a["browser_download_url"].as_str()?.to_string()))).find(|(n, _)| n.ends_with(".zip")).map(|x| x.1).ok_or("Softcam introuvable sur GitHub")?;
            let z = d.join("softcam.zip");
            crate::createur::telecharger(&url, &z, msg, "la webcam virtuelle")?;
            let mut a = zip::ZipArchive::new(std::fs::File::open(&z).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            a.extract(&d).map_err(|e| e.to_string())?;
            let _ = std::fs::remove_file(&z);
            trouver_dll(&d).ok_or("softcam.dll absent de l'archive")?
        }
    };
    *msg.lock().unwrap() = "Enregistrement de la webcam virtuelle (Windows demande l'autorisation)…".into();
    let script = format!("$p = Start-Process -FilePath regsvr32.exe -ArgumentList '/s','\"{}\"' -Verb RunAs -Wait -PassThru; exit $p.ExitCode", dll.display());
    crate::optimiseur::ps(&script).map_err(|e| format!("enregistrement refusé : {e}"))?;
    let _ = std::fs::write(d.join("enregistre"), "1");
    Ok(dll)
}

type Creer = unsafe extern "C" fn(i32, i32, f32) -> *mut core::ffi::c_void;
type Envoyer = unsafe extern "C" fn(*mut core::ffi::c_void, *const core::ffi::c_void);
type Supprimer = unsafe extern "C" fn(*mut core::ffi::c_void);

/// Image du téléphone en 1280×720 (bandes noires si besoin), pixels BGR de haut en bas.
pub fn cadre_bgr(img: &image::RgbaImage) -> Vec<u8> {
    let e = (LARGEUR as f32 / img.width().max(1) as f32).min(HAUTEUR as f32 / img.height().max(1) as f32);
    let (l, h) = (((img.width() as f32 * e) as u32).max(1), ((img.height() as f32 * e) as u32).max(1));
    let r = image::imageops::resize(img, l, h, image::imageops::FilterType::Triangle);
    let (ox, oy) = ((LARGEUR - l) / 2, (HAUTEUR - h) / 2);
    let mut v = vec![0u8; (LARGEUR * HAUTEUR * 3) as usize];
    for y in 0..h {
        for x in 0..l {
            let p = r.get_pixel(x, y);
            let i = (((oy + y) * LARGEUR + ox + x) * 3) as usize;
            v[i] = p[2];
            v[i + 1] = p[1];
            v[i + 2] = p[0];
        }
    }
    v
}

/// Lance (ou arrête) l'envoi des images du téléphone vers la webcam virtuelle.
pub fn activer(oui: bool) {
    if !oui {
        ACTIVE.store(false, Relaxed);
        return;
    }
    if ACTIVE.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        let msg: Arc<Mutex<String>> = Arc::default();
        let r = (|| -> Result<(), String> {
            let dll = installer(&msg).inspect_err(|e| *ETAT.lock().unwrap() = e.clone())?;
            use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            let nom: Vec<u16> = dll.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
            let h = unsafe { LoadLibraryW(nom.as_ptr()) };
            if h.is_null() {
                return Err("softcam.dll ne se charge pas".into());
            }
            let f = |n: &[u8]| unsafe { GetProcAddress(h, n.as_ptr()) };
            let (Some(c), Some(e), Some(s)) = (f(b"scCreateCamera\0"), f(b"scSendFrame\0"), f(b"scDeleteCamera\0")) else {
                return Err("softcam.dll incomplet".into());
            };
            let (creer, envoyer, supprimer): (Creer, Envoyer, Supprimer) = unsafe { (std::mem::transmute(c), std::mem::transmute(e), std::mem::transmute(s)) };
            let cam = unsafe { creer(LARGEUR as i32, HAUTEUR as i32, 30.0) };
            if cam.is_null() {
                return Err("la webcam virtuelle est déjà utilisée par un autre programme".into());
            }
            *ETAT.lock().unwrap() = "📹 Webcam virtuelle active : choisis « DirectShow Softcam » comme caméra.".into();
            let mut cadre = vec![0u8; (LARGEUR * HAUTEUR * 3) as usize];
            let mut derniere: Option<Instant> = None;
            while ACTIVE.load(Relaxed) {
                let image = crate::pont::WEBCAM.lock().unwrap().clone();
                if let Some((jpeg, quand)) = image {
                    if derniere != Some(quand) {
                        derniere = Some(quand);
                        if let Ok(i) = image::load_from_memory(&jpeg) {
                            cadre = cadre_bgr(&i.to_rgba8());
                        }
                    }
                }
                unsafe { envoyer(cam, cadre.as_ptr() as *const _) };
                std::thread::sleep(Duration::from_millis(33));
            }
            unsafe { supprimer(cam) };
            Ok(())
        })();
        if let Err(e) = r {
            *ETAT.lock().unwrap() = format!("✖ {e}");
        } else {
            ETAT.lock().unwrap().clear();
        }
        ACTIVE.store(false, Relaxed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadre() {
        // image portrait : bandes noires à gauche et à droite, pixels en BGR
        let i = image::RgbaImage::from_pixel(9, 16, image::Rgba([10, 20, 30, 255]));
        let v = cadre_bgr(&i);
        assert_eq!(v.len(), (LARGEUR * HAUTEUR * 3) as usize);
        let centre = ((HAUTEUR / 2 * LARGEUR + LARGEUR / 2) * 3) as usize;
        assert_eq!(&v[centre..centre + 3], &[30, 20, 10]);
        assert_eq!(&v[..3], &[0, 0, 0]);
    }
}
