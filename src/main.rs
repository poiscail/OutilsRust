#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Outils : tous les outils dans un seul programme.
mod bureau;
mod capture;
mod coeur;
mod coffre;
mod coffrefichiers;
mod creatif;
mod demarrage;
mod dependances;
mod discord;
mod convertisseur;
mod disque;
mod economiseur;
mod extras;
mod fichiers2;
mod outil;
mod pdf;
mod dnd;
mod hosts;
mod images;
mod jeux2;
mod jeux3;
mod lanceur;
mod listes;
mod ludique;
mod macros;
mod majlogiciels;
mod minecraft;
mod minijeux;
mod minuteur;
mod mcplus;
mod mcplus2;
mod fichiers3;
mod systeme3;
mod quotidien2;
mod micro;
mod creatif2;
mod interface;
mod interface2;
mod jeux5;
mod fichiers5;
mod systeme5;
mod creatif4;
mod minijeux4;
mod recherche;
mod pont;
mod jeux6;
mod recuperer;
mod jeux4;
mod mcplus3;
mod fichiers4;
mod systeme4;
mod reseau3;
mod quotidien3;
mod creatif3;
mod minijeux3;
mod minijeux2;
mod mixeur;
mod moniteur;
mod mods;
mod notes;
mod optimiseur;
mod planif;
mod pressepapiers;
mod promos;
mod quotidien;
mod renommer;
mod reseau;
mod reseau2;
mod santedisque;
mod sauvegardes;
mod son;
mod speedrun;
mod steam2;
mod systeme;
mod systeme2;
mod tests_materiel;
mod telephone;
mod theme;
mod traducteur;
mod video;
mod viseur;
mod voix;
mod tradinstant;
mod tri;

