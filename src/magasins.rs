//! Explorer tout un magasin en ligne : trouver ses produits, puis leur nom, prix, prix barré (promo),
//! photo et catégorie. Dans l'ordre :
//! 1. les boutiques Shopify publient tout leur catalogue (`/products.json`) ;
//! 2. les boutiques WooCommerce aussi (`/wp-json/wc/store/v1/products`) ;
//! 3. sinon, le plan du site (sitemap, annoncé dans robots.txt) donne les pages produits ;
//! 4. sinon, on suit les liens depuis la page donnée.
//! On respecte robots.txt et on va doucement (une page à la fois, avec une pause).
use crate::catalogue_prix::{lire_page, lire_prix, telecharger_page, Infos};
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Trouve {
    pub url: String,
    pub infos: Infos,
}

/// « https://www.magasin.fr/rayon/x » → « https://www.magasin.fr ».
pub fn origine(url: &str) -> String {
    let u = if url.contains("://") { url.to_string() } else { format!("https://{url}") };
    u.splitn(4, '/').take(3).collect::<Vec<_>>().join("/")
}

fn hote(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split(['/', '?', '#', ':']).next().unwrap_or("").trim_start_matches("www.").to_lowercase()
}

/// Règles de robots.txt pour tout le monde : (chemins interdits, plans du site).
pub fn lire_robots(t: &str) -> (Vec<String>, Vec<String>) {
    let (mut interdits, mut plans) = (vec![], vec![]);
    let mut pour_nous = false;
    for l in t.lines() {
        let l = l.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = l.split_once(':') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim());
        match k.as_str() {
            "user-agent" => pour_nous = v == "*",
            "disallow" if pour_nous && !v.is_empty() => interdits.push(v.split('*').next().unwrap_or(v).to_string()),
            "sitemap" => plans.push(v.to_string()),
            _ => {}
        }
    }
    (interdits, plans)
}

pub fn autorise(url: &str, interdits: &[String]) -> bool {
    let chemin = url.split("://").nth(1).and_then(|r| r.find('/').map(|i| &r[i..])).unwrap_or("/");
    !interdits.iter().any(|d| !d.is_empty() && chemin.starts_with(d.as_str()))
}

/// Adresses `<loc>` d'un plan du site : (sous-plans, pages).
pub fn lire_sitemap(xml: &str) -> (Vec<String>, Vec<String>) {
    let re = regex::Regex::new(r"(?is)<(sitemap|url)>\s*.*?<loc>\s*(?:<!\[CDATA\[)?(.*?)(?:\]\]>)?\s*</loc>").unwrap();
    let (mut plans, mut pages) = (vec![], vec![]);
    for c in re.captures_iter(xml) {
        let u = crate::catalogue_prix::entites(c[2].trim());
        if c[1].eq_ignore_ascii_case("sitemap") {
            plans.push(u);
        } else {
            pages.push(u);
        }
    }
    (plans, pages)
}

/// Une adresse qui ressemble à une page produit.
pub fn ressemble_produit(u: &str) -> bool {
    let b = u.to_lowercase();
    let chemin = b.split("://").nth(1).and_then(|r| r.find('/').map(|i| r[i..].to_string())).unwrap_or_default();
    if chemin.len() < 3 || ["/blog", "/cms", "/aide", "/help", "/contact", "/compte", "/account", "/panier", "/cart", "/login", "/magasin", "/store-locator", "/actualite", "/news"].iter().any(|x| chemin.starts_with(x)) {
        return false;
    }
    ["/product", "/produit", "/products/", "/p/", "/dp/", "-p-", "/item", "/article", "/fiche", "/ref/", "/sku", "/mp/"].iter().any(|x| chemin.contains(x))
        || (chemin.ends_with(".html") && chemin.chars().filter(|c| c.is_ascii_digit()).count() >= 3)
}

