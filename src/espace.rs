//! Espace : planétarium (le ciel de ce soir au-dessus de chez toi, 11) et système solaire en 3D (12).
//! Positions des planètes : éléments orbitaux approchés du JPL (précision d'environ un degré, largement
//! assez pour repérer une planète). Étoiles et constellations : d3-celestial (Olaf Frohn, BSD 3-Clause).
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

const RAD: f64 = PI / 180.0;

pub fn jour_julien(unix: f64) -> f64 {
    unix / 86400.0 + 2440587.5
}

pub fn maintenant() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// (nom, a, e, I, L, ϖ, Ω) puis leurs variations par siècle (JPL, 1800-2050).
pub const PLANETES: [(&str, [f64; 6], [f64; 6]); 9] = [
    ("Mercure", [0.38709927, 0.20563593, 7.00497902, 252.25032350, 77.45779628, 48.33076593], [0.00000037, 0.00001906, -0.00594749, 149472.67411175, 0.16047689, -0.12534081]),
    ("Vénus", [0.72333566, 0.00677672, 3.39467605, 181.97909950, 131.60246718, 76.67984255], [0.00000390, -0.00004107, -0.00078890, 58517.81538729, 0.00268329, -0.27769418]),
    ("Terre", [1.00000261, 0.01671123, -0.00001531, 100.46457166, 102.93768193, 0.0], [0.00000562, -0.00004392, -0.01294668, 35999.37244981, 0.32327364, 0.0]),
    ("Mars", [1.52371034, 0.09339410, 1.84969142, -4.55343205, -23.94362959, 49.55953891], [0.00001847, 0.00007882, -0.00813131, 19140.30268499, 0.44441088, -0.29257343]),
    ("Jupiter", [5.20288700, 0.04838624, 1.30439695, 34.39644051, 14.72847983, 100.47390909], [-0.00011607, -0.00013253, -0.00183714, 3034.74612775, 0.21252668, 0.20469106]),
    ("Saturne", [9.53667594, 0.05386179, 2.48599187, 49.95424423, 92.59887831, 113.66242448], [-0.00125060, -0.00050991, 0.00193609, 1222.49362201, -0.41897216, -0.28867794]),
    ("Uranus", [19.18916464, 0.04725744, 0.77263783, 313.23810451, 170.95427630, 74.01692503], [-0.00196176, -0.00004397, -0.00242939, 428.48202785, 0.40805281, 0.04240589]),
    ("Neptune", [30.06992276, 0.00859048, 1.77004347, -55.12002969, 44.96476227, 131.78422574], [0.00026291, 0.00005105, 0.00035372, 218.45945325, -0.32241464, -0.00508664]),
    // Pluton (planète naine), éléments moyens J2000
    ("Pluton", [39.48211675, 0.24882730, 17.14001206, 238.92903833, 224.06891629, 110.30393684], [-0.00031596, 0.00005170, 0.00004818, 145.20780515, -0.04062942, -0.01183482]),
];

