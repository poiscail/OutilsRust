//! Outils lui-même, suite : dock flottant (3), statistiques d'utilisation (4), visite guidée (5),
//! centre de notifications (7). (1 · thèmes : theme.rs ; 6 · mode mini : main.rs.)
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn heure() -> String {
    let h = crate::minecraft::horodatage(); // AAAA-MM-JJ_HH-MM-SS
    format!("{}/{} {}", h.get(8..10).unwrap_or(""), h.get(5..7).unwrap_or(""), h.get(11..16).unwrap_or("").replace('-', ":"))
}

// ---------------------------------------------------------------------------------------------------
// 7 · Centre de notifications
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone)]
pub struct Notif {
    pub quand: String,
    pub titre: String,
    pub texte: String,
    pub lue: bool,
}

static NOTIFS: Mutex<Option<Vec<Notif>>> = Mutex::new(None);

fn avec_notifs<R>(f: impl FnOnce(&mut Vec<Notif>) -> R) -> R {
    let mut g = NOTIFS.lock().unwrap_or_else(|e| e.into_inner());
    let v = g.get_or_insert_with(|| charger("notifications"));
    f(v)
}

/// Appelé par chaque notification d'Outils.
pub fn pousser(titre: &str, texte: &str) {
    avec_notifs(|v| {
        v.push(Notif { quand: heure(), titre: titre.into(), texte: texte.into(), lue: false });
        let n = v.len();
        if n > 200 {
            v.drain(..n - 200);
        }
        enregistrer("notifications", v);
    });
}

pub fn non_lues() -> usize {
    avec_notifs(|v| v.iter().filter(|n| !n.lue).count())
}

/// Fenêtre du centre de notifications (ouverte par la cloche du menu).
pub fn fenetre_notifs(ctx: &egui::Context, ouvert: &mut bool) {
    let mut garder = *ouvert;
    egui::Window::new("🔔 Notifications").open(&mut garder).default_width(420.0).default_height(420.0).collapsible(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("✔ Tout marquer comme lu").clicked() {
                avec_notifs(|v| v.iter_mut().for_each(|n| n.lue = true));
            }
            if ui.button("🗑 Tout effacer").clicked() {
                avec_notifs(|v| v.clear());
            }
        });
        ui.separator();
        let liste = avec_notifs(|v| v.clone());
        if liste.is_empty() {
            ui.weak("Aucune notification.");
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for n in liste.iter().rev() {
                ui.horizontal(|ui| {
                    ui.label(if n.lue { "  " } else { "🔴" });
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(&n.titre);
                            ui.weak(&n.quand);
                        });
                        ui.label(&n.texte);
                    });
                });
                ui.add_space(4.0);
            }
        });
    });
    if !garder || !*ouvert {
        // à la fermeture, tout est considéré comme vu
        avec_notifs(|v| {
            v.iter_mut().for_each(|n| n.lue = true);
            enregistrer("notifications", v);
        });
    }
    *ouvert = garder;
}

// ---------------------------------------------------------------------------------------------------
// 4 · Statistiques d'utilisation
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Usage {
    pub secondes: u64,
    pub ouvertures: u32,
    pub dernier: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Utilisation {
    outils: BTreeMap<String, Usage>,
    jours: BTreeMap<String, u64>, // secondes d'Outils ouvert au premier plan, par jour
    depuis: String,
}

struct Compteur {
    u: Utilisation,
    onglet: String,
    tic: Instant,
    sauve: Instant,
    fraction: f64, // temps pas encore compté (moins d'une seconde)
}

static USAGE: Mutex<Option<Compteur>> = Mutex::new(None);

/// À chaque image : compte le temps passé sur l'onglet affiché (fenêtre au premier plan seulement).
pub fn compter(onglet: &str, actif: bool) {
    let mut g = USAGE.lock().unwrap_or_else(|e| e.into_inner());
    let c = g.get_or_insert_with(|| {
        let mut u: Utilisation = charger("utilisation");
        if u.depuis.is_empty() {
            u.depuis = heure();
        }
        Compteur { u, onglet: String::new(), tic: Instant::now(), sauve: Instant::now(), fraction: 0.0 }
    });
    let dt = c.tic.elapsed().as_secs_f64().min(2.0);
    c.tic = Instant::now();
    if c.onglet != onglet {
        c.onglet = onglet.to_string();
        let e = c.u.outils.entry(onglet.to_string()).or_default();
        e.ouvertures += 1;
        e.dernier = heure();
    }
    if actif {
        c.fraction += dt;
        if c.fraction >= 1.0 {
            let s = c.fraction.floor();
            c.fraction -= s;
            c.u.outils.entry(onglet.to_string()).or_default().secondes += s as u64;
            let jour = crate::minecraft::horodatage().get(..10).unwrap_or("").to_string();
            *c.u.jours.entry(jour).or_default() += s as u64;
        }
    }
    if c.sauve.elapsed() > Duration::from_secs(60) {
        c.sauve = Instant::now();
        enregistrer("utilisation", &c.u);
    }
}

pub fn duree(s: u64) -> String {
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h {:02}", s / 3600, s % 3600 / 60),
    }
}

