package chat.songbird.camera;

import android.content.Context;
import android.view.View;

import androidx.camera.view.PreviewView;

/**
 * The native component {@code songbird/camera}: CameraX's {@link PreviewView} showing the
 * session its {@code session} prop names (D-0113). Made and driven by the Rust component the
 * camera crate registers with NSH (D-0188). UI thread.
 */
public final class CameraPreview {
    private final PreviewView view;

    public CameraPreview(Context context) {
        view = new PreviewView(context);
        view.setScaleType(PreviewView.ScaleType.FILL_CENTER);
        // A TextureView: it clips to the rounded box NSH lays out, which a SurfaceView does not.
        view.setImplementationMode(PreviewView.ImplementationMode.COMPATIBLE);
    }

    public View view() {
        return view;
    }

    /** The {@code session} prop; 0 or less shows none. */
    public void setSession(long id) {
        CameraBridge.attachPreview(Math.max(id, 0), view);
    }

    public void destroy() {
        CameraBridge.detachPreview(view);
    }
}
