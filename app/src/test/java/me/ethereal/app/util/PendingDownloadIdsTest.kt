package me.ethereal.app.util

import java.io.File
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class PendingDownloadIdsTest {
    @Test
    fun `only ids started by this process are consumed`() {
        val pending = PendingDownloadIds()

        assertTrue(pending.remember(42))
        assertFalse(pending.consume(41))
        assertTrue(pending.consume(42))
        assertFalse(pending.consume(42))
    }

    @Test
    fun `invalid download ids are never accepted`() {
        val pending = PendingDownloadIds()

        assertFalse(pending.remember(-1))
        assertFalse(pending.remember(0))
        assertFalse(pending.consume(-1))
        assertFalse(pending.consume(0))
    }

    @Test
    fun `download completion receiver authenticates sender and id`() {
        val source = sequenceOf(
            File("src/main/java/me/ethereal/app/util/Downloader.kt"),
            File("app/src/main/java/me/ethereal/app/util/Downloader.kt"),
        ).first { it.isFile }.readText()
        val receiver = source
            .substringAfter("val receiver = object : BroadcastReceiver()")
            .substringBefore("onDispose")

        assertTrue(receiver.contains("pendingDownloadIds.consume(id)"))
        assertTrue(receiver.contains("SEND_DOWNLOAD_COMPLETED_INTENTS"))
        assertTrue(receiver.contains("ContextCompat.RECEIVER_EXPORTED"))
    }
}
