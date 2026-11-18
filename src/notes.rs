//! Notes rapides : notes colorées, post-its épinglés toujours devant, rappels avec notification.
//! Enregistrées dans %APPDATA%\Outils\notes.json.
use crate::pressepapiers::{civil, decalage, jours, maintenant_utc};
use crate::systeme;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const COULEURS: [[u8; 3]; 6] = [[255, 214, 102], [255, 145, 190], [150, 210, 255], [160, 230, 160], [200, 170, 255], [70, 70, 80]];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Note {
    pub id: u64,
    pub titre: String,
    pub texte: String,
    pub couleur: [u8; 3],
    pub epinglee: bool,
    pub rappel: Option<i64>, // secondes UTC
}

impl Default for Note {
    fn default() -> Self {
        Note { id: 0, titre: String::new(), texte: String::new(), couleur: COULEURS[0], epinglee: false, rappel: None }
    }
}

fn fichier() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or(".".into())).join("Outils").join("notes.json")
}

/// "JJ/MM/AAAA HH:MM" en heure locale.
pub fn format_local(utc: i64, off: i64) -> String {
    let l = utc + off;
    let (y, m, d) = civil(l.div_euclid(86400));
    let s = l.rem_euclid(86400);
    format!("{d:02}/{m:02}/{y:04} {:02}:{:02}", s / 3600, s / 60 % 60)
}

/// Lit "JJ/MM/AAAA HH:MM" (ou "JJ/MM HH:MM", ou "HH:MM" = aujourd'hui) en heure locale -> UTC.
pub fn lire_local(t: &str, maintenant: i64, off: i64) -> Option<i64> {
    let t = t.trim();
    let (date, heure) = match t.rsplit_once(' ') {
        Some((d, h)) => (Some(d.trim()), h),
        None => (None, t),
    };
    let (h, mi) = heure.split_once([':', 'h'])?;
    let (h, mi): (i64, i64) = (h.trim().parse().ok()?, if mi.trim().is_empty() { 0 } else { mi.trim().parse().ok()? });
    if h > 23 || mi > 59 {
        return None;
    }
    let (ya, ma, da) = civil((maintenant + off).div_euclid(86400));
    let (y, m, d) = match date {
        None => (ya, ma, da),
        Some(dt) => {
            let p: Vec<&str> = dt.split('/').collect();
            let d: i64 = p.first()?.parse().ok()?;
            let m: i64 = p.get(1)?.parse().ok()?;
            let y: i64 = match p.get(2) {
                Some(y) => {
                    let y: i64 = y.parse().ok()?;
                    if y < 100 { 2000 + y } else { y }
                }
                None => ya,
            };
            if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
                return None;
            }
            (y, m, d)
        }
    };
    Some(jours(y, m, d) * 86400 + h * 3600 + mi * 60 - off)
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn texte_sur(c: [u8; 3]) -> Color32 {
    let lum = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
    if lum > 140.0 { Color32::from_rgb(30, 30, 30) } else { Color32::from_rgb(240, 240, 240) }
}

type Partage = Arc<Mutex<Vec<Note>>>;

pub struct Notes {
    notes: Partage,
    modifie: Arc<AtomicBool>,
    choisie: Option<u64>,
    saisie_rappel: String,
    erreur_rappel: bool,
    recherche: String,
}