/// Couleur, rayon affiché, et fiche (diamètre, jour, année, lunes, le saviez-tu).
pub const FICHES: [([u8; 3], f32, &str); 9] = [
    ([180, 170, 160], 3.0, "4 879 km · un jour = 176 jours terrestres · une année = 88 jours · 0 lune. La planète la plus proche du Soleil : 430 °C le jour, −180 °C la nuit."),
    ([240, 210, 140], 5.0, "12 104 km · un jour = 243 jours · une année = 225 jours · 0 lune. La plus chaude (465 °C) à cause de son épaisse atmosphère ; elle tourne à l'envers."),
    ([80, 140, 240], 5.0, "12 742 km · un jour = 24 h · une année = 365,25 jours · 1 lune. La seule planète connue avec de la vie."),
    ([220, 100, 60], 4.0, "6 779 km · un jour = 24 h 37 · une année = 687 jours · 2 lunes (Phobos, Déimos). Le volcan Olympus Mons y fait 22 km de haut."),
    ([220, 180, 140], 11.0, "139 820 km · un jour = 10 h · une année = 11,9 ans · 95 lunes. La plus grosse : 1 300 Terres tiendraient dedans. Sa Grande Tache rouge est une tempête plus grande que la Terre."),
    ([230, 210, 150], 10.0, "116 460 km · un jour = 10 h 30 · une année = 29,5 ans · 146 lunes. Ses anneaux font 280 000 km de large mais à peine 1 km d'épaisseur."),
    ([150, 220, 230], 7.0, "50 724 km · un jour = 17 h · une année = 84 ans · 28 lunes. Elle tourne couchée sur le côté."),
    ([80, 110, 230], 7.0, "49 244 km · un jour = 16 h · une année = 165 ans · 16 lunes. Les vents les plus rapides du système solaire : 2 000 km/h."),
    ([200, 180, 160], 2.5, "2 377 km · un jour = 6,4 jours · une année = 248 ans · 5 lunes. Planète naine depuis 2006 ; elle a un grand cœur de glace."),
];

/// Position héliocentrique écliptique (UA) de la planète `k` au jour julien `jd`.
pub fn position(k: usize, jd: f64) -> [f64; 3] {
    position_anomalie(k, jd, None)
}

