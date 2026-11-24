import com.android.build.api.variant.ApplicationAndroidComponentsExtension
import org.gradle.process.ExecOperations
import java.io.ByteArrayOutputStream
import java.util.Properties
import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// ---------------------------------------------------------------------------------------------
// Rust core: cargo-ndk builds libkweb_core.so, uniffi-bindgen turns it into Kotlin bindings.
// ---------------------------------------------------------------------------------------------

val rustRepoRoot: File = rootDir.parentFile
val rustAbis = listOf("arm64-v8a", "x86_64")
val minSdkVersion = 29

/** `-PskipNative` (or skipNative=true in gradle.properties) skips cargo-ndk; . */
val skipNative: Boolean = providers.gradleProperty("skipNative").map { it != "false" }.getOrElse(false)

val rustSourceFiles = files(
    fileTree(rustRepoRoot.resolve("core")) { include("Cargo.toml", "build.rs", "src/**") },
    rustRepoRoot.resolve("Cargo.toml"),
    rustRepoRoot.resolve("Cargo.lock"),
)

/** Where cargo is: on the PATH, or else where rustup put it (F-Droid's build server does not pass a PATH on). */
object Cargo {
    val exe: String by lazy {
        val name = if (System.getProperty("os.name").lowercase().contains("win")) "cargo.exe" else "cargo"
        val onPath = System.getenv("PATH").orEmpty().split(File.pathSeparator).any { File(it, name).isFile }
        val cargoHome = System.getenv("CARGO_HOME") ?: File(System.getProperty("user.home"), ".cargo").path
        val installed = File(cargoHome, "bin/$name")
        if (!onPath && installed.isFile) installed.absolutePath else "cargo"
    }
}

abstract class CargoNdkTask @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Internal abstract val repoRoot: DirectoryProperty
    @get:InputFiles @get:PathSensitive(PathSensitivity.RELATIVE) abstract val rustSources: ConfigurableFileCollection
    @get:Input abstract val release: Property<Boolean>
    @get:Input abstract val abis: ListProperty<String>
    @get:Input abstract val minSdk: Property<Int>
    @get:OutputDirectory abstract val outDir: DirectoryProperty

    /** Runs [args] and returns its output, or null if the program is missing or fails. */
    private fun probe(vararg args: String, dir: File): String? {
        val out = ByteArrayOutputStream()
        return try {
            val result = exec.exec {
                commandLine(*args)
                workingDir = dir
                standardOutput = out
                errorOutput = out
                isIgnoreExitValue = true
            }
            if (result.exitValue == 0) out.toString().trim() else null
        } catch (_: Exception) {
            null
        }
    }

    @TaskAction
    fun build() {
        val root = repoRoot.get().asFile
        val problems = mutableListOf<String>()

        if (probe(Cargo.exe, "--version", dir = root) == null) {
            problems += "cargo is not on PATH (install Rust from https://rustup.rs)."
        } else if (probe(Cargo.exe, "ndk", "--version", dir = root) == null) {
            problems += "cargo-ndk is not installed (cargo install cargo-ndk)."
        }
        val sdk = System.getenv("ANDROID_HOME") ?: System.getenv("ANDROID_SDK_ROOT")
        val ndkFromEnv = listOf("ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "ANDROID_NDK")
            .mapNotNull { System.getenv(it) }.any { File(it).isDirectory }
        val ndkInSdk = sdk != null && File(sdk, "ndk").list().orEmpty().isNotEmpty()
        if (!ndkFromEnv && !ndkInSdk) {
            problems += "the Android NDK was not found: set ANDROID_NDK_HOME to an installed NDK " +
                "(sdkmanager \"ndk;<version>\") or install one under \$ANDROID_HOME/ndk."
        }
        if (problems.isNotEmpty()) {
            throw GradleException(
                "Cannot build the Rust core for Android:\n  - " + problems.joinToString("\n  - ") +
                    "\nTo build only the Kotlin side (the APK will not contain the native library and cannot run), " +
                    "pass -PskipNative.",
            )
        }

        val out = outDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        exec.exec {
            workingDir = root
            val cmd = mutableListOf(Cargo.exe, "ndk")
            abis.get().forEach { cmd += listOf("-t", it) }
            cmd += listOf("--platform", minSdk.get().toString(), "-o", out.absolutePath, "build", "-p", "kweb-core")
            if (release.get()) cmd += listOf("--release", "--locked")
            commandLine(cmd)
            if (release.get()) {
                // Same bytes from any checkout path and any user: no build paths in the library, no build id.
                val cargoHome = System.getenv("CARGO_HOME") ?: File(System.getProperty("user.home"), ".cargo").path
                environment(
                    "RUSTFLAGS",
                    "--remap-path-prefix=${root.absolutePath}=/kweb --remap-path-prefix=$cargoHome=/cargo " +
                        "-C link-arg=-Wl,--build-id=none",
                )
            }
        }
    }
}

