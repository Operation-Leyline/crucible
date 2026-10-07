plugins {
    `java-library`
    `maven-publish`
}

group = "example"
version = "0.0.0-SNAPSHOT"

repositories {
    mavenCentral()
}

dependencies {
    api("com.google.code.gson:gson:2.13.2")
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
    withSourcesJar()
}

tasks.withType<JavaCompile>().configureEach {
    options.release.set(17)
    options.encoding = "UTF-8"
}

publishing {
    publications {
        create<MavenPublication>("sdk") {
            from(components["java"])
        }
    }
}
