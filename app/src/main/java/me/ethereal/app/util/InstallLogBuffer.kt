package me.ethereal.app.util

import java.util.ArrayDeque

internal const val MAX_INSTALL_LOG_CHARS = 100_000
internal const val MAX_INSTALL_LOG_LINE_CHARS = 4_096
internal const val MAX_PENDING_INSTALL_LOG_EVENTS = 128
internal const val INSTALL_LOG_BATCH_SIZE = 32

private const val CLEAR_LOG_PREFIX = "[H[J"
private const val TRUNCATED_LINE_SUFFIX = "...[truncated]"

internal data class InstallLogSnapshot(
    val text: String,
    val hasMore: Boolean,
)

internal class InstallLogBuffer(
    private val maxChars: Int = MAX_INSTALL_LOG_CHARS,
    private val maxLineChars: Int = MAX_INSTALL_LOG_LINE_CHARS,
    private val maxPendingEvents: Int = MAX_PENDING_INSTALL_LOG_EVENTS,
) {
    private val pending = ArrayDeque<String>()
    private val retained = ArrayDeque<String>()
    private var retainedChars = 0
    private var droppedEvents = 0L

    init {
        require(maxChars > 0) { "maxChars must be positive" }
        require(maxLineChars > 0) { "maxLineChars must be positive" }
        require(maxPendingEvents > 0) { "maxPendingEvents must be positive" }
    }

    @Synchronized
    fun offer(rawLine: String) {
        if (pending.size == maxPendingEvents) {
            pending.removeFirst()
            if (droppedEvents < Long.MAX_VALUE) droppedEvents++
        }
        pending.addLast(limitLine(rawLine))
    }

    @Synchronized
    fun drain(maxEvents: Int = INSTALL_LOG_BATCH_SIZE): InstallLogSnapshot? {
        require(maxEvents > 0) { "maxEvents must be positive" }
        if (pending.isEmpty() && droppedEvents == 0L) return null

        appendDroppedMarker()
        repeat(minOf(maxEvents, pending.size)) {
            applyEvent(pending.removeFirst())
        }
        return InstallLogSnapshot(snapshotText(), pending.isNotEmpty())
    }

    @Synchronized
    fun flush(): InstallLogSnapshot {
        appendDroppedMarker()
        while (pending.isNotEmpty()) applyEvent(pending.removeFirst())
        return InstallLogSnapshot(snapshotText(), false)
    }

    @Synchronized
    internal fun pendingEventCount(): Int = pending.size

    private fun limitLine(line: String): String {
        if (line.length <= maxLineChars) return line
        if (maxLineChars <= TRUNCATED_LINE_SUFFIX.length) {
            return TRUNCATED_LINE_SUFFIX.take(maxLineChars)
        }
        return line.take(maxLineChars - TRUNCATED_LINE_SUFFIX.length) + TRUNCATED_LINE_SUFFIX
    }

    private fun appendDroppedMarker() {
        if (droppedEvents == 0L) return
        applyEvent("[$droppedEvents earlier log events dropped]")
        droppedEvents = 0L
    }

    private fun applyEvent(line: String) {
        if (line.startsWith(CLEAR_LOG_PREFIX)) {
            retained.clear()
            retainedChars = 0
            appendRetained(line.substring(CLEAR_LOG_PREFIX.length))
        } else {
            appendRetained(line)
            appendRetained("\n")
        }
    }

    private fun appendRetained(value: String) {
        if (value.isEmpty()) return
        val bounded = if (value.length > maxChars) value.takeLast(maxChars) else value
        retained.addLast(bounded)
        retainedChars += bounded.length

        while (retainedChars > maxChars) {
            val excess = retainedChars - maxChars
            val first = retained.removeFirst()
            if (first.length <= excess) {
                retainedChars -= first.length
            } else {
                retained.addFirst(first.substring(excess))
                retainedChars -= excess
            }
        }
    }

    private fun snapshotText(): String {
        return StringBuilder(retainedChars).apply {
            retained.forEach { append(it) }
        }.toString()
    }
}
