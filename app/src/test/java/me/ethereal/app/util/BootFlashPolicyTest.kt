package me.ethereal.app.util

import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse

class BootFlashPolicyTest {
    @Test
    fun `OEM releases use inferred KMI generation`() {
        assertEquals("init_boot", defaultPartitionFor("5.15.153-oem", 33))
        assertEquals("init_boot", defaultPartitionFor("5.15.153-oem", 34))
        assertEquals("init_boot", defaultPartitionFor("5.10.218-oem", 33))
        assertEquals("boot", defaultPartitionFor("5.10.218-oem", 31))
    }

    @Test
    fun `explicit generation remains authoritative when KMI is unsupported`() {
        assertEquals("init_boot", defaultPartitionFor("6.6.1-android14-oem", 34))
        assertEquals("boot", defaultPartitionFor("5.10.1-android12-oem", 33))
        assertEquals("boot", defaultPartitionFor("unknown", 34))
    }

    @Test
    fun `legacy publication replaces only after staging completes`() {
        val dir = Files.createTempDirectory("ethereal-publish-test").toFile()
        try {
            val source = dir.resolve("source.img").apply { writeText("new image") }
            val downloads = dir.resolve("downloads").apply { mkdirs() }
            val output = downloads.resolve("Ethereal-boot.img").apply { writeText("old image") }

            assertFailsWith<IllegalStateException> {
                publishLegacyPatchedImage(source, downloads, output.name, "forced") { _, _ ->
                    error("rename failed")
                }
            }

            assertEquals("old image", output.readText())
            assertFalse(downloads.resolve(".${output.name}.forced.pending").exists())

            val published = publishLegacyPatchedImage(source, downloads, output.name, "success")
            assertEquals("new image", published.readText())
            assertFalse(downloads.resolve(".${output.name}.success.pending").exists())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `MediaStore publication requires a visible row update`() {
        requireMediaStorePublished(1, "Ethereal-boot.img")
        assertFailsWith<IllegalStateException> {
            requireMediaStorePublished(0, "Ethereal-boot.img")
        }
    }
}
