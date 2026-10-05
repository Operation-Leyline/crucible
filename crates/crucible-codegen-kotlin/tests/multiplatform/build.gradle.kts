plugins { kotlin("multiplatform") version "2.2.20" }
repositories { maven { url = uri(providers.gradleProperty("sdkRepository").get()); content { includeGroup("example") } }; mavenCentral() }
kotlin {
    jvm()
    // Mocha's 2 s default is too short for a scenario loop on a loaded host.
    js(IR) { nodejs { testTask { useMocha { timeout = "60s" } } } }
    if (providers.gradleProperty("crucible.native").orNull != "false") { macosArm64(); iosArm64(); iosSimulatorArm64(); mingwX64() }
    compilerOptions { optIn.addAll("kotlin.time.ExperimentalTime") }
    sourceSets {
        commonMain.dependencies { implementation("example:weather-kotlin-mpp:0.0.0-test"); implementation("example:json-rest-kotlin-mpp:0.0.0-test"); implementation("example:json-json10-kotlin-mpp:0.0.0-test"); implementation("example:json-json11-kotlin-mpp:0.0.0-test"); implementation("example:stream-kotlin-mpp:0.0.0-test"); implementation("example:policy-kotlin-mpp:0.0.0-test") }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation("io.ktor:ktor-client-mock:3.3.3")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
        }
    }
}

// Preserve the consumer assertion and stack location in CI's plain-text log.
tasks.withType<org.gradle.api.tasks.testing.Test>().configureEach {
    testLogging.exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
}
