package me.ethereal.app.ui.screen

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.util.Log
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.appcompat.app.AppCompatDelegate
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.ui.graphics.Color

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BugReport
import androidx.compose.material.icons.filled.ColorLens
import androidx.compose.material.icons.filled.Commit
import androidx.compose.material.icons.filled.DarkMode
import androidx.compose.material.icons.filled.DeveloperMode
import androidx.compose.material.icons.filled.Engineering
import androidx.compose.material.icons.automirrored.filled.FeaturedPlayList
import androidx.compose.material.icons.filled.FormatColorFill
import androidx.compose.material.icons.filled.InvertColors
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Translate
import androidx.compose.material.icons.filled.DeleteForever
import androidx.compose.material.icons.filled.Update
import androidx.compose.material3.AlertDialogDefaults
import androidx.compose.material3.BasicAlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.livedata.observeAsState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringArrayResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.content.FileProvider
import androidx.core.content.edit
import androidx.core.os.LocaleListCompat
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import me.ethereal.app.EtherealApplication
import me.ethereal.app.BuildConfig
import me.ethereal.app.Natives
import me.ethereal.app.R
import me.ethereal.app.ui.component.rememberLoadingDialog
import me.ethereal.app.ui.component.SettingsGroup
import me.ethereal.app.ui.component.settings.SegmentedColumn
import me.ethereal.app.ui.component.settings.SettingsJumpPageWidget
import me.ethereal.app.ui.component.settings.SettingsSwitchWidget
import me.ethereal.app.ui.theme.refreshTheme
import me.ethereal.app.util.becomeRoot
import me.ethereal.app.util.getBugreportFile
import me.ethereal.app.util.getKernelVersionCode
import me.ethereal.app.util.isGkiKernel
import me.ethereal.app.util.isGlobalNamespaceEnabled
import me.ethereal.app.util.outputStream
import me.ethereal.app.util.rootShellForResult
import me.ethereal.app.util.setGlobalNamespaceEnabled
import me.ethereal.app.util.ui.EtherealDialogBlurBehindUtils
import me.ethereal.app.util.ui.LocalSnackbarHost
import me.ethereal.app.util.ui.NavigationBarsSpacer
import java.io.File
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

