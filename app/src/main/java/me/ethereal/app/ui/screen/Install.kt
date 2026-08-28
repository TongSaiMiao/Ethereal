package me.ethereal.app.ui.screen

import android.content.Context
import android.net.Uri
import android.os.Environment
import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Save
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.ui.graphics.Color
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.key
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.dropUnlessResumed
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import com.ramcosta.composedestinations.navigation.DestinationsNavigator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.yield
import me.ethereal.app.R
import me.ethereal.app.ui.component.KeyEventBlocker
import me.ethereal.app.ui.component.rememberCustomDialog
import me.ethereal.app.util.InstallLogBuffer
import me.ethereal.app.util.hasMetaModule
import me.ethereal.app.util.installModule
import me.ethereal.app.util.reboot
import me.ethereal.app.util.ui.LocalSnackbarHost
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.Properties
import java.util.zip.ZipEntry
import java.util.zip.ZipInputStream

enum class MODULE_TYPE {
    MODULE
}

@Composable
@Destination<RootGraph>
fun InstallScreen(navigator: DestinationsNavigator, uri: Uri, type: MODULE_TYPE) {
    var text by rememberSaveable { mutableStateOf("") }
    val installLog = remember { InstallLogBuffer() }
    val logSignal = remember { Channel<Unit>(Channel.CONFLATED) }
    var showFloatAction by rememberSaveable { mutableStateOf(false) }

    fun enqueueLog(line: String) {
        installLog.offer(line)
        logSignal.trySend(Unit)
    }
    val metaModuleAlertDialog = rememberCustomDialog { dismiss: () -> Unit ->
        val uriHandler = LocalUriHandler.current
        AlertDialog(
            onDismissRequest = { dismiss() },
            icon = {
                Icon(Icons.Outlined.Info, contentDescription = null)
            },
            title = {
                Row(modifier = Modifier
                    .fillMaxWidth(),
                    horizontalArrangement = Arrangement.Center
                ) {
                    Text(text = stringResource(R.string.warning_of_meta_module_title))
                }
            },
            text = {
                Text(text = stringResource(R.string.warning_of_meta_module_summary))
            },
            confirmButton = {
                FilledTonalButton(onClick = { dismiss() }) {
                    Text(text = stringResource(id = android.R.string.ok))
                }
            },
            dismissButton = {
                OutlinedButton(onClick = {
                    uriHandler.openUri("/meta-module.html")
                }) {
                    Text(text = stringResource(id = R.string.learn_more))
                }
            },
        )
    }

    val context = LocalContext.current
    val snackBarHost = LocalSnackbarHost.current
    val scope = rememberCoroutineScope()
    val scrollState = rememberScrollState()

    LaunchedEffect(logSignal) {
        for (ignored in logSignal) {
            while (true) {
                val snapshot = installLog.drain() ?: break
                text = snapshot.text
                if (!snapshot.hasMore) break
                yield()
            }
        }
    }

    LaunchedEffect(Unit) {
        if (text.isNotEmpty()) {
            return@LaunchedEffect
        }
        withContext(Dispatchers.IO) {
            installModule(uri, type, onFinish = { success ->
                if (!success) return@installModule

                scope.launch {
                    text = installLog.flush().text
                    showFloatAction = true

                    val needsMetaModule = withContext(Dispatchers.IO) {
                        if (hasMetaModule()) return@withContext false
                        val moduleId = getModuleIdFromUri(context, uri)
                        val mountOldDirectory = File("/data/adb/modules/$moduleId/system")
                        val mountNewDirectory =
                            File("/data/adb/modules_update/$moduleId/system")
                        mountNewDirectory.isDirectory || mountOldDirectory.isDirectory
                    }
                    if (needsMetaModule) metaModuleAlertDialog.show()
                }

            }, onStdout = { line ->
                enqueueLog(line)
            }, onStderr = { line ->
                enqueueLog(line)
            })
        }
    }

    Scaffold(
        containerColor = Color.Transparent,
        topBar = {
        TopBar(onBack = dropUnlessResumed {
            navigator.popBackStack()
        }, onSave = {
            val flushed = installLog.flush()
            if (flushed.text.isNotEmpty()) text = flushed.text
            val content = flushed.text.ifEmpty { text }
            scope.launch {
                val savedPath = withContext(Dispatchers.IO) {
                    val format = SimpleDateFormat("yyyy-MM-dd-HH-mm-ss", Locale.getDefault())
                    val date = format.format(Date())
                    val file = File(
                        Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
                        "Ethereal_install_${type}_log_${date}.log"
                    )
                    file.writeText(content)
                    file.absolutePath
                }
                snackBarHost.showSnackbar("Log saved to $savedPath")
            }
        })
    }, floatingActionButton = {
        if (showFloatAction) {
            val reboot = stringResource(id = R.string.reboot)
            ExtendedFloatingActionButton(
                onClick = {
                    scope.launch {
                        withContext(Dispatchers.IO) {
                            reboot()
                        }
                    }
                },
                icon = { Icon(Icons.Filled.Refresh, reboot) },
                text = { Text(text = reboot) },
            )
        }

    }, snackbarHost = { SnackbarHost(snackBarHost) }) { innerPadding ->
        KeyEventBlocker {
            it.key == Key.VolumeDown || it.key == Key.VolumeUp
        }
        Column(
            modifier = Modifier
                .fillMaxSize(1f)
                .padding(innerPadding)
                .verticalScroll(scrollState),
        ) {
            LaunchedEffect(text) {
                scrollState.animateScrollTo(scrollState.maxValue)
            }
            Text(
                modifier = Modifier.padding(8.dp),
                text = text,
                fontSize = MaterialTheme.typography.bodySmall.fontSize,
                fontFamily = FontFamily.Monospace,
                lineHeight = MaterialTheme.typography.bodySmall.lineHeight,
            )
        }
    }
}

fun isUriAccessible(context: Context, uri: Uri): Boolean {
    if (uri == Uri.EMPTY) return false

    return try {
        context.contentResolver.openInputStream(uri)?.use {} != null
    } catch (e: Exception) {
        Log.e("ModuleInstall", "URI is inaccessible: $uri", e)
        false
    }
}


fun extractModuleId(context: Context, uri: Uri): String? {
    if (uri == Uri.EMPTY) return null

    context.contentResolver.openInputStream(uri)?.use { inputStream ->
        ZipInputStream(inputStream).use { zip ->
            var entry: ZipEntry?

            while (zip.nextEntry.also { entry = it } != null) {
                if (entry?.name == "module.prop") {
                    val prop = Properties()
                    prop.load(zip)
                    return prop.getProperty("id")
                }
            }
        }
    }

    return null
}

suspend fun getModuleIdFromUri(context: Context, uri: Uri): String? {
    return withContext(Dispatchers.IO) {
        try {
            if (uri == Uri.EMPTY) {
                return@withContext null
            }
            if (!isUriAccessible(context, uri)) {
                return@withContext null
            }
            extractModuleId(context, uri)
        } catch (_: Exception) {
            null
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun TopBar(onBack: () -> Unit = {}, onSave: () -> Unit = {}) {
    TopAppBar(
        title = { Text(stringResource(R.string.module_install)) },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = Color.Transparent),
        navigationIcon = {
        IconButton(
            onClick = onBack
        ) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null) }
    }, actions = {
        IconButton(onClick = onSave) {
            Icon(
                imageVector = Icons.Filled.Save, contentDescription = "Localized description"
            )
        }
    })
}

@Preview
@Composable
fun InstallPreview() {
//    InstallScreen(DestinationsNavigator(), uri = Uri.EMPTY)
}
