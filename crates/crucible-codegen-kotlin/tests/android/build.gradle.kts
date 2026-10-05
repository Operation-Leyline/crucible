plugins {
    id("com.android.application") version "8.11.1"
    id("org.jetbrains.kotlin.android") version "2.2.20"
}
android {
    namespace = "example.consumer"
    compileSdk = 35
    defaultConfig {
        applicationId = "example.crucible.kotlin.qualification"
        minSdk = 26
        targetSdk = 35
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    lint { checkDependencies = true }
}
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_1_8); optIn.add("kotlin.time.ExperimentalTime") } }
dependencies {
    implementation("example:weather-kotlin-android:0.0.0-test")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
