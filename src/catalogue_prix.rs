//! Catalogue de prix : tu choisis des produits de magasins en ligne (leurs liens), Outils lit le nom,
//! le prix et la photo de chacun, suit les changements de prix, et fait un beau PDF rangé (par
//! catégorie ou par magasin, du moins cher au plus cher, avec les totaux).
//! Les pages sont lues comme le ferait un navigateur ; pour les sites qui bloquent, le bouton 🛒 du
//! navigateur d'Outils envoie la page affichée, et on peut toujours remplir un produit à la main.
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::papiers::{apercu_pages, couper, largeur, pdf_complet, Op, A4};
use eframe::egui::{self, Color32, RichText};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------------------------------
// Lire une page produit
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Infos {
    pub nom: String,
    pub prix: Option<f64>,
    pub devise: String,
    pub image: Option<String>,
    pub dispo: Option<bool>,
}

/// « 1 299,90 », « 1,299.90 », « 19.99 », « 24 » → nombre.
pub fn lire_prix(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
    if t.is_empty() {
        return None;
    }
    // le dernier séparateur suivi de 1 ou 2 chiffres est la virgule décimale
    let dec = t.rfind([',', '.']).filter(|&i| (1..=2).contains(&(t.len() - i - 1)));
    let n = match dec {
        Some(i) => format!("{}.{}", t[..i].replace([',', '.'], ""), &t[i + 1..]),
        None => t.replace([',', '.'], ""),
    };
    n.parse().ok().filter(|p: &f64| *p > 0.0 && *p < 10_000_000.0)
}

/// Petit décodage des entités HTML courantes.
pub fn entites(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut reste = s;
    while let Some(i) = reste.find('&') {
        o.push_str(&reste[..i]);
        let apres = &reste[i..];
        let fin = apres.find(';').filter(|&f| f <= 10);
        let rempl = fin.and_then(|f| {
            let e = &apres[1..f];
            let c = match e {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                "eacute" => Some('é'),
                "egrave" => Some('è'),
                "ecirc" => Some('ê'),
                "agrave" => Some('à'),
                "acirc" => Some('â'),
                "ccedil" => Some('ç'),
                "ocirc" => Some('ô'),
                "ucirc" => Some('û'),
                "icirc" => Some('î'),
                "euro" => Some('€'),
                _ if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                _ if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            }?;
            Some((c, f + 1))
        });
        match rempl {
            Some((c, n)) => {
                o.push(c);
                reste = &apres[n..];
            }
            None => {
                o.push('&');
                reste = &apres[1..];
            }
        }
    }
    o.push_str(reste);
    o.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn absolue(u: &str, base: &str) -> String {
    let u = u.trim();
    if u.starts_with("http") {
        u.to_string()
    } else if let Some(r) = u.strip_prefix("//") {
        format!("https://{r}")
    } else {
        let origine: String = base.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
        format!("{origine}/{}", u.trim_start_matches('/'))
    }
}

fn texte_json(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Array(a) => a.first().and_then(texte_json),
        serde_json::Value::Object(o) => o.get("url").or_else(|| o.get("contentUrl")).or_else(|| o.get("@id")).and_then(texte_json),
        _ => None,
    }
}

fn est_produit(v: &serde_json::Value) -> bool {
    match &v["@type"] {
        serde_json::Value::String(t) => t == "Product" || t == "ProductGroup",
        serde_json::Value::Array(a) => a.iter().any(|t| t == "Product"),
        _ => false,
    }
}

fn chercher_produit(v: &serde_json::Value, prof: usize) -> Option<&serde_json::Value> {
    if prof > 6 {
        return None;
    }
    if est_produit(v) {
        return Some(v);
    }
    match v {
        serde_json::Value::Array(a) => a.iter().find_map(|x| chercher_produit(x, prof + 1)),
        serde_json::Value::Object(o) => o.values().find_map(|x| chercher_produit(x, prof + 1)),
        _ => None,
    }
}

