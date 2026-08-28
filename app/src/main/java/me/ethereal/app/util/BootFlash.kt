package me.ethereal.app.util

import android.content.ContentValues
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Parcelable
import android.provider.MediaStore
import android.util.Log
import kotlinx.parcelize.Parcelize
import me.ethereal.app.EtherealApplication
import me.ethereal.app.etherealApp
import java.io.File
import java.io.FileOutputStream
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption

private const val TAG = "BootFlash"
private const val MAX_BOOT_INPUT_BYTES = 512L * 1024 * 1024
private const val MAX_KO_INPUT_BYTES = 64L * 1024 * 1024

@Parcelize
sealed class LkmSelection : Parcelable {
    @Parcelize
    data class LkmUri(val uriString: String) : LkmSelection()
    @Parcelize
    data class KmiString(val value: String) : LkmSelection()
    @Parcelize
    data object KmiNone : LkmSelection()
}

@Parcelize
sealed class FlashIt : Parcelable {
    @Parcelize
    sealed class BootSource : Parcelable {
        @Parcelize
        data class ImageFile(val image: String) : BootSource()

        @Parcelize
        data object Direct : BootSource()
    }

    @Parcelize
    data class FlashBoot(
        val source: BootSource,
        val lkm: LkmSelection,
        val ota: Boolean,
    ) : FlashIt()

}

private fun sysprop(key: String): String {
    return runCatching {
        val c = Class.forName("android.os.SystemProperties")
        c.getMethod("get", String::class.java, String::class.java).invoke(c, key, "") as String
    }.getOrDefault("")
}

fun slotSuffix(ota: Boolean = false): String {
    val current = sequenceOf(
        sysprop("ro.boot.slot_suffix"),
        sysprop("ro.boot.slot"),
    ).mapNotNull { value ->
        when (value.trim().lowercase()) {
            "a", "_a" -> "_a"
            "b", "_b" -> "_b"
            else -> null
        }
    }.firstOrNull() ?: return ""
    if (!ota) return current
    return if (current == "_a") "_b" else "_a"
}

internal fun defaultPartitionFor(release: String, initialSdk: Int): String {
    val kmi = inferKmi(release, initialSdk)
    val generation = kmi
        ?.let { Regex("^android(\\d+)-").find(it)?.groupValues?.get(1)?.toIntOrNull() }
        ?: Regex("android(\\d+)").find(release)?.groupValues?.get(1)?.toIntOrNull()
    return if (generation != null && generation >= 13) "init_boot" else "boot"
}

fun defaultPartition(): String {
    val release = runCatching { android.system.Os.uname().release }.getOrDefault("")
    return defaultPartitionFor(release, getInitialSdk())
}

fun isGki2Device(): Boolean = defaultPartition() == "init_boot"

fun resolvePartitionDev(name: String, ota: Boolean): String? {
    val suffix = slotSuffix(ota)
    if (ota && suffix.isEmpty()) return null
    return resolvePartitionDevForSuffix(name, suffix)
}

private fun resolvePartitionDevForSuffix(name: String, suffix: String): String? {
    val roots = listOf("/dev/block/by-name", "/dev/block/bootdevice/by-name")
    // Never mix a slotted boot with an unslotted init_boot. A pair must come
    // from exactly the same slot suffix or the operation is rejected.
    val candidates = roots.map { "$it/$name$suffix" }
    return candidates.firstOrNull { File(it).exists() }
}

private fun copyUri(uri: Uri, dest: File, maxBytes: Long = MAX_BOOT_INPUT_BYTES) {
    dest.parentFile?.mkdirs()
    try {
        etherealApp.contentResolver.openInputStream(uri)?.use { input ->
            FileOutputStream(dest).use { output ->
                val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                var total = 0L
                while (true) {
                    val read = input.read(buffer)
                    if (read < 0) break
                    if (read == 0) continue
                    total = Math.addExact(total, read.toLong())
                    check(total <= maxBytes) {
                        "selected input exceeds ${maxBytes / (1024 * 1024)} MiB"
                    }
                    output.write(buffer, 0, read)
                }
                output.fd.sync()
                check(total > 0L) { "selected input is empty" }
            }
        } ?: error("open $uri")
    } catch (t: Throwable) {
        dest.delete()
        throw t
    }
}

