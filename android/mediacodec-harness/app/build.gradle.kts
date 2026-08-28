plugins {
    id("com.android.application")
}

abstract class BuildRustTask : Exec() {
    @get:OutputDirectory
    abstract val outputDirectory: DirectoryProperty
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

}

val buildRust by tasks.registering(BuildRustTask::class) {
    inputs.files(
        fileTree("../../..") {
            include("Cargo.toml", "Cargo.lock")
            include("crates/rawshift-hwdec/src/**")
            include("crates/rawshift-hwdec/Cargo.toml")
            include("android/mediacodec-harness/native/**")
            include("android/mediacodec-harness/fixtures/data/**")
            exclude("android/mediacodec-harness/native/target/**")
        }
    )
    outputDirectory.set(layout.buildDirectory.dir("rust-jni-libs"))
    doFirst {
        commandLine(
            "bash",
            rootProject.file("build-rust.sh").absolutePath,
            outputDirectory.get().dir("arm64-v8a").asFile.absolutePath,
        )
    }
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(
            buildRust,
            BuildRustTask::outputDirectory,
        )
    }
}

dependencies {
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test:runner:1.7.0")
}
