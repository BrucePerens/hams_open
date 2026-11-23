plugins {
    id("org.jetbrains.kotlin.jvm")
}


dependencies {
    implementation("org.json:json:20240303")
    testImplementation(kotlin("test"))
}

tasks.test { useJUnitPlatform() }
