import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.spotless"
    compileSdk = 37
    // The NDK cargo-ndk builds the Rust library with.
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "dev.spotless"
        // Android 10, as the library is built for.
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        // 64-bit ARM phones only, for now.
        ndk { abiFilters += "arm64-v8a" }
    }

    buildFeatures { compose = true }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.activity.compose)
    implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")
    debugImplementation(libs.compose.ui.tooling)
}

/** Runs cargo from the repository's root, where the Rust workspace is. */
abstract class Cargo : DefaultTask() {
    @get:Inject abstract val exec: ExecOperations

    @get:Internal abstract val workspace: DirectoryProperty

    @get:Internal abstract val ndk: DirectoryProperty

    @get:Input abstract val release: Property<Boolean>

    init {
        // Cargo knows what changed; it is quick when nothing did.
        outputs.upToDateWhen { false }
    }

    protected fun cargo(vararg args: String) {
        exec.exec {
            workingDir = workspace.get().asFile
            // rustup's cargo, which reads rust-toolchain.toml.
            executable = "${System.getProperty("user.home")}/.cargo/bin/cargo"
            environment("ANDROID_NDK_HOME", ndk.get().asFile.path)
            args(*args)
        }
    }

    protected fun profile() = if (release.get()) "release" else "debug"
}

/** libspotless_ffi.so for the phone, laid out as jniLibs. */
abstract class BuildRust : Cargo() {
    @get:OutputDirectory abstract val jniLibs: DirectoryProperty

    @TaskAction
    fun build() {
        val mode = if (release.get()) arrayOf("--release") else emptyArray()
        cargo(
            "ndk", "-t", "arm64-v8a", "-P", "29", "-o", jniLibs.get().asFile.path,
            "build", "-p", "spotless-ffi", *mode,
        )
    }
}

/** The Kotlin bindings, read off the library BuildRust made. */
abstract class Bindings : Cargo() {
    @get:OutputDirectory abstract val sources: DirectoryProperty

    @TaskAction
    fun generate() {
        val library = workspace.get().asFile
            .resolve("target/aarch64-linux-android/${profile()}/libspotless_ffi.so")
        sources.get().asFile.deleteRecursively()
        cargo(
            "run", "-q", "-p", "spotless-ffi", "--features", "bindgen", "--bin", "uniffi-bindgen",
            "--", "generate", "--library", library.path, "--language", "kotlin", "--no-format",
            "--out-dir", sources.get().asFile.path,
        )
    }
}

androidComponents {
    onVariants { variant ->
        val name = variant.name.replaceFirstChar { it.uppercase() }
        val release = variant.buildType == "release"
        val workspace = rootProject.layout.projectDirectory.dir("..")
        val ndk = androidComponents.sdkComponents.ndkDirectory
        val rust = tasks.register<BuildRust>("buildRust$name") {
            this.workspace = workspace
            this.ndk = ndk
            this.release = release
        }
        val bindings = tasks.register<Bindings>("bindings$name") {
            this.workspace = workspace
            this.ndk = ndk
            this.release = release
            dependsOn(rust)
        }
        variant.sources.jniLibs?.addGeneratedSourceDirectory(rust, BuildRust::jniLibs)
        variant.sources.kotlin?.addGeneratedSourceDirectory(bindings, Bindings::sources)
    }
}
