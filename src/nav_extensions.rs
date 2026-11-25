//! Extensions du navigateur : installation depuis le Chrome Web Store (fichier .crx), dossiers
//! d'extensions, et extensions maison d'Outils. Rien ici ne dépend de Windows (testable partout).
use std::io::Read;
use std::path::{Path, PathBuf};

/// Identifiant d'extension Chrome (32 lettres de a à p) dans un lien du Chrome Web Store ou tel quel.
pub fn id_extension(t: &str) -> Option<String> {
    let t = t.trim();
    let morceaux = t.split(['/', '?', '#', '&', '=']);
    morceaux.map(|m| m.trim().to_lowercase()).find(|m| m.len() == 32 && m.chars().all(|c| ('a'..='p').contains(&c)))
}

/// Adresse de téléchargement du .crx d'une extension du Chrome Web Store.
pub fn url_crx(id: &str) -> String {
    format!("https://clients2.google.com/service/update2/crx?response=redirect&prodversion=131.0.0.0&acceptformat=crx2,crx3&x=id%3D{id}%26installsource%3Dondemand%26uc")
}

/// Contenu zip d'un fichier .crx (versions 2 et 3).
pub fn zip_du_crx(b: &[u8]) -> Result<&[u8], String> {
    if b.starts_with(b"PK") {
        return Ok(b);
    }
    if b.len() < 16 || &b[..4] != b"Cr24" {
        return Err("ce n'est pas une extension Chrome (.crx)".into());
    }
    let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    let debut = match u(4) {
        3 => 12 + u(8),
        2 => 16 + u(8) + u(12),
        v => return Err(format!("format .crx {v} inconnu")),
    };
    b.get(debut..).filter(|z| z.starts_with(b"PK")).ok_or_else(|| "fichier .crx abîmé".into())
}

/// Identifiant Chrome d'une clé publique : SHA-256, 16 premiers octets, chaque chiffre hexadécimal → a..p.
pub fn id_de_cle(cle: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(cle).iter().take(16).flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect()
}

/// Lit un entier « varint » protobuf.
fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for k in 0..10 {
        let o = *b.get(*i)?;
        *i += 1;
        v |= ((o & 0x7F) as u64) << (7 * k);
        if o < 0x80 {
            return Some(v);
        }
    }
    None
}

/// Champs « octets » (type 2) d'un message protobuf : (numéro, contenu).
fn champs(b: &[u8]) -> Vec<(u64, &[u8])> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let Some(t) = varint(b, &mut i) else { break };
        match t & 7 {
            0 => {
                if varint(b, &mut i).is_none() {
                    break;
                }
            }
            2 => {
                let Some(n) = varint(b, &mut i) else { break };
                let Some(c) = b.get(i..i + n as usize) else { break };
                v.push((t >> 3, c));
                i += n as usize;
            }
            5 => i += 4,
            1 => i += 8,
            _ => break,
        }
    }
    v
}

/// Clé publique (Base64) de l'extension, tirée de l'en-tête d'un .crx version 3 : en l'ajoutant au
/// manifest, l'extension garde son vrai identifiant (et ses réglages).
pub fn cle_publique(crx: &[u8], id: &str) -> Option<String> {
    use base64::Engine;
    if crx.len() < 12 || &crx[..4] != b"Cr24" || u32::from_le_bytes([crx[4], crx[5], crx[6], crx[7]]) != 3 {
        return None;
    }
    let n = u32::from_le_bytes([crx[8], crx[9], crx[10], crx[11]]) as usize;
    let entete = crx.get(12..12 + n)?;
    // CrxFileHeader : champ 2 = preuves RSA { 1 : clé publique, 2 : signature }
    champs(entete)
        .into_iter()
        .filter(|(f, _)| *f == 2 || *f == 3)
        .flat_map(|(_, preuve)| champs(preuve).into_iter().filter(|(f, _)| *f == 1).map(|(_, c)| c).collect::<Vec<_>>())
        .find(|c| id_de_cle(c) == id)
        .map(|c| base64::engine::general_purpose::STANDARD.encode(c))
}

