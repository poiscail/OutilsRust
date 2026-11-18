//! Liste des outils ajoutés en lot (trait `Outil`), dans l'ordre du menu de chaque catégorie.
use crate::outil::Outil;
use crate::{ludique, minijeux, speedrun, tests_materiel};

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
        Box::new(ludique::MotsDePasse::new()),
        // fun
        Box::new(ludique::Des::new()),
        Box::new(minijeux::MiniJeux::new()),
    ]
}
