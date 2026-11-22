package fr.poiscail.outils;

import android.service.quicksettings.Tile;
import android.service.quicksettings.TileService;
import android.widget.Toast;

/** Base des tuiles qui pilotent le PC : un appui envoie la commande, la tuile dit si ça a marché. */
abstract class TuilePc extends TileService {
    abstract String commande();

    abstract String libelle();

    @Override
    public void onStartListening() {
        Tile t = getQsTile();
        if (t == null) return;
        t.setState(Tile.STATE_INACTIVE);
        t.setLabel(libelle());
        t.updateTile();
    }

    @Override
    public void onClick() {
        final Tile t = getQsTile();
        if (t != null) {
            t.setState(Tile.STATE_ACTIVE);
            t.updateTile();
        }
        new Thread(() -> {
            final String err = PcDistant.commande(this, commande());
            new android.os.Handler(getMainLooper()).post(() -> {
                if (err != null) Toast.makeText(this, "Outils : " + err, Toast.LENGTH_SHORT).show();
                if (t != null) {
                    t.setState(Tile.STATE_INACTIVE);
                    t.updateTile();
                }
            });
        }).start();
    }
}
