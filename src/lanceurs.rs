//! Les autres magasins pour le lanceur de jeux : GOG, Riot, Ubisoft, EA, Battle.net, Xbox (Game Pass),
//! et les jaquettes des jeux qui n'en ont pas (cherchées sur le magasin Steam, gardées en cache).
use crate::lanceur::Jeu;
use crate::optimiseur::{reg_sous_cles, reg_texte};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

fn jeu(id: String, nom: String, cmd: String, args: String, dir: String) -> Jeu {
    Jeu { id, nom, cmd, args, dir, cover: None, steam: false }
}

/// « "C:\Jeux\x.exe",0 » → « C:\Jeux\x.exe »
pub fn exe_icone(icone: &str) -> Option<String> {
    let t = icone.trim().trim_matches('"');
    let t = match t.rfind(',') {
        Some(k) if t[k + 1..].trim().parse::<i32>().is_ok() => &t[..k],
        _ => t,
    };
    let t = t.trim().trim_matches('"');
    t.to_lowercase().ends_with(".exe").then(|| t.to_string())
}

pub fn scan_gog() -> Vec<Jeu> {
    let base = r"HKLM\SOFTWARE\WOW6432Node\GOG.com\Games";
    let mut v = vec![];
    for k in reg_sous_cles(base) {
        let p = format!(r"{base}\{k}");
        let t = |n: &str| reg_texte(&p, n).filter(|s| !s.trim().is_empty());
        if let (Some(nom), Some(exe)) = (t("gameName"), t("exe")) {
            if Path::new(&exe).is_file() {
                v.push(jeu(format!("gog:{k}"), nom, exe, t("launchParam").unwrap_or_default(), t("path").unwrap_or_default()));
            }
        }
    }
    v
}

/// Ligne « product_install_full_path: "C:/Riot Games/VALORANT/live" » d'un fichier de réglages Riot.
pub fn chemin_riot(yaml: &str) -> Option<String> {
    yaml.lines().find_map(|l| l.trim().strip_prefix("product_install_full_path:")).map(|s| s.trim().trim_matches('"').replace('/', "\\"))
}

pub fn nom_riot(produit: &str) -> Option<&'static str> {
    Some(match produit {
        "valorant" => "VALORANT",
        "league_of_legends" => "League of Legends",
        "bacon" => "Legends of Runeterra",
        "lion" => "2XKO",
        _ => return None,
    })
}

pub fn scan_riot() -> Vec<Jeu> {
    let pd = PathBuf::from(std::env::var("ProgramData").unwrap_or(r"C:\ProgramData".into())).join("Riot Games");
    let Some(client) = std::fs::read_to_string(pd.join("RiotClientInstalls.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|j| j["rc_default"].as_str().or(j["rc_live"].as_str()).map(|s| s.replace('/', "\\")))
    else {
        return vec![];
    };
    let mut v = vec![];
    for e in std::fs::read_dir(pd.join("Metadata")).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        let Some((produit, ligne)) = n.split_once('.') else { continue };
        let Some(nom) = nom_riot(produit) else { continue };
        let yaml = std::fs::read_to_string(e.path().join(format!("{n}.product_settings.yaml"))).unwrap_or_default();
        let dir = chemin_riot(&yaml).unwrap_or_default();
        if !dir.is_empty() && !Path::new(&dir).is_dir() {
            continue;
        }
        v.push(jeu(format!("riot:{produit}"), nom.into(), client.clone(), format!("--launch-product={produit} --launch-patchline={ligne}"), dir));
    }
    v
}

pub fn scan_ubisoft() -> Vec<Jeu> {
    let base = r"HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    let mut v = vec![];
    for k in reg_sous_cles(base) {
        let Some(dir) = reg_texte(&format!(r"{base}\{k}"), "InstallDir").filter(|d| Path::new(d).is_dir()) else { continue };
        let dir = dir.replace('/', "\\");
        let nom = Path::new(dir.trim_end_matches('\\')).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| format!("Ubisoft {k}"));
        v.push(jeu(format!("ubi:{k}"), nom, format!("uplay://launch/{k}/0"), String::new(), dir));
    }
    v
}

