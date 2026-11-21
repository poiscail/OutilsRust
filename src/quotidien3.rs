//! Quotidien, suite : calculatrice et convertisseur (50), actus RSS (51), comptes à rebours (52),
//! emploi du temps et devoirs (55), radio en ligne (56).
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::quotidien2::{date_de_jours, jour_semaine, jours_depuis_1970, lire_date, lire_heure, texte_date};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{BufRead, Read};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// (date AAAA-MM-JJ, minutes depuis minuit)
fn maintenant() -> (String, u32) {
    let h = crate::minecraft::horodatage();
    let n = |a: usize| h.get(a..a + 2).and_then(|x| x.parse::<u32>().ok()).unwrap_or(0);
    (h.get(..10).unwrap_or("").to_string(), n(11) * 60 + n(14))
}

fn jours_entre(a: &str, b: &str) -> Option<i64> {
    let (x, y) = (lire_date(a)?, lire_date(b)?);
    Some(jours_depuis_1970(y.0, y.1, y.2) - jours_depuis_1970(x.0, x.1, x.2))
}

const JOURS: [&str; 7] = ["Lundi", "Mardi", "Mercredi", "Jeudi", "Vendredi", "Samedi", "Dimanche"];

// ---------------------------------------------------------------------------------------------------
// 50 · Calculatrice et convertisseur
// ---------------------------------------------------------------------------------------------------

/// Calcule une expression : + - * / ^ %, parenthèses, sqrt sin cos tan ln log abs round, pi, e, « , » décimale.
pub fn calculer(s: &str) -> Result<f64, String> {
    let t: Vec<char> = s.replace(',', ".").replace('×', "*").replace('÷', "/").replace('−', "-").chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Analyse { t, i: 0 };
    let v = p.somme()?;
    if p.i < p.t.len() {
        return Err(format!("« {} » inattendu", p.t[p.i]));
    }
    if v.is_finite() { Ok(v) } else { Err("résultat impossible".into()) }
}

struct Analyse {
    t: Vec<char>,
    i: usize,
}

