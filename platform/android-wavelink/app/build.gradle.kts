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
        versionCode = 1
        versionName = "0.0.1"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
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
}

dependencies {
    testImplementation(libs.junit)
}
