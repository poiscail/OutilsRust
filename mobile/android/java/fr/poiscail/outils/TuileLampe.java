package fr.poiscail.outils;

import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraManager;
import android.service.quicksettings.Tile;
import android.service.quicksettings.TileService;

/** Tuile « Lampe » : allume ou éteint le flash, et suit son état (si une autre appli l'allume). */
public class TuileLampe extends TileService {
    static boolean allumee = false;
    private SuiviLampe suivi;

    private String camera(CameraManager cm) throws Exception {
        for (String id : cm.getCameraIdList()) {
            Boolean flash = cm.getCameraCharacteristics(id).get(CameraCharacteristics.FLASH_INFO_AVAILABLE);
            if (flash != null && flash) return id;
        }
        return null;
    }

    void afficher() {
        Tile t = getQsTile();
        if (t == null) return;
        t.setState(allumee ? Tile.STATE_ACTIVE : Tile.STATE_INACTIVE);
        t.setLabel("Lampe");
        t.updateTile();
    }

    @Override
    public void onStartListening() {
        CameraManager cm = (CameraManager) getSystemService(CAMERA_SERVICE);
        suivi = new SuiviLampe(this);
        try {
            cm.registerTorchCallback(suivi, null);
        } catch (Exception e) {
            // pas de suivi
        }
        afficher();
    }

    @Override
    public void onStopListening() {
        try {
            ((CameraManager) getSystemService(CAMERA_SERVICE)).unregisterTorchCallback(suivi);
        } catch (Exception e) {
            // déjà retiré
        }
    }

    @Override
    public void onClick() {
        CameraManager cm = (CameraManager) getSystemService(CAMERA_SERVICE);
        try {
            String id = camera(cm);
            if (id == null) return;
            allumee = !allumee;
            cm.setTorchMode(id, allumee);
        } catch (Exception e) {
            allumee = false;
        }
        afficher();
    }
}
