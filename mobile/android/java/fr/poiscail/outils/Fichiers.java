package fr.poiscail.outils;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileNotFoundException;

/**
 * Donne accès (en lecture) aux fichiers du dossier « partage » de l'appli, par une adresse content://.
 * Sert à installer une mise à jour d'Outils (l'installeur d'Android ne lit pas les fichiers directement).
 */
public class Fichiers extends ContentProvider {
    private File fichier(Uri uri) throws FileNotFoundException {
        File dossier = new File(getContext().getFilesDir(), "partage");
        File f = new File(dossier, uri.getLastPathSegment());
        if (!f.getParentFile().equals(dossier) || !f.exists()) throw new FileNotFoundException(uri.toString());
        return f;
    }

    @Override public boolean onCreate() { return true; }

    @Override public String getType(Uri uri) {
        String n = uri.getLastPathSegment();
        if (n.endsWith(".jpg")) return "image/jpeg";
        return n.endsWith(".apk") ? "application/vnd.android.package-archive" : "application/octet-stream";
    }

    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        String nom = uri.getLastPathSegment();
        // l'appareil photo d'Android écrit ici les pages du scanner de documents
        if (mode.contains("w") && nom != null && nom.matches("scan_[0-9]+\\.jpg")) {
            File dossier = new File(getContext().getFilesDir(), "partage");
            dossier.mkdirs();
            return ParcelFileDescriptor.open(new File(dossier, nom), ParcelFileDescriptor.MODE_READ_WRITE | ParcelFileDescriptor.MODE_CREATE | ParcelFileDescriptor.MODE_TRUNCATE);
        }
        return ParcelFileDescriptor.open(fichier(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override public Cursor query(Uri uri, String[] p, String s, String[] a, String o) {
        MatrixCursor c = new MatrixCursor(new String[] { OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE });
        try {
            File f = fichier(uri);
            c.addRow(new Object[] { f.getName(), f.length() });
        } catch (FileNotFoundException e) { }
        return c;
    }

    @Override public Uri insert(Uri uri, ContentValues v) { return null; }
    @Override public int delete(Uri uri, String s, String[] a) { return 0; }
    @Override public int update(Uri uri, ContentValues v, String s, String[] a) { return 0; }
}
