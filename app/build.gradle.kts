@file:Suppress("UnstableApiUsage")

import com.android.build.gradle.tasks.PackageApplication
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import java.util.Properties

plugins {
    alias(libs.plugins.agp.app)
    alias(libs.plugins.kotlin.compose.compiler)
    alias(libs.plugins.ksp)
    alias(libs.plugins.lsplugin.apksign)
    alias(libs.plugins.lsplugin.resopt)
    id("kotlin-parcelize")
}

val androidCompileSdkVersion: Int = rootProject.extra["androidCompileSdkVersion"] as Int
val androidCompileNdkVersion: String = rootProject.extra["androidCompileNdkVersion"] as String
val androidBuildToolsVersion: String = rootProject.extra["androidBuildToolsVersion"] as String
val androidMinSdkVersion: Int = rootProject.extra["androidMinSdkVersion"] as Int
val androidTargetSdkVersion: Int = rootProject.extra["androidTargetSdkVersion"] as Int
val managerVersionCode: Int = rootProject.extra["managerVersionCode"] as Int
val managerVersionName: String = rootProject.extra["managerVersionName"] as String
val branchName: String = rootProject.extra["branchName"] as String
val requiredKmis = listOf(
    "android12-5.4",
    "android12-5.10",
    "android13-5.10",
    "android13-5.15",
    "android14-5.15",
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
)
val kernelFeatureMarker = rootProject.file("kmod/feature-marker.txt").readText().trim()
val kmodAssetsDir = layout.buildDirectory.dir("generated/kmod-assets")

fun readElfU16(bytes: ByteArray, offset: Int): Int? {
    if (offset < 0 || offset + 2 > bytes.size) return null
    return (bytes[offset].toInt() and 0xff) or
        ((bytes[offset + 1].toInt() and 0xff) shl 8)
}

fun readElfU32(bytes: ByteArray, offset: Int): Long? {
    if (offset < 0 || offset + 4 > bytes.size) return null
    var value = 0L
    for (index in 3 downTo 0) {
        value = (value shl 8) or (bytes[offset + index].toLong() and 0xff)
    }
    return value
}

fun readElfU64(bytes: ByteArray, offset: Int): Long? {
    if (offset < 0 || offset + 8 > bytes.size || bytes[offset + 7] < 0) return null
    var value = 0L
    for (index in 7 downTo 0) {
        value = (value shl 8) or (bytes[offset + index].toLong() and 0xff)
    }
    return value
}

data class ElfSection(val offset: Int, val size: Int)

fun elfSection(bytes: ByteArray, wanted: String): ElfSection? {
    if (bytes.size < 64 || bytes[4] != 2.toByte() || bytes[5] != 1.toByte()) return null
    val sectionOffset = readElfU64(bytes, 0x28) ?: return null
    val entrySize = readElfU16(bytes, 0x3a) ?: return null
    val sectionCount = readElfU16(bytes, 0x3c) ?: return null
    val namesIndex = readElfU16(bytes, 0x3e) ?: return null
    if (entrySize < 64 || sectionCount == 0 || namesIndex >= sectionCount) return null
    if (sectionOffset > Int.MAX_VALUE) return null
    val tableOffset = sectionOffset.toInt()
    val tableSize = entrySize.toLong() * sectionCount
    if (tableOffset < 0 || tableOffset.toLong() + tableSize > bytes.size) return null

    fun headerOffset(index: Int): Int = tableOffset + index * entrySize
    val namesHeader = headerOffset(namesIndex)
    val namesOffset = readElfU64(bytes, namesHeader + 24) ?: return null
    val namesSize = readElfU64(bytes, namesHeader + 32) ?: return null
    if (namesOffset > Int.MAX_VALUE || namesSize > Int.MAX_VALUE ||
        namesOffset + namesSize > bytes.size) return null
    val namesStart = namesOffset.toInt()
    val namesEnd = namesStart + namesSize.toInt()

    for (index in 0 until sectionCount) {
        val header = headerOffset(index)
        val nameOffset = readElfU32(bytes, header)?.toInt() ?: return null
        val start = namesStart + nameOffset
        if (start !in namesStart until namesEnd) continue
        var end = start
        while (end < namesEnd && bytes[end] != 0.toByte()) end++
        if (bytes.copyOfRange(start, end).toString(Charsets.US_ASCII) == wanted) {
            val dataOffset = readElfU64(bytes, header + 24) ?: return null
            val dataSize = readElfU64(bytes, header + 32) ?: return null
            if (dataOffset > Int.MAX_VALUE || dataSize > Int.MAX_VALUE ||
                dataOffset + dataSize > bytes.size) return null
            return ElfSection(dataOffset.toInt(), dataSize.toInt())
        }
    }
    return null
}

