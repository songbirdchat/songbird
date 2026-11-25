package chat.songbird.camera;

import android.Manifest;
import android.app.Activity;
import android.app.admin.DevicePolicyManager;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.hardware.camera2.CameraAccessException;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraManager;
import android.os.Handler;
import android.os.Looper;
import android.util.Size;

import androidx.annotation.NonNull;
import androidx.camera.camera2.interop.Camera2CameraInfo;
import androidx.camera.camera2.interop.ExperimentalCamera2Interop;
import androidx.camera.core.Camera;
import androidx.camera.core.CameraSelector;
import androidx.camera.core.CameraState;
import androidx.camera.core.ImageAnalysis;
import androidx.camera.core.ImageCapture;
import androidx.camera.core.ImageCaptureException;
import androidx.camera.core.ImageProxy;
import androidx.camera.core.Preview;
import androidx.camera.core.UseCase;
import androidx.camera.core.resolutionselector.ResolutionSelector;
import androidx.camera.core.resolutionselector.ResolutionStrategy;
import androidx.camera.lifecycle.ProcessCameraProvider;
import androidx.camera.view.PreviewView;
import androidx.lifecycle.Lifecycle;
import androidx.lifecycle.LifecycleOwner;
import androidx.lifecycle.LifecycleRegistry;

import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/**
 * The Android side of Songbird's camera (D-0110, D-0113): CameraX, open source, on the device,
 * with no Google Play Services and no ML Kit. Rust ({@code songbird-camera}'s {@code android.rs})
 * calls the static methods from any thread; they hop to the main thread, where CameraX binds.
 * Results go back through the native methods, implemented in Rust.
 *
 * <p>Nothing here keeps a frame or a photo: frames are handed to Rust and closed, photos are
 * handed over as bytes in memory. QR codes are decoded in Rust ({@code rqrr}).
 */
public final class CameraBridge {
    private CameraBridge() {}

    /** Access, as Rust's {@code Access}: not asked, granted, denied, restricted. */
    static final int NOT_ASKED = 0, GRANTED = 1, DENIED = 2, RESTRICTED = 3;
    /** Why a camera cannot run, as Rust's {@code Unavailable}. */
    static final int NO_CAMERA = 0, IN_USE = 1, INTERRUPTED = 2, SYSTEM_OFF = 3, FAILED = 4;
    /** Purposes, as Rust's {@code Purpose}. */
    static final int SCAN = 0, PHOTO = 1, VIDEO = 2, CALL = 3;

    private static final int REQUEST_CAMERA = 0x5B;
    private static final String PREFS = "songbird.camera";
    private static final Handler MAIN = new Handler(Looper.getMainLooper());
    /** Analysis frames: one thread, so a slow decode drops frames rather than queueing them. */
    private static final ExecutorService ANALYSIS = Executors.newSingleThreadExecutor();

    /** The activity, weakly: it may go while Rust still holds a session. */
    private static java.lang.ref.WeakReference<Activity> activity = new java.lang.ref.WeakReference<>(null);
    /** The application's context: lives as long as the process, so a static holds no leak. */
    @android.annotation.SuppressLint("StaticFieldLeak")
    private static Context app;
    /** Changed on the main thread; read from Rust's threads too (the torch). */
    private static final Map<Long, Session> SESSIONS = new ConcurrentHashMap<>();
    private static final Map<Long, PreviewView> PREVIEWS = new ConcurrentHashMap<>(); // main thread only

    // ------------------------------------------------------------ the activity

    /** The activity is up: Rust may call from now on. */
    public static void attach(Activity a) {
        activity = new java.lang.ref.WeakReference<>(a);
        app = a.getApplicationContext();
        nativeInit();
    }

    /** The activity is going: every session stops. */
    public static void detach(Activity a) {
        if (activity.get() != a) return;
        for (Session s : new ArrayList<>(SESSIONS.values())) s.close();
        SESSIONS.clear();
        activity = new java.lang.ref.WeakReference<>(null);
    }

    /**
     * The activity stopped (another app is in front, the screen is off): every camera stops,
     * whatever Rust asked for. Rust's own rule (no camera in the background) starts them again
     * when the app returns.
     */
    public static void onActivityStopped(Activity a) {
        if (activity.get() != a) return;
        for (Session s : new ArrayList<>(SESSIONS.values())) s.close();
        SESSIONS.clear();
    }

    /** The permission dialog answered. */
    public static void onPermissionResult(int request, int[] results) {
        if (request != REQUEST_CAMERA) return;
        nativeAccess(access());
    }

    // ------------------------------------------------------------ called from Rust