abstract class UniffiBindingsTask @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Internal abstract val repoRoot: DirectoryProperty
    @get:InputFiles @get:PathSensitive(PathSensitivity.RELATIVE) abstract val rustSources: ConfigurableFileCollection

    /** Output of [CargoNdkTask]; when absent a host build of the core is used instead (-PskipNative). */
    @get:InputDirectory @get:Optional @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val nativeLibs: DirectoryProperty
    @get:OutputDirectory abstract val outDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val root = repoRoot.get().asFile
        val library: File
        if (nativeLibs.isPresent) {
            // Metadata is read from the ELF file; it is the same for every ABI.
            library = nativeLibs.get().asFile.resolve("arm64-v8a/libkweb_core.so")
        } else {
            exec.exec {
                workingDir = root
                commandLine(Cargo.exe, "build", "-p", "kweb-core")
            }
            val os = System.getProperty("os.name").lowercase()
            val fileName = when {
                "win" in os -> "kweb_core.dll"
                "mac" in os -> "libkweb_core.dylib"
                else -> "libkweb_core.so"
            }
            val targetDir = System.getenv("CARGO_TARGET_DIR")?.let { root.resolve(it) } ?: root.resolve("target")
            library = targetDir.resolve("debug/$fileName")
        }
        check(library.isFile) { "Rust library not found at $library" }

        val out = outDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        exec.exec {
            workingDir = root
            commandLine(
                Cargo.exe, "run", "-q", "-p", "kweb-core", "--features", "bindgen", "--bin", "uniffi-bindgen", "--",
                "generate", library.absolutePath, "--language", "kotlin", "--no-format", "--out-dir", out.absolutePath,
            )
        }
    }
}

// ---------------------------------------------------------------------------------------------
// rustls-platform-verifier: the Kotlin half must have exactly the version of the Rust crate
// `rustls-platform-verifier-android` in Cargo.lock. The Kotlin half is copied into the app
// (src/main/kotlin/org/rustls/platformverifier, see android/third_party/rustls-platform-verifier/README.md),
// so the build stops when the crate moves on and the copy did not.
// ---------------------------------------------------------------------------------------------

val rustlsVerifierVersion: Provider<String> =
    providers.fileContents(layout.file(provider { rustRepoRoot.resolve("Cargo.lock") })).asText.map { lock ->
        val lines = lock.lines()
        val at = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
        check(at >= 0) { "rustls-platform-verifier-android not found in Cargo.lock" }
        lines.drop(at + 1).first { it.trimStart().startsWith("version = ") }.substringAfter('"').substringBefore('"')
    }

// ---------------------------------------------------------------------------------------------
// Version: the one place. versionCode goes up by one per release; versionName is the release name.
// ---------------------------------------------------------------------------------------------

val appVersionCode = 10
val appVersionName = "0.2.7"

// Release signing: the key lives outside the repository (see dev/README.md, "Release").
val keystorePropertiesFile: File? =
    (System.getenv("KWEB_KEYSTORE_PROPERTIES")?.takeIf { it.isNotBlank() }?.let(::File)
        ?: File(System.getProperty("user.home"), "tools/kweb-release/keystore.properties"))
        .takeIf { it.isFile }
