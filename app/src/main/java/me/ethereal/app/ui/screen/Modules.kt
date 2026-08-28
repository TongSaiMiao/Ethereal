package me.ethereal.app.ui.screen

import android.app.Activity.RESULT_OK
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
import android.util.Patterns
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBarScrollBehavior
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.net.toUri
import androidx.lifecycle.viewmodel.compose.viewModel
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import com.ramcosta.composedestinations.generated.destinations.ExecuteModuleActionScreenDestination
import com.ramcosta.composedestinations.generated.destinations.InstallScreenDestination
import com.ramcosta.composedestinations.navigation.DestinationsNavigator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import me.ethereal.app.EtherealApplication
import me.ethereal.app.R
import me.ethereal.app.etherealApp
import me.ethereal.app.ui.WebUIActivity
import me.ethereal.app.ui.component.ConfirmResult
import me.ethereal.app.ui.component.ModuleRemoveButton
import me.ethereal.app.ui.component.ModuleStateIndicator
import me.ethereal.app.ui.component.ModuleUndoRemoveButton
import me.ethereal.app.ui.component.ModuleUpdateButton
import me.ethereal.app.ui.component.SearchAppBar
import me.ethereal.app.ui.component.WarningCard
import me.ethereal.app.ui.component.pinnedScrollBehavior
import me.ethereal.app.ui.component.rememberConfirmDialog
import me.ethereal.app.ui.component.rememberLoadingDialog
import me.ethereal.app.ui.viewmodel.ModuleViewModel
import me.ethereal.app.util.DownloadListener
import me.ethereal.app.util.download
import me.ethereal.app.util.hasMagisk
import me.ethereal.app.util.reboot
import me.ethereal.app.util.toggleModule
import me.ethereal.app.util.ui.LocalSnackbarHost
import me.ethereal.app.util.undoRemoveModule
import me.ethereal.app.util.uninstallModule
import okhttp3.Request
import java.io.File

@OptIn(ExperimentalMaterial3Api::class)
@Destination<RootGraph>
@Composable
fun ModuleScreen(navigator: DestinationsNavigator) {
    val snackBarHost = LocalSnackbarHost.current
    val context = LocalContext.current

    val state by EtherealApplication.serviceStateLiveData.observeAsState(EtherealApplication.State.UNKNOWN_STATE)
    if (state != EtherealApplication.State.SERVICE_INSTALLED && state != EtherealApplication.State.SERVICE_NEED_UPDATE) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(12.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Row {
                Text(
                    text = stringResource(id = R.string.module_not_installed),
                    style = MaterialTheme.typography.titleMedium
                )
            }
        }
        return
    }

    val viewModel = viewModel<ModuleViewModel>()

    LaunchedEffect(Unit) {
        if (viewModel.moduleList.isEmpty() || viewModel.isNeedRefresh) {
            viewModel.fetchModuleList()
        }
    }
    val webUILauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult()
    ) { viewModel.fetchModuleList() }
    val scrollBehavior = pinnedScrollBehavior()

    val magiskPresent by produceState<Boolean?>(initialValue = null) {
        value = withContext(Dispatchers.IO) { hasMagisk() }
    }
    val hideInstallButton = magiskPresent != false

    val moduleListState = rememberLazyListState()

    Scaffold(
        topBar = {
            SearchAppBar(
                searchText = viewModel.search,
                onSearchTextChange = { viewModel.search = it },
                searchBarPlaceHolderText = stringResource(R.string.search_modules)
            )
        },
        floatingActionButton = {
            if (hideInstallButton) return@Scaffold
            val selectZipLauncher = rememberLauncherForActivityResult(
                contract = ActivityResultContracts.StartActivityForResult()
            ) {
                if (it.resultCode != RESULT_OK) {
                    return@rememberLauncherForActivityResult
                }
                val data = it.data ?: return@rememberLauncherForActivityResult
                val uri = data.data ?: return@rememberLauncherForActivityResult

                Log.i("ModuleScreen", "select zip result: $uri")

                navigator.navigate(InstallScreenDestination(uri, MODULE_TYPE.MODULE))

                viewModel.markNeedRefresh()
            }

            FloatingActionButton(
                contentColor = MaterialTheme.colorScheme.onPrimary,
                containerColor = MaterialTheme.colorScheme.primary,
                onClick = {
                    // select the zip file to install
                    val intent = Intent(Intent.ACTION_GET_CONTENT)
                    intent.type = "application/zip"
                    selectZipLauncher.launch(intent)
                }) {
                Icon(
                    painter = painterResource(id = R.drawable.package_import),
                    contentDescription = null
                )
            }
        },
        snackbarHost = { SnackbarHost(snackBarHost) }
    ) { innerPadding ->
        when {
            magiskPresent == null -> {
                Box(
                    modifier = Modifier.fillMaxSize(),
                    contentAlignment = Alignment.Center,
                ) {
                    CircularProgressIndicator()
                }
            }

            magiskPresent == true -> {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .padding(24.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text(
                        stringResource(R.string.module_magisk_conflict),
                        textAlign = TextAlign.Center,
                    )
                }
            }

            else -> {
                ModuleList(
                    navigator = navigator,
                    viewModel = viewModel,
                    modules = viewModel.moduleList,
                    modifier = Modifier
                        .padding(innerPadding)
                        .fillMaxSize(),
                    state = moduleListState,
                    onInstallModule = {
                        navigator.navigate(InstallScreenDestination(it, MODULE_TYPE.MODULE))
                    },
                    onClickModule = { id, name, hasWebUi ->
                        if (hasWebUi) {
                            webUILauncher.launch(
                                Intent(
                                    context, WebUIActivity::class.java
                                ).setData("ethereal://webui/$id".toUri()).putExtra("id", id)
                                    .putExtra("name", name)
                            )
                        }
                    },
                    snackBarHost = snackBarHost,
                    scrollBehavior = scrollBehavior
                )
            }
        }
    }
}

