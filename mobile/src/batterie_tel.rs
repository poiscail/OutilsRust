//! 🔋 Batterie (8) : état en direct, autonomie estimée, applis les plus utilisées, alerte de charge à 80 %
//! pour ménager la batterie, alerte de surchauffe.
use crate::android;
use crate::commun::*;
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, RichText};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Reglages {
    alerte: bool,
    seuil: i32,
    chaleur: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { alerte: true, seuil: 80, chaleur: true }
    }
}

/// Temps restant (en minutes) : jusqu'à la charge complète, ou jusqu'à la batterie vide.
pub fn autonomie(niveau: i32, charge_mah: i32, courant_ma: i32, en_charge: bool) -> Option<u32> {
    if courant_ma == 0 || charge_mah <= 0 || niveau <= 0 {
        return None;
    }
    let capacite = charge_mah as f32 * 100.0 / niveau as f32;
    let reste = if en_charge { capacite - charge_mah as f32 } else { charge_mah as f32 };
    let i = courant_ma.unsigned_abs() as f32;
    (i > 10.0).then(|| (reste / i * 60.0) as u32)
}

fn veiller(r: Arc<Mutex<Reglages>>) {
    static LANCE: AtomicBool = AtomicBool::new(false);
    if LANCE.swap(true, Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let (mut prevenu, mut chaud) = (false, false);
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let Some(b) = android::batterie() else { continue };
            let x = r.lock().unwrap().clone();
            if b.en_charge && x.alerte && b.niveau >= x.seuil && !prevenu {
                android::notifier(4600, "🔋 Batterie chargée", &format!("{} % : débranche pour ménager la batterie", b.niveau));
                prevenu = true;
            }
            if !b.en_charge {
                prevenu = false;
            }
            if x.chaleur && b.temperature >= 42.0 && !chaud {
                android::notifier(4601, "🔥 Téléphone chaud", &format!("Batterie à {:.0} °C : enlève la coque, arrête les jeux un moment", b.temperature));
                chaud = true;
            }
            if b.temperature < 38.0 {
                chaud = false;
            }
        }
    });
}

pub struct BatterieOutil {
    r: Arc<Mutex<Reglages>>,
    b: Option<android::Batterie>,
    vu: Instant,
    courbe: Vec<f32>,
    applis: Arc<Mutex<Option<Vec<(String, u64)>>>>,
    e: Etat,
}

impl BatterieOutil {
    pub fn new() -> Self {
        let r = Arc::new(Mutex::new(charger::<Reglages>("batterie")));
        veiller(r.clone());
        BatterieOutil { r, b: None, vu: Instant::now() - Duration::from_secs(9), courbe: vec![], applis: Arc::default(), e: Arc::default() }
    }
}

