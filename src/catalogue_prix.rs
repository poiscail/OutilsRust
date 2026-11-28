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
    /// prix normal (barré) quand le produit est en promo
    pub prix_normal: Option<f64>,
    /// rayon du magasin (fil d'Ariane ou type de produit)
    pub categorie: String,
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
        // prix barré : priceSpecification de type « StrikethroughPrice » ou « ListPrice »
        if i.prix_normal.is_none() {
            let specs = &offre["priceSpecification"];
            let liste: Vec<&serde_json::Value> = if specs.is_array() { specs.as_array().unwrap().iter().collect() } else { vec![specs] };
            i.prix_normal = liste.iter().find(|x| x["priceType"].as_str().is_some_and(|t| t.contains("Strikethrough") || t.contains("ListPrice") || t.contains("MSRP"))).and_then(|x| texte_json(&x["price"])).and_then(|x| lire_prix(&x));
        }
        if i.categorie.is_empty() {
            i.categorie = p["category"].as_str().map(|c| entites(c.rsplit(['>', '/']).next().unwrap_or(c).trim())).unwrap_or_default();
        }
    }
    // rayon : avant-dernier élément du fil d'Ariane
    if i.categorie.is_empty() {
        for c in re_ld.captures_iter(html) {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(c[1].trim()) else { continue };
            let liste = if v["@type"] == "BreadcrumbList" { Some(&v) } else { v["@graph"].as_array().and_then(|g| g.iter().find(|x| x["@type"] == "BreadcrumbList")) };
            if let Some(items) = liste.and_then(|b| b["itemListElement"].as_array()).filter(|a| a.len() >= 2) {
                let x = &items[items.len() - 2];
                let nom = x["name"].as_str().or(x["item"]["name"].as_str()).unwrap_or("");
                if !nom.is_empty() && !nom.eq_ignore_ascii_case("accueil") && !nom.eq_ignore_ascii_case("home") {
                    i.categorie = entites(nom);
                }
            }
        }
    }
    // 2. balises meta (Open Graph, microdonnées)
    let re_meta = regex::Regex::new(r#"(?is)<meta\s[^>]*>"#).unwrap();
    let mut promo_meta: Option<f64> = None;
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
            "product:sale_price:amount" | "og:sale_price:amount" => promo_meta = lire_prix(&val),
            "product:original_price:amount" | "og:original_price:amount" | "product:regular_price:amount" if i.prix_normal.is_none() => i.prix_normal = lire_prix(&val),
            _ => {}
        }
    }
    // prix soldé annoncé à part : le prix « normal » devient le prix barré
    if let (Some(s), Some(p)) = (promo_meta, i.prix) {
        if s < p {
            i.prix_normal = Some(p);
            i.prix = Some(s);
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
    // prix barré affiché dans la page : <del>, <s>, ou une classe « old-price », « prix-barre »…
    if i.prix_normal.is_none() {
        if let Some(p) = i.prix {
            let re_barre = regex::Regex::new(r#"(?is)(?:<(?:del|s|strike)\b[^>]*>|class\s*=\s*["'][^"']*(?:old[-_]?price|price[-_]?old|was[-_]?price|prix[-_]?barr|price[-_]?before|crossed|strike|compare[-_]?at|initial[-_]?price|price--compare|reference[-_]?price|prix[-_]?initial|ancien[-_]?prix)[^"']*["'][^>]*>)(.{0,240}?)(?:</(?:del|s|strike|span|div|p)>)"#).unwrap();
            let re_nb = regex::Regex::new(r"(\d{1,3}(?:[ \u{a0}.]\d{3})*(?:[,.]\d{1,2})?)\s*(?:€|&euro;|&#8364;|EUR)").unwrap();
            let re_balise = regex::Regex::new(r"<[^>]*>").unwrap();
            i.prix_normal = re_barre.captures_iter(html).filter_map(|c| {
                let t = entites(&re_balise.replace_all(&c[1], " "));
                re_nb.captures(&t).and_then(|n| lire_prix(&n[1]))
            }).find(|b| *b > p * 1.01 && *b < p * 10.0);
        }
    }
    // prix barré dans les données de la page (Shopify et autres : compareAtPrice, compare_at_price…)
    if i.prix_normal.is_none() {
        if let Some(p) = i.prix {
            let html2 = html.replace("&quot;", "\"");
            let re_cmp = regex::Regex::new(r#"(?i)"?(?:compare_?at_?price|regular_?price|old_?price|was_?price|list_?price)"?\s*[:=]\s*"?(\d+(?:\.\d+)?)"#).unwrap();
            i.prix_normal = re_cmp.captures_iter(&html2).filter_map(|c| c[1].parse::<f64>().ok()).map(|b| if b > p * 40.0 && !a_des_decimales(b) { b / 100.0 } else { b }).find(|b| *b > p * 1.01 && *b < p * 10.0);
        }
    }
    if i.prix_normal.is_some_and(|b| i.prix.is_none_or(|p| b <= p * 1.005)) {
        i.prix_normal = None;
    }
    i.nom = i.nom.chars().take(160).collect();
    i
}

/// Le nombre a des décimales (sinon un très grand prix entier est sans doute en centimes).
fn a_des_decimales(b: f64) -> bool {
    b.fract() != 0.0
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

/// Télécharge la photo, la réduit et la garde.
fn telecharger_image(url: &str, id: u64) -> Option<PathBuf> {
    // les CDN donnent souvent une vignette minuscule (« width=100 ») : on demande plus grand
    let re_l = regex::Regex::new(r"([?&])(width|w)=\d{1,3}\b").unwrap();
    let url = re_l.replace(url, "${1}${2}=600").into_owned();
    let url = url.as_str();
    let r = agent_web().get(url).set("Accept", "image/avif,image/webp,image/*").call().ok()?;
    let mut b = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(r.into_reader(), 12 << 20), &mut b).ok()?;
    // fond blanc sous la transparence, puis JPEG (léger même pour des centaines de produits)
    let img = image::load_from_memory(&b).ok()?.thumbnail(360, 360).to_rgba8();
    let rgb = image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let q = img.get_pixel(x, y);
        let a = q[3] as u32;
        image::Rgb([0, 1, 2].map(|k| ((q[k] as u32 * a + 255 * (255 - a)) / 255) as u8))
    });
    let _ = std::fs::create_dir_all(dossier_images());
    let p = dossier_images().join(format!("{id}.jpg"));
    let f = std::fs::File::create(&p).ok()?;
    image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 85).encode_image(&rgb).ok()?;
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
    /// prix barré : le produit est en promo
    pub prix_normal: Option<f64>,
    pub devise: String,
    pub image: Option<PathBuf>,
    pub image_url: String,
    pub magasin: String,
    pub categorie: String,
    pub dispo: Option<bool>,
    pub quantite: u32,
    /// prix du relevé précédent, s'il a changé
    pub ancien_prix: Option<f64>,
    pub maj: i64,
    pub erreur: String,
}