fun validateKernelModule(file: File, kmi: String) {
    require(file.isFile && file.length() > 64L) { "missing kernel module: $file" }
    val bytes = file.readBytes()
    require(bytes.size >= 4 && bytes[0] == 0x7f.toByte() &&
        bytes[1] == 'E'.code.toByte() && bytes[2] == 'L'.code.toByte() &&
        bytes[3] == 'F'.code.toByte()) { "$file is not ELF" }
    val binaryText = bytes.toString(Charsets.ISO_8859_1)
    require(binaryText.contains(kernelFeatureMarker)) {
        "$file is stale: missing Ethereal feature marker $kernelFeatureMarker"
    }
    require(binaryText.contains("name=ethereal")) {
        "$file has the wrong module identity"
    }
    val majorMinor = kmi.substringAfterLast('-')
    require(binaryText.contains("vermagic=$majorMinor")) {
        "$file does not match KMI $kmi (expected vermagic $majorMinor.x)"
    }
    require(binaryText.contains("modversions")) {
        "$file does not enforce symbol CRC compatibility"
    }
    val basicVersions = elfSection(bytes, "__versions")
    val extendedCrcs = elfSection(bytes, "__version_ext_crcs")
    val extendedNames = elfSection(bytes, "__version_ext_names")
    val hasBasicVersions = (basicVersions?.size ?: 0) > 0
    val hasExtendedVersions = (extendedCrcs?.size ?: 0) > 0 &&
        (extendedNames?.size ?: 0) > 0
    require(hasBasicVersions || hasExtendedVersions) {
        "$file has no non-empty basic or extended modversion records"
    }
    if ((extendedCrcs?.size ?: 0) > 0 || (extendedNames?.size ?: 0) > 0) {
        require(hasExtendedVersions) {
            "$file has incomplete extended modversion sections"
        }
        require(extendedCrcs!!.size % 4 == 0) {
            "$file has a misaligned __version_ext_crcs section"
        }
        val names = bytes.copyOfRange(
            extendedNames!!.offset,
            extendedNames.offset + extendedNames.size,
        )
        val crcCount = extendedCrcs.size / 4
        var position = 0
        repeat(crcCount) { index ->
            var nameEnd = position
            while (nameEnd < names.size && names[nameEnd] != 0.toByte()) nameEnd++
            require(nameEnd < names.size) {
                "$file has fewer extended modversion names than CRCs"
            }
            require(nameEnd > position) {
                "$file has an empty extended modversion name at index $index"
            }
            position = nameEnd + 1
        }
        require(names.drop(position).all { it == 0.toByte() }) {
            "$file has more extended modversion names than CRCs or non-zero name padding"
        }
    }
    val legacyMarkers = listOf("r" + "patch", "a" + "patch", "r" + "p/su")
    require(legacyMarkers.none { binaryText.contains(it, ignoreCase = true) }) {
        "$file contains a legacy brand or runtime path"
    }
    val hostBuildPath = Regex(
        """(?i)(?:(?<![a-z])[a-z]:[\\/]|/(?:mnt/[a-z]/|home/[^/\u0000\s]+/|Users/[^/\u0000\s]+/|__w/|tmp/ethereal-kmod\.|root/(?:gki-src|qemu-build|gki-module-build|android-ndk)/))"""
    )
    require(!hostBuildPath.containsMatchIn(binaryText)) {
        "$file contains an absolute host build path"
    }
}

apksign {
    storeFileProperty = "KEYSTORE_FILE"
    storePasswordProperty = "KEYSTORE_PASSWORD"
    keyAliasProperty = "KEY_ALIAS"
    keyPasswordProperty = "KEY_PASSWORD"
}

val ccache = System.getenv("PATH")?.split(File.pathSeparator)
    ?.map { File(it, "ccache") }?.firstOrNull { it.exists() }?.absolutePath

