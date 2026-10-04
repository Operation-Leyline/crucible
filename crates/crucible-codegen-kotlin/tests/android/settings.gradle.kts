pluginManagement { repositories { google(); mavenCentral(); gradlePluginPortal() } }
dependencyResolutionManagement {
    repositories {
        maven { url = uri(providers.gradleProperty("sdkRepository").get()); content { includeGroup("example") } }
        google()
        mavenCentral()
    }
}
rootProject.name = "crucible-kotlin-android"