private fun nativeLib(name: String): File {
    return File(etherealApp.applicationInfo.nativeLibraryDir, name)
}

private fun stageTools(work: File, line: (String) -> Unit) {
    work.mkdirs()
    NativeAssets.stagePatchDir(work)
    // The APK keeps a compatibility asset for explicit selection, but it must
    // never become an implicit 6.1 fallback for an unknown/ambiguous KMI.
    File(work, "ethereal.ko").delete()
    val kmi = getKmi()
    val release = runCatching { android.system.Os.uname().release }.getOrDefault("")
    line("- uname $release")
    line("- kmi ${kmi ?: "(none)"}")
    line("- no generic KO selected; first-stage loader will choose an exact named KMI")
    work.listFiles()?.filter { it.name.startsWith("ethereal") && it.name.endsWith(".ko") }?.forEach {
        line("- staged ${it.name} ${it.length()}")
    }
}

/** ColorOS 16 denies execve() of ELFs under app data. /system/bin/sh is
 *  allowed after SuperCall; the script copies tools to /data/adb and runs. */
private fun runSh(script: String, cwd: File, onLine: (String) -> Unit): Int {
    if (!becomeRoot()) error("SuperCall root required to patch")
    cwd.mkdirs()
    val file = File(cwd, "run.sh")
    file.writeText(script.replace("\r\n", "\n"))
    file.setReadable(true, false)
    onLine("+ /system/bin/sh ${file.absolutePath}")
    val p = ProcessBuilder("/system/bin/sh", file.absolutePath)
        .directory(cwd)
        .redirectErrorStream(true)
        .start()
    p.inputStream.bufferedReader().forEachLine(onLine)
    return p.waitFor()
}

private fun runProcess(args: List<String>, cwd: File, onLine: (String) -> Unit): Int {
    onLine("+ ${args.joinToString(" ")}")
    val process = ProcessBuilder(args)
        .directory(cwd)
        .redirectErrorStream(true)
        .start()
    process.inputStream.bufferedReader().forEachLine(onLine)
    return process.waitFor()
}

internal fun selectedImageOutputName(displayName: String?, uriLastPathSegment: String?): String {
    fun baseName(value: String?): String? = value
        ?.trim()
        ?.substringAfterLast('/')
        ?.substringAfterLast('\\')
        ?.takeIf { it.isNotBlank() }

    baseName(displayName)?.let { return it }

    // Some document providers are shy about DISPLAY_NAME. Keep a real .img basename
    // when the URI gives us one; otherwise stay neutral instead of guessing a partition.
    return baseName(uriLastPathSegment)
        ?.substringAfterLast(':')
        ?.takeIf { it.endsWith(".img", ignoreCase = true) }
        ?: "selected-image.img"
}

private fun selectedName(uri: Uri): String {
    val queried = runCatching {
        etherealApp.contentResolver.query(
            uri,
            arrayOf(android.provider.OpenableColumns.DISPLAY_NAME),
            null,
            null,
            null,
        )?.use { cursor ->
            val index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
            if (index >= 0 && cursor.moveToFirst()) cursor.getString(index) else null
        }
    }.getOrNull()
    return selectedImageOutputName(queried, uri.lastPathSegment)
}

private fun replaceWithRollback(pending: File, output: File) {
    val backup = File(output.parentFile, ".${output.name}.${System.nanoTime().toString(16)}.backup")
    var oldMoved = false
    try {
        if (output.exists()) {
            Files.move(output.toPath(), backup.toPath())
            oldMoved = true
        }
        Files.move(pending.toPath(), output.toPath())
        if (oldMoved) Files.deleteIfExists(backup.toPath())
    } catch (t: Throwable) {
        if (oldMoved && !output.exists()) {
            runCatching { Files.move(backup.toPath(), output.toPath()) }
        }
        throw t
    } finally {
        Files.deleteIfExists(pending.toPath())
        if (output.exists()) Files.deleteIfExists(backup.toPath())
    }
}