fn position_anomalie(k: usize, jd: f64, m_force: Option<f64>) -> [f64; 3] {
    let t = (jd - 2451545.0) / 36525.0;
    let (_, e0, d) = PLANETES[k];
    let el: Vec<f64> = (0..6).map(|i| e0[i] + d[i] * t).collect();
    let (a, e, i, l, peri, noeud) = (el[0], el[1], el[2] * RAD, el[3], el[4], el[5]);
    let w = (peri - noeud) * RAD;
    let o = noeud * RAD;
    let m = m_force.unwrap_or(((l - peri) % 360.0) * RAD);
    // équation de Kepler
    let mut ea = m + e * m.sin();
    for _ in 0..12 {
        ea -= (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
    }
    let (xp, yp) = (a * (ea.cos() - e), a * (1.0 - e * e).sqrt() * ea.sin());
    let (cw, sw, co, so, ci, si) = (w.cos(), w.sin(), o.cos(), o.sin(), i.cos(), i.sin());
    [
        (cw * co - sw * so * ci) * xp + (-sw * co - cw * so * ci) * yp,
        (cw * so + sw * co * ci) * xp + (-sw * so + cw * co * ci) * yp,
        (sw * si) * xp + (cw * si) * yp,
    ]
}

/// Trajectoire complète de l'orbite (pour la dessiner).
pub fn orbite(k: usize, jd: f64, n: usize) -> Vec<[f64; 3]> {
    (0..=n).map(|i| position_anomalie(k, jd, Some(i as f64 / n as f64 * 2.0 * PI))).collect()
}

/// Écliptique → équatorial ; renvoie (ascension droite, déclinaison) en degrés.
pub fn equatorial(v: [f64; 3]) -> (f64, f64) {
    let eps = 23.43928 * RAD;
    let (x, y, z) = (v[0], v[1] * eps.cos() - v[2] * eps.sin(), v[1] * eps.sin() + v[2] * eps.cos());
    let ra = y.atan2(x) / RAD;
    (ra.rem_euclid(360.0), z.atan2((x * x + y * y).sqrt()) / RAD)
}

/// Ascension droite et déclinaison vues de la Terre : planètes (0..9 sauf la Terre), Soleil.
pub fn geocentrique(k: usize, jd: f64) -> (f64, f64) {
    let t = position(2, jd);
    let p = if k == usize::MAX { [0.0; 3] } else { position(k, jd) };
    equatorial([p[0] - t[0], p[1] - t[1], p[2] - t[2]])
}

/// Lune (formule courte) : (ascension droite, déclinaison, phase éclairée 0..1).
pub fn lune(jd: f64) -> (f64, f64, f64) {
    let d = jd - 2451545.0;
    let l0 = 218.316 + 13.176396 * d;
    let m = (134.963 + 13.064993 * d) * RAD;
    let f = (93.272 + 13.229350 * d) * RAD;
    let lambda = (l0 + 6.289 * m.sin()) * RAD;
    let beta = 5.128 * f.sin() * RAD;
    let v = [beta.cos() * lambda.cos(), beta.cos() * lambda.sin(), beta.sin()];
    let (ra, dec) = equatorial(v);
    let (sra, sdec) = geocentrique(usize::MAX, jd);
    // élongation Lune-Soleil
    let c = (dec * RAD).sin() * (sdec * RAD).sin() + (dec * RAD).cos() * (sdec * RAD).cos() * ((ra - sra) * RAD).cos();
    (ra, dec, (1.0 - c) / 2.0)
}

/// Hauteur et azimut (degrés, azimut depuis le nord vers l'est).
pub fn horizontal(ra: f64, dec: f64, jd: f64, lat: f64, lon: f64) -> (f64, f64) {
    let gmst = 280.46061837 + 360.98564736629 * (jd - 2451545.0);
    let h = ((gmst + lon - ra).rem_euclid(360.0)) * RAD;
    let (p, d) = (lat * RAD, dec * RAD);
    let alt = (p.sin() * d.sin() + p.cos() * d.cos() * h.cos()).asin();
    let az = (-h.sin() * d.cos()).atan2(p.cos() * d.sin() - p.sin() * d.cos() * h.cos());
    (alt / RAD, (az / RAD).rem_euclid(360.0))
}

/// Couleur d'une étoile d'après son indice B-V.
fn couleur_etoile(bv: f32) -> Color32 {
    let t = ((bv + 0.3) / 2.0).clamp(0.0, 1.0);
    let (r, g, b) = if t < 0.4 { (170.0 + t * 200.0, 200.0 + t * 120.0, 255.0) } else { (255.0, 250.0 - (t - 0.4) * 170.0, 240.0 - (t - 0.4) * 300.0) };
    Color32::from_rgb(r.min(255.0) as u8, g.min(255.0) as u8, b.clamp(60.0, 255.0) as u8)
}

pub struct Ciel {
    pub etoiles: Vec<(f64, f64, f32, f32, String)>,
    pub constellations: Vec<(String, String, f64, f64)>,
    pub lignes: Vec<(String, Vec<(f64, f64)>)>,
}

pub fn ciel() -> Ciel {
    let mut c = Ciel { etoiles: Vec::new(), constellations: Vec::new(), lignes: Vec::new() };
    for l in include_str!("donnees/ciel.txt").lines() {
        let v: Vec<&str> = l.split(' ').collect();
        let n = |i: usize| v.get(i).and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0);
        match v[0] {
            "E" => c.etoiles.push((n(1), n(2), n(3) as f32, n(4) as f32, v.get(5).map(|s| s.to_string()).unwrap_or_default())),
            // le nom peut contenir des espaces : il est entre l'abréviation et les deux coordonnées
            "C" if v.len() >= 5 => c.constellations.push((v[1].into(), v[2..v.len() - 2].join(" ").replace(['_', '\u{2005}', '\u{a0}'], " "), n(v.len() - 2), n(v.len() - 1))),
            "L" => c.lignes.push((v[1].into(), v[2..].iter().filter_map(|p| p.split_once(',')).filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))).collect())),
            _ => {}
        }
    }
    c
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
struct Lieu {
    nom: String,
    lat: f64,
    lon: f64,
}

impl Default for Lieu {
    fn default() -> Self {
        Lieu { nom: "Paris".into(), lat: 48.857, lon: 2.352 }
    }
}

pub struct Planetarium {
    ciel: Ciel,
    lieu: Lieu,
    recherche: String,
    decalage_h: f64,
    noms: bool,
    lignes: bool,
    defile: f64,
    message: String,
}

