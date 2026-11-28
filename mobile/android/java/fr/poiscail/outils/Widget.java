package fr.poiscail.outils;

import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.os.BatteryManager;
import android.widget.RemoteViews;
import java.io.File;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.file.Files;
import java.text.SimpleDateFormat;
import java.util.Date;
import java.util.Locale;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Widget d'écran d'accueil : état du PC (processeur, température, jeu, mémoire), prochain rendez-vous
 * du jour, batterie, et des boutons pour le PC (verrouiller, pause, volume). Un appui ailleurs ouvre Outils.
 */
public class Widget extends AppWidgetProvider {
    static final String ACTION = "fr.poiscail.outils.WIDGET";

    @Override
    public void onReceive(Context c, Intent i) {
        if (!ACTION.equals(i.getAction())) {
            super.onReceive(c, i);
            return;
        }
        final String cmd = i.getStringExtra("cmd");
        final PendingResult attente = goAsync();
        final Context ctx = c.getApplicationContext();
        new Thread(() -> {
            try {
                if (cmd != null && !cmd.equals("maj")) commandePc(ctx, cmd);
                AppWidgetManager m = AppWidgetManager.getInstance(ctx);
                int[] ids = m.getAppWidgetIds(new android.content.ComponentName(ctx, Widget.class));
                RemoteViews v = vue(ctx);
                for (int id : ids) m.updateAppWidget(id, v);
            } catch (Exception e) {
                // PC injoignable : rien à faire
            } finally {
                attente.finish();
            }
        }).start();
    }

    /** Envoie une commande au PC (même adresse et même clé que l'appli). */
    static void commandePc(Context c, String cmd) throws Exception {
        JSONObject p = new JSONObject(lire(new File(c.getFilesDir(), "pc.json")).trim());
        URL u = new URL("http://" + p.getString("ip") + ":" + p.getInt("port") + "/cmd/" + cmd + "?k=" + p.getString("cle"));
        HttpURLConnection h = (HttpURLConnection) u.openConnection();
        h.setRequestMethod("POST");
        h.setDoOutput(true);
        h.setConnectTimeout(2500);
        h.setReadTimeout(3000);
        h.getOutputStream().close();
        h.getResponseCode();
        h.disconnect();
    }

