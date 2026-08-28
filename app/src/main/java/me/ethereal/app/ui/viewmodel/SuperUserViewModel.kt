package me.ethereal.app.ui.viewmodel

import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.graphics.drawable.Drawable
import android.os.Parcelable
import android.os.UserHandle
import android.os.UserManager
import android.util.Log
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.parcelize.Parcelize
import me.ethereal.app.Natives
import me.ethereal.app.EtherealApplication
import me.ethereal.app.etherealApp
import me.ethereal.app.util.HanziToPinyin
import me.ethereal.app.util.PkgConfig
import java.text.Collator
import java.util.Locale


class SuperUserViewModel : ViewModel() {
    companion object {
        private const val TAG = "SuperUserViewModel"
        private val appsLock = Any()
        var apps by mutableStateOf<List<AppInfo>>(emptyList())

        fun getAppIconDrawable(context: Context, packageName: String): Drawable? {
            val appList = synchronized(appsLock) { apps }
            val appDetail = appList.find { it.packageName == packageName }
            return appDetail?.packageInfo?.applicationInfo?.loadIcon(context.packageManager)
        }
    }

    @Parcelize
    data class AppInfo(
        val label: String,
        val pinyin: String,
        val packageInfo: PackageInfo,
        val config: PkgConfig.Config
    ) : Parcelable {
        val packageName: String
            get() = packageInfo.packageName
        val uid: Int
            get() = packageInfo.applicationInfo?.uid ?: 0
    }

    var search by mutableStateOf("")
    var showSystemApps by mutableStateOf(false)
    var isRefreshing by mutableStateOf(false)
        private set

    private val collator = Collator.getInstance(Locale.getDefault())

    private val sortedList by derivedStateOf {
        val comparator = compareBy<AppInfo> {
            when {
                it.config.allow != 0 -> 0
                it.config.exclude == 1 -> 1
                else -> 2
            }
        }.then(compareBy(collator, AppInfo::label))
        apps.sortedWith(comparator)
    }

    val appList by derivedStateOf {
        val query = search.lowercase()
        sortedList.filter {
            it.label.lowercase().contains(query) || it.packageName.lowercase()
                .contains(query) || it.pinyin.contains(query)
        }.filter {
            val flags = it.packageInfo.applicationInfo?.flags ?: 0
            it.uid == 2000 // Always show shell
                    || showSystemApps || flags.and(ApplicationInfo.FLAG_SYSTEM) == 0
        }.filter {
            it.packageName != etherealApp.packageName
        }
    }