impl Planetarium {
    pub fn new() -> Self {
        Planetarium { ciel: ciel(), lieu: charger("planetarium_lieu"), recherche: String::new(), decalage_h: 0.0, noms: true, lignes: true, defile: 0.0, message: String::new() }
    }
}

/// Jours depuis 1970 d'une date (calendrier grégorien).
fn jours_depuis_1970(a: i64, m: i64, j: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let ae = a - ere * 400;
    let mp = (m + 9) % 12;
    let jda = (153 * mp + 2) / 5 + j - 1;
    let jde = ae * 365 + ae / 4 - ae / 100 + jda;
    ere * 146097 + jde - 719468
}

/// Date d'un nombre de jours depuis 1970 : (année, mois, jour).
fn date_civile(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let ere = z.div_euclid(146097);
    let jde = z - ere * 146097;
    let ae = (jde - jde / 1460 + jde / 36524 - jde / 146096) / 365;
    let jda = jde - (365 * ae + ae / 4 - ae / 100);
    let mp = (5 * jda + 2) / 153;
    let j = jda - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (ae + ere * 400 + if m <= 2 { 1 } else { 0 }, m, j)
}

/// Décalage de l'heure locale (secondes), d'après l'horloge du système.
fn decalage_local() -> f64 {
    let h = crate::minecraft::horodatage();
    let n = |a: usize, b: usize| h.get(a..b).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
    let local = jours_depuis_1970(n(0, 4), n(4, 6), n(6, 8)) * 86400 + n(9, 11) * 3600 + n(11, 13) * 60 + n(13, 15);
    ((local as f64 - maintenant()) / 900.0).round() * 900.0
}

fn date_texte(unix: f64) -> String {
    let t = (unix + decalage_local()) as i64;
    let (a, m, j) = date_civile(t.div_euclid(86400));
    let s = t.rem_euclid(86400);
    format!("{j:02}/{m:02}/{a} {:02}:{:02}", s / 3600, s % 3600 / 60)
}

