package me.ethereal.app.util

import android.content.Context
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.system.Os
import android.util.Log
import com.topjohnwu.superuser.CallbackList
import com.topjohnwu.superuser.Shell
import com.topjohnwu.superuser.ShellUtils
import me.ethereal.app.EtherealApplication
import me.ethereal.app.BuildConfig
import me.ethereal.app.etherealApp
import me.ethereal.app.ui.screen.MODULE_TYPE
import java.io.File
import java.util.Properties

private const val TAG = "EtherealCli"

class RootShellInitializer : Shell.Initializer() {
    override fun onInit(context: Context, shell: Shell): Boolean {
        shell.newJob().add("export PATH=\$PATH:/system/bin:/system/xbin:/system_ext/bin:/vendor/bin:/dev").exec()
        return true
    }
}

private fun tryBuildShell(builder: Shell.Builder, vararg args: String): Shell? {
    return try {
        val sh = builder.build(*args)
        if (sh.isRoot) sh else {
            sh.close()
            null
        }
    } catch (e: Throwable) {
        Log.w(TAG, "shell cmd ${args.joinToString(" ")}: $e")
        null
    }
}

fun applyMagiskContext() {
    val ctx = EtherealApplication.MAGISK_SCONTEXT
    for (path in arrayOf("/proc/self/attr/current", "/proc/thread-self/attr/current")) {
        runCatching { File(path).writeText(ctx) }
    }
}

/** SuperCall only. Never exec a `su` binary from the app sandbox — ColorOS
 *  treats that as a root request and kills the process if the user denies. */
fun becomeRoot(): Boolean {
    runCatching { me.ethereal.app.Natives.su(0, EtherealApplication.MAGISK_SCONTEXT) }
    if (android.os.Process.myUid() != 0) {
        runCatching { me.ethereal.app.Natives.su() }
    }
    if (android.os.Process.myUid() != 0) return false
    applyMagiskContext()
    return true
}

private fun openRootShell(globalMnt: Boolean): Shell? {
    Shell.enableVerboseLogging = BuildConfig.DEBUG
    if (!becomeRoot()) {
        Log.e(TAG, "not uid 0 after SuperCall, skip su/sh exec")
        return null
    }
    val builder = Shell.Builder.create()
        .setTimeout(5)
        .setInitializers(RootShellInitializer::class.java)
    tryBuildShell(builder, "/system/bin/sh")?.let { return it }
    Log.e(TAG, "/system/bin/sh failed as uid 0")
    return null
}

fun createRootShell(globalMnt: Boolean = false): Shell {
    return openRootShell(globalMnt)
        ?: throw IllegalStateException("no root shell")
}

fun tryGetRootShell(globalMnt: Boolean = false): Shell? {
    return runCatching { openRootShell(globalMnt) }.getOrNull()
}

object EtherealCli {
    private val lock = Any()

    @Volatile
    private var shell: Shell? = null

    @Volatile
    private var globalShell: Shell? = null

    val SHELL: Shell
        get() = shell(false)

    val GLOBAL_MNT_SHELL: Shell
        get() = shell(true)

    fun shell(globalMnt: Boolean): Shell {
        val cached = if (globalMnt) globalShell else shell
        if (cached != null && cached.isRoot) return cached
        synchronized(lock) {
            val again = if (globalMnt) globalShell else shell
            if (again != null && again.isRoot) return again
            val opened = openRootShell(globalMnt)
                ?: throw IllegalStateException("Unable to create a root shell")
            if (globalMnt) globalShell = opened else {
                shell = opened
                runCatching {
                    Shell.setDefaultBuilder(
                        Shell.Builder.create()
                            .setTimeout(5)
                            .setCommands("/system/bin/sh")
                            .setInitializers(RootShellInitializer::class.java)
                    )
                }
            }
            return opened
        }
    }

    @Synchronized
    fun refresh() {
        val old = shell
        val oldGlobal = globalShell
        shell = null
        globalShell = null
        runCatching { old?.close() }
        runCatching { oldGlobal?.close() }
        runCatching { openRootShell(false)?.also { shell = it } }
    }
}

fun getRootShell(globalMnt: Boolean = false): Shell {
    return tryGetRootShell(globalMnt)
        ?: runCatching { EtherealCli.shell(globalMnt) }.getOrNull()
        ?: throw IllegalStateException("no root shell")
}

inline fun <T> withNewRootShell(
    globalMnt: Boolean = false,
    block: Shell.() -> T
): T {
    return createRootShell(globalMnt).use(block)
}

fun rootAvailable(): Boolean {
    return tryGetRootShell()?.isRoot == true
}

fun shellForResult(shell: Shell, vararg cmds: String): Shell.Result {
    val out = ArrayList<String>()
    val err = ArrayList<String>()
    return shell.newJob().add(*cmds).to(out, err).exec()
}

