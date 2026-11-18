//! Chronomètre de speedrun (69) : segments, record personnel, meilleurs segments, écart en direct.
//! Raccourcis globaux du pavé numérique : 1 = départ / segment suivant, 2 = annuler le dernier segment,
//! 3 = remise à zéro, 8 = pause (à activer dans l'onglet ; les touches gardent leur effet normal). Petite fenêtre toujours devant pour suivre en jeu.
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Course {
    pub nom: String,
    pub segments: Vec<String>,
    pub record: Vec<Option<u64>>,    // temps cumulés du record (ms)
    pub meilleurs: Vec<Option<u64>>, // meilleur temps de chaque segment (ms)
    pub essais: u32,
}

#[derive(Default)]
pub struct Partie {
    pub debut: Option<Instant>,
    pub pause_depuis: Option<Instant>,
    pub pauses: Duration,
    pub temps: Vec<u64>, // cumulés de cet essai
}

impl Partie {
    pub fn ecoule(&self) -> u64 {
        let Some(d) = self.debut else { return 0 };
        let fin = self.pause_depuis.unwrap_or_else(Instant::now);
        (fin.duration_since(d).saturating_sub(self.pauses)).as_millis() as u64
    }
}

pub fn format_temps(ms: u64) -> String {
    let (h, m, s, c) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms / 10 % 100);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{c:02}") } else if m > 0 { format!("{m}:{s:02}.{c:02}") } else { format!("{s}.{c:02}") }
}

pub fn format_ecart(ms: i64) -> String {
    let signe = if ms < 0 { "−" } else { "+" };
    format!("{signe}{}", format_temps(ms.unsigned_abs()))
}

/// Termine un essai : met à jour record et meilleurs segments. Renvoie vrai si c'est un nouveau record.
pub fn terminer(c: &mut Course, temps: &[u64]) -> bool {
    let n = c.segments.len();
    c.record.resize(n, None);
    c.meilleurs.resize(n, None);
    for (i, t) in temps.iter().enumerate().take(n) {
        let seg = t - if i == 0 { 0 } else { temps[i - 1] };
        if c.meilleurs[i].is_none_or(|b| seg < b) {
            c.meilleurs[i] = Some(seg);
        }
    }
    let complet = temps.len() == n && n > 0;
    let record = complet && c.record.last().copied().flatten().is_none_or(|r| temps[n - 1] < r);
    if record {
        c.record = temps.iter().map(|t| Some(*t)).collect();
    }
    record
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Donnees {
    courses: Vec<Course>,
    choisie: usize,
}

pub struct Speedrun {
    d: Donnees,
    p: Arc<Mutex<Partie>>,
    appuis: Arc<Mutex<Vec<u32>>>,
    fenetre: bool,
    actif: Arc<std::sync::atomic::AtomicBool>,
    nouveau_segment: String,
    message: String,
}

/// Surveille le pavé numérique (sans le bloquer : on lit l'état des touches, on ne les capture pas).
fn raccourcis(appuis: Arc<Mutex<Vec<u32>>>, actif: Arc<std::sync::atomic::AtomicBool>) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_NUMPAD1, VK_NUMPAD2, VK_NUMPAD3, VK_NUMPAD8};
    std::thread::spawn(move || {
        let touches = [(1, VK_NUMPAD1), (2, VK_NUMPAD2), (3, VK_NUMPAD3), (8, VK_NUMPAD8)];
        let mut avant = [false; 4];
        loop {
            std::thread::sleep(Duration::from_millis(15));
            if !actif.load(std::sync::atomic::Ordering::Relaxed) {
                continue;
            }
            for (k, (id, vk)) in touches.iter().enumerate() {
                let bas = unsafe { GetAsyncKeyState(*vk as i32) } < 0;
                if bas && !avant[k] {
                    appuis.lock().unwrap().push(*id);
                    if let Some(c) = CTX.get() {
                        c.request_repaint();
                    }
                }
                avant[k] = bas;
            }
        }
    });
}

static CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

impl Speedrun {
    pub fn new() -> Self {
        let mut d: Donnees = charger("speedrun");
        if d.courses.is_empty() {
            d.courses.push(Course { nom: "Mon jeu · Any%".into(), segments: vec!["Niveau 1".into(), "Niveau 2".into(), "Boss".into()], ..Default::default() });
        }
        Speedrun { d, p: Arc::default(), appuis: Arc::default(), fenetre: false, actif: Arc::default(), nouveau_segment: String::new(), message: String::new() }
    }