use eframe::egui;
use outil::Cat;
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
    "💽 Espace disque", "💾 Sauvegardes jeux", "🔁 Convertisseur", "📝 Notes", "📡 Réseau", "💬 Discord", "📸 Capture", "🎚 Mixeur audio", "🗓 Planificateur", "📱 Téléphone", "🌙 Ne pas déranger", "🔌 Démarrage PC", "🏷 Promos Steam", "🧩 Mods", "⬆ Mises à jour logiciels", "🩺 Santé des disques", "🗄 Coffre de fichiers", "🎯 Viseur", "⌨ Macros", "💭 Traduction instantanée", "🌌 Économiseur d'écran",
];
const PRESSE: usize = 5;
/// Catégorie de chaque onglet historique (None : l'Accueil, toujours en haut).
const CATS: [Option<Cat>; 33] = [
    None, Some(Cat::Fichiers), Some(Cat::Jeux), Some(Cat::Quotidien), Some(Cat::Quotidien), Some(Cat::Quotidien), Some(Cat::Minecraft),
    Some(Cat::Jeux), Some(Cat::Systeme), Some(Cat::Fichiers), Some(Cat::Quotidien), Some(Cat::Fun), Some(Cat::Fichiers), Some(Cat::Jeux),
    Some(Cat::Fichiers), Some(Cat::Quotidien), Some(Cat::Reseau), Some(Cat::Reseau), Some(Cat::Creatif), Some(Cat::Creatif), Some(Cat::Systeme),
    Some(Cat::Reseau), Some(Cat::Quotidien), Some(Cat::Systeme), Some(Cat::Jeux), Some(Cat::Jeux), Some(Cat::Systeme), Some(Cat::Systeme),
    Some(Cat::Fichiers), Some(Cat::Jeux), Some(Cat::Jeux), Some(Cat::Quotidien), Some(Cat::Fun),
];

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
    macros: macros::Macros,
    tradinstant: tradinstant::TradInstant,
    economiseur: economiseur::Economiseur,
    theme: theme::Theme,
    maj: systeme::MajPartagee,
    maj_ignoree: bool,
    disques: (Option<Instant>, Vec<(String, u64, u64)>),
    lance_au_demarrage: bool, // lancé par Windows : on démarre rangé
    onglet: usize,            // index dans ONGLETS, puis dans extras (à partir de ONGLETS.len())
    extras: Vec<Box<dyn outil::Outil>>,
    recherche: String,
    favoris: Vec<String>,
    palette: Option<(String, usize)>, // Ctrl+K : texte, ligne choisie
    plantages: usize,
    erreur_onglet: Option<(usize, String)>,
    dock: interface::Dock,
    notifs_ouvert: bool,
    visite: Option<usize>,
    mini: Option<egui::Vec2>, // mode mini : taille d'avant
    nouveautes: bool,
    tv: bool,
    manette: u16,
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx); // jaquettes des jeux
        systeme::nettoyer_ancienne();
        let racine = racine();
        let theme = theme::Theme::new(); // règle aussi notifications, zone et raccourci
        systeme::lancer(cc.egui_ctx.clone());
        coeur::raccourci_palette();
        systeme3::lancer_retardes();
        let maj = systeme::MajPartagee::default();
        if theme.r.maj {
            systeme::verifier(maj.clone());
        }
        let minecraft = minecraft::Minecraft::new();
        let sauvegardes = sauvegardes::Sauvegardes::new();
        let planif = planif::Planif::new(planif::Executeur { mc: minecraft.pilote(), jeux: sauvegardes.poignee() });
        let telephone = telephone::Telephone::new(minecraft.pilote());
        let moniteur = moniteur::Moniteur::new(&racine);
        let _ = telephone::MESURES.set(moniteur.lecteur());
        let extras = extras::tous(minecraft.pilote(), moniteur.lecteur());
        App {
            tri: tri::Tri::new(&racine), lanceur: lanceur::Lanceur::new(), coffre: coffre::CoffreUi::new(), traducteur: traducteur::Traducteur::new(),
            presse: pressepapiers::PressePapiers::new(), minecraft, moniteur,
            optimiseur: optimiseur::Optimiseur::new(), renommer: renommer::Renommer::new(), minuteur: minuteur::Minuteur::new(), disque: disque::Disque::new(), sauvegardes, convertisseur: convertisseur::Convertisseur::new(), notes: notes::Notes::new(), reseau: reseau::Reseau::new(), discord: discord::Discord::new(), capture: capture::Capture::new(), mixeur: mixeur::Mixeur::new(), planif, telephone, dnd: dnd::Dnd::new(), demarrage: demarrage::DemarragePc::new(), promos: promos::Promos::new(), mods: mods::Mods::new(), majlog: majlogiciels::MajLogiciels::new(),
            sante: santedisque::SanteDisque::new(), coffre_f: coffrefichiers::CoffreFichiers::new(), viseur: viseur::Viseur::new(), macros: macros::Macros::new(), tradinstant: tradinstant::TradInstant::new(),
            economiseur: economiseur::Economiseur::new(theme.dossier_fonds().to_path_buf()), theme, maj,
            maj_ignoree: false, disques: (None, vec![]), lance_au_demarrage: std::env::args().any(|a| a == "--demarrage"), onglet: 0, extras, favoris: outil::charger("onglets_favoris"), palette: None, plantages: coeur::nouveaux_plantages(), erreur_onglet: None, recherche: String::new(),
            dock: interface::Dock::new(), notifs_ouvert: false, visite: interface::visite_a_faire().then_some(0), mini: None, nouveautes: interface2::nouvelle_version(), tv: false, manette: 0,
        }
    }

    fn accueil(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Accueil");
            if ui.small_button("❓ Visite guidée").clicked() {
                self.visite = Some(0);
            }
        });
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
            for (k, o) in self.extras.iter().enumerate() {
                if let Some(r) = o.resume() {
                    if ui.button(o.nom()).clicked() {
                        self.onglet = ONGLETS.len() + k;
                    }
                    ui.label(r);
                    ui.end_row();
                }
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
        let tous = self.entrees();
        for c in Cat::TOUTES {
            ui.label(c.nom());
            ui.horizontal_wrapped(|ui| {
                for (i, n, _) in tous.iter().filter(|e| e.2 == Some(c)) {
                    if ui.add(egui::Button::new(n.as_str()).min_size(egui::vec2(170.0, 30.0))).clicked() {
                        self.onglet = *i;
                    }
                }
            });
        }
    }

    /// (index, nom, catégorie) de tous les onglets.
    fn entrees(&self) -> Vec<(usize, String, Option<Cat>)> {
        let mut v: Vec<(usize, String, Option<Cat>)> = ONGLETS.iter().enumerate().map(|(i, n)| (i, n.to_string(), CATS[i])).collect();
        v.extend(self.extras.iter().enumerate().map(|(k, o)| (ONGLETS.len() + k, o.nom().to_string(), Some(o.cat()))));
        v
    }
}