    /** Whether the app may use the camera; never prompts. */
    public static int access() {
        Context c = app;
        if (c == null) return RESTRICTED;
        DevicePolicyManager dpm = (DevicePolicyManager) c.getSystemService(Context.DEVICE_POLICY_SERVICE);
        if (dpm != null && dpm.getCameraDisabled(null)) return RESTRICTED;
        if (c.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) return GRANTED;
        if (!prefs(c).getBoolean("asked", false)) return NOT_ASKED;
        // Asked before: Android lets the app ask again until "Don't allow" twice; after that,
        // only Settings can change it.
        Activity a = activity.get();
        if (a != null && a.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)) return NOT_ASKED;
        return DENIED;
    }

    /** Shows the system's dialog; the answer comes back through {@link #nativeAccess}. */
    public static void requestAccess() {
        MAIN.post(() -> {
            Activity a = activity.get();
            if (a == null) {
                nativeAccess(DENIED);
                return;
            }
            prefs(a).edit().putBoolean("asked", true).apply();
            chat.songbird.app.Results.ask(a, new String[] {Manifest.permission.CAMERA}, REQUEST_CAMERA);
        });
    }

    /** The cameras, one per line: {@code id \t facing \t name}; facing 0 front, 1 back, 2 external. */
    public static String lenses() {
        Context c = app;
        if (c == null) return "";
        CameraManager manager = (CameraManager) c.getSystemService(Context.CAMERA_SERVICE);
        StringBuilder out = new StringBuilder();
        try {
            for (String id : manager.getCameraIdList()) {
                Integer facing = manager.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING);
                int f = facing == null ? 2 : facing == CameraCharacteristics.LENS_FACING_FRONT ? 0
                        : facing == CameraCharacteristics.LENS_FACING_BACK ? 1 : 2;
                String name = f == 0 ? "Front camera" : f == 1 ? "Back camera" : "External camera";
                out.append(id.replace('\t', ' ').replace('\n', ' ')).append('\t').append(f).append('\t')
                        .append(name).append('\n');
            }
        } catch (CameraAccessException | RuntimeException e) {
            return "";
        }
        return out.toString();
    }

    /** Starts session {@code id}; it reports through {@link #nativeRunning} and {@link #nativeUnavailable}. */
    public static void open(long id, int purpose, String lens) {
        MAIN.post(() -> {
            if (app == null) {
                nativeUnavailable(id, FAILED);
                return;
            }
            Session s = new Session(id, purpose, lens);
            SESSIONS.put(id, s);
            s.start();
        });
    }

    /** Stops session {@code id}. */
    public static void close(long id) {
        MAIN.post(() -> {
            Session s = SESSIONS.remove(id);
            if (s != null) s.close();
        });
    }

    /** Switches session {@code id} to another camera. */
    public static void setLens(long id, String lens) {
        MAIN.post(() -> {
            Session s = SESSIONS.get(id);
            if (s != null) {
                s.lens = lens;
                s.bind();
            }
        });
    }

    /** Turns the torch on or off; false when the camera has none, or is not bound yet. */
    public static boolean setTorch(long id, boolean on) {
        Session session = SESSIONS.get(id);
        if (session == null || !session.hasTorch) return false;
        MAIN.post(() -> {
            Session s = SESSIONS.get(id);
            if (s != null && s.camera != null) s.camera.getCameraControl().enableTorch(on);
        });
        return true;
    }

    /** Takes a photo; the JPEG comes back through {@link #nativePhoto} (null on failure). */
    public static void takePhoto(long id) {
        MAIN.post(() -> {
            Session s = SESSIONS.get(id);
            if (s == null || s.capture == null) {
                nativePhoto(id, null);
                return;
            }
            s.capture.takePicture(ANALYSIS, new ImageCapture.OnImageCapturedCallback() {
                @Override
                public void onCaptureSuccess(@NonNull ImageProxy image) {
                    byte[] jpeg = null;
                    try {
                        ByteBuffer b = image.getPlanes()[0].getBuffer();
                        jpeg = new byte[b.remaining()];
                        b.get(jpeg);
                    } finally {
                        image.close();
                    }
                    nativePhoto(id, jpeg);
                }

                @Override
                public void onError(@NonNull ImageCaptureException e) {
                    nativePhoto(id, null);
                }
            });
        });
    }

    // ------------------------------------------------------------ the preview component

    /** A preview shows session {@code id} (or nothing for 0). */
    static void attachPreview(long id, PreviewView view) {
        PREVIEWS.values().remove(view);
        if (id != 0) PREVIEWS.put(id, view);
        Session s = SESSIONS.get(id);
        if (s != null) s.showIn(view);
    }

    static void detachPreview(PreviewView view) {
        PREVIEWS.values().remove(view);
    }

    // ------------------------------------------------------------ implemented in Rust

    /** A call's video frame from Rust (D-0183). */
    static void videoFrame(long feed, int width, int height, java.nio.ByteBuffer rgba) {
        VideoFrameView.frame(feed, width, height, rgba);
    }

    static void videoClear(long feed) {
        VideoFrameView.clear(feed);
    }

    static native void nativeInit();

    static native void nativeAccess(int access);

    static native void nativeRunning(long id, String lens);

    static native void nativeUnavailable(long id, int why);

    /** A YUV_420_888 frame: the planes are only valid during the call. */
    static native void nativeFrame(long id, ByteBuffer y, int yStride, ByteBuffer u, ByteBuffer v, int uvStride,
            int uvPixelStride, int width, int height, int rotation, long timestampNanos);

    static native void nativePhoto(long id, byte[] jpeg);

    // ------------------------------------------------------------ sessions

    private static SharedPreferences prefs(Context c) {
        return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    /** One camera use, with a lifecycle of its own: it runs from {@code start} to {@code close}. */
    private static final class Session implements LifecycleOwner {
        final long id;
        final int purpose;
        String lens;
        final LifecycleRegistry lifecycle = new LifecycleRegistry(this);
        ProcessCameraProvider provider;
        Camera camera;
        Preview preview;
        ImageCapture capture;
        /** This session's use cases: only these are unbound, never another session's. */
        UseCase[] bound = new UseCase[0];
        /** Read on the analysis thread. */
        volatile boolean closed;
        volatile boolean hasTorch;

        Session(long id, int purpose, String lens) {
            this.id = id;
            this.purpose = purpose;
            this.lens = lens;
        }

        @NonNull
        @Override
        public Lifecycle getLifecycle() {
            return lifecycle;
        }

        void start() {
            lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_CREATE);
            lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_START);
            lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_RESUME);
            var future = ProcessCameraProvider.getInstance(app);
            future.addListener(() -> {
                if (closed) return;
                try {
                    provider = future.get();
                } catch (Exception e) {
                    nativeUnavailable(id, FAILED);
                    return;
                }
                bind();
            }, MAIN::post);
        }

        @ExperimentalCamera2Interop
        private CameraSelector selector() {
            return new CameraSelector.Builder()
                    .addCameraFilter(infos -> {
                        List<androidx.camera.core.CameraInfo> out = new ArrayList<>();
                        for (var info : infos) {
                            if (Camera2CameraInfo.from(info).getCameraId().equals(lens)) out.add(info);
                        }
                        return out;
                    })
                    .build();
        }

        @androidx.annotation.OptIn(markerClass = ExperimentalCamera2Interop.class)
        void bind() {
            if (provider == null || closed) return;
            provider.unbind(bound);
            List<UseCase> cases = new ArrayList<>();
            preview = new Preview.Builder().build();
            cases.add(preview);
            ResolutionSelector hd = new ResolutionSelector.Builder()
                    .setResolutionStrategy(new ResolutionStrategy(new Size(1280, 720),
                            ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER))
                    .build();
            if (purpose == SCAN || purpose == CALL) {
                ImageAnalysis analysis = new ImageAnalysis.Builder()
                        .setResolutionSelector(hd)
                        .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                        .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_YUV_420_888)
                        .build();
                analysis.setAnalyzer(ANALYSIS, this::frame);
                cases.add(analysis);
            }
            if (purpose == PHOTO) {
                capture = new ImageCapture.Builder()
                        .setCaptureMode(ImageCapture.CAPTURE_MODE_MINIMIZE_LATENCY)
                        .build();
                cases.add(capture);
            }
            try {
                bound = cases.toArray(new UseCase[0]);
                camera = provider.bindToLifecycle(this, selector(), bound);
                hasTorch = camera.getCameraInfo().hasFlashUnit();
            } catch (IllegalArgumentException e) {
                nativeUnavailable(id, NO_CAMERA);
                return;
            } catch (RuntimeException e) {
                nativeUnavailable(id, FAILED);
                return;
            }
            PreviewView view = PREVIEWS.get(id);
            if (view != null) showIn(view);
            camera.getCameraInfo().getCameraState().observe(this, state -> {
                CameraState.StateError error = state.getError();
                if (error != null) {
                    nativeUnavailable(id, why(error.getCode()));
                } else if (state.getType() == CameraState.Type.OPEN) {
                    nativeRunning(id, lens);
                }
            });
        }

        void showIn(PreviewView view) {
            if (preview != null) preview.setSurfaceProvider(view.getSurfaceProvider());
        }

        private void frame(ImageProxy image) {
            try {
                if (closed) return;
                ImageProxy.PlaneProxy[] p = image.getPlanes();
                nativeFrame(id, p[0].getBuffer(), p[0].getRowStride(), p[1].getBuffer(), p[2].getBuffer(),
                        p[1].getRowStride(), p[1].getPixelStride(), image.getWidth(), image.getHeight(),
                        image.getImageInfo().getRotationDegrees(), image.getImageInfo().getTimestamp());
            } finally {
                image.close();
            }
        }

        void close() {
            closed = true;
            if (provider != null) provider.unbind(bound);
            bound = new UseCase[0];
            if (lifecycle.getCurrentState().isAtLeast(Lifecycle.State.RESUMED)) {
                lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_PAUSE);
                lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_STOP);
                lifecycle.handleLifecycleEvent(Lifecycle.Event.ON_DESTROY);
            }
            camera = null;
            preview = null;
            capture = null;
        }

        private static int why(int code) {
            switch (code) {
                case CameraState.ERROR_CAMERA_IN_USE:
                case CameraState.ERROR_MAX_CAMERAS_IN_USE:
                    return IN_USE;
                case CameraState.ERROR_CAMERA_DISABLED:
                case CameraState.ERROR_DO_NOT_DISTURB_MODE_ENABLED:
                    return SYSTEM_OFF;
                case CameraState.ERROR_CAMERA_REMOVED:
                    return NO_CAMERA;
                default:
                    return FAILED;
            }
        }
    }
}
