package me.ethereal.app.util

import java.util.concurrent.ConcurrentHashMap

internal class PendingDownloadIds {
    private val ids = ConcurrentHashMap.newKeySet<Long>()

    fun remember(id: Long): Boolean = id > 0 && ids.add(id)

    fun consume(id: Long): Boolean = id > 0 && ids.remove(id)
}
