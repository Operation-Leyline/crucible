plugins {
    kotlin("multiplatform") version "2.2.20"
    id("com.android.library") version "8.11.1"
    `maven-publish`
}
group = "@GROUP@"
version = "@VERSION@"
repositories { google(); mavenCentral() }
val nativeEnabled = providers.gradleProperty("crucible.native").orNull != "false"
kotlin {
    jvm()
    androidTarget { publishLibraryVariants("release") }
    js(IR) { nodejs(); browser() }
    if (nativeEnabled) { iosArm64(); iosSimulatorArm64(); macosArm64(); mingwX64() }
    applyDefaultHierarchyTemplate()
    compilerOptions {
        optIn.addAll("kotlin.time.ExperimentalTime", "kotlin.io.encoding.ExperimentalEncodingApi", "kotlin.uuid.ExperimentalUuidApi", "kotlinx.serialization.ExperimentalSerializationApi", "kotlin.ExperimentalUnsignedTypes")
    }
    sourceSets {
        commonMain.dependencies {
            api("com.ionspin.kotlin:bignum:0.3.10")
            api("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.2")
            api("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
            api("io.ktor:ktor-client-core:3.3.3")
            implementation("org.jetbrains.kotlinx:kotlinx-datetime:0.7.1")
        }
        commonTest.dependencies { implementation(kotlin("test")); implementation("io.ktor:ktor-client-mock:3.3.3"); implementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.10.2") }
        jvmMain.dependencies { implementation("io.ktor:ktor-client-cio:3.3.3") }
        androidMain.dependencies { implementation("io.ktor:ktor-client-cio:3.3.3") }
        jsMain.dependencies { implementation("io.ktor:ktor-client-js:3.3.3") }
        if (nativeEnabled) {
            getByName("appleMain").dependencies { implementation("io.ktor:ktor-client-darwin:3.3.3") }
            getByName("mingwMain").dependencies { implementation("io.ktor:ktor-client-winhttp:3.3.3") }
        }
    }
}
android { namespace = "@PACKAGE@"; compileSdk = 35; defaultConfig { minSdk = 26 }; compileOptions { sourceCompatibility = JavaVersion.VERSION_1_8; targetCompatibility = JavaVersion.VERSION_1_8 } }
publishing { repositories { maven { name = "qualification"; url = uri(providers.gradleProperty("qualificationRepository").orElse(layout.buildDirectory.dir("repository").map { it.asFile.absolutePath })) } } }
