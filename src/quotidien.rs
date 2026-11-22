//! Réveil (123), temps d'écran (134), mode concentration (135).
use crate::outil::{charger, enregistrer, Cat, Outil};
use crate::planif::{lire_heure, moment, JOURS};
use crate::pressepapiers::{civil, decalage, maintenant_utc};
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

// ---------------------------------------------------------------------------------------------------
// 123 · Réveil
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Alarme {
    pub libelle: String,
    pub heure: String,
    pub jours: [bool; 7],
    pub actif: bool,
    pub son: Option<PathBuf>, // .wav ; sinon son de Windows
    pub reveiller_pc: bool,
    pub dernier_jour: i64,
}

impl Default for Alarme {
    fn default() -> Self {
        Alarme { libelle: "Réveil".into(), heure: "07:30".into(), jours: [true, true, true, true, true, false, false], actif: true, son: None, reveiller_pc: true, dernier_jour: -1 }
    }
}

/// Prochain déclenchement (UTC) d'une alarme après `maintenant`.
pub fn prochaine(a: &Alarme, maintenant: i64, off: i64) -> Option<i64> {
    let h = lire_heure(&a.heure)?;
    if !a.actif || !a.jours.iter().any(|&j| j) {
        return None;
    }
    let (jour, _, _) = moment(maintenant, off);
    (0..8).map(|k| jour + k).find_map(|j| {
        let t = j * 86400 + h * 60 - off;
        let js = (j + 3).rem_euclid(7) as usize;
        (a.jours[js] && t > maintenant).then_some(t)
    })
}

fn jouer(son: Option<&PathBuf>) {
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_FILENAME, SND_LOOP, SND_NODEFAULT};
    unsafe {
        match son.filter(|p| p.is_file()) {
            Some(p) => PlaySoundW(w(&p.to_string_lossy()).as_ptr(), std::ptr::null_mut(), SND_FILENAME | SND_ASYNC | SND_LOOP | SND_NODEFAULT),
            None => PlaySoundW(w("SystemExclamation").as_ptr(), std::ptr::null_mut(), SND_ALIAS | SND_ASYNC | SND_LOOP),
        };
    }
}

fn couper() {
    unsafe { windows_sys::Win32::Media::Audio::PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
}

#[derive(Default)]
struct EtatReveil {
    alarmes: Vec<Alarme>,
    sonne: Option<String>,
    rappel: Option<(i64, Alarme)>, // répétition (snooze)
}

pub struct Reveil {
    e: Arc<Mutex<EtatReveil>>,
}

impl Reveil {
    pub fn new() -> Self {
        let e = Arc::new(Mutex::new(EtatReveil { alarmes: charger("reveil"), ..Default::default() }));
        // fil de surveillance : sonne même fenêtre cachée
        let e2 = e.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            let m = maintenant_utc();
            let (jour, js, minute) = moment(m, decalage());
            let mut x = e2.lock().unwrap();
            let mut a_sonner = None;
            for a in x.alarmes.iter_mut() {
                if let Some(h) = lire_heure(&a.heure) {
                    if a.actif && a.jours[js] && a.dernier_jour != jour && minute >= h && minute - h < 3 {
                        a.dernier_jour = jour;
                        a_sonner = Some(a.clone());
                    }
                }
            }
            if let Some((t, a)) = x.rappel.clone() {
                if m >= t {
                    x.rappel = None;
                    a_sonner = Some(a);
                }
            }
            if let Some(a) = a_sonner {
                enregistrer("reveil", &x.alarmes);
                if x.sonne.is_none() {
                    jouer(a.son.as_ref());
                    x.sonne = Some(a.libelle.clone());
                    crate::systeme::notifier_toujours("⏰ Réveil", &a.libelle);
                    crate::systeme::montrer();
                }
            }
        });
        // fil du minuteur matériel : sort le PC de veille pour la prochaine alarme
        let e3 = e.clone();
        std::thread::spawn(move || unsafe {
            use windows_sys::Win32::System::Threading::{CreateWaitableTimerW, SetWaitableTimer, WaitForSingleObject};
            let minuteur = CreateWaitableTimerW(std::ptr::null(), 1, std::ptr::null());
            loop {
                let m = maintenant_utc();
                let off = decalage();
                let prochaine_reveil = e3.lock().unwrap().alarmes.iter().filter(|a| a.reveiller_pc).filter_map(|a| prochaine(a, m, off)).min();
                if let Some(t) = prochaine_reveil {
                    // FILETIME : centaines de ns depuis 1601 ; on réveille 60 s avant pour laisser le PC démarrer
                    let ft: i64 = (t - 60 + 11_644_473_600) * 10_000_000;
                    SetWaitableTimer(minuteur, &ft, 0, None, std::ptr::null(), 1);
                }
                WaitForSingleObject(minuteur, 60_000);
            }
        });
        Reveil { e }
    }
}

