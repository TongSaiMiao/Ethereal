package me.ethereal.app.util

import android.util.Log
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.res.stringResource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import me.ethereal.app.R

private fun querySELinuxStatus(): Pair<Boolean, String> {
    runCatching {
        val enforce = java.io.File("/sys/fs/selinux/enforce").readText().trim()
        val text = when (enforce) {
            "1" -> "Enforcing"
            "0" -> "Permissive"
            else -> enforce
        }
        if (text.isNotEmpty()) return true to text
    }
    return false to ""
}

@Composable
fun getSELinuxStatus(): String {
    // The getenforce shell round trip runs once on the IO dispatcher, not on
    // every recomposition of the caller.
    val status by produceState<Pair<Boolean, String>?>(null) {
        value = withContext(Dispatchers.IO) { querySELinuxStatus() }
    }
    val (success, output) = status ?: return stringResource(R.string.home_selinux_status_unknown)
    if (success) {
        return when (output) {
            "Enforcing" -> stringResource(R.string.home_selinux_status_enforcing)
            "Permissive" -> stringResource(R.string.home_selinux_status_permissive)
            "Disabled" -> stringResource(R.string.home_selinux_status_disabled)
            else -> stringResource(R.string.home_selinux_status_unknown)
        }
    }

    return if (output.endsWith("Permission denied")) {
        stringResource(R.string.home_selinux_status_enforcing)
    } else {
        stringResource(R.string.home_selinux_status_unknown)
    }
}

private fun getSystemProperty(key: String): Boolean {
    try {
        val c = Class.forName("android.os.SystemProperties")
        val get = c.getMethod(
            "getBoolean",
            String::class.java,
            Boolean::class.javaPrimitiveType
        )
        return get.invoke(c, key, false) as Boolean
    } catch (e: Exception) {
        Log.e("Ethereal", "[DeviceUtils] Failed to get system property: ", e)
    }
    return false
}

// Check to see if device supports A/B (seamless) system updates
fun isABDevice(): Boolean {
    return getSystemProperty("ro.build.ab_update")
}