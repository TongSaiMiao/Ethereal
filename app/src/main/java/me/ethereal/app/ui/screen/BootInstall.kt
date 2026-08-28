package me.ethereal.app.ui.screen

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Parcelable
import android.provider.OpenableColumns
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Input
import androidx.compose.material.icons.filled.AutoFixHigh
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ElevatedCard
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import com.ramcosta.composedestinations.generated.destinations.FlashScreenDestination
import com.ramcosta.composedestinations.navigation.DestinationsNavigator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.parcelize.Parcelize
import me.ethereal.app.EtherealApplication
import me.ethereal.app.R
import me.ethereal.app.ui.component.rememberConfirmDialog
import me.ethereal.app.ui.component.settings.SettingsBaseWidget
import me.ethereal.app.util.FlashIt
import me.ethereal.app.util.LkmSelection
import me.ethereal.app.util.becomeRoot
import me.ethereal.app.util.isABDevice
import me.ethereal.app.util.isGki2Device
import me.ethereal.app.util.isGkiKernel
import me.ethereal.app.util.isKoFile
import me.ethereal.app.util.slotSuffix

@Parcelize
sealed class InstallMethod : Parcelable {
    @Parcelize
    data class SelectFile(
        val bootUri: String? = null,
        val initBootUri: String? = null,
        val gki2: Boolean,
        @param:StringRes override val label: Int = R.string.select_file,
        override val summary: String?,
    ) : InstallMethod()

    @Parcelize
    data object DirectInstall : InstallMethod() {
        override val label: Int get() = R.string.direct_install
    }

    @Parcelize
    data object DirectInstallToInactiveSlot : InstallMethod() {
        override val label: Int get() = R.string.install_inactive_slot
    }

    abstract val label: Int
    open val summary: String? get() = null
}

internal enum class BootImageKind { INIT_BOOT, BOOT }

internal fun updateBootImageSelection(
    current: InstallMethod.SelectFile?,
    kind: BootImageKind,
    uri: String,
    gki2: Boolean,
    summary: String?,
): InstallMethod.SelectFile {
    val selection = current
        ?.takeIf { it.gki2 == gki2 }
        ?: InstallMethod.SelectFile(gki2 = gki2, summary = summary)
    return when (kind) {
        BootImageKind.INIT_BOOT -> selection.copy(initBootUri = uri)
        BootImageKind.BOOT -> selection.copy(bootUri = uri)
    }
}

internal fun hasRequiredBootImages(method: InstallMethod.SelectFile): Boolean =
    method.bootUri != null &&
        (!method.gki2 || (method.initBootUri != null && method.initBootUri != method.bootUri))

