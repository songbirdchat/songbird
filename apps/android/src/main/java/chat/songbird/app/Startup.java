package chat.songbird.app;

import android.app.Activity;
import android.app.Application;
import android.content.ContentProvider;
import android.content.ContentValues;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;

import chat.songbird.camera.CameraBridge;
import chat.songbird.device.DeviceBridge;

/**
 * Songbird's start-up (D-0188). NSH 0.7's own {@code AppActivity} runs the app (it keeps the
 * running app across activity recreation, takes shares and owns the system bars) and the
 * camera's views register as Rust components; what Songbird's own activity did besides (D-0113)
 * is here: the camera and device bridges follow the activity in front. A content provider, as
 * NSH's docs say for work at start-up (an NSH app has no {@code Application} class of its own);
 * it serves no data. Permission answers and activity results come to {@link Results}.
 */
public final class Startup extends ContentProvider {
    /** The activity the bridges were given last. UI thread. */
    private static Activity current;

    @Override
    public boolean onCreate() {
        Application app = (Application) getContext().getApplicationContext();
        app.registerActivityLifecycleCallbacks(new Application.ActivityLifecycleCallbacks() {
            @Override
            public void onActivityCreated(Activity a, Bundle state) {
                attach(a);
            }

            @Override
            public void onActivityStarted(Activity a) {
                attach(a);
                DeviceBridge.setFront(true);
            }

            @Override
            public void onActivityResumed(Activity a) {}

            @Override
            public void onActivityPaused(Activity a) {}

            @Override
            public void onActivityStopped(Activity a) {
                DeviceBridge.setFront(false);
                CameraBridge.onActivityStopped(a);
            }

            @Override
            public void onActivitySaveInstanceState(Activity a, Bundle state) {}

            @Override
            public void onActivityDestroyed(Activity a) {
                CameraBridge.detach(a);
                DeviceBridge.detach(a);
                if (current == a) current = null;
            }
        });
        return true;
    }

    /**
     * Gives the bridges `a` once, as it is created: before NSH's activity starts the Rust app,
     * as Songbird's own activity did. Their native halves live in NSH's library, which the
     * activity loads a moment later: loaded here first (loading it again is nothing), and only
     * once an activity runs, so a push alone doesn't load it.
     */
    private static void attach(Activity a) {
        if (current == a) return;
        current = a;
        System.loadLibrary("nsh");
        CameraBridge.attach(a);
        DeviceBridge.attach(a);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] args, String order) {
        return null;
    }

    @Override
    public String getType(Uri uri) {
        return null;
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] args) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] args) {
        return 0;
    }
}
