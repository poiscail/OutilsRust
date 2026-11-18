//! Liste des outils ajoutés en lot (trait `Outil`), dans l'ordre du menu de chaque catégorie.
use crate::outil::Outil;
use crate::{bureau, fichiers2, images, listes, ludique, minijeux, quotidien, pdf, reseau2, speedrun, systeme2, tests_materiel, video, voix};

pub fn tous() -> Vec<Box<dyn Outil>> {
    vec![
        // jeux
        Box::new(ludique::Tirage::new()),
        Box::new(speedrun::Speedrun::new()),
        Box::new(ludique::Reflexes::new()),
        Box::new(ludique::Sensibilite::new()),
        Box::new(tests_materiel::TestSouris),
        Box::new(tests_materiel::TestClavier),
        // fichiers
        Box::new(fichiers2::Favoris::new()),
        Box::new(images::Visionneuse::new()),
        Box::new(images::DoublonsImages::new()),
        Box::new(images::PhotosOutils::new()),
        Box::new(video::VideoRapide::new()),
        Box::new(pdf::OutilPdf::new()),
        Box::new(fichiers2::Archives::new()),
        Box::new(fichiers2::GrosFichiers::new()),
        Box::new(fichiers2::Deplacer::new()),
        Box::new(fichiers2::Recuperation::new()),
        Box::new(fichiers2::Destructeur::new()),
        // système
        Box::new(systeme2::InfosPc::new()),
        Box::new(systeme2::GestProcessus::new()),
        Box::new(systeme2::Services::new()),
        Box::new(systeme2::Pilotes::new()),
        Box::new(systeme2::Energie::new()),
        Box::new(bureau::BancDisque::new()),
        Box::new(bureau::Ecrans::new()),
        Box::new(bureau::Bureaux::new()),
        Box::new(bureau::Transparence::new()),
        Box::new(systeme2::Plantages::new()),
        Box::new(systeme2::Reparation::new()),
        // réseau
        Box::new(reseau2::Telechargeur::new()),
        Box::new(reseau2::BloqueurUi::new()),
        Box::new(reseau2::DnsRapide::new()),
        Box::new(reseau2::WifiMdp::new()),
        Box::new(reseau2::Ports::new()),
        Box::new(reseau2::HistoCoupures::new()),
        Box::new(reseau2::Reveil::new()),
        Box::new(reseau2::Surveillance::new()),
        Box::new(reseau2::LiensCourts::new()),
        // quotidien
        Box::new(listes::Liste::new(&listes::TACHES)),
        Box::new(listes::Kanban::new()),
        Box::new(quotidien::Reveil::new()),
        Box::new(quotidien::TempsEcran::new()),
        Box::new(quotidien::ModeConcentration::new()),
        Box::new(listes::Liste::new(&listes::LIENS)),
        Box::new(listes::Liste::new(&listes::FILMS)),
        Box::new(listes::Liste::new(&listes::LECTURE)),
        Box::new(voix::Voix::new()),
        Box::new(ludique::MotsDePasse::new()),
        // fun
        Box::new(ludique::Des::new()),
        Box::new(minijeux::MiniJeux::new()),
    ]
}