impl Analyse {
    fn voir(&self) -> Option<char> {
        self.t.get(self.i).copied()
    }
    fn somme(&mut self) -> Result<f64, String> {
        let mut v = self.produit()?;
        while let Some(c) = self.voir() {
            match c {
                '+' => {
                    self.i += 1;
                    v += self.produit()?;
                }
                '-' => {
                    self.i += 1;
                    v -= self.produit()?;
                }
                _ => break,
            }
        }
        Ok(v)
    }
    fn produit(&mut self) -> Result<f64, String> {
        let mut v = self.unaire()?;
        loop {
            match self.voir() {
                Some('*') => {
                    self.i += 1;
                    v *= self.unaire()?;
                }
                Some('/') => {
                    self.i += 1;
                    let d = self.unaire()?;
                    if d == 0.0 {
                        return Err("division par zéro".into());
                    }
                    v /= d;
                }
                Some('%') => {
                    self.i += 1;
                    // « 20% » seul = 0,2 ; « 7 % 3 » = reste
                    if self.voir().is_some_and(|c| c.is_ascii_digit() || c == '(' || c == '.') {
                        let d = self.unaire()?;
                        v %= d;
                    } else {
                        v /= 100.0;
                    }
                }
                // multiplication implicite : 2(3+1), 2pi
                Some(c) if c == '(' || c.is_alphabetic() => v *= self.unaire()?,
                _ => break,
            }
        }
        Ok(v)
    }
    fn puissance(&mut self) -> Result<f64, String> {
        let b = self.atome()?;
        if self.voir() == Some('^') {
            self.i += 1;
            let e = self.unaire()?; // associatif à droite, « 2^-1 » permis
            return Ok(b.powf(e));
        }
        Ok(b)
    }
    fn unaire(&mut self) -> Result<f64, String> {
        match self.voir() {
            Some('-') => {
                self.i += 1;
                Ok(-self.unaire()?)
            }
            Some('+') => {
                self.i += 1;
                self.unaire()
            }
            _ => self.puissance(),
        }
    }
    fn atome(&mut self) -> Result<f64, String> {
        match self.voir() {
            Some('(') => {
                self.i += 1;
                let v = self.somme()?;
                if self.voir() != Some(')') {
                    return Err("parenthèse non fermée".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let d = self.i;
                while self.voir().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                if self.voir() == Some('e') && self.t.get(self.i + 1).is_some_and(|c| c.is_ascii_digit() || *c == '-') {
                    self.i += 2;
                    while self.voir().is_some_and(|c| c.is_ascii_digit()) {
                        self.i += 1;
                    }
                }
                self.t[d..self.i].iter().collect::<String>().parse().map_err(|_| "nombre mal écrit".to_string())
            }
            Some(c) if c.is_alphabetic() => {
                let d = self.i;
                while self.voir().is_some_and(|c| c.is_alphabetic()) {
                    self.i += 1;
                }
                let n: String = self.t[d..self.i].iter().collect::<String>().to_lowercase();
                match n.as_str() {
                    "pi" | "π" => return Ok(std::f64::consts::PI),
                    "e" => return Ok(std::f64::consts::E),
                    _ => {}
                }
                let x = self.atome()?;
                Ok(match n.as_str() {
                    "sqrt" | "racine" | "rac" => x.sqrt(),
                    "sin" => x.to_radians().sin(),
                    "cos" => x.to_radians().cos(),
                    "tan" => x.to_radians().tan(),
                    "ln" => x.ln(),
                    "log" => x.log10(),
                    "abs" => x.abs(),
                    "round" | "arrondi" => x.round(),
                    _ => return Err(format!("fonction inconnue « {n} »")),
                })
            }
            Some(c) => Err(format!("« {c} » inattendu")),
            None => Err("expression incomplète".into()),
        }
    }
}

pub fn joli(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64).chars().rev().collect::<Vec<_>>().chunks(3).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ").chars().rev().collect::<String>().replace("- ", "-");
    }
    let s = format!("{:.10}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

/// (catégorie, [(unité, facteur vers l'unité de base)])
pub const UNITES: [(&str, &[(&str, f64)]); 7] = [
    ("Longueur", &[("mm", 0.001), ("cm", 0.01), ("m", 1.0), ("km", 1000.0), ("pouce", 0.0254), ("pied", 0.3048), ("yard", 0.9144), ("mile", 1609.344), ("mille marin", 1852.0)]),
    ("Masse", &[("mg", 1e-6), ("g", 0.001), ("kg", 1.0), ("tonne", 1000.0), ("once", 0.028_349_523_125), ("livre (lb)", 0.453_592_37), ("stone", 6.350_293_18)]),
    ("Volume", &[("ml", 0.001), ("cl", 0.01), ("l", 1.0), ("m³", 1000.0), ("cuillère à café", 0.005), ("cuillère à soupe", 0.015), ("tasse (US)", 0.236_588), ("gallon (US)", 3.785_411_784)]),
    ("Vitesse", &[("m/s", 1.0), ("km/h", 1.0 / 3.6), ("mph", 0.447_04), ("nœud", 0.514_444)]),
    ("Données", &[("octet", 1.0), ("Ko", 1024.0), ("Mo", 1_048_576.0), ("Go", 1_073_741_824.0), ("To", 1_099_511_627_776.0), ("bit", 0.125), ("Mbit", 131_072.0)]),
    ("Durée", &[("seconde", 1.0), ("minute", 60.0), ("heure", 3600.0), ("jour", 86_400.0), ("semaine", 604_800.0), ("an", 31_557_600.0)]),
    ("Surface", &[("cm²", 1e-4), ("m²", 1.0), ("are", 100.0), ("hectare", 10_000.0), ("km²", 1e6), ("pied²", 0.092_903)]),
];

pub fn convertir(v: f64, de: f64, vers: f64) -> f64 {
    v * de / vers
}

/// Température : de / vers parmi « °C », « °F », « K ».
pub fn temperature(v: f64, de: &str, vers: &str) -> f64 {
    let c = match de {
        "°F" => (v - 32.0) * 5.0 / 9.0,
        "K" => v - 273.15,
        _ => v,
    };
    match vers {
        "°F" => c * 9.0 / 5.0 + 32.0,
        "K" => c + 273.15,
        _ => c,
    }
}

pub struct Calculatrice {
    expr: String,
    historique: Vec<(String, String)>,
    cat: usize,
    valeur: f64,
    de: usize,
    vers: usize,
    temp: (f64, &'static str, &'static str),
    taux: Arc<Mutex<Option<Result<BTreeMap<String, f64>, String>>>>,
    montant: f64,
    devises: (String, String),
}

impl Calculatrice {
    pub fn new() -> Self {
        Calculatrice { expr: String::new(), historique: charger("calculs"), cat: 0, valeur: 1.0, de: 2, vers: 3, temp: (20.0, "°C", "°F"), taux: Arc::default(), montant: 10.0, devises: ("EUR".into(), "USD".into()) }
    }
}

impl Outil for Calculatrice {
    fn nom(&self) -> &'static str {
        "🧮 Calculatrice et convertisseur"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Calculatrice");
        let r = ui.add(egui::TextEdit::singleline(&mut self.expr).hint_text("ex. (12,5 + 3) * 2 ^ 3 - sqrt(16)   ou   250 * 15%").font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
        let res = (!self.expr.trim().is_empty()).then(|| calculer(&self.expr));
        match &res {
            Some(Ok(v)) => {
                ui.label(RichText::new(format!("= {}", joli(*v))).size(28.0).strong());
            }
            Some(Err(e)) => {
                ui.weak(e);
            }
            None => {}
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(Ok(v)) = res {
                self.historique.push((self.expr.clone(), joli(v)));
                if self.historique.len() > 30 {
                    self.historique.remove(0);
                }
                enregistrer("calculs", &self.historique);
                self.expr = joli(v).replace(' ', "");
            }
            r.request_focus();
        }
        ui.horizontal_wrapped(|ui| {
            for t in ["7", "8", "9", "/", "(", ")", "4", "5", "6", "*", "^", "sqrt(", "1", "2", "3", "-", "%", "pi", "0", ",", "+", "C"] {
                if ui.add(egui::Button::new(RichText::new(t).size(16.0).monospace()).min_size(egui::vec2(44.0, 32.0))).clicked() {
                    if t == "C" {
                        self.expr.clear();
                    } else {
                        self.expr.push_str(t);
                    }
                }
            }
        });
        if !self.historique.is_empty() {
            ui.collapsing("Historique", |ui| {
                for (e, v) in self.historique.iter().rev() {
                    if ui.small_button(format!("{e} = {v}")).clicked() {
                        self.expr = e.clone();
                    }
                }
            });
        }
        ui.separator();
        ui.heading("Convertisseur");
        ui.horizontal_wrapped(|ui| {
            for (k, (n, _)) in UNITES.iter().enumerate() {
                if ui.selectable_label(self.cat == k, *n).clicked() {
                    self.cat = k;
                    (self.de, self.vers) = (0, 1);
                }
            }
        });
        let u = UNITES[self.cat].1;
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut self.valeur).speed(0.1));
            egui::ComboBox::from_id_salt("de").selected_text(u[self.de.min(u.len() - 1)].0).show_ui(ui, |ui| {
                for (k, (n, _)) in u.iter().enumerate() {
                    ui.selectable_value(&mut self.de, k, *n);
                }
            });
            if ui.button("⇄").clicked() {
                std::mem::swap(&mut self.de, &mut self.vers);
            }
            egui::ComboBox::from_id_salt("vers").selected_text(u[self.vers.min(u.len() - 1)].0).show_ui(ui, |ui| {
                for (k, (n, _)) in u.iter().enumerate() {
                    ui.selectable_value(&mut self.vers, k, *n);
                }
            });
            let r = convertir(self.valeur, u[self.de.min(u.len() - 1)].1, u[self.vers.min(u.len() - 1)].1);
            ui.label(RichText::new(format!("= {} {}", joli((r * 1e6).round() / 1e6), u[self.vers.min(u.len() - 1)].0)).size(18.0).strong());
        });
        ui.horizontal(|ui| {
            ui.label("🌡");
            ui.add(egui::DragValue::new(&mut self.temp.0).speed(0.5));
            for (de, n) in [(&mut self.temp.1, "de"), (&mut self.temp.2, "vers")] {
                egui::ComboBox::from_id_salt(n).width(60.0).selected_text(*de).show_ui(ui, |ui| {
                    for x in ["°C", "°F", "K"] {
                        ui.selectable_value(de, x, x);
                    }
                });
            }
            ui.label(RichText::new(format!("= {:.1} {}", temperature(self.temp.0, self.temp.1, self.temp.2), self.temp.2)).size(18.0).strong());
        });
        ui.separator();
        ui.strong("💱 Devises");
        if self.taux.lock().unwrap().is_none() {
            *self.taux.lock().unwrap() = Some(Err("chargement des taux…".into()));
            let t = self.taux.clone();
            std::thread::spawn(move || {
                let r = ureq::get("https://open.er-api.com/v6/latest/EUR").timeout(Duration::from_secs(15)).call().map_err(|e| e.to_string())
                    .and_then(|r| r.into_json::<serde_json::Value>().map_err(|e| e.to_string()))
                    .map(|j| j["rates"].as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect()).unwrap_or_default());
                *t.lock().unwrap() = Some(r);
            });
        }
        match self.taux.lock().unwrap().clone() {
            Some(Ok(t)) => {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.montant).speed(0.5));
                    for (d, id) in [(&mut self.devises.0, "dev1"), (&mut self.devises.1, "dev2")] {
                        egui::ComboBox::from_id_salt(id).width(70.0).selected_text(d.as_str()).show_ui(ui, |ui| {
                            for k in ["EUR", "USD", "GBP", "CHF", "CAD", "JPY", "CNY", "KRW", "MAD", "DZD", "TND", "XOF", "AUD", "BRL", "TRY", "PLN", "SEK"].iter().chain(t.keys().map(|s| s.as_str()).collect::<Vec<_>>().iter()) {
                                ui.selectable_value(d, k.to_string(), *k);
                            }
                        });
                    }
                    if let (Some(a), Some(b)) = (t.get(&self.devises.0), t.get(&self.devises.1)) {
                        ui.label(RichText::new(format!("= {:.2} {}", self.montant / a * b, self.devises.1)).size(18.0).strong());
                    }
                });
                ui.weak("Taux du jour (ExchangeRate-API).");
            }
            Some(Err(e)) => {
                ui.weak(e);
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 51 · Actus (flux RSS)
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Article {
    pub source: String,
    pub titre: String,
    pub lien: String,
    pub resume: String,
    pub date: String,
}

fn balise(x: &str, nom: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"(?s)<{nom}(?:\s[^>]*)?>(.*?)</{nom}>")).ok()?;
    re.captures(x).map(|c| nettoyer(&c[1]))
}

