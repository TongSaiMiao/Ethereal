package me.ethereal.app.util

import me.ethereal.app.etherealApp
import java.io.File

/** Unpack ramdisk helpers from assets. ET_EXEC binaries must not live in jniLibs. */
object NativeAssets {
    fun extract(asset: String, dest: File) {
        dest.parentFile?.mkdirs()
        etherealApp.assets.open(asset).use { input ->
            dest.outputStream().use { output -> input.copyTo(output) }
        }
        dest.setReadable(true, false)
        dest.setExecutable(true, false)
    }

    fun extractIfPresent(asset: String, dest: File): Boolean {
        return runCatching {
            etherealApp.assets.open(asset).use { }
            extract(asset, dest)
            dest.exists() && dest.length() > 64L
        }.getOrDefault(false)
    }

    fun stagePatchDir(dir: File) {
        dir.mkdirs()
        extractIfPresent("ethereal-init", File(dir, "ethinit"))
        extractIfPresent("su", File(dir, "su"))
        extractIfPresent("ethd.full", File(dir, "ethd.full"))
        val kmods = runCatching { etherealApp.assets.list("kmod") }.getOrNull() ?: emptyArray()
        for (n in kmods) {
            if (n.endsWith(".ko")) extractIfPresent("kmod/$n", File(dir, n))
        }
    }

    fun stageAppFiles() {
        val files = etherealApp.filesDir
        extractIfPresent("ethereal-init", File(files, "ethinit"))
        extractIfPresent("su", File(files, "su"))
        extractIfPresent("ethd.full", File(files, "ethd.full"))
    }
}