@OptIn(ExperimentalMaterial3Api::class)
@Destination<RootGraph>
@Composable
fun BootInstallScreen(navigator: DestinationsNavigator) {
    val context = LocalContext.current
    var installMethod by rememberSaveable { mutableStateOf<InstallMethod?>(null) }
    var lkmSelection by rememberSaveable { mutableStateOf<LkmSelection>(LkmSelection.KmiNone) }
    val isGKI = remember { isGkiKernel() }
    val isGki2 = remember { isGki2Device() }

    val onInstall = {
        installMethod?.let { method ->
            val isOta = method is InstallMethod.DirectInstallToInactiveSlot
            val source = when (method) {
                is InstallMethod.SelectFile -> {
                    val boot = method.bootUri ?: return@let
                    if (method.gki2) {
                        val initBoot = method.initBootUri ?: return@let
                        if (initBoot == boot) return@let
                        FlashIt.BootSource.Gki2Files(initBoot, boot)
                    } else {
                        FlashIt.BootSource.Gki1File(boot)
                    }
                }
                InstallMethod.DirectInstall,
                InstallMethod.DirectInstallToInactiveSlot -> FlashIt.BootSource.Direct
            }
            navigator.navigate(
                FlashScreenDestination(
                    FlashIt.FlashBoot(
                        source = source,
                        lkm = lkmSelection,
                        ota = isOta,
                    )
                )
            )
        }
        Unit
    }

    val installOnlySupportKoFile = stringResource(R.string.install_only_support_ko_file)
    val selectLkmLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) {
        if (it.resultCode == Activity.RESULT_OK) {
            it.data?.data?.let { uri ->
                if (isKoFile(uri)) {
                    lkmSelection = LkmSelection.LkmUri(uri.toString())
                } else {
                    lkmSelection = LkmSelection.KmiNone
                    Toast.makeText(context, installOnlySupportKoFile, Toast.LENGTH_SHORT).show()
                }
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.install)) },
                navigationIcon = {
                    IconButton(onClick = { navigator.popBackStack() }) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = Color.Transparent)
            )
        },
        containerColor = Color.Transparent
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .verticalScroll(rememberScrollState())
                .padding(top = 12.dp)
        ) {
            SelectInstallMethod(
                isGKI = isGKI,
                isGki2 = isGki2,
                onSelected = { installMethod = it },
                selectedMethod = installMethod
            )

            AnimatedVisibility(
                visible = installMethod is InstallMethod.DirectInstall ||
                    installMethod is InstallMethod.DirectInstallToInactiveSlot,
                enter = fadeIn() + expandVertically(),
                exit = shrinkVertically() + fadeOut()
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(16.dp)
                ) {
                    ElevatedCard(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(bottom = 12.dp)
                    ) {
                        val isOta = installMethod is InstallMethod.DirectInstallToInactiveSlot
                        val suffix = produceState(initialValue = "", isOta) {
                            value = slotSuffix(isOta)
                        }.value
                        SettingsBaseWidget(
                            icon = Icons.Default.Edit,
                            title = "${stringResource(R.string.install_select_partition)} ($suffix)",
                            description = if (isGki2) "init_boot$suffix + boot$suffix" else "boot$suffix",
                            onClick = null,
                        ) { }
                    }
                }
            }

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(16.dp)
            ) {
                if (isGKI) {
                    ElevatedCard(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(bottom = 12.dp)
                    ) {
                        SettingsBaseWidget(
                            title = stringResource(id = R.string.install_upload_lkm_file),
                            onClick = { _ ->
                                selectLkmLauncher.launch(Intent(Intent.ACTION_GET_CONTENT).apply {
                                    type = "application/octet-stream"
                                })
                            },
                            description = (lkmSelection as? LkmSelection.LkmUri)?.let {
                                stringResource(
                                    id = R.string.selected_lkm,
                                    Uri.parse(it.uriString).lastPathSegment ?: "(file)"
                                )
                            },
                            icon = Icons.AutoMirrored.Filled.Input,
                        ) { }
                    }
                }

                Button(
                    modifier = Modifier.fillMaxWidth(),
                    enabled = when (val method = installMethod) {
                        null -> false
                        is InstallMethod.SelectFile -> hasRequiredBootImages(method)
                        else -> true
                    },
                    onClick = onInstall,
                    shape = MaterialTheme.shapes.medium,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.primary,
                        contentColor = MaterialTheme.colorScheme.onPrimary,
                        disabledContainerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f),
                        disabledContentColor = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.6f)
                    )
                ) {
                    Text(
                        stringResource(id = R.string.install_next),
                        style = MaterialTheme.typography.bodyMedium
                    )
                }
            }
        }
    }
}

