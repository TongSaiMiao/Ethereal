plugins {
    alias(libs.plugins.agp.app) apply false
    alias(libs.plugins.kotlin) apply false
    alias(libs.plugins.kotlin.compose.compiler) apply false
}

extra.set("androidMinSdkVersion", 26)
extra.set("androidTargetSdkVersion", 36)
extra.set("androidCompileSdkVersion", 37)
extra.set("androidBuildToolsVersion", "36.1.0")
extra.set("androidCompileNdkVersion", "29.0.14206865")
extra.set("managerVersionCode", getVersionCode())
extra.set("managerVersionName", getVersionName())
extra.set("branchName", getBranch())
fun Project.exec(command: String) = providers.exec {
    commandLine(command.split(" "))
}.standardOutput.asText.get().trim()

fun getGitCommitCount(): Int {
    return exec("git rev-list --count HEAD").trim().toInt()
}

fun getGitDescribe(): String {
    return exec("git rev-parse --verify --short HEAD").trim()
}

fun getVersionCode(): Int {
    val props = java.util.Properties().apply {
        File(rootDir, "version.properties").inputStream().use { load(it) }
    }
    val epoch = props.getProperty("managerVersionEpoch").toInt()
    return epoch + getGitCommitCount()
}

fun getBranch(): String {
    return exec("git rev-parse --abbrev-ref HEAD").trim()
}

fun getVersionName(): String {
    return getGitDescribe()
}

tasks.register("printVersion") {
    doLast {
        println("Version code: ${project.extra["managerVersionCode"]}")
        println("Version name: ${project.extra["managerVersionName"]}")
    }
}