impl Notes {
    pub fn new() -> Self {
        let v: Vec<Note> = std::fs::read_to_string(fichier()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let notes: Partage = Arc::new(Mutex::new(v));
        let modifie: Arc<AtomicBool> = Arc::default();
        // fil des rappels : tourne même fenêtre cachée
        let (n2, m2) = (notes.clone(), modifie.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(15));
            let maintenant = maintenant_utc();
            let mut dus = vec![];
            for n in n2.lock().unwrap().iter_mut() {
                if n.rappel.is_some_and(|r| r <= maintenant) {
                    n.rappel = None;
                    dus.push(n.clone());
                }
            }
            for n in dus {
                use windows_sys::Win32::{System::Diagnostics::Debug::MessageBeep, UI::WindowsAndMessaging::MB_ICONASTERISK};
                unsafe { MessageBeep(MB_ICONASTERISK) };
                let titre = if n.titre.is_empty() { "Rappel".to_string() } else { format!("Rappel : {}", n.titre) };
                systeme::notifier_toujours(&titre, n.texte.lines().next().unwrap_or(""));
                m2.store(true, Relaxed);
            }
        });
        Notes { notes, modifie, choisie: None, saisie_rappel: String::new(), erreur_rappel: false, recherche: String::new() }
    }

    fn enregistrer(&self) {
        let f = fichier();
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        if let Ok(s) = serde_json::to_string_pretty(&*self.notes.lock().unwrap()) {
            let _ = std::fs::write(f, s);
        }
    }

    pub fn resume(&self) -> String {
        let n = self.notes.lock().unwrap();
        let prochains: Vec<i64> = n.iter().filter_map(|x| x.rappel).collect();
        match prochains.iter().min() {
            Some(r) => format!("{} note(s) · prochain rappel le {}", n.len(), format_local(*r, decalage())),
            None => format!("{} note(s)", n.len()),
        }
    }

    /// À chaque image : post-its et sauvegarde.
    pub fn tic(&mut self, ctx: &egui::Context) {
        let epinglees: Vec<(u64, String)> = self.notes.lock().unwrap().iter().filter(|n| n.epinglee)
            .map(|n| (n.id, if n.titre.is_empty() { "Note".into() } else { n.titre.clone() })).collect();
        for (id, titre) in epinglees {
            let (notes, modifie) = (self.notes.clone(), self.modifie.clone());
            ctx.show_viewport_deferred(
                egui::ViewportId::from_hash_of(("postit", id)),
                egui::ViewportBuilder::default().with_title(format!("📌 {titre}")).with_inner_size([260.0, 220.0]).with_always_on_top(),
                move |ctx, _| {
                    let mut v = notes.lock().unwrap();
                    let Some(n) = v.iter_mut().find(|n| n.id == id) else { return };
                    if ctx.input(|i| i.viewport().close_requested()) {
                        n.epinglee = false;
                        modifie.store(true, Relaxed);
                        return;
                    }
                    let fond = rgb(n.couleur);
                    egui::CentralPanel::default().frame(egui::Frame::new().fill(fond).inner_margin(8.0)).show(ctx, |ui| {
                        ui.visuals_mut().override_text_color = Some(texte_sur(n.couleur));
                        ui.visuals_mut().extreme_bg_color = fond;
                        let avant = n.texte.clone();
                        ui.add_sized(ui.available_size(), egui::TextEdit::multiline(&mut n.texte).frame(false));
                        if n.texte != avant {
                            modifie.store(true, Relaxed);
                        }
                    });
                },
            );
        }
        if self.modifie.swap(false, Relaxed) {
            self.enregistrer();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Notes rapides");
        let off = decalage();
        let avant = self.notes.lock().unwrap().clone();
        let mut notes = avant.clone();
        ui.horizontal(|ui| {
            if ui.button("➕ Nouvelle note").clicked() {
                let id = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                notes.insert(0, Note { id, couleur: COULEURS[notes.len() % COULEURS.len()], ..Default::default() });
                self.choisie = Some(id);
                self.saisie_rappel.clear();
            }
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍 Chercher").desired_width(200.0));
        });
        ui.separator();
        let q = self.recherche.to_lowercase();
        ui.columns(2, |cols| {
            // liste à gauche : cartes colorées
            egui::ScrollArea::vertical().id_salt("liste_notes").show(&mut cols[0], |ui| {
                for n in notes.iter().filter(|n| q.is_empty() || n.titre.to_lowercase().contains(&q) || n.texte.to_lowercase().contains(&q)) {
                    let choisie = self.choisie == Some(n.id);
                    let rep = egui::Frame::new().fill(rgb(n.couleur)).corner_radius(6.0).inner_margin(8.0)
                        .stroke(if choisie { egui::Stroke::new(2.0f32, Color32::WHITE) } else { egui::Stroke::NONE })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let c = texte_sur(n.couleur);
                            let titre = if n.titre.is_empty() { n.texte.lines().next().unwrap_or("(vide)").to_string() } else { n.titre.clone() };
                            ui.label(egui::RichText::new(format!("{}{titre}", if n.epinglee { "📌 " } else { "" })).strong().color(c));
                            let extrait: String = n.texte.chars().take(90).collect();
                            ui.label(egui::RichText::new(extrait.replace('\n', " ")).small().color(c));
                            if let Some(r) = n.rappel {
                                ui.label(egui::RichText::new(format!("⏰ {}", format_local(r, off))).small().color(c));
                            }
                        })
                        .response;
                    if ui.interact(rep.rect, ui.id().with(("note", n.id)), egui::Sense::click()).clicked() {
                        self.choisie = Some(n.id);
                        self.saisie_rappel = n.rappel.map(|r| format_local(r, off)).unwrap_or_default();
                        self.erreur_rappel = false;
                    }
                    ui.add_space(4.0);
                }
                if notes.is_empty() {
                    ui.weak("Aucune note. Clique sur « Nouvelle note ».");
                }
            });
            // édition à droite
            let ui = &mut cols[1];
            let mut supprimer = false;
            if let Some(n) = self.choisie.and_then(|id| notes.iter_mut().find(|n| n.id == id)) {
                ui.add(egui::TextEdit::singleline(&mut n.titre).hint_text("Titre").font(egui::TextStyle::Heading).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    for c in COULEURS {
                        let (r, rep) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                        ui.painter().circle_filled(r.center(), 9.0, rgb(c));
                        if n.couleur == c {
                            ui.painter().circle_stroke(r.center(), 10.0, egui::Stroke::new(2.0f32, Color32::WHITE));
                        }
                        if rep.clicked() {
                            n.couleur = c;
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut n.epinglee, "📌 Post-it à l'écran");
                });
                ui.add(egui::TextEdit::multiline(&mut n.texte).hint_text("Écris ici…").desired_rows(12).desired_width(f32::INFINITY));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("⏰ Rappel");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.saisie_rappel).hint_text("JJ/MM/AAAA HH:MM ou HH:MM").desired_width(150.0));
                    if r.changed() {
                        if self.saisie_rappel.trim().is_empty() {
                            n.rappel = None;
                            self.erreur_rappel = false;
                        } else {
                            match lire_local(&self.saisie_rappel, maintenant_utc(), off) {
                                Some(t) => {
                                    n.rappel = Some(t);
                                    self.erreur_rappel = false;
                                }
                                None => self.erreur_rappel = true,
                            }
                        }
                    }
                    if self.erreur_rappel {
                        ui.colored_label(Color32::LIGHT_RED, "date invalide");
                    }
                });
                ui.horizontal(|ui| {
                    let m = maintenant_utc();
                    let demain9 = ((m + off).div_euclid(86400) + 1) * 86400 + 9 * 3600 - off;
                    for (nom, t) in [("+10 min", m + 600), ("+1 h", m + 3600), ("Ce soir 20h", ((m + off).div_euclid(86400)) * 86400 + 20 * 3600 - off), ("Demain 9h", demain9)] {
                        if ui.small_button(nom).clicked() {
                            n.rappel = Some(t);
                            self.saisie_rappel = format_local(t, off);
                            self.erreur_rappel = false;
                        }
                    }
                    if n.rappel.is_some() && ui.small_button("Retirer").clicked() {
                        n.rappel = None;
                        self.saisie_rappel.clear();
                    }
                });
                if n.rappel.is_some_and(|r| r <= maintenant_utc()) {
                    ui.colored_label(Color32::YELLOW, "Cette heure est passée : le rappel sonnera tout de suite.");
                }
                ui.add_space(8.0);
                if ui.button("🗑 Supprimer la note").clicked() {
                    supprimer = true;
                }
            } else {
                ui.weak("Choisis une note à gauche.");
            }
            if supprimer {
                notes.retain(|n| Some(n.id) != self.choisie);
                self.choisie = None;
            }
        });
        if notes != avant {
            // le fil des rappels ou un post-it a pu changer la liste entre-temps : on fusionne au plus simple
            let mut v = self.notes.lock().unwrap();
            if *v == avant {
                *v = notes;
            }
            drop(v);
            self.enregistrer();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_locales() {
        let off = 3600; // UTC+1
        let t = lire_local("18/11/2026 19:30", 0, off).unwrap();
        assert_eq!(format_local(t, off), "18/11/2026 19:30");
        let midi = jours(2026, 11, 18) * 86400 + 12 * 3600 - off;
        assert_eq!(format_local(lire_local("21h", midi, off).unwrap(), off), "18/11/2026 21:00");
        assert_eq!(format_local(lire_local("25/12 08:05", midi, off).unwrap(), off), "25/12/2026 08:05");
        assert!(lire_local("32/01/2026 10:00", midi, off).is_none());
        assert!(lire_local("25:00", midi, off).is_none());
    }
}
