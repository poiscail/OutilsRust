package fr.poiscail.outils;

import android.app.Activity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.widget.Toast;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;

/**
 * « Partager › Outils » depuis n'importe quelle appli : copie ce qui est partagé (photos, vidéos,
 * fichiers, texte ou lien) dans le dossier privé de l'appli, puis ouvre Outils, qui propose quoi en faire.
 */
public class Partage extends Activity {
    @Override
    protected void onCreate(Bundle b) {
        super.onCreate(b);
        final Intent i = getIntent();
        final File dossier = new File(getFilesDir(), "partage_entrant");
        dossier.mkdirs();
        Toast.makeText(this, "Outils : réception…", Toast.LENGTH_SHORT).show();
        new Thread(() -> {
            try {
                ArrayList<Uri> uris = new ArrayList<>();
                if (Intent.ACTION_SEND_MULTIPLE.equals(i.getAction())) {
                    ArrayList<Uri> l = i.getParcelableArrayListExtra(Intent.EXTRA_STREAM);
                    if (l != null) uris.addAll(l);
                } else {
                    Uri u = i.getParcelableExtra(Intent.EXTRA_STREAM);
                    if (u != null) uris.add(u);
                }
                for (Uri u : uris) copier(u, dossier);
                CharSequence t = i.getCharSequenceExtra(Intent.EXTRA_TEXT);
                if (t != null && t.length() > 0 && uris.isEmpty()) {
                    try (OutputStream o = new FileOutputStream(new File(dossier, "partage_texte.txt"))) {
                        o.write(t.toString().getBytes("UTF-8"));
                    }
                }
            } catch (Exception e) {
                // rien : Outils affichera ce qui a pu être copié
            }
            runOnUiThread(() -> {
                Intent ouvrir = getPackageManager().getLaunchIntentForPackage(getPackageName());
                if (ouvrir != null) {
                    ouvrir.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                    startActivity(ouvrir);
                }
                finish();
            });
        }).start();
    }

    private String nom(Uri u) {
        String n = null;
        try (Cursor c = getContentResolver().query(u, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) n = c.getString(0);
        } catch (Exception e) {
            // nom inconnu
        }
        if (n == null || n.isEmpty()) n = u.getLastPathSegment();
        if (n == null || n.isEmpty()) n = "fichier";
        return n.replace('/', '_').replace('\\', '_');
    }

    private void copier(Uri u, File dossier) throws Exception {
        String n = nom(u);
        File f = new File(dossier, n);
        int k = 2;
        while (f.exists()) {
            int p = n.lastIndexOf('.');
            f = new File(dossier, p > 0 ? n.substring(0, p) + " (" + k + ")" + n.substring(p) : n + " (" + k + ")");
            k++;
        }
        try (InputStream in = getContentResolver().openInputStream(u); OutputStream out = new FileOutputStream(f)) {
            if (in == null) return;
            byte[] tampon = new byte[1 << 16];
            int lu;
            while ((lu = in.read(tampon)) > 0) out.write(tampon, 0, lu);
        }
    }
}