val baseFlags = listOf(
    "-Wall", "-Qunused-arguments", "-fno-rtti", "-fvisibility=hidden",
    "-fvisibility-inlines-hidden", "-fno-exceptions", "-fno-stack-protector",
    "-fomit-frame-pointer", "-Wno-builtin-macro-redefined", "-Wno-unused-value",
    "-D__FILE__=__FILE_NAME__",
    "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON", "-Wno-unused", "-Wno-unused-parameter",
    "-Wno-unused-command-line-argument", "-Wno-incompatible-function-pointer-types",
    "-U_FORTIFY_SOURCE", "-D_FORTIFY_SOURCE=0"
)

val baseArgs = mutableListOf(
    "-DANDROID_STL=none", "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON",
    "-DCMAKE_CXX_STANDARD=23", "-DCMAKE_C_STANDARD=23",
    "-DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON", "-DCMAKE_VISIBILITY_INLINES_HIDDEN=ON",
    "-DCMAKE_CXX_VISIBILITY_PRESET=hidden", "-DCMAKE_C_VISIBILITY_PRESET=hidden"
).apply { if (ccache != null) add("-DANDROID_CCACHE=$ccache") }

android {
    namespace = "me.ethereal.app"

    buildTypes {
        debug {
            isDebuggable = true
            isMinifyEnabled = false
            isShrinkResources = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            externalNativeBuild {
                cmake {
                    arguments += listOf("-DCMAKE_CXX_FLAGS_DEBUG=-Og", "-DCMAKE_C_FLAGS_DEBUG=-Og")
                }
            }
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            isDebuggable = false
            multiDexEnabled = true
            vcsInfo.include = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            externalNativeBuild {
                cmake {
                    val relFlags = listOf(
                        "-flto", "-ffunction-sections", "-fdata-sections", "-Wl,--gc-sections",
                        "-fno-unwind-tables", "-fno-asynchronous-unwind-tables", "-Wl,--exclude-libs,ALL",
                        "-Ofast", "-fmerge-all-constants", "-flto=full", "-ffat-lto-objects",
                        "-fno-semantic-interposition", "-fno-threadsafe-statics"
                    )
                    cppFlags += relFlags
                    cFlags += relFlags
                    arguments += listOf("-DCMAKE_BUILD_TYPE=Release", "-DCMAKE_CXX_FLAGS_RELEASE=-O3 -DNDEBUG", "-DCMAKE_C_FLAGS_RELEASE=-O3 -DNDEBUG")
                }
            }
        }
    }

    dependenciesInfo.includeInApk = false

    buildFeatures {
        aidl = true
        buildConfig = true
        compose = true
        prefab = true
    }

    defaultConfig {
        applicationId = "me.ethereal.app"
        minSdk = androidMinSdkVersion
        targetSdk = androidTargetSdkVersion
        versionCode = managerVersionCode
        versionName = managerVersionName
        ndk.abiFilters.addAll(arrayOf("arm64-v8a"))
        externalNativeBuild {
            cmake {
                cppFlags += baseFlags + "-std=c++2b"
                cFlags += baseFlags + "-std=c2x"
                arguments += baseArgs
                abiFilters("arm64-v8a")
            }
        }
        base.archivesName = "Ethereal_${managerVersionCode}_${managerVersionName}_${branchName}"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    packaging {
        jniLibs {
            useLegacyPackaging = true
            // ET_EXEC named *.so crashes ColorOS 16 / 16KB devices on extract.
            excludes += setOf("**/libsu.so", "**/libethinit.so", "**/libethd.full.so")
        }
        resources {
            excludes += "**"
            merges += "META-INF/com/google/android/**"
        }
    }

    externalNativeBuild {
        cmake {
            version = "3.28.0+"
            path("src/main/cpp/CMakeLists.txt")
        }
    }

    androidResources {
        generateLocaleConfig = true
    }

    compileSdk = androidCompileSdkVersion
    ndkVersion = androidCompileNdkVersion
    buildToolsVersion = androidBuildToolsVersion

    lint {
        abortOnError = true
        checkReleaseBuilds = false
        // Community translations are intentionally partial and fall back to values/.
        disable += "MissingTranslation"
    }

    android.sourceSets.named("main") {
        kotlin.directories += "build/generated/ksp/$name/kotlin"
        jniLibs.directories += "libs"
        assets.directories += "build/generated/kmod-assets"
    }
}

// https://stackoverflow.com/a/77745844
tasks.withType<PackageApplication> {
    doFirst { appMetadata.asFile.orNull?.writeText("") }
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(21)
    }
}