private fun replaceAtomically(pending: File, output: File) {
    try {
        Files.move(
            pending.toPath(),
            output.toPath(),
            StandardCopyOption.ATOMIC_MOVE,
            StandardCopyOption.REPLACE_EXISTING,
        )
    } catch (_: AtomicMoveNotSupportedException) {
        replaceWithRollback(pending, output)
    }
}

internal fun publishLegacyPatchedImage(
    source: File,
    downloads: File,
    safeName: String,
    nonce: String = System.nanoTime().toString(16),
    replace: (File, File) -> Unit = ::replaceAtomically,
): File {
    require(source.isFile && source.length() > 0L) { "patched image is missing or empty" }
    require(safeName == File(safeName).name && safeName.isNotBlank()) { "invalid output name" }
    check(downloads.isDirectory || downloads.mkdirs()) { "create ${downloads.absolutePath}" }

    val output = File(downloads, safeName)
    val pending = File(downloads, ".$safeName.$nonce.pending")
    check(!pending.exists()) { "staging file already exists: ${pending.name}" }
    try {
        FileOutputStream(pending).use { out ->
            source.inputStream().use { input -> input.copyTo(out) }
            out.fd.sync()
        }
        check(pending.length() == source.length()) { "staged image size changed" }
        replace(pending, output)
        check(output.isFile && output.length() == source.length()) { "publish Downloads/$safeName" }
        return output
    } finally {
        runCatching { Files.deleteIfExists(pending.toPath()) }
    }
}

internal fun requireMediaStorePublished(updatedRows: Int, safeName: String) {
    check(updatedRows > 0) { "publish Downloads/$safeName" }
}

private fun publishPatchedImage(source: File, displayName: String): String {
    val safeName = "Ethereal-${displayName.substringAfterLast('/').substringAfterLast('\\')}"
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
        val downloads = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
        return publishLegacyPatchedImage(source, downloads, safeName).absolutePath
    }

    val values = ContentValues().apply {
        put(MediaStore.MediaColumns.DISPLAY_NAME, safeName)
        put(MediaStore.MediaColumns.MIME_TYPE, "application/octet-stream")
        put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
        put(MediaStore.MediaColumns.IS_PENDING, 1)
    }
    val resolver = etherealApp.contentResolver
    val collection = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
    val uri = resolver.insert(collection, values) ?: error("create Downloads/$safeName")
    try {
        resolver.openOutputStream(uri, "w")?.use { output ->
            source.inputStream().use { input -> input.copyTo(output) }
        } ?: error("open output $uri")
        values.clear()
        values.put(MediaStore.MediaColumns.IS_PENDING, 0)
        requireMediaStorePublished(resolver.update(uri, values, null, null), safeName)
        return "Downloads/$safeName"
    } catch (t: Throwable) {
        runCatching { resolver.delete(uri, null, null) }
        throw t
    }
}

fun runFlash(
    item: FlashIt,
    onFinish: (Boolean, Int) -> Unit,
    onStdout: (String) -> Unit,
    onStderr: (String) -> Unit,
) {
    val line: (String) -> Unit = { s ->
        onStdout(s)
        Log.i(TAG, s)
    }
    try {
        when (item) {
            is FlashIt.FlashBoot -> flashBoot(item, line, onFinish)
        }
    } catch (t: Throwable) {
        onStderr(t.message ?: t.toString())
        line("FAILED: ${t.message}")
        onFinish(false, 1)
    }
}

