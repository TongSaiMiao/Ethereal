package me.ethereal.app

import android.os.Parcelable
import android.util.Log
import androidx.annotation.Keep
import androidx.compose.runtime.Immutable
import kotlinx.parcelize.Parcelize

object Natives {
    @Volatile
    private var loaded = false
    private val loadLock = Any()

    fun ensureLoaded(): Boolean {
        if (loaded) return true
        synchronized(loadLock) {
            if (loaded) return true
            return try {
                System.loadLibrary("etherealjni")
                val token = EtherealApplication.readManagerToken()
                if (token == null || !nativeSetManagerToken(token)) {
                    Log.e(TAG, "manager authentication token is unavailable")
                    return false
                }
                loaded = true
                true
            } catch (t: Throwable) {
                Log.e(TAG, "load etherealjni failed", t)
                false
            }
        }
    }

    private external fun nativeSetManagerToken(token: ByteArray): Boolean

    @Immutable
    @Parcelize
    @Keep
    data class Profile(
        var uid: Int = 0,
        var toUid: Int = 0,
        var scontext: String = EtherealApplication.DEFAULT_SCONTEXT,
    ) : Parcelable

    private external fun nativeSu(toUid: Int, scontext: String?): Long

    fun su(toUid: Int, scontext: String?): Boolean {
        if (!ensureLoaded()) return false
        return runCatching { nativeSu(toUid, scontext) == 0L }.getOrDefault(false)
    }

    fun su(): Boolean {
        return su(0, "")
    }

    private external fun nativeReady(): Boolean

    fun ready(): Boolean {
        if (!ensureLoaded()) return false
        return runCatching { nativeReady() }.getOrDefault(false)
    }

    private external fun nativeSuPath(): String

    fun suPath(): String {
        if (!ensureLoaded()) return ""
        return runCatching { nativeSuPath() }.getOrDefault("")
    }

    private external fun nativeSuUids(): IntArray

    fun suUids(): IntArray {
        if (!ensureLoaded()) return intArrayOf()
        return runCatching { nativeSuUids() }.getOrDefault(intArrayOf())
    }

    private external fun nativeGrantSu(uid: Int, toUid: Int, scontext: String?): Long

    fun grantSu(uid: Int, toUid: Int, scontext: String?): Long {
        if (!ensureLoaded()) return -1L
        return runCatching { nativeGrantSu(uid, toUid, scontext) }.getOrDefault(-1L)
    }

    private external fun nativeRevokeSu(uid: Int): Long
    fun revokeSu(uid: Int): Long {
        if (!ensureLoaded()) return -1L
        return runCatching { nativeRevokeSu(uid) }.getOrDefault(-1L)
    }

    private external fun nativeSetUidExclude(uid: Int, exclude: Int): Int
    fun setUidExclude(uid: Int, exclude: Int): Int {
        if (!ensureLoaded()) return -1
        return runCatching { nativeSetUidExclude(uid, exclude) }.getOrDefault(-1)
    }

    private external fun nativeGetUidExclude(uid: Int): Int
    fun isUidExcluded(uid: Int): Int {
        if (!ensureLoaded()) return 0
        return runCatching { nativeGetUidExclude(uid) }.getOrDefault(0)
    }

    private external fun nativeSuProfile(uid: Int): Profile?
    fun suProfile(uid: Int): Profile {
        if (!ensureLoaded()) return Profile(uid = uid)
        return runCatching { nativeSuProfile(uid) }.getOrNull() ?: Profile(uid = uid)
    }

    fun kernelPresent(): Boolean {
        return runCatching {
            java.io.File("/sys/module/ethereal").exists()
        }.getOrDefault(false)
    }

    fun daemonPresent(): Boolean {
        return runCatching {
            java.io.File(EtherealApplication.ETHD_PATH).exists()
        }.getOrDefault(false)
    }

    private external fun nativeResetSuPath(path: String): Boolean
    fun resetSuPath(path: String): Boolean {
        if (!ensureLoaded()) return false
        return runCatching { nativeResetSuPath(path) }.getOrDefault(false)
    }

    private external fun nativeControlFeature(featureName: String, state: Int): Long
    fun controlFeature(featureName: String, enable: Boolean): Long {
        if (!ensureLoaded()) return -1L
        return runCatching { nativeControlFeature(featureName, if (enable) 1 else 0) }.getOrDefault(-1L)
    }
}
