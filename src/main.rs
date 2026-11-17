#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Outils : tous les outils dans un seul programme. Les outils pas encore réécrits en Rust
//! ont un onglet qui lance leur ancien exe.
mod coffre;
mod lanceur;
mod traducteur;
mod tri;

use eframe::egui;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// (nom, exe de l'ancien outil relatif à la racine Outils)
const ANCIENS: &[(&str, &str)] = &[
    ("Presse-papiers", r"GestionnairePressePapiers\publish\GestionnairePressePapiers.exe"),
    ("Serveur Minecraft", r"GestionServeurMinecraft\GestionServeurMinecraft.exe"),
    ("Moniteur Jeu", r"MoniteurJeu\MoniteurJeu.exe"),
    ("Optimiseur", r"OptimiseurWindows\OptimiseurWindows.exe"),
];

/// Dossier "Outils" : le premier parent de l'exe qui contient OutilsRust.
fn racine() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.ancestors().find(|d| d.join("OutilsRust").is_dir()).map(Path::to_path_buf)
        .unwrap_or_else(|| exe.parent().map(Path::to_path_buf).unwrap_or_default())
}

struct App {
    racine: PathBuf,
    tri: tri::Tri,
    lanceur: lanceur::Lanceur,
    coffre: coffre::CoffreUi,
    traducteur: traducteur::Traducteur,
    onglet: usize, // 0 = Tri, 1 = Lanceur, 2 = Coffre, 3 = Traducteur, 4.. = ANCIENS[i-4]
    erreur: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        egui_extras::install_image_loaders(&cc.egui_ctx); // jaquettes des jeux
        let racine = racine();
        App { tri: tri::Tri::new(&racine), lanceur: lanceur::Lanceur::new(), coffre: coffre::CoffreUi::new(), traducteur: traducteur::Traducteur::new(), racine, onglet: 0, erreur: None }
    }

    fn ancien(&mut self, ui: &mut egui::Ui, nom: &str, rel: &str) {
        ui.heading(nom);
        ui.label("Pas encore réécrit en Rust.");
        ui.add_space(8.0);
        let exe = self.racine.join(rel);
        if !exe.is_file() {
            ui.colored_label(egui::Color32::GRAY, format!("Introuvable : {}", exe.display()));
            return;
        }
        if ui.button("Ouvrir l'ancienne version").clicked() {
            // "start" passe par le shell : gère la demande de droits admin (UAC)
            let r = Command::new("cmd").args(["/c", "start", ""]).arg(&exe)
                .current_dir(exe.parent().unwrap_or(&self.racine)).creation_flags(CREATE_NO_WINDOW).spawn();
            self.erreur = r.err().map(|e| e.to_string());
        }
        if let Some(e) = &self.erreur {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.tri.relever();
        self.coffre.tic(ctx);
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::SidePanel::left("outils").exact_width(200.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading("Outils");
            ui.separator();
            let mut noms = vec!["Tri Téléchargements", "Lanceur Jeux", "Coffre", "Traducteur"];
            noms.extend(ANCIENS.iter().map(|(n, _)| *n));
            for (i, n) in noms.iter().enumerate() {
                if ui.selectable_label(self.onglet == i, *n).clicked() {
                    self.onglet = i;
                    self.erreur = None;
                }
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.onglet {
            0 => self.tri.ui(ui),
            1 => self.lanceur.ui(ui),
            2 => self.coffre.ui(ui),
            3 => self.traducteur.ui(ui),
            i => {
                let (n, rel) = ANCIENS[i - 4];
                self.ancien(ui, n, rel);
            }
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
