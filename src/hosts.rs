//! Blocage de sites par le fichier hosts de Windows (droits admin). Chaque bloc d'Outils est marqué
//! « # outils:<marque> » pour pouvoir l'enlever sans toucher au reste du fichier.
use std::path::PathBuf;

pub fn chemin() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or("C:\\Windows".into())).join(r"System32\drivers\etc\hosts")
}

/// Nom de domaine propre à partir de ce que l'utilisateur tape (adresse complète, www…).
pub fn domaine(s: &str) -> Option<String> {
    let s = s.trim().to_lowercase();
    let s = s.split("://").last()?.split(['/', '?', '#', ':']).next()?.trim_start_matches("www.").to_string();
    (s.contains('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')).then_some(s)
}

/// Contenu du fichier avec le bloc `marque` remplacé par ces domaines (vide = bloc retiré).
pub fn avec_bloc(contenu: &str, marque: &str, domaines: &[String]) -> String {
    let etiquette = format!("# outils:{marque}");
    let mut lignes: Vec<String> = contenu.lines().filter(|l| !l.trim_end().ends_with(&etiquette)).map(str::to_string).collect();
    while lignes.last().is_some_and(|l| l.trim().is_empty()) {
        lignes.pop();
    }
    for d in domaines.iter().filter_map(|d| domaine(d)) {
        for h in [d.clone(), format!("www.{d}")] {
            lignes.push(format!("0.0.0.0 {h} {etiquette}"));
        }
    }
    lignes.join("\r\n") + "\r\n"
}

pub fn appliquer(marque: &str, domaines: &[String]) -> Result<(), String> {
    let f = chemin();
    let contenu = std::fs::read_to_string(&f).unwrap_or_default();
    let nouveau = avec_bloc(&contenu, marque, domaines);
    if nouveau == contenu {
        return Ok(());
    }
    std::fs::write(&f, nouveau).map_err(|e| format!("fichier hosts non modifiable (lance Outils en administrateur) : {e}"))?;
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("ipconfig").arg("/flushdns").creation_flags(0x0800_0000).status();
    Ok(())
}

/// Domaines actuellement bloqués par ce bloc.
pub fn bloques(marque: &str) -> Vec<String> {
    let etiquette = format!("# outils:{marque}");
    std::fs::read_to_string(chemin()).unwrap_or_default().lines().filter(|l| l.trim_end().ends_with(&etiquette))
        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string)).filter(|d| !d.starts_with("www.")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocs() {
        assert_eq!(domaine("https://www.YouTube.com/watch?v=1"), Some("youtube.com".into()));
        assert_eq!(domaine("pas un site"), None);
        let base = "127.0.0.1 localhost\r\n";
        let a = avec_bloc(base, "focus", &["youtube.com".into()]);
        assert!(a.contains("0.0.0.0 youtube.com # outils:focus") && a.contains("0.0.0.0 www.youtube.com # outils:focus"));
        assert!(a.starts_with("127.0.0.1 localhost"));
        let b = avec_bloc(&a, "focus", &[]);
        assert_eq!(b, base);
        let c = avec_bloc(&a, "pub", &["ads.com".into()]);
        assert!(c.contains("youtube.com") && c.contains("ads.com"));
    }
}