@Destination<RootGraph>
@Composable
@OptIn(ExperimentalMaterial3Api::class)
fun SettingScreen() {
    val kernelState by EtherealApplication.kernelStateLiveData.observeAsState(EtherealApplication.State.UNKNOWN_STATE)
    val state by EtherealApplication.serviceStateLiveData.observeAsState(EtherealApplication.State.UNKNOWN_STATE)
    val managerAccessState by EtherealApplication.managerAccessStateLiveData.observeAsState(
        EtherealApplication.ManagerAccessState.UNKNOWN
    )
    val kernelReady = kernelState == EtherealApplication.State.KERNEL_WORKING
    val managerAccessReady =
        managerAccessState == EtherealApplication.ManagerAccessState.AUTHENTICATED
    val serviceReady =
        (state == EtherealApplication.State.SERVICE_INSTALLING || state == EtherealApplication.State.SERVICE_INSTALLED || state == EtherealApplication.State.SERVICE_NEED_UPDATE)
    var isGlobalNamespaceEnabled by rememberSaveable {
        mutableStateOf(false)
    }
    var namespaceLoaded by remember { mutableStateOf(false) }
    // The check shells out as root; run it once off the main thread instead of
    // synchronously in composition on every recomposition. The switch stays
    // disabled until the real value lands so a fast tap can't act on the
    // placeholder and get overwritten by the late result.
    LaunchedEffect(kernelReady && serviceReady) {
        if (kernelReady && serviceReady) {
            isGlobalNamespaceEnabled = withContext(Dispatchers.IO) { isGlobalNamespaceEnabled() }
            namespaceLoaded = true
        }
    }

    val snackBarHost = LocalSnackbarHost.current

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.settings)) },
                colors = TopAppBarDefaults.topAppBarColors(
                    containerColor = Color.Transparent
                )
            )
        },
        containerColor = Color.Transparent,
        snackbarHost = { SnackbarHost(snackBarHost) }
    ) { paddingValues ->

        val loadingDialog = rememberLoadingDialog()

        val showLanguageDialog = rememberSaveable { mutableStateOf(false) }
        LanguageDialog(showLanguageDialog)

        val showResetSuPathDialog = remember { mutableStateOf(false) }
        if (showResetSuPathDialog.value) {
            ResetSUPathDialog(showResetSuPathDialog)
        }

        val showThemeChooseDialog = remember { mutableStateOf(false) }
        if (showThemeChooseDialog.value) {
            ThemeChooseDialog(showThemeChooseDialog)
        }

        var showLogBottomSheet by remember { mutableStateOf(false) }
        val saveLog = stringResource(R.string.save_log)

        val scope = rememberCoroutineScope()
        val context = LocalContext.current
        val logSavedMessage = stringResource(R.string.log_saved)
        val exportBugreportLauncher = rememberLauncherForActivityResult(
            ActivityResultContracts.CreateDocument("application/gzip")
        ) { uri: Uri? ->
            if (uri != null) {
                scope.launch {
                    loadingDialog.show()
                    val saved = try {
                        withContext(Dispatchers.IO) {
                            uri.outputStream().use { output ->
                                getBugreportFile(context).inputStream().use {
                                    it.copyTo(output)
                                }
                            }
                        }
                        true
                    } catch (t: Throwable) {
                        Log.e("BugreportExport", "save bugreport", t)
                        false
                    } finally {
                        loadingDialog.hide()
                    }
                    if (saved) snackBarHost.showSnackbar(message = logSavedMessage)
                }
            }
        }

        Column(
            modifier = Modifier
                .padding(paddingValues)
                .fillMaxWidth()
                .verticalScroll(rememberScrollState()),
        ) {

            val context = LocalContext.current
            val scope = rememberCoroutineScope()
            val prefs = EtherealApplication.sharedPreferences

            val kernelVersion = remember { getKernelVersionCode() }
            val kernelSupported = (kernelVersion ?: 0) >= 419
            val isGki = remember { isGkiKernel() }
            var sucompatEnabled by rememberSaveable {
                mutableStateOf(prefs.getBoolean("sucompat_enabled", false))
            }
            var selinuxHideEnabled by rememberSaveable {
                mutableStateOf(prefs.getBoolean("selinux_hide_enabled", false))
            }
            val showSelinuxHideWarning = remember { mutableStateOf(false) }
            fun applySelinuxHide(enabled: Boolean) {
                scope.launch(Dispatchers.IO) {
                    val command = if (enabled) {
                        "touch ${EtherealApplication.SELINUX_HIDE_FILE}"
                    } else {
                        "rm -f ${EtherealApplication.SELINUX_HIDE_FILE}"
                    }
                    val result = runCatching { rootShellForResult(command) }.getOrNull()
                    Log.d("SelinuxHideToggle", "$command result: ${result?.code}")
                    if (result?.isSuccess == true) {
                        prefs.edit { putBoolean("selinux_hide_enabled", enabled) }
                        withContext(Dispatchers.Main) {
                            selinuxHideEnabled = enabled
                        }
                    }
                }
            }
            var enableWebDebugging by rememberSaveable {
                mutableStateOf(prefs.getBoolean("enable_web_debugging", false))
            }

            val configEnabled = kernelReady && managerAccessReady && serviceReady
            SettingsGroup(title = stringResource(R.string.settings_group_configuration)) {
                SettingsSwitchWidget(
                    icon = Icons.Filled.Engineering,
                    title = stringResource(id = R.string.settings_global_namespace_mode),
                    description = stringResource(id = R.string.settings_global_namespace_mode_summary),
                    checked = isGlobalNamespaceEnabled,
                    enabled = configEnabled && namespaceLoaded,
                    onCheckedChange = {
                        scope.launch(Dispatchers.IO) {
                            setGlobalNamespaceEnabled(if (it) "1" else "0")
                            withContext(Dispatchers.Main) {
                                isGlobalNamespaceEnabled = it
                            }
                        }
                    }
                )
                SettingsSwitchWidget(
                    icon = Icons.AutoMirrored.Filled.FeaturedPlayList,
                    title = stringResource(id = R.string.settings_sucompat),
                    description = stringResource(id = R.string.settings_sucompat_summary),
                    checked = sucompatEnabled,
                    enabled = configEnabled,
                    onCheckedChange = { enabled ->
                        scope.launch(Dispatchers.IO) {
                            val result = if (enabled) {
                                runCatching { rootShellForResult("touch ${EtherealApplication.SUCOMPAT_FILE}") }
                                Natives.controlFeature("sucompat_extra", true)
                            } else {
                                runCatching { rootShellForResult("rm -f ${EtherealApplication.SUCOMPAT_FILE}") }
                                Natives.controlFeature("sucompat_extra", false)
                            }
                            Log.d(
                                "SucompatToggle",
                                "sucompat_extra ${if (enabled) "enable" else "disable"} result: $result"
                            )
                            if (result == 0L) {
                                prefs.edit { putBoolean("sucompat_enabled", enabled) }
                                withContext(Dispatchers.Main) {
                                    sucompatEnabled = enabled
                                }
                            }
                        }
                    }
                )
                SettingsSwitchWidget(
                    icon = Icons.Filled.Security,
                    title = stringResource(id = R.string.settings_selinux_hide),
                    description = stringResource(id = R.string.settings_selinux_hide_summary),
                    checked = selinuxHideEnabled,
                    enabled = configEnabled && kernelSupported,
                    onCheckedChange = { enabled ->
                        if (enabled) {
                            val below510 = (kernelVersion ?: 0) < 510
                            if (below510 || !isGki) {
                                showSelinuxHideWarning.value = true
                            } else {
                                applySelinuxHide(true)
                            }
                        } else {
                            applySelinuxHide(false)
                        }
                    }
                )
                if (BuildConfig.DEBUG) {
                    SettingsSwitchWidget(
                        icon = Icons.Filled.DeveloperMode,
                        title = stringResource(id = R.string.enable_web_debugging),
                        description = stringResource(id = R.string.enable_web_debugging_summary),
                        checked = enableWebDebugging,
                        enabled = true,
                        onCheckedChange = {
                            EtherealApplication.sharedPreferences.edit {
                                putBoolean("enable_web_debugging", it)
                            }
                            enableWebDebugging = it
                        }
                    )
                }
            }

            if (showSelinuxHideWarning.value) {
                SelinuxHideWarningDialog(
                    showDialog = showSelinuxHideWarning,
                    kernelVersion = kernelVersion,
                    isGki = isGki,
                    onConfirm = { applySelinuxHide(true) },
                )
            }

            var checkUpdate by rememberSaveable {
                mutableStateOf(prefs.getBoolean("check_update", true))
            }
            var nightFollowSystem by rememberSaveable {
                mutableStateOf(prefs.getBoolean("night_mode_follow_sys", true))
            }
            var nightThemeEnabled by rememberSaveable {
                mutableStateOf(prefs.getBoolean("night_mode_enabled", false))
            }
            val isDynamicColorSupport = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
            var useSystemDynamicColor by rememberSaveable {
                mutableStateOf(prefs.getBoolean("use_system_color_theme", true))
            }

            SettingsGroup(title = stringResource(R.string.settings_group_appearance)) {
                SettingsSwitchWidget(
                    icon = Icons.Filled.Update,
                    title = stringResource(id = R.string.settings_check_update),
                    description = stringResource(id = R.string.settings_check_update_summary),
                    checked = checkUpdate,
                    onCheckedChange = {
                        prefs.edit { putBoolean("check_update", it) }
                        checkUpdate = it
                    }
                )
                SettingsSwitchWidget(
                    icon = Icons.Filled.InvertColors,
                    title = stringResource(id = R.string.settings_night_mode_follow_sys),
                    description = stringResource(id = R.string.settings_night_mode_follow_sys_summary),
                    checked = nightFollowSystem,
                    onCheckedChange = {
                        prefs.edit { putBoolean("night_mode_follow_sys", it) }
                        nightFollowSystem = it
                        refreshTheme.value = true
                    }
                )
                if (!nightFollowSystem) {
                    SettingsSwitchWidget(
                        icon = Icons.Filled.DarkMode,
                        title = stringResource(id = R.string.settings_night_theme_enabled),
                        checked = nightThemeEnabled,
                        onCheckedChange = {
                            prefs.edit { putBoolean("night_mode_enabled", it) }
                            nightThemeEnabled = it
                            refreshTheme.value = true
                        }
                    )
                }
                if (isDynamicColorSupport) {
                    SettingsSwitchWidget(
                        icon = Icons.Filled.ColorLens,
                        title = stringResource(id = R.string.settings_use_system_color_theme),
                        description = stringResource(id = R.string.settings_use_system_color_theme_summary),
                        checked = useSystemDynamicColor,
                        onCheckedChange = {
                            prefs.edit { putBoolean("use_system_color_theme", it) }
                            useSystemDynamicColor = it
                            refreshTheme.value = true
                        }
                    )
                }
                if (!isDynamicColorSupport || !useSystemDynamicColor) {
                    SettingsJumpPageWidget(
                        icon = Icons.Filled.FormatColorFill,
                        title = stringResource(id = R.string.settings_custom_color_theme),
                        description = stringResource(
                            colorNameToString(prefs.getString("custom_color", "blue").toString())
                        ),
                        onClick = { showThemeChooseDialog.value = true }
                    )
                }
            }

            val showUninstallDialog = remember { mutableStateOf(false) }
            if (showUninstallDialog.value) {
                UninstallDialog(showDialog = showUninstallDialog)
            }

            SegmentedColumn(title = stringResource(R.string.settings_group_other)) {
                item(visible = serviceReady) {
                    SettingsJumpPageWidget(
                        icon = Icons.Filled.DeleteForever,
                        title = stringResource(id = R.string.settings_uninstall),
                        description = stringResource(id = R.string.settings_uninstall_summary),
                        onClick = { showUninstallDialog.value = true }
                    )
                }
                item(visible = managerAccessReady) {
                    SettingsJumpPageWidget(
                        icon = Icons.Filled.Commit,
                        title = stringResource(id = R.string.setting_reset_su_path),
                        onClick = { showResetSuPathDialog.value = true }
                    )
                }
                item {
                    SettingsJumpPageWidget(
                        icon = Icons.Filled.Translate,
                        title = stringResource(id = R.string.settings_app_language),
                        description = AppCompatDelegate.getApplicationLocales()[0]?.displayLanguage?.replaceFirstChar {
                            if (it.isLowerCase()) it.titlecase(Locale.getDefault()) else it.toString()
                        } ?: stringResource(id = R.string.system_default),
                        onClick = { showLanguageDialog.value = true }
                    )
                }
                item {
                    SettingsJumpPageWidget(
                        icon = Icons.Filled.BugReport,
                        title = stringResource(id = R.string.send_log),
                        onClick = { showLogBottomSheet = true }
                    )
                }
            }
            if (showLogBottomSheet) {
                ModalBottomSheet(
                    onDismissRequest = { showLogBottomSheet = false },
                    contentWindowInsets = { WindowInsets(0, 0, 0, 0) },
                    content = {
                        Row(
                            modifier = Modifier
                                .padding(10.dp)
                                .align(Alignment.CenterHorizontally)

                        ) {
                            Box {
                                Column(
                                    modifier = Modifier
                                        .padding(16.dp)
                                        .clickable {
                                            scope.launch {
                                                val formatter =
                                                    DateTimeFormatter.ofPattern("yyyy-MM-dd_HH_mm")
                                                val current = LocalDateTime.now().format(formatter)
                                                exportBugreportLauncher.launch("Ethereal_bugreport_${current}.tar.gz")
                                                showLogBottomSheet = false
                                            }
                                        }
                                ) {
                                    Icon(
                                        Icons.Filled.Save,
                                        contentDescription = null,
                                        modifier = Modifier.align(Alignment.CenterHorizontally)
                                    )
                                    Text(
                                        text = stringResource(id = R.string.save_log),
                                        modifier = Modifier.padding(top = 16.dp),
                                        textAlign = TextAlign.Center

                                    )
                                }

                            }
                            Box {
                                Column(
                                    modifier = Modifier
                                        .padding(16.dp)
                                        .clickable {
                                            scope.launch {
                                                val bugreport = loadingDialog.withLoading {
                                                    withContext(Dispatchers.IO) {
                                                        getBugreportFile(context)
                                                    }
                                                }

                                                val uri: Uri = FileProvider.getUriForFile(
                                                    context,
                                                    "${BuildConfig.APPLICATION_ID}.fileprovider",
                                                    bugreport
                                                )

                                                val shareIntent = Intent(Intent.ACTION_SEND).apply {
                                                    putExtra(Intent.EXTRA_STREAM, uri)
                                                    setDataAndType(uri, "application/gzip")
                                                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                                                }

                                                context.startActivity(
                                                    Intent.createChooser(
                                                        shareIntent,
                                                        saveLog
                                                    )
                                                )
                                                showLogBottomSheet = false
                                            }
                                        }) {
                                    Icon(
                                        Icons.Filled.Share,
                                        contentDescription = null,
                                        modifier = Modifier.align(Alignment.CenterHorizontally)
                                    )
                                    Text(
                                        text = stringResource(id = R.string.send_log),
                                        modifier = Modifier.padding(top = 16.dp),
                                        textAlign = TextAlign.Center

                                    )
                                }

                            }
                        }
                        NavigationBarsSpacer()
                    })
            }


        }

    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun UninstallDialog(showDialog: MutableState<Boolean>) {
    val scope = rememberCoroutineScope()
    BasicAlertDialog(
        onDismissRequest = { showDialog.value = false },
        properties = DialogProperties(
            decorFitsSystemWindows = true,
            usePlatformDefaultWidth = false,
        )
    ) {
        Surface(
            modifier = Modifier
                .width(320.dp)
                .wrapContentHeight(),
            shape = RoundedCornerShape(20.dp),
            tonalElevation = AlertDialogDefaults.TonalElevation,
            color = AlertDialogDefaults.containerColor,
        ) {
            Column(modifier = Modifier.padding(PaddingValues(all = 24.dp))) {
                Box(
                    Modifier
                        .padding(PaddingValues(bottom = 16.dp))
                        .align(Alignment.CenterHorizontally)
                ) {
                    Text(
                        text = stringResource(id = R.string.home_dialog_uninstall_title),
                        style = MaterialTheme.typography.headlineSmall
                    )
                }
                Text(
                    text = stringResource(id = R.string.home_dialog_uninstall_message),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(PaddingValues(bottom = 24.dp))
                )
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End
                ) {
                    TextButton(onClick = { showDialog.value = false }) {
                        Text(text = stringResource(id = android.R.string.cancel))
                    }
                    TextButton(onClick = {
                        showDialog.value = false
                        scope.launch(Dispatchers.IO) {
                            EtherealApplication.uninstallEthereal()
                        }
                    }) {
                        Text(text = stringResource(id = R.string.home_dialog_uninstall_service_only))
                    }
                }
            }
            val dialogWindowProvider = LocalView.current.parent as? DialogWindowProvider
            dialogWindowProvider?.window?.let { EtherealDialogBlurBehindUtils.setupWindowBlurListener(it) }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThemeChooseDialog(showDialog: MutableState<Boolean>) {
    val prefs = EtherealApplication.sharedPreferences

    BasicAlertDialog(
        onDismissRequest = { showDialog.value = false }, properties = DialogProperties(
            decorFitsSystemWindows = true,
            usePlatformDefaultWidth = false,
        )
    ) {
        Surface(
            modifier = Modifier
                .width(310.dp)
                .wrapContentHeight(),
            shape = RoundedCornerShape(30.dp),
            tonalElevation = AlertDialogDefaults.TonalElevation,
            color = AlertDialogDefaults.containerColor,
        ) {
            LazyColumn {
                items(colorsList()) {
                    ListItem(
                        headlineContent = { Text(text = stringResource(it.nameId)) },
                        modifier = Modifier.clickable {
                            showDialog.value = false
                            prefs.edit { putString("custom_color", it.name) }
                            refreshTheme.value = true
                        })
                }

            }

            val dialogWindowProvider = LocalView.current.parent as? DialogWindowProvider
            dialogWindowProvider?.window?.let { EtherealDialogBlurBehindUtils.setupWindowBlurListener(it) }
        }
    }

}

private data class ThemeColor(
    val name: String, @param:StringRes val nameId: Int
)

private fun colorsList(): List<ThemeColor> {
    return listOf(
        ThemeColor("amber", R.string.amber_theme),
        ThemeColor("blue_grey", R.string.blue_grey_theme),
        ThemeColor("blue", R.string.blue_theme),
        ThemeColor("brown", R.string.brown_theme),
        ThemeColor("cyan", R.string.cyan_theme),
        ThemeColor("deep_orange", R.string.deep_orange_theme),
        ThemeColor("deep_purple", R.string.deep_purple_theme),
        ThemeColor("green", R.string.green_theme),
        ThemeColor("indigo", R.string.indigo_theme),
        ThemeColor("light_blue", R.string.light_blue_theme),
        ThemeColor("light_green", R.string.light_green_theme),
        ThemeColor("lime", R.string.lime_theme),
        ThemeColor("orange", R.string.orange_theme),
        ThemeColor("pink", R.string.pink_theme),
        ThemeColor("purple", R.string.purple_theme),
        ThemeColor("red", R.string.red_theme),
        ThemeColor("sakura", R.string.sakura_theme),
        ThemeColor("teal", R.string.teal_theme),
        ThemeColor("yellow", R.string.yellow_theme),
    )
}

@Composable
private fun colorNameToString(colorName: String): Int {
    return colorsList().find { it.name == colorName }?.nameId ?: R.string.blue_theme
}

val suPathChecked: (path: String) -> Boolean = {
    it.startsWith("/") && it.trim().length > 1
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ResetSUPathDialog(showDialog: MutableState<Boolean>) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var suPath by remember { mutableStateOf(EtherealApplication.DEFAULT_SU_PATH) }
    LaunchedEffect(Unit) {
        val current = withContext(Dispatchers.IO) {
            runCatching { Natives.suPath() }.getOrDefault("")
        }
        if (current.isNotBlank()) suPath = current
    }
    BasicAlertDialog(
        onDismissRequest = { showDialog.value = false }, properties = DialogProperties(
            decorFitsSystemWindows = true,
            usePlatformDefaultWidth = false,
        )
    ) {
        Surface(
            modifier = Modifier
                .width(310.dp)
                .wrapContentHeight(),
            shape = RoundedCornerShape(30.dp),
            tonalElevation = AlertDialogDefaults.TonalElevation,
            color = AlertDialogDefaults.containerColor,
        ) {
            Column(modifier = Modifier.padding(PaddingValues(all = 24.dp))) {
                Box(
                    Modifier
                        .padding(PaddingValues(bottom = 16.dp))
                        .align(Alignment.Start)
                ) {
                    Text(
                        text = stringResource(id = R.string.setting_reset_su_path),
                        style = MaterialTheme.typography.headlineSmall
                    )
                }
                Box(
                    Modifier
                        .weight(weight = 1f, fill = false)
                        .padding(PaddingValues(bottom = 12.dp))
                        .align(Alignment.Start)
                ) {
                    OutlinedTextField(
                        value = suPath,
                        onValueChange = {
                            suPath = it
                        },
                        label = { Text(stringResource(id = R.string.setting_reset_su_new_path)) },
                        visualTransformation = VisualTransformation.None,
                    )
                }

                Row(
                    modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End
                ) {
                    TextButton(onClick = { showDialog.value = false }) {

                        Text(stringResource(id = android.R.string.cancel))
                    }

                    Button(enabled = suPathChecked(suPath), onClick = {
                        showDialog.value = false
                        scope.launch {
                            val success = withContext(Dispatchers.IO) {
                                runCatching {
                                    check(becomeRoot()) { "SuperCall did not make uid 0" }
                                    check(Natives.resetSuPath(suPath)) {
                                        "kernel rejected the su path"
                                    }
                                    val target = File(EtherealApplication.SU_PATH_FILE)
                                    target.parentFile?.let { parent ->
                                        check(parent.isDirectory || parent.mkdirs()) {
                                            "create $parent"
                                        }
                                    }
                                    target.writeText(suPath)
                                    true
                                }.getOrDefault(false)
                            }
                            Toast.makeText(
                                context,
                                if (success) R.string.success else R.string.failure,
                                Toast.LENGTH_SHORT
                            ).show()
                        }
                    }) {
                        Text(stringResource(id = android.R.string.ok))
                    }
                }
            }
            val dialogWindowProvider = LocalView.current.parent as? DialogWindowProvider
            dialogWindowProvider?.window?.let { EtherealDialogBlurBehindUtils.setupWindowBlurListener(it) }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SelinuxHideWarningDialog(
    showDialog: MutableState<Boolean>,
    kernelVersion: Int?,
    isGki: Boolean,
    onConfirm: () -> Unit,
) {
    BasicAlertDialog(
        onDismissRequest = { showDialog.value = false }, properties = DialogProperties(
            decorFitsSystemWindows = true,
            usePlatformDefaultWidth = false,
        )
    ) {
        Surface(
            modifier = Modifier
                .width(310.dp)
                .wrapContentHeight(),
            shape = RoundedCornerShape(30.dp),
            tonalElevation = AlertDialogDefaults.TonalElevation,
            color = AlertDialogDefaults.containerColor,
        ) {
            Column(modifier = Modifier.padding(PaddingValues(all = 24.dp))) {
                Box(
                    Modifier
                        .padding(PaddingValues(bottom = 16.dp))
                        .align(Alignment.Start)
                ) {
                    Text(
                        text = stringResource(id = R.string.settings_selinux_hide_warning_title),
                        style = MaterialTheme.typography.headlineSmall
                    )
                }
                if ((kernelVersion ?: 0) < 510) {
                    Box(
                        Modifier
                            .padding(PaddingValues(bottom = 8.dp))
                            .align(Alignment.Start)
                    ) {
                        Text(
                            text = stringResource(id = R.string.settings_selinux_hide_warning_below_5_10),
                            style = MaterialTheme.typography.bodyMedium
                        )
                    }
                }
                if (!isGki) {
                    Box(
                        Modifier
                            .padding(PaddingValues(bottom = 16.dp))
                            .align(Alignment.Start)
                    ) {
                        Text(
                            text = stringResource(id = R.string.settings_selinux_hide_warning_non_gki),
                            style = MaterialTheme.typography.bodyMedium
                        )
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End
                ) {
                    TextButton(onClick = { showDialog.value = false }) {
                        Text(stringResource(id = android.R.string.cancel))
                    }

                    Button(onClick = {
                        showDialog.value = false
                        onConfirm()
                    }) {
                        Text(stringResource(id = android.R.string.ok))
                    }
                }
            }
            val dialogWindowProvider = LocalView.current.parent as? DialogWindowProvider
            dialogWindowProvider?.window?.let { EtherealDialogBlurBehindUtils.setupWindowBlurListener(it) }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LanguageDialog(showLanguageDialog: MutableState<Boolean>) {

    val languages = stringArrayResource(id = R.array.languages)
    val languagesValues = stringArrayResource(id = R.array.languages_values)

    if (showLanguageDialog.value) {
        BasicAlertDialog(
            onDismissRequest = { showLanguageDialog.value = false }
        ) {
            Surface(
                modifier = Modifier
                    .width(150.dp)
                    .wrapContentHeight(),
                shape = RoundedCornerShape(28.dp),
                tonalElevation = AlertDialogDefaults.TonalElevation,
                color = AlertDialogDefaults.containerColor,
            ) {
                LazyColumn {
                    itemsIndexed(languages) { index, item ->
                        ListItem(
                            headlineContent = { Text(item) },
                            modifier = Modifier.clickable {
                                showLanguageDialog.value = false
                                if (index == 0) {
                                    AppCompatDelegate.setApplicationLocales(
                                        LocaleListCompat.getEmptyLocaleList()
                                    )
                                } else {
                                    AppCompatDelegate.setApplicationLocales(
                                        LocaleListCompat.forLanguageTags(
                                            languagesValues[index]
                                        )
                                    )
                                }
                            }
                        )
                    }
                }
            }
            val dialogWindowProvider = LocalView.current.parent as? DialogWindowProvider
            dialogWindowProvider?.window?.let { EtherealDialogBlurBehindUtils.setupWindowBlurListener(it) }
        }
    }
}