kotlin {
    jvmToolchain(21)
    compilerOptions {
        jvmTarget = JvmTarget.JVM_21
    }
}

val localSdkDir = rootProject.file("local.properties").takeIf(File::isFile)?.let { file ->
    Properties().apply { file.inputStream().use { load(it) } }.getProperty("sdk.dir")
}
fun hasRequiredNdkVersion(directory: File, version: String): Boolean {
    if (!directory.isDirectory) return false
    val sourceProperties = directory.resolve("source.properties")
    if (!sourceProperties.isFile) return false
    val properties = Properties().apply {
        sourceProperties.inputStream().use { load(it) }
    }
    return properties.getProperty("Pkg.Revision")?.trim() == version
}

val androidNdkHome = (rootProject.extra["androidCompileNdkVersion"] as? String)?.let { ver ->
    val sdkCandidates = listOfNotNull(
        localSdkDir,
        System.getenv("ANDROID_HOME"),
        System.getenv("ANDROID_SDK_ROOT"),
    ).distinct()
    val candidates = sdkCandidates.map { File(it, "ndk/$ver") } +
        listOfNotNull(
            System.getenv("ANDROID_NDK_HOME")?.let(::File),
            System.getenv("ANDROID_NDK_ROOT")?.let(::File),
        )
    candidates.firstOrNull { hasRequiredNdkVersion(it, ver) }?.absolutePath
}
val localCargoHome = File(rootProject.rootDir, ".tools/cargo").takeIf { it.isDirectory }
val localRustupHome = File(rootProject.rootDir, ".tools/rustup").takeIf { it.isDirectory }
val cargoExecutable = localCargoHome
    ?.resolve("bin/cargo.exe")
    ?.takeIf { it.isFile }
    ?.absolutePath
    ?: "cargo"
val ndkClang = androidNdkHome
    ?.let { File(it, "toolchains/llvm/prebuilt") }
    ?.listFiles()
    ?.firstOrNull { it.isDirectory }
    ?.resolve("bin")
    ?.let { dir -> listOf("clang.exe", "clang").map(dir::resolve).firstOrNull(File::isFile) }

fun Exec.configureRustEnvironment() {
    executable(cargoExecutable)
    localCargoHome?.let { environment("CARGO_HOME", it.absolutePath) }
    localRustupHome?.let { environment("RUSTUP_HOME", it.absolutePath) }
    val remap = "--remap-path-prefix=${rootProject.rootDir.absolutePath}=/workspace/Ethereal"
    val rustFlags = listOfNotNull(System.getenv("RUSTFLAGS")?.takeIf { it.isNotBlank() }, remap)
        .joinToString(" ")
    environment("RUSTFLAGS", rustFlags)
    val cRemap = listOf(
        "-ffile-prefix-map=${rootProject.rootDir.absolutePath}=/workspace/Ethereal",
        "-fdebug-prefix-map=${rootProject.rootDir.absolutePath}=/workspace/Ethereal",
        "-fmacro-prefix-map=${rootProject.rootDir.absolutePath}=/workspace/Ethereal",
    ).joinToString(" ")
    val cFlags = listOfNotNull(
        System.getenv("CFLAGS_aarch64_linux_android")?.takeIf { it.isNotBlank() },
        cRemap,
    ).joinToString(" ")
    val cxxFlags = listOfNotNull(
        System.getenv("CXXFLAGS_aarch64_linux_android")?.takeIf { it.isNotBlank() },
        cRemap,
    ).joinToString(" ")
    environment("CFLAGS_aarch64_linux_android", cFlags)
    environment("CXXFLAGS_aarch64_linux_android", cxxFlags)
}

fun cargoNdkBuild(crateDir: String, binName: String, soName: String) {
    val build = tasks.register<Exec>("cargoBuild_${binName}") {
        configureRustEnvironment()
        args("ndk", "-t", "arm64-v8a", "build", "--release", "--locked")
        workingDir("${project.rootDir}/$crateDir")
        doFirst {
            requireNotNull(androidNdkHome) {
                "Android NDK $androidCompileNdkVersion is required to build $binName"
            }
            environment("ANDROID_NDK_HOME", androidNdkHome)
            environment("ANDROID_NDK_ROOT", androidNdkHome)
        }
    }
    tasks.register<Copy>("copy_${binName}") {
        dependsOn(build)
        from("${project.rootDir}/$crateDir/target/aarch64-linux-android/release/$binName")
        into("${project.projectDir}/libs/arm64-v8a")
        rename(binName, soName)
    }
}

