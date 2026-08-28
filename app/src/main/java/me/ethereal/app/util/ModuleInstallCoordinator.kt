package me.ethereal.app.util

import java.io.File
import java.nio.file.Files
import java.nio.file.LinkOption
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

internal const val MODULE_INSTALL_ARCHIVE_PREFIX = "ethereal-module-archive-"
internal const val MODULE_INSTALL_ARCHIVE_SUFFIX = ".zip"
internal val MODULE_INSTALL_STALE_AGE_MILLIS: Long = TimeUnit.HOURS.toMillis(6)

private val moduleArchiveName = Regex(
    "^${Regex.escape(MODULE_INSTALL_ARCHIVE_PREFIX)}[0-9a-f]{32}" +
        "${Regex.escape(MODULE_INSTALL_ARCHIVE_SUFFIX)}$"
)
private val modulePendingName = Regex(
    "^\\.${Regex.escape(MODULE_INSTALL_ARCHIVE_PREFIX)}[0-9a-f]{32}" +
        "${Regex.escape(MODULE_INSTALL_ARCHIVE_SUFFIX)}\\.[A-Za-z0-9_-]+\\.pending$"
)

internal fun isOwnedModuleInstallFile(name: String): Boolean {
    return moduleArchiveName.matches(name) || modulePendingName.matches(name)
}

internal fun deleteStaleModuleInstallFiles(
    cacheDir: File,
    nowMillis: Long = System.currentTimeMillis(),
    staleAfterMillis: Long = MODULE_INSTALL_STALE_AGE_MILLIS,
): Int {
    require(staleAfterMillis > 0) { "staleAfterMillis must be positive" }
    if (!cacheDir.isDirectory) return 0

    var deleted = 0
    cacheDir.listFiles().orEmpty().forEach { candidate ->
        if (!isOwnedModuleInstallFile(candidate.name)) return@forEach
        val path = candidate.toPath()
        if (!Files.isRegularFile(path, LinkOption.NOFOLLOW_LINKS)) return@forEach
        val modified = runCatching {
            Files.getLastModifiedTime(path, LinkOption.NOFOLLOW_LINKS).toMillis()
        }.getOrNull() ?: return@forEach
        if (modified > nowMillis) return@forEach
        val age = runCatching { Math.subtractExact(nowMillis, modified) }
            .getOrDefault(Long.MAX_VALUE)
        if (age < staleAfterMillis) return@forEach
        if (Files.deleteIfExists(path)) deleted++
    }
    return deleted
}

internal class ModuleInstallCoordinator(
    private val idFactory: () -> String = {
        UUID.randomUUID().toString().replace("-", "")
    },
) {
    private val lock = ReentrantLock()

    fun cleanup(cacheDir: File): Int = lock.withLock {
        deleteStaleModuleInstallFiles(cacheDir)
    }

    fun <T> withArchive(cacheDir: File, block: (File) -> T): T = lock.withLock {
        check(cacheDir.isDirectory || cacheDir.mkdirs()) { "create ${cacheDir.absolutePath}" }
        deleteStaleModuleInstallFiles(cacheDir)

        val archive = generateSequence {
            File(
                cacheDir,
                MODULE_INSTALL_ARCHIVE_PREFIX + idFactory() + MODULE_INSTALL_ARCHIVE_SUFFIX,
            )
        }.take(32).firstOrNull { candidate ->
            isOwnedModuleInstallFile(candidate.name) &&
                !Files.exists(candidate.toPath(), LinkOption.NOFOLLOW_LINKS)
        } ?: error("Unable to allocate a unique module archive path")

        try {
            block(archive)
        } finally {
            Files.deleteIfExists(archive.toPath())
        }
    }
}

private val processModuleInstallCoordinator = ModuleInstallCoordinator()

internal fun cleanupStaleModuleInstallFiles(cacheDir: File): Int {
    return processModuleInstallCoordinator.cleanup(cacheDir)
}

internal fun <T> withModuleInstallArchive(cacheDir: File, block: (File) -> T): T {
    return processModuleInstallCoordinator.withArchive(cacheDir, block)
}