fun rootShellForResult(vararg cmds: String): Shell.Result {
    val out = ArrayList<String>()
    val err = ArrayList<String>()
    val shell = tryGetRootShell() ?: throw IllegalStateException("no root shell")
    return shell.newJob().add(*cmds).to(out, err).exec()
}

fun execEthd(args: String, newShell: Boolean = false): Boolean {
    return runCatching {
        if (newShell) {
            withNewRootShell {
                ShellUtils.fastCmdResult(this, "${EtherealApplication.ETHD_PATH} $args")
            }
        } else {
            val shell = tryGetRootShell() ?: return false
            ShellUtils.fastCmdResult(shell, "${EtherealApplication.ETHD_PATH} $args")
        }
    }.getOrDefault(false)
}

fun listModules(): String {
    val shell = tryGetRootShell() ?: return "[]"
    return runCatching {
        val out = shell.newJob().add("${EtherealApplication.ETHD_PATH} module list").to(ArrayList(), null).exec().out
        out.joinToString("\n").ifBlank { "[]" }
    }.getOrDefault("[]")
}

fun hasMetaModule(): Boolean {
    return getMetaModuleImplement() != "None"
}

fun getMetaModuleImplement(): String {
    try {
        val metaModuleProp = File("/data/adb/metamodule/module.prop")
        if (!metaModuleProp.isFile) {
            Log.i(TAG, "Meta module implement: None")
            return "None"
        }

        val prop = Properties()
        metaModuleProp.inputStream().use { prop.load(it) }

        val name = prop.getProperty("name")
        Log.i(TAG, "Meta module implement: $name")
        return name
    } catch (t : Throwable) {
        Log.i(TAG, "Meta module implement: None")
        return "None"
    }
}

fun toggleModule(id: String, enable: Boolean): Boolean {
    val cmd = if (enable) {
        "module enable $id"
    } else {
        "module disable $id"
    }
    val result = execEthd(cmd,true)
    Log.i(TAG, "$cmd result: $result")
    return result
}

fun uninstallModule(id: String): Boolean {
    val cmd = "module uninstall $id"
    val result = execEthd(cmd,true)
    Log.i(TAG, "uninstall module $id result: $result")
    return result
}

fun undoRemoveModule(id: String): Boolean {
    val cmd = "module undo-uninstall $id"
    val result = execEthd(cmd,true)
    Log.i(TAG, "undo-uninstall module $id result: $result")
    return result
}

fun installModule(
    uri: Uri, type: MODULE_TYPE, onFinish: (Boolean) -> Unit, onStdout: (String) -> Unit, onStderr: (String) -> Unit
): Boolean = withModuleInstallArchive(etherealApp.cacheDir) { file ->
    val resolver = etherealApp.contentResolver
    try {
        resolver.openInputStream(uri)?.use { input ->
            stageBoundedFile(input, file)
        } ?: error("Unable to open selected module archive")
    } catch (t: Throwable) {
        Log.e(TAG, "stage module archive failed", t)
        onStderr(t.message ?: "Unable to stage selected module archive")
        onFinish(false)
        return@withModuleInstallArchive false
    }

    val stdoutCallback: CallbackList<String?> = object : CallbackList<String?>() {
        override fun onAddElement(s: String?) {
            onStdout(s ?: "")
        }
    }

    val stderrCallback: CallbackList<String?> = object : CallbackList<String?>() {
        override fun onAddElement(s: String?) {
            onStderr(s ?: "")
        }
    }

    val shell = tryGetRootShell() ?: run {
        onFinish(false)
        return@withModuleInstallArchive false
    }

    val result = try {
        if (type == MODULE_TYPE.MODULE) {
            val cmd = "${EtherealApplication.ETHD_PATH} module install ${file.absolutePath}"
            shell.newJob().add(cmd).to(stdoutCallback, stderrCallback)
                .exec().isSuccess
        } else {
            false
        }
    } catch (t: Throwable) {
        Log.e(TAG, "install $type module failed", t)
        onStderr(t.message ?: "Module installation failed")
        false
    }

    Log.i(TAG, "install $type module $uri result: $result")
    onFinish(result)
    result
}

fun runModuleAction(
    moduleId: String, onStdout: (String) -> Unit, onStderr: (String) -> Unit
): Boolean {
    val stdoutCallback: CallbackList<String?> = object : CallbackList<String?>() {
        override fun onAddElement(s: String?) {
            onStdout(s ?: "")
        }
    }

    val stderrCallback: CallbackList<String?> = object : CallbackList<String?>() {
        override fun onAddElement(s: String?) {
            onStderr(s ?: "")
        }
    }

    return runCatching {
        val result = withNewRootShell {
            newJob().add("${EtherealApplication.ETHD_PATH} module action $moduleId")
                .to(stdoutCallback, stderrCallback).exec()
        }
        Log.i(TAG, "Modules runAction result: $result")
        result.isSuccess
    }.getOrDefault(false)
}