impl Default for Produit {
    fn default() -> Self {
        Produit { id: 0, url: String::new(), nom: String::new(), prix: None, prix_normal: None, devise: "EUR".into(), image: None, image_url: String::new(), magasin: String::new(), categorie: String::new(), dispo: None, quantite: 1, ancien_prix: None, maj: 0, erreur: String::new() }
    }
}

impl Produit {
    /// Remise en % si le produit est en promo (prix barré, ou baisse depuis le dernier relevé).
    pub fn remise(&self) -> Option<f64> {
        let p = self.prix?;
        let avant = self.prix_normal.or(self.ancien_prix.filter(|a| *a > p))?;
        (avant > p * 1.005).then(|| (1.0 - p / avant) * 100.0)
    }
    pub fn avant(&self) -> Option<f64> {
        let p = self.prix?;
        self.prix_normal.or(self.ancien_prix).filter(|a| *a > p * 1.005)
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
    /// 0 = moins cher d'abord, 1 = plus cher d'abord, 2 = par nom, 3 = plus grosse promo d'abord
    pub tri: u8,
    pub totaux: bool,
    /// une section « En promo » au début du PDF
    pub promos_d_abord: bool,
    pub seulement_promos: bool,
    /// magasins explorés en entier : (adresse, nombre maximum de produits)
    pub magasins: Vec<(String, usize)>,
    pub prochain: u64,
}

impl Default for Catalogue {
    fn default() -> Self {
        Catalogue { titre: "Les prix de mes magasins".into(), produits: vec![], regrouper: 1, tri: 0, totaux: false, promos_d_abord: true, seulement_promos: false, magasins: vec![], prochain: 1 }
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
    p.prix_normal = i.prix_normal;
    p.devise = i.devise;
    p.dispo = i.dispo;
    p.maj = maintenant;
    p.erreur = if i.prix.is_none() { "prix introuvable sur la page : remplis-le à la main".into() } else { String::new() };
    if p.magasin.is_empty() {
        p.magasin = magasin_de(&p.url);
    }
    if p.categorie.is_empty() {
        p.categorie = i.categorie;
    }
    if let Some(u) = &i.image {
        p.image_url = u.clone();
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

fn trier(c: &Catalogue, v: &mut [usize], tri: u8) {
    v.sort_by(|&a, &b| {
        let (pa, pb) = (&c.produits[a], &c.produits[b]);
        match tri {
            0 => pa.prix.unwrap_or(f64::MAX).total_cmp(&pb.prix.unwrap_or(f64::MAX)),
            1 => pb.prix.unwrap_or(-1.0).total_cmp(&pa.prix.unwrap_or(-1.0)),
            3 => pb.remise().unwrap_or(0.0).total_cmp(&pa.remise().unwrap_or(0.0)).then(pa.prix.unwrap_or(f64::MAX).total_cmp(&pb.prix.unwrap_or(f64::MAX))),
            _ => pa.nom.to_lowercase().cmp(&pb.nom.to_lowercase()),
        }
    });
}

/// Groupes de produits (titre, indices) parmi `sous`, selon le réglage, triés.
pub fn groupes(c: &Catalogue, sous: &[usize]) -> Vec<(String, Vec<usize>)> {
    let mut g: Vec<(String, Vec<usize>)> = vec![];
    for &k in sous {
        let p = &c.produits[k];
        let cle = match c.regrouper {
            0 => if p.categorie.trim().is_empty() { "Autres produits".to_string() } else { p.categorie.trim().to_string() },
            1 => if p.magasin.is_empty() { "Autre".into() } else { p.magasin.clone() },
            _ => String::new(),
        };
        match g.iter_mut().find(|x| x.0 == cle) {
            Some(x) => x.1.push(k),
            None => g.push((cle, vec![k])),
        }
    }
    g.sort_by(|a, b| (a.0 == "Autres produits").cmp(&(b.0 == "Autres produits")).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    for (_, v) in &mut g {
        trier(c, v, c.tri);
    }
    g
}

const ROSE: [u8; 3] = [214, 51, 120];
const ROUGE: [u8; 3] = [220, 38, 38];
const VERT: [u8; 3] = [30, 150, 70];
const NOIR: [u8; 3] = [25, 25, 30];
const GRIS: [u8; 3] = [110, 110, 120];

struct Gabarit {
    colonnes: usize,
    hc: f32,
    himg: f32,
    tnom: f32,
    lignes: usize,
    tprix: f32,
}

/// Pages du PDF et ordre des images (indices de produits).
pub fn mise_en_page(c: &Catalogue, date: &str) -> (Vec<Vec<Op>>, Vec<usize>) {
    let (marge, haut, bas) = (32.0, 38.0, A4.1 - 36.0);
    let promos: Vec<usize> = (0..c.produits.len()).filter(|&k| c.produits[k].remise().is_some()).collect();
    let section_promos = (c.promos_d_abord || c.seulement_promos) && !promos.is_empty();
    let reste: Vec<usize> = if c.seulement_promos { vec![] } else { (0..c.produits.len()).filter(|k| !section_promos || !promos.contains(k)).collect() };
    let nb = if c.seulement_promos { promos.len() } else { c.produits.len() };
    // beaucoup de produits : cartes plus petites, 4 par ligne
    let g = if nb > 30 { Gabarit { colonnes: 4, hc: 186.0, himg: 86.0, tnom: 8.5, lignes: 2, tprix: 13.0 } } else { Gabarit { colonnes: 3, hc: 228.0, himg: 112.0, tnom: 9.5, lignes: 3, tprix: 16.0 } };
    let esp = 10.0;
    let lc = (A4.0 - 2.0 * marge - esp * (g.colonnes as f32 - 1.0)) / g.colonnes as f32;
    let mut pages: Vec<Vec<Op>> = vec![vec![]];
    let mut images: Vec<usize> = vec![];
    let total_qte = |v: &[usize]| v.iter().filter_map(|&k| c.produits[k].prix.map(|p| p * c.produits[k].quantite.max(1) as f64)).sum::<f64>();
    let devise = c.produits.first().map(|p| p.devise.clone()).unwrap_or_default();
    let nb_magasins = {
        let mut m: Vec<&str> = c.produits.iter().map(|p| p.magasin.as_str()).filter(|m| !m.is_empty()).collect();
        m.sort();
        m.dedup();
        m.len()
    };
    // en-tête
    let mut y = haut;
    {
        let p = pages.last_mut().unwrap();
        p.push(Op::Rect { x: 0.0, y: 0.0, l: A4.0, h: 6.0, c: ROSE });
        p.push(Op::Texte { x: marge, y: y + 18.0, taille: 20.0, gras: true, c: NOIR, s: c.titre.clone() });
        y += 30.0;
        let mut sous = format!("{} produit(s) de {nb_magasins} magasin(s) · prix relevés le {date}", c.produits.len());
        if !promos.is_empty() {
            sous += &format!(" · {} en promo", promos.len());
        }
        if c.totaux && !c.seulement_promos {
            sous += &format!(" · total {}", euros(total_qte(&(0..c.produits.len()).collect::<Vec<_>>()), &devise));
        }
        p.push(Op::Texte { x: marge, y: y + 10.0, taille: 10.0, gras: false, c: GRIS, s: sous });
        y += 24.0;
    }
    let mut sections: Vec<(String, Vec<usize>, bool)> = vec![];
    if section_promos {
        let mut v = promos.clone();
        trier(c, &mut v, 3);
        sections.push((format!("EN PROMO — {} produit(s), les plus grosses remises d'abord", v.len()), v, true));
    }
    for (t, v) in groupes(c, &reste) {
        sections.push((t, v, false));
    }
    for (titre, ids, est_promo) in sections {
        if y + 30.0 + g.hc > bas {
            pages.push(vec![]);
            y = haut;
        }
        if !titre.is_empty() {
            let p = pages.last_mut().unwrap();
            let (fond, encre) = if est_promo { ([254, 226, 226], ROUGE) } else { ([248, 228, 238], ROSE) };
            p.push(Op::Rect { x: marge, y, l: A4.0 - 2.0 * marge, h: 22.0, c: fond });
            p.push(Op::Texte { x: marge + 8.0, y: y + 15.5, taille: 12.0, gras: true, c: encre, s: titre.clone() });
            if c.totaux && !est_promo {
                let t = format!("{} article(s) · {}", ids.len(), euros(total_qte(&ids), &devise));
                p.push(Op::Texte { x: A4.0 - marge - 8.0 - largeur(&t, 10.0, false), y: y + 15.0, taille: 10.0, gras: false, c: GRIS, s: t });
            } else if !est_promo {
                let n = ids.iter().filter(|&&k| c.produits[k].remise().is_some()).count();
                let t = if n > 0 { format!("{} produit(s) · {n} en promo", ids.len()) } else { format!("{} produit(s)", ids.len()) };
                p.push(Op::Texte { x: A4.0 - marge - 8.0 - largeur(&t, 10.0, false), y: y + 15.0, taille: 10.0, gras: false, c: GRIS, s: t });
            }
            y += 30.0;
        }
        let moins_cher = if ids.len() > 1 && ids.len() <= 40 && c.tri != 2 && !est_promo { ids.iter().filter(|&&k| c.produits[k].prix.is_some()).min_by(|&&a, &&b| c.produits[a].prix.unwrap().total_cmp(&c.produits[b].prix.unwrap())).copied() } else { None };
        for (n, &k) in ids.iter().enumerate() {
            let col = n % g.colonnes;
            if col == 0 && n > 0 {
                y += g.hc + esp;
            }
            if y + g.hc > bas {
                pages.push(vec![]);
                y = haut;
            }
            let x = marge + col as f32 * (lc + esp);
            let pr = &c.produits[k];
            let remise = pr.remise();
            let p = pages.last_mut().unwrap();
            // carte (bord rouge si promo)
            p.push(Op::Rect { x, y, l: lc, h: g.hc, c: if remise.is_some() { ROUGE } else { [230, 230, 236] } });
            let bord = if remise.is_some() { 1.6 } else { 1.0 };
            p.push(Op::Rect { x: x + bord, y: y + bord, l: lc - 2.0 * bord, h: g.hc - 2.0 * bord, c: [255, 255, 255] });
            if pr.image.is_some() {
                p.push(Op::Image { x: x + 7.0, y: y + 7.0, l: lc - 14.0, h: g.himg, k: images.len() });
                images.push(k);
            } else {
                p.push(Op::Rect { x: x + 7.0, y: y + 7.0, l: lc - 14.0, h: g.himg, c: [244, 244, 246] });
                p.push(Op::Texte { x: x + lc / 2.0 - largeur("pas de photo", 8.0, false) / 2.0, y: y + 7.0 + g.himg / 2.0, taille: 8.0, gras: false, c: GRIS, s: "pas de photo".into() });
            }
            if let Some(r) = remise {
                let t = format!("-{:.0} %", r.round());
                let l = largeur(&t, 10.0, true) + 10.0;
                p.push(Op::Rect { x: x + 5.0, y: y + 5.0, l, h: 16.0, c: ROUGE });
                p.push(Op::Texte { x: x + 10.0, y: y + 16.5, taille: 10.0, gras: true, c: [255, 255, 255], s: t });
            }
            let mut ty = y + g.himg + 22.0;
            let lignes = couper(&pr.nom, g.tnom, true, lc - 14.0);
            for (i, l) in lignes.iter().take(g.lignes).enumerate() {
                let l = if i + 1 == g.lignes && lignes.len() > g.lignes { format!("{}…", l.chars().take(l.chars().count().saturating_sub(1)).collect::<String>()) } else { l.clone() };
                p.push(Op::Texte { x: x + 7.0, y: ty, taille: g.tnom, gras: true, c: NOIR, s: l });
                ty += g.tnom * 1.25;
            }
            let prix = pr.prix.map(|v| euros(v, &pr.devise)).unwrap_or_else(|| "prix inconnu".into());
            let yp = y + g.hc - 28.0;
            p.push(Op::Texte { x: x + 7.0, y: yp, taille: g.tprix, gras: true, c: if remise.is_some() { ROUGE } else if pr.prix.is_some() { NOIR } else { GRIS }, s: prix.clone() });
            if let Some(a) = pr.avant() {
                // ancien prix barré, à côté
                let t = euros(a, &pr.devise);
                let xa = x + 12.0 + largeur(&prix, g.tprix, true);
                let la = largeur(&t, 9.0, false);
                if xa + la < x + lc - 4.0 {
                    p.push(Op::Texte { x: xa, y: yp, taille: 9.0, gras: false, c: GRIS, s: t });
                    p.push(Op::Ligne { x1: xa - 1.0, y1: yp - 3.2, x2: xa + la + 1.0, y2: yp - 3.2, e: 0.9, c: GRIS });
                }
            }
            let mut bas_t = pr.magasin.clone();
            if pr.quantite > 1 {
                bas_t += &format!(" · ×{}", pr.quantite);
            }
            if pr.dispo == Some(false) {
                bas_t += " · indisponible";
            }
            p.push(Op::Texte { x: x + 7.0, y: y + g.hc - 11.0, taille: 8.0, gras: false, c: if pr.dispo == Some(false) { ROUGE } else { GRIS }, s: bas_t });
            if moins_cher == Some(k) {
                let t = "LE MOINS CHER";
                let l = largeur(t, 7.0, true) + 10.0;
                p.push(Op::Rect { x: x + lc - l - 5.0, y: y + 5.0, l, h: 13.0, c: VERT });
                p.push(Op::Texte { x: x + lc - l, y: y + 14.5, taille: 7.0, gras: true, c: [255, 255, 255], s: t.into() });
            }
        }
        if !ids.is_empty() {
            y += g.hc + esp + 8.0;
        }
    }
    let n = pages.len();
    for (k, p) in pages.iter_mut().enumerate() {
        let t = format!("{} / {n}", k + 1);
        p.push(Op::Texte { x: A4.0 - marge - largeur(&t, 8.0, false), y: A4.1 - 16.0, taille: 8.0, gras: false, c: GRIS, s: t });
    }
    (pages, images)
}

// ---------------------------------------------------------------------------------------------------
// L'outil
// ---------------------------------------------------------------------------------------------------

/// Pages reçues du navigateur d'Outils (bouton 🛒) : (adresse, HTML).
static RECUS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

pub fn recevoir(url: &str, html: String) {
    RECUS.lock().unwrap().push((url.to_string(), html));
}

fn maintenant() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Ajoute ou met à jour le produit de cette adresse ; renvoie (id, image à télécharger).
fn integrer(c: &mut Catalogue, url: &str, magasin: Option<&str>, i: Infos) -> (u64, Option<String>) {
    let cle = url.split(['?', '#']).next().unwrap_or(url).to_string();
    let k = match c.produits.iter().position(|p| p.url.split(['?', '#']).next().unwrap_or(&p.url) == cle) {
        Some(k) => k,
        None => {
            let id = c.prochain.max(1);
            c.prochain = id + 1;
            c.produits.push(Produit { id, url: url.to_string(), magasin: magasin.map(String::from).unwrap_or_else(|| magasin_de(url)), ..Default::default() });
            c.produits.len() - 1
        }
    };
    let id = c.produits[k].id;
    (id, appliquer(&mut c.produits[k], i, maintenant()))
}

pub struct CataloguePrix {
    c: Arc<Mutex<Catalogue>>,
    onglet: usize,
    liens: String,
    site: String,
    max: usize,
    occupe: Arc<std::sync::atomic::AtomicBool>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    message: Arc<Mutex<String>>,
    vignettes: std::collections::HashMap<PathBuf, Option<egui::TextureHandle>>,
    apercu: bool,
    filtre: String,
    filtre_promos: bool,
    filtre_magasin: String,
    edite: Option<u64>,
}

impl CataloguePrix {
    pub fn new() -> Self {
        CataloguePrix {
            c: Arc::new(Mutex::new(charger("catalogue_prix"))),
            onglet: 0,
            liens: String::new(),
            site: String::new(),
            max: 300,
            occupe: Arc::default(),
            stop: Arc::default(),
            message: Arc::default(),
            vignettes: Default::default(),
            apercu: false,
            filtre: String::new(),
            filtre_promos: false,
            filtre_magasin: String::new(),
            edite: None,
        }
    }

    /// Travail de fond : magasins entiers, liens, pages reçues du navigateur ; puis les photos.
    fn travailler(&self, magasins: Vec<(String, usize)>, liens: Vec<String>, pages: Vec<(String, String)>) {
        use std::sync::atomic::Ordering::Relaxed;
        if self.occupe.swap(true, Relaxed) {
            *self.message.lock().unwrap() = "Attends la fin du travail en cours (ou ⏹ Arrêter).".into();
            return;
        }
        self.stop.store(false, Relaxed);
        let (c, oc, st, msg) = (self.c.clone(), self.occupe.clone(), self.stop.clone(), self.message.clone());
        std::thread::spawn(move || {
            let dire = |s: String| *msg.lock().unwrap() = s;
            let mut images: Vec<(u64, String)> = vec![];
            let mut bilan: Vec<String> = vec![];
            for (u, h) in pages {
                let i = lire_page(&h, &u);
                let (id, img) = integrer(&mut c.lock().unwrap(), &u, None, i);
                images.extend(img.map(|x| (id, x)));
            }
            for (site, max) in magasins {
                let nom = magasin_de(&crate::magasins::origine(&site));
                let mut n_promos = 0;
                let r = crate::magasins::explorer(&site, max, &st, &|s| dire(format!("🏬 {nom} : {s}")), &mut |t| {
                    if t.infos.prix_normal.is_some() {
                        n_promos += 1;
                    }
                    let (id, img) = integrer(&mut c.lock().unwrap(), &t.url, Some(&nom), t.infos);
                    images.extend(img.map(|x| (id, x)));
                });
                bilan.push(match r {
                    Ok(n) => format!("{nom} : {n} produits, {n_promos} en promo"),
                    Err(e) => format!("{nom} : {e}"),
                });
                enregistrer("catalogue_prix", &*c.lock().unwrap());
            }
            let n_liens = liens.len();
            for (k, u) in liens.into_iter().enumerate() {
                if st.load(Relaxed) {
                    break;
                }
                dire(format!("Lecture des produits : {}/{n_liens}…", k + 1));
                if let Some(t) = crate::magasins::produit_shopify(&u) {
                    let (id, img) = integrer(&mut c.lock().unwrap(), &u, None, t.infos);
                    images.extend(img.map(|x| (id, x)));
                    continue;
                }
                match telecharger_page(&u) {
                    Ok(h) => {
                        let i = lire_page(&h, &u);
                        let (id, img) = integrer(&mut c.lock().unwrap(), &u, None, i);
                        images.extend(img.map(|x| (id, x)));
                    }
                    Err(e) => {
                        let (id, _) = integrer(&mut c.lock().unwrap(), &u, None, Infos::default());
                        if let Some(p) = c.lock().unwrap().produits.iter_mut().find(|p| p.id == id) {
                            p.erreur = e;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(600));
            }
            let n_img = images.len();
            for (k, (id, u)) in images.into_iter().enumerate() {
                if st.load(Relaxed) {
                    break;
                }
                if k % 5 == 0 {
                    dire(format!("Photos : {}/{n_img}…", k + 1));
                }
                if let Some(f) = telecharger_image(&u, id) {
                    if let Some(p) = c.lock().unwrap().produits.iter_mut().find(|p| p.id == id) {
                        p.image = Some(f);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            enregistrer("catalogue_prix", &*c.lock().unwrap());
            let g = c.lock().unwrap();
            let promos = g.produits.iter().filter(|p| p.remise().is_some()).count();
            let mut fin = format!("✔ Terminé · {} produits · 🔥 {promos} en promo", g.produits.len());
            if !bilan.is_empty() {
                fin += &format!(" ({})", bilan.join(" ; "));
            }
            dire(fin);
            oc.store(false, Relaxed);
        });
    }

    fn faire_pdf(&mut self) {
        let c = self.c.lock().unwrap().clone();
        let (pages, ordre) = mise_en_page(&c, &crate::papiers::date_du_jour());
        let images: Vec<RgbaImage> = ordre.iter().map(|&k| c.produits[k].image.as_ref().and_then(|f| image::open(f).ok()).map(|i| i.to_rgba8()).unwrap_or_else(|| RgbaImage::from_pixel(4, 4, image::Rgba([244, 244, 246, 255])))).collect();
        let octets = pdf_complet(&pages, None, &images);
        let nom = format!("{}.pdf", c.titre.chars().map(|x| if x.is_alphanumeric() || x == ' ' || x == '-' { x } else { '_' }).collect::<String>().trim());
        if let Some(p) = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(nom).save_file() {
            *self.message.lock().unwrap() = match std::fs::write(&p, octets) {
                Ok(_) => {
                    crate::outil::ouvrir(&p);
                    format!("✔ PDF enregistré ({} pages) : {}", pages.len(), p.display())
                }
                Err(e) => format!("✖ {e}"),
            };
        }
    }

    fn vignette(&mut self, ctx: &egui::Context, f: &PathBuf) -> Option<egui::TextureHandle> {
        self.vignettes
            .entry(f.clone())
            .or_insert_with(|| {
                image::open(f).ok().map(|i| {
                    let t = i.thumbnail(96, 96).to_rgba8();
                    ctx.load_texture(f.to_string_lossy(), egui::ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw()), Default::default())
                })
            })
            .clone()
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
        use std::sync::atomic::Ordering::Relaxed;
        let recus: Vec<(String, String)> = std::mem::take(&mut *RECUS.lock().unwrap());
        if !recus.is_empty() {
            self.travailler(vec![], vec![], recus);
        }
        let avant = self.c.lock().unwrap().clone();
        let occupe = self.occupe.load(Relaxed);
        ui.heading("🛒 Catalogue de prix");
        ui.weak("Donne les sites de tes magasins : Outils fait le tour de leurs produits, relève tous les prix, repère les promos et fait un PDF rangé, avec les promos en avant.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.onglet, 0, RichText::new("🏬 Des magasins entiers").strong());
            ui.selectable_value(&mut self.onglet, 1, RichText::new("🔗 Des produits précis").strong());
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            if self.onglet == 0 {
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.site).desired_width(280.0).hint_text("https://www.mon-magasin.fr (ou une page de rayon)"));
                    ui.label("jusqu'à");
                    ui.add(egui::DragValue::new(&mut self.max).range(20..=5000).speed(10));
                    ui.label("produits");
                    if ui.add_enabled(!occupe && !self.site.trim().is_empty(), egui::Button::new(RichText::new("🔎 Explorer le magasin").strong())).clicked() {
                        let s = self.site.trim().to_string();
                        let mut g = self.c.lock().unwrap();
                        g.magasins.retain(|m| crate::magasins::origine(&m.0) != crate::magasins::origine(&s));
                        g.magasins.push((s.clone(), self.max));
                        drop(g);
                        self.travailler(vec![(s, self.max)], vec![], vec![]);
                        self.site.clear();
                    }
                });
                let magasins = self.c.lock().unwrap().magasins.clone();
                if !magasins.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add_enabled(!occupe, egui::Button::new("🔄 Relever tous les magasins")).on_hover_text("Relève à nouveau tous les prix ; les baisses depuis la dernière fois comptent comme des promos").clicked() {
                            self.travailler(magasins.clone(), vec![], vec![]);
                        }
                        let mut retirer = None;
                        for (s, _) in &magasins {
                            let nom = magasin_de(&crate::magasins::origine(s));
                            let g = self.c.lock().unwrap();
                            let n = g.produits.iter().filter(|p| p.magasin == nom).count();
                            let pr = g.produits.iter().filter(|p| p.magasin == nom && p.remise().is_some()).count();
                            drop(g);
                            ui.label(RichText::new(format!("🏬 {nom} : {n} · 🔥 {pr}")).background_color(ui.visuals().faint_bg_color));
                            if ui.small_button("✖").on_hover_text("Retirer ce magasin et ses produits").clicked() {
                                retirer = Some((s.clone(), nom));
                            }
                        }
                        if let Some((s, nom)) = retirer {
                            let mut g = self.c.lock().unwrap();
                            g.magasins.retain(|m| m.0 != s);
                            g.produits.retain(|p| p.magasin != nom);
                        }
                    });
                }
                ui.weak("Marche très bien avec les boutiques Shopify et WooCommerce (beaucoup de petits et moyens magasins), et avec la plupart des sites qui ont un plan du site. Les très gros sites (Amazon, Leclerc…) refusent souvent les robots : pour eux, utilise le bouton 🛒 du navigateur d'Outils sur chaque produit. Outils va doucement pour ne pas gêner les sites (300 produits ≈ 3 à 5 minutes, beaucoup moins pour Shopify).");
            } else {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.liens).desired_rows(2).desired_width(ui.available_width() - 150.0).hint_text("https://www.magasin.fr/produit…\n(un lien par ligne, ou plusieurs collés d'un coup)"));
                    if ui.add_enabled(!occupe, egui::Button::new(RichText::new("➕ Ajouter").strong())).clicked() {
                        let re = regex::Regex::new(r#"https?://[^\s"'<>]+"#).unwrap();
                        let urls: Vec<String> = re.find_iter(&self.liens).map(|m| m.as_str().trim_end_matches([',', ';', ')']).to_string()).collect();
                        if !urls.is_empty() {
                            self.travailler(vec![], urls, vec![]);
                        }
                        self.liens.clear();
                    }
                });
                if ui.small_button("✏ Ajouter un produit à la main").clicked() {
                    let mut g = self.c.lock().unwrap();
                    let id = g.prochain.max(1);
                    g.prochain = id + 1;
                    g.produits.push(Produit { id, magasin: "Magasin".into(), ..Default::default() });
                    self.edite = Some(id);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if occupe {
                ui.spinner();
                if ui.button("⏹ Arrêter").clicked() {
                    self.stop.store(true, Relaxed);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
            }
            ui.label(self.message.lock().unwrap().clone());
        });
        // réglages du PDF
        let mut g = self.c.lock().unwrap();
        let n_total = g.produits.len();
        let n_promos = g.produits.iter().filter(|p| p.remise().is_some()).count();
        ui.horizontal_wrapped(|ui| {
            ui.label("📄 PDF :");
            ui.add(egui::TextEdit::singleline(&mut g.titre).desired_width(200.0));
            egui::ComboBox::from_id_salt("cat_regrouper").selected_text(["par rayon", "par magasin", "tout ensemble"][g.regrouper.min(2) as usize]).show_ui(ui, |ui| {
                for (k, t) in ["par rayon", "par magasin", "tout ensemble"].iter().enumerate() {
                    ui.selectable_value(&mut g.regrouper, k as u8, *t);
                }
            });
            const TRIS: [&str; 4] = ["moins cher d'abord", "plus cher d'abord", "par nom", "plus grosse promo d'abord"];
            egui::ComboBox::from_id_salt("cat_tri").selected_text(TRIS[g.tri.min(3) as usize]).show_ui(ui, |ui| {
                for (k, t) in TRIS.iter().enumerate() {
                    ui.selectable_value(&mut g.tri, k as u8, *t);
                }
            });
            ui.checkbox(&mut g.promos_d_abord, "🔥 promos en tête");
            ui.checkbox(&mut g.seulement_promos, "seulement les promos");
            ui.checkbox(&mut g.totaux, "totaux");
        });
        drop(g);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(n_total > 0, egui::Button::new(RichText::new("📄 Faire le PDF").strong())).clicked() {
                self.faire_pdf();
            }
            ui.toggle_value(&mut self.apercu, "👁 Aperçu");
            ui.label(format!("{n_total} produits · 🔥 {n_promos} en promo"));
        });
        if self.apercu && n_total > 0 {
            let c = self.c.lock().unwrap().clone();
            let (pages, _) = mise_en_page(&c, &crate::papiers::date_du_jour());
            egui::ScrollArea::vertical().id_salt("apercu_catalogue").max_height(400.0).show(ui, |ui| apercu_pages(ui, &pages[..pages.len().min(2)], None));
        }
        // fiche du produit en cours de modification
        if let Some(id) = self.edite {
            let mut g = self.c.lock().unwrap();
            if let Some(p) = g.produits.iter_mut().find(|p| p.id == id) {
                let mut fermer = false;
                let mut choisir_photo = false;
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.nom).desired_width(f32::INFINITY).hint_text("nom du produit"));
                    ui.horizontal_wrapped(|ui| {
                        let mut t = p.prix.map(|v| format!("{v:.2}")).unwrap_or_default();
                        ui.label("prix");
                        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(70.0)).changed() {
                            p.prix = lire_prix(&t);
                        }
                        let mut b = p.prix_normal.map(|v| format!("{v:.2}")).unwrap_or_default();
                        ui.label("prix barré");
                        if ui.add(egui::TextEdit::singleline(&mut b).desired_width(70.0).hint_text("si promo")).changed() {
                            p.prix_normal = lire_prix(&b);
                        }
                        ui.label("×");
                        ui.add(egui::DragValue::new(&mut p.quantite).range(1..=99));
                        ui.add(egui::TextEdit::singleline(&mut p.categorie).desired_width(120.0).hint_text("rayon"));
                        ui.add(egui::TextEdit::singleline(&mut p.magasin).desired_width(100.0).hint_text("magasin"));
                        if ui.button("🖼 Photo").clicked() {
                            choisir_photo = true;
                        }
                        if ui.button("✔ Fermer").clicked() {
                            fermer = true;
                        }
                    });
                });
                if choisir_photo {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                        p.image = Some(f);
                    }
                }
                if fermer {
                    self.edite = None;
                }
            } else {
                self.edite = None;
            }
        }
        // liste (rapide même avec des milliers de produits)
        ui.separator();
        let magasins_liste: Vec<String> = {
            let g = self.c.lock().unwrap();
            let mut m: Vec<String> = g.produits.iter().map(|p| p.magasin.clone()).collect();
            m.sort();
            m.dedup();
            m
        };
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.filtre).desired_width(180.0).hint_text("🔎 filtrer"));
            ui.toggle_value(&mut self.filtre_promos, "🔥 promos seulement");
            egui::ComboBox::from_id_salt("filtre_magasin").selected_text(if self.filtre_magasin.is_empty() { "tous les magasins" } else { &self.filtre_magasin }).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.filtre_magasin, String::new(), "tous les magasins");
                for m in &magasins_liste {
                    ui.selectable_value(&mut self.filtre_magasin, m.clone(), m);
                }
            });
            if ui.small_button("🗑 Tout vider").on_hover_text("Retire tous les produits et magasins").clicked() {
                let mut g = self.c.lock().unwrap();
                g.produits.clear();
                g.magasins.clear();
            }
        });
        let visibles: Vec<Produit> = {
            let g = self.c.lock().unwrap();
            let f = self.filtre.to_lowercase();
            let mut v: Vec<Produit> = g.produits.iter().filter(|p| (!self.filtre_promos || p.remise().is_some()) && (self.filtre_magasin.is_empty() || p.magasin == self.filtre_magasin) && (f.is_empty() || p.nom.to_lowercase().contains(&f) || p.categorie.to_lowercase().contains(&f))).cloned().collect();
            v.sort_by(|a, b| b.remise().unwrap_or(0.0).total_cmp(&a.remise().unwrap_or(0.0)).then(a.magasin.cmp(&b.magasin)));
            v
        };
        let mut supprimer = None;
        let ctx = ui.ctx().clone();
        egui::ScrollArea::vertical().id_salt("produits").auto_shrink(false).show_rows(ui, 56.0, visibles.len(), |ui, plage| {
            for p in &visibles[plage] {
                ui.horizontal(|ui| {
                    ui.set_height(52.0);
                    match p.image.as_ref().and_then(|f| self.vignette(&ctx, f)) {
                        Some(t) => {
                            ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(48.0, 48.0)).corner_radius(6.0));
                        }
                        None => {
                            ui.add_sized([48.0, 48.0], egui::Label::new("🖼"));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if let Some(r) = p.remise() {
                                ui.label(RichText::new(format!("-{:.0} %", r.round())).strong().color(Color32::WHITE).background_color(Color32::from_rgb(220, 38, 38)));
                            }
                            let nom: String = p.nom.chars().take(90).collect();
                            ui.label(RichText::new(if nom.is_empty() { "(sans nom)".into() } else { nom }).strong());
                        });
                        ui.horizontal(|ui| {
                            let prix = p.prix.map(|v| euros(v, &p.devise)).unwrap_or_else(|| "prix ?".into());
                            ui.label(RichText::new(prix).strong().color(if p.remise().is_some() { Color32::from_rgb(240, 80, 80) } else { ui.visuals().strong_text_color() }));
                            if let Some(a) = p.avant() {
                                ui.label(RichText::new(euros(a, &p.devise)).strikethrough().weak());
                            }
                            ui.weak(format!("· 🏬 {}", p.magasin));
                            if !p.categorie.is_empty() {
                                ui.weak(format!("· {}", p.categorie));
                            }
                            if p.dispo == Some(false) {
                                ui.colored_label(Color32::from_rgb(230, 110, 90), "· indisponible");
                            }
                            if !p.erreur.is_empty() {
                                ui.colored_label(Color32::from_rgb(230, 150, 70), format!("· {}", p.erreur));
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").clicked() {
                            supprimer = Some(p.id);
                        }
                        if ui.small_button("✏").clicked() {
                            self.edite = Some(p.id);
                        }
                        if !p.url.is_empty() && ui.small_button("🔗").on_hover_text(&p.url).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(&p.url));
                        }
                    });
                });
            }
        });
        if let Some(id) = supprimer {
            self.c.lock().unwrap().produits.retain(|p| p.id != id);
        }
        let apres = self.c.lock().unwrap().clone();
        if apres != avant && !occupe {
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
        let h = r#"<html><head><title>X</title><script type="application/ld+json">{"@context":"https://schema.org","@graph":[{"@type":"BreadcrumbList","itemListElement":[{"name":"Accueil"},{"name":"Audio"},{"name":"Casque"}]},{"@type":"Product","name":"Casque Sans Fil &amp; Micro","image":["/img/casque.jpg"],"offers":{"@type":"Offer","price":"79.99","priceCurrency":"EUR","availability":"https://schema.org/InStock","priceSpecification":[{"price":"79.99"},{"@type":"UnitPriceSpecification","priceType":"https://schema.org/StrikethroughPrice","price":"99.99"}]}}]}</script></head></html>"#;
        let i = lire_page(h, "https://www.exemple.fr/p/casque");
        assert_eq!(i.nom, "Casque Sans Fil & Micro");
        assert_eq!((i.prix, i.prix_normal), (Some(79.99), Some(99.99)));
        assert_eq!(i.image.as_deref(), Some("https://www.exemple.fr/img/casque.jpg"));
        assert_eq!((i.dispo, i.categorie.as_str()), (Some(true), "Audio"));
        let h = r#"<meta property="og:title" content="Lampe d'architecte"><meta content="//cdn.x.com/l.webp" property="og:image"><meta property="product:price:amount" content="34,50">"#;
        let i = lire_page(h, "https://x.com/l");
        assert_eq!((i.nom.as_str(), i.prix, i.image.as_deref(), i.prix_normal), ("Lampe d'architecte", Some(34.5), Some("https://cdn.x.com/l.webp"), None));
        // prix soldé dans les meta
        let i = lire_page(r#"<meta property="og:title" content="Pull"><meta property="product:price:amount" content="40"><meta property="product:sale_price:amount" content="29.90">"#, "https://x.fr/p");
        assert_eq!((i.prix, i.prix_normal), (Some(29.9), Some(40.0)));
        // prix barré dans la page
        let i = lire_page(r#"<title>Tabouret</title><span class="price">49,99 €</span> <span class="old-price"><span>69,99&nbsp;€</span></span>"#, "https://y.fr/t");
        assert_eq!((i.prix, i.prix_normal), (Some(49.99), Some(69.99)));
        let i = lire_page(r#"<title>Table</title><span>Prix : 1 249,00 €</span><del>1 499,00 €</del>"#, "https://y.fr/t");
        assert_eq!((i.prix, i.prix_normal), (Some(1249.0), Some(1499.0)));
        // un « ancien prix » plus bas n'est pas une promo
        let i = lire_page(r#"<title>Chaise</title><span>Prix : 30,00 €</span><del>20,00 €</del>"#, "https://y.fr/c");
        assert_eq!(i.prix_normal, None);
        // une baisse depuis le relevé précédent compte comme une promo
        let mut p = Produit { id: 1, url: "https://www.fnac.com/x".into(), prix: Some(100.0), maj: 5, nom: "A".into(), ..Default::default() };
        appliquer(&mut p, Infos { nom: "B".into(), prix: Some(80.0), devise: "EUR".into(), ..Default::default() }, 10);
        assert_eq!((p.prix, p.ancien_prix, p.nom.as_str(), p.magasin.as_str()), (Some(80.0), Some(100.0), "A", "Fnac"));
        assert_eq!(p.remise().map(|r| r.round()), Some(20.0));
        // le même produit (adresse avec ou sans paramètres) n'est pas ajouté deux fois
        let mut c = Catalogue::default();
        integrer(&mut c, "https://m.fr/p/1?ref=a", None, Infos { prix: Some(5.0), ..Default::default() });
        integrer(&mut c, "https://m.fr/p/1", None, Infos { prix: Some(4.0), ..Default::default() });
        assert_eq!(c.produits.len(), 1);
        assert_eq!(c.produits[0].ancien_prix, Some(5.0));
    }

    #[test]
    fn pdf() {
        let mut c = Catalogue { regrouper: 0, ..Default::default() };
        for k in 0..8u64 {
            c.produits.push(Produit { id: k, nom: format!("Produit numéro {k} avec un nom assez long pour passer à la ligne"), prix: Some(10.0 + k as f64), prix_normal: (k % 3 == 1).then_some(20.0 + k as f64), magasin: if k % 2 == 0 { "Fnac".into() } else { "Darty".into() }, categorie: if k < 4 { "Cuisine".into() } else { "Jeux".into() }, image: (k % 3 == 0).then(|| PathBuf::from("x.png")), ..Default::default() });
        }
        let promos: Vec<usize> = (0..8).filter(|&k| c.produits[k].remise().is_some()).collect();
        assert_eq!(promos, vec![1, 4, 7]);
        let g = groupes(&c, &(0..8).collect::<Vec<_>>());
        assert_eq!(g.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["Cuisine", "Jeux"]);
        let (pages, images) = mise_en_page(&c, "28 novembre 2026");
        assert_eq!(images.len(), 3);
        let texte: String = pages.iter().flatten().filter_map(|o| if let Op::Texte { s, .. } = o { Some(s.clone()) } else { None }).collect::<Vec<_>>().join("|");
        assert!(texte.contains("EN PROMO — 3 produit(s)"));
        assert!(texte.contains("-48 %")); // 11 € au lieu de 21 €
        let imgs: Vec<RgbaImage> = images.iter().map(|_| RgbaImage::from_pixel(20, 10, image::Rgba([200, 30, 30, 255]))).collect();
        let o = pdf_complet(&pages, None, &imgs);
        assert!(o.windows(8).any(|w| w == b"/Img2 Do"));
        // seulement les promos
        c.seulement_promos = true;
        let (pages, _) = mise_en_page(&c, "x");
        let n = pages.iter().flatten().filter(|o| matches!(o, Op::Texte { taille, gras: true, .. } if *taille > 12.0 && *taille < 17.0)).count();
        assert_eq!(n, 3);
        c.seulement_promos = false;
        // beaucoup de produits : 4 par ligne, plusieurs pages numérotées
        for k in 8..80u64 {
            c.produits.push(Produit { id: k, nom: "Z".into(), prix: Some(1.0), ..Default::default() });
        }
        let (pages, _) = mise_en_page(&c, "x");
        assert!(pages.len() >= 4);
        // vrai catalogue enregistré → PDF : CATALOGUE_JSON=fichier.json PAPIERS_TEST=dossier
        if let (Ok(j), Ok(d)) = (std::env::var("CATALOGUE_JSON"), std::env::var("PAPIERS_TEST")) {
            let c: Catalogue = serde_json::from_str(&std::fs::read_to_string(j).unwrap()).unwrap();
            let (pages, ordre) = mise_en_page(&c, "28 novembre 2026");
            let imgs: Vec<RgbaImage> = ordre.iter().map(|&k| image::open(c.produits[k].image.as_ref().unwrap()).unwrap().to_rgba8()).collect();
            std::fs::write(format!("{d}/catalogue_reel.pdf"), pdf_complet(&pages, None, &imgs)).unwrap();
        }
        if let Ok(d) = std::env::var("PAPIERS_TEST") {
            c.produits.truncate(14);
            let (pages, images) = mise_en_page(&c, "28 novembre 2026");
            let imgs: Vec<RgbaImage> = images.iter().map(|_| RgbaImage::from_fn(60, 40, |x, y| image::Rgba([(x * 4) as u8, (y * 6) as u8, 160, 255]))).collect();
            std::fs::write(format!("{d}/catalogue.pdf"), pdf_complet(&pages, None, &imgs)).unwrap();
        }
    }
}
