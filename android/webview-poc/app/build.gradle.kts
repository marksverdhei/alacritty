plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.alacritty.webview"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.alacritty.webview"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0-poc"
        // arm64-v8a covers every modern phone; x86_64 covers the
        // Android emulator that most contributors will use to test.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    lint {
        // POC-scope: x86_64 absence is intentional (only arm64 binary built);
        // launcher icon left default — both are non-functional polish items
        // that the production app would address.
        disable += listOf("ChromeOsAbiSupport", "MissingApplicationIcon")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // proguard left default; this POC has no user code worth shrinking.
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        jniLibs {
            // The lib*.so file in jniLibs/ is actually our pty server ELF,
            // not a real shared library. useLegacyPackaging = true tells
            // AGP to mark `extractNativeLibs="true"` in the merged manifest
            // so the installer copies the file to nativeLibraryDir/ where
            // we can fork+exec it. Without this, modern AGP keeps the file
            // inside the APK zip and exec() will fail.
            useLegacyPackaging = true
        }
    }
}

dependencies {
    // Pinned to versions that work with AGP 8.11.x + compileSdk 36.
    // androidx.activity 1.13+ requires compileSdk 36; core 1.19+ requires 37.
    // Bumping further means waiting on android-37 platform install.
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("androidx.core:core-ktx:1.16.0")
}
