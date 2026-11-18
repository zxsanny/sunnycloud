import java.util.Properties

plugins {
    id("com.android.application")
    id("dev.flutter.flutter-gradle-plugin")
}

// The Rust library of every ABI is built by scripts/ci/mobile-build-native.sh into app/src/main/jniLibs/<abi>/.
val nativeAbis = listOf("arm64-v8a", "armeabi-v7a", "x86_64")
val nativeLibrary = "libmobile_backup.so"

// Release signing: android/key.properties (gitignored; keys are the owner's, R18) names storeFile, storePassword,
// keyAlias and keyPassword. Without the file the release build is signed with the debug key and is not shippable.
val keyProperties = Properties().apply {
    val file = rootProject.file("key.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}
val hasReleaseKey = keyProperties.containsKey("storeFile")

android {
    namespace = "org.sunnycloud.backup"
    compileSdk = 35

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "org.sunnycloud.backup"
        minSdk = 30
        targetSdk = 35
        versionCode = flutter.versionCode
        versionName = flutter.versionName
        ndk { abiFilters += nativeAbis }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        if (hasReleaseKey) {
            create("release") {
                storeFile = rootProject.file(keyProperties.getProperty("storeFile"))
                storePassword = keyProperties.getProperty("storePassword")
                keyAlias = keyProperties.getProperty("keyAlias")
                keyPassword = keyProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            proguardFiles("proguard-rules.pro")
            signingConfig = signingConfigs.getByName(if (hasReleaseKey) "release" else "debug")
        }
    }

    // Native libraries stay page-aligned and uncompressed in the APK (extractNativeLibs=false in the manifest).
    packaging { jniLibs { useLegacyPackaging = false } }

    testOptions {
        unitTests.all {
            // The JVM smoke test loads the host build of the same Rust library through JNA.
            val hostLibDir = providers.gradleProperty("nativeHostLibDir")
                .orElse(rootProject.file("../../target/debug").path)
            it.systemProperty("jna.library.path", hostLibDir.get())
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    testImplementation("net.java.dev.jna:jna:5.17.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
}

// An APK without the library would start and fail at the first call, so a missing library fails the build.
val verifyNativeLibraries by tasks.registering {
    val jniLibs = layout.projectDirectory.dir("src/main/jniLibs")
    doLast {
        val missing = nativeAbis.filter { !jniLibs.file("$it/$nativeLibrary").asFile.isFile }
        check(missing.isEmpty()) {
            "$nativeLibrary is missing for ${missing.joinToString()}; run scripts/ci/mobile-build-native.sh first"
        }
    }
}
tasks.matching { it.name == "preBuild" }.configureEach { dependsOn(verifyNativeLibraries) }
