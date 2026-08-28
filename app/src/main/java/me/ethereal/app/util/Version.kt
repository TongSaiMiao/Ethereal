package me.ethereal.app.util

import android.util.Log
import androidx.core.content.pm.PackageInfoCompat
import me.ethereal.app.EtherealApplication
import me.ethereal.app.etherealApp

/**
 * version string is like 0.9.0 or 0.9.0-dev
 * version uint is hex number like: 0x000900
 */
object Version {

    private fun installedEthdVersionString(): String {
        val resultShell = runCatching {
            rootShellForResult("${EtherealApplication.ETHD_PATH} -V")
        }.getOrNull()
        installedEthdVersionString = if (resultShell?.isSuccess == true) {
            val result = resultShell.out.toString()
            Log.i("Ethereal", "[installedEthdVersionString@Version] resultFromShell: $result")
            Regex("\\d+").find(result)?.value ?: "0"
        } else {
            "0"
        }
        return installedEthdVersionString
    }

    fun installedEthdVersionUInt(): Int {
        installedEthdVersionInt = installedEthdVersionString().toInt()
        return installedEthdVersionInt
    }


    fun getManagerVersion(): Pair<String, Long> {
        val packageInfo = etherealApp.packageManager.getPackageInfo(etherealApp.packageName, 0)!!
        val versionCode = PackageInfoCompat.getLongVersionCode(packageInfo)
        return Pair(packageInfo.versionName!!, versionCode)
    }

    var installedEthdVersionInt: Int = 0
    var installedEthdVersionString: String = "0"
}
