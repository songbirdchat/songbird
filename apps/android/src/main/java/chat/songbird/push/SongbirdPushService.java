package chat.songbird.push;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.util.Base64;

import com.longbridge.nsh.AppActivity;
import chat.songbird.device.DeviceBridge;

import org.unifiedpush.android.connector.FailedReason;
import org.unifiedpush.android.connector.PushService;
import org.unifiedpush.android.connector.data.PushEndpoint;
import org.unifiedpush.android.connector.data.PushMessage;

/**
 * UnifiedPush (D-0176): the user's own distributor wakes Songbird; no Google services. The
 * endpoint and its keys go to Songbird's api as a Web Push registration, sealed there to the push
 * service's key. A push carries nothing but "fetch your mail" (PROTOCOL §9), and a ring looks
 * the same as a message: with the app in front its connection already has both; otherwise one
 * quiet notification says there may be something new, and opening it connects and fetches.
 */
public final class SongbirdPushService extends PushService {
    static final String PREFS = "songbird.push";
    private static final String CHANNEL = "songbird.wake";

    @Override
    public void onNewEndpoint(PushEndpoint endpoint, String instance) {
        if (endpoint.getPubKeySet() == null) return;
        byte[] p256dh = Base64.decode(endpoint.getPubKeySet().getPubKey(), Base64.URL_SAFE | Base64.NO_WRAP | Base64.NO_PADDING);
        byte[] auth = Base64.decode(endpoint.getPubKeySet().getAuth(), Base64.URL_SAFE | Base64.NO_WRAP | Base64.NO_PADDING);
        getSharedPreferences(PREFS, MODE_PRIVATE).edit()
                .putString("endpoint", endpoint.getUrl())
                .putString("p256dh", Base64.encodeToString(p256dh, Base64.NO_WRAP))
                .putString("auth", Base64.encodeToString(auth, Base64.NO_WRAP))
                .apply();
        DeviceBridge.pushEndpoint(endpoint.getUrl(), p256dh, auth);
    }

    @Override
    public void onMessage(PushMessage message, String instance) {
        if (DeviceBridge.inFront()) return; // the connection has it already
        NotificationManager nm = getSystemService(NotificationManager.class);
        if (nm == null) return;
        nm.createNotificationChannel(new NotificationChannel(CHANNEL, "New activity",
                NotificationManager.IMPORTANCE_HIGH));
        PendingIntent open = PendingIntent.getActivity(this, 0,
                new Intent(this, AppActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification n = new Notification.Builder(this, CHANNEL)
                .setSmallIcon(android.R.drawable.stat_notify_chat)
                .setContentTitle("Songbird")
                .setContentText("You may have new messages or a call.")
                .setContentIntent(open)
                .setAutoCancel(true)
                .setCategory(Notification.CATEGORY_MESSAGE)
                .build();
        nm.notify(1, n);
    }

    @Override
    public void onRegistrationFailed(FailedReason reason, String instance) {
        clear(this);
    }

    @Override
    public void onUnregistered(String instance) {
        clear(this);
    }

    static void clear(Context c) {
        c.getSharedPreferences(PREFS, MODE_PRIVATE).edit().clear().apply();
    }

    /** The endpoint kept, if any: (url, p256dh, auth). */
    public static Object[] saved(Context c) {
        SharedPreferences p = c.getSharedPreferences(PREFS, MODE_PRIVATE);
        String url = p.getString("endpoint", null);
        if (url == null) return null;
        return new Object[] {url, Base64.decode(p.getString("p256dh", ""), Base64.NO_WRAP),
                Base64.decode(p.getString("auth", ""), Base64.NO_WRAP)};
    }
}