    fn course(&mut self) -> &mut Course {
        self.d.choisie = self.d.choisie.min(self.d.courses.len() - 1);
        &mut self.d.courses[self.d.choisie]
    }

    fn action(&mut self, a: u32) {
        let mut p = self.p.lock().unwrap();
        let n = self.d.courses[self.d.choisie.min(self.d.courses.len() - 1)].segments.len();
        match a {
            1 => {
                if p.debut.is_none() {
                    *p = Partie { debut: Some(Instant::now()), ..Default::default() };
                    drop(p);
                    self.course().essais += 1;
                    enregistrer("speedrun", &self.d);
                } else if p.pause_depuis.is_none() && p.temps.len() < n {
                    let t = p.ecoule();
                    p.temps.push(t);
                    if p.temps.len() == n {
                        p.pause_depuis = Some(Instant::now()); // fin : le chrono s'arrête
                        let temps = p.temps.clone();
                        drop(p);
                        let record = terminer(self.course(), &temps);
                        self.message = if record { "🏆 Nouveau record !".into() } else { "Course terminée.".into() };
                        enregistrer("speedrun", &self.d);
                    }
                }
            }
            2 => {
                p.temps.pop();
            }
            3 => {
                let temps = p.temps.clone();
                *p = Partie::default();
                drop(p);
                if !temps.is_empty() {
                    terminer(self.course(), &temps); // garde les meilleurs segments d'un essai abandonné
                    enregistrer("speedrun", &self.d);
                }
                self.message.clear();
            }
            8 => {
                if p.debut.is_some() {
                    match p.pause_depuis.take() {
                        Some(t) => p.pauses += t.elapsed(),
                        None => p.pause_depuis = Some(Instant::now()),
                    }
                }
            }
            _ => {}
        }
    }

    fn tableau(ui: &mut egui::Ui, c: &Course, p: &Partie, grand: bool) {
        let actuel = p.temps.len();
        egui::Grid::new(("splits", grand)).num_columns(3).spacing([18.0, 4.0]).show(ui, |ui| {
            for (i, s) in c.segments.iter().enumerate() {
                let txt = if i == actuel && p.debut.is_some() { RichText::new(s).strong().color(Color32::from_rgb(255, 150, 200)) } else { RichText::new(s) };
                ui.label(txt);
                let record = c.record.get(i).copied().flatten();
                match (p.temps.get(i), record) {
                    (Some(t), Some(r)) => {
                        let e = *t as i64 - r as i64;
                        let seg = t - if i == 0 { 0 } else { p.temps[i - 1] };
                        let or = c.meilleurs.get(i).copied().flatten().is_none_or(|b| seg < b);
                        let couleur = if or { Color32::GOLD } else if e < 0 { Color32::LIGHT_GREEN } else { Color32::from_rgb(255, 110, 110) };
                        ui.colored_label(couleur, format_ecart(e));
                        ui.label(format_temps(*t));
                    }
                    (Some(t), None) => {
                        ui.label("");
                        ui.label(format_temps(*t));
                    }
                    (None, Some(r)) => {
                        ui.label("");
                        ui.weak(format_temps(r));
                    }
                    _ => {
                        ui.label("");
                        ui.weak("—");
                    }
                }
                ui.end_row();
            }
        });
        let couleur = if p.pause_depuis.is_some() { Color32::GRAY } else { Color32::from_rgb(120, 230, 150) };
        ui.label(RichText::new(format_temps(p.ecoule())).size(if grand { 54.0 } else { 34.0 }).monospace().strong().color(couleur));
    }
}

