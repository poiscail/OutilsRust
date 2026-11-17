#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Outils : tous les outils dans un seul programme.
mod coffre;
mod lanceur;
mod minecraft;
mod moniteur;
mod optimiseur;
mod pressepapiers;
mod theme;
mod traducteur;
mod tri;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Dossier "Outils" : le premier parent de l'exe qui contient OutilsRust.
fn racine() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors().find(|d| d.join("OutilsRust").is_dir()).map(Path::to_path_buf)
        .unwrap_or_else(|| exe.parent().map(Path::to_path_buf).unwrap_or_default())
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
    theme: theme::Theme,
    onglet: usize, // index dans la liste des noms du panneau
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx); // jaquettes des jeux
        let racine = racine();
        App { tri: tri::Tri::new(&racine), lanceur: lanceur::Lanceur::new(), coffre: coffre::CoffreUi::new(), traducteur: traducteur::Traducteur::new(), presse: pressepapiers::PressePapiers::new(), minecraft: minecraft::Minecraft::new(), moniteur: moniteur::Moniteur::new(&racine), optimiseur: optimiseur::Optimiseur::new(), theme: theme::Theme::new(), onglet: 0 }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.theme.peindre(ctx); // avant les panneaux : le fond passe dessous
        self.tri.relever();
        self.coffre.tic(ctx);
        self.minecraft.tic();
        self.moniteur.tic(ctx);
        if self.presse.tic(ctx) {
            self.onglet = 4;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::SidePanel::left("outils").exact_width(200.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Outils");
            ui.separator();
            let noms = ["Tri Téléchargements", "Lanceur Jeux", "Coffre", "Traducteur", "Presse-papiers", "Serveur Minecraft", "Moniteur Jeu", "Optimiseur", "Personnaliser"];
            for (i, n) in noms.iter().enumerate() {
                if ui.selectable_label(self.onglet == i, *n).clicked() {
                    self.onglet = i;
                }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.onglet {
            0 => self.tri.ui(ui),
            1 => self.lanceur.ui(ui),
            2 => self.coffre.ui(ui),
            3 => self.traducteur.ui(ui),
            4 => self.presse.ui(ui),
            5 => self.minecraft.ui(ui),
            6 => self.moniteur.ui(ui),
            7 => self.optimiseur.ui(ui),
            8 => self.theme.ui(ui),
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
