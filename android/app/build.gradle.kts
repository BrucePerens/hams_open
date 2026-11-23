plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The shared Rust core for arm64-v8a: cargo + the NDK clang linker (ANDROID_NDK_HOME, see daemons/hams_net_core/tools/cross_build.sh),
// copied to jniLibs so the APK carries libhams_net_ffi.so.
val repo = rootProject.projectDir.parentFile
val jniOut = layout.buildDirectory.dir("generated/jniLibs")
val buildNativeArm64 by tasks.registering(Exec::class) {
    val ndk = providers.environmentVariable("ANDROID_NDK_HOME").orElse("/home/bruce/agent_work/tools/android/ndk/28.2.13676358").get()
    val cargoTarget = layout.buildDirectory.dir("native").get().asFile.absolutePath
    workingDir = repo.resolve("daemons/hams_net_ffi")
    environment("CARGO_TARGET_DIR", cargoTarget)
    environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang")
    commandLine("cargo", "build", "--release", "--lib", "--target", "aarch64-linux-android")
    inputs.dir(repo.resolve("daemons/hams_net_ffi/src"))
    inputs.dir(repo.resolve("daemons/hams_net_core/src"))
    outputs.dir(jniOut)
    doLast {
        copy {
            from("$cargoTarget/aarch64-linux-android/release/libhams_net_ffi.so")
            into(jniOut.get().dir("arm64-v8a"))
        }
    }
}

android {
    sourceSets.getByName("main").jniLibs.srcDir(jniOut)
    namespace = "com.hams.net"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.hams.net"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation(project(":core"))
    implementation("com.wireguard.android:tunnel:1.0.20230706")
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach { dependsOn(buildNativeArm64) }
