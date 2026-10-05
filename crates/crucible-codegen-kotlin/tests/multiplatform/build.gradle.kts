plugins { kotlin("multiplatform") version "2.2.20" }
repositories { maven { url = uri(providers.gradleProperty("sdkRepository").get()); content { includeGroup("example") } }; mavenCentral() }
kotlin {
    jvm()
    js(IR) { nodejs() }
    if (providers.gradleProperty("crucible.native").orNull != "false") { macosArm64(); iosArm64(); iosSimulatorArm64(); mingwX64() }
    compilerOptions { optIn.addAll("kotlin.time.ExperimentalTime") }
    sourceSets {
        commonMain.dependencies { implementation("example:weather-kotlin-mpp:0.0.0-test"); implementation("example:json-rest-kotlin-mpp:0.0.0-test"); implementation("example:json-json10-kotlin-mpp:0.0.0-test"); implementation("example:json-json11-kotlin-mpp:0.0.0-test") }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation("io.ktor:ktor-client-mock:3.3.3")
            implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2")
        }
    }
}
