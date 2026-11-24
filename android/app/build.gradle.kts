plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// The shared Rust core for arm64-v8a: cargo + the NDK clang linker (ANDROID_NDK_HOME, see daemons/hams_net_core/tools/cross_build.sh),
// copied to jniLibs so the APK carries libhams_net_ffi.so.
val repo = rootProject.projectDir.parentFile
val jniOut = layout.buildDirectory.dir("generated/jniLibs")
val abis = mapOf("arm64-v8a" to "aarch64-linux-android", "x86_64" to "x86_64-linux-android")
val buildNative by tasks.registering(Exec::class) {
    val ndk = providers.environmentVariable("ANDROID_NDK_HOME").orElse("/home/bruce/agent_work/tools/android/ndk/28.2.13676358").get()
    val cargoTarget = layout.buildDirectory.dir("native").get().asFile.absolutePath
    val bin = "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
    workingDir = repo.resolve("daemons/hams_net_ffi")
    environment("CARGO_TARGET_DIR", cargoTarget)
    environment("CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER", "$bin/aarch64-linux-android24-clang")
    environment("CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER", "$bin/x86_64-linux-android24-clang")
    for ((_, triple) in abis) {
        val api = "$bin/${triple}24-clang"
        environment("CC_${triple.replace('-', '_')}", api)
        environment("AR_${triple.replace('-', '_')}", "$bin/llvm-ar")
    }
    commandLine("sh", "-c", abis.values.joinToString(" && ") { "cargo build --release --lib --target $it" })
    inputs.dir(repo.resolve("daemons/hams_net_ffi/src"))
    inputs.dir(repo.resolve("daemons/hams_net_core/src"))
    outputs.dir(jniOut)
    doLast {
        for ((abi, triple) in abis) {
            copy {
                from("$cargoTarget/$triple/release/libhams_net_ffi.so")
                into(jniOut.get().dir(abi))
            }
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
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
}

dependencies {
    implementation(project(":core")) { exclude(group = "net.java.dev.jna") }
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("org.jetbrains.kotlin:kotlin-test-junit:2.0.21")
    androidTestImplementation("net.java.dev.jna:jna:5.14.0@aar")
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    implementation("com.github.mik3y:usb-serial-for-android:3.8.1")
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach { dependsOn(buildNative) }
