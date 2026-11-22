package fr.poiscail.outils;

/** Tuile « Couper le son du PC ». */
public class TuileMuet extends TuilePc {
    @Override
    String commande() {
        return "muet";
    }

    @Override
    String libelle() {
        return "PC muet";
    }
}
