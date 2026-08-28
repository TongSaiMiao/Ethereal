package me.ethereal.app.ui.screen

import android.os.Build
import android.system.Os
import androidx.annotation.StringRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Android
import androidx.compose.material.icons.filled.Fingerprint
import androidx.compose.material.icons.filled.Memory
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PhoneAndroid
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.Security
import androidx.compose.material.icons.filled.Storage
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.outlined.Block
import androidx.compose.material.icons.outlined.Clear
import androidx.compose.material.icons.outlined.TaskAlt
import androidx.compose.material.icons.outlined.Warning
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.livedata.observeAsState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import com.ramcosta.composedestinations.generated.destinations.AboutScreenDestination
import com.ramcosta.composedestinations.generated.destinations.BootInstallScreenDestination
import com.ramcosta.composedestinations.navigation.DestinationsNavigator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import me.ethereal.app.EtherealApplication
import me.ethereal.app.R
import me.ethereal.app.etherealApp
import me.ethereal.app.ui.component.LabelText
import me.ethereal.app.ui.component.ProvideMenuShape
import me.ethereal.app.ui.component.WarningCard
import me.ethereal.app.ui.component.rememberConfirmDialog
import me.ethereal.app.util.LatestVersionInfo
import me.ethereal.app.util.Version
import me.ethereal.app.util.Version.getManagerVersion
import me.ethereal.app.util.checkNewVersion
import me.ethereal.app.util.getSELinuxStatus
import me.ethereal.app.util.reboot

private val managerVersion by lazy {
    runCatching { getManagerVersion() }.getOrDefault("Ethereal" to 0L)
}

@OptIn(ExperimentalMaterial3Api::class)
@Destination<RootGraph>(start = true)
@Composable
fun HomeScreen(navigator: DestinationsNavigator) {
    val kernelState by EtherealApplication.kernelStateLiveData.observeAsState(EtherealApplication.State.UNKNOWN_STATE)
    val serviceState by EtherealApplication.serviceStateLiveData.observeAsState(EtherealApplication.State.UNKNOWN_STATE)
    val managerAccessState by EtherealApplication.managerAccessStateLiveData.observeAsState(
        EtherealApplication.ManagerAccessState.UNKNOWN
    )
    val scrollState = rememberScrollState()

    Scaffold(
        topBar = {
            TopBar(
                navigator,
                managerAccessState == EtherealApplication.ManagerAccessState.AUTHENTICATED,
            )
        },
        containerColor = Color.Transparent,
        contentWindowInsets = WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal)
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .padding(innerPadding)
                .padding(horizontal = 16.dp)
                .verticalScroll(scrollState),
            verticalArrangement = Arrangement.spacedBy(10.dp)
        ) {
            KStatusCard(kernelState, serviceState, onClick = {
                navigator.navigate(BootInstallScreenDestination)
            })
            val prefs = EtherealApplication.sharedPreferences
            val checkUpdate by produceState(initialValue = prefs.getBoolean("check_update", true)) {
                val listener = android.content.SharedPreferences.OnSharedPreferenceChangeListener { p, key ->
                    if (key == "check_update") {
                        value = p.getBoolean(key, true)
                    }
                }
                prefs.registerOnSharedPreferenceChangeListener(listener)
                awaitDispose { prefs.unregisterOnSharedPreferenceChangeListener(listener) }
            }
            if (checkUpdate) {
                UpdateCard()
            }
            InfoCard(kernelState, serviceState)
            LearnMoreCard()
            Spacer(Modifier)
        }
    }
}