cargoNdkBuild("ramtool", "ramtool", "libramtool.so")
cargoNdkBuild("ethd", "ethd", "libethd.so")

// Freestanding C stub (no libc / no dynamic linker). A regular Android
// executable cannot be injected into first-stage init: it needs the linker
// before the platform has mounted it.
val ethinitOutDir = File("${project.rootDir}/ethinit/target/aarch64-linux-android/release")
tasks.register<Exec>("buildEthinit") {
    doFirst { ethinitOutDir.mkdirs() }
    doFirst {
        requireNotNull(ndkClang) {
            "Android NDK $androidCompileNdkVersion clang is required to build ethinit"
        }
        executable(ndkClang.absolutePath)
    }
    args(
        "--target=aarch64-linux-android24",
        "-nostdlib",
        "-nostartfiles",
        "-ffreestanding",
        "-fPIC",
        "-fno-builtin",
        "-fvisibility=hidden",
        "-fno-stack-protector",
        "-fno-unwind-tables",
        "-fomit-frame-pointer",
        "-mbranch-protection=none",
        "-Os",
        "-static",
        "-Wl,-e,_start",
        "-Wl,--gc-sections",
        "-Wl,--build-id=none",
        "-Wl,--no-dynamic-linker",
        "-Wl,-z,norelro",
        "-Wl,-z,max-page-size=16384",
        "-fuse-ld=lld",
        "-o", File(ethinitOutDir, "ethinit").absolutePath,
        "${project.rootDir}/ethinit/start.S",
        "${project.rootDir}/ethinit/ethinit.c",
    )
}
tasks.register<Copy>("copy_ethinit") {
    dependsOn("buildEthinit")
    from(File(ethinitOutDir, "ethinit"))
    into("${project.projectDir}/src/main/assets")
    rename("ethinit", "ethereal-init")
    doLast {
        val dest = File("${project.rootDir}/ethd/embedded")
        dest.mkdirs()
        val src = File(ethinitOutDir, "ethinit")
        if (src.exists()) src.copyTo(File(dest, "ethinit"), overwrite = true)
    }
}

val ethsuOutDir = File("${project.rootDir}/ethsu/target/aarch64-linux-android/release")
val ethsuBinary = File(ethsuOutDir, "ethsu")
tasks.register<Exec>("buildEthsu") {
    doFirst { ethsuOutDir.mkdirs() }
    doFirst {
        requireNotNull(ndkClang) {
            "Android NDK $androidCompileNdkVersion clang is required to build ethsu"
        }
        executable(ndkClang.absolutePath)
    }
    args(
        "--target=aarch64-linux-android26",
        "-std=c17",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-O2",
        "-static",
        "-fPIE",
        "-fstack-protector-strong",
        "-D_FORTIFY_SOURCE=2",
        "-ffile-prefix-map=${project.rootDir}=/workspace/Ethereal",
        "-fdebug-prefix-map=${project.rootDir}=/workspace/Ethereal",
        "-fmacro-prefix-map=${project.rootDir}=/workspace/Ethereal",
        "-Wl,--build-id=none",
        "-Wl,--gc-sections",
        "-Wl,-z,max-page-size=16384",
        "-s",
        "-o", ethsuBinary.absolutePath,
        "${project.rootDir}/ethsu/ethsu.c",
    )
}
tasks.register("copy_ethsu") {
    dependsOn("buildEthsu")
    val asset = File("${project.projectDir}/src/main/assets/su")
    val jniCopy = File("${project.projectDir}/src/main/jniLibs/arm64-v8a/libsu.so")
    outputs.files(asset, jniCopy)
    doLast {
        require(ethsuBinary.isFile && ethsuBinary.length() > 64L) {
            "missing freshly built ethsu: $ethsuBinary"
        }
        asset.parentFile.mkdirs()
        jniCopy.parentFile.mkdirs()
        ethsuBinary.copyTo(asset, overwrite = true)
        ethsuBinary.copyTo(jniCopy, overwrite = true)
    }
}