/// Infos d'un produit à partir du HTML de sa page.
pub fn lire_page(html: &str, url: &str) -> Infos {
    let mut i = Infos::default();
    // 1. données structurées (schema.org), utilisées par presque tous les magasins
    let re_ld = regex::Regex::new(r#"(?is)<script[^>]*application/ld\+json[^>]*>(.*?)</script>"#).unwrap();
    for c in re_ld.captures_iter(html) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(c[1].trim()) else { continue };
        let Some(p) = chercher_produit(&v, 0) else { continue };
        if i.nom.is_empty() {
            i.nom = p["name"].as_str().map(entites).unwrap_or_default();
        }
        if i.image.is_none() {
            i.image = texte_json(&p["image"]).map(|x| absolue(&x, url));
        }
        let offres = if p["offers"].is_null() { &p["hasVariant"][0]["offers"] } else { &p["offers"] };
        let offre = if offres.is_array() { &offres[0] } else { offres };
        if i.prix.is_none() {
            i.prix = ["price", "lowPrice", "highPrice"].iter().find_map(|k| texte_json(&offre[*k]).and_then(|x| lire_prix(&x)));
            if i.prix.is_none() {
                i.prix = texte_json(&offre["priceSpecification"]["price"]).and_then(|x| lire_prix(&x));
            }
        }
        if i.devise.is_empty() {
            i.devise = offre["priceCurrency"].as_str().unwrap_or("").to_string();
        }
        if i.dispo.is_none() {
            i.dispo = offre["availability"].as_str().map(|a| a.contains("InStock") || a.contains("LimitedAvailability") || a.contains("PreOrder"));
        }
    }
    // 2. balises meta (Open Graph, microdonnées)
    let re_meta = regex::Regex::new(r#"(?is)<meta\s[^>]*>"#).unwrap();
    // valeur entre "…" (peut contenir une apostrophe) ou entre '…'
    let re_attr = regex::Regex::new(r#"(?is)(property|name|itemprop|content)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap();
    for m in re_meta.find_iter(html) {
        let (mut cle, mut val) = (String::new(), String::new());
        for a in re_attr.captures_iter(m.as_str()) {
            let v = a.get(2).or(a.get(3)).map_or("", |x| x.as_str());
            if a[1].eq_ignore_ascii_case("content") {
                val = v.to_string();
            } else {
                cle = v.to_lowercase();
            }
        }
        match cle.as_str() {
            "og:title" | "twitter:title" if i.nom.is_empty() => i.nom = entites(&val),
            "og:image" | "og:image:secure_url" | "twitter:image" if i.image.is_none() && !val.is_empty() => i.image = Some(absolue(&val, url)),
            "product:price:amount" | "og:price:amount" | "price" if i.prix.is_none() => i.prix = lire_prix(&val),
            "product:price:currency" | "og:price:currency" | "pricecurrency" if i.devise.is_empty() => i.devise = val.clone(),
            _ => {}
        }
    }
    // 3. en dernier recours : le titre et le premier prix en euros de la page
    if i.nom.is_empty() {
        let re_t = regex::Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap();
        i.nom = re_t.captures(html).map(|c| entites(&c[1])).unwrap_or_default();
    }
    if i.prix.is_none() {
        let re_p = regex::Regex::new(r"(\d{1,3}(?:[ \u{a0}.]\d{3})*[,.]\d{2}|\d+)\s*(?:€|&euro;|&#8364;|EUR)").unwrap();
        i.prix = re_p.captures(html).and_then(|c| lire_prix(&c[1]));
        if i.prix.is_some() && i.devise.is_empty() {
            i.devise = "EUR".into();
        }
    }
    if i.devise.is_empty() {
        i.devise = "EUR".into();
    }
    i.nom = i.nom.chars().take(160).collect();
    i
}

fn agent_web() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(25))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36")
        .build()
}

/// Télécharge la page d'un produit.
pub fn telecharger_page(url: &str) -> Result<String, String> {
    let r = agent_web()
        .get(url)
        .set("Accept", "text/html,application/xhtml+xml")
        .set("Accept-Language", "fr-FR,fr;q=0.9")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(403 | 429 | 503, _) => "le site refuse les robots : ouvre la page dans le navigateur d'Outils et clique sur 🛒".to_string(),
            ureq::Error::Status(404, _) => "page introuvable".to_string(),
            e => e.to_string(),
        })?;
    let mut b = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 8 << 20), &mut b).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn dossier_images() -> PathBuf {
    crate::outil::racine().join("catalogue_prix")
}

/// Télécharge la photo, la réduit et la garde (PNG).
fn telecharger_image(url: &str, id: u64) -> Option<PathBuf> {
    let r = agent_web().get(url).set("Accept", "image/avif,image/webp,image/*").call().ok()?;
    let mut b = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 12 << 20), &mut b).ok()?;
    let img = image::load_from_memory(&b).ok()?.thumbnail(480, 480).to_rgba8();
    let _ = std::fs::create_dir_all(dossier_images());
    let p = dossier_images().join(format!("{id}.png"));
    img.save(&p).ok()?;
    Some(p)
}

// ---------------------------------------------------------------------------------------------------
// Le catalogue
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Produit {
    pub id: u64,
    pub url: String,
    pub nom: String,
    pub prix: Option<f64>,
    pub devise: String,
    pub image: Option<PathBuf>,
    pub magasin: String,
    pub categorie: String,
    pub dispo: Option<bool>,
    pub quantite: u32,
    pub ancien_prix: Option<f64>,
    pub maj: i64,
    pub erreur: String,
}

impl Default for Produit {
    fn default() -> Self {
        Produit { id: 0, url: String::new(), nom: String::new(), prix: None, devise: "EUR".into(), image: None, magasin: String::new(), categorie: String::new(), dispo: None, quantite: 1, ancien_prix: None, maj: 0, erreur: String::new() }
    }
}