impl Outil for Planetarium {
    fn nom(&self) -> &'static str {
        "🌌 Planétarium"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let avant = self.lieu.clone();
        ui.horizontal_wrapped(|ui| {
            ui.heading("Planétarium");
            ui.label(format!("📍 {}", self.lieu.nom));
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("ta ville").desired_width(140.0));
            if ui.button("🔎").clicked() && !self.recherche.trim().is_empty() {
                let r: Option<serde_json::Value> = ureq::get("https://geocoding-api.open-meteo.com/v1/search").query("name", self.recherche.trim()).query("language", "fr").query("count", "1").call().ok().and_then(|r| r.into_json().ok());
                match r.as_ref().and_then(|j| j["results"].get(0)) {
                    Some(v) => {
                        self.lieu = Lieu { nom: v["name"].as_str().unwrap_or("?").into(), lat: v["latitude"].as_f64().unwrap_or(0.0), lon: v["longitude"].as_f64().unwrap_or(0.0) };
                        self.message.clear();
                    }
                    None => self.message = "ville introuvable".into(),
                }
            }
            ui.label(&self.message);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Heure");
            ui.add(egui::Slider::new(&mut self.decalage_h, -24.0..=48.0).suffix(" h").step_by(0.25));
            if ui.small_button("Maintenant").clicked() {
                self.decalage_h = 0.0;
                self.defile = 0.0;
            }
            if ui.small_button("Ce soir 22 h").clicked() {
                let local = maintenant() + decalage_local();
                let jour = (local / 86400.0).floor() * 86400.0;
                self.decalage_h = (jour + 22.0 * 3600.0 - local) / 3600.0;
            }
            ui.label("Accéléré");
            ui.add(egui::Slider::new(&mut self.defile, 0.0..=2000.0).logarithmic(true).show_value(false));
            ui.checkbox(&mut self.lignes, "Constellations");
            ui.checkbox(&mut self.noms, "Noms");
        });
        if self.defile > 0.0 {
            self.decalage_h += self.defile * ui.input(|i| i.stable_dt) as f64 / 3600.0;
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(10));
        }
        let unix = maintenant() + self.decalage_h * 3600.0;
        let jd = jour_julien(unix);
        let (lat, lon) = (self.lieu.lat, self.lieu.lon);
        ui.label(RichText::new(date_texte(unix)).strong());
        let cote = ui.available_width().min(ui.available_height() - 40.0).max(300.0);
        let (r, rep) = ui.allocate_exact_size(Vec2::splat(cote), Sense::hover());
        let p = ui.painter_at(r);
        let c = r.center();
        let rayon = cote / 2.0 - 16.0;
        // couleur du ciel selon la hauteur du Soleil
        let (sra, sdec) = geocentrique(usize::MAX, jd);
        let (salt, _) = horizontal(sra, sdec, jd, lat, lon);
        let jour = ((salt + 12.0) / 18.0).clamp(0.0, 1.0) as f32;
        let fond = Color32::from_rgb((6.0 + 60.0 * jour) as u8, (8.0 + 110.0 * jour) as u8, (22.0 + 170.0 * jour) as u8);
        p.circle_filled(c, rayon, fond);
        let vers = |alt: f64, az: f64| -> Option<Pos2> {
            if alt < -0.5 {
                return None;
            }
            let rr = (90.0 - alt) / 90.0 * rayon as f64;
            // nord en haut, est à gauche (on regarde le ciel au-dessus de soi)
            Some(Pos2::new(c.x - (rr * (az * RAD).sin()) as f32, c.y - (rr * (az * RAD).cos()) as f32))
        };
        let mut objets: Vec<(Pos2, String)> = Vec::new();
        if self.lignes {
            for (_, l) in &self.ciel.lignes {
                let pts: Vec<Option<Pos2>> = l.iter().map(|(ra, dec)| {
                    let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
                    vers(a, z)
                }).collect();
                for w in pts.windows(2) {
                    if let (Some(a), Some(b)) = (w[0], w[1]) {
                        p.line_segment([a, b], Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(120, 150, 220, 90)));
                    }
                }
            }
            if self.noms {
                for (_, nom, ra, dec) in &self.ciel.constellations {
                    let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
                    if let Some(q) = vers(a, z) {
                        p.text(q, egui::Align2::CENTER_CENTER, nom, FontId::proportional(11.0), Color32::from_rgba_unmultiplied(140, 170, 230, 150));
                    }
                }
            }
        }
        let visibilite = (1.0 - jour).max(0.05);
        for (ra, dec, mag, bv, nom) in &self.ciel.etoiles {
            let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
            if let Some(q) = vers(a, z) {
                let taille = ((5.8 - mag) * 0.55).clamp(0.6, 4.0);
                let col = couleur_etoile(*bv).gamma_multiply(visibilite);
                p.circle_filled(q, taille, col);
                if !nom.is_empty() {
                    objets.push((q, format!("⭐ {nom} (magnitude {mag:.1})")));
                    if self.noms && *mag < 1.6 {
                        p.text(q + Vec2::new(5.0, -5.0), egui::Align2::LEFT_BOTTOM, nom, FontId::proportional(10.0), Color32::from_white_alpha(140));
                    }
                }
            }
        }
        // planètes, Lune, Soleil
        let mut visibles = Vec::new();
        for k in [0usize, 1, 3, 4, 5, 6, 7] {
            let (ra, dec) = geocentrique(k, jd);
            let (a, z) = horizontal(ra, dec, jd, lat, lon);
            if let Some(q) = vers(a, z) {
                let col = FICHES[k].0;
                p.circle_filled(q, 4.5, Color32::from_rgb(col[0], col[1], col[2]));
                p.text(q + Vec2::new(6.0, 0.0), egui::Align2::LEFT_CENTER, PLANETES[k].0, FontId::proportional(12.0), Color32::from_rgb(col[0], col[1], col[2]));
                objets.push((q, format!("🪐 {} · hauteur {a:.0}°", PLANETES[k].0)));
                if a > 5.0 {
                    visibles.push(format!("{} ({})", PLANETES[k].0, direction(z)));
                }
            }
        }
        let (lra, ldec, phase) = lune(jd);
        let (la, lz) = horizontal(lra, ldec, jd, lat, lon);
        if let Some(q) = vers(la, lz) {
            p.circle_filled(q, 8.0, Color32::from_gray(60));
            p.circle_filled(q + Vec2::new(((1.0 - phase) * 10.0) as f32 - 5.0, 0.0), 8.0 * phase.max(0.1) as f32, Color32::from_rgb(235, 235, 210));
            objets.push((q, format!("🌙 Lune · éclairée à {:.0} %", phase * 100.0)));
        }
        if let Some(q) = vers(salt, horizontal(sra, sdec, jd, lat, lon).1) {
            p.circle_filled(q, 12.0, Color32::from_rgb(255, 220, 90));
            objets.push((q, "☀ Soleil".into()));
        }
        // horizon et points cardinaux
        p.circle_stroke(c, rayon, Stroke::new(2.0_f32, Color32::from_gray(120)));
        for (t, az) in [("N", 0.0), ("E", 90.0), ("S", 180.0), ("O", 270.0)] {
            let q = Pos2::new(c.x - ((rayon + 9.0) as f64 * (az * RAD).sin()) as f32, c.y - ((rayon + 9.0) as f64 * (az * RAD).cos()) as f32);
            p.text(q, egui::Align2::CENTER_CENTER, t, FontId::proportional(14.0), Color32::WHITE);
        }
        if let Some(h) = rep.hover_pos() {
            if let Some((_, t)) = objets.iter().filter(|(q, _)| q.distance(h) < 10.0).min_by(|a, b| a.0.distance(h).total_cmp(&b.0.distance(h))) {
                p.text(h + Vec2::new(12.0, 12.0), egui::Align2::LEFT_TOP, t, FontId::proportional(13.0), Color32::YELLOW);
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("🌙 Lune éclairée à {:.0} % · {}", phase * 100.0, if la > 0.0 { format!("visible ({})", direction(lz)) } else { "sous l'horizon".into() }));
            if !visibles.is_empty() && salt < -6.0 {
                ui.label(format!("· 🔭 à voir : {}", visibles.join(", ")));
            }
        });
        if self.lieu != avant {
            enregistrer("planetarium_lieu", &self.lieu);
        }
    }
}