fun reboot(reason: String = "") {
    val shell = tryGetRootShell() ?: return
    runCatching {
        if (reason == "recovery") {
            shell.newJob().add("/system/bin/input keyevent 26").exec()
        }
        shell.newJob()
            .add("/system/bin/svc power reboot $reason || /system/bin/reboot $reason").exec()
    }
}

/**
 * Detect the Kernel Module Interface (KMI) of the running kernel, e.g.
 * `android14-5.15`, from `uname -r`.
 */
fun getKmi(): String? {
    val release = runCatching { Os.uname().release }.getOrNull() ?: return null
    return inferKmi(release, getInitialSdk())
}

internal fun getInitialSdk(): Int {
    val currentSdk = Build.VERSION.SDK_INT
    runCatching {
        Build.VERSION::class.java.getField("DEVICE_INITIAL_SDK_INT").getInt(null)
    }.getOrNull()?.takeIf { it > 0 }?.let { return it }
    return runCatching {
        val systemProperties = Class.forName("android.os.SystemProperties")
        val get = systemProperties.getMethod(
            "get",
            String::class.java,
            String::class.java,
        )
        (get.invoke(null, "ro.product.first_api_level", "") as? String)
            ?.toIntOrNull()
    }.getOrNull()?.takeIf { it > 0 } ?: currentSdk
}

private val supportedKmis = setOf(
    "android12-5.4",
    "android12-5.10",
    "android13-5.10",
    "android13-5.15",
    "android14-5.15",
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
)

internal fun inferKmi(release: String, initialSdk: Int): String? {
    val version = Regex("^(\\d+)\\.(\\d+)").find(release) ?: return null
    val majorMinor = "${version.groupValues[1]}.${version.groupValues[2]}"
    Regex("android(\\d+)").find(release)?.groupValues?.get(1)?.let { generation ->
        return "android$generation-$majorMinor".takeIf(supportedKmis::contains)
    }

    return when (majorMinor) {
        "6.12" -> "android16-6.12"
        "6.6" -> "android15-6.6"
        "6.1" -> "android14-6.1"
        "5.15" -> when (initialSdk) {
            33 -> "android13-5.15"
            34 -> "android14-5.15"
            else -> null
        }
        "5.10" -> when (initialSdk) {
            31, 32 -> "android12-5.10"
            33 -> "android13-5.10"
            else -> null
        }
        "5.4" -> "android12-5.4".takeIf { initialSdk == 31 || initialSdk == 32 }
        else -> null
    }
}

/**
 * Running kernel version as a comparable integer, e.g. `4.19` -> 419,
 * `5.10` -> 510, `6.1` -> 601. Returns null if it can't be parsed.
 */
fun getKernelVersionCode(): Int? {
    val release = runCatching { Os.uname().release }.getOrNull() ?: return null
    val m = Regex("^(\\d+)\\.(\\d+)").find(release) ?: return null
    val major = m.groupValues[1].toIntOrNull() ?: return null
    val minor = m.groupValues[2].toIntOrNull() ?: return null
    return major * 100 + minor
}

/** Whether the running kernel is a GKI kernel (i.e. exposes `android<N>` in uname). */
fun isGkiKernel(): Boolean = getKmi() != null

fun hasMagisk(): Boolean {
    val shell = tryGetRootShell() ?: return false
    return runCatching {
        val result = shell.newJob().add("nsenter --mount=/proc/1/ns/mnt which magisk").exec()
        Log.i(TAG, "has magisk: ${result.isSuccess}")
        result.isSuccess
    }.getOrDefault(false)
}

fun isGlobalNamespaceEnabled(): Boolean {
    val shell = tryGetRootShell() ?: return false
    return runCatching {
        val result = ShellUtils.fastCmd(shell, "cat ${EtherealApplication.GLOBAL_NAMESPACE_FILE}")
        Log.i(TAG, "is global namespace enabled: $result")
        result == "1"
    }.getOrDefault(false)
}

fun setGlobalNamespaceEnabled(value: String) {
    val shell = tryGetRootShell() ?: return
    runCatching {
        shell.newJob().add("echo $value > ${EtherealApplication.GLOBAL_NAMESPACE_FILE}")
            .submit { result ->
                Log.i(TAG, "setGlobalNamespaceEnabled result: ${result.isSuccess} [${result.out}]")
            }
    }
}

fun getFileNameFromUri(context: Context, uri: Uri): String? {
    var fileName: String? = null
    val cursor: Cursor? = context.contentResolver.query(uri, null, null, null, null)
    cursor?.use {
        if (it.moveToFirst()) {
            fileName = it.getString(it.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME))
        }
    }
    return fileName
}
