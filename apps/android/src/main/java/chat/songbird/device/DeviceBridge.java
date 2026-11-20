package chat.songbird.device;

import android.app.Activity;
import android.hardware.biometrics.BiometricManager;
import android.hardware.biometrics.BiometricPrompt;
import android.os.Build;
import android.os.CancellationSignal;
import android.os.Handler;
import android.os.Looper;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;

import java.lang.ref.WeakReference;
import java.security.KeyStore;
import java.util.Arrays;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/**
 * The Android side of Songbird's device services (D-0172), called from Rust
 * ({@code songbird-device}'s {@code android.rs}) on any thread.
 *
 * <ul>
 *   <li>The vault key is wrapped by a non-exportable AES-256 key in the Android Keystore
 *       (StrongBox where the device has it): {@code iv (12) || AES-GCM(key)}. The Keystore key
 *       never leaves its hardware; the wrapped bytes are Rust's to keep.</li>
 *   <li>Biometrics: the platform's BiometricPrompt, strong class only (no androidx, no Play
 *       Services). The app's passcode is the fallback, so the prompt has a cancel button only.</li>
 * </ul>
 */
public final class DeviceBridge {
    private DeviceBridge() {}

    private static final String ALIAS = "chat.songbird.vault";
    private static WeakReference<Activity> activity = new WeakReference<>(null);
    private static final Handler MAIN = new Handler(Looper.getMainLooper());

    /** The activity is up: Rust may call from now on. */
    public static void attach(Activity a) {
        activity = new WeakReference<>(a);
        nativeInit();
    }

    /** The activity is going. */
    public static void detach(Activity a) {
        if (activity.get() == a) activity = new WeakReference<>(null);
    }

    // ------------------------------------------------------------ the vault key

    private static SecretKey keystoreKey() throws Exception {
        KeyStore ks = KeyStore.getInstance("AndroidKeyStore");
        ks.load(null);
        KeyStore.Entry e = ks.getEntry(ALIAS, null);
        if (e instanceof KeyStore.SecretKeyEntry) return ((KeyStore.SecretKeyEntry) e).getSecretKey();
        try {
            return generate(true);
        } catch (Exception noStrongBox) {
            return generate(false);
        }
    }

    private static SecretKey generate(boolean strongBox) throws Exception {
        KeyGenParameterSpec.Builder b = new KeyGenParameterSpec.Builder(ALIAS,
                KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true);
        if (strongBox && Build.VERSION.SDK_INT >= 28) b.setIsStrongBoxBacked(true);
        KeyGenerator g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
        g.init(b.build());
        return g.generateKey();
    }

    /** {@code iv || AES-GCM(key)} under the Keystore key; null on failure. */
    static byte[] wrapKey(byte[] key) {
        try {
            Cipher c = Cipher.getInstance("AES/GCM/NoPadding");
            c.init(Cipher.ENCRYPT_MODE, keystoreKey());
            byte[] iv = c.getIV();
            byte[] ct = c.doFinal(key);
            byte[] out = Arrays.copyOf(iv, iv.length + ct.length);
            System.arraycopy(ct, 0, out, iv.length, ct.length);
            return out;
        } catch (Exception e) {
            return null;
        }
    }

    /** The key {@link #wrapKey} wrapped; null when it doesn't open (another device's, damaged). */
    static byte[] unwrapKey(byte[] wrapped) {
        if (wrapped == null || wrapped.length < 12 + 16) return null;
        try {
            Cipher c = Cipher.getInstance("AES/GCM/NoPadding");
            c.init(Cipher.DECRYPT_MODE, keystoreKey(), new GCMParameterSpec(128, wrapped, 0, 12));
            return c.doFinal(wrapped, 12, wrapped.length - 12);
        } catch (Exception e) {
            return null;
        }
    }

    // ------------------------------------------------------------ biometrics

    /** Whether a strong biometric is enrolled and usable now. */
    @SuppressWarnings("deprecation") // Android 10's canAuthenticate(), before authenticator classes
    static boolean biometrics() {
        Activity a = activity.get();
        if (a == null || Build.VERSION.SDK_INT < 29) return false;
        BiometricManager m = a.getSystemService(BiometricManager.class);
        if (m == null) return false;
        if (Build.VERSION.SDK_INT >= 30) {
            return m.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                    == BiometricManager.BIOMETRIC_SUCCESS;
        }
        return m.canAuthenticate() == BiometricManager.BIOMETRIC_SUCCESS;
    }

