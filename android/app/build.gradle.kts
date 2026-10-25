import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

/**
 * The version, from SPOTLESS_VERSION (x.y.z, which the Android Release
 * workflow takes from the android-v tag), or 0.0.0-dev without it. The
 * version code is made from it, x * 10000 + y * 100 + z, so it goes up
 * with every release.
 */
data class Version(val name: String, val code: Int)

val appVersion: Version = System.getenv("SPOTLESS_VERSION").let { name ->
    if (name == null) return@let Version("0.0.0-dev", 1)
    val parts = Regex("""(\d+)\.(\d{1,2})\.(\d{1,2})""").matchEntire(name)?.destructured
        ?: throw GradleException("SPOTLESS_VERSION $name isn't x.y.z, with y and z below 100")
    val (major, minor, patch) = parts.toList().map { it.toInt() }
    Version(name, major * 10000 + minor * 100 + patch)
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
        versionCode = appVersion.code
        versionName = appVersion.name
        // 64-bit ARM phones only, for now.
        ndk { abiFilters += "arm64-v8a" }
        manifestPlaceholders["appName"] = "Spotless"
    }

    signingConfigs {
        // The release key, given by the environment (as the Android Release
        // workflow does from the repository's secrets). Without it, release
        // builds come out unsigned.
        val keystore = System.getenv("SPOTLESS_KEYSTORE")
        if (keystore != null) {
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("SPOTLESS_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("SPOTLESS_KEY_ALIAS")
                keyPassword = System.getenv("SPOTLESS_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        // A separate app beside the released one, so trying a build keeps
        // the released app's sign-in and playlists.
        debug {
            applicationIdSuffix = ".debug"
            manifestPlaceholders["appName"] = "Spotless debug"
        }
        release {
            // JNA and the UniFFI bindings find classes by reflection, which
            // shrinking would need keep rules for.
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    buildFeatures { compose = true }

    testOptions { unitTests { isIncludeAndroidResources = true } }

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
    // The screenshots' test: Robolectric for Android, Roborazzi to draw,
    // and JNA's own jar, which can load the core built for this computer.
    debugImplementation(libs.compose.ui.test.manifest)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation("net.java.dev.jna:jna:${libs.versions.jna.get()}")
}

tasks.withType<Test>().configureEach {
    // The core built for this computer, by packaging/screenshots.sh
    // (cargo build --release -p spotless-ffi).
    val workspace = rootProject.layout.projectDirectory.dir("..")
    systemProperty("jna.library.path", workspace.dir("target/release").asFile.path)
    // Where the screenshots go: -Pscreenshots=<folder>, or app/build/screenshots.
    val screenshots = providers.gradleProperty("screenshots")
        .orElse(layout.buildDirectory.dir("screenshots").map { it.asFile.path })
    systemProperty("spotless.screenshots", screenshots.get())
    systemProperty("roborazzi.test.record", "true")
    // Drawn again each time it's asked for: the core may have changed.
    outputs.upToDateWhen { false }
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
