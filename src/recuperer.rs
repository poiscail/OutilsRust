//! « Récupérer » un jeu gratuit en un clic : ouvre sa page directement dans le lanceur (Steam, Epic Games),
//! là où il ne reste qu'à cliquer sur « Obtenir » / « Ajouter au compte ».
//! (Récupérer à ta place demanderait ton mot de passe, et Epic interdit les robots : on ne le fait pas.)
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::sync::Mutex;
use std::time::Duration;

/// Page du magasin → adresse qui ouvre le lanceur directement sur le jeu.
pub fn lien_lanceur(magasin: &str) -> Option<String> {
    let m = magasin.split(['?', '#']).next()?.trim_end_matches('/');
    if m.contains("store.steampowered.com/app/") {
        let id: String = m.split("/app/").nth(1)?.chars().take_while(|c| c.is_ascii_digit()).collect();
        return (!id.is_empty()).then(|| format!("steam://store/{id}"));
    }
    if m.contains("epicgames.com") {
        for genre in ["/p/", "/bundles/", "/product/"] {
            if let Some(reste) = m.split(genre).nth(1) {
                let slug = reste.split('/').next()?;
                let g = if genre == "/bundles/" { "bundles" } else { "p" };
                return (!slug.is_empty()).then(|| format!("com.epicgames.launcher://store/{g}/{slug}"));
            }
        }
    }
    None
}

fn est_magasin(u: &str) -> bool {
    ["store.steampowered.com", "epicgames.com", "gog.com", "ubisoft.com", "ea.com", "humblebundle.com", "itch.io", "amazon.com", "microsoft.com", "xbox.com"].iter().any(|d| u.contains(d))
}

static LIENS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// Suit les redirections du lien du site d'annonces jusqu'à la vraie page du magasin.
pub fn page_magasin(url: &str) -> Option<String> {
    if let Some(m) = LIENS.lock().unwrap().get_or_insert_with(|| crate::outil::charger("liens_magasins")).get(url) {
        return Some(m.clone());
    }
    let a = ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(15)).user_agent("Mozilla/5.0 Outils").build();
    let mut u = url.to_string();
    for _ in 0..6 {
        if est_magasin(&u) {
            break;
        }
        let r = match a.get(&u).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(_) => return None,
        };
        match r.header("location") {
            Some(l) if l.starts_with("http") => u = l.to_string(),
            Some(l) => u = format!("{}{}", u.split('/').take(3).collect::<Vec<_>>().join("/"), l),
            None => break,
        }
    }
    if est_magasin(&u) {
        let mut g = LIENS.lock().unwrap();
        let m = g.get_or_insert_with(HashMap::new);
        m.insert(url.to_string(), u.clone());
        crate::outil::enregistrer("liens_magasins", &*m);
    }
    Some(u)
}

fn ouvrir(lien: &str) {
    let _ = std::process::Command::new("cmd").args(["/c", "start", "", lien]).creation_flags(0x0800_0000).spawn();
}

/// Ouvre le jeu dans son lanceur (ou sa page web si le magasin n'a pas de lanceur).
pub fn recuperer(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        let page = page_magasin(&url).unwrap_or_else(|| url.clone());
        match lien_lanceur(&page) {
            Some(l) => ouvrir(&l),
            None => ouvrir(&page),
        }
    });
}

/// Ouvre plusieurs jeux l'un après l'autre (le temps de cliquer « Obtenir » sur chacun).
pub fn recuperer_tous(urls: Vec<String>) {
    std::thread::spawn(move || {
        for (k, u) in urls.iter().enumerate() {
            if k > 0 {
                std::thread::sleep(Duration::from_secs(6));
            }
            recuperer(u);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liens() {
        assert_eq!(lien_lanceur("https://store.steampowered.com/app/1234560/Super_Jeu/?snr=1").as_deref(), Some("steam://store/1234560"));
        assert_eq!(lien_lanceur("https://store.epicgames.com/fr/p/hogwarts-legacy").as_deref(), Some("com.epicgames.launcher://store/p/hogwarts-legacy"));
        assert_eq!(lien_lanceur("https://www.epicgames.com/store/fr/product/celeste/home").as_deref(), Some("com.epicgames.launcher://store/p/celeste"));
        assert_eq!(lien_lanceur("https://store.epicgames.com/fr/bundles/pack-genial").as_deref(), Some("com.epicgames.launcher://store/bundles/pack-genial"));
        assert_eq!(lien_lanceur("https://www.gog.com/fr/game/witcher"), None);
        assert!(est_magasin("https://www.gog.com/x") && !est_magasin("https://www.gamerpower.com/open/x"));
    }
}
