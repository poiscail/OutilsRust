#![allow(dead_code, unused_imports)]
//! Outils pour Linux.
//!
//! Les jeux, les outils du quotidien et de sécurité sont **les mêmes fichiers que sous Windows**
//! (inclus avec `#[path]`) ; ce qui est propre à Windows est mis de côté avec `#[cfg(windows)]`.
//! Les outils système (infos, processus, disques, nettoyage, mises à jour, démarrage) et la sécurité
//! du réseau sont écrits pour Linux.

#[path = "../../src/minijeux.rs"]
mod minijeux;
#[path = "../../src/minijeux2.rs"]
mod minijeux2;
#[path = "../../src/minijeux3.rs"]
mod minijeux3;
#[path = "../../src/minijeux4.rs"]
mod minijeux4;
#[path = "../../src/ludique.rs"]
mod ludique;
#[path = "../../src/quotidien2.rs"]
mod quotidien2;
#[path = "../../src/quotidien3.rs"]
mod quotidien3;
#[path = "../../src/ctf.rs"]
mod ctf;
#[path = "../../src/secu_mdp.rs"]
mod secu_mdp;
#[path = "../../src/secu_fichiers.rs"]
mod secu_fichiers;
#[path = "../../src/dns_maison.rs"]
mod dns_maison;
#[path = "../../src/ports_risques.rs"]
mod ports_risques;
#[path = "../../src/pratique.rs"]
mod pratique;
#[path = "../../src/upnp.rs"]
mod upnp;
#[path = "../../src/vpn.rs"]
mod vpn;
#[path = "../../src/musique.rs"]
mod musique;
#[path = "../../src/createur.rs"]
mod createur;
#[path = "../../src/fete.rs"]
mod fete;
#[path = "../../src/atelier.rs"]
mod atelier;
#[path = "../../src/scene.rs"]
mod scene;
#[path = "../../src/blocs.rs"]
mod blocs;
#[path = "../../src/apprendre.rs"]
mod apprendre;
#[path = "../../src/ia.rs"]
mod ia;
#[path = "../../src/detourage.rs"]
mod detourage;
#[path = "../../src/nav_extensions.rs"]
#[allow(dead_code)]
mod nav_extensions;
#[path = "../../src/physique.rs"]
mod physique;
#[path = "../../src/espace.rs"]
mod espace;
#[path = "../../src/jeux_plus.rs"]
mod jeux_plus;
#[path = "../../src/creatif_plus.rs"]
mod creatif_plus;
#[path = "../../src/logique.rs"]
mod logique;
#[path = "../../src/multiroom.rs"]
mod multiroom;
#[path = "../../src/vie.rs"]
mod vie;
#[path = "../../src/robots.rs"]
mod robots;
#[path = "../../src/papiers.rs"]
mod papiers;
#[path = "../../src/sans_dossiers.rs"]
mod sans_dossiers;
#[path = "../../src/donjon.rs"]
mod donjon;
#[path = "../../src/agent.rs"]
mod agent;
#[path = "../../src/bureau_infini.rs"]
mod bureau_infini;
#[path = "../../src/catalogue_prix.rs"]
mod catalogue_prix;
#[path = "../../src/magasins.rs"]
mod magasins;
#[path = "../../src/menu.rs"]
mod menu;
#[path = "../../src/fiches.rs"]
mod fiches;

mod capture_linux;
mod creatif;
mod lanceur_linux;
mod maj_linux;
mod mc_linux;
mod moniteur_linux;
mod pont_linux;
mod minecraft;
mod outil;
mod reseau3;
mod reseau_linux;
mod systeme;
mod systeme_linux;
mod telephone;

use eframe::egui::{self, Color32, RichText};
use outil::{Cat, Outil};
use std::time::Duration;

const EMOJIS: &[u8] = include_bytes!("../../polices/NotoEmoji-Regular.ttf");
const ROSE: Color32 = Color32::from_rgb(255, 95, 162);

