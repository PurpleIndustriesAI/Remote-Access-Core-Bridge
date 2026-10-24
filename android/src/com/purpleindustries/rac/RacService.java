package com.purpleindustries.rac;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.IBinder;

import java.net.Socket;
import java.util.concurrent.Semaphore;

import javax.net.ssl.SSLServerSocket;
import javax.net.ssl.SSLSocket;

/**
 * Receives from paired computers while switched on. It listens only while its notification is shown,
 * accepts only pinned certificates, and only from LAN, link-local or Tailscale addresses.
 */
public final class RacService extends Service {
    static final String STOP = "com.purpleindustries.rac.STOP";
    private static final String CHANNEL = "receiving";
    static volatile boolean running;
    private SSLServerSocket server;
    private Thread loop;
    private final Semaphore slots = new Semaphore(16);

    static void start(Context c) {
        c.startForegroundService(new Intent(c, RacService.class));
    }

    static void stop(Context c) {
        c.stopService(new Intent(c, RacService.class));
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && STOP.equals(intent.getAction())) {
            stopSelf();
            return START_NOT_STICKY;
        }
        startForeground(1, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE);
        if (loop == null) {
            loop = new Thread(this::serve, "rac-listener");
            loop.start();
        }
        return START_STICKY;
    }

    private Notification notification() {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Receiving from computers", NotificationManager.IMPORTANCE_LOW));
        PendingIntent open = PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class), PendingIntent.FLAG_IMMUTABLE);
        PendingIntent stop = PendingIntent.getService(this, 1, new Intent(this, RacService.class).setAction(STOP), PendingIntent.FLAG_IMMUTABLE);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(R.drawable.ic_stat)
                .setContentTitle("Remote Access Core (Bridge) is receiving")
                .setContentText("Paired computers can send text, images and files")
                .setContentIntent(open)
                .addAction(new Notification.Action.Builder(null, "Stop", stop).build())
                .setOngoing(true)
                .build();
    }

    private void serve() {
        try {
            server = Rac.listen(Rac.pins(this));
            running = true;
            Transfer.log("Receiving on port " + Rac.PORT);
            while (!server.isClosed()) {
                Socket s = server.accept();
                if (!Rac.allowed(s.getInetAddress()) || !slots.tryAcquire()) {
                    s.close();
                    continue;
                }
                new Thread(() -> handle((SSLSocket) s), "rac-conn").start();
            }
        } catch (Exception e) {
            if (server == null || !server.isClosed()) Transfer.log("Receiving stopped: " + e.getMessage());
        } finally {
            running = false;
            Transfer.log("Receiving is off");
        }
    }

    private void handle(SSLSocket s) {
        try (SSLSocket c = s) {
            c.setSoTimeout(Rac.IO_TIMEOUT_MS);
            c.startHandshake();
            Transfer.receive(this, c);
        } catch (Exception e) {
            // An unpaired or broken client is dropped. Paired computers get an error reply from receive().
        } finally {
            slots.release();
        }
    }

    /** Pins are read when listening starts, so a new pairing restarts the listener. */
    static void restartIfRunning(Context c) {
        if (running) {
            stop(c);
            start(c);
        }
    }

    @Override
    public void onDestroy() {
        try {
            if (server != null) server.close();
        } catch (Exception ignored) {
            // Already closed.
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }
}