/// Décompresse une extension (.crx ou .zip) dans `dest`.
pub fn installer_crx(octets: &[u8], dest: &Path) -> Result<(), String> {
    let z = zip_du_crx(octets)?;
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(z)).map_err(|e| e.to_string())?;
    a.extract(dest).map_err(|e| e.to_string())?;
    if !dest.join("manifest.json").is_file() {
        return Err("manifest.json absent : ce n'est pas une extension".into());
    }
    Ok(())
}

/// Télécharge et installe une extension du Chrome Web Store.
pub fn telecharger(id: &str, dossier: &Path) -> Result<String, String> {
    let r = ureq::get(&url_crx(id)).set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0.0.0").call().map_err(|e| format!("téléchargement impossible : {e}"))?;
    let mut b = Vec::new();
    r.into_reader().take(80 << 20).read_to_end(&mut b).map_err(|e| e.to_string())?;
    let dest = dossier.join(id);
    installer_crx(&b, &dest)?;
    // le vrai identifiant du Chrome Web Store (sinon il dépendrait du dossier)
    if let Some(cle) = cle_publique(&b, id) {
        let f = dest.join("manifest.json");
        if let Ok(mut j) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(t.trim_start_matches('\u{feff}')).ok()).ok_or(()) {
            if j.get("key").is_none() {
                j["key"] = serde_json::Value::String(cle);
                let _ = std::fs::write(&f, serde_json::to_string_pretty(&j).unwrap_or_default());
            }
        }
    }
    Ok(Manifeste::lire(&dest).map(|m| m.nom).unwrap_or_else(|| id.to_string()))
}

/// Ce qu'Outils lit du manifest.json d'une extension.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifeste {
    pub nom: String,
    pub version: String,
    pub fenetre: Option<String>,  // page « popup » du bouton de l'extension
    pub reglages: Option<String>, // page d'options
}

impl Manifeste {
    pub fn lire(d: &Path) -> Option<Manifeste> {
        let t = std::fs::read_to_string(d.join("manifest.json")).ok()?;
        let j: serde_json::Value = serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()?;
        let nom = traduire(d, &j, j["name"].as_str().unwrap_or("?"));
        let fenetre = ["action", "browser_action", "page_action"].iter().find_map(|k| j[k]["default_popup"].as_str()).map(str::to_string);
        let reglages = j["options_ui"]["page"].as_str().or(j["options_page"].as_str()).map(str::to_string);
        Some(Manifeste { nom, version: j["version"].as_str().unwrap_or("").to_string(), fenetre, reglages })
    }
}

/// « __MSG_appName__ » → le texte traduit (_locales/fr puis langue par défaut).
fn traduire(d: &Path, j: &serde_json::Value, t: &str) -> String {
    let Some(cle) = t.strip_prefix("__MSG_").and_then(|x| x.strip_suffix("__")) else { return t.to_string() };
    let defaut = j["default_locale"].as_str().unwrap_or("en");
    for l in ["fr", defaut, "en"] {
        let f = d.join("_locales").join(l).join("messages.json");
        if let Some(m) = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim_start_matches('\u{feff}')).ok()) {
            // les clés ne tiennent pas compte de la casse
            if let Some(o) = m.as_object() {
                if let Some((_, v)) = o.iter().find(|(k, _)| k.eq_ignore_ascii_case(cle)) {
                    if let Some(s) = v["message"].as_str() {
                        return s.to_string();
                    }
                }
            }
        }
    }
    cle.to_string()
}

/// Dossiers des extensions installées (actives) ou mises de côté (désactivées).
pub fn extensions_dans(d: &Path) -> Vec<(PathBuf, Manifeste)> {
    let mut v: Vec<(PathBuf, Manifeste)> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter_map(|p| Manifeste::lire(&p).map(|m| (p, m))).collect();
    v.sort_by(|a, b| a.1.nom.to_lowercase().cmp(&b.1.nom.to_lowercase()));
    v
}

// ---------------------------------------------------------------------------------------------------
// Extensions maison
// ---------------------------------------------------------------------------------------------------