/// Jeux EA et Battle.net : d'après la liste des programmes installés (éditeur), lancés par leur exe.
pub fn scan_editeurs() -> Vec<Jeu> {
    let mut v = vec![];
    let lanceurs = ["battle.net", "ea app", "origin", "ubisoft connect", "launcher", "redistributable", "directx", "anticheat", "anti-cheat", "uninstall"];
    for base in [r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"] {
        for k in reg_sous_cles(base) {
            let p = format!(r"{base}\{k}");
            let t = |n: &str| reg_texte(&p, n).filter(|s| !s.trim().is_empty());
            let (Some(nom), Some(editeur)) = (t("DisplayName"), t("Publisher")) else { continue };
            let e = editeur.to_lowercase();
            let (prefixe, ok) = if e.contains("blizzard") {
                ("bnet", true)
            } else if e.contains("electronic arts") {
                ("ea", true)
            } else {
                ("", false)
            };
            if !ok || lanceurs.iter().any(|l| nom.to_lowercase().contains(l)) {
                continue;
            }
            let Some(exe) = t("DisplayIcon").and_then(|i| exe_icone(&i)).filter(|x| Path::new(x).is_file()) else { continue };
            let dir = t("InstallLocation").unwrap_or_else(|| Path::new(&exe).parent().map(|d| d.display().to_string()).unwrap_or_default());
            v.push(jeu(format!("{prefixe}:{k}"), nom, exe, String::new(), dir));
        }
    }
    v
}

// ---------- Xbox / Game Pass ----------

/// Nom affiché et identifiant de l'appli dans MicrosoftGame.config.
pub fn lire_config_xbox(xml: &str) -> (Option<String>, Option<String>) {
    let attr = |balise: &str, nom: &str| -> Option<String> {
        let i = xml.find(&format!("<{balise} "))?;
        let bloc = &xml[i..xml[i..].find('>').map(|f| i + f).unwrap_or(xml.len())];
        let j = bloc.find(&format!("{nom}=\""))? + nom.len() + 2;
        Some(bloc[j..j + bloc[j..].find('"')?].to_string())
    };
    (attr("ShellVisuals", "DefaultDisplayName").filter(|n| !n.starts_with("ms-resource")), attr("Executable", "Id"))
}

static XBOX_EN_COURS: AtomicBool = AtomicBool::new(false);
/// Vrai quand une recherche en fond vient de finir : le lanceur relit la liste.
pub static NOUVEAUX: AtomicBool = AtomicBool::new(false);

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct JeuXbox {
    nom: String,
    famille: String,
    appli: String,
    dossier: String,
}

/// Jeux Xbox déjà trouvés (la recherche, lente, se fait en fond et remplit le cache).
pub fn scan_xbox() -> Vec<Jeu> {
    let l: Vec<JeuXbox> = crate::outil::charger("jeux_xbox");
    if !XBOX_EN_COURS.swap(true, Relaxed) {
        std::thread::spawn(chercher_xbox);
    }
    l.into_iter().map(|x| jeu(format!("xbox:{}", x.famille), x.nom, "explorer.exe".into(), format!(r"shell:AppsFolder\{}!{}", x.famille, x.appli), x.dossier)).collect()
}

fn chercher_xbox() {
    let script = "Get-AppxPackage | Where-Object { -not $_.IsFramework -and $_.InstallLocation -and (Test-Path (Join-Path $_.InstallLocation 'MicrosoftGame.config')) } | ForEach-Object { $_.PackageFamilyName + '|' + $_.InstallLocation }";
    let Ok(o) = std::process::Command::new("powershell").args(["-NoProfile", "-Command", script]).creation_flags(0x0800_0000).output() else { return };
    let mut v = vec![];
    for l in String::from_utf8_lossy(&o.stdout).lines() {
        let Some((famille, dossier)) = l.trim().split_once('|') else { continue };
        let xml = std::fs::read_to_string(Path::new(dossier).join("MicrosoftGame.config")).unwrap_or_default();
        let (nom, appli) = lire_config_xbox(&xml);
        let nom = nom.unwrap_or_else(|| famille.split('_').next().unwrap_or(famille).split('.').next_back().unwrap_or(famille).to_string());
        v.push(JeuXbox { nom, famille: famille.into(), appli: appli.unwrap_or_else(|| "Game".into()), dossier: dossier.into() });
    }
    let ancien: Vec<JeuXbox> = crate::outil::charger("jeux_xbox");
    let change = ancien.iter().map(|x| &x.famille).ne(v.iter().map(|x| &x.famille));
    if !v.is_empty() || o.status.success() {
        crate::outil::enregistrer("jeux_xbox", &v);
    }
    if change {
        NOUVEAUX.store(true, Relaxed);
    }
}

// ---------- jaquettes ----------

pub fn slug(nom: &str) -> String {
    let s: String = nom.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    s.split('-').filter(|x| !x.is_empty()).collect::<Vec<_>>().join("-")
}

fn dossier_jaquettes() -> PathBuf {
    crate::outil::chemin("x").parent().map(|p| p.join("jaquettes")).unwrap_or_else(|| PathBuf::from("jaquettes"))
}

pub fn jaquette_en_cache(nom: &str) -> Option<PathBuf> {
    let p = dossier_jaquettes().join(format!("{}.jpg", slug(nom)));
    (p.metadata().map(|m| m.len() > 1000).unwrap_or(false)).then_some(p)
}

static JAQUETTES_EN_COURS: AtomicBool = AtomicBool::new(false);