impl Outil for Reveil {
    fn nom(&self) -> &'static str {
        "⏰ Réveil"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Réveil");
        let mut x = self.e.lock().unwrap();
        if let Some(l) = x.sonne.clone() {
            egui::Frame::new().fill(Color32::from_rgb(120, 40, 80)).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                ui.label(RichText::new(format!("⏰ {l}")).size(30.0).strong());
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("⏹ Arrêter").size(20.0)).clicked() {
                        couper();
                        x.sonne = None;
                    }
                    if ui.button(RichText::new("😴 Encore 5 min").size(20.0)).clicked() {
                        couper();
                        x.sonne = None;
                        let a = x.alarmes.iter().find(|a| a.libelle == l).cloned().unwrap_or_default();
                        x.rappel = Some((maintenant_utc() + 300, a));
                    }
                });
            });
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        let avant = x.alarmes.clone();
        if ui.button("➕ Alarme").clicked() {
            x.alarmes.push(Alarme::default());
        }
        let off = decalage();
        let mut retirer = None;
        for (i, a) in x.alarmes.iter_mut().enumerate() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut a.actif, "");
                    ui.add(egui::TextEdit::singleline(&mut a.heure).desired_width(50.0).font(egui::TextStyle::Heading));
                    ui.add(egui::TextEdit::singleline(&mut a.libelle).desired_width(160.0));
                    for (k, n) in JOURS.iter().enumerate() {
                        ui.toggle_value(&mut a.jours[k], *n);
                    }
                    if ui.small_button("🗑").clicked() {
                        retirer = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut a.reveiller_pc, "Sortir le PC de veille");
                    let son = a.son.as_ref().map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()).unwrap_or_else(|| "son de Windows".into());
                    if ui.button(format!("🔔 {son}")).on_hover_text("Fichier .wav").clicked() {
                        a.son = rfd::FileDialog::new().add_filter("Son WAV", &["wav"]).pick_file();
                    }
                    if ui.small_button("▶").on_hover_text("Écouter").clicked() {
                        jouer(a.son.as_ref());
                        std::thread::spawn(|| {
                            std::thread::sleep(Duration::from_secs(3));
                            couper();
                        });
                    }
                    if let Some(t) = prochaine(a, maintenant_utc(), off) {
                        ui.weak(format!("prochaine : {}", crate::notes::format_local(t, off)));
                    }
                });
            });
        }
        if let Some(i) = retirer {
            x.alarmes.remove(i);
        }
        ui.weak("Pour sortir le PC de veille, Windows doit autoriser les « minuteurs de sortie de veille » (Options d'alimentation › Veille). Outils doit rester ouvert (même rangé).");
        if x.alarmes != avant {
            enregistrer("reveil", &x.alarmes);
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 134 · Temps d'écran
// ---------------------------------------------------------------------------------------------------

type Jours = BTreeMap<String, BTreeMap<String, u64>>; // "AAAA-MM-JJ" -> appli -> secondes

pub fn date_locale(utc: i64, off: i64) -> String {
    let (y, m, d) = civil((utc + off).div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn duree(s: u64) -> String {
    if s >= 3600 { format!("{} h {:02}", s / 3600, s / 60 % 60) } else { format!("{} min", s / 60) }
}

pub(crate) fn inactif_depuis() -> u64 {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut l = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        GetLastInputInfo(&mut l);
        (windows_sys::Win32::System::SystemInformation::GetTickCount().wrapping_sub(l.dwTime) / 1000) as u64
    }
}

fn appli_devant() -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let h = GetForegroundWindow();
        if h.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(h, &mut pid);
        Some(crate::reseau::nom_processus(pid).trim_end_matches(".exe").to_string())
    }
}

pub struct TempsEcran {
    j: Arc<Mutex<Jours>>,
    jour: i64, // décalage depuis aujourd'hui (0, -1, …)
}

