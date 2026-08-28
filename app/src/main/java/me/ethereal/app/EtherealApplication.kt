package me.ethereal.app

import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.os.Build
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.util.Log
import android.widget.Toast
import androidx.core.content.edit
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import me.ethereal.app.ui.CrashHandleActivity
import me.ethereal.app.util.NativeAssets
import me.ethereal.app.util.Version
import me.ethereal.app.BuildConfig
import me.ethereal.app.util.becomeRoot
import me.ethereal.app.util.cleanupStaleModuleInstallFiles
import okhttp3.Cache
import okhttp3.OkHttpClient
import java.io.File
import java.security.SecureRandom
import java.util.Locale
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread
import kotlin.system.exitProcess

lateinit var etherealApp: EtherealApplication

const val TAG = "Ethereal"

class EtherealApplication : Application(), Thread.UncaughtExceptionHandler {
    lateinit var okhttpClient: OkHttpClient

    init {
        Thread.setDefaultUncaughtExceptionHandler(this)
    }

    enum class State {
        UNKNOWN_STATE,

        KERNEL_WORKING,

        SERVICE_NOT_INSTALLED,
        SERVICE_INSTALLED,
        SERVICE_INSTALLING,
        SERVICE_NEED_UPDATE,
        SERVICE_UNINSTALLING,
    }

    enum class ManagerAccessState {
        UNKNOWN,
        UNAVAILABLE,
        AUTHENTICATED,
    }