/// Cherche en fond les jaquettes manquantes (une fois par jeu : un échec est retenu).
pub fn chercher_jaquettes(noms: Vec<String>) {
    if noms.is_empty() || JAQUETTES_EN_COURS.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let d = dossier_jaquettes();
        let _ = std::fs::create_dir_all(&d);
        let mut echecs: Vec<String> = crate::outil::charger("jaquettes_introuvables");
        let a = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).user_agent("Outils").build();
        let mut trouve = false;
        for n in noms {
            let s = slug(&n);
            if echecs.contains(&s) || jaquette_en_cache(&n).is_some() {
                continue;
            }
            let id = a.get("https://store.steampowered.com/api/storesearch/").query("term", &n).query("l", "french").query("cc", "FR").call().ok().and_then(|r| r.into_json::<serde_json::Value>().ok()).and_then(|j| {
                let items = j["items"].as_array()?.clone();
                // le résultat dont le nom ressemble le plus
                // seulement un résultat au nom proche (sinon : mauvaise jaquette pour un jeu absent de Steam)
                items
                    .iter()
                    .find(|i| slug(i["name"].as_str().unwrap_or("")) == s)
                    .or_else(|| items.first().filter(|i| {
                        let n = slug(i["name"].as_str().unwrap_or(""));
                        n.len() >= 4 && s.len() >= 4 && (n.starts_with(&s) || s.starts_with(&n))
                    }))
                    .and_then(|i| i["id"].as_u64())
            });
            let mut ok = false;
            if let Some(id) = id {
                for f in ["library_600x900.jpg", "header.jpg"] {
                    if let Ok(r) = a.get(&format!("https://cdn.akamai.steamstatic.com/steam/apps/{id}/{f}")).call() {
                        let mut b = vec![];
                        if std::io::Read::read_to_end(&mut r.into_reader(), &mut b).is_ok() && b.len() > 1000 {
                            ok = std::fs::write(d.join(format!("{s}.jpg")), &b).is_ok();
                            break;
                        }
                    }
                }
            }
            if ok {
                trouve = true;
            } else {
                echecs.push(s);
            }
            std::thread::sleep(Duration::from_millis(1200));
        }
        crate::outil::enregistrer("jaquettes_introuvables", &echecs);
        if trouve {
            NOUVEAUX.store(true, Relaxed);
        }
        JAQUETTES_EN_COURS.store(false, Relaxed);
    });
}

/// Tous les magasins en plus de Steam, Epic et FiveM.
pub fn autres() -> Vec<Jeu> {
    let mut v = scan_gog();
    v.extend(scan_riot());
    v.extend(scan_ubisoft());
    v.extend(scan_editeurs());
    v.extend(scan_xbox());
    v
}

/// Magasin d'un jeu d'après son identifiant (pour la petite étiquette sur la carte).
pub fn magasin(id: &str) -> &'static str {
    match id.split(':').next().unwrap_or("") {
        "steam" => "Steam",
        "epic" => "Epic",
        "gog" => "GOG",
        "riot" => "Riot",
        "ubi" => "Ubisoft",
        "ea" => "EA",
        "bnet" => "Battle.net",
        "xbox" => "Xbox",
        "fivem" => "FiveM",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lectures() {
        assert_eq!(exe_icone(r#""C:\Jeux\Overwatch\Overwatch Launcher.exe",0"#).as_deref(), Some(r"C:\Jeux\Overwatch\Overwatch Launcher.exe"));
        assert_eq!(exe_icone(r"C:\x\jeu.EXE").as_deref(), Some(r"C:\x\jeu.EXE"));
        assert_eq!(exe_icone(r"C:\x\jeu.ico"), None);
        assert_eq!(chemin_riot("x: 1\nproduct_install_full_path: \"C:/Riot Games/VALORANT/live\"\n").as_deref(), Some(r"C:\Riot Games\VALORANT\live"));
        assert_eq!(nom_riot("valorant"), Some("VALORANT"));
        assert_eq!(nom_riot("riot_client"), None);
        let xml = r#"<Game configVersion="1"><Identity Name="X"/><ExecutableList><Executable Name="game.exe" Id="Game" TargetDeviceFamily="PC"/></ExecutableList><ShellVisuals DefaultDisplayName="Forza Horizon 5" PublisherDisplayName="Xbox"/></Game>"#;
        assert_eq!(lire_config_xbox(xml), (Some("Forza Horizon 5".into()), Some("Game".into())));
        assert_eq!(lire_config_xbox(r#"<ShellVisuals DefaultDisplayName="ms-resource:Nom"/>"#).0, None);
        assert_eq!(slug("Baldur's Gate 3 : Édition"), "baldur-s-gate-3-édition");
        assert_eq!(magasin("gog:123"), "GOG");
    }
}
