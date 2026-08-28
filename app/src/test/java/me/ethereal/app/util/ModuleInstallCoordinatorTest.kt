package me.ethereal.app.util

import java.nio.file.Files
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

class ModuleInstallCoordinatorTest {
    @Test
    fun `cleanup removes only exact old regular files`() {
        val dir = Files.createTempDirectory("ethereal-install-cleanup").toFile()
        val now = 1_000_000L
        val staleAfter = 10_000L
        val id = "a".repeat(32)
        try {
            val staleArchive = dir.resolve("$MODULE_INSTALL_ARCHIVE_PREFIX$id.zip")
                .apply { writeText("old") }
            val stalePending = dir.resolve(".$MODULE_INSTALL_ARCHIVE_PREFIX$id.zip.123.pending")
                .apply { writeText("old") }
            val freshArchive = dir.resolve("$MODULE_INSTALL_ARCHIVE_PREFIX${"b".repeat(32)}.zip")
                .apply { writeText("fresh") }
            val lookalike = dir.resolve("$MODULE_INSTALL_ARCHIVE_PREFIX$id.zip.backup")
                .apply { writeText("keep") }
            val ownedDirectory = dir.resolve(
                "$MODULE_INSTALL_ARCHIVE_PREFIX${"c".repeat(32)}.zip"
            ).apply { mkdir() }

            staleArchive.setLastModified(now - staleAfter)
            stalePending.setLastModified(now - staleAfter - 1)
            freshArchive.setLastModified(now - staleAfter + 1)
            lookalike.setLastModified(now - staleAfter - 1)
            ownedDirectory.setLastModified(now - staleAfter - 1)

            assertEquals(2, deleteStaleModuleInstallFiles(dir, now, staleAfter))
            assertFalse(staleArchive.exists())
            assertFalse(stalePending.exists())
            assertTrue(freshArchive.exists())
            assertTrue(lookalike.exists())
            assertTrue(ownedDirectory.isDirectory)
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `coordinator serializes installs and gives each one a unique file`() {
        val ids = ArrayDeque(listOf("a".repeat(32), "b".repeat(32)))
        val coordinator = ModuleInstallCoordinator { ids.removeFirst() }
        val dir = Files.createTempDirectory("ethereal-install-lock").toFile()
        val firstEntered = CountDownLatch(1)
        val releaseFirst = CountDownLatch(1)
        val secondAttempting = CountDownLatch(1)
        val secondEntered = CountDownLatch(1)
        val executor = Executors.newFixedThreadPool(2)
        try {
            val first = executor.submit<String> {
                coordinator.withArchive(dir) { archive ->
                    archive.writeText("first")
                    firstEntered.countDown()
                    check(releaseFirst.await(5, TimeUnit.SECONDS))
                    archive.name
                }
            }
            assertTrue(firstEntered.await(5, TimeUnit.SECONDS))

            val second = executor.submit<String> {
                secondAttempting.countDown()
                coordinator.withArchive(dir) { archive ->
                    secondEntered.countDown()
                    archive.writeText("second")
                    archive.name
                }
            }
            assertTrue(secondAttempting.await(5, TimeUnit.SECONDS))
            assertFalse(secondEntered.await(200, TimeUnit.MILLISECONDS))

            releaseFirst.countDown()
            val firstName = first.get(5, TimeUnit.SECONDS)
            val secondName = second.get(5, TimeUnit.SECONDS)
            assertNotEquals(firstName, secondName)
            assertTrue(isOwnedModuleInstallFile(firstName))
            assertTrue(isOwnedModuleInstallFile(secondName))
            assertFalse(dir.resolve(firstName).exists())
            assertFalse(dir.resolve(secondName).exists())
        } finally {
            releaseFirst.countDown()
            executor.shutdownNow()
            dir.deleteRecursively()
        }
    }
}