    static PendingIntent bouton(Context c, String cmd, int n) {
        Intent i = new Intent(c, Widget.class).setAction(ACTION).putExtra("cmd", cmd);
        return PendingIntent.getBroadcast(c, 100 + n, i, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
    }
    @Override
    public void onUpdate(Context c, AppWidgetManager m, int[] ids) {
        final PendingResult attente = goAsync();
        final Context ctx = c.getApplicationContext();
        new Thread(() -> {
            try {
                RemoteViews v = vue(ctx);
                for (int id : ids) m.updateAppWidget(id, v);
            } catch (Exception e) {
                // widget laissé tel quel
            } finally {
                attente.finish();
            }
        }).start();
    }

    static String lire(File f) {
        try {
            return new String(Files.readAllBytes(f.toPath()), "UTF-8");
        } catch (Exception e) {
            return "";
        }
    }

    /** « 🖥 PC : 23 % · 🎮 jeu » ou « PC éteint ». */
    static String etatPc(Context c) {
        try {
            String t = lire(new File(c.getFilesDir(), "pc.json")).trim();
            if (t.isEmpty() || t.equals("null")) return "🖥 PC non associé";
            JSONObject p = new JSONObject(t);
            URL u = new URL("http://" + p.getString("ip") + ":" + p.getInt("port") + "/api/etat?k=" + p.getString("cle"));
            HttpURLConnection h = (HttpURLConnection) u.openConnection();
            h.setConnectTimeout(2500);
            h.setReadTimeout(3000);
            java.io.InputStream in = h.getInputStream();
            java.io.ByteArrayOutputStream o = new java.io.ByteArrayOutputStream();
            byte[] t2 = new byte[4096];
            int n;
            while ((n = in.read(t2)) > 0) o.write(t2, 0, n);
            String r = o.toString("UTF-8");
            h.disconnect();
            JSONObject e = new JSONObject(r);
            String s = "🖥 " + e.optString("nom", "PC") + " allumé";
            double cpu = e.optDouble("cpu", -1);
            if (cpu >= 0) s += " · " + Math.round(cpu) + " %";
            double temp = e.optDouble("cpu_t", -1);
            if (temp > 0) s += " · 🌡 " + Math.round(temp) + "°C";
            double ram = e.optDouble("ram", -1);
            if (ram >= 0) s += " · RAM " + Math.round(ram) + " %";
            String jeu = e.isNull("jeu") ? "" : e.optString("jeu", "");
            if (!jeu.isEmpty()) s += " · 🎮 " + jeu;
            if (e.optString("mc", "").equals("en ligne")) s += " · ⛏ serveur en ligne";
            return s;
        } catch (Exception e) {
            return "🖥 PC éteint ou pas sur le Wi-Fi";
        }
    }

    /** Prochain événement de l'agenda aujourd'hui. */
    static String agenda(Context c) {
        try {
            JSONArray a = new JSONArray(lire(new File(c.getFilesDir(), "agenda.json")));
            String auj = new SimpleDateFormat("yyyy-MM-dd", Locale.FRANCE).format(new Date());
            java.util.Calendar cal = java.util.Calendar.getInstance();
            int maintenant = cal.get(java.util.Calendar.HOUR_OF_DAY) * 60 + cal.get(java.util.Calendar.MINUTE);
            String meilleur = null;
            int mh = 99999;
            for (int i = 0; i < a.length(); i++) {
                JSONObject e = a.getJSONObject(i);
                if (!auj.equals(e.optString("date"))) continue;
                int h = e.isNull("heure") ? -1 : e.optInt("heure", -1);
                if (h >= 0 && h < maintenant) continue;
                int cle = h < 0 ? 0 : h;
                if (cle < mh) {
                    mh = cle;
                    meilleur = (h >= 0 ? String.format(Locale.FRANCE, "%dh%02d ", h / 60, h % 60) : "") + e.optString("titre");
                }
            }
            return meilleur == null ? "📅 Rien de prévu aujourd'hui" : "📅 " + meilleur;
        } catch (Exception e) {
            return "📅 Agenda vide";
        }
    }

    /** Identifiant d'une ressource (sans classe R : l'appli est assemblée sans elle). */
    static int res(Context c, String nom, String genre) {
        return c.getResources().getIdentifier(nom, genre, c.getPackageName());
    }

    static RemoteViews vue(Context c) {
        RemoteViews v = new RemoteViews(c.getPackageName(), res(c, "widget", "layout"));
        Intent b = c.registerReceiver(null, new IntentFilter(Intent.ACTION_BATTERY_CHANGED));
        String bat = "";
        if (b != null) {
            int niv = b.getIntExtra(BatteryManager.EXTRA_LEVEL, -1) * 100 / Math.max(1, b.getIntExtra(BatteryManager.EXTRA_SCALE, 100));
            boolean charge = b.getIntExtra(BatteryManager.EXTRA_PLUGGED, 0) != 0;
            bat = "🔋 " + niv + " %" + (charge ? " ⚡" : "");
        }
        v.setTextViewText(res(c, "ligne_pc", "id"), etatPc(c));
        v.setTextViewText(res(c, "ligne_agenda", "id"), agenda(c));
        v.setTextViewText(res(c, "ligne_batterie", "id"), bat + "   · " + new SimpleDateFormat("HH:mm", Locale.FRANCE).format(new Date()));
        String[][] boutons = {{"b_verrou", "verrouiller"}, {"b_pause", "lecture"}, {"b_moins", "vol_moins"}, {"b_plus", "vol_plus"}, {"b_maj", "maj"}};
        for (int k = 0; k < boutons.length; k++) {
            v.setOnClickPendingIntent(res(c, boutons[k][0], "id"), bouton(c, boutons[k][1], k));
        }
        Intent ouvrir = c.getPackageManager().getLaunchIntentForPackage(c.getPackageName());
        if (ouvrir != null) {
            v.setOnClickPendingIntent(res(c, "racine", "id"), PendingIntent.getActivity(c, 0, ouvrir, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT));
        }
        return v;
    }
}