    companion object {
        const val ETHD_PATH = "/data/adb/ethd"
        const val ETHEREAL_FOLDER = "/data/adb/eth/"
        private const val ETHEREAL_BIN_FOLDER = ETHEREAL_FOLDER + "bin/"
        private const val ETHEREAL_LOG_FOLDER = ETHEREAL_FOLDER + "log/"
        private const val ETHD_LINK_PATH = ETHEREAL_BIN_FOLDER + "ethd"
        const val PACKAGE_CONFIG_FILE = ETHEREAL_FOLDER + "package_config"
        const val SU_PATH_FILE = ETHEREAL_FOLDER + "su_path"
        const val SAFEMODE_FILE = "/dev/.safemode"
        const val GLOBAL_NAMESPACE_FILE = "/data/adb/.global_namespace_enable"
        const val SUCOMPAT_FILE = ETHEREAL_FOLDER + "sucompat"
        const val SELINUX_HIDE_FILE = ETHEREAL_FOLDER + "selinux_hide"

        @Deprecated("Use 'ethd -V'")
        const val ETHEREAL_VERSION_PATH = ETHEREAL_FOLDER + "version"
        private const val MAGISKPOLICY_BIN_PATH = ETHEREAL_BIN_FOLDER + "magiskpolicy"
        private const val BUSYBOX_BIN_PATH = ETHEREAL_BIN_FOLDER + "busybox"
        private const val RESETPROP_BIN_PATH = ETHEREAL_BIN_FOLDER + "resetprop"
        const val DEFAULT_SCONTEXT = "u:r:untrusted_app:s0"
        const val MAGISK_SCONTEXT = "u:r:magisk:s0"

        const val DEFAULT_SU_PATH = "/system/bin/su"

        const val SP_NAME = "config"
        private const val MANAGER_TOKEN_FILE = "manager_token"
        private const val MANAGER_TOKEN_SIZE = 32
        private const val SHOW_BACKUP_WARN = "show_backup_warning"
        lateinit var sharedPreferences: SharedPreferences

        private val _kernelStateLiveData = MutableLiveData(State.UNKNOWN_STATE)
        val kernelStateLiveData: LiveData<State> = _kernelStateLiveData

        private val _serviceStateLiveData = MutableLiveData(State.UNKNOWN_STATE)
        val serviceStateLiveData: LiveData<State> = _serviceStateLiveData
        private val serviceState = AtomicReference(State.UNKNOWN_STATE)

        private val _managerAccessStateLiveData =
            MutableLiveData(ManagerAccessState.UNKNOWN)
        val managerAccessStateLiveData: LiveData<ManagerAccessState> =
            _managerAccessStateLiveData

        @Volatile
        private var kernelPresent = false

        private fun publishServiceState(state: State) {
            serviceState.set(state)
            _serviceStateLiveData.postValue(state)
        }

        private fun publishManagerAccessState(state: ManagerAccessState) {
            _managerAccessStateLiveData.postValue(state)
        }

        private fun beginServiceTransition(expected: Set<State>, next: State): Boolean {
            while (true) {
                val current = serviceState.get()
                if (current !in expected) return false
                if (serviceState.compareAndSet(current, next)) {
                    _serviceStateLiveData.postValue(next)
                    return true
                }
            }
        }

        @Synchronized
        fun ensureManagerToken(): Boolean {
            val existing = readManagerToken()
            if (existing != null) return true
            return runCatching {
                val token = ByteArray(MANAGER_TOKEN_SIZE)
                val random = SecureRandom()
                do {
                    random.nextBytes(token)
                } while (token.all { it == 0.toByte() })
                etherealApp.openFileOutput(MANAGER_TOKEN_FILE, Context.MODE_PRIVATE).use {
                    it.write(token)
                    it.fd.sync()
                }
                readManagerToken() != null
            }.onFailure {
                Log.e(TAG, "failed to create manager authentication token", it)
            }.getOrDefault(false)
        }

        fun readManagerToken(): ByteArray? {
            return runCatching {
                val file = File(etherealApp.filesDir, MANAGER_TOKEN_FILE)
                if (!file.isFile || file.length() != MANAGER_TOKEN_SIZE.toLong()) return null
                file.readBytes().takeIf { bytes ->
                    bytes.size == MANAGER_TOKEN_SIZE && bytes.any { it != 0.toByte() }
                }
            }.getOrNull()
        }

        fun requireManagerTokenFile(): File {
            val file = File(etherealApp.filesDir, MANAGER_TOKEN_FILE)
            check(readManagerToken() != null) {
                "manager authentication token is missing or invalid; restart Ethereal before patching"
            }
            return file
        }

        private fun copyExec(src: File, dst: File) {
            check(src.isFile && src.length() >= 64L) { "missing executable $src" }
            val parent = checkNotNull(dst.parentFile) { "missing parent for $dst" }
            check(parent.isDirectory || parent.mkdirs()) { "create $parent" }
            val pending = File(parent, ".${dst.name}.${System.nanoTime().toString(16)}.pending")
            try {
                src.inputStream().use { input ->
                    pending.outputStream().use { output ->
                        input.copyTo(output)
                        output.fd.sync()
                    }
                }
                check(pending.length() == src.length()) { "short copy $src -> $pending" }
                check(pending.setReadable(true, false) || pending.canRead()) {
                    "make readable $pending"
                }
                check(pending.setExecutable(true, false) || pending.canExecute()) {
                    "make executable $pending"
                }
                Os.rename(pending.absolutePath, dst.absolutePath)
                check(dst.isFile && dst.length() == src.length() && dst.canExecute()) {
                    "invalid installed executable $dst"
                }
            } finally {
                pending.delete()
            }
        }

        private fun linkTo(target: String, link: String) {
            runCatching { Os.remove(link) }.onFailure { error ->
                if (error !is ErrnoException || error.errno != OsConstants.ENOENT) throw error
            }
            Os.symlink(target, link)
            check(Os.readlink(link) == target) { "invalid symlink $link" }
        }

        private fun touch(path: String) {
            val f = File(path)
            val parent = checkNotNull(f.parentFile) { "missing parent for $f" }
            check(parent.isDirectory || parent.mkdirs()) { "create $parent" }
            check(f.exists() || f.createNewFile()) { "create $f" }
        }

        private fun runEthdCommand(vararg args: String): Int? {
            val process = ProcessBuilder(listOf(ETHD_PATH) + args)
                .redirectErrorStream(true)
                .start()
            val output = StringBuilder()
            val collector = thread(name = "ethd-${args.firstOrNull() ?: "probe"}-output", isDaemon = true) {
                runCatching {
                    process.inputStream.bufferedReader().useLines { lines ->
                        lines.take(128).forEach { line -> output.appendLine(line) }
                    }
                }
            }
            val finished = process.waitFor(15, TimeUnit.SECONDS)
            if (!finished) {
                process.destroyForcibly()
                process.waitFor(2, TimeUnit.SECONDS)
            }
            collector.join(2_000L)
            if (output.isNotBlank()) Log.d(TAG, "ethd ${args.joinToString(" ")}:\n$output")
            return if (finished) process.exitValue() else null
        }

        @Suppress("DEPRECATION")
        fun uninstallEthereal() {
            if (!beginServiceTransition(
                    setOf(State.SERVICE_INSTALLED),
                    State.SERVICE_UNINSTALLING,
                )
            ) return
            runCatching { Natives.resetSuPath(DEFAULT_SU_PATH) }
            runCatching {
                if (becomeRoot()) {
                    File(ETHD_PATH).delete()
                    File(ETHEREAL_BIN_FOLDER).deleteRecursively()
                    File(ETHEREAL_LOG_FOLDER).deleteRecursively()
                    File(ETHEREAL_VERSION_PATH).delete()
                }
            }
            Log.d(TAG, "Ethereal uninstalled...")
            publishServiceState(
                if (kernelPresent) State.SERVICE_NOT_INSTALLED else State.UNKNOWN_STATE
            )
        }

        @Suppress("DEPRECATION")
        fun installEthereal() {
            if (!beginServiceTransition(
                    setOf(State.SERVICE_NOT_INSTALLED, State.SERVICE_NEED_UPDATE),
                    State.SERVICE_INSTALLING,
                )
            ) return
            if (!becomeRoot()) {
                Log.e(TAG, "installEthereal: SuperCall did not make uid 0")
                publishServiceState(State.SERVICE_NOT_INSTALLED)
                return
            }
            runCatching { Natives.resetSuPath(DEFAULT_SU_PATH) }
            try {
                val nativeDir = File(etherealApp.applicationInfo.nativeLibraryDir)
                File(ETHEREAL_BIN_FOLDER).mkdirs()
                File(ETHEREAL_LOG_FOLDER).mkdirs()
                copyExec(File(nativeDir, "libethd.so"), File(ETHD_PATH))
                linkTo(ETHD_PATH, ETHD_LINK_PATH)
                linkTo(ETHD_PATH, MAGISKPOLICY_BIN_PATH)
                linkTo(ETHD_PATH, RESETPROP_BIN_PATH)
                copyExec(File(nativeDir, "libbusybox.so"), File(BUSYBOX_BIN_PATH))
                copyExec(File(nativeDir, "libramtool.so"), File("${ETHEREAL_BIN_FOLDER}ramtool"))
                copyExec(File(etherealApp.filesDir, "ethinit"), File("${ETHEREAL_BIN_FOLDER}ethinit"))
                val full = File(etherealApp.filesDir, "ethd.full")
                if (full.exists()) {
                    copyExec(full, File("${ETHEREAL_BIN_FOLDER}ethd.full"))
                }
                touch(PACKAGE_CONFIG_FILE)
                touch(SU_PATH_FILE)
                val suPath = File(SU_PATH_FILE)
                if (suPath.length() == 0L) suPath.writeText(DEFAULT_SU_PATH)
                File(ETHEREAL_VERSION_PATH).writeText(
                    Version.getManagerVersion().second.toString()
                )
                val requiredArtifactsReady = listOf(
                    File(ETHD_PATH),
                    File(BUSYBOX_BIN_PATH),
                    File("${ETHEREAL_BIN_FOLDER}ramtool"),
                    File("${ETHEREAL_BIN_FOLDER}ethinit"),
                    File(ETHD_LINK_PATH),
                    File(MAGISKPOLICY_BIN_PATH),
                    File(RESETPROP_BIN_PATH),
                ).all { it.isFile && it.length() >= 64L && it.canExecute() }
                val daemonProbeExitCode = runEthdCommand("--version")
                val sepolicyExitCode = if (daemonProbeExitCode == 0) {
                    runEthdCommand("sepolicy", "--magisk", "--live")
                } else null
                check(
                    serviceInstallSucceeded(
                        requiredArtifactsReady,
                        daemonProbeExitCode,
                        sepolicyExitCode,
                    )
                ) {
                    "service verification failed: artifacts=$requiredArtifactsReady " +
                        "daemon=$daemonProbeExitCode sepolicy=$sepolicyExitCode"
                }
                Log.d(TAG, "Ethereal installed...")
                publishServiceState(State.SERVICE_INSTALLED)
            } catch (e: Exception) {
                Log.e(TAG, "installEthereal", e)
                publishServiceState(State.SERVICE_NOT_INSTALLED)
            }
        }

        fun kernelModulePresent(): Boolean {
            return modulePathPresent {
                Os.access("/sys/module/ethereal", OsConstants.F_OK)
            }
        }

        fun refreshState() {
            publishManagerAccessState(ManagerAccessState.UNKNOWN)
            val present = kernelModulePresent()
            kernelPresent = present
            Log.d(TAG, "sys/module/ethereal present=$present")
            _kernelStateLiveData.postValue(
                if (present) State.KERNEL_WORKING else State.UNKNOWN_STATE
            )
            publishServiceState(
                if (present) State.SERVICE_NOT_INSTALLED else State.UNKNOWN_STATE
            )
            if (!present) {
                publishManagerAccessState(ManagerAccessState.UNAVAILABLE)
                return
            }
            thread {
                try {
                    var ready = false
                    repeat(3) { attempt ->
                        if (!ready && kernelModulePresent()) {
                            ready = Natives.ready()
                            if (!ready && attempt < 2) Thread.sleep(600L)
                        }
                    }
                    if (!ready) {
                        Log.w(TAG, "kernel module present but manager authentication failed")
                        publishManagerAccessState(ManagerAccessState.UNAVAILABLE)
                        publishServiceState(State.SERVICE_NOT_INSTALLED)
                        return@thread
                    }
                    publishManagerAccessState(ManagerAccessState.AUTHENTICATED)
                    deployBundledSu()
                    if (android.os.Process.myUid() == 0) installEthereal()
                    else publishServiceState(State.SERVICE_NOT_INSTALLED)
                } catch (t: Throwable) {
                    Log.e(TAG, "installEthereal", t)
                    publishManagerAccessState(ManagerAccessState.UNAVAILABLE)
                    publishServiceState(State.SERVICE_NOT_INSTALLED)
                }
            }
        }

        fun deployBundledSu() {
            try {
                val lib = File(etherealApp.applicationInfo.nativeLibraryDir, "libsu.so")
                val local = File(etherealApp.filesDir, "su")
                if (lib.exists()) {
                    lib.copyTo(local, overwrite = true)
                    local.setReadable(true, false)
                    local.setExecutable(true, false)
                }
                if (!becomeRoot()) {
                    Log.w(TAG, "deployBundledSu: SuperCall SU failed uid=${android.os.Process.myUid()}")
                    return
                }
                val src = when {
                    local.exists() -> local
                    lib.exists() -> lib
                    else -> return
                }
                File(ETHEREAL_BIN_FOLDER).mkdirs()
                File("/dev/.ethereal").mkdirs()
                for (d in arrayOf(
                    "/dev/.ethereal/su",
                    "${ETHEREAL_FOLDER}su",
                )) {
                    try {
                        copyExec(src, File(d))
                    } catch (e: Exception) {
                        Log.w(TAG, "copy su to $d: $e")
                    }
                }
                Log.d(TAG, "deployBundledSu done")
            } catch (e: Exception) {
                Log.e(TAG, "deployBundledSu", e)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        etherealApp = this

        val isArm64 = Build.SUPPORTED_ABIS.any { it == "arm64-v8a" }
        if (!isArm64) {
            Toast.makeText(applicationContext, "Unsupported architecture!", Toast.LENGTH_LONG)
                .show()
        }

        sharedPreferences = getSharedPreferences(SP_NAME, Context.MODE_PRIVATE)
        ensureManagerToken()
        runCatching { cleanupStaleModuleInstallFiles(cacheDir) }
            .onFailure { Log.w(TAG, "clean stale module archives", it) }
        runCatching { NativeAssets.stageAppFiles() }

        okhttpClient =
            OkHttpClient.Builder().cache(Cache(File(cacheDir, "okhttp"), 10 * 1024 * 1024))
                .addInterceptor { block ->
                    block.proceed(
                        block.request().newBuilder()
                            .header("User-Agent", "Ethereal/${BuildConfig.VERSION_CODE}")
                            .header("Accept-Language", Locale.getDefault().toLanguageTag()).build()
                    )
                }.build()
    }

    fun getBackupWarningState(): Boolean {
        return sharedPreferences.getBoolean(SHOW_BACKUP_WARN, true)
    }

    fun updateBackupWarningState(state: Boolean) {
        sharedPreferences.edit { putBoolean(SHOW_BACKUP_WARN, state) }
    }

    override fun uncaughtException(t: Thread, e: Throwable) {
        val exceptionMessage = Log.getStackTraceString(e)
        val threadName = t.name
        Log.e(TAG, "Error on thread $threadName:\n $exceptionMessage")
        runCatching {
            File(filesDir, "last_crash.txt").writeText("$threadName\n$exceptionMessage")
        }
        val main = try {
            android.os.Looper.getMainLooper().thread
        } catch (_: Throwable) {
            null
        }
        if (main != null && t !== main) {
            Log.e(TAG, "background crash kept process alive")
            return
        }
        runCatching {
            val intent = Intent(this, CrashHandleActivity::class.java).apply {
                putExtra("exception_message", exceptionMessage)
                putExtra("thread", threadName)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
            }
            startActivity(intent)
        }
        exitProcess(10)
    }
}
