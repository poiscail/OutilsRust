//! Planificateur : tâches à heure fixe certains jours (sauvegardes des jeux, nettoyage, serveur Minecraft,
//! programme, notification, veille / arrêt du PC). Tourne dans un fil : marche fenêtre cachée.
//! Réglages dans %APPDATA%\Outils\planificateur.json.
use crate::minecraft::Pilote;
use crate::pressepapiers::{decalage, maintenant_utc};
use crate::{sauvegardes, systeme};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
pub const JOURS: [&str; 7] = ["Lun", "Mar", "Mer", "Jeu", "Ven", "Sam", "Dim"];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Action {
    SauvegarderJeux,
    Nettoyage,
    McDemarrer,
    McArreter,
    McRedemarrer,
    McSauvegarder,
    McCommande(String),
    Programme(String),
    Notification(String),
    Veille,
    Eteindre,
    Redemarrer,
}

impl Action {
    pub fn nom(&self) -> &'static str {
        match self {
            Action::SauvegarderJeux => "💾 Sauvegarder les jeux",
            Action::Nettoyage => "🧹 Nettoyage rapide (fichiers temporaires…)",
            Action::McDemarrer => "⛏ Démarrer le serveur Minecraft",
            Action::McArreter => "⛏ Arrêter le serveur Minecraft",
            Action::McRedemarrer => "⛏ Redémarrer le serveur Minecraft (prévient 1 min avant)",
            Action::McSauvegarder => "⛏ Sauvegarder le monde Minecraft",
            Action::McCommande(_) => "⛏ Commande serveur",
            Action::Programme(_) => "▶ Lancer un programme / une commande",
            Action::Notification(_) => "🔔 Notification",
            Action::Veille => "🌙 Mettre le PC en veille",
            Action::Eteindre => "⏻ Éteindre le PC (1 min pour annuler)",
            Action::Redemarrer => "🔄 Redémarrer le PC (1 min pour annuler)",
        }
    }

    fn toutes() -> Vec<Action> {
        vec![
            Action::SauvegarderJeux, Action::Nettoyage, Action::McDemarrer, Action::McArreter, Action::McRedemarrer, Action::McSauvegarder,
            Action::McCommande("say Bonjour !".into()), Action::Programme(String::new()), Action::Notification("Rappel".into()), Action::Veille,
            Action::Eteindre, Action::Redemarrer,
        ]
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Tache {
    pub nom: String,
    pub actif: bool,
    pub heure: String, // "HH:MM"
    pub jours: [bool; 7],
    pub actions: Vec<Action>,
    pub dernier_jour: i64, // jour local (depuis 1970) de la dernière exécution
}

impl Default for Tache {
    fn default() -> Self {
        Tache { nom: "Nouvelle tâche".into(), actif: true, heure: "04:00".into(), jours: [true; 7], actions: vec![], dernier_jour: -1 }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("planificateur.json")
}

pub fn lire_heure(h: &str) -> Option<i64> {
    let (a, b) = h.trim().split_once([':', 'h'])?;
    let (a, b): (i64, i64) = (a.trim().parse().ok()?, if b.trim().is_empty() { 0 } else { b.trim().parse().ok()? });
    (a < 24 && b < 60).then_some(a * 60 + b)
}

/// (jour local depuis 1970, jour de la semaine 0 = lundi, minute du jour)
pub fn moment(utc: i64, off: i64) -> (i64, usize, i64) {
    let l = utc + off;
    let jour = l.div_euclid(86400);
    (jour, (jour + 3).rem_euclid(7) as usize, l.rem_euclid(86400) / 60)
}

/// La tâche doit-elle partir maintenant ? (dans l'heure qui suit l'horaire, une fois par jour)
pub fn due(t: &Tache, jour: i64, jsem: usize, minute: i64) -> bool {
    let Some(h) = lire_heure(&t.heure) else { return false };
    t.actif && t.jours[jsem] && t.dernier_jour != jour && minute >= h && minute - h < 60
}

#[derive(Clone)]
pub struct Executeur {
    pub mc: Pilote,
    pub jeux: Arc<Mutex<sauvegardes::Reglages>>,
}

impl Executeur {
    pub fn executer(&self, a: &Action) -> String {
        let cmd = |prog: &str, args: &[&str]| Command::new(prog).args(args).creation_flags(CREATE_NO_WINDOW).spawn().map(|_| ()).map_err(|e| e.to_string());
        let r: Result<String, String> = match a {
            Action::SauvegarderJeux => {
                let r = self.jeux.lock().unwrap().clone();
                Ok(sauvegardes::tout_sauvegarder(&r))
            }
            Action::Nettoyage => Ok(crate::optimiseur::nettoyage_rapide()),
            Action::McDemarrer => self.mc.demarrer().map(|_| "Serveur démarré.".into()),
            Action::McArreter => {
                self.mc.arreter();
                Ok("Serveur arrêté.".into())
            }
            Action::McRedemarrer => {
                if self.mc.statut().is_some() {
                    self.mc.commande("say Redémarrage du serveur dans 1 minute.");
                    std::thread::sleep(Duration::from_secs(50));
                    self.mc.commande("say Redémarrage dans 10 secondes…");
                    std::thread::sleep(Duration::from_secs(10));
                    self.mc.arreter();
                    std::thread::sleep(Duration::from_secs(3));
                }
                self.mc.demarrer().map(|_| "Serveur redémarré.".into())
            }
            Action::McSauvegarder => self.mc.sauvegarder().map(|p| format!("Monde sauvegardé : {}", p.display())),
            Action::McCommande(c) => {
                if self.mc.statut().is_some() {
                    self.mc.commande(c);
                    Ok(format!("Commande envoyée : {c}"))
                } else {
                    Err("serveur arrêté".into())
                }
            }
            Action::Programme(p) => {
                if p.trim().is_empty() {
                    Err("aucun programme".into())
                } else {
                    cmd("cmd.exe", &["/C", p]).map(|_| format!("Lancé : {p}"))
                }
            }
            Action::Notification(t) => {
                systeme::notifier_toujours("Planificateur", t);
                Ok("Notification affichée.".into())
            }
            Action::Veille => {
                std::thread::sleep(Duration::from_secs(5));
                unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
                Ok("Mise en veille.".into())
            }
            Action::Eteindre => cmd("shutdown.exe", &["/s", "/t", "60", "/c", "Arrêt planifié par Outils (shutdown /a pour annuler)"]).map(|_| "Arrêt dans 1 min.".into()),
            Action::Redemarrer => cmd("shutdown.exe", &["/r", "/t", "60", "/c", "Redémarrage planifié par Outils (shutdown /a pour annuler)"]).map(|_| "Redémarrage dans 1 min.".into()),
        };
        r.unwrap_or_else(|e| format!("Échec ({}) : {e}", a.nom()))
    }
}

#[derive(Default)]
struct Partage {
    taches: Vec<Tache>,
    journal: Vec<String>,
    en_cours: usize,
}

pub struct Planif {
    p: Arc<Mutex<Partage>>,
    ex: Executeur,
    choisie: Option<usize>,
}

fn enregistrer(t: &[Tache]) {
    let f = fichier();
    let _ = std::fs::create_dir_all(f.parent().unwrap());
    let _ = std::fs::write(f, serde_json::to_string_pretty(t).unwrap_or_default());
}

fn lancer(p: &Arc<Mutex<Partage>>, ex: &Executeur, t: Tache) {
    let (p, ex) = (p.clone(), ex.clone());
    p.lock().unwrap().en_cours += 1;
    std::thread::spawn(move || {
        let off = decalage();
        let heure = |utc: i64| {
            let (_, _, m) = moment(utc, off);
            format!("{:02}:{:02}", m / 60, m % 60)
        };
        for a in &t.actions {
            let r = ex.executer(a);
            let mut x = p.lock().unwrap();
            x.journal.insert(0, format!("{} · {} · {r}", heure(maintenant_utc()), t.nom));
            x.journal.truncate(100);
        }
        p.lock().unwrap().en_cours -= 1;
        systeme::notifier("Planificateur", &format!("« {} » terminé.", t.nom));
    });
}

impl Planif {
    pub fn new(ex: Executeur) -> Self {
        let taches: Vec<Tache> = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let p = Arc::new(Mutex::new(Partage { taches, ..Default::default() }));
        let (p2, ex2) = (p.clone(), ex.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(20));
            let (jour, jsem, minute) = moment(maintenant_utc(), decalage());
            let a_lancer: Vec<Tache> = {
                let mut x = p2.lock().unwrap();
                let mut v = vec![];
                for t in x.taches.iter_mut() {
                    if due(t, jour, jsem, minute) {
                        t.dernier_jour = jour;
                        v.push(t.clone());
                    }
                }
                if !v.is_empty() {
                    enregistrer(&x.taches);
                }
                v
            };
            for t in a_lancer {
                lancer(&p2, &ex2, t);
            }
        });
        Planif { p, ex, choisie: None }
    }

    pub fn resume(&self) -> String {
        let x = self.p.lock().unwrap();
        let actives = x.taches.iter().filter(|t| t.actif).count();
        match x.journal.first() {
            Some(j) => format!("{actives} tâche(s) active(s) · dernier : {j}"),
            None => format!("{actives} tâche(s) active(s)"),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Planificateur");
        let avant = self.p.lock().unwrap().taches.clone();
        let mut taches = avant.clone();
        let (journal, en_cours) = {
            let x = self.p.lock().unwrap();
            (x.journal.clone(), x.en_cours)
        };
        ui.horizontal(|ui| {
            if ui.button("➕ Nouvelle tâche").clicked() {
                taches.push(Tache::default());
                self.choisie = Some(taches.len() - 1);
            }
            if ui.button("🌙 Exemple : entretien de nuit").clicked() {
                taches.push(Tache {
                    nom: "Entretien de nuit".into(), heure: "04:00".into(),
                    actions: vec![Action::SauvegarderJeux, Action::McSauvegarder, Action::McRedemarrer, Action::Nettoyage],
                    ..Default::default()
                });
                self.choisie = Some(taches.len() - 1);
            }
            if en_cours > 0 {
                ui.spinner();
                ui.label(format!("{en_cours} tâche(s) en cours"));
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            }
        });
        ui.weak("Les tâches partent à l'heure dite (ou dans l'heure qui suit si le PC était occupé), tant qu'Outils est ouvert, même rangé.");
        ui.separator();
        let mut supprimer = None;
        let mut lancer_maintenant = None;
        ui.columns(2, |c| {
            for (i, t) in taches.iter_mut().enumerate() {
                c[0].horizontal(|ui| {
                    ui.checkbox(&mut t.actif, "");
                    let jours: String = if t.jours.iter().all(|&j| j) {
                        "tous les jours".into()
                    } else {
                        JOURS.iter().zip(t.jours).filter(|(_, j)| *j).map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
                    };
                    if ui.selectable_label(self.choisie == Some(i), format!("{} · {} · {}", t.nom, t.heure, jours)).clicked() {
                        self.choisie = Some(i);
                    }
                });
            }
            if taches.is_empty() {
                c[0].weak("Aucune tâche.");
            }
            let ui = &mut c[1];
            if let Some(t) = self.choisie.and_then(|i| taches.get_mut(i)) {
                ui.add(egui::TextEdit::singleline(&mut t.nom).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    ui.label("Heure");
                    ui.add(egui::TextEdit::singleline(&mut t.heure).desired_width(60.0));
                    if lire_heure(&t.heure).is_none() {
                        ui.colored_label(egui::Color32::LIGHT_RED, "HH:MM");
                    }
                });
                ui.horizontal(|ui| {
                    for (k, n) in JOURS.iter().enumerate() {
                        ui.toggle_value(&mut t.jours[k], *n);
                    }
                });
                ui.separator();
                ui.strong("Actions (dans l'ordre)");
                let mut retirer = None;
                let mut monter = None;
                for (k, a) in t.actions.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", k + 1));
                        ui.label(a.nom());
                        match a {
                            Action::McCommande(s) | Action::Programme(s) | Action::Notification(s) => {
                                ui.add(egui::TextEdit::singleline(s).desired_width(180.0));
                            }
                            _ => {}
                        }
                        if k > 0 && ui.small_button("⬆").clicked() {
                            monter = Some(k);
                        }
                        if ui.small_button("✖").clicked() {
                            retirer = Some(k);
                        }
                    });
                }
                if let Some(k) = monter {
                    t.actions.swap(k, k - 1);
                }
                if let Some(k) = retirer {
                    t.actions.remove(k);
                }
                egui::ComboBox::from_id_salt("ajout_action").selected_text("➕ Ajouter une action").width(320.0).show_ui(ui, |ui| {
                    for a in Action::toutes() {
                        if ui.selectable_label(false, a.nom()).clicked() {
                            t.actions.push(a);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!t.actions.is_empty(), egui::Button::new("▶ Lancer maintenant")).clicked() {
                        lancer_maintenant = Some(t.clone());
                    }
                    if ui.button("🗑 Supprimer").clicked() {
                        supprimer = self.choisie;
                    }
                });
            } else {
                ui.weak("Choisis une tâche à gauche.");
            }
        });
        if let Some(t) = lancer_maintenant {
            lancer(&self.p, &self.ex, t);
        }
        if let Some(i) = supprimer {
            taches.remove(i);
            self.choisie = None;
        }
        ui.separator();
        ui.strong("Journal");
        egui::ScrollArea::vertical().id_salt("journal_planif").max_height(160.0).show(ui, |ui| {
            for l in &journal {
                ui.label(l);
            }
            if journal.is_empty() {
                ui.weak("Rien pour l'instant.");
            }
        });
        if taches != avant {
            let mut x = self.p.lock().unwrap();
            if x.taches == avant {
                x.taches = taches;
                enregistrer(&x.taches);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heures() {
        assert_eq!(lire_heure("04:30"), Some(270));
        assert_eq!(lire_heure("21h"), Some(1260));
        assert_eq!(lire_heure("24:00"), None);
    }

    #[test]
    fn semaine() {
        // 18/11/2026 est un mercredi
        let utc = crate::pressepapiers::jours(2026, 11, 18) * 86400 + 10 * 3600;
        let (_, js, m) = moment(utc, 3600);
        assert_eq!(js, 2);
        assert_eq!(m, 11 * 60);
    }

    #[test]
    fn declenchement() {
        let mut t = Tache { heure: "04:00".into(), ..Default::default() };
        assert!(!due(&t, 10, 0, 239)); // avant
        assert!(due(&t, 10, 0, 240));
        assert!(due(&t, 10, 0, 299));
        assert!(!due(&t, 10, 0, 300)); // plus d'une heure après : on laisse tomber
        t.dernier_jour = 10;
        assert!(!due(&t, 10, 0, 245)); // déjà faite aujourd'hui
        t.dernier_jour = 9;
        t.jours[0] = false;
        assert!(!due(&t, 10, 0, 245)); // pas le lundi
    }
}