fn outils() -> Vec<Box<dyn Outil>> {
    vec![
        // jeux (propres à Linux)
        Box::new(lanceur_linux::Lanceur::new()),
        Box::new(moniteur_linux::Moniteur::new()),
        Box::new(mc_linux::ServeurMc::new()),
        Box::new(pratique::Sorties::new()),
        Box::new(pratique::ReglagesGraphiques::new()),
        Box::new(pont_linux::Telephone::new()),
        Box::new(capture_linux::Capture::new()),
        // système et fichiers (propres à Linux)
        Box::new(systeme_linux::InfosPc::new()),
        Box::new(systeme_linux::Processus::new()),
        Box::new(systeme_linux::Nettoyage::new()),
        Box::new(systeme_linux::MisesAJour::new()),
        Box::new(systeme_linux::Demarrage::new()),
        Box::new(systeme_linux::Disques::new()),
        // fichiers, liens et e-mails suspects
        Box::new(secu_fichiers::FichierSuspect::new()),
        Box::new(secu_fichiers::Hexa::new()),
        Box::new(secu_fichiers::Metadonnees::new()),
        // réseau
        Box::new(reseau_linux::SecuReseau::new()),
        Box::new(secu_fichiers::Lien::new()),
        Box::new(secu_fichiers::Mail::new()),
        Box::new(ctf::CalculReseau::new()),
        Box::new(dns_maison::DnsMaison::new()),
        // quotidien
        Box::new(pratique::ResumeJour::new()),
        Box::new(quotidien2::Meteo::new()),
        Box::new(pratique::Abonnements::new()),
        Box::new(pratique::TraductionDocs::new()),
        Box::new(ia::Assistant::new()),
        Box::new(espace::Planetarium::new()),
        Box::new(espace::SystemeSolaire::new()),
        Box::new(apprendre::Maths::new()),
        Box::new(apprendre::Periodique::new()),
        Box::new(vpn::Vpn::new()),
        Box::new(musique::Musique::new()),
        Box::new(createur::Shorts::new()),
        Box::new(createur::Studio::new()),
        Box::new(createur::Skins::new()),
        Box::new(blocs::PixelArtMc::new(None)),
        Box::new(blocs::Structures::new(None)),
        Box::new(ia::VoixRealiste::new()),
        Box::new(detourage::Detourage::new()),
        Box::new(ia::RecherchePhotos::new()),
        Box::new(creatif_plus::Piano::new()),
        Box::new(creatif_plus::Symetrie::new()),
        Box::new(creatif_plus::ArtAscii::new()),
        Box::new(creatif_plus::QrCodes::new()),
        Box::new(atelier::CreateurBd::new()),
        Box::new(atelier::CreateurPolice::new()),
        Box::new(physique::Physique::new()),
        Box::new(vie::EcosystemeOutil::new()),
        Box::new(vie::Evolution::new()),
        Box::new(robots::CombatRobots::new()),
        Box::new(bureau_infini::Bureau::new()),
        Box::new(agent::Agent::new()),
        Box::new(sans_dossiers::SansDossiers::new()),
        Box::new(donjon::Donjon::new()),
        Box::new(catalogue_prix::CataloguePrix::new()),
        Box::new(papiers::Lettres::new()),
        Box::new(papiers::CvOutil::new()),
        Box::new(logique::Labyrinthes::new()),
        Box::new(logique::Tortue::new()),
        Box::new(jeux_plus::Visee::new()),
        Box::new(jeux_plus::TestManette::new()),
        Box::new(jeux_plus::TierLists::new()),
        Box::new(jeux_plus::Rythme::new()),
        Box::new(jeux_plus::Puzzle::new()),
        Box::new(fete::Roue::new()),
        Box::new(fete::EscapeGame::new()),
        Box::new(multiroom::MusiqueMaison::new()),
        Box::new(scene::Visualiseur::new()),
        Box::new(scene::StopMotion::new()),
        Box::new(scene::Karaoke::new()),
        Box::new(quotidien2::Agenda::new()),
        Box::new(quotidien3::Ecole::new()),
        Box::new(quotidien3::ComptesRebours::new()),
        Box::new(quotidien3::Calculatrice::new()),
        Box::new(quotidien3::Actus::new()),
        Box::new(quotidien2::EauCalories::new()),
        Box::new(secu_mdp::Solidite::new()),
        Box::new(secu_mdp::Codes2fa::new()),
        Box::new(secu_mdp::Fuites::new()),
        Box::new(ludique::MotsDePasse::new()),
        Box::new(ludique::Des::new()),
        // jeux et défis
        Box::new(minijeux::MiniJeux::new()),
        Box::new(minijeux3::Echecs::new()),
        Box::new(minijeux4::Dames::new()),
        Box::new(minijeux3::Solitaire::new()),
        Box::new(minijeux4::Cartes::new()),
        Box::new(minijeux4::SudokuJour::new()),
        Box::new(minijeux3::MotsMeles::new()),
        Box::new(minijeux4::QuizDrapeaux::new()),
        Box::new(minijeux3::Frappe::new()),
        Box::new(minijeux3::Clicker::new()),
        Box::new(ludique::Reflexes::new()),
        Box::new(ludique::Sensibilite::new()),
        Box::new(minijeux3::ADeux::new()),
        Box::new(minijeux3::Reseau::new()),
        Box::new(minijeux4::Navale::new()),
        Box::new(minijeux4::SnakeDeux::new()),
        Box::new(minijeux4::CourseFrappe::new()),
        Box::new(ctf::Defis::new()),
        Box::new(ctf::Decodeur::new()),
        Box::new(ctf::Stegano::new()),
        Box::new(ctf::Hachage::new()),
        Box::new(ctf::Entrainement::new()),
    ]
}