val keystoreProperties = Properties().also { props ->
    keystorePropertiesFile?.inputStream()?.use { props.load(it) }
}

android {
    namespace = "ovh.kweb.mail"
    compileSdk = 36
    // Pinned for the same bytes everywhere (r28c); the recipe in dev/fdroid names the same one.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "ovh.kweb.mail"
        minSdk = minSdkVersion
        targetSdk = 36
        versionCode = appVersionCode
        versionName = appVersionName
        // Also filters the ABIs of JNA's bundled libjnidispatch.
        ndk { abiFilters += rustAbis }
        // False in the "fdroid" flavor, which has no updater.
        buildConfigField("boolean", "UPDATER", "true")
    }

    // No signing blob in the APK: it is not reproducible and F-Droid does not want it.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    signingConfigs {
        if (keystorePropertiesFile != null) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isDebuggable = false
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            if (keystorePropertiesFile != null) {
                signingConfig = signingConfigs.getByName("release")
            } else {
                logger.lifecycle("kweb: no release keystore found; the release APK will be unsigned.")
            }
        }
    }

    // Two stores. "github" is the build with the updater. "fdroid" has none (src/fdroid/AndroidManifest.xml):
    // F-Droid delivers updates itself. F-Droid's build server has no keystore, so its APK comes out unsigned;
    // built here with the keystore it carries the project's signature (for a reproducible-build entry).
    flavorDimensions += "store"
    productFlavors {
        create("github") {
            dimension = "store"
            isDefault = true
        }
        create("fdroid") {
            dimension = "store"
            buildConfigField("boolean", "UPDATER", "false")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// The F-Droid flavor is only ever released: no fdroidDebug, so assembleDebug builds one APK.
val vendoredRustlsVerifier = "0.2.0"
tasks.named("preBuild") {
    val locked = rustlsVerifierVersion
    val vendored = vendoredRustlsVerifier
    doFirst {
        check(locked.get() == vendored) {
            "Cargo.lock has rustls-platform-verifier-android ${locked.get()}, the copied Kotlin half is $vendored: " +
                "see android/third_party/rustls-platform-verifier/README.md"
        }
    }
}

androidComponents.beforeVariants { variant ->
    if (variant.productFlavors.any { it.second == "fdroid" } && variant.buildType == "debug") variant.enable = false
}

androidComponents.onVariants { variant ->
    val cap = variant.name.replaceFirstChar { it.uppercase() }
    val native = if (skipNative) null else tasks.register<CargoNdkTask>("buildRust$cap") {
        group = "rust"
        description = "Builds libkweb_core.so for ${rustAbis.joinToString()} with cargo-ndk"
        repoRoot.set(layout.dir(provider { rustRepoRoot }))
        rustSources.from(rustSourceFiles)
        release.set(variant.buildType == "release")
        abis.set(rustAbis)
        minSdk.set(minSdkVersion)
        outDir.set(layout.buildDirectory.dir("rust/jniLibs/${variant.name}"))
    }
    val bindings = tasks.register<UniffiBindingsTask>("generateUniffiBindings$cap") {
        group = "rust"
        description = "Generates the Kotlin bindings of the core with uniffi-bindgen"
        repoRoot.set(layout.dir(provider { rustRepoRoot }))
        rustSources.from(rustSourceFiles)
        if (native != null) nativeLibs.set(native.flatMap { it.outDir })
        outDir.set(layout.buildDirectory.dir("generated/uniffi/${variant.name}"))
    }
    variant.sources.kotlin?.addGeneratedSourceDirectory(bindings, UniffiBindingsTask::outDir)
    if (native != null) variant.sources.jniLibs?.addGeneratedSourceDirectory(native, CargoNdkTask::outDir)
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.work.runtime)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.browser)
    implementation(libs.kotlinx.coroutines.android)
    debugImplementation(libs.androidx.compose.ui.tooling)
    testImplementation(libs.junit)

    implementation(libs.jna.get().let { "${it.module}:${it.versionConstraint.requiredVersion}@aar" })
}