/// Retire CDATA, balises HTML et entités courantes.
pub fn nettoyer(s: &str) -> String {
    let s = s.trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>");
    let s = regex::Regex::new(r"<[^>]+>").unwrap().replace_all(s, " ");
    let s = s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#039;", "'").replace("&#39;", "'").replace("&apos;", "'").replace("&nbsp;", " ").replace("&#8217;", "’").replace("&amp;", "&");
    regex::Regex::new(r"\s+").unwrap().replace_all(&s, " ").trim().to_string()
}

/// Articles d'un flux RSS 2.0 ou Atom.
pub fn lire_flux(xml: &str, source: &str) -> Vec<Article> {
    let items = regex::Regex::new(r"(?s)<item[\s>](.*?)</item>|<entry[\s>](.*?)</entry>").unwrap();
    let lien_atom = regex::Regex::new(r#"<link[^>]*href="([^"]+)""#).unwrap();
    items.captures_iter(xml).filter_map(|c| {
        let b = c.get(1).or(c.get(2))?.as_str();
        let titre = balise(b, "title")?;
        let lien = balise(b, "link").filter(|l| l.starts_with("http")).or_else(|| lien_atom.captures(b).map(|x| x[1].to_string())).unwrap_or_default();
        let resume = balise(b, "description").or_else(|| balise(b, "summary")).or_else(|| balise(b, "content")).unwrap_or_default();
        let date = balise(b, "pubDate").or_else(|| balise(b, "updated")).or_else(|| balise(b, "published")).or_else(|| balise(b, "dc:date")).unwrap_or_default();
        Some(Article { source: source.to_string(), titre, lien, resume: resume.chars().take(300).collect(), date })
    }).collect()
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct ReglageRss {
    pub flux: Vec<(String, String)>,
    pub lus: Vec<String>,
}

impl Default for ReglageRss {
    fn default() -> Self {
        ReglageRss {
            flux: vec![
                ("Jeuxvideo.com".into(), "https://www.jeuxvideo.com/rss/rss.xml".into()),
                ("Numerama".into(), "https://www.numerama.com/feed/".into()),
                ("Le Monde".into(), "https://www.lemonde.fr/rss/une.xml".into()),
            ],
            lus: vec![],
        }
    }
}

pub struct Actus {
    r: Arc<Mutex<ReglageRss>>,
    articles: Arc<Mutex<Vec<Article>>>,
    charge: Arc<AtomicBool>,
    source: Option<String>,
    nom: String,
    url: String,
}

fn charger_flux(r: &ReglageRss) -> Vec<Article> {
    let mut v = vec![];
    for (n, u) in &r.flux {
        if let Ok(t) = ureq::get(u).set("User-Agent", "Mozilla/5.0 Outils").timeout(Duration::from_secs(15)).call().and_then(|x| Ok(x.into_string()?)) {
            v.extend(lire_flux(&t, n).into_iter().take(30));
        }
    }
    v
}

impl Actus {
    pub fn new() -> Self {
        let r: Arc<Mutex<ReglageRss>> = Arc::new(Mutex::new(charger("flux_rss")));
        let articles: Arc<Mutex<Vec<Article>>> = Arc::default();
        let charge: Arc<AtomicBool> = Arc::default();
        let (r2, a2, c2) = (r.clone(), articles.clone(), charge.clone());
        std::thread::spawn(move || loop {
            c2.store(true, Relaxed);
            let v = charger_flux(&r2.lock().unwrap().clone());
            if !v.is_empty() {
                *a2.lock().unwrap() = v;
            }
            c2.store(false, Relaxed);
            std::thread::sleep(Duration::from_secs(1800));
        });
        Actus { r, articles, charge, source: None, nom: String::new(), url: String::new() }
    }
}

impl Outil for Actus {
    fn nom(&self) -> &'static str {
        "📰 Actus (RSS)"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let lus = self.r.lock().unwrap().lus.clone();
        let n = self.articles.lock().unwrap().iter().filter(|a| !lus.contains(&a.lien)).count();
        (n > 0).then(|| format!("📰 {n} article(s) non lu(s)"))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Actus");
        let mut r = self.r.lock().unwrap().clone();
        let avant = r.clone();
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.source.is_none(), "Toutes").clicked() {
                self.source = None;
            }
            let mut retirer = None;
            for (k, (n, _)) in r.flux.iter().enumerate() {
                let b = ui.selectable_label(self.source.as_ref() == Some(n), n);
                if b.clicked() {
                    self.source = Some(n.clone());
                }
                b.context_menu(|ui| {
                    if ui.button("🗑 Retirer ce flux").clicked() {
                        retirer = Some(k);
                        ui.close_menu();
                    }
                });
            }
            if let Some(k) = retirer {
                r.flux.remove(k);
            }
            if self.charge.load(Relaxed) {
                ui.spinner();
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else if ui.small_button("⟳").clicked() {
                let (a, c, rr) = (self.articles.clone(), self.charge.clone(), r.clone());
                c.store(true, Relaxed);
                std::thread::spawn(move || {
                    *a.lock().unwrap() = charger_flux(&rr);
                    c.store(false, Relaxed);
                });
            }
        });
        ui.collapsing("➕ Ajouter un site", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.nom).hint_text("nom").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(&mut self.url).hint_text("adresse du flux RSS (…/feed, …/rss.xml)").desired_width(300.0));
                if ui.button("Ajouter").clicked() && self.url.starts_with("http") {
                    let n = if self.nom.trim().is_empty() { self.url.split('/').nth(2).unwrap_or("Site").to_string() } else { self.nom.trim().to_string() };
                    r.flux.push((n, std::mem::take(&mut self.url)));
                    self.nom.clear();
                }
            });
            ui.weak("La plupart des sites ont un flux : cherche « RSS » en bas de page, ou ajoute /feed à l'adresse.");
        });
        if ui.small_button("✔ Tout marquer comme lu").clicked() {
            r.lus = self.articles.lock().unwrap().iter().map(|a| a.lien.clone()).collect();
        }
        let a = self.articles.lock().unwrap().clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for x in a.iter().filter(|x| self.source.as_ref().is_none_or(|s| *s == x.source)) {
                let lu = r.lus.contains(&x.lien);
                ui.horizontal(|ui| {
                    ui.weak(&x.source);
                    let t = RichText::new(&x.titre).size(15.0);
                    if ui.link(if lu { t.weak() } else { t.strong() }).clicked() {
                        let _ = std::process::Command::new("explorer").arg(&x.lien).spawn();
                        if !lu {
                            r.lus.push(x.lien.clone());
                        }
                    }
                });
                if !x.resume.is_empty() && !lu {
                    ui.label(RichText::new(&x.resume).small());
                }
                ui.add_space(4.0);
            }
        });
        if r.lus.len() > 2000 {
            let n = r.lus.len() - 2000;
            r.lus.drain(..n);
        }
        if r != avant {
            enregistrer("flux_rss", &r);
            *self.r.lock().unwrap() = r;
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 52 · Comptes à rebours
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Rebours {
    pub nom: String,
    pub date: String,
    pub emoji: String,
}

pub struct ComptesRebours {
    l: Vec<Rebours>,
    nouveau: Rebours,
    prevenu: String,
}

impl ComptesRebours {
    pub fn new() -> Self {
        ComptesRebours { l: charger("rebours"), nouveau: Rebours { emoji: "🎉".into(), ..Default::default() }, prevenu: String::new() }
    }
}

impl Outil for ComptesRebours {
    fn nom(&self) -> &'static str {
        "⏳ Comptes à rebours"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let (auj, _) = maintenant();
        let p = self.l.iter().filter_map(|r| Some((jours_entre(&auj, &r.date)?, r))).filter(|x| x.0 >= 0).min_by_key(|x| x.0)?;
        Some(if p.0 == 0 { format!("{} C'est aujourd'hui : {} !", p.1.emoji, p.1.nom) } else { format!("{} {} dans {} jour(s)", p.1.emoji, p.1.nom, p.0) })
    }
    fn tic(&mut self, _: &egui::Context) {
        let (auj, _) = maintenant();
        if self.prevenu != auj {
            self.prevenu = auj.clone();
            for r in &self.l {
                if r.date == auj {
                    crate::systeme::notifier_toujours(&format!("{} C'est le jour !", r.emoji), &r.nom);
                }
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Comptes à rebours");
        let (auj, _) = maintenant();
        let mut v: Vec<(i64, usize)> = self.l.iter().enumerate().filter_map(|(k, r)| Some((jours_entre(&auj, &r.date)?, k))).collect();
        v.sort();
        let mut retirer = None;
        ui.horizontal_wrapped(|ui| {
            for (j, k) in &v {
                let r = &self.l[*k];
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(190.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(&r.emoji).size(32.0));
                        ui.label(RichText::new(&r.nom).strong());
                        let (texte, c) = match j {
                            0 => ("Aujourd'hui !".to_string(), Color32::from_rgb(255, 120, 170)),
                            j if *j < 0 => (format!("il y a {} jour(s)", -j), Color32::GRAY),
                            1 => ("Demain".into(), Color32::from_rgb(250, 210, 90)),
                            j => (format!("{j} jours"), Color32::WHITE),
                        };
                        ui.label(RichText::new(texte).size(26.0).color(c));
                        if *j > 13 {
                            ui.weak(format!("{} semaines", j / 7));
                        }
                        ui.weak(lire_date(&r.date).map(|(a, m, d)| format!("{d:02}/{m:02}/{a}")).unwrap_or_default());
                        if ui.small_button("🗑").clicked() {
                            retirer = Some(*k);
                        }
                    });
                });
            }
        });
        if let Some(k) = retirer {
            self.l.remove(k);
            enregistrer("rebours", &self.l);
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.emoji).desired_width(30.0));
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.nom).hint_text("vacances, sortie d'un jeu, anniversaire…").desired_width(220.0));
            ui.add(egui::TextEdit::singleline(&mut self.nouveau.date).hint_text("JJ/MM/AAAA").desired_width(90.0));
            let date = {
                let p: Vec<&str> = self.nouveau.date.split(['/', '-', '.']).collect();
                (p.len() == 3).then(|| (p[2].parse::<i32>().ok(), p[1].parse::<u32>().ok(), p[0].parse::<u32>().ok())).and_then(|(a, m, j)| Some(texte_date(a?, m?, j?)))
            };
            if ui.add_enabled(date.is_some() && !self.nouveau.nom.trim().is_empty(), egui::Button::new("➕ Ajouter")).clicked() {
                let mut n = std::mem::replace(&mut self.nouveau, Rebours { emoji: "🎉".into(), ..Default::default() });
                n.date = date.unwrap();
                self.l.push(n);
                enregistrer("rebours", &self.l);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for e in ["🎉", "🏖", "🎂", "🎮", "🎄", "✈", "🎓", "💍", "🎬", "⚽"] {
                if ui.small_button(e).clicked() {
                    self.nouveau.emoji = e.into();
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------------
// 55 · Emploi du temps et devoirs
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Cours {
    pub jour: usize,
    pub debut: u32,
    pub fin: u32,
    pub matiere: String,
    pub salle: String,
    pub couleur: [u8; 3],
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Devoir {
    pub matiere: String,
    pub pour: String,
    pub texte: String,
    pub fait: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Scolaire {
    pub cours: Vec<Cours>,
    pub devoirs: Vec<Devoir>,
    pub rappel_veille: bool,
    pub prevenu: String,
}

/// Cours en cours ou prochain du jour : (cours, déjà commencé).
pub fn cours_maintenant(c: &[Cours], jour: usize, minute: u32) -> Option<(&Cours, bool)> {
    c.iter().filter(|x| x.jour == jour && x.fin > minute).min_by_key(|x| x.debut).map(|x| (x, x.debut <= minute))
}

const COULEURS: [[u8; 3]; 8] = [[255, 120, 170], [120, 170, 255], [110, 220, 130], [250, 210, 90], [190, 120, 255], [255, 160, 80], [90, 200, 230], [200, 200, 200]];

pub struct Ecole {
    s: Scolaire,
    vue: u8,
    nc: Cours,
    nc_heures: (String, String),
    nd: Devoir,
    nd_date: String,
}

impl Ecole {
    pub fn new() -> Self {
        Ecole { s: charger("ecole"), vue: 0, nc: Cours { couleur: COULEURS[0], ..Default::default() }, nc_heures: ("8h".into(), "9h".into()), nd: Devoir::default(), nd_date: String::new() }
    }
}

fn hm(m: u32) -> String {
    format!("{}h{:02}", m / 60, m % 60)
}

impl Outil for Ecole {
    fn nom(&self) -> &'static str {
        "🎒 Emploi du temps et devoirs"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let (auj, m) = maintenant();
        let (a, mo, j) = lire_date(&auj)?;
        let js = jour_semaine(a, mo, j) as usize;
        let n = jours_depuis_1970(a, mo, j);
        let (da, dm, dj) = date_de_jours(n + 1);
        let demain = texte_date(da, dm, dj);
        let devoirs = self.s.devoirs.iter().filter(|d| !d.fait && (d.pour == demain || d.pour == auj)).count();
        let cours = cours_maintenant(&self.s.cours, js, m).map(|(c, en)| if en { format!("📚 {} jusqu'à {}", c.matiere, hm(c.fin)) } else { format!("📚 {} à {}", c.matiere, hm(c.debut)) });
        let d = (devoirs > 0).then(|| format!("{devoirs} devoir(s) pour demain"));
        match (cours, d) {
            (None, None) => None,
            (a, b) => Some([a, b].into_iter().flatten().collect::<Vec<_>>().join(" · ")),
        }
    }
    fn tic(&mut self, _: &egui::Context) {
        // la veille à 18 h : rappel des devoirs pas faits
        let (auj, m) = maintenant();
        if self.s.rappel_veille && m >= 18 * 60 && self.s.prevenu != auj {
            self.s.prevenu = auj.clone();
            if let Some((a, mo, j)) = lire_date(&auj) {
                let (da, dm, dj) = date_de_jours(jours_depuis_1970(a, mo, j) + 1);
                let demain = texte_date(da, dm, dj);
                let l: Vec<String> = self.s.devoirs.iter().filter(|d| !d.fait && d.pour == demain).map(|d| d.matiere.clone()).collect();
                if !l.is_empty() {
                    crate::systeme::notifier_toujours("🎒 Devoirs pour demain", &l.join(", "));
                }
            }
            enregistrer("ecole", &self.s);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Emploi du temps et devoirs");
        let avant = self.s.clone();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.vue, 0, "📅 Emploi du temps");
            ui.selectable_value(&mut self.vue, 1, format!("📝 Devoirs ({})", self.s.devoirs.iter().filter(|d| !d.fait).count()));
        });
        let (auj, minute) = maintenant();
        let js = lire_date(&auj).map(|(a, m, j)| jour_semaine(a, m, j) as usize).unwrap_or(0);
        if self.vue == 0 {
            let debut = self.s.cours.iter().map(|c| c.debut).min().unwrap_or(8 * 60).min(8 * 60) / 60 * 60;
            let fin = self.s.cours.iter().map(|c| c.fin).max().unwrap_or(17 * 60).max(17 * 60);
            let jours = if self.s.cours.iter().any(|c| c.jour == 5) { 6 } else { 5 };
            let h_px = 0.9f32;
            let (zone, _) = ui.allocate_exact_size(egui::vec2(60.0 + jours as f32 * 130.0, 24.0 + (fin - debut) as f32 * h_px), egui::Sense::hover());
            let p = ui.painter_at(zone);
            let y = |m: u32| zone.top() + 24.0 + (m - debut) as f32 * h_px;
            for h in (debut / 60)..=(fin / 60) {
                p.text(egui::pos2(zone.left() + 4.0, y(h * 60)), egui::Align2::LEFT_CENTER, format!("{h}h"), egui::FontId::proportional(12.0), Color32::GRAY);
                p.hline(zone.left() + 40.0..=zone.right(), y(h * 60), egui::Stroke::new(0.5_f32, Color32::from_gray(60)));
            }
            let mut retirer = None;
            for d in 0..jours {
                let x = zone.left() + 60.0 + d as f32 * 130.0;
                p.text(egui::pos2(x + 62.0, zone.top() + 10.0), egui::Align2::CENTER_CENTER, JOURS[d], egui::FontId::proportional(14.0), if d == js { Color32::from_rgb(255, 120, 170) } else { Color32::WHITE });
                for (k, c) in self.s.cours.iter().enumerate().filter(|(_, c)| c.jour == d) {
                    let r = egui::Rect::from_min_max(egui::pos2(x + 2.0, y(c.debut) + 1.0), egui::pos2(x + 126.0, y(c.fin) - 1.0));
                    let col = Color32::from_rgb(c.couleur[0], c.couleur[1], c.couleur[2]);
                    p.rect_filled(r, 4.0, col.gamma_multiply(if d == js && c.debut <= minute && minute < c.fin { 1.0 } else { 0.55 }));
                    p.text(r.left_top() + egui::vec2(5.0, 3.0), egui::Align2::LEFT_TOP, format!("{}\n{} {}", c.matiere, hm(c.debut), c.salle), egui::FontId::proportional(12.0), Color32::BLACK);
                    if ui.interact(r, ui.id().with(("cours", k)), egui::Sense::click()).on_hover_text("Clic droit : supprimer").secondary_clicked() {
                        retirer = Some(k);
                    }
                }
            }
            if minute >= debut && minute <= fin && js < jours {
                p.hline(zone.left() + 60.0 + js as f32 * 130.0..=zone.left() + 60.0 + js as f32 * 130.0 + 128.0, y(minute), egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 80, 80)));
            }
            if let Some(k) = retirer {
                self.s.cours.remove(k);
            }
            ui.separator();
            ui.strong("Ajouter un cours");
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("jour_cours").width(100.0).selected_text(JOURS[self.nc.jour]).show_ui(ui, |ui| {
                    for (k, j) in JOURS.iter().enumerate().take(6) {
                        ui.selectable_value(&mut self.nc.jour, k, *j);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.nc_heures.0).desired_width(50.0));
                ui.label("→");
                ui.add(egui::TextEdit::singleline(&mut self.nc_heures.1).desired_width(50.0));
                ui.add(egui::TextEdit::singleline(&mut self.nc.matiere).hint_text("matière").desired_width(120.0));
                ui.add(egui::TextEdit::singleline(&mut self.nc.salle).hint_text("salle").desired_width(60.0));
                for c in COULEURS {
                    let b = egui::Button::new("  ").fill(Color32::from_rgb(c[0], c[1], c[2])).selected(self.nc.couleur == c);
                    if ui.add(b).clicked() {
                        self.nc.couleur = c;
                    }
                }
                let h = (lire_heure(&self.nc_heures.0), lire_heure(&self.nc_heures.1));
                if ui.add_enabled(matches!(h, (Some(a), Some(b)) if b > a) && !self.nc.matiere.trim().is_empty(), egui::Button::new("➕")).clicked() {
                    let mut c = self.nc.clone();
                    (c.debut, c.fin) = (h.0.unwrap(), h.1.unwrap());
                    self.s.cours.push(c);
                    self.nc_heures = (self.nc_heures.1.clone(), String::new());
                }
            });
            // couleur automatique par matière
            if let Some(c) = self.s.cours.iter().find(|c| c.matiere.eq_ignore_ascii_case(self.nc.matiere.trim()) && !self.nc.matiere.trim().is_empty()) {
                self.nc.couleur = c.couleur;
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                let matieres: Vec<String> = {
                    let mut m: Vec<String> = self.s.cours.iter().map(|c| c.matiere.clone()).collect();
                    m.sort();
                    m.dedup();
                    m
                };
                egui::ComboBox::from_id_salt("mat_dev").width(120.0).selected_text(if self.nd.matiere.is_empty() { "matière".to_string() } else { self.nd.matiere.clone() }).show_ui(ui, |ui| {
                    for m in matieres {
                        ui.selectable_value(&mut self.nd.matiere, m.clone(), m);
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.nd.texte).hint_text("à faire").desired_width(240.0));
                ui.add(egui::TextEdit::singleline(&mut self.nd_date).hint_text("pour le JJ/MM").desired_width(80.0));
                let date = {
                    let p: Vec<&str> = self.nd_date.split(['/', '-']).collect();
                    let a = auj.get(..4).and_then(|x| x.parse::<i32>().ok()).unwrap_or(2026);
                    (p.len() >= 2).then(|| (p[1].parse::<u32>().ok(), p[0].parse::<u32>().ok())).and_then(|(m, j)| {
                        let (m, j) = (m?, j?);
                        let t = texte_date(a, m, j);
                        // une date déjà passée = l'année prochaine
                        Some(if t < auj { texte_date(a + 1, m, j) } else { t })
                    })
                };
                if ui.add_enabled(date.is_some() && !self.nd.texte.trim().is_empty(), egui::Button::new("➕")).clicked() {
                    let mut d = std::mem::take(&mut self.nd);
                    d.pour = date.unwrap();
                    self.s.devoirs.push(d);
                    self.nd_date.clear();
                }
            });
            ui.checkbox(&mut self.s.rappel_veille, "Me rappeler la veille à 18 h les devoirs pas faits");
            self.s.devoirs.sort_by(|a, b| a.fait.cmp(&b.fait).then(a.pour.cmp(&b.pour)));
            let mut retirer = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (k, d) in self.s.devoirs.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut d.fait, "");
                        let reste = jours_entre(&auj, &d.pour).unwrap_or(0);
                        let (quand, c) = match reste {
                            r if r < 0 => ("en retard".to_string(), Color32::LIGHT_RED),
                            0 => ("aujourd'hui".into(), Color32::LIGHT_RED),
                            1 => ("demain".into(), Color32::from_rgb(250, 210, 90)),
                            r => (lire_date(&d.pour).map(|(a, m, j)| format!("{} {j}/{m:02}", JOURS[jour_semaine(a, m, j) as usize])).unwrap_or(format!("dans {r} j")), Color32::GRAY),
                        };
                        ui.colored_label(if d.fait { Color32::DARK_GRAY } else { c }, quand);
                        ui.strong(&d.matiere);
                        ui.label(if d.fait { RichText::new(&d.texte).strikethrough().weak() } else { RichText::new(&d.texte) });
                        if ui.small_button("🗑").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
            });
            if let Some(k) = retirer {
                self.s.devoirs.remove(k);
            }
        }
        if self.s != avant {
            enregistrer("ecole", &self.s);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 56 · Radio en ligne
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
#[serde(default)]
pub struct Station {
    pub nom: String,
    pub url: String,
    pub pays: String,
    pub genres: String,
}

/// « StreamTitle='Artiste - Titre'; » ou ligne ffmpeg « StreamTitle : … »
pub fn titre_en_cours(l: &str) -> Option<String> {
    let c = regex::Regex::new(r"StreamTitle\s*[:=]\s*'?([^';]*)'?").unwrap().captures(l)?;
    let t = c[1].trim().to_string();
    (!t.is_empty()).then_some(t)
}

struct Lecteur {
    _flux: rodio::OutputStream,
    sink: Arc<rodio::Sink>,
    stop: Arc<AtomicBool>,
}

pub struct Radio {
    favoris: Vec<Station>,
    q: String,
    res: Arc<Mutex<Option<Result<Vec<Station>, String>>>>,
    lecture: Option<(Station, Lecteur)>,
    titre: Arc<Mutex<String>>,
    etat: Arc<Mutex<String>>,
    volume: f32,
}

fn chercher_radios(q: &str) -> Result<Vec<Station>, String> {
    let mut r = ureq::get("https://de1.api.radio-browser.info/json/stations/search").set("User-Agent", "Outils/1.0").timeout(Duration::from_secs(15))
        .query("limit", "40").query("order", "clickcount").query("reverse", "true").query("hidebroken", "true");
    r = if q.trim().is_empty() { r.query("countrycode", "FR") } else { r.query("name", q.trim()) };
    let j: serde_json::Value = r.call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    Ok(j.as_array().into_iter().flatten().filter_map(|s| Some(Station {
        nom: s["name"].as_str()?.trim().to_string(),
        url: s["url_resolved"].as_str().filter(|u| !u.is_empty()).or(s["url"].as_str())?.to_string(),
        pays: s["countrycode"].as_str().unwrap_or("").to_string(),
        genres: s["tags"].as_str().unwrap_or("").split(',').take(3).collect::<Vec<_>>().join(", "),
    })).collect())
}

impl Radio {
    pub fn new() -> Self {
        let mut favoris: Vec<Station> = charger("radios");
        if favoris.is_empty() {
            favoris = vec![
                Station { nom: "France Inter".into(), url: "https://icecast.radiofrance.fr/franceinter-midfi.mp3".into(), pays: "FR".into(), genres: "généraliste".into() },
                Station { nom: "FIP".into(), url: "https://icecast.radiofrance.fr/fip-midfi.mp3".into(), pays: "FR".into(), genres: "éclectique".into() },
                Station { nom: "Mouv'".into(), url: "https://icecast.radiofrance.fr/mouv-midfi.mp3".into(), pays: "FR".into(), genres: "rap".into() },
            ];
        }
        Radio { favoris, q: String::new(), res: Arc::default(), lecture: None, titre: Arc::default(), etat: Arc::default(), volume: charger::<Option<f32>>("radio_volume").unwrap_or(0.7) }
    }

    fn arreter(&mut self) {
        if let Some((_, l)) = self.lecture.take() {
            l.stop.store(true, Relaxed);
            l.sink.stop();
        }
        self.titre.lock().unwrap().clear();
    }

    /// ffmpeg décode le flux (MP3, AAC, HLS…) en PCM, rodio le joue.
    fn jouer(&mut self, s: Station) {
        self.arreter();
        let (flux, h) = match rodio::OutputStream::try_default() {
            Ok(x) => x,
            Err(e) => {
                *self.etat.lock().unwrap() = format!("sortie audio : {e}");
                return;
            }
        };
        let Ok(sink) = rodio::Sink::try_new(&h) else { return };
        sink.set_volume(self.volume);
        let sink = Arc::new(sink);
        let stop: Arc<AtomicBool> = Arc::default();
        let (sk, st, titre, etat, url) = (sink.clone(), stop.clone(), self.titre.clone(), self.etat.clone(), s.url.clone());
        *etat.lock().unwrap() = "Connexion…".into();
        std::thread::spawn(move || {
            let mut c = match crate::dependances::commande("ffmpeg") {
                Ok(c) => c,
                Err(e) => {
                    *etat.lock().unwrap() = e;
                    return;
                }
            };
            let enfant = c.args(["-hide_banner", "-loglevel", "info", "-reconnect", "1", "-reconnect_streamed", "1", "-reconnect_delay_max", "5", "-i", &url, "-vn", "-f", "s16le", "-ac", "2", "-ar", "44100", "-"])
                .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null()).spawn();
            let mut enfant = match enfant {
                Ok(e) => e,
                Err(e) => {
                    *etat.lock().unwrap() = e.to_string();
                    return;
                }
            };
            if let Some(err) = enfant.stderr.take() {
                let t = titre.clone();
                std::thread::spawn(move || {
                    for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                        if let Some(x) = titre_en_cours(&l) {
                            *t.lock().unwrap() = x;
                        }
                    }
                });
            }
            let mut sortie = enfant.stdout.take().unwrap();
            let mut buf = vec![0u8; 44_100]; // ≈ 0,25 s
            let mut premier = true;
            while !st.load(Relaxed) {
                let mut n = 0;
                while n < buf.len() {
                    match sortie.read(&mut buf[n..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => n += k,
                    }
                }
                if n < 4 {
                    *etat.lock().unwrap() = "Flux interrompu.".into();
                    break;
                }
                if premier {
                    premier = false;
                    *etat.lock().unwrap() = String::new();
                }
                let ech: Vec<i16> = buf[..n - n % 4].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
                sk.append(rodio::buffer::SamplesBuffer::new(2, 44_100, ech));
                // pas plus d'une seconde d'avance
                while sk.len() > 4 && !st.load(Relaxed) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            let _ = enfant.kill();
        });
        self.lecture = Some((s, Lecteur { _flux: flux, sink, stop }));
    }
}

impl Outil for Radio {
    fn nom(&self) -> &'static str {
        "📻 Radio en ligne"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn resume(&self) -> Option<String> {
        let (s, _) = self.lecture.as_ref()?;
        let t = self.titre.lock().unwrap().clone();
        Some(format!("📻 {}{}", s.nom, if t.is_empty() { String::new() } else { format!(" — {t}") }))
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Radio en ligne");
        egui::Frame::group(ui.style()).show(ui, |ui| {
            match self.lecture.as_ref().map(|x| x.0.nom.clone()) {
                Some(nom) => {
                    let mut stop = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("📻 {nom}")).size(18.0).strong());
                        stop = ui.button("⏹").clicked();
                    });
                    if stop {
                        self.arreter();
                    }
                    let t = self.titre.lock().unwrap().clone();
                    if !t.is_empty() {
                        ui.label(format!("🎵 {t}"));
                    }
                    let e = self.etat.lock().unwrap().clone();
                    if !e.is_empty() {
                        ui.weak(e);
                    }
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }
                None => {
                    ui.weak("Choisis une radio.");
                }
            }
            ui.horizontal(|ui| {
                ui.label("🔊");
                if ui.add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false)).changed() {
                    if let Some((_, l)) = &self.lecture {
                        l.sink.set_volume(self.volume);
                    }
                    enregistrer("radio_volume", &Some(self.volume));
                }
            });
        });
        ui.strong("⭐ Mes radios");
        let mut jouer = None;
        let mut retirer = None;
        ui.horizontal_wrapped(|ui| {
            for (k, s) in self.favoris.iter().enumerate() {
                let b = ui.button(&s.nom).on_hover_text("Clic droit : retirer");
                if b.clicked() {
                    jouer = Some(s.clone());
                }
                if b.secondary_clicked() {
                    retirer = Some(k);
                }
            }
        });
        if let Some(k) = retirer {
            self.favoris.remove(k);
            enregistrer("radios", &self.favoris);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.q).hint_text("chercher une radio (vide = les plus écoutées en France)").desired_width(320.0));
            if ui.button("🔍").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let (q, res) = (self.q.clone(), self.res.clone());
                *res.lock().unwrap() = Some(Err("recherche…".into()));
                std::thread::spawn(move || *res.lock().unwrap() = Some(chercher_radios(&q)));
            }
        });
        let mut garder = None;
        match self.res.lock().unwrap().clone() {
            Some(Ok(v)) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for s in v {
                        ui.horizontal(|ui| {
                            if ui.small_button("▶").clicked() {
                                jouer = Some(s.clone());
                            }
                            if ui.small_button("⭐").clicked() {
                                garder = Some(s.clone());
                            }
                            ui.strong(&s.nom);
                            ui.weak(format!("{} · {}", s.pays, s.genres));
                        });
                    }
                });
            }
            Some(Err(e)) => {
                ui.weak(e);
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
            None => {}
        }
        if let Some(s) = garder {
            if !self.favoris.iter().any(|x| x.url == s.url) {
                self.favoris.push(s);
                enregistrer("radios", &self.favoris);
            }
        }
        if let Some(s) = jouer {
            self.jouer(s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculs() {
        let ok = |s: &str| calculer(s).unwrap();
        assert_eq!(ok("1+2*3"), 7.0);
        assert_eq!(ok("(1+2)*3"), 9.0);
        assert_eq!(ok("2^3^2"), 512.0);
        assert_eq!(ok("-2^2"), -4.0);
        assert_eq!(ok("12,5 + 0,5"), 13.0);
        assert_eq!(ok("250*15%"), 37.5);
        assert_eq!(ok("7 % 3"), 1.0);
        assert_eq!(ok("sqrt(16) + abs(-2)"), 6.0);
        assert!((ok("2pi") - std::f64::consts::TAU).abs() < 1e-12);
        assert!((ok("sin(30)") - 0.5).abs() < 1e-12);
        assert_eq!(ok("3(2+1)"), 9.0);
        assert_eq!(ok("1e3+1"), 1001.0);
        assert!(calculer("1/0").is_err());
        assert!(calculer("(1+2").is_err());
        assert!(calculer("2+").is_err());
        assert_eq!(joli(1234567.0), "1 234 567");
        assert_eq!(joli(-1500.0), "-1 500");
        assert_eq!(joli(0.1 + 0.2), "0,3");
        assert_eq!(convertir(1.0, 1609.344, 1000.0), 1.609344);
        assert!((temperature(100.0, "°C", "°F") - 212.0).abs() < 1e-9);
        assert!((temperature(0.0, "K", "°C") + 273.15).abs() < 1e-9);
    }

    #[test]
    fn flux_et_divers() {
        let rss = r#"<rss><channel><title>Site</title><item><title><![CDATA[Un &amp; deux]]></title><link>https://a.fr/1</link><description>&lt;p&gt;Texte&lt;/p&gt; ici</description><pubDate>Mon, 01 Jan 2026</pubDate></item></channel></rss>"#;
        let v = lire_flux(rss, "S");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].titre, "Un & deux");
        assert_eq!(v[0].lien, "https://a.fr/1");
        assert_eq!(v[0].resume, "<p>Texte</p> ici");
        let atom = r#"<feed><entry><title>Atom!</title><link rel="alternate" href="https://b.fr/x"/><summary>Résumé</summary><updated>2026-01-01</updated></entry></feed>"#;
        let v = lire_flux(atom, "A");
        assert_eq!((v[0].titre.as_str(), v[0].lien.as_str()), ("Atom!", "https://b.fr/x"));
        assert_eq!(titre_en_cours("    StreamTitle     : Daft Punk - One More Time"), Some("Daft Punk - One More Time".into()));
        assert_eq!(titre_en_cours("StreamTitle='Artiste - Titre';StreamUrl='';"), Some("Artiste - Titre".into()));
        assert_eq!(titre_en_cours("StreamTitle=''"), None);
        let c = vec![Cours { jour: 0, debut: 480, fin: 540, matiere: "Maths".into(), ..Default::default() }, Cours { jour: 0, debut: 600, fin: 660, matiere: "Histoire".into(), ..Default::default() }];
        assert_eq!(cours_maintenant(&c, 0, 500).map(|x| (x.0.matiere.as_str(), x.1)), Some(("Maths", true)));
        assert_eq!(cours_maintenant(&c, 0, 545).map(|x| (x.0.matiere.as_str(), x.1)), Some(("Histoire", false)));
        assert!(cours_maintenant(&c, 0, 700).is_none());
        assert_eq!(jours_entre("2026-12-30", "2027-01-02"), Some(3));
    }
}
