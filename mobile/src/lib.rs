#![allow(dead_code, unused_imports)]
//! Outils sur téléphone Android.
//!
//! Les jeux et outils du quotidien sont **les mêmes fichiers que sur PC** (inclus avec `#[path]`) :
//! une correction ou un nouveau jeu profite aux deux versions. Ce qui est propre à Windows est mis de
//! côté avec `#[cfg(windows)]`, et quelques petits modules (`outil`, `creatif`, `systeme`…) remplacent
//! ceux du PC.
//!
//! Adaptations au tactile :
//! * accueil en grandes tuiles, un outil à la fois en plein écran, bouton ← (et touche Retour) ;
//! * manette tactile (flèches, Espace, Entrée) et pavé de chiffres pour les jeux pensés pour le clavier :
//!   les appuis sont transformés en vraies touches pour le jeu ;
//! * clavier à l'écran fiable pour tous les champs de texte (⌨ en haut) ;
//! * boutons et textes plus grands, défilement au doigt.

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

mod android;
mod appli;
mod batterie_tel;
mod photos_tel;
mod capteurs_tel;
mod commun;
mod creatif;
mod fichiers_tel;
mod jeux_tel;
mod monpc;
mod reseau_tel;
mod wifi_tel;
mod minecraft;
mod outil;
mod systeme;
mod telephone;

use eframe::egui::{self, Color32, Key, RichText, Vec2};
use outil::{charger, enregistrer, Outil};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const ROSE: Color32 = Color32::from_rgb(255, 95, 162);

/// Commandes tactiles dont un outil a besoin.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pave {
    Aucun,
    /// flèches + Espace + Entrée (+ ZQSD pour le 2e joueur)
    Manette,
    /// chiffres 1 à 9 et effacer
    Chiffres,
}

/// Ajoute les polices de symboles d'Android (flèches, cartes, pièces d'échecs, idéogrammes…)
/// derrière celles d'egui, pour que les icônes des outils s'affichent.
fn polices_du_telephone(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();
    for n in ["NotoSansSymbols-Regular-Subsetted.ttf", "NotoSansSymbols-Regular-Subsetted2.ttf", "NotoSansSymbols2-Regular.ttf", "NotoSansMath-Regular.ttf", "DroidSansFallback.ttf", "NotoSansCJK-Regular.ttc"] {
        if let Ok(o) = std::fs::read(format!("/system/fonts/{n}")) {
            f.font_data.insert(n.into(), std::sync::Arc::new(egui::FontData::from_owned(o)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                f.families.entry(fam).or_default().push(n.into());
            }
        }
    }
    ctx.set_fonts(f);
}

/// Le texte s'il est affichable, sinon le remplaçant.
fn ou(ui: &egui::Ui, texte: &'static str, remplacant: &'static str) -> &'static str {
    if ui.fonts(|f| f.has_glyphs(&egui::FontId::proportional(20.0), texte)) { texte } else { remplacant }
}

pub struct Fiche {
    pub outil: Box<dyn Outil>,
    pub rubrique: &'static str,
    pub pave: Pave,
    pub aide: &'static str,
}

pub const RUBRIQUES: [(&str, &str); 5] = [("🖥", "PC et réseau"), ("📱", "Téléphone"), ("🎮", "Jeux"), ("♟", "Réflexion"), ("📆", "Au quotidien")];

