plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "dev.wavelink.app"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.wavelink.app"
        minSdk = 29
        targetSdk = 34
        versionCode = 3
        versionName = "0.0.3"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        // WS-G (BRIDGE_PLAN step 1): the Rust bridge ships arm64-v8a /
        // armeabi-v7a / x86_64 in jniLibs (produced by the cargoNdkBuild task);
        // the ABIs are pinned so packaging is deterministic.
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        // "17", not JvmTarget.JVM_17.toString(): the latter yields "JVM_17",
        // which the Kotlin Gradle plugin rejects ("Unknown Kotlin JVM target").
        jvmTarget = "17"
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
}

// WS-G (BRIDGE_PLAN step 1): build libwdr_bridge.so for the pinned ABIs via
// cargo-ndk into src/main/jniLibs. Skips with a warning when no NDK /
// cargo-ndk is available, so host `assembleDebug` builds green without them;
// the android-ci release job provides both and produces the real .so.
val cargoNdkBuild = tasks.register<Exec>("cargoNdkBuild") {
    val ndkHomeSet = !System.getenv("ANDROID_NDK_HOME").isNullOrEmpty()
            || !System.getenv("NDK_HOME").isNullOrEmpty()
    val cargoNdkPresent = runCatching {
        ProcessBuilder("cargo", "ndk", "--version")
            .redirectErrorStream(true)
            .start()
            .let { it.waitFor(); it.exitValue() == 0 }
    }.getOrDefault(false)
    enabled = ndkHomeSet && cargoNdkPresent
    doFirst {
        if (!enabled) {
            logger.warn(
                "cargoNdkBuild skipped (cargo-ndk=$cargoNdkPresent, NDK env=$ndkHomeSet). " +
                        "Host assembleDebug ships without libwdr_bridge.so; the android-ci " +
                        "gate builds it (WdrEngineLoader tolerates its absence)."
            )
        }
    }
    if (enabled) {
        workingDir = rootProject.projectDir.resolve("../../")
        commandLine(
            "cargo", "ndk",
            "-t", "arm64-v8a", "-t", "armeabi-v7a", "-t", "x86_64",
            "-o", "${projectDir}/src/main/jniLibs",
            "build", "--release", "-p", "wdr_bridge",
        )
        // The ff bridge .so crosses cmake for vendored libopus/libFLAC (R08).
        // A failure must NOT fail the whole Android release — the app stays
        // functional without the .so (uses-native-library required=false +
        // WdrEngineLoader). Warn loudly instead so the android-ci gate is still
        // visible, not silent.
        isIgnoreExitValue = true
        doLast {
            if (executionResult.get().exitValue != 0) {
                logger.warn(
                    "cargoNdkBuild produced no libwdr_bridge.so (cargo-ndk/cmake " +
                            "failed — R08). The APK is built without the Rust bridge; " +
                            "the android-ci/device gate must land it before any " +
                            "streaming claim."
                )
            }
        }
    }
}
tasks.named("preBuild") {
    dependsOn(cargoNdkBuild)
}

dependencies {
    testImplementation(libs.junit)
}