fn texte(u: &str) -> Result<String, String> {
    if u.ends_with(".gz") {
        let r = ureq::get(u).timeout(Duration::from_secs(30)).call().map_err(|e| e.to_string())?;
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::Read::take(flate2::read::GzDecoder::new(r.into_reader()), 64 << 20), &mut s).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    telecharger_page(u)
}

fn json(u: &str) -> Option<serde_json::Value> {
    let r = ureq::get(u).set("Accept", "application/json").set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/131.0 Safari/537.36").timeout(Duration::from_secs(25)).call().ok()?;
    r.into_json().ok()
}

/// Produits d'une boutique Shopify (une page de l'API publique).
pub fn lire_shopify(j: &serde_json::Value, base: &str) -> Vec<Trouve> {
    let mut v = vec![];
    for p in j["products"].as_array().into_iter().flatten() {
        let variantes = p["variants"].as_array().cloned().unwrap_or_default();
        let var = variantes.iter().find(|x| x["available"].as_bool().unwrap_or(true)).or(variantes.first());
        let Some(var) = var else { continue };
        let prix = var["price"].as_str().and_then(lire_prix).or(var["price"].as_f64());
        let barre = var["compare_at_price"].as_str().and_then(lire_prix).or(var["compare_at_price"].as_f64());
        v.push(Trouve {
            url: format!("{}/products/{}", base, p["handle"].as_str().unwrap_or("")),
            infos: Infos {
                nom: crate::catalogue_prix::entites(p["title"].as_str().unwrap_or("")),
                prix,
                prix_normal: barre.filter(|b| prix.is_some_and(|p| *b > p * 1.005)),
                devise: "EUR".into(),
                image: p["images"][0]["src"].as_str().map(String::from),
                dispo: Some(variantes.iter().any(|x| x["available"].as_bool().unwrap_or(true))),
                categorie: p["product_type"].as_str().unwrap_or("").to_string(),
            },
        });
    }
    v
}

/// Fiche officielle d'un produit Shopify (`/products/x.json`), plus fiable que la page.
pub fn produit_shopify(url: &str) -> Option<Trouve> {
    let propre = url.split(['?', '#']).next()?.trim_end_matches('/');
    if !propre.contains("/products/") {
        return None;
    }
    let j = json(&format!("{propre}.json"))?;
    let base = origine(propre);
    lire_shopify(&serde_json::json!({ "products": [j["product"].clone()] }), &base).into_iter().next().map(|mut t| {
        t.url = url.to_string();
        t
    })
}

/// Produits d'une boutique WooCommerce (une page de l'API publique).
pub fn lire_woo(j: &serde_json::Value) -> Vec<Trouve> {
    let mut v = vec![];
    for p in j.as_array().into_iter().flatten() {
        let pr = &p["prices"];
        let div = 10f64.powi(pr["currency_minor_unit"].as_i64().unwrap_or(2) as i32);
        let n = |k: &str| pr[k].as_str().and_then(|s| s.parse::<f64>().ok()).filter(|x| *x > 0.0).map(|x| x / div);
        let prix = n("price");
        v.push(Trouve {
            url: p["permalink"].as_str().unwrap_or("").to_string(),
            infos: Infos {
                nom: crate::catalogue_prix::entites(p["name"].as_str().unwrap_or("")),
                prix,
                prix_normal: n("regular_price").filter(|b| prix.is_some_and(|p| *b > p * 1.005)),
                devise: pr["currency_code"].as_str().unwrap_or("EUR").to_string(),
                image: p["images"][0]["src"].as_str().map(String::from),
                dispo: p["is_in_stock"].as_bool(),
                categorie: p["categories"][0]["name"].as_str().map(crate::catalogue_prix::entites).unwrap_or_default(),
            },
        });
    }
    v
}

