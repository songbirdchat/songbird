package chat.songbird.share;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.graphics.PixelFormat;
import android.hardware.display.DisplayManager;
import android.hardware.display.VirtualDisplay;
import android.media.Image;
import android.media.ImageReader;
import android.media.projection.MediaProjection;
import android.media.projection.MediaProjectionManager;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.IBinder;
import android.util.DisplayMetrics;
import android.view.WindowManager;

import com.longbridge.nsh.AppActivity;
import chat.songbird.device.DeviceBridge;

/**
 * Sharing the screen in a call (D-0180): MediaProjection, after the system's own prompt, inside
 * this foreground service, whose notification says the screen is shared and stops it. Frames
 * go to Rust ({@code nativeScreenFrame}) at up to 15 a second, at most 1280 pixels on the long
 * side, and from there only into the call, encrypted. Nothing is recorded.
 */
public final class ScreenShareService extends Service {
    public static final String EXTRA_CODE = "code";
    public static final String EXTRA_DATA = "data";
    private static final String CHANNEL = "screen";
    private static final int NOTIFICATION = 7;
    private static final String STOP = "chat.songbird.share.STOP";
    private static final int MAX_SIDE = 1280;
    private static final long FRAME_NS = 1_000_000_000L / 15;

    private static ScreenShareService running;

    private MediaProjection projection;
    private VirtualDisplay display;
    private ImageReader reader;
    private HandlerThread thread;
    private long last;
    private boolean stoppedHere;

    /** Stops sharing from the app (no "ended" goes back: the app asked). */
    public static void stopFromApp() {
        ScreenShareService s = running;
        if (s != null) {
            s.stoppedHere = true;
            s.finish();
        }
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent != null && STOP.equals(intent.getAction())) {
            finish();
            return START_NOT_STICKY;
        }
        if (intent == null || running != null) {
            DeviceBridge.screenStarted(false);
            stopSelf();
            return START_NOT_STICKY;
        }
        // The service is in the foreground before the projection is taken, as Android 14 asks.
        if (android.os.Build.VERSION.SDK_INT >= 29) {
            startForeground(NOTIFICATION, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION);
        } else {
            startForeground(NOTIFICATION, notification());
        }
        MediaProjectionManager mpm = getSystemService(MediaProjectionManager.class);
        Intent data = extra(intent);
        projection = data == null ? null : mpm.getMediaProjection(intent.getIntExtra(EXTRA_CODE, 0), data);
        if (projection == null) {
            DeviceBridge.screenStarted(false);
            stopForeground(STOP_FOREGROUND_REMOVE);
            stopSelf();
            return START_NOT_STICKY;
        }
        running = this;
        thread = new HandlerThread("songbird-screen");
        thread.start();
        Handler handler = new Handler(thread.getLooper());
        projection.registerCallback(new MediaProjection.Callback() {
            @Override
            public void onStop() {
                finish();
            }
        }, handler);

        DisplayMetrics m = new DisplayMetrics();
        WindowManager wm = getSystemService(WindowManager.class);
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            m.widthPixels = wm.getMaximumWindowMetrics().getBounds().width();
            m.heightPixels = wm.getMaximumWindowMetrics().getBounds().height();
        } else {
            realMetrics(wm, m);
        }
        int dpi = getResources().getDisplayMetrics().densityDpi;
        float scale = Math.min(1f, (float) MAX_SIDE / Math.max(m.widthPixels, m.heightPixels));
        int w = Math.max(2, Math.round(m.widthPixels * scale) & ~1);
        int h = Math.max(2, Math.round(m.heightPixels * scale) & ~1);

        reader = ImageReader.newInstance(w, h, PixelFormat.RGBA_8888, 2);
        reader.setOnImageAvailableListener(r -> {
            Image image = r.acquireLatestImage();
            if (image == null) return;
            try {
                long now = image.getTimestamp();
                if (now - last < FRAME_NS) return;
                last = now;
                Image.Plane p = image.getPlanes()[0];
                DeviceBridge.screenFrame(p.getBuffer(), image.getWidth(), image.getHeight(), p.getRowStride(), 0);
            } finally {
                image.close();
            }
        }, handler);
        display = projection.createVirtualDisplay("Songbird screen", w, h, dpi,
                DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR, reader.getSurface(), null, handler);
        DeviceBridge.screenStarted(display != null);
        if (display == null) finish();
        return START_NOT_STICKY;
    }

    @SuppressWarnings("deprecation")
    private static Intent extra(Intent intent) {
        if (android.os.Build.VERSION.SDK_INT >= 33) return intent.getParcelableExtra(EXTRA_DATA, Intent.class);
        return intent.getParcelableExtra(EXTRA_DATA);
    }

    @SuppressWarnings("deprecation")
    private static void realMetrics(WindowManager wm, DisplayMetrics m) {
        wm.getDefaultDisplay().getRealMetrics(m);
    }

    private Notification notification() {
        NotificationManager nm = getSystemService(NotificationManager.class);
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Screen sharing",
                NotificationManager.IMPORTANCE_LOW));
        PendingIntent open = PendingIntent.getActivity(this, 1,
                new Intent(this, AppActivity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        PendingIntent stop = PendingIntent.getService(this, 2,
                new Intent(this, ScreenShareService.class).setAction(STOP),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.ic_menu_share)
                .setContentTitle("You're sharing your screen")
                .setContentText("Everyone in the call sees it.")
                .setOngoing(true)
                .setContentIntent(open)
                .addAction(new Notification.Action.Builder((android.graphics.drawable.Icon) null, "Stop sharing", stop).build())
                .build();
    }

    /** Ends capture and the service; tells Rust unless the app stopped it. */
    private synchronized void finish() {
        if (running != this) return;
        running = null;
        if (display != null) display.release();
        if (reader != null) reader.close();
        if (projection != null) projection.stop();
        if (thread != null) thread.quitSafely();
        if (!stoppedHere) DeviceBridge.screenEnded();
        stopForeground(STOP_FOREGROUND_REMOVE);
        stopSelf();
    }

    @Override
    public void onDestroy() {
        finish();
        super.onDestroy();
    }
}