impl Drop for App {
    fn drop(&mut self) {
        systeme::fin();
    }
}

impl eframe::App for App {
    /// Mode grand écran : la manette devient clavier (croix = Tab, A = Entrée, B = Échap, stick droit = défilement).
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        use interface2::boutons::*;
        let Some((b, defil)) = interface2::lire_manette() else { return };
        let nouveaux = interface2::appuyes(self.manette, b);
        self.manette = b;
        let premier_plan = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if nouveaux & START != 0 && premier_plan {
            self.basculer_tv(ctx);
        }
        if !self.tv || !premier_plan {
            return;
        }
        let touche = |k: egui::Key, maj: bool| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers { shift: maj, ..Default::default() } };
        for (bouton, k, maj) in [(BAS, egui::Key::Tab, false), (HAUT, egui::Key::Tab, true), (GAUCHE, egui::Key::ArrowLeft, false), (DROITE, egui::Key::ArrowRight, false), (A, egui::Key::Enter, false), (B, egui::Key::Escape, false)] {
            if nouveaux & bouton != 0 {
                raw.events.push(touche(k, maj));
            }
        }
        if nouveaux & LB != 0 {
            self.onglet_voisin(-1);
        }
        if nouveaux & RB != 0 {
            self.onglet_voisin(1);
        }
        if nouveaux & BACK != 0 {
            self.onglet = 0;
        }
        if defil != 0.0 {
            raw.events.push(egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, defil * 18.0), modifiers: Default::default() });
        }
    }

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
        for o in &mut self.extras {
            o.tic(ctx);
        }
        let jeu = self.lanceur.jeu_actif();
        if let Ok(mut j) = outil::JEU_ACTIF.lock() {
            j.clone_from(&jeu);
        }
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
                    let notes = self.maj.lock().unwrap().notes.clone();
                    if !notes.trim().is_empty() {
                        ui.menu_button("Nouveautés", |ui| {
                            ui.set_max_width(420.0);
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| ui.label(notes));
                        });
                    }
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
        // dock : favoris à jour, et clic dans le dock → ouvrir l'outil
        {
            let mut f = self.dock.favoris.lock().unwrap();
            if *f != self.favoris {
                f.clone_from(&self.favoris);
            }
        }
        if interface2::NOMS.lock().unwrap().is_empty() {
            *interface2::NOMS.lock().unwrap() = self.entrees().into_iter().skip(1).map(|e| e.1).collect();
        }
        let demande = interface2::OUVRIR.lock().unwrap().take();
        if let Some(n) = demande {
            if let Some(e) = self.entrees().into_iter().find(|e| e.1 == n) {
                self.onglet = e.0;
            }
            if self.mini.is_some() {
                self.sortir_mini(ctx);
            }
        }
        if self.nouveautes {
            interface2::fenetre_nouveautes(ctx, &mut self.nouveautes);
        }
        if self.tv {
            if (ctx.zoom_factor() - 1.6).abs() > 0.01 {
                ctx.set_zoom_factor(1.6);
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let choix = self.dock.choix.lock().unwrap().take();
        if let Some(n) = choix {
            self.onglet = self.entrees().into_iter().find(|e| e.1 == n).map(|e| e.0).unwrap_or(0);
            if self.mini.is_some() {
                self.sortir_mini(ctx);
            }
        }
        self.dock.afficher(ctx);
        if let Some(mut e) = self.visite {
            self.visite = interface::visite(ctx, &mut e).then_some(e);
        }
        if self.notifs_ouvert {
            interface::fenetre_notifs(ctx, &mut self.notifs_ouvert);
        }
        if self.mini.is_some() {
            self.ui_mini(ctx);
            return;
        }
        egui::SidePanel::left("outils").exact_width(210.0).show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("Outils");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("📺").on_hover_text("Mode grand écran, pilotable à la manette (Start pour sortir)").clicked() {
                        self.basculer_tv(ctx);
                    }
                    if ui.small_button("🗗").on_hover_text("Mode mini (petite fenêtre toujours visible)").clicked() {
                        self.mini = Some(ctx.input(|i| i.viewport().inner_rect.map(|r| r.size())).unwrap_or(egui::vec2(1100.0, 720.0)));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(340.0, 520.0)));
                        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
                    }
                    let dock = self.dock.actif.load(Relaxed);
                    if ui.selectable_label(dock, "📌").on_hover_text("Dock flottant avec tes favoris").clicked() {
                        self.dock.basculer(!dock);
                    }
                    let n = interface::non_lues();
                    let cloche = if n > 0 { format!("🔔{n}") } else { "🔔".into() };
                    if ui.add(egui::Button::new(egui::RichText::new(cloche).color(if n > 0 { egui::Color32::from_rgb(255, 120, 170) } else { ui.visuals().text_color() })).small()).on_hover_text("Notifications").clicked() {
                        self.notifs_ouvert = !self.notifs_ouvert;
                    }
                });
            });
            ui.separator();
            ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔍 Chercher un outil").desired_width(f32::INFINITY));
            let tous = self.entrees();
            let actuelle = tous.iter().find(|e| e.0 == self.onglet).and_then(|e| e.2);
            egui::ScrollArea::vertical().id_salt("menu").max_height(ui.available_height() - 24.0).show(ui, |ui| {
                if ui.selectable_label(self.onglet == 0, ONGLETS[0]).clicked() {
                    self.onglet = 0;
                }
                let mut basculer_favori: Option<String> = None;
                for (i, n, _) in tous.iter().filter(|e| self.favoris.contains(&e.1)) {
                    let r = ui.selectable_label(self.onglet == *i, format!("⭐ {n}"));
                    if r.clicked() {
                        self.onglet = *i;
                    }
                    r.context_menu(|ui| {
                        if ui.button("Retirer des favoris").clicked() {
                            basculer_favori = Some(n.clone());
                            ui.close_menu();
                        }
                    });
                }
                if !self.favoris.is_empty() {
                    ui.separator();
                }
                let q = self.recherche.trim().to_lowercase();
                if !q.is_empty() {
                    for (i, n, _) in tous.iter().filter(|e| e.0 != 0 && e.1.to_lowercase().contains(&q)) {
                        if ui.selectable_label(self.onglet == *i, n.as_str()).clicked() {
                            self.onglet = *i;
                        }
                    }
                    return;
                }
                for c in Cat::TOUTES {
                    let id = ui.make_persistent_id(("cat", c));
                    let etat = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, actuelle == Some(c));
                    etat.show_header(ui, |ui| ui.strong(c.nom())).body(|ui| {
                        for (i, n, _) in tous.iter().filter(|e| e.2 == Some(c)) {
                            let r = ui.selectable_label(self.onglet == *i, n.as_str()).on_hover_text("Clic droit : favori");
                            if r.clicked() {
                                self.onglet = *i;
                            }
                            r.context_menu(|ui| {
                                if ui.button(if self.favoris.contains(n) { "Retirer des favoris" } else { "⭐ Ajouter aux favoris" }).clicked() {
                                    basculer_favori = Some(n.clone());
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                }
                if let Some(n) = basculer_favori {
                    if let Some(k) = self.favoris.iter().position(|f| *f == n) {
                        self.favoris.remove(k);
                    } else {
                        self.favoris.push(n);
                    }
                    outil::enregistrer("onglets_favoris", &self.favoris);
                }
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| ui.weak(concat!("v", env!("CARGO_PKG_VERSION"))));
        });
        let nom_onglet = self.entrees().into_iter().find(|e| e.0 == self.onglet).map(|e| e.1).unwrap_or_default();
        if let Ok(mut o) = coeur::ONGLET.lock() {
            o.clone_from(&nom_onglet);
        }
        interface::compter(&nom_onglet, ctx.input(|i| i.viewport().focused.unwrap_or(true)));
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.plantages > 0 {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 80), format!("💥 Outils a rencontré {} plantage(s) : un rapport est prêt.", self.plantages));
                    if ui.button("Voir").clicked() {
                        if let Some(k) = self.extras.iter().position(|o| o.nom().starts_with("📜")) {
                            self.onglet = ONGLETS.len() + k;
                        }
                        self.plantages = 0;
                    }
                    if ui.small_button("✖").clicked() {
                        coeur::marquer_vus();
                        self.plantages = 0;
                    }
                });
                ui.separator();
            }
            if let Some((o, m)) = self.erreur_onglet.clone().filter(|e| e.0 == self.onglet) {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("💥 Cet onglet a planté : {m}"));
                ui.weak("Le reste d'Outils marche toujours. Le rapport est dans Système › Journal et plantages : envoie-le-moi.");
                if ui.button("Réessayer").clicked() {
                    self.erreur_onglet = None;
                }
                let _ = o;
                return;
            }
            let sensible = [3, 5].contains(&self.onglet) || ["Wi-Fi", "Coffre de fichiers", "Mots de passe"].iter().any(|x| nom_onglet.contains(x));
            if sensible && outil::STREAM.load(Relaxed) {
                ui.heading("🎥 Masqué pendant le stream");
                ui.weak("Le mode stream est actif (Jeux › Mode stream) : cet onglet contient des informations personnelles.");
                return;
            }
            let onglet = self.onglet;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match onglet {
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
            30 => self.macros.ui(ui),
            31 => self.tradinstant.ui(ui),
            32 => self.economiseur.ui(ui),
            23 => {
                if self.demarrage.ui(ui) {
                    self.onglet = 8;
                }
            }
            i => {
                if let Some(o) = self.extras.get_mut(i.wrapping_sub(ONGLETS.len())) {
                    o.ui(ui);
                }
            }
            }));
            if let Err(e) = r {
                let m = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "erreur inconnue".into());
                self.erreur_onglet = Some((onglet, m));
                self.plantages = coeur::nouveaux_plantages();
            }
        });
        self.palette_ui(ctx);
    }
}

