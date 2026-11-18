#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Outils : tous les outils dans un seul programme.
mod capture;
mod coffre;
mod coffrefichiers;
mod demarrage;
mod discord;
mod convertisseur;
mod disque;
mod dnd;
mod lanceur;
mod majlogiciels;
mod minecraft;
mod minuteur;
mod mixeur;
mod moniteur;
mod mods;
mod notes;
mod optimiseur;
mod planif;
mod pressepapiers;
mod promos;
mod renommer;
mod reseau;
mod santedisque;
mod sauvegardes;
mod systeme;
mod telephone;
mod theme;
mod traducteur;
mod viseur;
mod tri;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

/// Dossier "Outils" : le premier parent de l'exe qui contient OutilsRust.
fn racine() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors().find(|d| d.join("OutilsRust").is_dir()).map(Path::to_path_buf)
        .unwrap_or_else(|| exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

const ONGLETS: &[&str] = &[
    "🏠 Accueil", "📂 Tri Téléchargements", "🎮 Lanceur Jeux", "🔒 Coffre", "🌐 Traducteur", "📋 Presse-papiers",
    "⛏ Serveur Minecraft", "📊 Moniteur Jeu", "🚀 Optimiseur", "✏ Renommer", "⏱ Minuteur", "🎨 Personnaliser",
    "💽 Espace disque", "💾 Sauvegardes jeux", "🔁 Convertisseur", "📝 Notes", "📡 Réseau", "💬 Discord", "📸 Capture", "🎚 Mixeur audio", "🗓 Planificateur", "📱 Téléphone", "🌙 Ne pas déranger", "🔌 Démarrage PC", "🏷 Promos Steam", "🧩 Mods", "⬆ Mises à jour logiciels", "🩺 Santé des disques", "🗄 Coffre de fichiers", "🎯 Viseur",
];
const PRESSE: usize = 5;

fn go(octets: u64) -> String {
    format!("{:.0} Go", octets as f64 / (1u64 << 30) as f64)
}

struct App {
    tri: tri::Tri,
    lanceur: lanceur::Lanceur,
    coffre: coffre::CoffreUi,
    traducteur: traducteur::Traducteur,
    presse: pressepapiers::PressePapiers,
    minecraft: minecraft::Minecraft,
    moniteur: moniteur::Moniteur,
    optimiseur: optimiseur::Optimiseur,
    renommer: renommer::Renommer,
    minuteur: minuteur::Minuteur,
    disque: disque::Disque,
    sauvegardes: sauvegardes::Sauvegardes,
    convertisseur: convertisseur::Convertisseur,
    notes: notes::Notes,
    reseau: reseau::Reseau,
    discord: discord::Discord,
    capture: capture::Capture,
    mixeur: mixeur::Mixeur,
    planif: planif::Planif,
    telephone: telephone::Telephone,
    dnd: dnd::Dnd,
    demarrage: demarrage::DemarragePc,
    promos: promos::Promos,
    mods: mods::Mods,
    majlog: majlogiciels::MajLogiciels,
    sante: santedisque::SanteDisque,
    coffre_f: coffrefichiers::CoffreFichiers,
    viseur: viseur::Viseur,
    theme: theme::Theme,
    maj: systeme::MajPartagee,
    maj_ignoree: bool,
    disques: (Option<Instant>, Vec<(String, u64, u64)>),
    lance_au_demarrage: bool, // lancé par Windows : on démarre rangé
    onglet: usize,            // index dans ONGLETS
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx); // jaquettes des jeux
        systeme::nettoyer_ancienne();
        let racine = racine();
        let theme = theme::Theme::new(); // règle aussi notifications, zone et raccourci
        systeme::lancer(cc.egui_ctx.clone());
        let maj = systeme::MajPartagee::default();
        if theme.r.maj {
            systeme::verifier(maj.clone());
        }
        let minecraft = minecraft::Minecraft::new();
        let sauvegardes = sauvegardes::Sauvegardes::new();
        let planif = planif::Planif::new(planif::Executeur { mc: minecraft.pilote(), jeux: sauvegardes.poignee() });
        let telephone = telephone::Telephone::new(minecraft.pilote());
        App {
            tri: tri::Tri::new(&racine), lanceur: lanceur::Lanceur::new(), coffre: coffre::CoffreUi::new(), traducteur: traducteur::Traducteur::new(),
            presse: pressepapiers::PressePapiers::new(), minecraft, moniteur: moniteur::Moniteur::new(&racine),
            optimiseur: optimiseur::Optimiseur::new(), renommer: renommer::Renommer::new(), minuteur: minuteur::Minuteur::new(), disque: disque::Disque::new(), sauvegardes, convertisseur: convertisseur::Convertisseur::new(), notes: notes::Notes::new(), reseau: reseau::Reseau::new(), discord: discord::Discord::new(), capture: capture::Capture::new(), mixeur: mixeur::Mixeur::new(), planif, telephone, dnd: dnd::Dnd::new(), demarrage: demarrage::DemarragePc::new(), promos: promos::Promos::new(), mods: mods::Mods::new(), majlog: majlogiciels::MajLogiciels::new(),
            sante: santedisque::SanteDisque::new(), coffre_f: coffrefichiers::CoffreFichiers::new(), viseur: viseur::Viseur::new(), theme, maj,
            maj_ignoree: false, disques: (None, vec![]), lance_au_demarrage: std::env::args().any(|a| a == "--demarrage"), onglet: 0,
        }
    }

    fn accueil(&mut self, ui: &mut egui::Ui) {
        ui.heading("Accueil");
        ui.add_space(8.0);
        if self.disques.0.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
            self.disques = (Some(Instant::now()), systeme::disques());
        }
        ui.strong("Disques");
        for (d, libre, total) in &self.disques.1 {
            let pris = 1.0 - *libre as f32 / (*total).max(1) as f32;
            ui.horizontal(|ui| {
                ui.label(d);
                ui.add(egui::ProgressBar::new(pris).desired_width(260.0).text(format!("{} libres sur {}", go(*libre), go(*total))));
            });
        }
        ui.add_space(8.0);
        ui.separator();
        let coffre = match self.coffre.faibles() {
            None => "Verrouillé".to_string(),
            Some(0) => "Ouvert · aucun mot de passe faible".to_string(),
            Some(n) => format!("Ouvert · {n} mot(s) de passe faible(s)"),
        };
        let lignes = [
            (1, self.tri.resume()),
            (6, self.minecraft.resume()),
            (3, coffre),
            (10, self.minuteur.resume()),
            (13, self.sauvegardes.resume()),
            (15, self.notes.resume()),
            (18, self.capture.resume()),
            (19, self.mixeur.resume()),
            (20, self.planif.resume()),
            (21, self.telephone.resume()),
            (22, self.dnd.resume()),
            (24, self.promos.resume()),
            (27, self.sante.resume()),
        ];
        egui::Grid::new("accueil").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
            for (i, texte) in lignes {
                if ui.button(ONGLETS[i]).clicked() {
                    self.onglet = i;
                }
                ui.label(texte);
                ui.end_row();
            }
            ui.label("🔄 Mise à jour");
            ui.horizontal(|ui| {
                ui.label(self.maj.lock().unwrap().etat.clone());
                if ui.small_button("Vérifier").clicked() {
                    systeme::verifier(self.maj.clone());
                }
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.separator();
        ui.strong("Tous les outils");
        ui.horizontal_wrapped(|ui| {
            for (i, n) in ONGLETS.iter().enumerate().skip(1) {
                if ui.add(egui::Button::new(*n).min_size(egui::vec2(170.0, 34.0))).clicked() {
                    self.onglet = i;
                }
            }
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        systeme::fin();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if std::mem::take(&mut self.lance_au_demarrage) {
            if self.theme.r.zone {
                systeme::cacher();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        if systeme::QUITTER.load(Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if ctx.input(|i| i.viewport().close_requested()) && systeme::zone_active() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose); // fermer = ranger dans la zone de notification
            systeme::cacher();
        }
        self.theme.peindre(ctx); // avant les panneaux : le fond passe dessous
        self.tri.relever();
        self.coffre.tic(ctx);
        self.minecraft.tic();
        self.moniteur.tic(ctx);
        self.minuteur.tic();
        self.notes.tic(ctx);
        self.reseau.tic();
        self.viseur.tic();
        let jeu = self.lanceur.jeu_actif();
        if self.capture.tic(jeu.is_some()) {
            self.onglet = 18;
        }
        self.mixeur.tic(jeu.as_deref());
        self.dnd.tic(jeu.as_deref(), &self.mixeur);
        self.discord.tic(jeu, self.minecraft.statut());
        if self.presse.tic(ctx) {
            self.onglet = PRESSE;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        let dispo = self.maj.lock().unwrap().dispo.clone();
        if let (Some((version, _)), false) = (dispo, self.maj_ignoree) {
            egui::TopBottomPanel::top("maj").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("🔄 Nouvelle version {version} disponible."));
                    if ui.button("Installer et relancer").clicked() {
                        systeme::installer(self.maj.clone());
                    }
                    if ui.button("Plus tard").clicked() {
                        self.maj_ignoree = true;
                    }
                    ui.weak(self.maj.lock().unwrap().etat.clone());
                });
            });
        }
        egui::SidePanel::left("outils").exact_width(210.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Outils");
            ui.separator();
            // 25 onglets : la liste défile si la fenêtre est petite
            egui::ScrollArea::vertical().id_salt("menu").max_height(ui.available_height() - 24.0).show(ui, |ui| {
                for (i, n) in ONGLETS.iter().enumerate() {
                    if ui.selectable_label(self.onglet == i, *n).clicked() {
                        self.onglet = i;
                    }
                }
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| ui.weak(concat!("v", env!("CARGO_PKG_VERSION"))));
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.onglet {
            0 => self.accueil(ui),
            1 => self.tri.ui(ui),
            2 => self.lanceur.ui(ui),
            3 => self.coffre.ui(ui),
            4 => self.traducteur.ui(ui),
            5 => self.presse.ui(ui),
            6 => self.minecraft.ui(ui),
            7 => self.moniteur.ui(ui),
            8 => self.optimiseur.ui(ui),
            9 => self.renommer.ui(ui),
            10 => self.minuteur.ui(ui),
            11 => self.theme.ui(ui),
            12 => self.disque.ui(ui),
            13 => self.sauvegardes.ui(ui),
            14 => self.convertisseur.ui(ui),
            15 => self.notes.ui(ui),
            16 => self.reseau.ui(ui),
            17 => self.discord.ui(ui),
            18 => self.capture.ui(ui),
            19 => self.mixeur.ui(ui),
            20 => self.planif.ui(ui),
            21 => self.telephone.ui(ui),
            22 => self.dnd.ui(ui, &self.mixeur),
            24 => self.promos.ui(ui),
            25 => self.mods.ui(ui),
            26 => self.majlog.ui(ui),
            27 => self.sante.ui(ui),
            28 => self.coffre_f.ui(ui),
            29 => self.viseur.ui(ui),
            23 => {
                if self.demarrage.ui(ui) {
                    self.onglet = 8;
                }
            }
            _ => {}
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("Outils"),
        ..Default::default()
    };
    eframe::run_native("Outils", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
