//! L'agent du PC, depuis le téléphone : « compresse mes vidéos et préviens-moi quand c'est fini ».
//! Le PC prépare le plan, tu le vérifies ici, il le fait, et le téléphone est prévenu à la fin.
use crate::commun::{en_fond, etat_ui, gros_bouton, Etat};
use crate::monpc::{encoder, Pc};
use crate::outil::{charger, Cat, Outil};
use eframe::egui::{self, Color32, RichText};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const EXEMPLES: [&str; 5] = [
    "range mes téléchargements et préviens-moi quand c'est fini",
    "compresse les vidéos de plus de 1 Go dans mes vidéos",
    "quels sont les plus gros fichiers",
    "combien reste-t-il de place",
    "supprime les doublons de mes images",
];

pub struct AgentTel {
    texte: String,
    plan: Arc<Mutex<Option<Value>>>,
    acceptees: Vec<bool>,
    suivi: Arc<Mutex<Option<Value>>>,
    dernier_suivi: Instant,
    e: Etat,
}

impl AgentTel {
    pub fn new() -> Self {
        AgentTel { texte: String::new(), plan: Arc::default(), acceptees: vec![], suivi: Arc::default(), dernier_suivi: Instant::now(), e: Etat::default() }
    }
    fn pc() -> Option<Pc> {
        charger("pc")
    }
    fn preparer(&mut self) {
        let Some(p) = Self::pc() else { return };
        let (t, plan) = (self.texte.trim().to_string(), self.plan.clone());
        self.acceptees.clear();
        en_fond(&self.e, "Le PC prépare le plan…", move || {
            let j = p.get_json(&format!("/api/agent/plan?texte={}", encoder(&t))).map_err(|e| format!("{e} (Outils sur le PC est-il à jour ?)"))?;
            let n = j["etapes"].as_array().map_or(0, |a| a.len());
            *plan.lock().unwrap() = Some(j);
            Ok(if n == 0 { "Le PC n'a pas compris, essaie plus simplement.".into() } else { format!("{n} étape(s)") })
        });
    }
}

impl Outil for AgentTel {
    fn nom(&self) -> &'static str {
        "🤖 Dire au PC quoi faire"
    }
    fn cat(&self) -> Cat {
        Cat::Reseau
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        let Some(p) = Self::pc() else {
            ui.label("Associe d'abord le téléphone à ton PC dans « Mon PC ».");
            return;
        };
        let occupe = etat_ui(ui, &self.e);
        ui.label(RichText::new("Dis au PC ce qu'il doit faire :").strong());
        let r = ui.add(egui::TextEdit::multiline(&mut self.texte).desired_rows(2).desired_width(f32::INFINITY).hint_text("ex. : range mes téléchargements et préviens-moi quand c'est fini"));
        if r.gained_focus() {
            // le clavier de l'appli s'ouvre tout seul sur les champs de texte
        }
        ui.horizontal_wrapped(|ui| {
            for x in EXEMPLES {
                if ui.small_button(x).clicked() {
                    self.texte = x.into();
                }
            }
        });
        if gros_bouton(ui, "📋 Préparer le plan") && !occupe && !self.texte.trim().is_empty() {
            self.preparer();
        }
        let plan = self.plan.lock().unwrap().clone();
        if let Some(j) = plan {
            let etapes = j["etapes"].as_array().cloned().unwrap_or_default();
            for x in j["incompris"].as_array().into_iter().flatten() {
                ui.colored_label(Color32::from_rgb(240, 150, 100), format!("Pas compris : « {} »", x.as_str().unwrap_or("")));
            }
            if !etapes.is_empty() {
                self.acceptees.resize(etapes.len(), false);
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new("Voilà ce que le PC va faire :").strong());
                    for (k, e) in etapes.iter().enumerate() {
                        let d = e["description"].as_str().unwrap_or("");
                        if e["dangereuse"].as_bool().unwrap_or(false) {
                            ui.checkbox(&mut self.acceptees[k], RichText::new(d).color(Color32::from_rgb(240, 150, 80))).on_hover_text("Supprime ou renomme : coche pour accepter");
                        } else {
                            ui.label(format!("{}. {d}", k + 1));
                        }
                    }
                });
                let pret = etapes.iter().enumerate().all(|(k, e)| !e["dangereuse"].as_bool().unwrap_or(false) || self.acceptees[k]);
                if !pret {
                    ui.weak("Coche les étapes en orange pour les accepter.");
                }
                if pret && gros_bouton(ui, "▶ Lancer sur le PC") && !occupe {
                    let plan_txt = serde_json::to_string(&etapes).unwrap_or_default();
                    let (p2, suivi) = (p.clone(), self.suivi.clone());
                    *self.plan.lock().unwrap() = None;
                    en_fond(&self.e, "Envoi au PC…", move || {
                        let r = p2.post(&format!("/api/agent/lancer?plan={}", encoder(&plan_txt)), b"")?;
                        *suivi.lock().unwrap() = Some(serde_json::json!({ "en_cours": true, "lignes": [] }));
                        Ok(r)
                    });
                }
            }
        }
        // suivi en direct
        let suivi = self.suivi.lock().unwrap().clone();
        if let Some(s) = suivi {
            let en_cours = s["en_cours"].as_bool().unwrap_or(false);
            ui.separator();
            ui.label(RichText::new(if en_cours { "⏳ Le PC travaille…" } else { "✔ Terminé" }).strong());
            for l in s["lignes"].as_array().into_iter().flatten() {
                let t = l.as_str().unwrap_or("");
                let c = if t.starts_with('✔') { Color32::from_rgb(110, 210, 120) } else if t.starts_with('✖') { Color32::from_rgb(240, 120, 100) } else { ui.visuals().text_color() };
                ui.colored_label(c, t);
            }
            if en_cours && self.dernier_suivi.elapsed() > Duration::from_millis(1500) {
                self.dernier_suivi = Instant::now();
                let (p2, suivi) = (p.clone(), self.suivi.clone());
                std::thread::spawn(move || {
                    if let Ok(j) = p2.get_json("/api/agent/suivi") {
                        *suivi.lock().unwrap() = Some(j);
                    }
                });
            }
            if en_cours {
                ui.ctx().request_repaint_after(Duration::from_millis(500));
            } else {
                if s["annulable"].as_u64().unwrap_or(0) > 0 && ui.button("↩ Tout remettre comme avant").clicked() && !occupe {
                    let p2 = p.clone();
                    en_fond(&self.e, "…", move || p2.post("/api/agent/annuler", b""));
                }
                ui.weak("Tu seras aussi prévenu par une notification quand le PC finit.");
            }
        }
    }
}