pub fn catalogue() -> Vec<Fiche> {
    let f = |o: Box<dyn Outil>, rubrique, pave, aide| Fiche { outil: o, rubrique, pave, aide };
    use Pave::*;
    vec![
        // PC et réseau
        f(Box::new(monpc::MonPc::new()), "PC et réseau", Aucun, "Télécommande, état, jeux, fichiers, presse-papiers et Minecraft de ton PC (Outils ouvert sur le PC, même Wi-Fi)."),
        f(Box::new(wifi_tel::Wifi::new()), "PC et réseau", Aucun, "Signal, appareils connectés, débit et ping."),
        f(Box::new(reseau_tel::MaBox::new()), "PC et réseau", Aucun, "Optimise ton Wi-Fi : diagnostic noté, meilleur canal, signal pièce par pièce, DNS, intrus, partage par QR code."),
        f(Box::new(reseau_tel::Mobile::new()), "PC et réseau", Aucun, "Signal 4G/5G, données par appli et forfait, Wi-Fi ou 4G, économies de données, coupures."),
        // Téléphone
        f(Box::new(fichiers_tel::FichiersTel::new()), "Téléphone", Aucun, "Espace, doublons, rangement, renommage, fichiers et applis louches. Rien n'est effacé sans passer par la corbeille d'Outils."),
        f(Box::new(batterie_tel::BatterieOutil::new()), "Téléphone", Aucun, "État de la batterie, autonomie, alerte de charge à 80 % et de surchauffe."),
        f(Box::new(photos_tel::Nettoyeur::new()), "Téléphone", Aucun, "Photos floues, rafales et vieilles captures d'écran, à trier en un geste."),
        f(Box::new(photos_tel::Coffre::new()), "Téléphone", Aucun, "Photos cachées et chiffrées, protégées par un code."),
        f(Box::new(photos_tel::Compresseur::new()), "Téléphone", Aucun, "Réduire une vidéo pour WhatsApp ou Discord."),
        f(Box::new(capteurs_tel::Lampe::new()), "Téléphone", Aucun, "Lampe, stroboscope et SOS."),
        f(Box::new(capteurs_tel::ScannerQr::new()), "Téléphone", Aucun, "Lit les QR codes ; celui d'Outils sur PC associe le téléphone."),
        f(Box::new(capteurs_tel::Boussole::new()), "Téléphone", Aucun, "Boussole et niveau à bulle."),
        f(Box::new(capteurs_tel::Sonometre::new()), "Téléphone", Aucun, "Niveau de bruit autour de toi."),
        f(Box::new(capteurs_tel::Sport::new()), "Téléphone", Aucun, "Podomètre, chrono à tours et fractionné."),
        // Jeux
        f(Box::new(jeux_tel::Labyrinthe::new()), "Jeux", Aucun, "Incline le téléphone pour guider la bille."),
        f(Box::new(jeux_tel::FaceAFace::new()), "Jeux", Aucun, "À deux sur le même écran : réflexes et air hockey."),
        f(Box::new(jeux_tel::Badges::new()), "Jeux", Aucun, "Les succès gagnés dans tous les jeux."),
        f(Box::new(minijeux::MiniJeux::new()), "Jeux", Manette, "12 jeux : serpent, casse-briques, démineur, blocs qui tombent… Utilise la manette en bas."),
        f(Box::new(minijeux4::Cartes::new()), "Jeux", Aucun, "Huit américain et bataille contre le téléphone."),
        f(Box::new(minijeux3::Solitaire::new()), "Jeux", Aucun, "Touche une carte puis l'endroit où la poser."),
        f(Box::new(minijeux4::Navale::new()), "Jeux", Aucun, "Contre le téléphone, ou contre un PC / un autre téléphone sur le même Wi-Fi."),
        f(Box::new(minijeux4::SnakeDeux::new()), "Jeux", Manette, "En réseau avec un PC ou un téléphone : la manette en bas dirige ton serpent."),
        f(Box::new(minijeux3::ADeux::new()), "Jeux", Aucun, "Morpion et 4 en ligne à deux sur le même téléphone : passez-vous-le !"),
        f(Box::new(minijeux3::Reseau::new()), "Jeux", Aucun, "4 en ligne et morpion contre un PC ou un téléphone du même Wi-Fi."),
        f(Box::new(minijeux3::Clicker::new()), "Jeux", Aucun, "Tape sur le dango !"),
        f(Box::new(ludique::Reflexes::new()), "Jeux", Aucun, "Touche l'écran dès que la couleur change."),
        f(Box::new(minijeux4::CourseFrappe::new()), "Jeux", Aucun, "Course de frappe contre un PC ou un téléphone du même Wi-Fi (clavier ⌨ en haut)."),
        // Réflexion
        f(Box::new(minijeux4::SudokuJour::new()), "Réflexion", Chiffres, "La même grille pour tout le monde chaque jour. Touche une case puis un chiffre."),
        f(Box::new(minijeux3::Echecs::new()), "Réflexion", Aucun, "Touche une pièce puis sa case d'arrivée."),
        f(Box::new(minijeux4::Dames::new()), "Réflexion", Aucun, "Dames 10×10, règles internationales."),
        f(Box::new(minijeux4::QuizDrapeaux::new()), "Réflexion", Aucun, "100 pays : drapeaux et capitales."),
        f(Box::new(minijeux3::MotsMeles::new()), "Réflexion", Aucun, "Glisse le doigt sur les lettres d'un mot."),
        f(Box::new(minijeux3::Frappe::new()), "Réflexion", Aucun, "Vitesse de frappe (clavier ⌨ en haut)."),
        // Au quotidien
        f(Box::new(quotidien2::Meteo::new()), "Au quotidien", Aucun, "Prévisions heure par heure et sur 7 jours."),
        f(Box::new(quotidien2::Agenda::new()), "Au quotidien", Aucun, "Événements et rappels (affichés quand l'appli est ouverte)."),
        f(Box::new(quotidien3::Ecole::new()), "Au quotidien", Aucun, "Emploi du temps et devoirs."),
        f(Box::new(quotidien3::ComptesRebours::new()), "Au quotidien", Aucun, "Vacances, anniversaires, sorties de jeux…"),
        f(Box::new(quotidien3::Calculatrice::new()), "Au quotidien", Aucun, "Calculs, unités et devises."),
        f(Box::new(quotidien2::EauCalories::new()), "Au quotidien", Aucun, "Verres d'eau et calories de la journée."),
        f(Box::new(quotidien3::Actus::new()), "Au quotidien", Aucun, "Actus de tes sites préférés (RSS)."),
        f(Box::new(ludique::Des::new()), "Au quotidien", Aucun, "Dés, pile ou face, tirage au sort."),
        f(Box::new(ludique::MotsDePasse::new()), "Au quotidien", Aucun, "Mots de passe solides."),
    ]
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Reglages {
    favoris: Vec<String>,
    recents: Vec<String>,
    taille: f32,
    clair: bool,
    /// 0 = comme Android, 1 = sombre, 2 = clair
    theme: u8,
    veille: bool,
    maj_verifiee: u64,
}

/// Touches à rejouer au jeu à l'image suivante.
#[derive(Default)]
struct Injection {
    touches: Vec<Key>,
    texte: String,
}

impl Injection {
    fn appliquer(&mut self, ctx: &egui::Context) {
        if self.touches.is_empty() && self.texte.is_empty() {
            return;
        }
        let touches = std::mem::take(&mut self.touches);
        let texte = std::mem::take(&mut self.texte);
        ctx.input_mut(|i| {
            if !texte.is_empty() {
                i.events.push(egui::Event::Text(texte));
            }
            for k in touches {
                let m = egui::Modifiers::NONE;
                i.events.push(egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
                i.events.push(egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
            }
        });
    }
}

pub struct App {
    fiches: Vec<Fiche>,
    ouvert: Option<usize>,
    reglages: Reglages,
    recherche: String,
    clavier: bool,
    majuscules: bool,
    injection: Injection,
    toast: Option<(String, Instant)>,
    joueur2: bool,
    parametres: bool,
    e: commun::Etat,
    theme_verifie: Instant,
    sombre: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        polices_du_telephone(&cc.egui_ctx);
        let mut reglages: Reglages = charger("telephone");
        if reglages.taille == 0.0 {
            reglages.taille = 1.0;
        }
        // essai sur PC : OUTILS_OUVRIR=numéro ouvre directement un outil
        let ouvert = std::env::var("OUTILS_OUVRIR").ok().and_then(|x| x.parse().ok());
        if android::version_android() >= 33 && !android::a_permission("android.permission.POST_NOTIFICATIONS") {
            android::demander(&["android.permission.POST_NOTIFICATIONS"]);
        }
        if reglages.theme == 0 && reglages.clair {
            reglages.theme = 2; // ancien réglage
        }
        if reglages.veille {
            android::veille(true);
        }
        if commun::maintenant() > reglages.maj_verifiee + 86_400 {
            reglages.maj_verifiee = commun::maintenant();
            enregistrer("telephone", &reglages);
            std::thread::spawn(|| {
                let _ = appli::verifier();
            });
        }
        let a = App { fiches: catalogue(), ouvert, reglages, recherche: String::new(), clavier: false, majuscules: false, injection: Injection::default(), toast: None, joueur2: false, parametres: false, e: Default::default(), theme_verifie: Instant::now(), sombre: true };
        a.style(&cc.egui_ctx);
        a
    }

    fn style(&self, ctx: &egui::Context) {
        let sombre = match self.reglages.theme {
            1 => true,
            2 => false,
            _ => android::mode_sombre(),
        };
        ctx.set_visuals(if sombre { egui::Visuals::dark() } else { egui::Visuals::light() });
        ctx.style_mut(|s| {
            let t = self.reglages.taille;
            for (st, taille) in [(egui::TextStyle::Body, 17.0), (egui::TextStyle::Button, 17.0), (egui::TextStyle::Heading, 24.0), (egui::TextStyle::Small, 13.0), (egui::TextStyle::Monospace, 15.0)] {
                s.text_styles.insert(st.clone(), egui::FontId::new(taille * t, if st == egui::TextStyle::Monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional }));
            }
            // cibles assez grandes pour un doigt
            s.spacing.button_padding = Vec2::new(14.0, 9.0);
            s.spacing.interact_size = Vec2::new(44.0, 40.0);
            s.spacing.item_spacing = Vec2::new(10.0, 9.0);
            s.spacing.scroll.bar_width = 6.0;
            s.spacing.slider_width = 200.0;
            s.visuals.selection.bg_fill = ROSE.gamma_multiply(0.6);
            s.visuals.hyperlink_color = ROSE;
        });
    }

    fn ouvrir(&mut self, k: usize) {
        self.ouvert = Some(k);
        let nom = self.fiches[k].outil.nom().to_string();
        self.reglages.recents.retain(|n| *n != nom);
        self.reglages.recents.insert(0, nom);
        self.reglages.recents.truncate(6);
        enregistrer("telephone", &self.reglages);
    }

    fn tuile(&mut self, ui: &mut egui::Ui, k: usize, largeur: f32) {
        let nom = self.fiches[k].outil.nom();
        let (icone, titre) = nom.split_once(' ').unwrap_or(("•", nom));
        let icone = if ui.fonts(|f| f.has_glyphs(&egui::FontId::proportional(32.0), icone)) { icone } else { "✦" };
        let fav = self.reglages.favoris.iter().any(|f| f == nom);
        let (r, rep) = ui.allocate_exact_size(Vec2::new(largeur, 92.0), egui::Sense::click());
        let fond = if rep.is_pointer_button_down_on() { ROSE.gamma_multiply(0.35) } else { ui.visuals().faint_bg_color };
        ui.painter().rect_filled(r, 14.0, fond);
        ui.painter().rect_stroke(r, 14.0, egui::Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
        ui.painter().text(r.center_top() + Vec2::new(0.0, 30.0), egui::Align2::CENTER_CENTER, icone, egui::FontId::proportional(32.0), ui.visuals().strong_text_color());
        let galerie = ui.painter().layout(titre.to_string(), egui::FontId::proportional(14.5), ui.visuals().text_color(), largeur - 12.0);
        ui.painter().galley(egui::pos2(r.center().x - galerie.size().x / 2.0, r.top() + 56.0), galerie, ui.visuals().text_color());
        if fav {
            ui.painter().text(r.right_top() + Vec2::new(-12.0, 12.0), egui::Align2::CENTER_CENTER, "★", egui::FontId::proportional(14.0), Color32::from_rgb(250, 210, 90));
        }
        if rep.clicked() {
            self.ouvrir(k);
        }
        if rep.long_touched() || rep.secondary_clicked() {
            let n = nom.to_string();
            if fav {
                self.reglages.favoris.retain(|f| *f != n);
                self.toast = Some(("Retiré des favoris".into(), Instant::now()));
            } else {
                self.reglages.favoris.push(n);
                self.toast = Some(("★ Ajouté aux favoris".into(), Instant::now()));
            }
            enregistrer("telephone", &self.reglages);
        }
    }

    fn grille(&mut self, ui: &mut egui::Ui, liste: &[usize]) {
        let colonnes = ((ui.available_width() / 150.0).floor() as usize).clamp(2, 6);
        let esp = 10.0;
        let largeur = (ui.available_width() - esp * (colonnes as f32 - 1.0)) / colonnes as f32;
        for ligne in liste.chunks(colonnes) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = esp;
                for &k in ligne {
                    self.tuile(ui, k, largeur);
                }
            });
        }
    }

    fn accueil(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Outils").size(30.0).strong().color(ROSE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("⚙").size(20.0)).clicked() {
                    self.parametres = true;
                }
                if ui.button("A+").clicked() {
                    self.reglages.taille = (self.reglages.taille + 0.1).min(1.6);
                    enregistrer("telephone", &self.reglages);
                    self.style(ui.ctx());
                }
                if ui.button("A−").clicked() {
                    self.reglages.taille = (self.reglages.taille - 0.1).max(0.8);
                    enregistrer("telephone", &self.reglages);
                    self.style(ui.ctx());
                }
            });
        });
        if let Some((v, _)) = appli::MAJ.lock().unwrap().clone() {
            if ui.add(egui::Button::new(RichText::new(format!("⬆ Nouvelle version {v} disponible : touche pour l'installer")).strong()).fill(ROSE.gamma_multiply(0.5)).min_size(Vec2::new(ui.available_width(), 40.0))).clicked() {
                self.parametres = true;
            }
        }
        let r = ui.add(egui::TextEdit::singleline(&mut self.recherche).hint_text("🔎 Chercher un jeu ou un outil").desired_width(f32::INFINITY).margin(Vec2::new(10.0, 8.0)));
        if r.gained_focus() {
            self.clavier = true;
        }
        let q = self.recherche.trim().to_lowercase();
        if !q.is_empty() {
            let liste: Vec<usize> = (0..self.fiches.len()).filter(|k| self.fiches[*k].outil.nom().to_lowercase().contains(&q) || self.fiches[*k].aide.to_lowercase().contains(&q)).collect();
            if liste.is_empty() {
                ui.weak("Rien trouvé.");
            }
            self.grille(ui, &liste);
            return;
        }
        let trouver = |noms: &[String], fiches: &[Fiche]| -> Vec<usize> { noms.iter().filter_map(|n| fiches.iter().position(|f| f.outil.nom() == n)).collect() };
        let fav = trouver(&self.reglages.favoris, &self.fiches);
        if !fav.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("★ Favoris").size(19.0).strong());
            self.grille(ui, &fav);
        }
        let rec: Vec<usize> = trouver(&self.reglages.recents, &self.fiches).into_iter().filter(|k| !fav.contains(k)).take(4).collect();
        if !rec.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new("🕘 Récents").size(19.0).strong());
            self.grille(ui, &rec);
        }
        for (icone, nom) in RUBRIQUES {
            let liste: Vec<usize> = (0..self.fiches.len()).filter(|k| self.fiches[*k].rubrique == nom).collect();
            ui.add_space(8.0);
            ui.label(RichText::new(format!("{icone} {nom}")).size(19.0).strong());
            self.grille(ui, &liste);
        }
        ui.add_space(10.0);
        ui.weak("Appui long sur une tuile : l'ajouter aux favoris. Les jeux en réseau marchent avec Outils sur PC, sur le même Wi-Fi.");
    }

    /// Affiche l'outil ouvert. Les jeux pensés pour un écran de PC (plateaux de 600 px…) dépassent :
    /// on baisse alors le zoom de l'appli juste assez pour qu'ils tiennent en largeur (egui recalcule
    /// tout, appuis compris), et on revient au zoom normal en quittant l'outil.
    fn outil_ajuste(&mut self, ui: &mut egui::Ui, k: usize) {
        let dispo = ui.available_width();
        let mut deborde = 0.0f32;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.weak(self.fiches[k].aide);
            ui.separator();
            let debut = ui.min_rect().left();
            self.fiches[k].outil.ui(ui);
            ui.add_space(30.0);
            deborde = ui.min_rect().right() - debut;
        });
        if deborde > dispo + 12.0 {
            let z = (ui.ctx().zoom_factor() * dispo / deborde).max(0.5);
            if z < ui.ctx().zoom_factor() - 0.01 {
                ui.ctx().set_zoom_factor(z);
            }
        }
    }

    /// Paramètres : apparence, veille, mises à jour, sauvegarde.
    fn parametres_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new(ou(ui, "⬅", "<")).size(24.0)).min_size(Vec2::new(52.0, 44.0))).clicked() {
                self.parametres = false;
            }
            ui.heading("Paramètres");
        });
        commun::etat_ui(ui, &self.e);
        let mut change = false;
        ui.label(RichText::new("🎨 Apparence").size(19.0).strong());
        ui.horizontal(|ui| {
            for (k, n) in ["Comme le téléphone", "Sombre", "Clair"].iter().enumerate() {
                change |= ui.selectable_value(&mut self.reglages.theme, k as u8, *n).changed();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Taille du texte");
            if ui.button("A−").clicked() {
                self.reglages.taille = (self.reglages.taille - 0.1).max(0.8);
                change = true;
            }
            if ui.button("A+").clicked() {
                self.reglages.taille = (self.reglages.taille + 0.1).min(1.6);
                change = true;
            }
        });
        ui.separator();
        ui.label(RichText::new("🔔 Rester actif quand l'appli est fermée").size(19.0).strong());
        if ui.checkbox(&mut self.reglages.veille, "Garder Outils en veille (rappels, coupures, intrus, forfait)").changed() {
            android::veille(self.reglages.veille);
            change = true;
        }
        ui.weak("Une petite notification permanente l'indique : c'est obligatoire sur Android.");
        if self.reglages.veille && !android::sans_optimisation_batterie() && ui.button("🔋 Ne pas couper Outils pour économiser la batterie").clicked() {
            android::ouvrir_reglage("android.settings.IGNORE_BATTERY_OPTIMIZATION_SETTINGS", false);
            self.e.lock().unwrap().1 = "Dans la liste : Outils › « Ne pas optimiser ».".into();
        }
        ui.separator();
        ui.label(RichText::new("⬆ Mises à jour").size(19.0).strong());
        ui.label(format!("Version installée : {}", appli::VERSION));
        let maj = appli::MAJ.lock().unwrap().clone();
        match maj {
            Some((v, url)) => {
                ui.label(RichText::new(format!("Nouvelle version {v} disponible !")).strong().color(commun::VERT));
                if !android::peut_installer() {
                    ui.weak("Android doit d'abord autoriser Outils à installer ses mises à jour.");
                    if ui.button("Autoriser").clicked() {
                        android::ouvrir_reglage("android.settings.MANAGE_UNKNOWN_APP_SOURCES", true);
                    }
                } else if commun::gros_bouton(ui, "⬇ Télécharger et installer") {
                    commun::en_fond(&self.e, "Téléchargement…", move || appli::installer(&url).map(|_| "Confirme l'installation dans la fenêtre d'Android.".into()));
                }
            }
            None => {
                if ui.button("Vérifier maintenant").clicked() {
                    commun::en_fond(&self.e, "Vérification…", || Ok(match appli::verifier()? {
                        Some((v, _)) => format!("Version {v} disponible"),
                        None => "✔ Tu as la dernière version.".into(),
                    }));
                }
            }
        }
        ui.separator();
        ui.label(RichText::new("💾 Sauvegarde").size(19.0).strong());
        ui.weak("Favoris, records, badges, agenda, réglages… dans un fichier (Téléchargements › Outils), à garder ou à envoyer au PC.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("💾 Sauvegarder").clicked() {
                commun::en_fond(&self.e, "Sauvegarde…", || appli::sauvegarder().map(|p| format!("✔ {}", p.display())));
            }
            let pc: Option<monpc::Pc> = charger("pc");
            if let Some(pc) = pc.filter(|p| !p.ip.is_empty()) {
                if ui.button("🖥 Sauvegarder sur le PC").clicked() {
                    commun::en_fond(&self.e, "Envoi au PC…", move || {
                        let p = appli::sauvegarder()?;
                        let b = std::fs::read(&p).map_err(|e| e.to_string())?;
                        pc.post(&format!("/envoyer?nom={}", monpc::encoder(&p.file_name().unwrap_or_default().to_string_lossy())), &b)?;
                        Ok("✔ Sauvegarde envoyée au PC (Téléchargements › Depuis le téléphone)".into())
                    });
                }
            }
        });
        for p in appli::sauvegardes().into_iter().take(5) {
            ui.horizontal(|ui| {
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                if ui.small_button("↩ Restaurer").clicked() {
                    let r = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())).and_then(|j| appli::restaurer_json(&j, &appli::dossier_donnees()));
                    match r {
                        Ok(n) => {
                            self.e.lock().unwrap().1 = format!("✔ {n} fichiers restaurés : l'appli redémarre.");
                            // les outils relisent leurs réglages au lancement
                            android::arriere_plan();
                            std::process::exit(0);
                        }
                        Err(e) => self.e.lock().unwrap().1 = format!("✖ {e}"),
                    }
                }
            });
        }
        if change {
            enregistrer("telephone", &self.reglages);
            self.style(ui.ctx());
        }
    }

    /// Manette tactile : chaque appui devient une vraie touche pour le jeu.
    fn manette(&mut self, ui: &mut egui::Ui) {
        let t = 64.0;
        let bouton = |ui: &mut egui::Ui, texte: &str| ui.add_sized([t, t], egui::Button::new(RichText::new(texte).size(26.0)).corner_radius(t / 2.0)).clicked();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.joueur2, "2e joueur (ZQSD)");
        });
        ui.horizontal(|ui| {
            // croix directionnelle
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(t + 8.0);
                    if bouton(ui, ou(ui, "⬆", "^")) {
                        self.injection.touches.push(if self.joueur2 { Key::Z } else { Key::ArrowUp });
                    }
                });
                ui.horizontal(|ui| {
                    if bouton(ui, ou(ui, "⬅", "<")) {
                        self.injection.touches.push(if self.joueur2 { Key::Q } else { Key::ArrowLeft });
                    }
                    ui.add_space(t + 8.0);
                    if bouton(ui, ou(ui, "➡", ">")) {
                        self.injection.touches.push(if self.joueur2 { Key::D } else { Key::ArrowRight });
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(t + 8.0);
                    if bouton(ui, ou(ui, "⬇", "v")) {
                        self.injection.touches.push(if self.joueur2 { Key::S } else { Key::ArrowDown });
                    }
                });
            });
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.add_space(t / 2.0);
                if ui.add_sized([t * 1.6, t], egui::Button::new(RichText::new("Espace").size(18.0))).clicked() {
                    self.injection.touches.push(Key::Space);
                }
                if ui.add_sized([t * 1.6, t], egui::Button::new(RichText::new("Entrée").size(18.0))).clicked() {
                    self.injection.touches.push(Key::Enter);
                }
            });
        });
    }

    fn chiffres(&mut self, ui: &mut egui::Ui) {
        let touches = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        let l = ((ui.available_width() - 9.0 * 4.0) / 10.0).clamp(26.0, 60.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = Vec2::new(2.0, 2.0);
            for (k, t) in touches.iter().enumerate() {
                if ui.add_sized([l, l * 1.1], egui::Button::new(RichText::new((k + 1).to_string()).size(24.0).strong())).clicked() {
                    self.injection.touches.push(*t);
                }
            }
            if ui.add_sized([l, l * 1.1], egui::Button::new(RichText::new("✖").size(22.0))).clicked() {
                self.injection.touches.push(Key::Backspace);
            }
        });
    }

    /// Clavier à l'écran : marche pour tous les champs de texte, même si le clavier d'Android ne s'ouvre pas.
    fn clavier(&mut self, ui: &mut egui::Ui) {
        let lignes: [&str; 4] = if self.majuscules { ["AZERTYUIOP", "QSDFGHJKLM", "WXCVBN'-", "1234567890"] } else { ["azertyuiop", "qsdfghjklm", "wxcvbné'", "1234567890"] };
        let l = ((ui.available_width() - 9.0 * 4.0) / 10.0).clamp(24.0, 56.0);
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        ui.spacing_mut().button_padding = Vec2::new(2.0, 2.0);
        for ligne in lignes {
            ui.horizontal(|ui| {
                for c in ligne.chars() {
                    if ui.add_sized([l, l * 1.05], egui::Button::new(RichText::new(c.to_string()).size(19.0))).clicked() {
                        self.injection.texte.push(c);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add_sized([l * 1.5, l], egui::SelectableLabel::new(self.majuscules, RichText::new("⇧").size(20.0))).clicked() {
                self.majuscules = !self.majuscules;
            }
            for c in ['à', 'è', 'ç', ',', '.', '?'] {
                if ui.add_sized([l * 0.9, l], egui::Button::new(RichText::new(c.to_string()).size(18.0))).clicked() {
                    self.injection.texte.push(c);
                }
            }
            if ui.add_sized([l * 2.0, l], egui::Button::new("espace")).clicked() {
                self.injection.texte.push(' ');
            }
            if ui.add_sized([l * 1.3, l], egui::Button::new(RichText::new("✖").size(20.0))).clicked() {
                self.injection.touches.push(Key::Backspace);
            }
            if ui.add_sized([l * 1.3, l], egui::Button::new(RichText::new("⏎").size(20.0))).clicked() {
                self.injection.touches.push(Key::Enter);
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.injection.appliquer(ctx);
        // Échap (clavier branché) : comme ←
        let retour = ctx.input(|i| i.key_pressed(Key::Escape));
        if retour {
            if self.clavier {
                self.clavier = false;
            } else if self.parametres {
                self.parametres = false;
            } else if self.ouvert.is_some() {
                self.ouvert = None;
            } else {
                android::arriere_plan();
            }
        }
        // thème « comme Android » : suit le mode sombre du téléphone
        if self.reglages.theme == 0 && self.theme_verifie.elapsed() > Duration::from_secs(3) {
            self.theme_verifie = Instant::now();
            let s = android::mode_sombre();
            if s != self.sombre {
                self.sombre = s;
                self.style(ctx);
            }
        }
        for f in self.fiches.iter_mut() {
            f.outil.tic(ctx);
        }
        if let Some((t, m)) = systeme::NOTIFICATIONS.lock().ok().and_then(|mut n| n.pop()) {
            self.toast = Some((format!("{t} — {m}"), Instant::now()));
        }
        let ouvert = self.ouvert;
        egui::TopBottomPanel::top("haut").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ouvert.is_some() && ui.add(egui::Button::new(RichText::new(ou(ui, "⬅", "<")).size(24.0)).min_size(Vec2::new(52.0, 44.0))).clicked() {
                    self.ouvert = None;
                    self.clavier = false;
                }
                match ouvert {
                    Some(k) => {
                        let nom = self.fiches[k].outil.nom();
                        let nom = if ui.fonts(|f| f.has_glyphs(&egui::FontId::proportional(20.0), nom)) { nom } else { nom.split_once(' ').map_or(nom, |x| x.1) };
                        ui.label(RichText::new(nom).size(20.0).strong());
                    }
                    None => {
                        ui.label(RichText::new("Accueil").size(18.0).weak());
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.clavier, RichText::new("⌨").size(22.0));
                });
            });
            ui.add_space(2.0);
        });
        if let Some((t, quand)) = &self.toast {
            if quand.elapsed() < Duration::from_secs(4) {
                egui::TopBottomPanel::top("toast").show(ctx, |ui| {
                    ui.label(RichText::new(t).color(Color32::WHITE).strong());
                });
                ctx.request_repaint_after(Duration::from_millis(500));
            } else {
                self.toast = None;
            }
        }
        // commandes en bas : clavier, manette ou chiffres
        let pave = ouvert.map(|k| self.fiches[k].pave).unwrap_or(Pave::Aucun);
        if self.clavier || pave != Pave::Aucun {
            egui::TopBottomPanel::bottom("bas").show(ctx, |ui| {
                ui.add_space(6.0);
                if self.clavier {
                    self.clavier(ui);
                } else if pave == Pave::Manette {
                    self.manette(ui);
                } else {
                    self.chiffres(ui);
                }
                ui.add_space(10.0);
            });
        }
        if self.parametres {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.parametres_ui(ui));
            });
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.ouvert {
            None => {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.accueil(ui));
            }
            Some(k) => self.outil_ajuste(ui, k),
        });
        // de retour à l'accueil : zoom normal
        if self.ouvert.is_none() && (ctx.zoom_factor() - 1.0).abs() > 0.01 {
            ctx.set_zoom_factor(1.0);
        }
    }
}

/// Point d'entrée Android.
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("Outils"));
    if let Some(d) = app.internal_data_path() {
        let _ = outil::DOSSIER.set(d);
    }
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    let r = eframe::run_native("Outils", options, Box::new(|cc| Ok(Box::new(App::new(cc)))));
    if let Err(e) = r {
        log::error!("Outils : {e}");
    }
}

/// Pour essayer l'appli sur PC : `cargo run --example bureau`.
pub fn lancer_bureau() -> eframe::Result {
    let options = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([412.0, 870.0]), ..Default::default() };
    eframe::run_native("Outils (téléphone)", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
