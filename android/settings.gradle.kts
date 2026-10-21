pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode = RepositoriesMode.FAIL_ON_PROJECT_REPOS
    repositories {
        google()
        mavenCentral()
        // rustls-platform-verifier's Kotlin part, published by its project
        // beside the crate (see app/build.gradle.kts).
        maven("https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/") {
            content { includeGroup("org.rustls") }
        }
    }
}

rootProject.name = "spotless"
include(":app")