    /** Shows the system prompt; the answer goes to {@code nativeAuthResult(id, code)}. */
    static void authenticate(String reason, long id) {
        MAIN.post(() -> {
            Activity a = activity.get();
            if (a == null || Build.VERSION.SDK_INT < 29) {
                nativeAuthResult(id, 3);
                return;
            }
            BiometricPrompt.Builder b = new BiometricPrompt.Builder(a)
                    .setTitle("Unlock Songbird")
                    .setDescription(reason)
                    .setNegativeButton("Use passcode", a.getMainExecutor(),
                            (dialog, which) -> nativeAuthResult(id, 1));
            if (Build.VERSION.SDK_INT >= 30) {
                b.setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG);
            }
            b.setConfirmationRequired(false);
            final boolean[] answered = {false};
            b.build().authenticate(new CancellationSignal(), a.getMainExecutor(),
                    new BiometricPrompt.AuthenticationCallback() {
                        @Override
                        public void onAuthenticationSucceeded(BiometricPrompt.AuthenticationResult r) {
                            if (answered[0]) return;
                            answered[0] = true;
                            nativeAuthResult(id, 0);
                        }

                        @Override
                        public void onAuthenticationError(int code, CharSequence msg) {
                            if (answered[0]) return;
                            answered[0] = true;
                            boolean cancelled = code == BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED
                                    || code == BiometricPrompt.BIOMETRIC_ERROR_CANCELED;
                            boolean locked = code == BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT
                                    || code == BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT_PERMANENT;
                            nativeAuthResult(id, cancelled ? 1 : locked ? 2 : 3);
                        }
                        // onAuthenticationFailed: one try not recognised; the prompt stays.
                    });
        });
    }

    // ------------------------------------------------------------ screen protection

    /** FLAG_SECURE: no screenshots, black recordings, a blank recents thumbnail. */
    static void setSecure(boolean on) {
        MAIN.post(() -> {
            Activity a = activity.get();
            if (a == null) return;
            if (on) {
                a.getWindow().addFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE);
            } else {
                a.getWindow().clearFlags(android.view.WindowManager.LayoutParams.FLAG_SECURE);
            }
        });
    }

    /** Android 13+: a blank thumbnail in the recent apps, screenshots still allowed. */
    static void hideInRecents(boolean on) {
        MAIN.post(() -> {
            Activity a = activity.get();
            if (a != null && Build.VERSION.SDK_INT >= 33) a.setRecentsScreenshotEnabled(!on);
        });
    }

    // ------------------------------------------------------------ location (D-0174)

    static final int LOCATION_REQUEST = 7201;
    private static final java.util.List<Long> WAITING = new java.util.ArrayList<>();

    /** One position for request {@code id}: the permission first if not granted yet. */
    static void locate(long id) {
        MAIN.post(() -> {
            Activity a = activity.get();
            if (a == null) {
                nativeLocation(id, 2, 0, 0, 0);
                return;
            }
            boolean granted = a.checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION)
                    == android.content.pm.PackageManager.PERMISSION_GRANTED
                    || a.checkSelfPermission(android.Manifest.permission.ACCESS_COARSE_LOCATION)
                    == android.content.pm.PackageManager.PERMISSION_GRANTED;
            if (granted) {
                fix(a, id);
                return;
            }
            WAITING.add(id);
            if (WAITING.size() == 1) {
                a.requestPermissions(new String[] {
                        android.Manifest.permission.ACCESS_FINE_LOCATION,
                        android.Manifest.permission.ACCESS_COARSE_LOCATION}, LOCATION_REQUEST);
            }
        });
    }

    /** The activity's permission answers: only the location request is ours. */
    public static void onPermissionResult(int request, int[] results) {
        if (request != LOCATION_REQUEST) return;
        boolean ok = false;
        for (int r : results) ok |= r == android.content.pm.PackageManager.PERMISSION_GRANTED;
        Activity a = activity.get();
        for (long id : new java.util.ArrayList<>(WAITING)) {
            if (ok && a != null) fix(a, id);
            else nativeLocation(id, 1, 0, 0, 0);
        }
        WAITING.clear();
    }

    @SuppressWarnings({"MissingPermission", "deprecation"})
    private static void fix(Activity a, long id) {
        android.location.LocationManager lm = a.getSystemService(android.location.LocationManager.class);
        if (lm == null) {
            nativeLocation(id, 2, 0, 0, 0);
            return;
        }
        String provider = lm.isProviderEnabled(android.location.LocationManager.NETWORK_PROVIDER)
                ? android.location.LocationManager.NETWORK_PROVIDER
                : lm.isProviderEnabled(android.location.LocationManager.GPS_PROVIDER)
                ? android.location.LocationManager.GPS_PROVIDER : null;
        if (provider == null) {
            nativeLocation(id, 2, 0, 0, 0);
            return;
        }
        final boolean[] answered = {false};
        java.util.function.Consumer<android.location.Location> answer = loc -> {
            if (answered[0]) return;
            answered[0] = true;
            if (loc == null) nativeLocation(id, 2, 0, 0, 0);
            else nativeLocation(id, 0, loc.getLatitude(), loc.getLongitude(), loc.getAccuracy());
        };
        // No fix in 30 seconds: unavailable.
        MAIN.postDelayed(() -> answer.accept(null), 30_000);
        try {
            if (Build.VERSION.SDK_INT >= 30) {
                lm.getCurrentLocation(provider, new CancellationSignal(), a.getMainExecutor(), answer::accept);
            } else {
                lm.requestSingleUpdate(provider, new android.location.LocationListener() {
                    @Override
                    public void onLocationChanged(android.location.Location loc) {
                        answer.accept(loc);
                    }
                }, Looper.getMainLooper());
            }
        } catch (SecurityException e) {
            answer.accept(null);
        }
    }

    // ------------------------------------------------------------ photos (D-0174)

    static final int STORAGE_REQUEST = 7202;

    /**
     * Saves a photo or video to the shared Pictures/Movies collections ({@code Songbird} in
     * them). Android 10+: MediaStore, no permission. Android 8 and 9 need storage access: asked
     * here the first time, and this save answers false (the person saves again once allowed).
     */
    static boolean saveMedia(byte[] data, String name, String mime, boolean video) {
        Activity a = activity.get();
        if (a == null) return false;
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                android.content.ContentValues v = new android.content.ContentValues();
                v.put(android.provider.MediaStore.MediaColumns.DISPLAY_NAME, name);
                v.put(android.provider.MediaStore.MediaColumns.MIME_TYPE, mime);
                v.put(android.provider.MediaStore.MediaColumns.RELATIVE_PATH,
                        (video ? android.os.Environment.DIRECTORY_MOVIES : android.os.Environment.DIRECTORY_PICTURES)
                                + "/Songbird");
                v.put(android.provider.MediaStore.MediaColumns.IS_PENDING, 1);
                android.net.Uri collection = video
                        ? android.provider.MediaStore.Video.Media.getContentUri(android.provider.MediaStore.VOLUME_EXTERNAL_PRIMARY)
                        : android.provider.MediaStore.Images.Media.getContentUri(android.provider.MediaStore.VOLUME_EXTERNAL_PRIMARY);
                android.content.ContentResolver r = a.getContentResolver();
                android.net.Uri item = r.insert(collection, v);
                if (item == null) return false;
                try (java.io.OutputStream out = r.openOutputStream(item)) {
                    if (out == null) return false;
                    out.write(data);
                }
                v.clear();
                v.put(android.provider.MediaStore.MediaColumns.IS_PENDING, 0);
                r.update(item, v, null, null);
                return true;
            }
            if (a.checkSelfPermission(android.Manifest.permission.WRITE_EXTERNAL_STORAGE)
                    != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                MAIN.post(() -> a.requestPermissions(
                        new String[] {android.Manifest.permission.WRITE_EXTERNAL_STORAGE}, STORAGE_REQUEST));
                return false;
            }
            @SuppressWarnings("deprecation")
            java.io.File dir = new java.io.File(android.os.Environment.getExternalStoragePublicDirectory(
                    video ? android.os.Environment.DIRECTORY_MOVIES : android.os.Environment.DIRECTORY_PICTURES),
                    "Songbird");
            if (!dir.isDirectory() && !dir.mkdirs()) return false;
            java.io.File f = new java.io.File(dir, name);
            for (int n = 2; f.exists() && n < 1000; n++) f = new java.io.File(dir, n + "-" + name);
            try (java.io.FileOutputStream out = new java.io.FileOutputStream(f)) {
                out.write(data);
            }
            android.media.MediaScannerConnection.scanFile(a, new String[] {f.getPath()}, new String[] {mime}, null);
            return true;
        } catch (Exception e) {
            return false;
        }
    }

    static native void nativeLocation(long id, int code, double lat, double lon, double accuracy);

    static native void nativeInit();

    static native void nativeAuthResult(long id, int code);
}