pub struct StatsUsage {
    tri_temps: bool,
}

impl StatsUsage {
    pub fn new() -> Self {
        StatsUsage { tri_temps: true }
    }
}

impl Outil for StatsUsage {
    fn nom(&self) -> &'static str {
        "📊 Mon utilisation d'Outils"
    }
    fn cat(&self) -> Cat {
        Cat::Systeme
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mon utilisation d'Outils");
        let (outils, jours, depuis) = {
            let g = USAGE.lock().unwrap_or_else(|e| e.into_inner());
            match g.as_ref() {
                Some(c) => (c.u.outils.clone(), c.u.jours.clone(), c.u.depuis.clone()),
                None => return,
            }
        };
        let total: u64 = outils.values().map(|u| u.secondes).sum();
        ui.label(format!("Depuis le {depuis} : {} au total, {} outils ouverts.", duree(total), outils.len()));
        ui.horizontal(|ui| {
            ui.label("Trier par");
            ui.selectable_value(&mut self.tri_temps, true, "temps");
            ui.selectable_value(&mut self.tri_temps, false, "ouvertures");
            if ui.small_button("🗑 Remettre à zéro").clicked() {
                *USAGE.lock().unwrap_or_else(|e| e.into_inner()) = None;
                enregistrer("utilisation", &Utilisation::default());
            }
        });
        let mut v: Vec<(String, Usage)> = outils.into_iter().collect();
        if self.tri_temps {
            v.sort_by(|a, b| b.1.secondes.cmp(&a.1.secondes));
        } else {
            v.sort_by(|a, b| b.1.ouvertures.cmp(&a.1.ouvertures));
        }
        let max = v.first().map(|x| if self.tri_temps { x.1.secondes } else { x.1.ouvertures as u64 }).unwrap_or(1).max(1);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("usage").striped(true).spacing([14.0, 4.0]).show(ui, |ui| {
                for (n, u) in &v {
                    ui.label(n);
                    let val = if self.tri_temps { u.secondes } else { u.ouvertures as u64 };
                    ui.add(egui::ProgressBar::new(val as f32 / max as f32).desired_width(160.0));
                    ui.label(duree(u.secondes));
                    ui.weak(format!("{} fois · {}", u.ouvertures, u.dernier));
                    ui.end_row();
                }
            });
        });
        ui.separator();
        ui.strong("14 derniers jours");
        let derniers: Vec<(String, u64)> = jours.into_iter().rev().take(14).collect::<Vec<_>>().into_iter().rev().collect();
        let m = derniers.iter().map(|x| x.1).max().unwrap_or(1).max(1);
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0 * 32.0, 100.0), egui::Sense::hover());
        for (k, (j, s)) in derniers.iter().enumerate() {
            let hh = *s as f32 / m as f32 * 80.0;
            let b = egui::Rect::from_min_max(egui::pos2(r.left() + k as f32 * 32.0 + 4.0, r.bottom() - 16.0 - hh), egui::pos2(r.left() + k as f32 * 32.0 + 28.0, r.bottom() - 16.0));
            ui.painter().rect_filled(b, 3.0, Color32::from_rgb(255, 120, 170));
            ui.painter().text(egui::pos2(b.center().x, r.bottom()), egui::Align2::CENTER_BOTTOM, j.get(8..10).unwrap_or(""), egui::FontId::proportional(11.0), Color32::GRAY);
            if ui.rect_contains_pointer(b) {
                egui::show_tooltip_text(ui.ctx(), ui.layer_id(), egui::Id::new(("jour", k)), format!("{j} : {}", duree(*s)));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 5 · Visite guidée
// ---------------------------------------------------------------------------------------------------

pub const ETAPES: [(&str, &str); 7] = [
    ("🌸 Bienvenue dans Outils", "Plus de 200 outils dans un seul programme : jeux, Minecraft, fichiers, système, réseau, quotidien, créatif et fun. Cette petite visite dure 30 secondes."),
    ("📂 Le menu", "À gauche, les outils sont rangés par catégorie. Tape dans « Chercher un outil » pour en trouver un tout de suite."),
    ("⭐ Les favoris", "Clic droit sur un outil du menu → « Ajouter aux favoris » : il reste tout en haut. Les favoris apparaissent aussi dans le dock et le mode mini."),
    ("⌨ La palette", "Ctrl+K dans Outils, ou Ctrl+Alt+K de n'importe où : tape quelques lettres pour ouvrir un outil ou lancer une action (capture, veille, serveur…)."),
    ("🏠 L'accueil", "L'Accueil résume tout : disques, météo, serveur Minecraft, téléchargements, rappels… Clique sur une ligne pour ouvrir l'outil."),
    ("🎨 Personnaliser", "Couleurs, thèmes, polices, fonds d'écran japonais, effets (pétales, neige…), démarrage avec Windows et icône près de l'horloge."),
    ("🔔 Les notifications", "La cloche en haut du menu garde toutes les alertes d'Outils. Le bouton 🗗 passe en mode mini, toujours visible. Bonne découverte !"),
];

/// Fenêtre de la visite ; renvoie false quand elle est finie.
pub fn visite(ctx: &egui::Context, etape: &mut usize) -> bool {
    let mut continuer = true;
    let (t, d) = ETAPES[(*etape).min(ETAPES.len() - 1)];
    egui::Window::new("visite").title_bar(false).resizable(false).fixed_size([440.0, 0.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(RichText::new(t).size(22.0).strong());
        ui.add_space(6.0);
        ui.label(RichText::new(d).size(15.0));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            for k in 0..ETAPES.len() {
                ui.label(if k == *etape { "●" } else { "○" });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dernier = *etape + 1 >= ETAPES.len();
                if ui.button(if dernier { "C'est parti !" } else { "Suivant ➡" }).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if dernier {
                        continuer = false;
                    } else {
                        *etape += 1;
                    }
                }
                if *etape > 0 && ui.button("⬅").clicked() {
                    *etape -= 1;
                }
                if !dernier && ui.small_button("Passer").clicked() {
                    continuer = false;
                }
            });
        });
    });
    if !continuer {
        enregistrer("visite_faite", &Some(true));
    }
    continuer
}

