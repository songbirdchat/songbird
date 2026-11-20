package chat.songbird.app;

import android.app.Activity;
import android.os.Bundle;

import chat.songbird.camera.CameraBridge;
import chat.songbird.camera.CameraPreview;
import chat.songbird.device.DeviceBridge;
import com.longbridge.nsh.NshView;
import com.longbridge.nsh.SongbirdHost;

/**
 * Songbird's activity (D-0113): NSH's packaged {@code AppActivity}, plus what a packaged app
 * cannot have, the camera's native component and its permission. Everything else, from the
 * window to the Rust root, is as NSH's own activity does it.
 */
public final class SongbirdActivity extends Activity {
    private NshView view;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        SongbirdHost.edgeToEdge(this);
        // Only a development build (debuggable) takes `nsh run`'s environment: the activity is
        // exported, so any app could start a store build with extras.
        if ((getApplicationInfo().flags & android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0) {
            SongbirdHost.applyEnv(getIntent().getExtras());
        }
        view = new NshView(this);
        SongbirdHost.standalone(view);
        CameraBridge.attach(this);
        DeviceBridge.attach(this);
        view.registerComponent("songbird/camera", CameraPreview::new);
        setContentView(view);
        view.handleNotificationIntent(getIntent());
        if (state == null) view.handleUrlSchemeIntent(getIntent());
        view.runRustApp("nsh");
    }

    @Override
    public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        CameraBridge.onPermissionResult(request, results);
        DeviceBridge.onPermissionResult(request, results);
    }

    @Override
    protected void onStart() {
        super.onStart();
        DeviceBridge.setFront(true);
    }

    @Override
    protected void onStop() {
        DeviceBridge.setFront(false);
        CameraBridge.onActivityStopped(this);
        super.onStop();
    }

    @Override
    public void onConfigurationChanged(android.content.res.Configuration config) {
        super.onConfigurationChanged(config);
        SongbirdHost.systemBars(this);
    }

    @Override
    protected void onNewIntent(android.content.Intent intent) {
        super.onNewIntent(intent);
        if (view != null) {
            view.handleNotificationIntent(intent);
            view.handleUrlSchemeIntent(intent);
        }
    }

    /** Android 12 and older: the app gets the back button first. */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (view == null || !view.handleBack()) super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        CameraBridge.detach(this);
        DeviceBridge.detach(this);
        view.destroy();
        super.onDestroy();
    }
}
