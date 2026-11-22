package fr.poiscail.outils;

import android.hardware.camera2.CameraManager;

/** Suit l'état du flash (allumé par une autre appli…) pour la tuile Lampe. */
final class SuiviLampe extends CameraManager.TorchCallback {
    private final TuileLampe tuile;

    SuiviLampe(TuileLampe t) {
        tuile = t;
    }

    @Override
    public void onTorchModeChanged(String id, boolean on) {
        TuileLampe.allumee = on;
        tuile.afficher();
    }
}