    /** List apps directly through PackageManager so this screen never needs privileged IPC. */
    suspend fun fetchAppList() {
        isRefreshing = true
        try {
            withContext(Dispatchers.IO) {
                runCatching { Natives.su() }
                val packages = loadInstalledPackages()
                val uids = runCatching { Natives.suUids().toList() }.getOrDefault(emptyList())
                Log.d(TAG, "all allows: $uids")

                val configs = runCatching {
                    runCatching { Natives.su() }
                    PkgConfig.readConfigs()
                }.getOrDefault(HashMap())
                Log.d(TAG, "all configs: $configs")

                val newApps = packages.mapNotNull { pi ->
                    val appInfo = pi.applicationInfo ?: return@mapNotNull null
                    val uid = appInfo.uid
                    val actProfile = if (uids.contains(uid)) {
                        runCatching { Natives.suProfile(uid) }.getOrNull()
                    } else {
                        null
                    }
                    val config = configs.getOrDefault(
                        uid,
                        PkgConfig.Config(
                            appInfo.packageName,
                            runCatching { Natives.isUidExcluded(uid) }.getOrDefault(0),
                            0,
                            Natives.Profile(uid = uid)
                        )
                    )
                    config.allow = 0
                    if (actProfile != null) {
                        config.allow = 1
                        config.profile = actProfile
                    }
                    val label = appInfo.loadLabel(etherealApp.packageManager).toString()
                    AppInfo(
                        label = label,
                        pinyin = runCatching { HanziToPinyin.getInstance().toPinyinString(label) }.getOrNull().orEmpty(),
                        packageInfo = pi,
                        config = config
                    )
                }

                withContext(Dispatchers.Main) {
                    synchronized(appsLock) {
                        apps = newApps
                    }
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Failed to fetch app list", e)
        } finally {
            isRefreshing = false
        }
    }

    @Suppress("UNCHECKED_CAST", "DEPRECATION")
    private fun loadInstalledPackages(): List<PackageInfo> {
        val pm = etherealApp.packageManager
        val byKey = LinkedHashMap<String, PackageInfo>()
        fun addAll(list: List<PackageInfo>) {
            for (pi in list) {
                val uid = pi.applicationInfo?.uid ?: continue
                byKey["${pi.packageName}:$uid"] = pi
            }
        }
        for (userId in collectUserIds()) {
            addAll(installedPackagesAsUser(pm, 0, userId))
        }
        if (byKey.isEmpty()) {
            addAll(pm.getInstalledPackages(0))
        }
        Log.d(TAG, "installed packages: ${byKey.size}")
        return byKey.values.toList()
    }

    private fun collectUserIds(): List<Int> {
        val ids = linkedSetOf(0)
        runCatching {
            val um = etherealApp.getSystemService(Context.USER_SERVICE) as UserManager
            for (handle in um.userProfiles) {
                ids.add(userIdOf(handle))
            }
        }
        return ids.toList()
    }

    private fun userIdOf(handle: UserHandle): Int {
        return runCatching {
            UserHandle::class.java.getMethod("getIdentifier").invoke(handle) as Int
        }.getOrDefault(handle.hashCode())
    }

    @Suppress("UNCHECKED_CAST")
    private fun installedPackagesAsUser(
        pm: PackageManager,
        flags: Int,
        userId: Int
    ): List<PackageInfo> {
        return runCatching {
            val method = pm.javaClass.getDeclaredMethod(
                "getInstalledPackagesAsUser",
                Int::class.javaPrimitiveType,
                Int::class.javaPrimitiveType
            )
            method.isAccessible = true
            method.invoke(pm, flags, userId) as List<PackageInfo>
        }.getOrDefault(emptyList())
    }

    // Replaces the app's config wholesale so the snapshot state holding `apps`
    // invalidates and the UI recomposes; mutating Config fields in place would
    // leave the list showing stale grant/exclude state after a refresh.
    private fun updateAppConfig(app: AppInfo, newConfig: PkgConfig.Config) {
        synchronized(appsLock) {
            // Grant/exclude are per-UID operations; every package sharing the
            // UID must show the new state, or its stale row could overwrite it.
            apps = apps.map {
                if (it.uid == app.uid) it.copy(config = newConfig.copy(pkg = it.packageName)) else it
            }
        }
    }

    fun setRootGranted(app: AppInfo, granted: Boolean) {
        val config = app.config
        val newConfig = if (granted) {
            config.copy(
                allow = 1,
                exclude = 0,
                profile = config.profile.copy(uid = app.uid, scontext = EtherealApplication.MAGISK_SCONTEXT)
            )
        } else {
            config.copy(allow = 0, profile = config.profile.copy(uid = app.uid))
        }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                PkgConfig.changeConfig(newConfig)
                if (granted) {
                    Natives.grantSu(app.uid, 0, newConfig.profile.scontext)
                    Natives.setUidExclude(app.uid, 0)
                } else {
                    Natives.revokeSu(app.uid)
                }
            }
            updateAppConfig(app, newConfig)
        }
    }

    fun setExcluded(app: AppInfo, excluded: Boolean) {
        val config = app.config
        val newConfig = if (excluded) {
            config.copy(
                allow = 0,
                exclude = 1,
                profile = config.profile.copy(uid = app.uid, scontext = EtherealApplication.DEFAULT_SCONTEXT)
            )
        } else {
            config.copy(exclude = 0, profile = config.profile.copy(uid = app.uid))
        }
        viewModelScope.launch {
            withContext(Dispatchers.IO) {
                if (excluded) {
                    Natives.revokeSu(app.uid)
                }
                PkgConfig.changeConfig(newConfig)
                Natives.setUidExclude(app.uid, newConfig.exclude)
            }
            updateAppConfig(app, newConfig)
        }
    }
}