private fun getMetaModuleWarningText(
    viewModel: ModuleViewModel,
    context: Context
) : String? {
    val needsMountModule = viewModel.moduleList.any { module ->
        val moduleDir = "/data/adb/modules/${module.id}"

        // Module requires mounting if it has a system dir and no skip_mount file
        val hasSystem = File("$moduleDir/system").isDirectory
        val isSkipped = File("$moduleDir/skip_mount").isFile

        hasSystem && !isSkipped
    }

    if (!needsMountModule) return null

    val metaDir = "/data/adb/metamodule"
    val metaProp = File("$metaDir/module.prop").isFile
    val metaRemoved = File("$metaDir/remove").isFile
    val metaDisabled = File("$metaDir/disable").isFile

    return when {
        !metaProp -> context.getString(R.string.no_meta_module_installed)
        metaRemoved -> context.getString(R.string.meta_module_removed)
        metaDisabled -> context.getString(R.string.meta_module_disabled)
        else -> null
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MetaModuleWarningCard(
    text: String
) {
    var show by rememberSaveable { mutableStateOf(true) }

    AnimatedVisibility(
        visible = show,
        enter = fadeIn() + expandVertically(),
        exit = fadeOut() + shrinkVertically()
    ) {
        WarningCard(
            message = text,
            onClose = {
                show = false
            }
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ModuleList(
    navigator: DestinationsNavigator,
    viewModel: ModuleViewModel,
    modules: List<ModuleViewModel.ModuleInfo>,
    modifier: Modifier = Modifier,
    state: LazyListState,
    onInstallModule: (Uri) -> Unit,
    onClickModule: (id: String, name: String, hasWebUi: Boolean) -> Unit,
    snackBarHost: SnackbarHostState,
    scrollBehavior: SearchBarScrollBehavior
) {
    val failedEnable = stringResource(R.string.module_failed_to_enable)
    val failedDisable = stringResource(R.string.module_failed_to_disable)
    val failedUninstall = stringResource(R.string.module_uninstall_failed)
    val failedUndoUninstall = stringResource(R.string.module_module_undo_uninstall_failed)
    val successUninstall = stringResource(R.string.module_uninstall_success)
    val successUndoUninstall = stringResource(R.string.module_module_undo_uninstall_success)
    val reboot = stringResource(id = R.string.reboot)
    val rebootToApply = stringResource(id = R.string.module_reboot_to_apply)
    val moduleStr = stringResource(id = R.string.modules)
    val uninstall = stringResource(id = R.string.module_remove)
    val cancel = stringResource(id = android.R.string.cancel)
    val moduleUninstallConfirm = stringResource(id = R.string.module_uninstall_confirm)
    val metaModuleUninstallConfirm = stringResource(R.string.metamodule_uninstall_confirm)
    val updateText = stringResource(R.string.module_update)
    val changelogText = stringResource(R.string.module_changelog)
    val downloadingText = stringResource(R.string.module_downloading)
    val startDownloadingText = stringResource(R.string.module_start_downloading)

    val context = LocalContext.current
    val loadingDialog = rememberLoadingDialog()
    val confirmDialog = rememberConfirmDialog()

    suspend fun onModuleUpdate(
        module: ModuleViewModel.ModuleInfo,
        changelogUrl: String,
        downloadUrl: String,
        fileName: String
    ) {
        val changelog = loadingDialog.withLoading {
            withContext(Dispatchers.IO) {
                runCatching {
                    if (Patterns.WEB_URL.matcher(changelogUrl).matches()) {
                        etherealApp.okhttpClient.newCall(
                                Request.Builder().url(changelogUrl).build()
                            ).execute().use { it.body?.string().orEmpty() }
                    } else {
                        changelogUrl
                    }
                }.getOrDefault("")
            }
        }


        if (changelog.isNotEmpty()) {
            // changelog is not empty, show it and wait for confirm
            val confirmResult = confirmDialog.awaitConfirm(
                changelogText,
                content = changelog,
                markdown = true,
                confirm = updateText,
            )

            if (confirmResult != ConfirmResult.Confirmed) {
                return
            }
        }

        withContext(Dispatchers.Main) {
            Toast.makeText(
                context, startDownloadingText.format(module.name), Toast.LENGTH_SHORT
            ).show()
        }

        val downloading = downloadingText.format(module.name)
        withContext(Dispatchers.IO) {
            download(
                context,
                downloadUrl,
                fileName,
                downloading,
                onDownloaded = onInstallModule,
                onDownloading = {
                    launch(Dispatchers.Main) {
                        Toast.makeText(context, downloading, Toast.LENGTH_SHORT).show()
                    }
                })
        }
    }

    suspend fun onModuleUninstall(module: ModuleViewModel.ModuleInfo) {
        val formatter = if (module.metamodule) metaModuleUninstallConfirm else moduleUninstallConfirm
        val confirmResult = confirmDialog.awaitConfirm(
            moduleStr,
            content = formatter.format(module.name),
            confirm = uninstall,
            dismiss = cancel
        )
        if (confirmResult != ConfirmResult.Confirmed) {
            return
        }

        val success = loadingDialog.withLoading {
            withContext(Dispatchers.IO) {
                uninstallModule(module.id)
            }
        }

        if (success) {
            viewModel.fetchModuleList()
        }
        val message = if (success) {
            successUninstall.format(module.name)
        } else {
            failedUninstall.format(module.name)
        }
        val actionLabel = if (success) {
            reboot
        } else {
            null
        }
        val result = snackBarHost.showSnackbar(
            message = message, actionLabel = actionLabel, duration = SnackbarDuration.Long
        )
        if (result == SnackbarResult.ActionPerformed) {
            withContext(Dispatchers.IO) { reboot() }
        }
    }

    suspend fun onUndoModuleUninstall(module: ModuleViewModel.ModuleInfo) {
        val success = loadingDialog.withLoading {
            withContext(Dispatchers.IO) {
                undoRemoveModule(module.id)
            }
        }

        if (success) {
            viewModel.fetchModuleList()
        }
        val message = if (success) {
            successUndoUninstall.format(module.name)
        } else {
            failedUndoUninstall.format(module.name)
        }
        val actionLabel = if (success) {
            reboot
        } else {
            null
        }
        val result = snackBarHost.showSnackbar(
            message = message, actionLabel = actionLabel, duration = SnackbarDuration.Long
        )
        if (result == SnackbarResult.ActionPerformed) {
            withContext(Dispatchers.IO) { reboot() }
        }
    }

    PullToRefreshBox(
        modifier = modifier,
        onRefresh = { viewModel.fetchModuleList() },
        isRefreshing = viewModel.isRefreshing
    ) {
        val metaModuleWarningText by produceState<String?>(initialValue = null, viewModel.moduleList) {
            value = withContext(Dispatchers.IO) {
                getMetaModuleWarningText(viewModel, context)
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxSize().nestedScroll(scrollBehavior.nestedScrollConnection),
            state = state,
            verticalArrangement = Arrangement.spacedBy(16.dp),
            contentPadding = remember {
                PaddingValues(
                    start = 16.dp,
                    top = 11.dp, // spacedBy - TopBar padding
                    end = 16.dp,
                    bottom = 16.dp + 16.dp + 56.dp /*  Scaffold Fab Spacing + Fab container height */
                )
            },
        ) {
            if (metaModuleWarningText != null) {
                item {
                    MetaModuleWarningCard(metaModuleWarningText!!)
                }
            }

            when {
                modules.isEmpty() -> {
                    item {
                        Box(
                            modifier = Modifier.fillParentMaxSize(),
                            contentAlignment = Alignment.Center
                        ) {
                            Text(
                                stringResource(R.string.module_empty), textAlign = TextAlign.Center
                            )
                        }
                    }
                }

                else -> {
                    items(modules) { module ->
                        var isChecked by rememberSaveable(module) { mutableStateOf(module.enabled) }
                        val scope = rememberCoroutineScope()
                        val updateInfo = module.updateInfo

                        ModuleItem(
                            navigator,
                            module,
                            isChecked,
                            updateInfo?.zipUrl ?: "",
                            onUninstall = {
                                scope.launch { onModuleUninstall(module) }
                            },
                            onUndoUninstall = {
                                scope.launch { onUndoModuleUninstall(module) }
                            },
                            onCheckChanged = {
                                scope.launch {
                                    val success = loadingDialog.withLoading {
                                        withContext(Dispatchers.IO) {
                                            toggleModule(module.id, !isChecked)
                                        }
                                    }
                                    if (success) {
                                        isChecked = it
                                        viewModel.fetchModuleList()

                                        val result = snackBarHost.showSnackbar(
                                            message = rebootToApply,
                                            actionLabel = reboot,
                                            duration = SnackbarDuration.Long
                                        )
                                        if (result == SnackbarResult.ActionPerformed) {
                                            withContext(Dispatchers.IO) { reboot() }
                                        }
                                    } else {
                                        val message = if (isChecked) failedDisable else failedEnable
                                        snackBarHost.showSnackbar(message.format(module.name))
                                    }
                                }
                            },
                            onUpdate = {
                                scope.launch {
                                    updateInfo?.let { info ->
                                        onModuleUpdate(
                                            module,
                                            info.changelog,
                                            info.zipUrl,
                                            "${module.name}-${info.version}.zip"
                                        )
                                    }
                                }
                            },
                            onClick = {
                                onClickModule(it.id, it.name, it.hasWebUi)
                            })
                        // fix last item shadow incomplete in LazyColumn
                        Spacer(Modifier.height(1.dp))
                    }
                }
            }
        }

        DownloadListener(context, onInstallModule)
    }
}

@Composable
private fun ModuleItem(
    navigator: DestinationsNavigator,
    module: ModuleViewModel.ModuleInfo,
    isChecked: Boolean,
    updateUrl: String,
    onUninstall: (ModuleViewModel.ModuleInfo) -> Unit,
    onUndoUninstall: (ModuleViewModel.ModuleInfo) -> Unit,
    onCheckChanged: (Boolean) -> Unit,
    onUpdate: (ModuleViewModel.ModuleInfo) -> Unit,
    onClick: (ModuleViewModel.ModuleInfo) -> Unit,
    modifier: Modifier = Modifier,
    alpha: Float = 1f,
) {
    val decoration = if (!module.remove) TextDecoration.None else TextDecoration.LineThrough
    val moduleAuthor = stringResource(id = R.string.module_author)
    val viewModel = viewModel<ModuleViewModel>()
    Surface(
        modifier = modifier,
        color = MaterialTheme.colorScheme.surface,
        tonalElevation = 1.dp,
        shape = RoundedCornerShape(20.dp)
    ) {

        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { onClick(module) },
            contentAlignment = Alignment.Center
        ) {
            Column(
                modifier = Modifier.fillMaxWidth()
            ) {
                Row(
                    modifier = Modifier.padding(all = 16.dp),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Column(
                        modifier = Modifier
                            .alpha(alpha = alpha)
                            .weight(1f),
                        verticalArrangement = Arrangement.spacedBy(2.dp)
                    ) {
                        SubcomposeLayout { constraints ->
                            val spacingPx = 6.dp.roundToPx()
                            var nameTextLayout: TextLayoutResult? = null
                            val metaPlaceable = if (module.metamodule) {
                                subcompose("meta") {
                                    Surface(
                                        shape = RoundedCornerShape(4.dp),
                                        color = MaterialTheme.colorScheme.tertiary
                                    ) {
                                        Text(
                                            text = "META",
                                            style = MaterialTheme.typography.labelSmall.copy(
                                                fontSize = 10.sp
                                            ),
                                            modifier = Modifier.padding(horizontal = 4.dp, vertical = 1.dp),
                                            color = MaterialTheme.colorScheme.onTertiary,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis
                                        )
                                    }
                                }.first().measure(Constraints(0, constraints.maxWidth, 0, constraints.maxHeight))
                            } else null

                            val reserved = (metaPlaceable?.width ?: 0) + if (metaPlaceable != null) spacingPx else 0
                            val nameMax = (constraints.maxWidth - reserved).coerceAtLeast(0)
                            val namePlaceable = subcompose("name") {
                                Text(
                                    text = module.name,
                                    style = MaterialTheme.typography.titleSmall.copy(fontWeight = FontWeight.Bold),
                                    maxLines = 2,
                                    textDecoration = decoration,
                                    overflow = TextOverflow.Ellipsis,
                                    onTextLayout = { nameTextLayout = it }
                                )
                            }.first().measure(Constraints(constraints.minWidth, nameMax, constraints.minHeight, constraints.maxHeight))

                            val width = (namePlaceable.width + reserved).coerceIn(constraints.minWidth, constraints.maxWidth)
                            val height = maxOf(namePlaceable.height, metaPlaceable?.height ?: 0)

                            layout(width, height) {
                                namePlaceable.placeRelative(0, 0)
                                val endX = nameTextLayout?.let { layoutRes ->
                                    val last = (layoutRes.lineCount - 1).coerceAtLeast(0)
                                    layoutRes.getLineRight(last).toInt()
                                } ?: namePlaceable.width
                                metaPlaceable?.placeRelative(endX + spacingPx, (height - (metaPlaceable.height)) / 2)
                            }
                        }

                        Text(
                            text = "${module.version}, $moduleAuthor ${module.author}",
                            style = MaterialTheme.typography.bodySmall,
                            textDecoration = decoration,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                    }

                    Switch(
                        enabled = !module.update,
                        checked = isChecked,
                        onCheckedChange = onCheckChanged
                    )
                }

                Text(
                    modifier = Modifier
                        .alpha(alpha = alpha)
                        .padding(horizontal = 16.dp),
                    text = module.description,
                    style = MaterialTheme.typography.bodySmall,
                    textDecoration = decoration,
                    color = MaterialTheme.colorScheme.outline
                )

                HorizontalDivider(
                    thickness = 1.5.dp,
                    color = MaterialTheme.colorScheme.surface,
                    modifier = Modifier.padding(top = 8.dp)
                )

                Row(
                    modifier = Modifier
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                        .fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    if (updateUrl.isNotEmpty()) {
                        ModuleUpdateButton(onClick = { onUpdate(module) })

                        Spacer(modifier = Modifier.width(12.dp))
                    }

                    if (module.hasWebUi) {
                        FilledTonalButton(
                            onClick = { onClick(module) },
                            enabled = true,
                            contentPadding = PaddingValues(12.dp)
                        ) {
                            Icon(
                                modifier = Modifier.size(20.dp),
                                painter = painterResource(id = R.drawable.webui),
                                contentDescription = stringResource(id = R.string.module_webui_open)
                            )
                        }

                        Spacer(modifier = Modifier.width(12.dp))
                    }

                    if (module.hasActionScript) {
                        FilledTonalButton(
                            onClick = {
                                navigator.navigate(ExecuteModuleActionScreenDestination(module.id))
                                viewModel.markNeedRefresh()
                            }, enabled = true, contentPadding = PaddingValues(12.dp)
                        ) {
                            Icon(
                                modifier = Modifier.size(20.dp),
                                painter = painterResource(id = R.drawable.play_circle),
                                contentDescription = stringResource(id = R.string.module_action)
                            )
                        }

                        Spacer(modifier = Modifier.width(12.dp))
                    }

                    Spacer(modifier = Modifier.weight(1f))

                    if (!module.remove) {
                        ModuleRemoveButton(
                            enabled = true,
                            onClick = { onUninstall(module) }
                        )
                    } else {
                        ModuleUndoRemoveButton(
                            enabled = true,
                            onClick = { onUndoUninstall(module) }
                        )
                    }
                }
            }

            if (module.remove) {
                ModuleStateIndicator(R.drawable.trash)
            }
            if (module.update) {
                ModuleStateIndicator(R.drawable.device_mobile_down)
            }
        }
    }
}
