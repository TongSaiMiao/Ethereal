package me.ethereal.app.util

import java.io.File
import java.io.FileOutputStream
import java.io.InputStream
import java.nio.file.Files
import java.nio.file.StandardCopyOption

internal const val MAX_MODULE_ARCHIVE_BYTES = 512L * 1024 * 1024

private fun publishAtomically(pending: File, output: File) {
    Files.move(
        pending.toPath(),
        output.toPath(),
        StandardCopyOption.ATOMIC_MOVE,
        StandardCopyOption.REPLACE_EXISTING,
    )
}

internal fun stageBoundedFile(
    input: InputStream,
    output: File,
    maxBytes: Long = MAX_MODULE_ARCHIVE_BYTES,
    publish: (File, File) -> Unit = ::publishAtomically,
): File {
    require(maxBytes > 0) { "maxBytes must be positive" }
    val parent = output.parentFile ?: error("output has no parent")
    check(parent.isDirectory || parent.mkdirs()) { "create ${parent.absolutePath}" }

    Files.deleteIfExists(output.toPath())
    val pending = File.createTempFile(".${output.name}.", ".pending", parent)
    var total = 0L
    try {
        FileOutputStream(pending).use { staged ->
            val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                if (read == 0) continue
                total = Math.addExact(total, read.toLong())
                check(total <= maxBytes) {
                    "module archive exceeds ${maxBytes / (1024 * 1024)} MiB"
                }
                staged.write(buffer, 0, read)
            }
            check(total > 0L) { "module archive is empty" }
            staged.fd.sync()
        }

        publish(pending, output)
        check(output.isFile && output.length() == total) { "staged module archive changed" }
        return output
    } catch (t: Throwable) {
        Files.deleteIfExists(output.toPath())
        throw t
    } finally {
        Files.deleteIfExists(pending.toPath())
    }
}