impl Outil for BatterieOutil {
    fn nom(&self) -> &'static str {
        "🔋 Batterie"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.vu.elapsed() > Duration::from_secs(2) {
            self.vu = Instant::now();
            self.b = android::batterie();
            if let Some(b) = &self.b {
                self.courbe.push(b.courant_ma as f32);
                if self.courbe.len() > 150 {
                    self.courbe.remove(0);
                }
            }
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
        let Some(b) = self.b.clone() else {
            ui.weak("Informations de batterie indisponibles.");
            return;
        };
        let c = if b.niveau <= 15 { ROUGE } else if b.niveau <= 35 { ORANGE } else { VERT };
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(format!("{} %", b.niveau)).size(64.0).strong().color(c));
            ui.label(RichText::new(if b.pleine { "pleine".to_string() } else if b.en_charge { format!("en charge ({})", b.branchement) } else { "sur batterie".into() }).size(18.0));
            if let Some(m) = autonomie(b.niveau, b.charge_mah, b.courant_ma, b.en_charge) {
                ui.label(RichText::new(format!("{} encore {} h {:02}", if b.en_charge { "pleine dans" } else { "tient" }, m / 60, m % 60)).strong());
            }
        });
        ui.add(egui::ProgressBar::new(b.niveau as f32 / 100.0).fill(c));
        egui::Grid::new("batt").striped(true).show(ui, |ui| {
            for (a, v) in [
                ("Santé", b.sante.to_string()),
                ("Température", format!("{:.1} °C{}", b.temperature, if b.temperature >= 40.0 { " 🔥" } else { "" })),
                ("Courant", format!("{} mA {}", b.courant_ma.abs(), if b.courant_ma > 0 { "(entre)" } else { "(sort)" })),
                ("Tension", format!("{:.2} V", b.tension)),
                ("Charge restante", if b.charge_mah > 0 { format!("{} mAh", b.charge_mah) } else { "?".into() }),
                ("Technologie", b.technologie.clone()),
            ] {
                ui.label(a);
                ui.strong(v);
                ui.end_row();
            }
        });
        ui.label("Courant (mA) — plus la courbe descend, plus le téléphone consomme :");
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 8.0, ui.visuals().extreme_bg_color);
        let max = self.courbe.iter().map(|x| x.abs()).fold(100.0f32, f32::max);
        let pts: Vec<egui::Pos2> = self.courbe.iter().enumerate().map(|(k, x)| egui::pos2(r.left() + k as f32 / 149.0 * r.width(), r.center().y - x / max * r.height() / 2.0)).collect();
        ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(2.0_f32, ROSE)));
        ui.separator();
        let mut x = self.r.lock().unwrap().clone();
        let mut change = ui.checkbox(&mut x.alerte, "Me prévenir quand la charge atteint").changed();
        if x.alerte {
            change |= ui.add(egui::Slider::new(&mut x.seuil, 60..=100).suffix(" %")).changed();
        }
        change |= ui.checkbox(&mut x.chaleur, "Me prévenir si la batterie chauffe (42 °C)").changed();
        if change {
            enregistrer("batterie", &x);
            *self.r.lock().unwrap() = x;
        }
        ui.weak("Charger jusqu'à 80 % seulement (et éviter de descendre sous 20 %) fait durer une batterie bien plus d'années.");
        ui.separator();
        ui.label(RichText::new("Applis les plus utilisées aujourd'hui").strong());
        if !android::acces_statistiques() {
            if ui.button("📊 Donner l'accès « Utilisation des applis »").clicked() {
                android::ouvrir_reglage("android.settings.USAGE_ACCESS_SETTINGS", false);
            }
        } else if ui.button("Calculer").clicked() {
            let a = self.applis.clone();
            en_fond(&self.e, "…", move || {
                let m = maintenant();
                *a.lock().unwrap() = Some(android::temps_par_appli(m - m % 86_400, m));
                Ok(String::new())
            });
        }
        etat_ui(ui, &self.e);
        if let Some(v) = self.applis.lock().unwrap().clone() {
            let max = v.first().map(|x| x.1).unwrap_or(1).max(1);
            for (n, m) in v.iter().take(12) {
                ui.horizontal_wrapped(|ui| {
                    ui.add_sized([130.0, 22.0], egui::Label::new(n).truncate());
                    ui.add(egui::ProgressBar::new(*m as f32 / max as f32).desired_width(ui.available_width() - 70.0).fill(ROSE));
                    ui.label(format!("{} h {:02}", m / 60, m % 60));
                });
            }
            ui.weak("L'écran allumé est ce qui use le plus la batterie : ces applis sont celles qui l'ont gardé allumé.");
        }
        ui.collapsing("💡 Faire durer la batterie", |ui| {
            for t in ["Luminosité automatique et mode sombre (sur écran OLED, le noir ne consomme rien).", "Couper la 5G si elle capte mal (« Réseau mobile » › Économie).", "Les jeux et le GPS sont les plus gourmands : ferme-les vraiment après usage.", "Évite la chaleur : pas de charge sous l'oreiller ni en plein soleil.", "Mode économie d'énergie d'Android sous 20 %."] {
                ui.label(format!("• {t}"));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temps_restant() {
        // 50 %, 2000 mAh restants, 1000 mA consommés : 2 h
        assert_eq!(autonomie(50, 2000, -1000, false), Some(120));
        // en charge : 2000 mAh manquants à 2000 mA → 1 h
        assert_eq!(autonomie(50, 2000, 2000, true), Some(60));
        assert_eq!(autonomie(50, 0, 500, false), None);
    }
}