pub fn visite_a_faire() -> bool {
    charger::<Option<bool>>("visite_faite").is_none()
}

// ---------------------------------------------------------------------------------------------------
// 3 · Dock flottant
// ---------------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct Dock {
    pub actif: Arc<AtomicBool>,
    pub favoris: Arc<Mutex<Vec<String>>>,
    pub choix: Arc<Mutex<Option<String>>>,
}

/// Icône d'un outil : son premier caractère (les noms commencent par un émoji).
pub fn icone(nom: &str) -> String {
    nom.split_whitespace().next().filter(|p| !p.chars().any(|c| c.is_alphanumeric())).map(String::from).unwrap_or_else(|| nom.chars().take(2).collect())
}

impl Dock {
    pub fn new() -> Self {
        let d = Dock::default();
        d.actif.store(charger::<Option<bool>>("dock").unwrap_or(false), Relaxed);
        d
    }

    pub fn basculer(&self, oui: bool) {
        self.actif.store(oui, Relaxed);
        enregistrer("dock", &Some(oui));
    }

    /// Fenêtre à part, toujours devant (elle se redessine seule, même si Outils est derrière).
    pub fn afficher(&self, ctx: &egui::Context) {
        if !self.actif.load(Relaxed) {
            return;
        }
        let n = self.favoris.lock().unwrap().len().max(1);
        let (fav, choix, actif) = (self.favoris.clone(), self.choix.clone(), self.actif.clone());
        let vp = egui::ViewportBuilder::default().with_title("Dock Outils").with_inner_size([64.0, 12.0 + 46.0 * n as f32 + 30.0]).with_always_on_top().with_resizable(false).with_minimize_button(false).with_maximize_button(false);
        ctx.show_viewport_deferred(egui::ViewportId::from_hash_of("dock"), vp, move |ctx, _| {
            if ctx.input(|i| i.viewport().close_requested()) {
                actif.store(false, Relaxed);
                enregistrer("dock", &Some(false));
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let l = fav.lock().unwrap().clone();
                    if l.is_empty() {
                        ui.small("Ajoute des favoris (clic droit dans le menu)");
                    }
                    for nom in l {
                        if ui.add(egui::Button::new(RichText::new(icone(&nom)).size(22.0)).min_size(egui::vec2(42.0, 40.0))).on_hover_text(&nom).clicked() {
                            *choix.lock().unwrap() = Some(nom);
                            crate::systeme::montrer();
                        }
                    }
                    if ui.small_button("🏠").on_hover_text("Ouvrir Outils").clicked() {
                        *choix.lock().unwrap() = Some(String::new());
                        crate::systeme::montrer();
                    }
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divers() {
        assert_eq!(icone("🎮 Batterie de la manette"), "🎮");
        assert_eq!(icone("Outils"), "Ou");
        assert_eq!(duree(42), "42 s");
        assert_eq!(duree(3_725), "1 h 02");
    }
}