private fun flashBoot(
    item: FlashIt.FlashBoot,
    line: (String) -> Unit,
    onFinish: (Boolean, Int) -> Unit,
) {
    val work = File(etherealApp.cacheDir, "bootflash")
    work.deleteRecursively()
    stageTools(work, line)
    when (val lkm = item.lkm) {
        is LkmSelection.LkmUri -> copyUri(
            Uri.parse(lkm.uriString),
            File(work, "ethereal.ko"),
            MAX_KO_INPUT_BYTES,
        )
        is LkmSelection.KmiString -> {
            val selected = File(work, "ethereal.${lkm.value}.ko")
            if (!selected.isFile) error("kernel module ${lkm.value} not bundled")
            selected.copyTo(File(work, "ethereal.ko"), overwrite = true)
        }
        else -> Unit
    }
    val imgIn = File(work, "in.img")
    val managerUid = etherealApp.applicationInfo.uid
    if (managerUid <= 0) error("invalid manager uid $managerUid")
    val managerToken = EtherealApplication.requireManagerTokenFile()
    val native = etherealApp.applicationInfo.nativeLibraryDir
    val ethd = nativeLib("libethd.so")
    val ethinit = File(work, "ethinit")
    val ko = File(work, "ethereal.ko")
    val su = File(work, "su")

    fun addPayloadArgs(args: MutableList<String>) {
        if (ethinit.isFile) args += listOf("--ethinit", ethinit.absolutePath)
        if (ko.isFile) args += listOf("--ko", ko.absolutePath)
    }

    // Offline really means offline. Keep this branch above becomeRoot(); picking a file
    // must never summon SuperCall or a root permission dialog.
    if (item.source !is FlashIt.BootSource.Direct) {
        if (!ethd.isFile || !ethd.canExecute()) error("Ethereal patcher is not executable")
        if (!nativeLib("libramtool.so").isFile) error("ramtool is not bundled")

        when (val source = item.source) {
            is FlashIt.BootSource.ImageFile -> {
                if (source.image.isBlank()) error("boot or init_boot image is required")
                val inputUri = Uri.parse(source.image)
                line("- copy selected boot/init_boot image")
                copyUri(inputUri, imgIn)
                val output = File(work, "out.img")
                val args = mutableListOf(
                    ethd.absolutePath,
                    "boot-patch",
                    "--image", imgIn.absolutePath,
                    "--out", output.absolutePath,
                    "--manager-uid", managerUid.toString(),
                    "--manager-token-file", managerToken.absolutePath,
                )
                addPayloadArgs(args)
                val rc = runProcess(args, work, line)
                if (rc != 0) error("boot-patch failed $rc")
                if (!output.isFile || output.length() == 0L) error("patched image not written")
                val published = publishPatchedImage(output, selectedName(inputUri))
                line("- wrote $published")
            }

            FlashIt.BootSource.Direct -> error("invalid file patch source")
        }
        line("- no physical partition was touched")
        onFinish(false, 0)
        return
    }

    if (!becomeRoot()) error("SuperCall root required to flash a partition")

    val suffix = slotSuffix(item.ota)
    if (item.ota && suffix.isEmpty()) error("inactive slot is unavailable")
    val gki2 = isGki2Device()
    val bootDev = resolvePartitionDevForSuffix("boot", suffix)
        ?: error("boot$suffix partition not found")
    val initBootDev = if (gki2) {
        resolvePartitionDevForSuffix("init_boot", suffix)
            ?: error("init_boot$suffix partition not found")
    } else null

    val script = buildString {
        appendLine("set -e")
        appendLine("ADB=/data/adb/eth/flash")
        appendLine("mkdir -p \"\$ADB\"")
        appendLine("chmod 700 \"\$ADB\"")
        appendLine("cp -f \"$native/libethd.so\" \"\$ADB/ethd\"")
        appendLine("cp -f \"$native/libramtool.so\" \"\$ADB/ramtool\"")
        if (ethinit.exists()) appendLine("cp -f \"${ethinit.absolutePath}\" \"\$ADB/ethinit\"")
        appendLine("cp -f \"${work.absolutePath}\"/ethereal*.ko \"\$ADB/\" 2>/dev/null || true")
        if (su.exists()) appendLine("cp -f \"${su.absolutePath}\" \"\$ADB/su\"")
        appendLine("chmod 755 \"\$ADB/ethd\" \"\$ADB/ramtool\"")
        appendLine("cd \"\$ADB\"")
        appendLine("rm -f in.img out.img in-init_boot.img in-boot.img out-init_boot.img out-boot.img")
        if (gki2) {
            appendLine("echo \"- dump init_boot$suffix + boot$suffix\"")
            appendLine("/system/bin/dd if=$initBootDev of=in-init_boot.img bs=4096")
            appendLine("/system/bin/dd if=$bootDev of=in-boot.img bs=4096")
            append("./ethd boot-patch-pair --init-boot in-init_boot.img --boot in-boot.img")
            append(" --out-init-boot out-init_boot.img --out-boot out-boot.img")
            append(" --manager-uid $managerUid")
            append(" --manager-token-file \"${managerToken.absolutePath}\"")
            if (ethinit.exists()) append(" --ethinit ethinit")
            if (ko.exists()) append(" --ko ethereal.ko")
            appendLine()
            appendLine("test -s out-init_boot.img && test -s out-boot.img")
            appendLine("test \"\$(wc -c < out-init_boot.img)\" = \"\$(wc -c < in-init_boot.img)\"")
            appendLine("test \"\$(wc -c < out-boot.img)\" = \"\$(wc -c < in-boot.img)\"")
            appendLine("echo \"- flash init_boot$suffix first\"")
            appendLine("if ! /system/bin/dd if=out-init_boot.img of=$initBootDev bs=4096 conv=fsync || ! /system/bin/toybox cmp -s out-init_boot.img $initBootDev; then")
            appendLine("  echo \"- init_boot write/verify failed; restoring backup\"")
            appendLine("  /system/bin/dd if=in-init_boot.img of=$initBootDev bs=4096 conv=fsync || true")
            appendLine("  /system/bin/sync")
            appendLine("  exit 1")
            appendLine("fi")
            appendLine("echo \"- flash boot$suffix second\"")
            appendLine("if ! /system/bin/dd if=out-boot.img of=$bootDev bs=4096 conv=fsync || ! /system/bin/toybox cmp -s out-boot.img $bootDev; then")
            appendLine("  echo \"- boot write/verify failed; restoring both backups\"")
            appendLine("  /system/bin/dd if=in-boot.img of=$bootDev bs=4096 conv=fsync || true")
            appendLine("  /system/bin/dd if=in-init_boot.img of=$initBootDev bs=4096 conv=fsync || true")
            appendLine("  /system/bin/sync")
            appendLine("  exit 1")
            appendLine("fi")
        } else {
            appendLine("echo \"- dump boot$suffix\"")
            appendLine("/system/bin/dd if=$bootDev of=in.img bs=4096")
            append("./ethd boot-patch --image in.img --out out.img --manager-uid $managerUid")
            append(" --manager-token-file \"${managerToken.absolutePath}\"")
            if (ethinit.exists()) append(" --ethinit ethinit")
            if (ko.exists()) append(" --ko ethereal.ko")
            appendLine()
            appendLine("test -s out.img")
            appendLine("test \"\$(wc -c < out.img)\" = \"\$(wc -c < in.img)\"")
            appendLine("echo \"- flash boot$suffix\"")
            appendLine("if ! /system/bin/dd if=out.img of=$bootDev bs=4096 conv=fsync || ! /system/bin/toybox cmp -s out.img $bootDev; then")
            appendLine("  echo \"- boot write/verify failed; restoring backup\"")
            appendLine("  /system/bin/dd if=in.img of=$bootDev bs=4096 conv=fsync || true")
            appendLine("  /system/bin/sync")
            appendLine("  exit 1")
            appendLine("fi")
        }
        appendLine("/system/bin/sync")
        appendLine("rm -f in.img out.img in-init_boot.img in-boot.img out-init_boot.img out-boot.img")
        appendLine("echo \"- done, reboot to apply\"")
    }
    val rc = runSh(script, work, line)
    if (rc != 0) error("boot-patch failed $rc")
    onFinish(true, 0)
}

fun isKoFile(uri: Uri): Boolean {
    val seg = uri.lastPathSegment ?: ""
    if (seg.endsWith(".ko", ignoreCase = true)) return true
    return try {
        etherealApp.contentResolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { c ->
                val idx = c.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                idx >= 0 && c.moveToFirst() && (c.getString(idx)?.endsWith(".ko", true) == true)
            } ?: false
    } catch (_: Throwable) {
        false
    }
}
