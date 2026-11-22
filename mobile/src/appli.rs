//! L'appli elle-même : mises à jour depuis GitHub (5) et sauvegarde / restauration des réglages (7).
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEPOT: &str = "https://api.github.com/repos/poiscail/OutilsRust/releases?per_page=10";

/// Nouvelle version trouvée : (version, adresse de l'APK).
pub static MAJ: Mutex<Option<(String, String)>> = Mutex::new(None);

pub fn plus_recente(a: &str, b: &str) -> bool {
    let v = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    v(a) > v(b)
}

/// Version écrite dans le nom de l'APK : « Outils-0.4.0.apk » ou « Outils-android-0.4.0.apk ».
pub fn version_apk(nom: &str) -> Option<String> {
    let n = nom.strip_suffix(".apk")?;
    let v = n.rsplit('-').next()?;
    (v.split('.').count() == 3 && v.split('.').all(|x| x.parse::<u32>().is_ok())).then(|| v.to_string())
}

/// Cherche dans les dernières versions publiées sur GitHub un APK plus récent que l'appli.
pub fn verifier() -> Result<Option<(String, String)>, String> {
    let j: serde_json::Value = ureq::get(DEPOT).set("User-Agent", "Outils-android").timeout(Duration::from_secs(15)).call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let mut meilleure: Option<(String, String)> = None;
    for r in j.as_array().into_iter().flatten() {
        for a in r["assets"].as_array().into_iter().flatten() {
            let Some(v) = a["name"].as_str().and_then(version_apk) else { continue };
            if plus_recente(&v, VERSION) && meilleure.as_ref().is_none_or(|m| plus_recente(&v, &m.0)) {
                meilleure = Some((v, a["browser_download_url"].as_str().unwrap_or("").to_string()));
            }
        }
    }
    *MAJ.lock().unwrap() = meilleure.clone();
    Ok(meilleure)
}

/// Télécharge la mise à jour puis ouvre l'installeur d'Android.
pub fn installer(url: &str) -> Result<(), String> {
    let r = ureq::get(url).set("User-Agent", "Outils-android").timeout(Duration::from_secs(600)).call().map_err(|e| e.to_string())?;
    let dest = crate::android::dossier_partage().join("maj.apk");
    let mut f = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
    std::io::copy(&mut r.into_reader(), &mut f).map_err(|e| e.to_string())?;
    crate::android::installer_apk("maj.apk");
    Ok(())
}

/// Tous les réglages et records de l'appli dans un seul fichier JSON.
pub fn sauvegarde_json(dossier: &Path) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for e in std::fs::read_dir(dossier).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") {
            if let (Some(n), Ok(t)) = (p.file_stem(), std::fs::read_to_string(&p)) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                    m.insert(n.to_string_lossy().to_string(), v);
                }
            }
        }
    }
    serde_json::json!({ "outils_android": VERSION, "fichiers": m })
}

pub fn dossier_donnees() -> PathBuf {
    crate::outil::chemin("x").parent().map(Path::to_path_buf).unwrap_or_else(|| ".".into())
}

pub fn sauvegarder() -> Result<PathBuf, String> {
    let j = sauvegarde_json(&dossier_donnees());
    let dest = crate::commun::dossier_outils().join(format!("outils-sauvegarde-{}.json", &crate::minecraft::horodatage()[..8]));
    std::fs::write(&dest, serde_json::to_string_pretty(&j).unwrap_or_default()).map_err(|e| e.to_string())?;
    crate::android::signaler_media(&[dest.display().to_string()]);
    Ok(dest)
}

/// Remet les fichiers d'une sauvegarde. Renvoie le nombre de fichiers restaurés.
pub fn restaurer_json(j: &serde_json::Value, dossier: &Path) -> Result<usize, String> {
    let f = j["fichiers"].as_object().ok_or("ce n'est pas une sauvegarde d'Outils")?;
    let mut n = 0;
    for (nom, v) in f {
        // noms simples seulement (pas de chemin)
        if nom.is_empty() || nom.contains(['/', '\\', '.']) {
            continue;
        }
        std::fs::write(dossier.join(format!("{nom}.json")), serde_json::to_string_pretty(v).unwrap_or_default()).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

pub fn sauvegardes() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(crate::commun::dossier_outils()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("outils-sauvegarde-"))).collect();
    v.sort();
    v.reverse();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_et_sauvegardes() {
        assert_eq!(version_apk("Outils-0.4.0.apk").as_deref(), Some("0.4.0"));
        assert_eq!(version_apk("Outils-android-1.2.3.apk").as_deref(), Some("1.2.3"));
        assert_eq!(version_apk("Outils.apk"), None);
        assert_eq!(version_apk("Outils.exe"), None);
        assert!(plus_recente("0.10.0", "0.9.9"));
        assert!(!plus_recente("0.3.0", "0.3.0"));
        let d = std::env::temp_dir().join(format!("outils_test_sauv_{}", crate::outil::hasard(1 << 30)));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("badges.json"), r#"{"l":{"pc":1}}"#).unwrap();
        std::fs::write(d.join("pas_json.txt"), "x").unwrap();
        let j = sauvegarde_json(&d);
        assert_eq!(j["fichiers"]["badges"]["l"]["pc"], 1);
        let d2 = d.join("restau");
        std::fs::create_dir_all(&d2).unwrap();
        let mut j2 = j.clone();
        j2["fichiers"]["../evil"] = serde_json::json!(1);
        assert_eq!(restaurer_json(&j2, &d2).unwrap(), 1);
        assert!(d2.join("badges.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
