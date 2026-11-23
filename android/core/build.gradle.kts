plugins {
    id("org.jetbrains.kotlin.jvm")
}

// The shared Rust core (daemons/hams_net_core) through UniFFI: build the library and generate the Kotlin bindings.
val repo = rootProject.projectDir.parentFile
val nativeDir = layout.buildDirectory.dir("native")
val bindingsDir = layout.buildDirectory.dir("generated/uniffi/kotlin")
val cargo = providers.environmentVariable("CARGO").orElse("cargo")

val buildNative by tasks.registering(Exec::class) {
    workingDir = repo.resolve("daemons/hams_net_ffi")
    environment("CARGO_TARGET_DIR", nativeDir.get().asFile.absolutePath)
    commandLine(cargo.get(), "build", "--bins", "--lib")
    inputs.dir(repo.resolve("daemons/hams_net_ffi/src"))
    inputs.dir(repo.resolve("daemons/hams_net_core/src"))
    outputs.dir(nativeDir)
}

val generateBindings by tasks.registering(Exec::class) {
    dependsOn(buildNative)
    workingDir = repo.resolve("daemons/hams_net_ffi")
    commandLine(
        nativeDir.get().file("debug/uniffi-bindgen").asFile.absolutePath, "generate",
        "--library", nativeDir.get().file("debug/libhams_net_ffi.so").asFile.absolutePath,
        "--language", "kotlin", "--out-dir", bindingsDir.get().asFile.absolutePath,
    )
    inputs.file(nativeDir.get().file("debug/libhams_net_ffi.so"))
    outputs.dir(bindingsDir)
}

sourceSets.main { kotlin.srcDir(generateBindings.map { bindingsDir }) }

dependencies {
    implementation("org.json:json:20240303")
    implementation("net.java.dev.jna:jna:5.14.0")
    testImplementation(kotlin("test"))
}

tasks.test {
    useJUnitPlatform()
    systemProperty("jna.library.path", nativeDir.get().dir("debug").asFile.absolutePath)
}
