//! Problèmes en jeu : repère les chutes de FPS, la surchauffe et la mémoire pleine pendant une partie,
//! et dit la cause probable (processeur, carte graphique, chaleur, mémoire, autre programme).
use crate::moniteur::{Instantane, Lecteur};
use crate::outil::{charger, enregistrer, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Genre {
    ChuteFps,
    Surchauffe,
    Memoire,
}

impl Genre {
    fn texte(self) -> &'static str {
        match self {
            Genre::ChuteFps => "📉 Chute de FPS",
            Genre::Surchauffe => "🌡 Surchauffe",
            Genre::Memoire => "🧠 Mémoire pleine",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Evenement {
    pub heure: String,
    pub jeu: String,
    pub genre: Genre,
    pub detail: String,
    pub cause: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Reglages {
    pub actif: bool,
    /// prévenir tout de suite (sinon : un bilan à la fin de la partie)
    pub direct: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages { actif: true, direct: false }
    }
}

fn mediane(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// Cause la plus probable d'un ralentissement, d'après les mesures du moment.
pub fn cause(m: &Instantane) -> String {
    if m.cpu_t >= 90.0 || m.gpu_t >= 87.0 {
        return format!("Surchauffe ({:.0} °C processeur, {:.0} °C carte graphique) : le PC ralentit pour refroidir. Dépoussière les ventilateurs, surélève le portable, ou baisse un peu les graphismes.", m.cpu_t, m.gpu_t);
    }
    if m.ram >= 90.0 {
        return format!("Mémoire presque pleine ({:.0} %) : Windows doit utiliser le disque. Ferme le navigateur et les programmes inutiles pendant le jeu.", m.ram);
    }
    if m.gpu >= 96.0 {
        return format!("Carte graphique au maximum ({:.0} %) : la scène est trop lourde. Baisse la résolution, les ombres ou active le DLSS / FSR.", m.gpu);
    }
    if m.cpu >= 90.0 {
        return format!("Processeur saturé ({:.0} %) : le jeu ou un programme en arrière-plan prend tout. Regarde « Programmes gourmands ».", m.cpu);
    }
    "Ni le processeur ni la carte graphique ne sont au maximum : sans doute un chargement (disque), le jeu lui-même, ou un programme qui se réveille (mise à jour, antivirus).".into()
}

/// Analyse la fenêtre glissante (≈ 1 mesure par seconde). Renvoie un problème s'il dure.
pub fn diagnostic(f: &VecDeque<Instantane>) -> Option<(Genre, String)> {
    if f.len() < 20 {
        return None;
    }
    let n = f.len();
    let recent: Vec<&Instantane> = f.iter().skip(n - 4).collect();
    // surchauffe tenue 4 s
    if recent.iter().all(|m| m.cpu_t >= 92.0 || m.gpu_t >= 88.0) {
        let m = recent[3];
        return Some((Genre::Surchauffe, format!("{:.0} °C processeur · {:.0} °C carte graphique", m.cpu_t, m.gpu_t)));
    }
    if recent.iter().all(|m| m.ram >= 93.0) {
        return Some((Genre::Memoire, format!("{:.0} % de la mémoire utilisée", recent[3].ram)));
    }
    // chute de FPS : 4 s de suite sous 60 % de la médiane de la minute
    let mut avant: Vec<f32> = f.iter().take(n - 4).map(|m| m.fps).filter(|x| *x > 0.0).collect();
    let med = mediane(&mut avant);
    if med >= 30.0 && recent.iter().all(|m| m.fps > 0.0 && m.fps < med * 0.6) {
        let bas = recent.iter().map(|m| m.fps).fold(f32::MAX, f32::min);
        return Some((Genre::ChuteFps, format!("{bas:.0} FPS au lieu de {med:.0} habituellement")));
    }
    None
}

pub struct ProblemesJeu {
    lire: Lecteur,
    r: Reglages,
    fenetre: VecDeque<Instantane>,
    dernier: Instant,
    jeu: Option<String>,
    partie: Vec<Evenement>,
    journal: Vec<Evenement>,
    calme: Vec<(Genre, Instant)>,
}

impl ProblemesJeu {
    pub fn new(lire: Lecteur) -> Self {
        ProblemesJeu { lire, r: charger("problemes_jeu_reglages"), fenetre: VecDeque::new(), dernier: Instant::now(), jeu: None, partie: vec![], journal: charger("problemes_jeu"), calme: vec![] }
    }

    fn fin_de_partie(&mut self) {
        let Some(jeu) = self.jeu.take() else { return };
        self.fenetre.clear();
        if self.partie.is_empty() {
            return;
        }
        let mut genres: Vec<&str> = self.partie.iter().map(|e| e.genre.texte()).collect();
        genres.dedup();
        let cause = self.partie.last().map(|e| e.cause.clone()).unwrap_or_default();
        crate::systeme::notifier(&format!("Bilan de ta partie de {jeu}"), &format!("{} souci(s) : {}. Cause probable : {}", self.partie.len(), genres.join(", "), cause.split(':').next().unwrap_or("")));
        self.partie.clear();
    }
}

impl Outil for ProblemesJeu {
    fn nom(&self) -> &'static str {
        "🩺 Problèmes en jeu"
    }
    fn cat(&self) -> Cat {
        Cat::Jeux
    }
    fn tic(&mut self, _ctx: &egui::Context) {
        if !self.r.actif || self.dernier.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.dernier = Instant::now();
        let jeu_lance = crate::outil::JEU_ACTIF.lock().ok().and_then(|j| j.clone());
        let (m, jeu_mesure) = (self.lire)(jeu_lance.is_some());
        let jeu = jeu_lance.or(jeu_mesure);
        if jeu.is_none() {
            self.fin_de_partie();
            return;
        }
        if self.jeu != jeu {
            self.fin_de_partie();
            self.jeu.clone_from(&jeu);
        }
        self.fenetre.push_back(m);
        if self.fenetre.len() > 60 {
            self.fenetre.pop_front();
        }
        let Some((genre, detail)) = diagnostic(&self.fenetre) else { return };
        // un même souci au plus toutes les 3 minutes
        self.calme.retain(|c| c.1.elapsed() < Duration::from_secs(180));
        if self.calme.iter().any(|c| c.0 == genre) {
            return;
        }
        self.calme.push((genre, Instant::now()));
        let e = Evenement { heure: crate::minecraft::horodatage(), jeu: jeu.clone().unwrap_or_default(), genre, detail, cause: cause(&m) };
        if self.r.direct {
            crate::systeme::notifier(&format!("{} dans {}", genre.texte(), e.jeu), &e.cause);
        }
        self.partie.push(e.clone());
        self.journal.insert(0, e);
        self.journal.truncate(200);
        enregistrer("problemes_jeu", &self.journal);
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Problèmes en jeu");
        ui.weak("Pendant que tu joues, Outils surveille les FPS, la température et la mémoire. Quand ça rame, il note la cause probable — tu sais quoi changer.");
        let mut change = ui.checkbox(&mut self.r.actif, "Surveiller pendant les parties").changed();
        change |= ui.checkbox(&mut self.r.direct, "Prévenir tout de suite (sinon : un bilan à la fin de la partie)").changed();
        if change {
            enregistrer("problemes_jeu_reglages", &self.r);
        }
        ui.weak("Les FPS viennent du moniteur (Jeux › Moniteur Jeu) : il doit pouvoir mesurer le jeu (mode administrateur conseillé).");
        match &self.jeu {
            Some(j) => {
                let m = self.fenetre.back().copied();
                ui.label(RichText::new(format!("🎮 {j} en cours")).strong());
                if let Some(m) = m {
                    ui.label(format!("{:.0} FPS · processeur {:.0} % ({:.0} °C) · carte graphique {:.0} % ({:.0} °C) · mémoire {:.0} %", m.fps, m.cpu, m.cpu_t, m.gpu, m.gpu_t, m.ram));
                }
            }
            None => {
                ui.label("Aucun jeu en cours.");
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong(format!("Journal ({})", self.journal.len()));
            if !self.journal.is_empty() && ui.small_button("Vider").clicked() {
                self.journal.clear();
                enregistrer("problemes_jeu", &self.journal);
            }
        });
        if self.journal.is_empty() {
            ui.weak("Rien à signaler pour l'instant. Joue une partie : les soucis apparaîtront ici.");
            return;
        }
        // causes les plus fréquentes
        let mut compte: Vec<(&str, usize)> = vec![];
        for e in &self.journal {
            let c = e.cause.split(['(', ':']).next().unwrap_or("").trim();
            match compte.iter_mut().find(|x| x.0 == c) {
                Some(x) => x.1 += 1,
                None => compte.push((c, 1)),
            }
        }
        compte.sort_by(|a, b| b.1.cmp(&a.1));
        if let Some((c, n)) = compte.first() {
            ui.label(RichText::new(format!("Cause la plus fréquente : {c} ({n} fois)")).color(Color32::from_rgb(255, 200, 100)));
        }
        let mut ouvrir: Option<&str> = None;
        egui::ScrollArea::vertical().id_salt("problemes").show(ui, |ui| {
            for e in self.journal.iter().take(80) {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(e.genre.texte());
                        ui.weak(format!("{} · {}", e.heure.replace('_', " "), e.jeu));
                    });
                    ui.label(&e.detail);
                    ui.label(RichText::new(&e.cause).italics());
                    let outil = if e.cause.starts_with("Surchauffe") {
                        Some("🌡 Alerte de surchauffe")
                    } else if e.cause.starts_with("Mémoire") {
                        Some("🧠 Mémoire en veille")
                    } else if e.cause.starts_with("Processeur") || e.cause.starts_with("Ni le") {
                        Some("🐷 Programmes gourmands")
                    } else {
                        Some("🎚 Profils de performance par jeu")
                    };
                    if let Some(o) = outil {
                        if ui.small_button(format!("Ouvrir {o}")).clicked() {
                            ouvrir = Some(o);
                        }
                    }
                });
            }
        });
        if let Some(o) = ouvrir {
            *crate::interface2::OUVRIR.lock().unwrap() = Some(o.to_string());
        }
    }
    fn resume(&self) -> Option<String> {
        let d = self.journal.first()?;
        Some(format!("Dernier souci : {} dans {}", d.genre.texte(), d.jeu))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(fps: f32, cpu: f32, gpu: f32, t: f32, ram: f32) -> Instantane {
        Instantane { fps, cpu, cpu_t: t, gpu, gpu_t: t - 5.0, ram }
    }

    #[test]
    fn detection() {
        let mut f: VecDeque<Instantane> = (0..30).map(|_| m(120.0, 50.0, 70.0, 70.0, 50.0)).collect();
        assert!(diagnostic(&f).is_none());
        for _ in 0..4 {
            f.push_back(m(50.0, 98.0, 60.0, 70.0, 50.0));
        }
        let (g, d) = diagnostic(&f).unwrap();
        assert_eq!(g, Genre::ChuteFps);
        assert!(d.contains("50 FPS"), "{d}");
        assert!(cause(f.back().unwrap()).starts_with("Processeur"));
        assert!(cause(&m(40.0, 50.0, 99.0, 70.0, 50.0)).starts_with("Carte graphique"));
        assert!(cause(&m(40.0, 50.0, 50.0, 95.0, 50.0)).starts_with("Surchauffe"));
        assert!(cause(&m(40.0, 50.0, 50.0, 60.0, 95.0)).starts_with("Mémoire"));
        assert!(cause(&m(40.0, 40.0, 40.0, 60.0, 50.0)).starts_with("Ni le"));
        let mut chaud: VecDeque<Instantane> = (0..30).map(|_| m(100.0, 50.0, 70.0, 95.0, 50.0)).collect();
        assert_eq!(diagnostic(&chaud).unwrap().0, Genre::Surchauffe);
        chaud.clear();
        assert!(diagnostic(&chaud).is_none());
    }
}
