//! Minuteur Pomodoro : travail / pause en alternance, bip + notification à la fin de chaque phase.
use crate::systeme;
use eframe::egui;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Minuteur {
    travail: u32, // minutes
    pause: u32,
    en_travail: bool,
    fin: Option<Instant>, // en marche
    restant: Duration,    // à l'arrêt / en pause
    cycles: u32,
    generation: Arc<AtomicU64>, // invalide le fil de fin quand on pause ou remet à zéro
}

impl Minuteur {
    pub fn new() -> Self {
        Minuteur { travail: 25, pause: 5, en_travail: true, fin: None, restant: Duration::from_secs(25 * 60), cycles: 0, generation: Arc::default() }
    }

    fn duree_phase(&self) -> Duration {
        Duration::from_secs(60 * if self.en_travail { self.travail } else { self.pause } as u64)
    }

    fn restant(&self) -> Duration {
        self.fin.map_or(self.restant, |f| f.saturating_duration_since(Instant::now()))
    }

    fn demarrer(&mut self) {
        let g = self.generation.fetch_add(1, Relaxed) + 1;
        let gen = self.generation.clone();
        let d = self.restant;
        let texte = if self.en_travail { "Travail terminé, place à la pause." } else { "Pause terminée, au travail !" };
        self.fin = Some(Instant::now() + d);
        // fil à part : la fenêtre cachée ne repeint pas, mais le bip doit sonner quand même
        std::thread::spawn(move || {
            std::thread::sleep(d);
            if gen.load(Relaxed) == g {
                use windows_sys::Win32::{System::Diagnostics::Debug::MessageBeep, UI::WindowsAndMessaging::MB_ICONASTERISK};
                unsafe { MessageBeep(MB_ICONASTERISK) };
                systeme::notifier("Pomodoro", texte);
            }
        });
    }

    fn arreter(&mut self) {
        self.generation.fetch_add(1, Relaxed);
        self.restant = self.restant();
        self.fin = None;
    }

    fn remettre(&mut self) {
        self.arreter();
        self.en_travail = true;
        self.cycles = 0;
        self.restant = self.duree_phase();
    }

    /// Passe à la phase suivante quand le temps est écoulé (la suivante attend qu'on la lance).
    pub fn tic(&mut self) {
        if self.fin.is_some_and(|f| Instant::now() >= f) {
            self.fin = None;
            if self.en_travail {
                self.cycles += 1;
            }
            self.en_travail = !self.en_travail;
            self.restant = self.duree_phase();
        }
    }

    pub fn en_marche(&self) -> bool {
        self.fin.is_some()
    }

    pub fn resume(&self) -> String {
        let r = self.restant().as_secs();
        let phase = if self.en_travail { "Travail" } else { "Pause" };
        let etat = if self.en_marche() { "" } else { " (arrêté)" };
        format!("{phase} {:02}:{:02}{etat} · {} cycle(s)", r / 60, r % 60, self.cycles)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Minuteur Pomodoro");
        ui.add_space(12.0);
        let r = self.restant().as_secs();
        ui.label(egui::RichText::new(if self.en_travail { "Travail" } else { "Pause" }).size(22.0));
        ui.label(egui::RichText::new(format!("{:02}:{:02}", r / 60, r % 60)).size(72.0).monospace().strong());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if self.en_marche() {
                if ui.button("⏸ Pause").clicked() {
                    self.arreter();
                }
            } else if ui.button("▶ Démarrer").clicked() {
                self.demarrer();
            }
            if ui.button("⟲ Remettre à zéro").clicked() {
                self.remettre();
            }
            if ui.button("⏭ Passer").clicked() {
                self.arreter();
                self.fin = Some(Instant::now());
                self.tic();
            }
        });
        ui.add_space(8.0);
        ui.label(format!("Cycles terminés : {}", self.cycles));
        ui.separator();
        let avant = (self.travail, self.pause);
        ui.horizontal(|ui| {
            ui.label("Travail (min)");
            ui.add(egui::DragValue::new(&mut self.travail).range(1..=180));
            ui.label("Pause (min)");
            ui.add(egui::DragValue::new(&mut self.pause).range(1..=60));
        });
        if avant != (self.travail, self.pause) && !self.en_marche() {
            self.restant = self.duree_phase();
        }
        if self.en_marche() {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases() {
        let mut m = Minuteur::new();
        m.fin = Some(Instant::now() - Duration::from_secs(1));
        m.tic();
        assert!(!m.en_travail && m.cycles == 1 && m.restant == Duration::from_secs(300) && !m.en_marche());
        m.remettre();
        assert!(m.en_travail && m.cycles == 0 && m.restant == Duration::from_secs(1500));
    }
}