/// Liens d'une page, sur le même site.
pub fn liens(html: &str, base_url: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?is)href\s*=\s*["']([^"'#]+)["']"#).unwrap();
    let h = hote(base_url);
    let base = origine(base_url);
    let mut v = vec![];
    for c in re.captures_iter(html) {
        let u = c[1].trim();
        let abs = if u.starts_with("http") { u.to_string() } else if let Some(r) = u.strip_prefix("//") { format!("https://{r}") } else if u.starts_with('/') { format!("{base}{u}") } else { continue };
        if hote(&abs) == h && !abs.contains("mailto:") {
            v.push(crate::catalogue_prix::entites(&abs));
        }
    }
    v
}

/// Explore un magasin. `recu` reçoit chaque produit trouvé. Renvoie le nombre de produits.
pub fn explorer(depart: &str, max: usize, stop: &AtomicBool, dire: &dyn Fn(String), recu: &mut dyn FnMut(Trouve)) -> Result<usize, String> {
    let base = origine(depart.trim());
    let mut n = 0;
    // 1. Shopify
    dire("Je regarde comment ce magasin est fait…".into());
    if let Some(j) = json(&format!("{base}/products.json?limit=250&page=1")).filter(|j| j["products"].is_array()) {
        dire("Boutique Shopify : je lis tout le catalogue…".into());
        let mut page = 1;
        let mut j = Some(j);
        while let Some(x) = j.take() {
            let v = lire_shopify(&x, &base);
            if v.is_empty() {
                break;
            }
            for t in v {
                if n >= max || stop.load(Relaxed) {
                    return Ok(n);
                }
                recu(t);
                n += 1;
            }
            page += 1;
            dire(format!("{n} produits lus…"));
            std::thread::sleep(Duration::from_millis(600));
            j = json(&format!("{base}/products.json?limit=250&page={page}"));
        }
        return Ok(n);
    }
    // 2. WooCommerce
    if let Some(j) = json(&format!("{base}/wp-json/wc/store/v1/products?per_page=100&page=1")).filter(|j| j.as_array().is_some_and(|a| !a.is_empty() && a[0]["prices"].is_object())) {
        dire("Boutique WooCommerce : je lis tout le catalogue…".into());
        let mut page = 1;
        let mut j = Some(j);
        while let Some(x) = j.take() {
            let v = lire_woo(&x);
            if v.is_empty() {
                break;
            }
            for t in v {
                if n >= max || stop.load(Relaxed) {
                    return Ok(n);
                }
                recu(t);
                n += 1;
            }
            page += 1;
            dire(format!("{n} produits lus…"));
            std::thread::sleep(Duration::from_millis(600));
            j = json(&format!("{base}/wp-json/wc/store/v1/products?per_page=100&page={page}"));
        }
        return Ok(n);
    }
    // 3. plan du site
    let (interdits, mut plans) = texte(&format!("{base}/robots.txt")).map(|t| lire_robots(&t)).unwrap_or_default();
    if plans.is_empty() {
        plans = vec![format!("{base}/sitemap.xml"), format!("{base}/sitemap_index.xml")];
    }
    let mut pages: Vec<String> = vec![];
    let mut vus = HashSet::new();
    let mut a_lire: VecDeque<(String, bool)> = plans.into_iter().map(|p| (p, false)).collect();
    let mut lus = 0;
    while let Some((p, produits)) = a_lire.pop_front() {
        if lus > 40 || pages.len() >= max * 3 || stop.load(Relaxed) || !vus.insert(p.clone()) {
            continue;
        }
        lus += 1;
        dire(format!("Je lis le plan du site ({} pages trouvées)…", pages.len()));
        let Ok(xml) = texte(&p) else { continue };
        let (sous, urls) = lire_sitemap(&xml);
        // les plans de produits d'abord, pas ceux du blog ou des images
        let mut sous: Vec<String> = sous.into_iter().filter(|s| {
            let b = s.to_lowercase();
            !["image", "blog", "post", "news", "cms", "video", "store", "magasin"].iter().any(|x| b.contains(x))
        }).collect();
        sous.sort_by_key(|s| {
            let b = s.to_lowercase();
            !(b.contains("product") || b.contains("produit") || b.contains("article"))
        });
        for s in sous {
            let pr = s.to_lowercase().contains("product") || s.to_lowercase().contains("produit");
            a_lire.push_back((s, pr));
        }
        let plan_produits = produits || p.to_lowercase().contains("product") || p.to_lowercase().contains("produit");
        pages.extend(urls.into_iter().filter(|u| (plan_produits || ressemble_produit(u)) && autorise(u, &interdits)));
    }
    // 4. sinon : suivre les liens depuis la page donnée
    if pages.is_empty() {
        dire("Pas de plan du site : je suis les liens des rayons…".into());
        let mut file: VecDeque<String> = VecDeque::from([depart.trim().to_string()]);
        let mut vues = HashSet::new();
        let mut rayons = 0;
        while let Some(u) = file.pop_front() {
            if rayons >= 25 || pages.len() >= max * 2 || stop.load(Relaxed) || !vues.insert(u.clone()) || !autorise(&u, &interdits) {
                continue;
            }
            rayons += 1;
            let Ok(h) = telecharger_page(&u) else { continue };
            for l in liens(&h, &u) {
                if ressemble_produit(&l) {
                    if !pages.contains(&l) {
                        pages.push(l);
                    }
                } else {
                    let b = l.to_lowercase();
                    if ["categor", "collection", "rayon", "/c/", "univers", "shop", "boutique", "promo", "soldes"].iter().any(|x| b.contains(x)) {
                        file.push_back(l);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    let mut deja = HashSet::new();
    pages.retain(|p| deja.insert(p.split('?').next().unwrap_or(p).to_string()));
    if pages.is_empty() {
        return Err("aucune page produit trouvée sur ce site (il est peut-être fait pour être lu seulement par un navigateur)".into());
    }
    let total = pages.len().min(max);
    for (k, u) in pages.into_iter().take(max).enumerate() {
        if stop.load(Relaxed) {
            break;
        }
        dire(format!("Produits : {}/{total}…", k + 1));
        if let Ok(h) = telecharger_page(&u) {
            let i = lire_page(&h, &u);
            if i.prix.is_some() && !i.nom.is_empty() {
                recu(Trouve { url: u, infos: i });
                n += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if n == 0 {
        return Err("les pages produits n'affichent pas leur prix sans navigateur : utilise le bouton 🛒 du navigateur d'Outils".into());
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magasins() {
        assert_eq!(origine("www.x.fr/a/b"), "https://www.x.fr");
        let (interdits, plans) = lire_robots("User-agent: Googlebot\nDisallow: /tout\n\nUser-agent: *\nDisallow: /panier\nDisallow: /recherche*\nSitemap: https://x.fr/sitemap.xml");
        assert_eq!(interdits, vec!["/panier", "/recherche"]);
        assert_eq!(plans, vec!["https://x.fr/sitemap.xml"]);
        assert!(!autorise("https://x.fr/panier/1", &interdits));
        assert!(autorise("https://x.fr/produit/1", &interdits));
        let (s, p) = lire_sitemap("<sitemapindex><sitemap><loc>https://x.fr/sm-products-1.xml</loc></sitemap></sitemapindex>");
        assert_eq!((s.len(), p.len()), (1, 0));
        let (_, p) = lire_sitemap("<urlset><url><loc>https://x.fr/p/chaise-123</loc><lastmod>x</lastmod></url><url><loc><![CDATA[https://x.fr/a?b=1&amp;c=2]]></loc></url></urlset>");
        assert_eq!(p, vec!["https://x.fr/p/chaise-123", "https://x.fr/a?b=1&c=2"]);
        assert!(ressemble_produit("https://www.x.fr/produit/table-basse"));
        assert!(ressemble_produit("https://www.x.fr/table-basse-chene-12345.html"));
        assert!(!ressemble_produit("https://www.x.fr/blog/produit-du-mois"));
        assert!(!ressemble_produit("https://www.x.fr/"));
        // Shopify : prix barré = promo
        let j = serde_json::json!({"products": [{"title": "Bougie", "handle": "bougie", "product_type": "Déco", "images": [{"src": "https://cdn/b.jpg"}],
            "variants": [{"price": "12.00", "compare_at_price": "20.00", "available": true}]},
            {"title": "Vase", "handle": "vase", "variants": [{"price": "30.00", "compare_at_price": null, "available": false}]}]});
        let v = lire_shopify(&j, "https://boutique.fr");
        assert_eq!(v[0].url, "https://boutique.fr/products/bougie");
        assert_eq!((v[0].infos.prix, v[0].infos.prix_normal, v[0].infos.categorie.as_str()), (Some(12.0), Some(20.0), "Déco"));
        assert_eq!((v[1].infos.prix_normal, v[1].infos.dispo), (None, Some(false)));
        // WooCommerce : prix en centimes
        let j = serde_json::json!([{"name": "Tasse &amp; sous-tasse", "permalink": "https://w.fr/p/tasse", "is_in_stock": true, "categories": [{"name": "Cuisine"}],
            "prices": {"price": "899", "regular_price": "1299", "currency_code": "EUR", "currency_minor_unit": 2}, "images": [{"src": "https://w.fr/t.jpg"}]}]);
        let v = lire_woo(&j);
        assert_eq!((v[0].infos.nom.as_str(), v[0].infos.prix, v[0].infos.prix_normal), ("Tasse & sous-tasse", Some(8.99), Some(12.99)));
        // liens du même site
        let l = liens(r#"<a href="/produit/a">a</a><a href="https://autre.fr/x">b</a><a href="//www.x.fr/c">c</a>"#, "https://www.x.fr/rayon");
        assert_eq!(l, vec!["https://www.x.fr/produit/a", "https://www.x.fr/c"]);
    }

    /// Lecture de vraies données enregistrées : DONNEES_REELLES=dossier (shopify.json, page.html)
    #[test]
    fn donnees_reelles() {
        let Ok(d) = std::env::var("DONNEES_REELLES") else { return };
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{d}/shopify.json")).unwrap()).unwrap();
        let v = lire_shopify(&j, "https://www.allbirds.com");
        println!("Shopify : {} produits, {} en promo", v.len(), v.iter().filter(|t| t.infos.prix_normal.is_some()).count());
        for t in v.iter().take(3) {
            println!("  {} | {:?} | barré {:?} | {} | {}", t.infos.nom, t.infos.prix, t.infos.prix_normal, t.infos.categorie, t.url);
        }
        let h = std::fs::read_to_string(format!("{d}/page.html")).unwrap();
        let i = lire_page(&h, "https://www.allbirds.com/products/womens-tree-runners-wheat-dark-beige");
        println!("Page : {i:?}");
        assert!(v.len() > 100 && i.prix.is_some());
    }

    /// Essai sur un vrai magasin : EXPLORER_TEST=https://… cargo test explorer_reel -- --nocapture
    #[test]
    fn explorer_reel() {
        let Ok(u) = std::env::var("EXPLORER_TEST") else { return };
        let mut v = vec![];
        let r = explorer(&u, 40, &AtomicBool::new(false), &|s| println!("{s}"), &mut |t| v.push(t));
        println!("résultat : {r:?}");
        for t in v.iter().take(8) {
            println!("{} | {:?} | barré {:?} | {} | {:?}", t.infos.nom, t.infos.prix, t.infos.prix_normal, t.infos.categorie, t.infos.image.as_deref().map(|x| &x[..x.len().min(50)]));
        }
        println!("{} produits, {} en promo", v.len(), v.iter().filter(|t| t.infos.prix_normal.is_some()).count());
        assert!(!v.is_empty());
    }
}
