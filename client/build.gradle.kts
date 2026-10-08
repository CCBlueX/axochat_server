import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    kotlin("jvm") version "2.4.20"
    kotlin("plugin.serialization") version "2.4.20"
    `java-library`
    `maven-publish`
}

group = "net.ccbluex"
// CI publishes releases from `client-v<version>` tags and snapshots from pushes
version = providers.gradleProperty("releaseVersion")
    .orElse(providers.gradleProperty("version").map { "$it-SNAPSHOT" })
    .get()

repositories {
    mavenCentral()
}

dependencies {
    // the versions fabric-language-kotlin ships, so LiquidBounce does not bundle its own
    api("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.11.0")
    api("org.jetbrains.kotlinx:kotlinx-serialization-json:1.11.0")
    // the version LiquidBounce ships
    api("com.squareup.okhttp3:okhttp:5.5.0")

    testImplementation(kotlin("test"))
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.11.0")
}

java {
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
    withSourcesJar()
    withJavadocJar()
}

kotlin {
    explicitApi()
    compilerOptions {
        jvmTarget = JvmTarget.JVM_21
        freeCompilerArgs.add("-Xjdk-release=21")
        optIn.add("kotlinx.serialization.ExperimentalSerializationApi")
    }
}

tasks.test {
    useJUnitPlatform()
}

publishing {
    publications {
        create<MavenPublication>("maven") {
            from(components["java"])
            pom {
                name = "axochat-client"
                description = "Kotlin client for the axochat protocol behind LiquidChat."
                url = "https://github.com/CCBlueX/axochat"
                licenses {
                    license {
                        name = "MIT License"
                        url = "https://opensource.org/licenses/MIT"
                    }
                }
                developers {
                    developer {
                        id = "ccbluex"
                        name = "CCBlueX"
                    }
                }
                scm {
                    connection = "scm:git:git://github.com/CCBlueX/axochat.git"
                    developerConnection = "scm:git:ssh://github.com:CCBlueX/axochat.git"
                    url = "https://github.com/CCBlueX/axochat"
                }
            }
        }
    }
    repositories {
        maven {
            name = "ccbluex"
            url = uri("https://maven.ccbluex.net/" + if (version.toString().endsWith("-SNAPSHOT")) "snapshots" else "releases")
            credentials {
                username = providers.environmentVariable("MAVEN_TOKEN_NAME").orNull
                password = providers.environmentVariable("MAVEN_TOKEN_SECRET").orNull
            }
            authentication {
                create<BasicAuthentication>("basic")
            }
        }
    }
}