@Composable
private fun SelectInstallMethod(
    isGKI: Boolean = false,
    isGki2: Boolean,
    onSelected: (InstallMethod) -> Unit = {},
    selectedMethod: InstallMethod? = null,
) {
    val rootAvailable by produceState(initialValue = false) {
        value = withContext(Dispatchers.IO) {
            EtherealApplication.kernelModulePresent() && becomeRoot()
        }
    }
    val isAbDevice = remember { isABDevice() }
    val selectFileTip = if (isGki2) {
        stringResource(R.string.select_pair_tip)
    } else {
        stringResource(R.string.select_file_tip, "boot")
    }

    val radioOptions = remember(rootAvailable, isAbDevice, selectFileTip, isGki2) {
        val list = mutableListOf<InstallMethod>(
            InstallMethod.SelectFile(gki2 = isGki2, summary = selectFileTip)
        )
        if (rootAvailable) {
            list.add(InstallMethod.DirectInstall)
            if (isAbDevice) list.add(InstallMethod.DirectInstallToInactiveSlot)
        }
        list
    }

    val context = LocalContext.current

    fun selectImage(kind: BootImageKind, uri: Uri) {
        val current = selectedMethod as? InstallMethod.SelectFile
        runCatching {
            context.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        }
        val oldUri = when (kind) {
            BootImageKind.INIT_BOOT -> current?.initBootUri
            BootImageKind.BOOT -> current?.bootUri
        }
        val retainedUri = when (kind) {
            BootImageKind.INIT_BOOT -> current?.bootUri
            BootImageKind.BOOT -> current?.initBootUri
        }
        if (oldUri != null && oldUri != uri.toString() && oldUri != retainedUri) {
            runCatching {
                context.contentResolver.releasePersistableUriPermission(
                    Uri.parse(oldUri),
                    Intent.FLAG_GRANT_READ_URI_PERMISSION,
                )
            }
        }
        onSelected(
            updateBootImageSelection(
                current = current,
                kind = kind,
                uri = uri.toString(),
                gki2 = isGki2,
                summary = selectFileTip,
            )
        )
    }

    val selectInitBootLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri?.let { selectImage(BootImageKind.INIT_BOOT, it) }
    }
    val selectBootLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri?.let { selectImage(BootImageKind.BOOT, it) }
    }

    val confirmDialog = rememberConfirmDialog(
        onConfirm = {
            onSelected(InstallMethod.DirectInstallToInactiveSlot)
        },
        onDismiss = null
    )
    val dialogTitle = stringResource(id = android.R.string.dialog_alert_title)
    val dialogContent = stringResource(id = R.string.install_inactive_slot_warning)

    val onClick = { option: InstallMethod ->
        when (option) {
            is InstallMethod.SelectFile -> {
                val current = (selectedMethod as? InstallMethod.SelectFile)
                    ?.takeIf { it.gki2 == isGki2 }
                    ?: option
                onSelected(current)
            }
            is InstallMethod.DirectInstall -> onSelected(option)
            is InstallMethod.DirectInstallToInactiveSlot -> {
                confirmDialog.showConfirm(dialogTitle, dialogContent)
            }
        }
    }

    var lkmExpanded by remember { mutableStateOf(true) }
    val notSelected = stringResource(R.string.file_not_selected)
    val selectedFiles = selectedMethod as? InstallMethod.SelectFile
    val initBootName = rememberDocumentDisplayName(selectedFiles?.initBootUri, notSelected)
    val bootName = rememberDocumentDisplayName(selectedFiles?.bootUri, notSelected)

    Column(modifier = Modifier.padding(horizontal = 16.dp)) {
        if (isGKI) {
            ElevatedCard(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 16.dp)
            ) {
                ListItem(
                    leadingContent = {
                        Icon(
                            Icons.Filled.AutoFixHigh,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary
                        )
                    },
                    headlineContent = {
                        Text(
                            stringResource(R.string.Lkm_install_methods),
                            style = MaterialTheme.typography.titleMedium
                        )
                    },
                    modifier = Modifier.clickable { lkmExpanded = !lkmExpanded }
                )
                AnimatedVisibility(
                    visible = lkmExpanded,
                    enter = fadeIn() + expandVertically(),
                    exit = shrinkVertically() + fadeOut()
                ) {
                    Column(modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 16.dp)) {
                        radioOptions.forEach { option ->
                            MethodRadio(option, selectedMethod, onClick)
                        }
                        selectedFiles?.let { files ->
                            if (files.gki2) {
                                SettingsBaseWidget(
                                    icon = Icons.AutoMirrored.Filled.Input,
                                    title = stringResource(R.string.select_init_boot_image),
                                    description = initBootName,
                                    onClick = { selectInitBootLauncher.launch(arrayOf("*/*")) },
                                ) { }
                            }
                            SettingsBaseWidget(
                                icon = Icons.AutoMirrored.Filled.Input,
                                title = stringResource(R.string.select_boot_image),
                                description = bootName,
                                onClick = { selectBootLauncher.launch(arrayOf("*/*")) },
                            ) { }
                        }
                    }
                }
            }
        } else {
            radioOptions.forEach { option ->
                MethodRadio(option, selectedMethod, onClick)
            }
            selectedFiles?.let { files ->
                if (files.gki2) {
                    SettingsBaseWidget(
                        icon = Icons.AutoMirrored.Filled.Input,
                        title = stringResource(R.string.select_init_boot_image),
                        description = initBootName,
                        onClick = { selectInitBootLauncher.launch(arrayOf("*/*")) },
                    ) { }
                }
                SettingsBaseWidget(
                    icon = Icons.AutoMirrored.Filled.Input,
                    title = stringResource(R.string.select_boot_image),
                    description = bootName,
                    onClick = { selectBootLauncher.launch(arrayOf("*/*")) },
                ) { }
            }
        }
    }
}

