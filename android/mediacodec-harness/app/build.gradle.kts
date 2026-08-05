plugins {
    id("com.android.application")
}

android {
    namespace = "org.visualcommons.rawshift.harness"
    compileSdk = 36
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "org.visualcommons.rawshift.harness"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    sourceSets.getByName("main").jniLibs.srcDir(layout.buildDirectory.dir("rust-jni-libs"))
}

val buildRust by tasks.registering(Exec::class) {
    val output = layout.buildDirectory.dir("rust-jni-libs/arm64-v8a")
    inputs.files(
        fileTree("../../..") {
            include("Cargo.toml", "Cargo.lock")
            include("crates/rawshift-hwdec/src/**")
            include("crates/rawshift-hwdec/Cargo.toml")
            include("android/mediacodec-harness/native/**")
            include("android/mediacodec-harness/fixtures/data/**")
        }
    )
    outputs.dir(output)
    commandLine(
        "bash",
        rootProject.file("build-rust.sh").absolutePath,
        output.get().asFile.absolutePath,
    )
}

tasks.named("preBuild").configure {
    dependsOn(buildRust)
}

dependencies {
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