impl TempsEcran {
    pub fn new() -> Self {
        let j: Arc<Mutex<Jours>> = Arc::new(Mutex::new(charger("tempsecran")));
        let j2 = j.clone();
        std::thread::spawn(move || {
            let mut compte = 0;
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if inactif_depuis() > 180 {
                    continue; // personne devant l'écran
                }
                if let Some(a) = appli_devant() {
                    let d = date_locale(maintenant_utc(), decalage());
                    *j2.lock().unwrap().entry(d).or_default().entry(a).or_default() += 5;
                }
                compte += 1;
                if compte % 12 == 0 {
                    let mut x = j2.lock().unwrap();
                    while x.len() > 60 {
                        let premier = x.keys().next().cloned().unwrap();
                        x.remove(&premier); // garde 60 jours
                    }
                    enregistrer("tempsecran", &*x);
                }
            }
        });
        TempsEcran { j, jour: 0 }
    }
}

impl Outil for TempsEcran {
    fn nom(&self) -> &'static str {
        "📈 Temps d'écran"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Temps d'écran");
        ui.weak("Temps passé par appli au premier plan (pas compté après 3 min sans toucher au PC).");
        let off = decalage();
        let j = self.j.lock().unwrap().clone();
        let date = date_locale(maintenant_utc() + self.jour * 86400, off);
        ui.horizontal(|ui| {
            if ui.button("◀").clicked() {
                self.jour -= 1;
            }
            ui.strong(if self.jour == 0 { "Aujourd'hui".to_string() } else if self.jour == -1 { "Hier".into() } else { date.clone() });
            if self.jour < 0 && ui.button("▶").clicked() {
                self.jour += 1;
            }
        });
        let jour = j.get(&date).cloned().unwrap_or_default();
        let total: u64 = jour.values().sum();
        ui.label(RichText::new(format!("Total : {}", duree(total))).size(24.0).strong());
        let mut v: Vec<(&String, &u64)> = jour.iter().collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        for (a, s) in v.iter().take(15) {
            ui.horizontal(|ui| {
                ui.add(egui::ProgressBar::new(**s as f32 / total.max(1) as f32).desired_width(220.0).text(duree(**s)));
                ui.label(*a);
            });
        }
        ui.separator();
        ui.strong("7 derniers jours");
        let jours: Vec<(String, u64)> = (0..7).rev().map(|k| {
            let d = date_locale(maintenant_utc() - k * 86400, off);
            let t = j.get(&d).map(|m| m.values().sum()).unwrap_or(0);
            (d, t)
        }).collect();
        let max = jours.iter().map(|x| x.1).max().unwrap_or(1).max(1);
        let (zone, _) = ui.allocate_exact_size(egui::vec2(420.0, 120.0), egui::Sense::hover());
        let l = zone.width() / 7.0;
        for (i, (d, t)) in jours.iter().enumerate() {
            let h = (zone.height() - 18.0) * *t as f32 / max as f32;
            let r = egui::Rect::from_min_max(egui::pos2(zone.left() + i as f32 * l + 6.0, zone.bottom() - 18.0 - h), egui::pos2(zone.left() + (i + 1) as f32 * l - 6.0, zone.bottom() - 18.0));
            ui.painter().rect_filled(r, 3.0, Color32::from_rgb(255, 95, 162));
            ui.painter().text(egui::pos2(r.center().x, zone.bottom()), egui::Align2::CENTER_BOTTOM, &d[8..], egui::FontId::proportional(11.0), Color32::GRAY);
            if ui.rect_contains_pointer(r) {
                egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("te"), |ui| ui.label(format!("{d} : {}", duree(*t))));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------
// 135 · Mode concentration
// ---------------------------------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Concentration {
    pub sites: Vec<String>,
    pub applis: Vec<String>, // exe à fermer
    pub minutes: u32,
    pub fin: Option<i64>,
}

impl Default for Concentration {
    fn default() -> Self {
        Concentration {
            sites: vec!["youtube.com".into(), "twitch.tv".into(), "tiktok.com".into(), "instagram.com".into(), "reddit.com".into()],
            applis: vec!["Discord.exe".into()], minutes: 50, fin: None,
        }
    }
}

pub struct ModeConcentration {
    c: Concentration,
    texte_sites: String,
    texte_applis: String,
    erreur: Option<String>,
    fermeur: Arc<AtomicBool>,
    applis: Arc<Mutex<Vec<String>>>,
}

impl ModeConcentration {
    pub fn new() -> Self {
        let c: Concentration = charger("concentration");
        let fermeur = Arc::new(AtomicBool::new(c.fin.is_some()));
        let applis = Arc::new(Mutex::new(c.applis.clone()));
        let m = ModeConcentration { texte_sites: c.sites.join("\n"), texte_applis: c.applis.join("\n"), c, erreur: None, fermeur, applis };
        // ferme les applis interdites tant que la séance dure
        let (f, a2) = (m.fermeur.clone(), m.applis.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if f.load(Relaxed) {
                for exe in a2.lock().unwrap().iter() {
                    use std::os::windows::process::CommandExt;
                    let _ = std::process::Command::new("taskkill").args(["/IM", exe, "/F"]).creation_flags(0x0800_0000).output();
                }
            }
        });
        m
    }