/// (dossier, nom affiché, description, fichiers)
pub const MAISON: &[(&str, &str, &str, &[(&str, &str)])] = &[
    ("outils-sombre", "🌙 Mode sombre partout", "Les sites clairs passent en sombre (les sites déjà sombres ne changent pas).", &[("manifest.json", SOMBRE_MANIFESTE), ("sombre.js", SOMBRE_JS)]),
    ("outils-video", "⏩ Vitesse des vidéos", "Alt+= / Alt+- pour accélérer ou ralentir n'importe quelle vidéo, Alt+0 pour revenir à la normale.", &[("manifest.json", VIDEO_MANIFESTE), ("video.js", VIDEO_JS)]),
    ("outils-lecture", "📖 Mode lecture", "Alt+L : l'article seul, sans pubs ni menus, en grand et lisible. Échap pour revenir.", &[("manifest.json", LECTURE_MANIFESTE), ("lecture.js", LECTURE_JS)]),
    ("outils-cookies", "🍪 Anti-bannières de cookies", "Refuse tout seul les cookies quand c'est possible et cache les bannières.", &[("manifest.json", COOKIES_MANIFESTE), ("cookies.js", COOKIES_JS)]),
];

/// Écrit une extension maison dans `dest`.
pub fn ecrire_maison(nom: &str, dest: &Path) -> Result<(), String> {
    let (_, _, _, fichiers) = MAISON.iter().find(|m| m.0 == nom).ok_or("extension inconnue")?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for (f, c) in fichiers.iter() {
        std::fs::write(dest.join(f), c).map_err(|e| e.to_string())?;
    }
    Ok(())
}

const SOMBRE_MANIFESTE: &str = r#"{
  "manifest_version": 3,
  "name": "Outils - Mode sombre partout",
  "version": "1.0",
  "content_scripts": [{ "matches": ["<all_urls>"], "js": ["sombre.js"], "run_at": "document_end", "all_frames": false }]
}"#;

const SOMBRE_JS: &str = r#"(function () {
  function clair(el) {
    var c = getComputedStyle(el).backgroundColor.match(/[\d.]+/g);
    if (!c || (c.length > 3 && +c[3] === 0)) return null;
    return (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / 255;
  }
  var l = clair(document.body) ?? clair(document.documentElement) ?? 1;
  if (l < 0.5) return; // déjà sombre
  var s = document.createElement('style');
  s.id = 'outils-sombre';
  s.textContent = 'html{filter:invert(.92) hue-rotate(180deg)!important;background:#fff!important}' +
    'img,video,picture,canvas,iframe,embed,object,svg image,[style*="background-image"]{filter:invert(1) hue-rotate(180deg)!important}';
  document.documentElement.appendChild(s);
})();
"#;

const VIDEO_MANIFESTE: &str = r#"{
  "manifest_version": 3,
  "name": "Outils - Vitesse des videos",
  "version": "1.0",
  "content_scripts": [{ "matches": ["<all_urls>"], "js": ["video.js"], "run_at": "document_idle", "all_frames": true }]
}"#;

const VIDEO_JS: &str = r#"(function () {
  var bulle;
  function montrer(v) {
    if (!bulle) {
      bulle = document.createElement('div');
      bulle.style.cssText = 'position:fixed;top:20px;left:50%;transform:translateX(-50%);z-index:2147483647;background:rgba(0,0,0,.75);color:#fff;font:bold 22px sans-serif;padding:8px 16px;border-radius:10px;pointer-events:none';
      document.documentElement.appendChild(bulle);
    }
    bulle.textContent = '⏩ ×' + v.toFixed(2).replace(/\.?0+$/, '');
    bulle.style.display = 'block';
    clearTimeout(bulle.t);
    bulle.t = setTimeout(function () { bulle.style.display = 'none'; }, 900);
  }
  document.addEventListener('keydown', function (e) {
    if (!e.altKey) return;
    var videos = document.querySelectorAll('video');
    if (!videos.length) return;
    var v = videos[0].playbackRate;
    if (e.key === '=' || e.key === '+') v = Math.min(v + 0.25, 4);
    else if (e.key === '-') v = Math.max(v - 0.25, 0.25);
    else if (e.key === '0') v = 1;
    else return;
    e.preventDefault();
    videos.forEach(function (x) { x.playbackRate = v; });
    montrer(v);
  }, true);
})();
"#;