fn direction(az: f64) -> &'static str {
    ["nord", "nord-est", "est", "sud-est", "sud", "sud-ouest", "ouest", "nord-ouest"][(((az + 22.5) / 45.0) as usize) % 8]
}

// ---------------------------------------------------------------------------------------------------
// 12 · Système solaire en 3D
// ---------------------------------------------------------------------------------------------------

pub struct SystemeSolaire {
    jours: f64,
    vitesse: f64,
    lacet: f32,
    tangage: f32,
    zoom: f32,
    reel: bool,
    choisi: Option<usize>,
    orbites: Vec<Vec<[f64; 3]>>,
}

impl SystemeSolaire {
    pub fn new() -> Self {
        let jd = jour_julien(maintenant());
        SystemeSolaire { jours: 0.0, vitesse: 1.0, lacet: 0.6, tangage: 0.5, zoom: 1.0, reel: false, choisi: None, orbites: (0..9).map(|k| orbite(k, jd, 180)).collect() }
    }

    /// Distance au Soleil affichée (compressée par défaut, sinon les planètes lointaines sont minuscules).
    fn echelle(&self, v: [f64; 3]) -> [f64; 3] {
        if self.reel {
            return v;
        }
        let r = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        let f = r.powf(0.45) / r;
        [v[0] * f, v[1] * f, v[2] * f]
    }
}