@Composable
fun RebootDropdownItem(@StringRes id: Int, reason: String = "", onClick: (() -> Unit)? = null) {
    val scope = rememberCoroutineScope()
    DropdownMenuItem(text = {
        Text(stringResource(id))
    }, onClick = onClick ?: {
        scope.launch(Dispatchers.IO) { reboot(reason) }
        Unit
    })
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TopBar(
    navigator: DestinationsNavigator,
    managerAccessReady: Boolean,
) {
    var showDropdownMoreOptions by remember { mutableStateOf(false) }
    var showDropdownReboot by remember { mutableStateOf(false) }

    TopAppBar(
        title = {
            Text(stringResource(R.string.app_name))
        },
        colors = TopAppBarDefaults.topAppBarColors(
            containerColor = Color.Transparent
        ),
        actions = {
        if (managerAccessReady) {
            val downloadTitle = stringResource(id = R.string.reboot_download)
            val downloadConfirmText = stringResource(id = R.string.reboot_download_confirm)
            val edlTitle = stringResource(id = R.string.reboot_edl)
            val edlConfirmText = stringResource(id = R.string.reboot_edl_confirm)
            var pendingRebootReason by remember { mutableStateOf<String?>(null) }
            val scope = rememberCoroutineScope()
            val rebootConfirmDialog = rememberConfirmDialog(onConfirm = {
                pendingRebootReason?.let { reason ->
                    scope.launch(Dispatchers.IO) { reboot(reason) }
                }
            })

            IconButton(onClick = {
                showDropdownReboot = true
            }) {
                Icon(
                    imageVector = Icons.Filled.PowerSettingsNew,
                    contentDescription = stringResource(id = R.string.reboot)
                )

                ProvideMenuShape(RoundedCornerShape(10.dp)) {
                    DropdownMenu(expanded = showDropdownReboot, onDismissRequest = {
                        showDropdownReboot = false
                    }) {
                        RebootDropdownItem(id = R.string.reboot)
                        RebootDropdownItem(id = R.string.reboot_recovery, reason = "recovery")
                        RebootDropdownItem(id = R.string.reboot_bootloader, reason = "bootloader")
                        RebootDropdownItem(id = R.string.reboot_download, onClick = {
                            showDropdownReboot = false
                            pendingRebootReason = "download"
                            rebootConfirmDialog.showConfirm(
                                title = downloadTitle, content = downloadConfirmText
                            )
                        })
                        RebootDropdownItem(id = R.string.reboot_edl, onClick = {
                            showDropdownReboot = false
                            pendingRebootReason = "edl"
                            rebootConfirmDialog.showConfirm(
                                title = edlTitle, content = edlConfirmText
                            )
                        })
                    }
                }
            }
        }

        Box {
            IconButton(onClick = { showDropdownMoreOptions = true }) {
                Icon(
                    imageVector = Icons.Filled.MoreVert,
                    contentDescription = stringResource(id = R.string.settings)
                )
                ProvideMenuShape(RoundedCornerShape(10.dp)) {
                    DropdownMenu(expanded = showDropdownMoreOptions, onDismissRequest = {
                        showDropdownMoreOptions = false
                    }) {
                        DropdownMenuItem(text = {
                            Text(stringResource(R.string.home_more_menu_about))
                        }, onClick = {
                            navigator.navigate(AboutScreenDestination)
                            showDropdownMoreOptions = false
                        })
                    }
                }
            }
        }
    })
}

@Composable
private fun KStatusCard(
    kernelState: EtherealApplication.State,
    serviceState: EtherealApplication.State,
    onClick: () -> Unit = {},
) {

    val working = kernelState == EtherealApplication.State.KERNEL_WORKING
    val serviceWorking = serviceState == EtherealApplication.State.SERVICE_INSTALLED
    val serviceInstalling = serviceState == EtherealApplication.State.SERVICE_INSTALLING
    val containerColor =
        if (working) MaterialTheme.colorScheme.secondaryContainer
        else MaterialTheme.colorScheme.errorContainer

    ElevatedCard(
        modifier = Modifier
            .clip(CardDefaults.elevatedShape)
            .clickable(onClick = onClick),
        colors = CardDefaults.elevatedCardColors(containerColor = containerColor),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp),
            verticalAlignment = Alignment.CenterVertically
        ) {
            when {
                working -> {
                    Icon(
                        Icons.Outlined.TaskAlt,
                        contentDescription = stringResource(R.string.home_working),
                        tint = MaterialTheme.colorScheme.primary,
                        modifier = Modifier
                            .size(28.dp)
                            .padding(horizontal = 4.dp),
                    )
                    Column(
                        Modifier
                            .padding(start = 20.dp)
                            .weight(1f)
                    ) {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier.fillMaxWidth()
                        ) {
                            Text(
                                text = stringResource(R.string.home_working),
                                style = MaterialTheme.typography.titleMedium,
                                color = MaterialTheme.colorScheme.primary,
                            )
                            Spacer(Modifier.width(8.dp))
                            LabelText(
                                label = stringResource(R.string.home_working_mode),
                                containerColor = MaterialTheme.colorScheme.primary
                            )
                        }
                        Spacer(Modifier.height(4.dp))
                        Text(
                            text = stringResource(
                                R.string.home_working_version,
                                managerVersion.second.toString()
                            ),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.secondary,
                        )
                        Spacer(Modifier.height(8.dp))
                        Surface(
                            shape = RoundedCornerShape(8.dp),
                            color = if (serviceWorking) {
                                MaterialTheme.colorScheme.primary.copy(alpha = 0.14f)
                            } else {
                                MaterialTheme.colorScheme.error.copy(alpha = 0.12f)
                            }
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 8.dp, vertical = 5.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = stringResource(R.string.module_service),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = if (serviceWorking) {
                                        MaterialTheme.colorScheme.primary
                                    } else {
                                        MaterialTheme.colorScheme.error
                                    }
                                )
                                Spacer(Modifier.width(6.dp))
                                Text(
                                    text = stringResource(
                                        when {
                                            serviceWorking -> R.string.home_working
                                            serviceInstalling -> R.string.home_installing
                                            else -> R.string.home_not_installed
                                        }
                                    ),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = if (serviceWorking) {
                                        MaterialTheme.colorScheme.onSecondaryContainer
                                    } else {
                                        MaterialTheme.colorScheme.error
                                    }
                                )
                            }
                        }
                    }
                }

                else -> {
                    Icon(
                        Icons.Outlined.Block,
                        contentDescription = stringResource(R.string.home_not_installed),
                        tint = MaterialTheme.colorScheme.error,
                        modifier = Modifier
                            .size(28.dp)
                            .padding(horizontal = 4.dp),
                    )
                    Column(Modifier.padding(start = 20.dp)) {
                        Text(
                            text = stringResource(R.string.app_name),
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.error
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            text = stringResource(R.string.home_not_installed),
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onErrorContainer
                        )
                        Spacer(Modifier.height(8.dp))
                        Surface(
                            shape = RoundedCornerShape(8.dp),
                            color = MaterialTheme.colorScheme.error.copy(alpha = 0.12f)
                        ) {
                            Row(
                                modifier = Modifier.padding(horizontal = 8.dp, vertical = 5.dp),
                                verticalAlignment = Alignment.CenterVertically
                            ) {
                                Text(
                                    text = stringResource(R.string.module_service),
                                    style = MaterialTheme.typography.labelMedium,
                                    color = MaterialTheme.colorScheme.error
                                )
                                Spacer(Modifier.width(6.dp))
                                Text(
                                    text = stringResource(R.string.home_not_installed),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.error
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun BackupWarningCard() {
    var show by rememberSaveable { mutableStateOf(etherealApp.getBackupWarningState()) }
    if (show) {
        ElevatedCard(
            modifier = Modifier.clip(CardDefaults.elevatedShape),
            elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
            colors = CardDefaults.elevatedCardColors(
                containerColor = MaterialTheme.colorScheme.errorContainer
            )
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(12.dp)
            ) {
                Column(
                    modifier = Modifier.padding(12.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Icon(Icons.Filled.Warning, contentDescription = "warning")
                }
                Column(
                    modifier = Modifier.padding(12.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .align(Alignment.CenterHorizontally),
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Text(
                            modifier = Modifier.weight(1f),
                            text = stringResource(id = R.string.patch_warnning),
                        )

                        Spacer(Modifier.width(12.dp))

                        Icon(
                            Icons.Outlined.Clear,
                            contentDescription = "",
                            modifier = Modifier.clickable {
                                show = false
                                etherealApp.updateBackupWarningState(false)
                            },
                        )
                    }
                }
            }
        }
    }
}

private fun getSystemVersion(): String {
    return "${Build.VERSION.RELEASE} ${if (Build.VERSION.PREVIEW_SDK_INT != 0) "Preview" else ""} (API ${Build.VERSION.SDK_INT})"
}

private fun getDeviceInfo(): String {
    var manufacturer =
        Build.MANUFACTURER[0].uppercaseChar().toString() + Build.MANUFACTURER.substring(1)
    if (!Build.BRAND.equals(Build.MANUFACTURER, ignoreCase = true)) {
        manufacturer += " " + Build.BRAND[0].uppercaseChar() + Build.BRAND.substring(1)
    }
    manufacturer += " " + Build.MODEL + " "
    return manufacturer
}

@Composable
private fun InfoCard(kernelState: EtherealApplication.State, serviceState: EtherealApplication.State) {
    ElevatedCard(
        modifier = Modifier.clip(CardDefaults.elevatedShape),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHighest
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 16.dp, top = 12.dp, end = 16.dp, bottom = 8.dp)
        ) {
            val uname = Os.uname()

            @Composable
            fun InfoCardItem(label: String, content: String, icon: ImageVector) {
                Row(
                    verticalAlignment = Alignment.Top,
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 8.dp)
                ) {
                    Icon(
                        imageVector = icon,
                        contentDescription = label,
                        modifier = Modifier
                            .size(28.dp)
                            .padding(vertical = 4.dp),
                    )
                    Spacer(modifier = Modifier.width(16.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text(text = label, style = MaterialTheme.typography.labelLarge)
                        Text(text = content, style = MaterialTheme.typography.bodyMedium)
                    }
                }
            }

            if (kernelState != EtherealApplication.State.UNKNOWN_STATE) {
                InfoCardItem(
                    stringResource(R.string.home_su_path),
                    EtherealApplication.DEFAULT_SU_PATH,
                    Icons.Filled.Security
                )
            }

            if (kernelState == EtherealApplication.State.KERNEL_WORKING) {
                InfoCardItem(
                    stringResource(R.string.home_ethereal_version),
                    managerVersion.second.toString(),
                    Icons.Filled.Storage
                )
            }

            InfoCardItem(
                stringResource(R.string.home_device_info),
                getDeviceInfo(),
                Icons.Filled.PhoneAndroid
            )
            InfoCardItem(
                stringResource(R.string.home_kernel),
                uname.release,
                Icons.Filled.Memory
            )
            InfoCardItem(
                stringResource(R.string.home_system_version),
                getSystemVersion(),
                Icons.Filled.Android
            )
            InfoCardItem(
                stringResource(R.string.home_fingerprint),
                Build.FINGERPRINT,
                Icons.Filled.Fingerprint
            )
            InfoCardItem(
                stringResource(R.string.home_selinux_status),
                getSELinuxStatus(),
                Icons.Filled.Security
            )
        }
    }
}

@Composable
fun UpdateCard() {
    val latestVersionInfo = LatestVersionInfo()
    val newVersion by produceState(initialValue = latestVersionInfo) {
        value = withContext(Dispatchers.IO) {
            checkNewVersion()
        }
    }
    val currentVersionCode = managerVersion.second
    val newVersionCode = newVersion.versionCode
    val newVersionUrl = newVersion.downloadUrl
    val changelog = newVersion.changelog

    val uriHandler = LocalUriHandler.current
    val title = stringResource(id = R.string.module_changelog)
    val updateText = stringResource(id = R.string.module_update)

    AnimatedVisibility(
        visible = newVersionCode > currentVersionCode,
        enter = fadeIn() + expandVertically(),
        exit = shrinkVertically() + fadeOut()
    ) {
        val updateDialog = rememberConfirmDialog(onConfirm = { uriHandler.openUri(newVersionUrl) })
        WarningCard(
            message = stringResource(id = R.string.home_new_ethereal_found).format(newVersionCode),
            color = MaterialTheme.colorScheme.outlineVariant,
            onClick = {
                if (changelog.isEmpty()) {
                    uriHandler.openUri(newVersionUrl)
                } else {
                    updateDialog.showConfirm(
                        title = title, content = changelog, markdown = true, confirm = updateText
                    )
                }
            }
        )
    }
}

@Composable
fun LearnMoreCard() {
    ElevatedCard(
        modifier = Modifier.clip(CardDefaults.elevatedShape),
        colors = CardDefaults.elevatedCardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerHighest
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp)
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column {
                Text(
                    text = stringResource(R.string.home_learn_ethereal),
                    style = MaterialTheme.typography.titleSmall
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    text = stringResource(R.string.home_click_to_learn_ethereal),
                    style = MaterialTheme.typography.bodyMedium
                )
            }
        }
    }
}