const LECTURE_MANIFESTE: &str = r#"{
  "manifest_version": 3,
  "name": "Outils - Mode lecture",
  "version": "1.0",
  "content_scripts": [{ "matches": ["<all_urls>"], "js": ["lecture.js"], "run_at": "document_idle" }]
}"#;

const LECTURE_JS: &str = r#"(function () {
  var vue = null;
  function principal() {
    var meilleur = null, score = 0;
    document.querySelectorAll('article, main, section, div').forEach(function (el) {
      var s = 0;
      el.querySelectorAll(':scope > p, :scope > div > p').forEach(function (p) { s += p.innerText.length; });
      if (s > score) { score = s; meilleur = el; }
    });
    return meilleur || document.body;
  }
  function ouvrir() {
    var src = principal().cloneNode(true);
    src.querySelectorAll('script,style,iframe,nav,aside,form,button,[class*=ad],[id*=ad],[class*=share],[class*=comment]').forEach(function (x) { x.remove(); });
    vue = document.createElement('div');
    vue.style.cssText = 'position:fixed;inset:0;z-index:2147483647;overflow:auto;background:#f8f4ec;color:#222;padding:40px 0';
    var page = document.createElement('div');
    page.style.cssText = 'max-width:720px;margin:auto;font:20px/1.7 Georgia,serif;padding:0 20px';
    var h = document.createElement('h1');
    h.textContent = document.title;
    h.style.cssText = 'font:bold 32px/1.3 sans-serif;margin-bottom:24px';
    page.appendChild(h);
    page.appendChild(src);
    page.querySelectorAll('img').forEach(function (i) { i.style.maxWidth = '100%'; i.style.height = 'auto'; });
    vue.appendChild(page);
    document.documentElement.appendChild(vue);
  }
  document.addEventListener('keydown', function (e) {
    if (e.altKey && (e.key === 'l' || e.key === 'L')) {
      e.preventDefault();
      if (vue) { vue.remove(); vue = null; } else ouvrir();
    } else if (e.key === 'Escape' && vue) { vue.remove(); vue = null; }
  }, true);
})();
"#;

const COOKIES_MANIFESTE: &str = r#"{
  "manifest_version": 3,
  "name": "Outils - Anti-bannieres de cookies",
  "version": "1.0",
  "content_scripts": [{ "matches": ["<all_urls>"], "js": ["cookies.js"], "run_at": "document_idle", "all_frames": true }]
}"#;