tasks.register("stageKernelModule") {
    val assetsDir = kmodAssetsDir
    val featureMarker = rootProject.file("kmod/feature-marker.txt")
    val modules = requiredKmis.associateWith { kmi ->
        rootProject.file("kmod/prebuilt/$kmi/ethereal.ko")
    }
    inputs.file(featureMarker)
    inputs.files(modules.values)
    outputs.dir(assetsDir)
    doLast {
        val assets = assetsDir.get().asFile
        assets.deleteRecursively()
        val moduleAssets = File(assets, "kmod")
        check(moduleAssets.mkdirs()) { "failed to create $moduleAssets" }
        modules.forEach { (kmi, ko) ->
            validateKernelModule(ko, kmi)
            val name = "ethereal.$kmi.ko"
            ko.copyTo(File(moduleAssets, name), overwrite = true)
        }
    }
}

tasks.register("stageEmbeddedForEthd") {
    dependsOn("copy_ramtool", "copy_ethinit", "stageKernelModule")
    doLast {
        val dest = File("${project.rootDir}/ethd/embedded")
        dest.mkdirs()
        File(dest, "ethereal.ko").delete()
        listOf(
            File("${project.rootDir}/ramtool/target/aarch64-linux-android/release/ramtool") to File(dest, "ramtool"),
            File("${project.rootDir}/ethinit/target/aarch64-linux-android/release/ethinit") to File(dest, "ethinit"),
        ).forEach { (src, out) ->
            if (src.exists()) src.copyTo(out, overwrite = true)
        }
    }
}

tasks.named("cargoBuild_ethd") {
    dependsOn("stageEmbeddedForEthd")
}
tasks.register("stagePatchAssets") {
    dependsOn("copy_ethinit", "copy_ethsu", "stageKernelModule", "cargoBuild_ethd")
    doLast {
        val assets = File("${project.projectDir}/src/main/assets")
        assets.mkdirs()
        require(File(assets, "su").isFile) { "freshly built ethsu asset is missing" }
        val ethdFull = File("${project.rootDir}/ethd/target/aarch64-linux-android/release/ethd")
        require(ethdFull.isFile && ethdFull.length() > 64L) {
            "missing freshly built Ethereal daemon: $ethdFull"
        }
        ethdFull.copyTo(File(assets, "ethd.full"), overwrite = true)
    }
}

tasks.configureEach {
    if (name.contains("lint", ignoreCase = true)) {
        dependsOn("stagePatchAssets")
    }
    if (name == "mergeDebugJniLibFolders" || name == "mergeReleaseJniLibFolders") {
        dependsOn("copy_ethd", "copy_ramtool", "copy_ethinit", "copy_ethsu")
    }
    if (name == "mergeDebugAssets" || name == "mergeReleaseAssets") {
        dependsOn("stageKernelModule", "copy_ethinit", "stagePatchAssets")
    }
}

tasks.register<Exec>("cargoCleanEthd") {
    configureRustEnvironment()
    args("clean")
    workingDir("${project.rootDir}/ethd")
}

tasks.register<Delete>("nativeClean") {
    dependsOn("cargoCleanEthd")
    delete(
        file("${project.projectDir}/libs/arm64-v8a/libethd.so"),
        file("${project.projectDir}/libs/arm64-v8a/libramtool.so"),
        file("${project.projectDir}/libs/arm64-v8a/libethinit.so"),
        file("${project.projectDir}/src/main/assets/su"),
        file("${project.projectDir}/src/main/jniLibs/arm64-v8a/libsu.so"),
        file("${project.rootDir}/ethsu/target"),
        file("${project.rootDir}/ethinit/target"),
    )
}

tasks.clean {
    dependsOn("nativeClean")
}

ksp {
    arg("compose-destinations.defaultTransitions", "none")
}

dependencies {
    implementation(libs.androidx.appcompat)
    implementation(libs.google.material)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.webkit)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.runtime.livedata)

    debugImplementation(libs.androidx.compose.ui.test.manifest)
    debugImplementation(libs.androidx.compose.ui.tooling)

    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)

    implementation(libs.compose.destinations.core)
    ksp(libs.compose.destinations.ksp)

    implementation(libs.com.github.topjohnwu.libsu.core)

    implementation(libs.io.coil.kt.coil3.coil.compose)

    implementation(libs.kotlinx.coroutines.core)

    implementation(libs.okhttp)

    implementation(libs.markdown)

    implementation(libs.ini4j)

    compileOnly(libs.cxx)
    testImplementation(kotlin("test-junit"))
}