struct App {
    outils: Vec<Box<dyn Outil>>,
    ouvert: Option<usize>,
    recherche: String,
    aide: bool,
    erreur: Option<(usize, String)>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let mut f = egui::FontDefinitions::default();
        f.font_data.insert("emojis".into(), std::sync::Arc::new(egui::FontData::from_static(EMOJIS)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            f.families.entry(fam).or_default().push("emojis".into());
        }
        // police du système pour les caractères rares (idéogrammes, symboles)
        for p in ["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/noto/NotoSans-Regular.ttf"] {
            if let Ok(o) = std::fs::read(p) {
                f.font_data.insert("systeme".into(), std::sync::Arc::new(egui::FontData::from_owned(o)));
                f.families.entry(egui::FontFamily::Proportional).or_default().push("systeme".into());
                break;
            }
        }
        cc.egui_ctx.set_fonts(f);
        cc.egui_ctx.style_mut(|s| {
            s.visuals.selection.bg_fill = ROSE.gamma_multiply(0.6);
            s.visuals.hyperlink_color = ROSE;
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        });
        menu::charger_langue();
        maj_linux::verifier();
        let ouvert = std::env::var("OUTILS_OUVRIR").ok().and_then(|x| x.parse().ok());
        App { outils: outils(), ouvert, recherche: String::new(), aide: false, erreur: None }
    }

    fn entrees(&self) -> Vec<(usize, String, Option<Cat>)> {
        let mut v = vec![(usize::MAX, "🏠 Accueil".to_string(), None)];
        v.extend(self.outils.iter().enumerate().map(|(k, o)| (k, o.nom().to_string(), Some(o.cat()))));
        v
    }

