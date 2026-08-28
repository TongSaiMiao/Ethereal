package me.ethereal.app.util

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class InstallLogBufferTest {
    @Test
    fun `single lines and retained content stay bounded`() {
        val logs = InstallLogBuffer(maxChars = 24, maxLineChars = 16, maxPendingEvents = 4)

        logs.offer("x".repeat(100))
        logs.offer("y".repeat(100))
        val snapshot = logs.flush()

        assertTrue(snapshot.text.length <= 24)
        assertTrue(snapshot.text.contains("truncated"))
        assertFalse(snapshot.hasMore)
    }

    @Test
    fun `pending queue drops oldest events at its fixed capacity`() {
        val logs = InstallLogBuffer(maxChars = 1_000, maxLineChars = 100, maxPendingEvents = 2)

        logs.offer("first")
        logs.offer("second")
        logs.offer("third")

        assertEquals(2, logs.pendingEventCount())
        val snapshot = logs.flush()
        assertTrue(snapshot.text.contains("1 earlier log events dropped"))
        assertFalse(snapshot.text.contains("first\n"))
        assertTrue(snapshot.text.contains("second\n"))
        assertTrue(snapshot.text.contains("third\n"))
    }

    @Test
    fun `drain processes bounded batches through one consumer`() {
        val logs = InstallLogBuffer(maxChars = 1_000, maxLineChars = 100, maxPendingEvents = 8)
        repeat(5) { logs.offer("line-$it") }

        val first = assertNotNull(logs.drain(2))
        assertTrue(first.hasMore)
        assertEquals(3, logs.pendingEventCount())

        val second = assertNotNull(logs.drain(8))
        assertFalse(second.hasMore)
        assertEquals(0, logs.pendingEventCount())
        repeat(5) { assertTrue(second.text.contains("line-$it")) }
    }

    @Test
    fun `clear event replaces previously retained log`() {
        val logs = InstallLogBuffer(maxChars = 100, maxLineChars = 100, maxPendingEvents = 4)
        logs.offer("old")
        logs.flush()

        logs.offer("[H[Jreplacement")

        assertEquals("replacement", logs.flush().text)
    }
}
