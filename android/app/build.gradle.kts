/*
 * MIT License
 *
 * Copyright © 2026 Yume-13
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "app.catlink"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.catlink"
        minSdk = 26
        targetSdk = 35
        // -PcatlinkVersion=MAJOR.MINOR.PATCH; versionCode = MAJOR*10000 + MINOR*100 + PATCH.
        val version = (project.findProperty("catlinkVersion") as String?) ?: "0.1.0"
        val (major, minor, patch) = version.split(".").map { it.toInt() }
        versionCode = major * 10000 + minor * 100 + patch
        versionName = version
    }

    // Release key from the environment (CATLINK_KEYSTORE = path to the keystore file);
    // without it, release builds are signed with the debug key.
    val releaseKeystore = System.getenv("CATLINK_KEYSTORE")?.let { file(it) }?.takeIf { it.isFile }
    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = System.getenv("CATLINK_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("CATLINK_KEY_ALIAS")
                keyPassword = System.getenv("CATLINK_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }
    buildFeatures {
        compose = true
    }
    packaging {
        resources.excludes += "DebugProbesKt.bin"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

// Writes the libraries packaged into the release APK (group:name:version), one per line.
tasks.register("listReleaseLibraries") {
    val out = layout.buildDirectory.file("outputs/release-libraries.txt")
    outputs.file(out)
    doLast {
        val ids = configurations.getByName("releaseRuntimeClasspath").incoming.resolutionResult.allComponents
            .mapNotNull { it.moduleVersion }
            .filter { it.version != "unspecified" && !it.name.endsWith("-bom") }
            .map { "${it.group}:${it.name}:${it.version}" }
            .sorted()
        out.get().asFile.writeText(ids.joinToString("\n") + "\n")
    }
}