impl Outil for Speedrun {
    fn nom(&self) -> &'static str {
        "⏱ Chrono speedrun"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }

    fn tic(&mut self, ctx: &egui::Context) {
        if CTX.get().is_none() {
            let _ = CTX.set(ctx.clone());
            raccourcis(self.appuis.clone(), self.actif.clone());
        }
        let appuis: Vec<u32> = std::mem::take(&mut *self.appuis.lock().unwrap());
        for a in appuis {
            self.action(a);
        }
        if self.fenetre {
            let c = self.d.courses[self.d.choisie.min(self.d.courses.len() - 1)].clone();
            let p = self.p.clone();
            let mut ferme = false;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("speedrun"),
                egui::ViewportBuilder::default().with_title("Speedrun").with_inner_size([300.0, 120.0 + 22.0 * c.segments.len() as f32]).with_always_on_top(),
                |ctx, _| {
                    egui::CentralPanel::default().show(ctx, |ui| Self::tableau(ui, &c, &p.lock().unwrap(), false));
                    ferme = ctx.input(|i| i.viewport().close_requested());
                    ctx.request_repaint_after(Duration::from_millis(50));
                },
            );
            if ferme {
                self.fenetre = false;
            }
        }
        if self.p.lock().unwrap().debut.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chrono speedrun");
        ui.weak("Pavé numérique : 1 départ / segment suivant · 2 annuler un segment · 3 remise à zéro · 8 pause : coche « Touches du pavé numérique » (marche même en jeu, sans bloquer les touches).");
        ui.horizontal(|ui| {
            let noms: Vec<String> = self.d.courses.iter().map(|c| c.nom.clone()).collect();
            egui::ComboBox::from_id_salt("course").selected_text(noms[self.d.choisie.min(noms.len() - 1)].clone()).show_ui(ui, |ui| {
                for (i, n) in noms.iter().enumerate() {
                    ui.selectable_value(&mut self.d.choisie, i, n);
                }
            });
            if ui.button("➕ Course").clicked() {
                self.d.courses.push(Course { nom: "Nouvelle course".into(), segments: vec!["Segment 1".into()], ..Default::default() });
                self.d.choisie = self.d.courses.len() - 1;
            }
            ui.checkbox(&mut self.fenetre, "Petite fenêtre toujours devant");
            let mut a = self.actif.load(std::sync::atomic::Ordering::Relaxed);
            if ui.checkbox(&mut a, "Touches du pavé numérique").changed() {
                self.actif.store(a, std::sync::atomic::Ordering::Relaxed);
            }
        });
        ui.horizontal(|ui| {
            for (a, n) in [(1, "▶ Départ / segment"), (2, "↶ Annuler"), (8, "⏸ Pause"), (3, "⟲ Remise à zéro")] {
                if ui.button(n).clicked() {
                    self.action(a);
                }
            }
        });
        if !self.message.is_empty() {
            ui.label(RichText::new(&self.message).size(18.0));
        }
        let c = self.course().clone();
        let total_meilleurs: Option<u64> = c.meilleurs.iter().copied().sum();
        ui.columns(2, |col| {
            Self::tableau(&mut col[0], &c, &self.p.lock().unwrap(), true);
            let ui = &mut col[1];
            let en_cours = self.p.lock().unwrap().debut.is_some();
            let mut c2 = c.clone();
            ui.add_enabled_ui(!en_cours, |ui| {
                ui.add(egui::TextEdit::singleline(&mut c2.nom).desired_width(f32::INFINITY));
                let mut retirer = None;
                for (i, s) in c2.segments.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(s).desired_width(200.0));
                        if ui.small_button("✖").clicked() {
                            retirer = Some(i);
                        }
                    });
                }
                if let Some(i) = retirer {
                    c2.segments.remove(i);
                    for v in [&mut c2.record, &mut c2.meilleurs] {
                        if i < v.len() {
                            v.remove(i);
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.nouveau_segment).hint_text("nouveau segment").desired_width(160.0));
                    if ui.button("➕").clicked() && !self.nouveau_segment.trim().is_empty() {
                        c2.segments.push(std::mem::take(&mut self.nouveau_segment));
                    }
                });
            });
            ui.separator();
            ui.label(format!("Essais : {}", c.essais));
            if let Some(Some(r)) = c.record.last() {
                ui.label(format!("Record : {}", format_temps(*r)));
            }
            if let Some(t) = total_meilleurs.filter(|_| c.meilleurs.len() == c.segments.len()) {
                ui.label(format!("Somme des meilleurs segments : {}", format_temps(t)));
            }
            if ui.button("Effacer le record").clicked() {
                c2.record.clear();
                c2.meilleurs.clear();
            }
            if c2 != c {
                *self.course() = c2;
                enregistrer("speedrun", &self.d);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_temps(65_430), "1:05.43");
        assert_eq!(format_temps(9_050), "9.05");
        assert_eq!(format_temps(3_725_000), "1:02:05.00");
        assert_eq!(format_ecart(-1500), "−1.50");
    }

    #[test]
    fn records() {
        let mut c = Course { segments: vec!["a".into(), "b".into()], ..Default::default() };
        assert!(terminer(&mut c, &[1000, 3000]));
        assert_eq!(c.meilleurs, vec![Some(1000), Some(2000)]);
        assert!(!terminer(&mut c, &[900, 3500])); // plus lent au total, mais meilleur 1er segment
        assert_eq!(c.meilleurs[0], Some(900));
        assert_eq!(c.record, vec![Some(1000), Some(3000)]);
        assert!(!terminer(&mut c, &[800])); // abandon : pas de record
        assert_eq!(c.meilleurs[0], Some(800));
    }
}
