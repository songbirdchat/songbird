plugins {
    id("com.android.application") version "9.5.1"
}

// NSH's Java sources, of the exact nsh-platform-android the app's Rust links (build.sh finds
// them with `cargo metadata` and passes `-PnshJava=<dir>`); the Rust library and the app's
// assets come from `nsh package --target android` (build.sh unpacks them into build/rust).
val nshJava = providers.gradleProperty("nshJava").orNull
    ?: error("pass -PnshJava=<nsh-platform-android>/java (build.sh does)")

android {
    namespace = "chat.songbird.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "chat.songbird.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    sourceSets {
        getByName("main") {
            java.directories.add(nshJava)
            jniLibs.directories.add("build/rust/jniLibs")
            assets.directories.add("build/rust/assets")
        }
    }

    packaging {
        // NSH reads its libraries from the APK as packaged apps do.
        jniLibs.useLegacyPackaging = true
    }

    lint {
        abortOnError = true
        checkDependencies = false
        // Back: Android 13 and later go through NSH's OnBackInvokedCallback
        // (`enableOnBackInvokedCallback`); `onBackPressed` serves Android 12 and older only, in
        // Songbird's activity as in NSH's own.
        disable += "GestureBackNavigation"
    }
}

dependencies {
    // CameraX (Apache-2.0, AndroidX): on the device, no Play Services, no ML Kit.
    val camerax = "1.6.2"
    implementation("androidx.camera:camera-core:$camerax")
    implementation("androidx.camera:camera-camera2:$camerax")
    implementation("androidx.camera:camera-lifecycle:$camerax")
    implementation("androidx.camera:camera-view:$camerax")
    // UnifiedPush (Apache-2.0): pushes through the user's own distributor, no Google services
    // (D-0176). Brings kotlin-stdlib and Tink (its Web Push decryption).
    implementation("org.unifiedpush.android:connector:3.3.6")
    // Google Play Billing (D-0178): Songbird's plans bought in Play; the purchase token goes to
    // Songbird's billing service, which verifies it with Google.
    implementation("com.android.billingclient:billing:9.1.0")
}
