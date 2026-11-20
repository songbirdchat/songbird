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
                                    || code == BiometricPrompt.BIOMETRIC_ERROR_CANCELED
                                    || code == BiometricPrompt.BIOMETRIC_ERROR_NEGATIVE_BUTTON;
                            boolean locked = code == BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT
                                    || code == BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT_PERMANENT;
                            nativeAuthResult(id, cancelled ? 1 : locked ? 2 : 3);
                        }
                        // onAuthenticationFailed: one try not recognised; the prompt stays.
                    });
        });
    }

    static native void nativeInit();

    static native void nativeAuthResult(long id, int code);
}