    fn accueil(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Outils pour Linux").color(ROSE).size(28.0));
        ui.label(format!("{} outils : système, sécurité, quotidien, jeux et défis. Tape ce que tu veux dans la recherche (« pc lent », « mot de passe », « disque plein »…).", self.outils.len()));
        ui.add_space(8.0);
        for c in Cat::TOUTES {
            let l: Vec<usize> = (0..self.outils.len()).filter(|k| self.outils[*k].cat() == c).collect();
            if l.is_empty() {
                continue;
            }
            ui.strong(menu::cat(c));
            ui.horizontal_wrapped(|ui| {
                for k in l {
                    let n = self.outils[k].nom();
                    let r = ui.add(egui::Button::new(menu::affiche(n)).min_size(egui::vec2(180.0, 30.0)));
                    let r = match menu::description(n) {
                        Some(d) => r.on_hover_text(d),
                        None => r,
                    };
                    if r.clicked() {
                        self.ouvert = Some(k);
                    }
                }
            });
            ui.add_space(6.0);
        }
        let resumes: Vec<(usize, String)> = self.outils.iter().enumerate().filter_map(|(k, o)| o.resume().map(|r| (k, r))).collect();
        if !resumes.is_empty() {
            ui.separator();
            for (k, r) in resumes {
                ui.horizontal(|ui| {
                    if ui.button(self.outils[k].nom()).clicked() {
                        self.ouvert = Some(k);
                    }
                    ui.label(r);
                });
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        for o in &mut self.outils {
            o.tic(ctx);
        }
        if let Some(n) = outil::OUVRIR.lock().ok().and_then(|mut o| o.take()) {
            self.ouvert = self.outils.iter().position(|o| o.nom() == n);
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let dispo = maj_linux::DISPO.lock().unwrap().clone();
        if let Some((v, _)) = dispo {
            egui::TopBottomPanel::top("maj").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("🔄 Nouvelle version {v} d'Outils disponible."));
                    if ui.button("Installer et relancer").clicked() {
                        maj_linux::installer();
                    }
                    ui.weak(maj_linux::ETAT.lock().unwrap().clone());
                });
            });
        }
        if let Some((t, m)) = systeme::NOTIFICATIONS.lock().ok().and_then(|mut n| n.pop()) {
            let _ = (t, m); // déjà montrée par le bureau (notify-send)
        }
        egui::SidePanel::left("menu").exact_width(230.0).show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("Outils").color(ROSE));
                ui.weak(format!("v{}", maj_linux::VERSION)).on_hover_text(maj_linux::ETAT.lock().unwrap().clone());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let en = menu::en();
                    if ui.small_button(if en { "🌐 EN" } else { "🌐 FR" }).clicked() {
                        menu::changer_langue(!en);
                    }
                });
            });
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text(menu::t("🔍 Chercher un outil")).desired_width(f32::INFINITY));
            let tous = self.entrees();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if ui.selectable_label(self.ouvert.is_none(), menu::t("🏠 Accueil")).clicked() {
                    self.ouvert = None;
                }
                let q = self.recherche.trim().to_string();
                if !q.is_empty() {
                    let sans_accueil: Vec<(usize, String, Option<Cat>)> = tous.iter().filter(|e| e.0 != usize::MAX).map(|e| (e.0 + 1, e.1.clone(), e.2)).collect();
                    for (i, n, _) in menu::chercher(&sans_accueil, &q) {
                        if ui.selectable_label(self.ouvert == Some(i - 1), menu::affiche(n)).clicked() {
                            self.ouvert = Some(i - 1);
                        }
                    }
                    return;
                }
                let actuelle = self.ouvert.map(|k| self.outils[k].cat());
                for c in Cat::TOUTES {
                    let l: Vec<&(usize, String, Option<Cat>)> = tous.iter().filter(|e| e.2 == Some(c)).collect();
                    if l.is_empty() {
                        continue;
                    }
                    let id = ui.make_persistent_id(("cat", c));
                    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, actuelle == Some(c)).show_header(ui, |ui| ui.strong(menu::cat(c))).body(|ui| {
                        let mut groupes: Vec<(&str, Vec<&(usize, String, Option<Cat>)>)> = vec![];
                        for e in l {
                            let g = menu::sous_cat(&e.1, c);
                            match groupes.iter_mut().find(|x| x.0 == g) {
                                Some(x) => x.1.push(e),
                                None => groupes.push((g, vec![e])),
                            }
                        }
                        let plusieurs = groupes.len() > 1;
                        for (g, l) in groupes {
                            if plusieurs {
                                ui.label(RichText::new(g).small().weak());
                            }
                            for (k, n, _) in l {
                            let r = ui.selectable_label(self.ouvert == Some(*k), menu::affiche(n));
                            let r = match menu::description(n) {
                                Some(d) => r.on_hover_text(d),
                                None => r,
                            };
                            if r.clicked() {
                                self.ouvert = Some(*k);
                            }
                            }
                        }
                    });
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(k) = self.ouvert else {
                egui::ScrollArea::vertical().show(ui, |ui| self.accueil(ui));
                return;
            };
            let nom = self.outils[k].nom();
            if let Some(d) = menu::description(nom) {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if ui.selectable_label(self.aide, "❓").on_hover_text(menu::t("Aide de cet outil")).clicked() {
                            self.aide = !self.aide;
                        }
                    });
                });
                if self.aide {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(d);
                    });
                }
            }
            if let Some((o, m)) = self.erreur.clone().filter(|e| e.0 == k) {
                let _ = o;
                ui.colored_label(Color32::LIGHT_RED, format!("💥 Cet outil a planté : {m}"));
                if ui.button("Réessayer").clicked() {
                    self.erreur = None;
                }
                return;
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| self.outils[k].ui(ui));
            }));
            if let Err(e) = r {
                let m = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "erreur inconnue".into());
                self.erreur = Some((k, m));
            }
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1150.0, 760.0]).with_title("Outils").with_app_id("outils"),
        ..Default::default()
    };
    eframe::run_native("Outils", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
