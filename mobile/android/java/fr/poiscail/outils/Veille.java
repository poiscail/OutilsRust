package fr.poiscail.outils;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/**
 * Service « au premier plan » : garde Outils en vie quand l'appli est fermée, pour que les rappels,
 * la surveillance du Wi-Fi, des coupures et du forfait continuent. Une petite notification permanente
 * l'indique (obligatoire sur Android).
 */
public class Veille extends Service {
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
        return START_NOT_STICKY;
    }
}