const COOKIES_JS: &str = r#"(function () {
  var refuser = ['#onetrust-reject-all-handler', '#didomi-notice-disagree-button', '.didomi-continue-without-agreeing',
    '#CybotCookiebotDialogBodyButtonDecline', '.fc-cta-do-not-consent', '#axeptio_btn_dismiss', '.qc-cmp2-summary-buttons button[mode="secondary"]',
    '[data-testid="uc-deny-all-button"]', '#tarteaucitronAllDenied2', '.cmplz-deny', '#cookie_action_close_header_reject', '.cky-btn-reject'];
  var textes = /^(tout refuser|refuser tout|refuser|continuer sans accepter|reject all|decline all|decline|refuse all|only necessary|nécessaires uniquement|uniquement les cookies nécessaires)$/i;
  var cacher = '#onetrust-banner-sdk,#onetrust-consent-sdk,#didomi-host,.qc-cmp2-container,#CybotCookiebotDialog,.cc-window,#cookie-banner,#cookies-banner,' +
    '#usercentrics-root,#axeptio_overlay,.fc-consent-root,#tarteaucitronRoot,.cky-consent-container,[id*="cookie-consent"],[class*="cookie-consent"]';
  function essayer() {
    for (var i = 0; i < refuser.length; i++) {
      var b = document.querySelector(refuser[i]);
      if (b && b.offsetParent !== null) { b.click(); return true; }
    }
    var boutons = document.querySelectorAll('button, a[role=button], [role=button]');
    for (var j = 0; j < boutons.length; j++) {
      var t = (boutons[j].innerText || '').trim();
      if (t.length < 45 && textes.test(t)) {
        var zone = boutons[j].closest('[id*=cookie],[class*=cookie],[id*=consent],[class*=consent],[id*=gdpr],[class*=gdpr],[aria-modal=true]');
        if (zone) { boutons[j].click(); return true; }
      }
    }
    return false;
  }
  var s = document.createElement('style');
  s.textContent = cacher + '{display:none!important}';
  var fin = Date.now() + 15000;
  var obs = new MutationObserver(function () { if (essayer() || Date.now() > fin) { obs.disconnect(); } });
  setTimeout(function () {
    if (!essayer()) {
      document.documentElement.appendChild(s);
      obs.observe(document.documentElement, { childList: true, subtree: true });
    }
    // certaines bannières bloquent le défilement
    document.documentElement.style.removeProperty('overflow');
    document.body && document.body.style.removeProperty('overflow');
  }, 600);
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_test(manifest: &str) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        z.start_file("manifest.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(manifest.as_bytes()).unwrap();
        z.add_directory("_locales/fr", zip::write::SimpleFileOptions::default()).unwrap();
        z.start_file("_locales/fr/messages.json", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(br#"{"appName":{"message":"Mon extension"}}"#).unwrap();
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn extensions() {
        let id = "cjpalhdlnbpafiamejdnhcphjbkeiagm";
        assert_eq!(id_extension(&format!("https://chromewebstore.google.com/detail/ublock-origin/{id}?hl=fr")).as_deref(), Some(id));
        assert_eq!(id_extension(id).as_deref(), Some(id));
        assert_eq!(id_extension("https://google.com"), None);
        assert!(url_crx(id).contains(id));
        // .crx version 3 : « Cr24 », version, taille de l'en-tête, en-tête, zip
        let z = zip_test(r#"{"manifest_version":3,"name":"__MSG_appName__","version":"2.1","default_locale":"en","action":{"default_popup":"popup.html"},"options_page":"options.html"}"#);
        let mut crx = b"Cr24".to_vec();
        crx.extend(3u32.to_le_bytes());
        crx.extend(5u32.to_le_bytes());
        crx.extend([9u8; 5]);
        crx.extend(&z);
        assert_eq!(zip_du_crx(&crx).unwrap(), &z[..]);
        // en-tête protobuf avec une clé : on retrouve l'identifiant
        let cle = b"une fausse cle publique RSA".to_vec();
        let id_cle = id_de_cle(&cle);
        assert!(id_cle.len() == 32 && id_cle.chars().all(|c| ('a'..='p').contains(&c)));
        let mut preuve = vec![0x0A, cle.len() as u8];
        preuve.extend(&cle);
        preuve.extend([0x12, 2, 7, 7]);
        let mut entete = vec![0x12, preuve.len() as u8];
        entete.extend(&preuve);
        let mut crx2 = b"Cr24".to_vec();
        crx2.extend(3u32.to_le_bytes());
        crx2.extend((entete.len() as u32).to_le_bytes());
        crx2.extend(&entete);
        crx2.extend(&z);
        use base64::Engine;
        assert_eq!(cle_publique(&crx2, &id_cle), Some(base64::engine::general_purpose::STANDARD.encode(&cle)));
        assert_eq!(cle_publique(&crx2, id), None);
        assert_eq!(zip_du_crx(&crx2).unwrap(), &z[..]);
        assert!(zip_du_crx(b"bonjour tout le monde").is_err());
        let d = std::env::temp_dir().join(format!("outils_ext_{}", std::process::id()));
        installer_crx(&crx, &d.join(id)).unwrap();
        let m = Manifeste::lire(&d.join(id)).unwrap();
        assert_eq!(m.nom, "Mon extension");
        assert_eq!(m.version, "2.1");
        assert_eq!(m.fenetre.as_deref(), Some("popup.html"));
        assert_eq!(m.reglages.as_deref(), Some("options.html"));
        for (n, ..) in MAISON {
            ecrire_maison(n, &d.join(n)).unwrap();
            let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.join(n).join("manifest.json")).unwrap()).unwrap();
            assert_eq!(j["manifest_version"], 3);
            for s in j["content_scripts"][0]["js"].as_array().unwrap() {
                assert!(d.join(n).join(s.as_str().unwrap()).is_file());
            }
        }
        assert_eq!(extensions_dans(&d).len(), 1 + MAISON.len());
        let _ = std::fs::remove_dir_all(&d);
    }
}