@Composable
private fun rememberDocumentDisplayName(uriString: String?, notSelected: String): String {
    val context = LocalContext.current
    val fallback = remember(uriString, notSelected) {
        uriString
            ?.let(Uri::parse)
            ?.lastPathSegment
            ?.takeIf(String::isNotBlank)
            ?: if (uriString == null) notSelected else "(file)"
    }
    return key(uriString, notSelected) {
        produceState(initialValue = fallback, uriString, notSelected) {
            if (uriString == null) {
                value = notSelected
                return@produceState
            }
            value = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.query(
                        Uri.parse(uriString),
                        arrayOf(OpenableColumns.DISPLAY_NAME),
                        null,
                        null,
                        null,
                    )?.use { cursor ->
                        val nameColumn = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        if (nameColumn >= 0 && cursor.moveToFirst()) {
                            cursor.getString(nameColumn)
                        } else {
                            null
                        }
                    }
                }.getOrNull()?.takeIf(String::isNotBlank) ?: fallback
            }
        }.value
    }
}

@Composable
private fun MethodRadio(
    option: InstallMethod,
    selectedOption: InstallMethod?,
    onClick: (InstallMethod) -> Unit,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val selected = option.javaClass == selectedOption?.javaClass
    Surface(
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer
        else MaterialTheme.colorScheme.surfaceContainerHighest,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 4.dp)
            .clip(MaterialTheme.shapes.medium)
    ) {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier
                .fillMaxWidth()
                .selectable(
                    selected = selected,
                    onClick = { onClick(option) },
                    role = Role.RadioButton,
                    indication = LocalIndication.current,
                    interactionSource = interactionSource
                )
                .padding(vertical = 8.dp, horizontal = 12.dp)
        ) {
            RadioButton(
                selected = selected,
                onClick = null,
                interactionSource = interactionSource,
                colors = RadioButtonDefaults.colors(
                    selectedColor = MaterialTheme.colorScheme.primary,
                    unselectedColor = MaterialTheme.colorScheme.onSurfaceVariant
                )
            )
            Column(
                modifier = Modifier
                    .padding(start = 10.dp)
                    .weight(1f)
            ) {
                Text(
                    text = stringResource(id = option.label),
                    style = MaterialTheme.typography.bodyLarge
                )
                option.summary?.let {
                    Text(
                        text = it,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }
            }
        }
    }
}
