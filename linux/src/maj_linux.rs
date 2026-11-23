//! Mise à jour automatique de la version Linux : cherche « Outils-linux-<version>.tar.gz » dans la
//! dernière publication GitHub, remplace le programme et relance Outils.
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEPOT: &str = "https://api.github.com/repos/poiscail/OutilsRust/releases/latest";

/// (version disponible, adresse de l'archive) si plus récente que celle-ci.
pub static DISPO: Mutex<Option<(String, String)>> = Mutex::new(None);
pub static ETAT: Mutex<String> = Mutex::new(String::new());

/// « Outils-linux-0.2.0.tar.gz » → « 0.2.0 ».
pub fn version_archive(nom: &str) -> Option<String> {
    nom.strip_prefix("Outils-linux-")?.strip_suffix(".tar.gz").map(String::from)
}

pub fn plus_recente(a: &str, b: &str) -> bool {
    let v = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    v(a) > v(b)
}

pub fn verifier() {
    std::thread::spawn(|| {
        let r = ureq::get(DEPOT).set("User-Agent", "Outils-linux").timeout(Duration::from_secs(20)).call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok());
        let Some(j) = r else {
            *ETAT.lock().unwrap() = "Impossible de vérifier les mises à jour.".into();
            return;
        };
        let trouve = j["assets"].as_array().into_iter().flatten().find_map(|a| {
            let v = version_archive(a["name"].as_str()?)?;
            Some((v, a["browser_download_url"].as_str()?.to_string()))
        });
        match trouve {
            Some((v, u)) if plus_recente(&v, VERSION) => {
                *ETAT.lock().unwrap() = format!("Nouvelle version {v} disponible.");
                *DISPO.lock().unwrap() = Some((v, u));
            }
            _ => *ETAT.lock().unwrap() = format!("Outils {VERSION} est à jour."),
        }
    });
}

/// Télécharge l'archive, remplace le programme en cours et relance Outils.
pub fn installer() {
    let Some((v, url)) = DISPO.lock().unwrap().clone() else { return };
    std::thread::spawn(move || {
        *ETAT.lock().unwrap() = format!("Téléchargement de la version {v}…");
        let res = (|| -> Result<PathBuf, String> {
            let r = ureq::get(&url).timeout(Duration::from_secs(600)).call().map_err(|e| e.to_string())?;
            let mut b = vec![];
            r.into_reader().read_to_end(&mut b).map_err(|e| e.to_string())?;
            let mut a = tar::Archive::new(flate2::read::GzDecoder::new(&b[..]));
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let neuf = exe.with_extension("nouveau");
            for e in a.entries().map_err(|e| e.to_string())? {
                let mut e = e.map_err(|e| e.to_string())?;
                let chemin = e.path().map_err(|e| e.to_string())?.to_path_buf();
                if chemin.file_name().is_some_and(|n| n == "outils") {
                    e.unpack(&neuf).map_err(|e| e.to_string())?;
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&neuf, std::fs::Permissions::from_mode(0o755));
                    std::fs::rename(&neuf, &exe).map_err(|e| e.to_string())?; // possible même pendant qu'il tourne
                    return Ok(exe);
                }
            }
            Err("programme absent de l'archive".into())
        })();
        match res {
            Ok(exe) => {
                *ETAT.lock().unwrap() = "Mise à jour installée : redémarrage…".into();
                let _ = std::process::Command::new(exe).spawn();
                std::process::exit(0);
            }
            Err(e) => *ETAT.lock().unwrap() = format!("✖ Mise à jour impossible : {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_archive("Outils-linux-0.2.0.tar.gz").as_deref(), Some("0.2.0"));
        assert_eq!(version_archive("Outils.exe"), None);
        assert!(plus_recente("0.2.0", "0.1.9") && plus_recente("1.0.0", "0.9.9") && !plus_recente("0.1.0", "0.1.0"));
    }
}