/// « www.boulanger.com » → « Boulanger ».
pub fn magasin_de(url: &str) -> String {
    let hote = url.split("://").nth(1).unwrap_or(url).split(['/', '?', ':']).next().unwrap_or("");
    let parties: Vec<&str> = hote.split('.').filter(|p| !matches!(*p, "www" | "m" | "fr" | "shop" | "boutique")).collect();
    let nom = if parties.len() >= 2 { parties[parties.len() - 2] } else { parties.first().copied().unwrap_or("") };
    let mut c = nom.chars();
    c.next().map(|p| p.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Catalogue {
    pub titre: String,
    pub produits: Vec<Produit>,
    /// 0 = par catégorie, 1 = par magasin, 2 = tout ensemble
    pub regrouper: u8,
    /// 0 = moins cher d'abord, 1 = plus cher d'abord, 2 = par nom
    pub tri: u8,
    pub totaux: bool,
    pub prochain: u64,
}

impl Default for Catalogue {
    fn default() -> Self {
        Catalogue { titre: "Mon catalogue de prix".into(), produits: vec![], regrouper: 0, tri: 0, totaux: true, prochain: 1 }
    }
}

/// Met à jour un produit avec les infos lues ; renvoie l'URL de l'image à télécharger si besoin.
pub fn appliquer(p: &mut Produit, i: Infos, maintenant: i64) -> Option<String> {
    if !i.nom.is_empty() && (p.nom.is_empty() || p.maj == 0) {
        p.nom = i.nom;
    }
    if let Some(n) = i.prix {
        if p.prix.is_some_and(|a| (a - n).abs() > 0.004) {
            p.ancien_prix = p.prix;
        }
        p.prix = Some(n);
    }
    p.devise = i.devise;
    p.dispo = i.dispo;
    p.maj = maintenant;
    p.erreur = if i.prix.is_none() { "prix introuvable sur la page : remplis-le à la main".into() } else { String::new() };
    if p.magasin.is_empty() {
        p.magasin = magasin_de(&p.url);
    }
    i.image.filter(|_| p.image.as_ref().is_none_or(|f| !f.exists()))
}

pub fn euros(p: f64, devise: &str) -> String {
    let c = (p * 100.0).round() as i64;
    let (ent, cts) = (c / 100, c % 100);
    let mut s = ent.to_string();
    let mut k = s.len() as i64 - 3;
    while k > 0 {
        s.insert(k as usize, ' ');
        k -= 3;
    }
    let sym = match devise {
        "EUR" | "" => "€",
        "USD" => "$",
        "GBP" => "£",
        "CHF" => "CHF",
        x => x,
    };
    format!("{s},{cts:02} {sym}")
}

/// Groupes de produits (titre, indices) selon le réglage, triés.
pub fn groupes(c: &Catalogue) -> Vec<(String, Vec<usize>)> {
    let mut g: Vec<(String, Vec<usize>)> = vec![];
    for (k, p) in c.produits.iter().enumerate() {
        let cle = match c.regrouper {
            0 => if p.categorie.trim().is_empty() { "Sans catégorie".to_string() } else { p.categorie.trim().to_string() },
            1 => if p.magasin.is_empty() { "Autre".into() } else { p.magasin.clone() },
            _ => String::new(),
        };
        match g.iter_mut().find(|x| x.0 == cle) {
            Some(x) => x.1.push(k),
            None => g.push((cle, vec![k])),
        }
    }
    g.sort_by(|a, b| (a.0 == "Sans catégorie").cmp(&(b.0 == "Sans catégorie")).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    for (_, v) in &mut g {
        v.sort_by(|&a, &b| {
            let (pa, pb) = (&c.produits[a], &c.produits[b]);
            match c.tri {
                0 => pa.prix.unwrap_or(f64::MAX).total_cmp(&pb.prix.unwrap_or(f64::MAX)),
                1 => pb.prix.unwrap_or(-1.0).total_cmp(&pa.prix.unwrap_or(-1.0)),
                _ => pa.nom.to_lowercase().cmp(&pb.nom.to_lowercase()),
            }
        });
    }
    g
}

const ROSE: [u8; 3] = [214, 51, 120];
const NOIR: [u8; 3] = [25, 25, 30];
const GRIS: [u8; 3] = [110, 110, 120];

/// Pages du PDF et ordre des images (indices de produits).
pub fn mise_en_page(c: &Catalogue, date: &str) -> (Vec<Vec<Op>>, Vec<usize>) {
    let (marge, haut, bas) = (36.0, 40.0, A4.1 - 40.0);
    let colonnes = 3;
    let esp = 12.0;
    let lc = (A4.0 - 2.0 * marge - esp * (colonnes as f32 - 1.0)) / colonnes as f32;
    let hc = 228.0;
    let mut pages: Vec<Vec<Op>> = vec![vec![]];
    let mut images: Vec<usize> = vec![];
    let total_qte = |v: &[usize]| v.iter().filter_map(|&k| c.produits[k].prix.map(|p| p * c.produits[k].quantite.max(1) as f64)).sum::<f64>();
    let tous: Vec<usize> = (0..c.produits.len()).collect();
    let devise = c.produits.first().map(|p| p.devise.clone()).unwrap_or_default();
    // en-tête
    let mut y = haut;
    let p = pages.last_mut().unwrap();
    p.push(Op::Rect { x: 0.0, y: 0.0, l: A4.0, h: 6.0, c: ROSE });
    p.push(Op::Texte { x: marge, y: y + 18.0, taille: 20.0, gras: true, c: NOIR, s: c.titre.clone() });
    y += 30.0;
    let mut sous = format!("{} produit(s) · prix relevés le {date}", c.produits.len());
    if c.totaux {
        sous += &format!(" · total {}", euros(total_qte(&tous), &devise));
    }
    p.push(Op::Texte { x: marge, y: y + 10.0, taille: 10.0, gras: false, c: GRIS, s: sous });
    y += 26.0;
    for (titre, ids) in groupes(c) {
        // titre du groupe (jamais seul en bas de page)
        if y + 24.0 + hc > bas {
            pages.push(vec![]);
            y = haut;
        }
        if !titre.is_empty() {
            let p = pages.last_mut().unwrap();
            p.push(Op::Rect { x: marge, y, l: A4.0 - 2.0 * marge, h: 22.0, c: [248, 228, 238] });
            p.push(Op::Texte { x: marge + 8.0, y: y + 15.5, taille: 12.0, gras: true, c: ROSE, s: titre.clone() });
            if c.totaux {
                let t = format!("{} article(s) · {}", ids.len(), euros(total_qte(&ids), &devise));
                p.push(Op::Texte { x: A4.0 - marge - 8.0 - largeur(&t, 10.0, false), y: y + 15.0, taille: 10.0, gras: false, c: GRIS, s: t });
            }
            y += 30.0;
        }
        let moins_cher = ids.iter().filter(|&&k| c.produits[k].prix.is_some()).min_by(|&&a, &&b| c.produits[a].prix.unwrap().total_cmp(&c.produits[b].prix.unwrap())).copied();
        for (n, &k) in ids.iter().enumerate() {
            let col = n % colonnes;
            if col == 0 && n > 0 {
                y += hc + esp;
            }
            if y + hc > bas {
                pages.push(vec![]);
                y = haut;
            }
            let x = marge + col as f32 * (lc + esp);
            let pr = &c.produits[k];
            let p = pages.last_mut().unwrap();
            // carte
            p.push(Op::Rect { x, y, l: lc, h: hc, c: [236, 236, 240] });
            p.push(Op::Rect { x: x + 1.0, y: y + 1.0, l: lc - 2.0, h: hc - 2.0, c: [255, 255, 255] });
            if pr.image.is_some() {
                p.push(Op::Image { x: x + 8.0, y: y + 8.0, l: lc - 16.0, h: 112.0, k: images.len() });
                images.push(k);
            } else {
                p.push(Op::Rect { x: x + 8.0, y: y + 8.0, l: lc - 16.0, h: 112.0, c: [244, 244, 246] });
                p.push(Op::Texte { x: x + lc / 2.0 - largeur("pas de photo", 9.0, false) / 2.0, y: y + 68.0, taille: 9.0, gras: false, c: GRIS, s: "pas de photo".into() });
            }
            let mut ty = y + 136.0;
            let lignes = couper(&pr.nom, 9.5, true, lc - 16.0);
            for (i, l) in lignes.iter().take(3).enumerate() {
                let l = if i == 2 && lignes.len() > 3 { format!("{}…", l.chars().take(l.chars().count().saturating_sub(1)).collect::<String>()) } else { l.clone() };
                p.push(Op::Texte { x: x + 8.0, y: ty, taille: 9.5, gras: true, c: NOIR, s: l });
                ty += 12.0;
            }
            let prix = pr.prix.map(|v| euros(v, &pr.devise)).unwrap_or_else(|| "prix inconnu".into());
            p.push(Op::Texte { x: x + 8.0, y: y + hc - 30.0, taille: 16.0, gras: true, c: if pr.prix.is_some() { ROSE } else { GRIS }, s: prix });
            if let (Some(a), Some(v)) = (pr.ancien_prix, pr.prix) {
                let t = format!("était {}", euros(a, &pr.devise));
                p.push(Op::Texte { x: x + lc - 8.0 - largeur(&t, 8.0, false), y: y + hc - 32.0, taille: 8.0, gras: false, c: if v < a { [30, 140, 60] } else { [200, 60, 50] }, s: t });
            }
            let mut bas_t = pr.magasin.clone();
            if pr.quantite > 1 {
                bas_t += &format!(" · ×{}", pr.quantite);
            }
            if pr.dispo == Some(false) {
                bas_t += " · indisponible";
            }
            p.push(Op::Texte { x: x + 8.0, y: y + hc - 12.0, taille: 8.5, gras: false, c: if pr.dispo == Some(false) { [200, 60, 50] } else { GRIS }, s: bas_t });
            if moins_cher == Some(k) && ids.len() > 1 && c.tri != 2 {
                let t = "LE MOINS CHER";
                let l = largeur(t, 7.5, true) + 10.0;
                p.push(Op::Rect { x: x + lc - l - 6.0, y: y + 6.0, l, h: 14.0, c: [30, 150, 70] });
                p.push(Op::Texte { x: x + lc - l - 1.0, y: y + 16.0, taille: 7.5, gras: true, c: [255, 255, 255], s: t.into() });
            }
        }
        if !ids.is_empty() {
            y += hc + esp + 8.0;
        }
    }
    // numéros de page
    let n = pages.len();
    for (k, p) in pages.iter_mut().enumerate() {
        let t = format!("{} / {n}", k + 1);
        p.push(Op::Texte { x: A4.0 - marge - largeur(&t, 8.0, false), y: A4.1 - 18.0, taille: 8.0, gras: false, c: GRIS, s: t });
    }
    (pages, images)
}

/// Pages reçues du navigateur d'Outils (bouton 🛒) : (adresse, HTML).
static RECUS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn recevoir(url: &str, html: String) {
    RECUS.lock().unwrap().push((url.to_string(), html));
}

pub struct CataloguePrix {
    c: Arc<Mutex<Catalogue>>,
    liens: String,
    occupe: Arc<Mutex<usize>>,
    message: Arc<Mutex<String>>,
    vignettes: std::collections::HashMap<PathBuf, egui::TextureHandle>,
    apercu: bool,
}

fn maintenant() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn date_texte() -> String {
    crate::papiers::date_du_jour()
}

impl CataloguePrix {
    pub fn new() -> Self {
        CataloguePrix { c: Arc::new(Mutex::new(charger("catalogue_prix"))), liens: String::new(), occupe: Arc::default(), message: Arc::default(), vignettes: Default::default(), apercu: false }
    }

    /// Lit (ou relit) les produits donnés, l'un après l'autre, en tâche de fond.
    fn lire(&self, ids: Vec<u64>, pages: Vec<(u64, String)>) {
        let (c, oc, msg) = (self.c.clone(), self.occupe.clone(), self.message.clone());
        *oc.lock().unwrap() += ids.len() + pages.len();
        std::thread::spawn(move || {
            let faire = |id: u64, html: Result<String, String>| {
                let url = c.lock().unwrap().produits.iter().find(|p| p.id == id).map(|p| p.url.clone()).unwrap_or_default();
                let image = match html {
                    Ok(h) => {
                        let i = lire_page(&h, &url);
                        let mut g = c.lock().unwrap();
                        g.produits.iter_mut().find(|p| p.id == id).and_then(|p| appliquer(p, i, maintenant()))
                    }
                    Err(e) => {
                        if let Some(p) = c.lock().unwrap().produits.iter_mut().find(|p| p.id == id) {
                            p.erreur = e;
                        }
                        None
                    }
                };
                if let Some(u) = image {
                    let f = telecharger_image(&u, id);
                    if let Some(p) = c.lock().unwrap().produits.iter_mut().find(|p| p.id == id) {
                        p.image = f.or(p.image.clone());
                    }
                }
                *oc.lock().unwrap() -= 1;
                enregistrer("catalogue_prix", &*c.lock().unwrap());
            };
            for (id, html) in pages {
                faire(id, Ok(html));
            }
            for (k, id) in ids.iter().enumerate() {
                let url = c.lock().unwrap().produits.iter().find(|p| p.id == *id).map(|p| p.url.clone()).unwrap_or_default();
                *msg.lock().unwrap() = format!("Lecture des produits : {}/{}…", k + 1, ids.len());
                faire(*id, telecharger_page(&url));
                // pas trop vite : on reste poli avec les magasins
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
            let g = c.lock().unwrap();
            let baisses = g.produits.iter().filter(|p| p.ancien_prix.zip(p.prix).is_some_and(|(a, v)| v < a)).count();
            *msg.lock().unwrap() = if baisses > 0 { format!("✔ À jour · 📉 {baisses} prix en baisse !") } else { "✔ À jour".into() };
        });
    }

    fn ajouter(&mut self, url: &str) -> u64 {
        let mut g = self.c.lock().unwrap();
        if let Some(p) = g.produits.iter().find(|p| p.url == url) {
            return p.id;
        }
        let id = g.prochain.max(1);
        g.prochain = id + 1;
        g.produits.push(Produit { id, url: url.to_string(), magasin: magasin_de(url), ..Default::default() });
        id
    }

    fn faire_pdf(&mut self) {
        let c = self.c.lock().unwrap().clone();
        let (pages, ordre) = mise_en_page(&c, &date_texte());
        let images: Vec<RgbaImage> = ordre.iter().map(|&k| c.produits[k].image.as_ref().and_then(|f| image::open(f).ok()).map(|i| i.to_rgba8()).unwrap_or_else(|| RgbaImage::from_pixel(4, 4, image::Rgba([244, 244, 246, 255])))).collect();
        let octets = pdf_complet(&pages, None, &images);
        let nom = format!("{}.pdf", c.titre.chars().map(|x| if x.is_alphanumeric() || x == ' ' || x == '-' { x } else { '_' }).collect::<String>().trim());
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(nom).save_file() {
            *self.message.lock().unwrap() = match std::fs::write(&p, octets) {
                Ok(_) => {
                    crate::outil::ouvrir(&p);
                    format!("✔ PDF enregistré : {}", p.display())
                }
                Err(e) => format!("✖ {e}"),
            };
        }
    }
}

impl Outil for CataloguePrix {
    fn nom(&self) -> &'static str {
        "🛒 Catalogue de prix"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        // pages envoyées par le navigateur
        let recus: Vec<(String, String)> = std::mem::take(&mut *RECUS.lock().unwrap());
        if !recus.is_empty() {
            let pages: Vec<(u64, String)> = recus.into_iter().map(|(u, h)| (self.ajouter(&u), h)).collect();
            self.lire(vec![], pages);
        }
        let avant = self.c.lock().unwrap().clone();
        ui.heading("🛒 Catalogue de prix");
        ui.weak("Colle les liens des produits qui t'intéressent (n'importe quel magasin en ligne). Outils lit le nom, le prix et la photo, surveille les baisses, et fait un PDF bien rangé.");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::multiline(&mut self.liens).desired_rows(2).desired_width(ui.available_width() - 150.0).hint_text("https://www.magasin.fr/produit…\n(un lien par ligne, ou plusieurs collés d'un coup)"));
            if ui.button(RichText::new("➕ Ajouter").strong()).clicked() {
                let re = regex::Regex::new(r#"https?://[^\s"'<>]+"#).unwrap();
                let urls: Vec<String> = re.find_iter(&self.liens).map(|m| m.as_str().trim_end_matches([',', ';', ')']).to_string()).collect();
                let ids: Vec<u64> = urls.iter().map(|u| self.ajouter(u)).collect();
                if !ids.is_empty() {
                    self.lire(ids, vec![]);
                }
                self.liens.clear();
            }
        });
        let n_occupe = *self.occupe.lock().unwrap();
        ui.horizontal_wrapped(|ui| {
            if n_occupe > 0 {
                ui.spinner();
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
            }
            ui.label(self.message.lock().unwrap().clone());
        });
        let mut g = self.c.lock().unwrap();
        ui.horizontal_wrapped(|ui| {
            ui.label("Titre :");
            ui.add(egui::TextEdit::singleline(&mut g.titre).desired_width(220.0));
            ui.label("Ranger :");
            egui::ComboBox::from_id_salt("cat_regrouper").selected_text(["par catégorie", "par magasin", "tout ensemble"][g.regrouper.min(2) as usize]).show_ui(ui, |ui| {
                for (k, t) in ["par catégorie", "par magasin", "tout ensemble"].iter().enumerate() {
                    ui.selectable_value(&mut g.regrouper, k as u8, *t);
                }
            });
            egui::ComboBox::from_id_salt("cat_tri").selected_text(["moins cher d'abord", "plus cher d'abord", "par nom"][g.tri.min(2) as usize]).show_ui(ui, |ui| {
                for (k, t) in ["moins cher d'abord", "plus cher d'abord", "par nom"].iter().enumerate() {
                    ui.selectable_value(&mut g.tri, k as u8, *t);
                }
            });
            ui.checkbox(&mut g.totaux, "totaux");
        });
        let ids: Vec<u64> = g.produits.iter().map(|p| p.id).collect();
        let total: f64 = g.produits.iter().filter_map(|p| p.prix.map(|v| v * p.quantite.max(1) as f64)).sum();
        let devise = g.produits.first().map(|p| p.devise.clone()).unwrap_or_default();
        let categories: Vec<String> = {
            let mut v: Vec<String> = g.produits.iter().map(|p| p.categorie.trim().to_string()).filter(|c| !c.is_empty()).collect();
            v.sort();
            v.dedup();
            v
        };
        drop(g);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!ids.is_empty() && n_occupe == 0, egui::Button::new("🔄 Relever tous les prix")).clicked() {
                self.lire(ids.clone(), vec![]);
            }
            if ui.add_enabled(!ids.is_empty(), egui::Button::new(RichText::new("📄 Faire le PDF").strong())).clicked() {
                self.faire_pdf();
            }
            ui.toggle_value(&mut self.apercu, "👁 Aperçu");
            if !ids.is_empty() {
                ui.label(RichText::new(format!("Total : {}", euros(total, &devise))).strong());
            }
        });
        if self.apercu {
            let c = self.c.lock().unwrap().clone();
            let (pages, _) = mise_en_page(&c, &date_texte());
            egui::ScrollArea::vertical().id_salt("apercu_catalogue").max_height(420.0).show(ui, |ui| apercu_pages(ui, &pages[..pages.len().min(2)], None));
        }
        ui.separator();
        let mut supprimer = None;
        let mut relire = None;
        egui::ScrollArea::vertical().id_salt("produits").show(ui, |ui| {
            let mut g = self.c.lock().unwrap();
            if g.produits.is_empty() {
                ui.label("Aucun produit pour l'instant : colle un lien au-dessus.");
            }
            for p in g.produits.iter_mut() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let tex = p.image.as_ref().filter(|f| f.exists()).and_then(|f| {
                            if !self.vignettes.contains_key(f) {
                                let t = image::open(f).ok()?.thumbnail(96, 96).to_rgba8();
                                let tx = ui.ctx().load_texture(f.to_string_lossy(), egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default());
                                self.vignettes.insert(f.clone(), tx);
                            }
                            self.vignettes.get(f).cloned()
                        });
                        match tex {
                            Some(t) => {
                                ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(64.0, 64.0)).corner_radius(6.0));
                            }
                            None => {
                                if ui.add_sized([64.0, 64.0], egui::Button::new("🖼")).on_hover_text("Choisir une photo").clicked() {
                                    if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                                        p.image = Some(f);
                                    }
                                }
                            }
                        }
                        ui.vertical(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut p.nom).desired_width(f32::INFINITY).hint_text("nom du produit").font(egui::TextStyle::Body));
                            ui.horizontal_wrapped(|ui| {
                                let mut t = p.prix.map(|v| format!("{v:.2}")).unwrap_or_default();
                                if ui.add(egui::TextEdit::singleline(&mut t).desired_width(70.0).hint_text("prix")).changed() {
                                    p.prix = lire_prix(&t);
                                }
                                ui.label(if p.devise == "EUR" || p.devise.is_empty() { "€" } else { &p.devise });
                                if let (Some(a), Some(v)) = (p.ancien_prix, p.prix) {
                                    if v < a {
                                        ui.colored_label(Color32::from_rgb(90, 200, 110), format!("📉 était {}", euros(a, &p.devise)));
                                    } else {
                                        ui.colored_label(Color32::from_rgb(230, 110, 90), format!("📈 était {}", euros(a, &p.devise)));
                                    }
                                }
                                ui.label("×");
                                ui.add(egui::DragValue::new(&mut p.quantite).range(1..=99));
                                ui.add(egui::TextEdit::singleline(&mut p.categorie).desired_width(110.0).hint_text("catégorie"));
                                if !categories.is_empty() {
                                    egui::ComboBox::from_id_salt(("cat", p.id)).selected_text("").width(24.0).show_ui(ui, |ui| {
                                        for c in &categories {
                                            if ui.selectable_label(p.categorie == *c, c).clicked() {
                                                p.categorie = c.clone();
                                            }
                                        }
                                    });
                                }
                                ui.weak(format!("🏬 {}", p.magasin));
                                if p.dispo == Some(false) {
                                    ui.colored_label(Color32::from_rgb(230, 110, 90), "indisponible");
                                }
                                if !p.url.is_empty() && ui.small_button("🔗").on_hover_text(&p.url).clicked() {
                                    ui.ctx().open_url(egui::OpenUrl::new_tab(&p.url));
                                }
                                if !p.url.is_empty() && ui.small_button("🔄").on_hover_text("Relever le prix").clicked() {
                                    relire = Some(p.id);
                                }
                                if ui.small_button("🗑").clicked() {
                                    supprimer = Some(p.id);
                                }
                            });
                            if !p.erreur.is_empty() {
                                ui.colored_label(Color32::from_rgb(230, 150, 70), &p.erreur);
                            }
                        });
                    });
                });
            }
        });
        if let Some(id) = supprimer {
            self.c.lock().unwrap().produits.retain(|p| p.id != id);
        }
        if let Some(id) = relire {
            self.lire(vec![id], vec![]);
        }
        if ui.small_button("✏ Ajouter un produit à la main").clicked() {
            let mut g = self.c.lock().unwrap();
            let id = g.prochain.max(1);
            g.prochain = id + 1;
            g.produits.push(Produit { id, magasin: "Magasin".into(), ..Default::default() });
        }
        let apres = self.c.lock().unwrap().clone();
        if apres != avant {
            enregistrer("catalogue_prix", &apres);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prix() {
        assert_eq!(lire_prix("1 299,90 €"), Some(1299.9));
        assert_eq!(lire_prix("1,299.90"), Some(1299.9));
        assert_eq!(lire_prix("19.99"), Some(19.99));
        assert_eq!(lire_prix("24"), Some(24.0));
        assert_eq!(lire_prix("1.299"), Some(1299.0));
        assert_eq!(lire_prix("gratuit"), None);
        assert_eq!(euros(1299.9, "EUR"), "1 299,90 €");
        assert_eq!(euros(5.0, "EUR"), "5,00 €");
        assert_eq!(entites("Caf&eacute; &amp; th&#233; &#x27;bio&#x27;"), "Café & thé 'bio'");
        assert_eq!(magasin_de("https://www.boulanger.com/ref/1234"), "Boulanger");
        assert_eq!(magasin_de("https://www.fnac.fr/a1"), "Fnac");
    }

    #[test]
    fn pages() {
        // données structurées
        let h = r#"<html><head><title>X</title><script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"BreadcrumbList"},{"@type":"Product","name":"Casque Sans Fil &amp; Micro","image":["/img/casque.jpg"],"offers":{"@type":"Offer","price":"79.99","priceCurrency":"EUR","availability":"https://schema.org/InStock"}}]}</script></head></html>"#;
        let i = lire_page(h, "https://www.exemple.fr/p/casque");
        assert_eq!(i.nom, "Casque Sans Fil & Micro");
        assert_eq!(i.prix, Some(79.99));
        assert_eq!(i.image.as_deref(), Some("https://www.exemple.fr/img/casque.jpg"));
        assert_eq!(i.dispo, Some(true));
        // seulement des balises meta
        let h = r#"<meta property="og:title" content="Lampe d'architecte"><meta content="//cdn.x.com/l.webp" property="og:image"><meta property="product:price:amount" content="34,50">"#;
        let i = lire_page(h, "https://x.com/l");
        assert_eq!((i.nom.as_str(), i.prix, i.image.as_deref()), ("Lampe d'architecte", Some(34.5), Some("https://cdn.x.com/l.webp")));
        // rien de structuré : le titre et le premier prix
        let i = lire_page("<title>Tabouret bois</title><span>Prix : 1 249,00 €</span>", "https://y.fr/t");
        assert_eq!((i.nom.as_str(), i.prix), ("Tabouret bois", Some(1249.0)));
        // un produit qui change de prix garde l'ancien
        let mut p = Produit { id: 1, url: "https://www.fnac.com/x".into(), prix: Some(100.0), maj: 5, nom: "A".into(), ..Default::default() };
        appliquer(&mut p, Infos { nom: "B".into(), prix: Some(80.0), devise: "EUR".into(), image: None, dispo: Some(true) }, 10);
        assert_eq!((p.prix, p.ancien_prix, p.nom.as_str(), p.magasin.as_str()), (Some(80.0), Some(100.0), "A", "Fnac"));
    }

    #[test]
    fn pdf() {
        let mut c = Catalogue::default();
        for k in 0..8u64 {
            c.produits.push(Produit { id: k, nom: format!("Produit numéro {k} avec un nom assez long pour passer à la ligne"), prix: Some(10.0 + k as f64), magasin: if k % 2 == 0 { "Fnac".into() } else { "Darty".into() }, categorie: if k < 4 { "Cuisine".into() } else { "Jeux".into() }, image: (k % 3 == 0).then(|| PathBuf::from("x.png")), ..Default::default() });
        }
        let g = groupes(&c);
        assert_eq!(g.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["Cuisine", "Jeux"]);
        assert_eq!(c.produits[g[0].1[0]].prix, Some(10.0)); // moins cher d'abord
        let (pages, images) = mise_en_page(&c, "28 novembre 2026");
        assert_eq!(images.len(), 3);
        let imgs: Vec<RgbaImage> = images.iter().map(|_| RgbaImage::from_pixel(20, 10, image::Rgba([200, 30, 30, 255]))).collect();
        let o = pdf_complet(&pages, None, &imgs);
        assert!(o.windows(8).any(|w| w == b"/Img2 Do"));
        assert!(o.windows(6).any(|w| w == b"Cuisin"));
        // beaucoup de produits : plusieurs pages, numérotées
        for k in 8..40u64 {
            c.produits.push(Produit { id: k, nom: "Z".into(), prix: Some(1.0), ..Default::default() });
        }
        let (pages, _) = mise_en_page(&c, "x");
        assert!(pages.len() >= 3);
        if let Ok(d) = std::env::var("PAPIERS_TEST") {
            c.produits.truncate(8);
            let (pages, images) = mise_en_page(&c, "28 novembre 2026");
            let imgs: Vec<RgbaImage> = images.iter().map(|_| RgbaImage::from_fn(60, 40, |x, y| image::Rgba([(x * 4) as u8, (y * 6) as u8, 160, 255]))).collect();
            std::fs::write(format!("{d}/catalogue.pdf"), pdf_complet(&pages, None, &imgs)).unwrap();
        }
    }
}