    fn arreter(&mut self) {
        self.c.fin = None;
        self.fermeur.store(false, Relaxed);
        if let Err(e) = crate::hosts::appliquer("concentration", &[]) {
            self.erreur = Some(e);
        }
        enregistrer("concentration", &self.c);
    }
}

impl Outil for ModeConcentration {
    fn nom(&self) -> &'static str {
        "🧘 Mode concentration"
    }
    fn cat(&self) -> Cat {
        Cat::Quotidien
    }
    fn tic(&mut self, _: &egui::Context) {
        if self.c.fin.is_some_and(|f| maintenant_utc() >= f) {
            self.arreter();
            crate::systeme::notifier_toujours("Concentration", "Séance terminée, bravo !");
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Mode concentration");
        match self.c.fin {
            Some(f) => {
                let r = (f - maintenant_utc()).max(0);
                ui.label(RichText::new(format!("🧘 {:02}:{:02} restantes", r / 60, r % 60)).size(40.0).strong());
                ui.weak("Sites bloqués et applis fermées jusqu'à la fin.");
                if ui.button("Arrêter maintenant").clicked() {
                    self.arreter();
                }
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
            None => {
                ui.horizontal(|ui| {
                    ui.label("Durée");
                    ui.add(egui::DragValue::new(&mut self.c.minutes).range(5..=480).suffix(" min"));
                    if ui.button(RichText::new("▶ Commencer").size(18.0)).clicked() {
                        self.c.sites = self.texte_sites.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
                        self.c.applis = self.texte_applis.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
                        self.erreur = crate::hosts::appliquer("concentration", &self.c.sites).err();
                        self.c.fin = Some(maintenant_utc() + self.c.minutes as i64 * 60);
                        *self.applis.lock().unwrap() = self.c.applis.clone();
                        self.fermeur.store(true, Relaxed);
                        enregistrer("concentration", &self.c);
                    }
                });
                ui.columns(2, |c| {
                    c[0].strong("Sites à bloquer");
                    c[0].add(egui::TextEdit::multiline(&mut self.texte_sites).desired_rows(8));
                    c[1].strong("Applis à fermer (nom de l'exe)");
                    c[1].add(egui::TextEdit::multiline(&mut self.texte_applis).desired_rows(8));
                });
                ui.weak("Le blocage des sites modifie le fichier hosts : il faut lancer Outils en administrateur. Les applis sont refermées toutes les 5 s.");
            }
        }
        if let Some(e) = &self.erreur {
            ui.colored_label(Color32::LIGHT_RED, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prochaines_alarmes() {
        // mercredi 18/11/2026 10:00 (UTC+1)
        let off = 3600;
        let m = crate::pressepapiers::jours(2026, 11, 18) * 86400 + 9 * 3600;
        let a = Alarme { heure: "07:30".into(), jours: [true; 7], ..Default::default() };
        let t = prochaine(&a, m, off).unwrap();
        assert_eq!(crate::notes::format_local(t, off), "19/11/2026 07:30");
        let we = Alarme { heure: "09:00".into(), jours: [false, false, false, false, false, true, false], ..Default::default() };
        assert_eq!(crate::notes::format_local(prochaine(&we, m, off).unwrap(), off), "21/11/2026 09:00");
        assert_eq!(date_locale(m, off), "2026-11-18");
        assert_eq!(duree(3725), "1 h 02");
    }
}