fn main() -> eframe::Result {
    coeur::activer_portable();
    coeur::installer_crochet();
    // lancé par Windows comme économiseur d'écran (Outils.scr) : /s plein écran, /p aperçu (ignoré)
    let arg = std::env::args().nth(1).map(|a| a.to_ascii_lowercase()).unwrap_or_default();
    let est_scr = std::env::current_exe().ok().and_then(|e| e.extension().map(|x| x.eq_ignore_ascii_case("scr"))).unwrap_or(false);
    if arg.starts_with("/p") {
        return Ok(());
    }
    if arg.starts_with("/s") || est_scr {
        return economiseur::lancer(); // la copie .scr ne lance jamais le hub
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]).with_title("Outils"),
        ..Default::default()
    };
    eframe::run_native("Outils", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[derive(Clone, Copy)]
enum Commande {
    Onglet(usize),
    Capture(i32),
    Veille,
    Verrouiller,
    ServeurDemarrer,
    ServeurArreter,
}

impl App {
    fn basculer_tv(&mut self, ctx: &egui::Context) {
        self.tv = !self.tv;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.tv));
        if !self.tv {
            self.theme.reappliquer();
        }
    }

    /// Outil précédent / suivant dans l'ordre du menu (bouton LB / RB).
    fn onglet_voisin(&mut self, sens: i32) {
        let v: Vec<usize> = self.entrees().into_iter().map(|e| e.0).collect();
        if let Some(k) = v.iter().position(|x| *x == self.onglet) {
            self.onglet = v[((k as i32 + sens).rem_euclid(v.len() as i32)) as usize];
        }
    }

    fn sortir_mini(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.mini.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(t));
        }
    }

    /// Mode mini : favoris et résumés dans une petite fenêtre toujours devant.
    fn ui_mini(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Outils");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("⤢ Agrandir").clicked() {
                        self.sortir_mini(ctx);
                    }
                    let n = interface::non_lues();
                    if ui.small_button(if n > 0 { format!("🔔{n}") } else { "🔔".into() }).clicked() {
                        self.notifs_ouvert = !self.notifs_ouvert;
                    }
                });
            });
            ui.separator();
            let tous = self.entrees();
            let mut ouvrir = None;
            ui.horizontal_wrapped(|ui| {
                for (i, n, _) in tous.iter().filter(|e| self.favoris.contains(&e.1)) {
                    if ui.add(egui::Button::new(egui::RichText::new(interface::icone(n)).size(20.0)).min_size(egui::vec2(40.0, 38.0))).on_hover_text(n.as_str()).clicked() {
                        ouvrir = Some(*i);
                    }
                }
            });
            if self.favoris.is_empty() {
                ui.weak("Ajoute des favoris (clic droit sur un outil du menu) pour les avoir ici.");
            }
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut lignes: Vec<(usize, String)> = vec![(6, self.minecraft.resume()), (10, self.minuteur.resume()), (20, self.planif.resume())];
                lignes.retain(|l| !l.1.is_empty());
                for (k, o) in self.extras.iter().enumerate() {
                    if let Some(r) = o.resume() {
                        lignes.push((ONGLETS.len() + k, r));
                    }
                }
                for (i, t) in lignes {
                    if ui.add(egui::Label::new(t).wrap().sense(egui::Sense::click())).on_hover_text("Ouvrir").clicked() {
                        ouvrir = Some(i);
                    }
                    ui.add_space(2.0);
                }
            });
            if let Some(i) = ouvrir {
                self.onglet = i;
                self.sortir_mini(ctx);
            }
        });
    }

    /// Ctrl+K : chercher un outil ou une action et la lancer au clavier.
    fn palette_ui(&mut self, ctx: &egui::Context) {
        let ouvrir = ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::K)))
            || coeur::PALETTE.swap(false, Relaxed);
        if ouvrir {
            self.palette = Some((String::new(), 0));
        }
        let Some((mut q, mut sel)) = self.palette.clone() else { return };
        let mut lignes: Vec<(String, Commande)> = self.entrees().into_iter().map(|(i, n, _)| (n, Commande::Onglet(i))).collect();
        lignes.extend([
            ("✂ Capture d'une zone".to_string(), Commande::Capture(1)),
            ("🖥 Capture de l'écran entier".to_string(), Commande::Capture(3)),
            ("💧 Pipette".to_string(), Commande::Capture(4)),
            ("📏 Règle à l'écran".to_string(), Commande::Capture(5)),
            ("🌙 Mettre le PC en veille".to_string(), Commande::Veille),
            ("🔒 Verrouiller le PC".to_string(), Commande::Verrouiller),
            ("⛏ Démarrer le serveur Minecraft".to_string(), Commande::ServeurDemarrer),
            ("⛏ Arrêter le serveur Minecraft".to_string(), Commande::ServeurArreter),
        ]);
        let mut v: Vec<(usize, &(String, Commande))> = lignes.iter().filter_map(|l| coeur::correspond(&l.0, &q).map(|s| (s, l))).collect();
        v.sort_by_key(|x| x.0);
        let v: Vec<&(String, Commande)> = v.into_iter().map(|x| x.1).take(12).collect();
        let mut choisie: Option<Commande> = None;
        let mut fermer = false;
        egui::Window::new("palette").title_bar(false).resizable(false).fixed_size([460.0, 0.0]).anchor(egui::Align2::CENTER_TOP, [0.0, 80.0]).show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Chercher un outil ou une action…").desired_width(f32::INFINITY).font(egui::TextStyle::Heading));
            r.request_focus();
            if r.changed() {
                sel = 0;
            }
            ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) {
                    sel = (sel + 1).min(v.len().saturating_sub(1));
                }
                if i.key_pressed(egui::Key::ArrowUp) {
                    sel = sel.saturating_sub(1);
                }
                if i.key_pressed(egui::Key::Escape) {
                    fermer = true;
                }
                if i.key_pressed(egui::Key::Enter) {
                    choisie = v.get(sel).map(|l| l.1);
                }
            });
            for (k, (n, c)) in v.iter().enumerate() {
                if ui.selectable_label(k == sel, n).clicked() {
                    choisie = Some(*c);
                }
            }
            ui.weak("↑ ↓ pour choisir · Entrée pour lancer · Échap pour fermer · Ctrl+Alt+K de partout");
        });
        if let Some(c) = choisie {
            fermer = true;
            match c {
                Commande::Onglet(i) => self.onglet = i,
                Commande::Capture(k) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    self.capture.declencher(k);
                }
                Commande::Veille => {
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_secs(1));
                        unsafe { windows_sys::Win32::System::Power::SetSuspendState(0, 0, 0) };
                    });
                }
                Commande::Verrouiller => unsafe {
                    windows_sys::Win32::System::Shutdown::LockWorkStation();
                },
                Commande::ServeurDemarrer => {
                    let p = self.minecraft.pilote();
                    std::thread::spawn(move || {
                        let _ = p.demarrer();
                    });
                }
                Commande::ServeurArreter => {
                    let p = self.minecraft.pilote();
                    std::thread::spawn(move || p.arreter());
                }
            }
        }
        self.palette = if fermer { None } else { Some((q, sel)) };
    }
}
