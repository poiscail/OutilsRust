package fr.poiscail.outils;

import android.content.Context;
import java.io.File;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.file.Files;
import org.json.JSONObject;

/** Envoie une commande à Outils sur le PC associé (pc.json de l'appli), pour les tuiles de réglages rapides. */
final class PcDistant {
    private PcDistant() {}

    /** Renvoie null si tout va bien, sinon le message d'erreur. */
    static String commande(Context c, String cmd) {
        try {
            File f = new File(c.getFilesDir(), "pc.json");
            if (!f.exists()) return "PC non associé";
            String t = new String(Files.readAllBytes(f.toPath()), "UTF-8").trim();
            if (t.equals("null") || t.isEmpty()) return "PC non associé";
            JSONObject j = new JSONObject(t);
            String url = "http://" + j.getString("ip") + ":" + j.getInt("port") + "/cmd/" + cmd + "?k=" + j.getString("cle");
            HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
            h.setConnectTimeout(3000);
            h.setReadTimeout(4000);
            h.setRequestMethod("POST");
            h.setDoOutput(true);
            try (OutputStream o = h.getOutputStream()) {
                o.write(new byte[0]);
            }
            int r = h.getResponseCode();
            h.disconnect();
            return r == 200 ? null : "Erreur " + r;
        } catch (Exception e) {
            return "PC injoignable";
        }
    }
}