impl Outil for SystemeSolaire {
    fn nom(&self) -> &'static str {
        "🪐 Système solaire en 3D"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Système solaire");
            ui.label("Vitesse");
            for (v, n) in [(0.0, "⏸"), (1.0, "1 j/s"), (10.0, "10 j/s"), (100.0, "100 j/s"), (365.0, "1 an/s")] {
                ui.selectable_value(&mut self.vitesse, v, n);
            }
            if ui.small_button("Aujourd'hui").clicked() {
                self.jours = 0.0;
            }
            ui.checkbox(&mut self.reel, "Distances réelles");
            ui.weak("Glisse pour tourner, molette pour zoomer, clique une planète.");
        });
        self.jours += self.vitesse * ui.input(|i| i.stable_dt) as f64;
        let unix = maintenant() + self.jours * 86400.0;
        let jd = jour_julien(unix);
        ui.label(RichText::new(date_texte(unix).split(' ').next().unwrap_or("").to_string()).strong());
        let (r, rep) = ui.allocate_exact_size(Vec2::new(ui.available_width(), (ui.available_height() - 70.0).max(300.0)), Sense::click_and_drag());
        if rep.dragged() {
            let d = rep.drag_delta();
            self.lacet += d.x * 0.01;
            self.tangage = (self.tangage + d.y * 0.01).clamp(-1.5, 1.5);
        }
        if rep.hovered() {
            let s = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (1.0 + s * 0.002)).clamp(0.2, 30.0);
        }
        let p = ui.painter_at(r);
        p.rect_filled(r, 6.0, Color32::from_rgb(4, 4, 12));
        let echelle_max = if self.reel { 31.0 } else { 31f64.powf(0.45) };
        let unite = (r.width().min(r.height()) as f64 / 2.0 / echelle_max * 0.95) * self.zoom as f64;
        let (cl, sl, ct, st) = (self.lacet.cos() as f64, self.lacet.sin() as f64, self.tangage.cos() as f64, self.tangage.sin() as f64);
        let projeter = |v: [f64; 3]| -> (Pos2, f64) {
            // rotation autour de z (lacet) puis de x (tangage)
            let x = v[0] * cl - v[1] * sl;
            let y = v[0] * sl + v[1] * cl;
            let (y2, z2) = (y * ct - v[2] * st, y * st + v[2] * ct);
            (Pos2::new(r.center().x + (x * unite) as f32, r.center().y + (y2 * unite * 1.0) as f32), z2)
        };
        // étoiles de fond fixes
        for k in 0..120u32 {
            let h = k.wrapping_mul(2654435761);
            let q = Pos2::new(r.left() + (h % 1000) as f32 / 1000.0 * r.width(), r.top() + ((h / 1000) % 1000) as f32 / 1000.0 * r.height());
            p.circle_filled(q, 0.8, Color32::from_gray(90 + (h % 100) as u8));
        }
        for (k, o) in self.orbites.iter().enumerate() {
            let pts: Vec<Pos2> = o.iter().map(|v| projeter(self.echelle(*v)).0).collect();
            let c = FICHES[k].0;
            p.add(egui::Shape::line(pts, Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 70))));
        }
        // Soleil
        let (sol, _) = projeter([0.0; 3]);
        for (rr, a) in [(26.0, 30u8), (18.0, 70), (11.0, 255)] {
            p.circle_filled(sol, rr * self.zoom.sqrt(), Color32::from_rgba_unmultiplied(255, 200, 60, a));
        }
        let mut corps: Vec<(f64, usize, Pos2)> = (0..9).map(|k| {
            let (q, z) = projeter(self.echelle(position(k, jd)));
            (z, k, q)
        }).collect();
        corps.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, k, q) in &corps {
            let (c, taille, _) = FICHES[*k];
            let t = taille * self.zoom.sqrt().clamp(0.6, 3.0);
            if *k == 5 {
                p.circle_stroke(*q, t * 1.9, Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(230, 210, 160, 150)));
            }
            p.circle_filled(*q, t, Color32::from_rgb(c[0], c[1], c[2]));
            if self.choisi == Some(*k) {
                p.circle_stroke(*q, t + 5.0, Stroke::new(2.0_f32, Color32::WHITE));
            }
            p.text(*q + Vec2::new(t + 4.0, 0.0), egui::Align2::LEFT_CENTER, PLANETES[*k].0, FontId::proportional(12.0), Color32::from_white_alpha(200));
        }
        if rep.clicked() {
            if let Some(h) = rep.interact_pointer_pos() {
                self.choisi = corps.iter().filter(|(_, _, q)| q.distance(h) < 18.0).min_by(|a, b| a.2.distance(h).total_cmp(&b.2.distance(h))).map(|x| x.1);
            }
        }
        if let Some(k) = self.choisi {
            let v = position(k, jd);
            let d = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let t = position(2, jd);
            let dt = ((v[0] - t[0]).powi(2) + (v[1] - t[1]).powi(2) + (v[2] - t[2]).powi(2)).sqrt();
            ui.label(RichText::new(PLANETES[k].0).size(20.0).strong());
            ui.label(FICHES[k].2);
            ui.weak(format!("À {:.2} UA du Soleil ({:.0} millions de km){}", d, d * 149.6, if k == 2 { String::new() } else { format!(" et à {:.0} millions de km de la Terre aujourd'hui", dt * 149.6) }));
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astronomie() {
        // 1er janvier 2000 à midi : le Soleil est vers 281° d'ascension droite et −23° de déclinaison
        let jd = 2451545.0;
        let (ra, dec) = geocentrique(usize::MAX, jd);
        assert!((ra - 281.3).abs() < 1.0 && (dec + 23.0).abs() < 1.0, "{ra} {dec}");
        // la Terre est à environ 1 UA du Soleil, Jupiter à environ 5
        let t = position(2, jd);
        assert!(((t[0].powi(2) + t[1].powi(2) + t[2].powi(2)).sqrt() - 0.983).abs() < 0.01);
        let j = position(4, jd);
        let dj = (j[0].powi(2) + j[1].powi(2)).sqrt();
        assert!(dj > 4.9 && dj < 5.5);
        // Mars le 1er janvier 2000 : vers 22 h d'ascension droite (environ 330°), déclinaison −13°
        let (ra, dec) = geocentrique(3, jd);
        assert!((ra - 330.0).abs() < 4.0 && (dec + 13.0).abs() < 3.0, "{ra} {dec}");
        // pleine Lune du 21 janvier 2000 (éclipse) : presque entièrement éclairée
        let (_, _, phase) = lune(2451565.2);
        assert!(phase > 0.95, "{phase}");
        // le Soleil est haut à midi en été à Paris, sous l'horizon à minuit
        let midi = jour_julien(1_719_316_800.0); // 25 juin 2024, 12 h UTC
        let (sra, sdec) = geocentrique(usize::MAX, midi);
        let (alt, az) = horizontal(sra, sdec, midi, 48.857, 2.352);
        assert!(alt > 60.0 && (az - 180.0).abs() < 25.0, "{alt} {az}");
        let (alt, _) = horizontal(sra, sdec, midi + 0.5, 48.857, 2.352);
        assert!(alt < -10.0);
        // l'étoile polaire reste à une hauteur proche de la latitude
        let c = ciel();
        let polaire = c.etoiles.iter().find(|e| e.4 == "Polaris").unwrap();
        let (alt, _) = horizontal(polaire.0, polaire.1, midi, 48.857, 2.352);
        assert!((alt - 48.857).abs() < 1.5, "{alt}");
        assert!(c.constellations.len() >= 88 && c.lignes.len() > 88 && c.etoiles.len() > 2000);
        assert!(c.constellations.iter().any(|x| x.1 == "Grande Ourse"));
        assert_eq!(orbite(3, jd, 36).len(), 37);
        assert_eq!(date_civile(jours_depuis_1970(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(jours_depuis_1970(1970, 1, 1), 0);
        assert_eq!(date_civile(19_723), (2024, 1, 1));
    }
}
