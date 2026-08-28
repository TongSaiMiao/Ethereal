package me.ethereal.app.util

import java.io.ByteArrayInputStream
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse

class BoundedFileStageTest {
    @Test
    fun `exact size limit publishes the complete file`() {
        val dir = Files.createTempDirectory("ethereal-stage-boundary").toFile()
        try {
            val bytes = byteArrayOf(1, 2, 3, 4)
            val output = dir.resolve("module.zip")

            stageBoundedFile(ByteArrayInputStream(bytes), output, bytes.size.toLong())

            assertContentEquals(bytes, output.readBytes())
            assertFalse(dir.hasPendingFiles())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `oversized input leaves no consumable or pending file`() {
        val dir = Files.createTempDirectory("ethereal-stage-limit").toFile()
        try {
            val output = dir.resolve("module.zip").apply { writeText("stale") }

            assertFailsWith<IllegalStateException> {
                stageBoundedFile(ByteArrayInputStream(ByteArray(5)), output, 4)
            }

            assertFalse(output.exists())
            assertFalse(dir.hasPendingFiles())
        } finally {
            dir.deleteRecursively()
        }
    }

    @Test
    fun `publication failure removes staging and output files`() {
        val dir = Files.createTempDirectory("ethereal-stage-publish").toFile()
        try {
            val output = dir.resolve("module.zip")

            assertFailsWith<IllegalStateException> {
                stageBoundedFile(ByteArrayInputStream(byteArrayOf(1)), output, 1) { _, _ ->
                    error("publish failed")
                }
            }

            assertFalse(output.exists())
            assertFalse(dir.hasPendingFiles())
        } finally {
            dir.deleteRecursively()
        }
    }

    private fun java.io.File.hasPendingFiles(): Boolean {
        return listFiles().orEmpty().any { it.name.endsWith(".pending") }
    }
}
