package fr.poiscail.outils;

/** Tuile « Lecture / pause » de la musique ou de la vidéo du PC. */
public class TuilePause extends TuilePc {
    @Override
    String commande() {
        return "lecture";
    }

    @Override
    String libelle() {
        return "PC ⏯";
    }
}
