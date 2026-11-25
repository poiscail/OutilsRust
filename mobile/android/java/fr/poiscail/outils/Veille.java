package fr.poiscail.outils;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.media.AudioAttributes;
import android.media.AudioManager;
import android.media.Ringtone;
import android.media.RingtoneManager;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.os.IBinder;
import android.os.VibrationEffect;
import android.os.Vibrator;
import java.io.File;
import java.io.FileInputStream;
import java.net.DatagramPacket;
import java.net.DatagramSocket;
import java.net.InetSocketAddress;
import org.json.JSONObject;

/**
 * Service « au premier plan » : garde Outils en vie quand l'appli est fermée, pour que les rappels,
 * la surveillance du Wi-Fi, des coupures et du forfait continuent. Une petite notification permanente
 * l'indique (obligatoire sur Android).
 */
public class Veille extends Service implements Runnable {
    /** Port où le PC envoie « faire sonner » (même appli fermée). */
    static final int PORT_SONNER = 8646;
    private static volatile Thread ecoute;
    private static volatile DatagramSocket prise;

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        NotificationManager nm = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        nm.createNotificationChannel(new NotificationChannel("veille", "Outils en veille", NotificationManager.IMPORTANCE_MIN));
        Intent ouvrir = getPackageManager().getLaunchIntentForPackage(getPackageName());
        PendingIntent pi = PendingIntent.getActivity(this, 0, ouvrir, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification n = new Notification.Builder(this, "veille")
                .setSmallIcon(getApplicationInfo().icon)
                .setContentTitle("Outils veille")
                .setContentText("Rappels, Wi-Fi et forfait surveillés")
                .setOngoing(true)
                .setContentIntent(pi)
                .build();
        if (Build.VERSION.SDK_INT >= 34) {
            startForeground(1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        } else {
            startForeground(1, n);
        }
        if (ecoute == null) {
            ecoute = new Thread(this, "outils-sonner");
            ecoute.setDaemon(true);
            ecoute.start();
        }
        return START_NOT_STICKY;
    }

    @Override
    public void onDestroy() {
        DatagramSocket p = prise;
        if (p != null) p.close();
        ecoute = null;
        super.onDestroy();
    }

    /** Clé d'association du PC (pc.json, écrit par l'appli). */
    private String cle() {
        try {
            File f = new File(getFilesDir(), "pc.json");
            byte[] b = new byte[(int) f.length()];
            FileInputStream in = new FileInputStream(f);
            int n = 0;
            while (n < b.length) {
                int r = in.read(b, n, b.length - n);
                if (r < 0) break;
                n += r;
            }
            in.close();
            return new JSONObject(new String(b, 0, n, "UTF-8")).optString("cle", "");
        } catch (Exception e) {
            return "";
        }
    }

    /** Écoute le PC : « OUTILS-SONNE|clé » fait sonner le téléphone. */
    @Override
    public void run() {
        WifiManager.MulticastLock verrou = null;
        try {
            WifiManager wm = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
            verrou = wm.createMulticastLock("outils-sonner");
            verrou.acquire();
            DatagramSocket s = new DatagramSocket(null);
            s.setReuseAddress(true);
            s.setBroadcast(true);
            s.bind(new InetSocketAddress(PORT_SONNER));
            prise = s;
            byte[] b = new byte[256];
            while (ecoute != null) {
                DatagramPacket p = new DatagramPacket(b, b.length);
                s.receive(p);
                String m = new String(p.getData(), 0, p.getLength(), "UTF-8");
                String c = cle();
                if (!c.isEmpty() && m.equals("OUTILS-SONNE|" + c)) sonner(30);
            }
        } catch (Exception e) {
            // port pris ou service arrêté : on s'arrête simplement
        } finally {
            if (verrou != null && verrou.isHeld()) verrou.release();
            ecoute = null;
        }
    }

    /** Sonnerie d'alarme à fond (même en silencieux) et vibrations pendant `secondes`. */
    private void sonner(int secondes) {
        try {
            AudioManager am = (AudioManager) getSystemService(AUDIO_SERVICE);
            am.setStreamVolume(AudioManager.STREAM_ALARM, am.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0);
            Ringtone r = RingtoneManager.getRingtone(this, RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM));
            Vibrator v = (Vibrator) getSystemService(VIBRATOR_SERVICE);
            if (r != null) {
                r.setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_ALARM).build());
                r.play();
            }
            long fin = System.currentTimeMillis() + secondes * 1000L;
            while (System.currentTimeMillis() < fin) {
                if (v != null) v.vibrate(VibrationEffect.createOneShot(400, VibrationEffect.DEFAULT_AMPLITUDE));
                Thread.sleep(700);
                if (r != null && !r.isPlaying()) r.play();
            }
            if (r != null) r.stop();
        } catch (Exception e) {
            // rien : on a essayé
        }
    }
}
