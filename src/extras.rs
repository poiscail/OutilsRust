//! Liste des outils ajoutés en lot (trait `Outil`), dans l'ordre du menu de chaque catégorie.
use crate::outil::Outil;
use crate::{listes, ludique, minijeux, quotidien, speedrun, tests_materiel, voix};

pub fn tous() -> Vec<Box<dyn Outil>> {
    vec![
        // jeux
        Box::new(ludique::Tirage::new()),
        Box::new(speedrun::Speedrun::new()),
        Box::new(ludique::Reflexes::new()),
        Box::new(ludique::Sensibilite::new()),
        Box::new(tests_materiel::TestSouris),
        Box::new(tests_materiel::TestClavier),
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
