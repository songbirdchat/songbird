package chat.songbird.app;

import android.app.Activity;
import android.app.Fragment;
import android.app.FragmentManager;
import android.content.Intent;

import chat.songbird.camera.CameraBridge;
import chat.songbird.device.DeviceBridge;

/**
 * Permission prompts and activity results for the bridges (D-0188). Android answers them only
 * to the activity that asked, which is NSH's {@code AppActivity} now, so a headless fragment of
 * it asks for them and hands the answers to the bridges, as NSH asks for its own. UI thread.
 */
@SuppressWarnings("deprecation") // android.app.Fragment: no androidx in the host
public final class Results extends Fragment {
    private static final String TAG = "chat.songbird.app.Results";

    /** Made by the system again after a configuration change, so the answer still comes. */
    public Results() {}

    /** The fragment of `a`, added now if it has none; null when `a` is going. */
    private static Results of(Activity a) {
        if (a.isFinishing() || a.isDestroyed()) return null;
        FragmentManager fm = a.getFragmentManager();
        Fragment f = fm.findFragmentByTag(TAG);
        if (f instanceof Results) return (Results) f;
        Results r = new Results();
        fm.beginTransaction().add(r, TAG).commitNowAllowingStateLoss();
        return r;
    }

    /** Shows the system's prompt for `permissions`; the answer goes to the bridges. */
    public static void ask(Activity a, String[] permissions, int request) {
        Results r = of(a);
        if (r == null) {
            // Nobody to ask through: as if the prompt was dismissed.
            answer(request, new int[0]);
            return;
        }
        r.requestPermissions(permissions, request);
    }

    /** Starts `intent` for a result; the result goes to the bridges. */
    public static void start(Activity a, Intent intent, int request) {
        Results r = of(a);
        if (r == null) {
            DeviceBridge.onActivityResult(request, Activity.RESULT_CANCELED, null);
            return;
        }
        r.startActivityForResult(intent, request);
    }

    @Override
    public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        answer(request, results);
    }

    @Override
    public void onActivityResult(int request, int result, Intent data) {
        DeviceBridge.onActivityResult(request, result, data);
    }

    /** Each bridge takes only its own request codes. */
    private static void answer(int request, int[] results) {
        CameraBridge.onPermissionResult(request, results);
        DeviceBridge.onPermissionResult(request, results);
    }
}
