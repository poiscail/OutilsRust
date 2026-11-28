//! Planétarium en réalité augmentée : on pointe le téléphone vers le ciel et on voit les étoiles,
//! planètes et constellations qui sont vraiment là, par-dessus l'image de la caméra.
//! L'orientation vient de l'accéléromètre (la verticale) et du magnétomètre (le nord).
use crate::android::{Camera, Capteurs};
use crate::espace::{ciel, couleur_etoile, geocentrique, horizontal, jour_julien, lune, maintenant, Ciel, FICHES, PLANETES};
use crate::outil::{charger, Cat, Outil};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

type V3 = [f32; 3];

fn croix(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn scal(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norme(a: V3) -> Option<V3> {
    let n = scal(a, a).sqrt();
    (n > 1e-3).then(|| [a[0] / n, a[1] / n, a[2] / n])
}

/// Repère de la caméra dans le monde (x = est, y = nord, z = haut) :
/// (direction visée, droite de l'écran, haut de l'écran).
pub fn repere(acc: V3, mag: V3) -> Option<(V3, V3, V3)> {
    let a = norme(acc)?;
    let e = norme(croix(mag, a))?; // est
    let n = croix(a, e); // nord
    // vecteur du téléphone (x droite, y haut, z vers soi) → monde
    let monde = |v: V3| [e[0] * v[0] + e[1] * v[1] + e[2] * v[2], n[0] * v[0] + n[1] * v[1] + n[2] * v[2], a[0] * v[0] + a[1] * v[1] + a[2] * v[2]];
    Some((monde([0.0, 0.0, -1.0]), monde([1.0, 0.0, 0.0]), monde([0.0, 1.0, 0.0])))
}

/// Repère à partir d'un cap et d'une hauteur (sans capteurs : on regarde au doigt).
pub fn repere_manuel(az: f32, alt: f32) -> (V3, V3, V3) {
    let (az, alt) = (az.to_radians(), alt.to_radians());
    let d = [alt.cos() * az.sin(), alt.cos() * az.cos(), alt.sin()];
    let r = [az.cos(), -az.sin(), 0.0];
    (d, r, croix(r, d))
}

/// Direction (hauteur, azimut en degrés) → vecteur du monde.
pub fn vecteur(alt: f64, az: f64) -> V3 {
    let (a, z) = (alt.to_radians() as f32, az.to_radians() as f32);
    [a.cos() * z.sin(), a.cos() * z.cos(), a.sin()]
}

/// Position à l'écran (centre, focale en pixels) ou None si derrière.
pub fn projeter(s: V3, (d, r, u): (V3, V3, V3), centre: Pos2, f: f32) -> Option<Pos2> {
    let z = scal(s, d);
    if z < 0.08 {
        return None;
    }
    Some(Pos2::new(centre.x + scal(s, r) / z * f, centre.y - scal(s, u) / z * f))
}

pub struct CielAr {
    ciel: Option<Ciel>,
    capteurs: Option<Capteurs>,
    cam: Option<Camera>,
    avec_camera: bool,
    acc: V3,
    mag: V3,
    manuel: (f32, f32),
    champ: f32,
    tex: Option<egui::TextureHandle>,
    choisi: Option<String>,
}

impl CielAr {
    pub fn new() -> Self {
        CielAr { ciel: None, capteurs: None, cam: None, avec_camera: true, acc: [0.0, 0.0, 9.8], mag: [0.0, 30.0, -30.0], manuel: (180.0, 30.0), champ: 62.0, tex: None, choisi: None }
    }
}

impl Outil for CielAr {
    fn nom(&self) -> &'static str {
        "🔭 Ciel en réalité augmentée"
    }
    fn cat(&self) -> Cat {
        Cat::Fun
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let ciel = self.ciel.get_or_insert_with(ciel);
        let lieu: serde_json::Value = charger("planetarium_lieu");
        let (lat, lon) = (lieu["lat"].as_f64().unwrap_or(48.857), lieu["lon"].as_f64().unwrap_or(2.352));
        let capteurs = self.capteurs.get_or_insert_with(|| Capteurs::demarrer(true, true, false));
        let m = capteurs.m.lock().unwrap().clone();
        // lissage : les capteurs tremblent
        if m.recu {
            for k in 0..3 {
                self.acc[k] += (m.acc[k] - self.acc[k]) * 0.15;
                self.mag[k] += (m.mag[k] - self.mag[k]) * 0.15;
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("📍 {}", lieu["nom"].as_str().unwrap_or("Paris")));
            if ui.toggle_value(&mut self.avec_camera, "📷 Caméra").changed() && !self.avec_camera {
                self.cam = None;
                self.tex = None;
            }
            ui.label("Zoom");
            ui.add(egui::Slider::new(&mut self.champ, 30.0..=90.0).show_value(false).suffix("°"));
        });
        if !m.recu {
            ui.weak("Pas de boussole sur cet appareil : glisse le doigt pour regarder autour de toi.");
        }
        // caméra en fond
        if self.avec_camera && self.cam.is_none() {
            if crate::android::a_permission("android.permission.CAMERA") {
                self.cam = Some(Camera::demarrer_avec(true));
            } else if ui.button("📷 Autoriser la caméra").clicked() {
                crate::android::demander(&["android.permission.CAMERA"]);
            }
        }
        if let Some(c) = &self.cam {
            if let Some((l, h, rgb)) = c.couleur.lock().unwrap().take() {
                let t = crate::pc_tel::tourner(l, h, &rgb, 3);
                let img = egui::ColorImage::from_rgb([h, l], &t);
                match &mut self.tex {
                    Some(x) => x.set(img, Default::default()),
                    None => self.tex = Some(ui.ctx().load_texture("ciel_cam", img, Default::default())),
                }
            }
        }
        let taille = Vec2::new(ui.available_width(), (ui.available_height() - 50.0).max(320.0));
        let (zone, rep) = ui.allocate_exact_size(taille, Sense::click_and_drag());
        let p = ui.painter_at(zone);
        let repere = if m.recu { repere(self.acc, self.mag) } else { None };
        let repere = repere.unwrap_or_else(|| {
            let d = rep.drag_delta();
            self.manuel.0 = (self.manuel.0 - d.x * 0.25).rem_euclid(360.0);
            self.manuel.1 = (self.manuel.1 + d.y * 0.25).clamp(-30.0, 89.0);
            repere_manuel(self.manuel.0, self.manuel.1)
        });
        let f = (zone.height() / 2.0) / (self.champ.to_radians() / 2.0).tan();
        let centre = zone.center();
        let unix = maintenant();
        let jd = jour_julien(unix);
        let (sra, sdec) = geocentrique(usize::MAX, jd);
        let (salt, saz) = horizontal(sra, sdec, jd, lat, lon);
        let jour = ((salt + 12.0) / 18.0).clamp(0.0, 1.0) as f32;
        match &self.tex {
            Some(t) => {
                // l'image remplit la zone (rognée sur les côtés)
                let s = t.size_vec2();
                let e = (zone.width() / s.x).max(zone.height() / s.y);
                egui::Image::new(t).paint_at(ui, Rect::from_center_size(centre, s * e));
                p.rect_filled(zone, 0.0, Color32::from_black_alpha(70));
            }
            None => {
                let fond = Color32::from_rgb((6.0 + 60.0 * jour) as u8, (8.0 + 110.0 * jour) as u8, (22.0 + 170.0 * jour) as u8);
                p.rect_filled(zone, 0.0, fond);
            }
        }
        let vers = |alt: f64, az: f64| projeter(vecteur(alt, az), repere, centre, f).filter(|q| zone.expand(40.0).contains(*q));
        // sol (sans caméra) et horizon
        let horizon: Vec<Pos2> = (0..=360).step_by(3).filter_map(|az| vers(0.0, az as f64)).collect();
        for w in horizon.windows(2) {
            if w[0].distance(w[1]) < zone.width() {
                p.line_segment([w[0], w[1]], Stroke::new(2.0_f32, Color32::from_rgb(120, 200, 120)));
            }
        }
        for (t, az) in [("N", 0.0), ("NE", 45.0), ("E", 90.0), ("SE", 135.0), ("S", 180.0), ("SO", 225.0), ("O", 270.0), ("NO", 315.0)] {
            if let Some(q) = vers(0.0, az) {
                p.text(q + Vec2::new(0.0, 14.0), Align2::CENTER_CENTER, t, FontId::proportional(18.0), Color32::from_rgb(150, 230, 150));
            }
        }
        let mut objets: Vec<(Pos2, String)> = vec![];
        for (_, l) in &ciel.lignes {
            let pts: Vec<Option<Pos2>> = l.iter().map(|(ra, dec)| {
                let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
                vers(a, z)
            }).collect();
            for w in pts.windows(2) {
                if let (Some(a), Some(b)) = (w[0], w[1]) {
                    p.line_segment([a, b], Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(140, 170, 240, 120)));
                }
            }
        }
        for (_, nom, ra, dec) in &ciel.constellations {
            let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
            if let Some(q) = vers(a, z) {
                p.text(q, Align2::CENTER_CENTER, nom, FontId::proportional(13.0), Color32::from_rgba_unmultiplied(160, 190, 250, 170));
            }
        }
        let visibilite = (1.0 - jour).max(0.35);
        for (ra, dec, mag, bv, nom) in &ciel.etoiles {
            let (a, z) = horizontal(*ra, *dec, jd, lat, lon);
            if let Some(q) = vers(a, z) {
                let t = ((5.8 - mag) * 0.8).clamp(0.8, 6.0);
                p.circle_filled(q, t, couleur_etoile(*bv).gamma_multiply(visibilite));
                if !nom.is_empty() {
                    if *mag < 1.6 {
                        p.text(q + Vec2::new(7.0, -7.0), Align2::LEFT_BOTTOM, nom, FontId::proportional(12.0), Color32::from_white_alpha(200));
                    }
                    objets.push((q, format!("⭐ {nom} · étoile, magnitude {mag:.1} · hauteur {a:.0}°")));
                }
            }
        }
        for k in [0usize, 1, 3, 4, 5, 6, 7] {
            let (ra, dec) = geocentrique(k, jd);
            let (a, z) = horizontal(ra, dec, jd, lat, lon);
            if let Some(q) = vers(a, z) {
                let c = FICHES[k].0;
                let col = Color32::from_rgb(c[0], c[1], c[2]);
                p.circle_filled(q, 7.0, col);
                p.text(q + Vec2::new(10.0, 0.0), Align2::LEFT_CENTER, PLANETES[k].0, FontId::proportional(15.0), col);
                objets.push((q, format!("🪐 {} · {}", PLANETES[k].0, FICHES[k].2)));
            }
        }
        let (lra, ldec, phase) = lune(jd);
        let (la, lz) = horizontal(lra, ldec, jd, lat, lon);
        if let Some(q) = vers(la, lz) {
            p.circle_filled(q, 16.0, Color32::from_rgb(235, 235, 210));
            p.text(q + Vec2::new(20.0, 0.0), Align2::LEFT_CENTER, "Lune", FontId::proportional(15.0), Color32::from_rgb(235, 235, 210));
            objets.push((q, format!("🌙 Lune · éclairée à {:.0} %", phase * 100.0)));
        }
        if let Some(q) = vers(salt, saz) {
            p.circle_filled(q, 22.0, Color32::from_rgb(255, 220, 90));
            objets.push((q, "☀ Soleil · ne le regarde jamais en face !".into()));
        }
        // viseur et direction visée
        p.circle_stroke(centre, 18.0, Stroke::new(1.5_f32, Color32::from_white_alpha(160)));
        let (d, _, _) = repere;
        let (alt, az) = (d[2].clamp(-1.0, 1.0).asin().to_degrees(), d[0].atan2(d[1]).to_degrees().rem_euclid(360.0));
        p.text(zone.left_bottom() + Vec2::new(10.0, -10.0), Align2::LEFT_BOTTOM, format!("cap {az:.0}° · hauteur {alt:.0}°"), FontId::proportional(14.0), Color32::WHITE);
        // toucher un objet : sa fiche
        if rep.clicked() {
            if let Some(h) = rep.interact_pointer_pos() {
                self.choisi = objets.iter().filter(|(q, _)| q.distance(h) < 36.0).min_by(|a, b| a.0.distance(h).total_cmp(&b.0.distance(h))).map(|x| x.1.clone());
            }
        }
        // sinon, ce qui est au centre du viseur
        let au_centre = objets.iter().filter(|(q, _)| q.distance(centre) < 24.0).min_by(|a, b| a.0.distance(centre).total_cmp(&b.0.distance(centre))).map(|x| x.1.clone());
        if let Some(t) = self.choisi.clone().or(au_centre) {
            ui.label(RichText::new(t).strong());
        } else {
            ui.weak("Vise une étoile avec le rond au centre, ou touche-la.");
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation() {
        // téléphone debout, écran vers soi, dos tourné vers le nord : gravité sur +y, champ vers le nord et le bas
        let (d, r, u) = repere([0.0, 9.8, 0.0], [0.0, -20.0, -40.0]).unwrap();
        assert!(d[1] > 0.99, "visée {d:?}"); // vers le nord
        assert!(r[0] > 0.99, "droite {r:?}"); // à droite : l'est
        assert!(u[2] > 0.99, "haut {u:?}"); // le haut de l'écran : le ciel
        // une étoile plein nord, sur l'horizon, est au centre ; une étoile à l'est est à droite
        let c = Pos2::new(200.0, 400.0);
        let q = projeter(vecteur(0.0, 0.0), (d, r, u), c, 500.0).unwrap();
        assert!(q.distance(c) < 1.0);
        let q = projeter(vecteur(0.0, 20.0), (d, r, u), c, 500.0).unwrap();
        assert!(q.x > c.x + 100.0);
        let q = projeter(vecteur(20.0, 0.0), (d, r, u), c, 500.0).unwrap();
        assert!(q.y < c.y - 100.0);
        assert!(projeter(vecteur(0.0, 180.0), (d, r, u), c, 500.0).is_none()); // derrière
        // le mode manuel donne le même repère
        let (d2, r2, u2) = repere_manuel(0.0, 0.0);
        assert!(scal(d, d2) > 0.99 && scal(r, r2) > 0.99 && scal(u, u2) > 0.99);
    }
}
