package me.ethereal.app.ui.screen

import android.os.Environment
import android.os.Handler
import android.os.Looper
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Save
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.key
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.ramcosta.composedestinations.annotation.Destination
import com.ramcosta.composedestinations.annotation.RootGraph
import com.ramcosta.composedestinations.navigation.DestinationsNavigator
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import me.ethereal.app.R
import me.ethereal.app.ui.component.KeyEventBlocker
import me.ethereal.app.util.FlashIt
import me.ethereal.app.util.reboot
import me.ethereal.app.util.runFlash
import me.ethereal.app.util.ui.LocalSnackbarHost
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

enum class FlashingStatus {
    FLASHING, SUCCESS, FAILED
}

@OptIn(ExperimentalMaterial3Api::class)
@Destination<RootGraph>
@Composable
fun FlashScreen(navigator: DestinationsNavigator, flashIt: FlashIt) {
    val context = LocalContext.current
    var text by rememberSaveable { mutableStateOf("") }
    val logContent = remember { StringBuilder() }
    var showFloatAction by rememberSaveable { mutableStateOf(false) }
    var hasExecuted by rememberSaveable { mutableStateOf(false) }
    var status by remember { mutableStateOf(FlashingStatus.FLASHING) }
    val snackBarHost = LocalSnackbarHost.current
    val scope = rememberCoroutineScope()
    val scrollState = rememberScrollState()
    val errorCodeString = stringResource(R.string.error_code)
    val checkLogString = stringResource(R.string.check_log)
    val logSavedString = stringResource(R.string.log_saved)

    LaunchedEffect(flashIt) {
        if (hasExecuted || text.isNotEmpty()) return@LaunchedEffect
        hasExecuted = true
        val main = Handler(Looper.getMainLooper())
        val appendLine: (String) -> Unit = { line ->
            logContent.append(line).append('\n')
            main.post { text += "$line\n" }
        }
        runCatching {
            withContext(Dispatchers.IO) {
                main.post { status = FlashingStatus.FLASHING }
                runFlash(
                    flashIt,
                    onFinish = { showReboot, code ->
                        main.post {
                            if (code != 0) {
                                text += "$errorCodeString $code.\n$checkLogString\n"
                                status = FlashingStatus.FAILED
                            } else {
                                status = FlashingStatus.SUCCESS
                            }
                            if (showReboot) {
                                text += "\n\n\n"
                                showFloatAction = true
                            }
                        }
                    },
                    onStdout = appendLine,
                    onStderr = appendLine,
                )
            }
        }.onFailure { t ->
            text += "FAILED: ${t.message}\n"
            status = FlashingStatus.FAILED
        }
    }

    val onBack: () -> Unit = {
        if (status != FlashingStatus.FLASHING) {
            navigator.popBackStack()
        }
    }
    BackHandler(enabled = true) { onBack() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.install)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = null)
                    }
                },
                actions = {
                    IconButton(onClick = {
                        scope.launch {
                            val format = SimpleDateFormat("yyyy-MM-dd-HH-mm-ss", Locale.getDefault())
                            val file = File(
                                Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
                                "Ethereal_install_log_${format.format(Date())}.log"
                            )
                            file.writeText(logContent.toString())
                            snackBarHost.showSnackbar(logSavedString + " ${file.absolutePath}")
                        }
                    }) {
                        Icon(Icons.Filled.Save, contentDescription = null)
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = Color.Transparent)
            )
        },
        floatingActionButton = {
            if (showFloatAction) {
                ExtendedFloatingActionButton(
                    onClick = {
                        scope.launch {
                            withContext(Dispatchers.IO) { reboot() }
                        }
                    },
                    icon = { Icon(Icons.Filled.Refresh, contentDescription = null) },
                    text = { Text(stringResource(R.string.reboot)) },
                    containerColor = MaterialTheme.colorScheme.secondaryContainer,
                    contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                    expanded = true
                )
            }
        },
        snackbarHost = { SnackbarHost(snackBarHost) },
        containerColor = Color.Transparent
    ) { innerPadding ->
        KeyEventBlocker { it.key == Key.VolumeDown || it.key == Key.VolumeUp }
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
        ) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .verticalScroll(scrollState)
            ) {
                LaunchedEffect(text) {
                    scrollState.animateScrollTo(scrollState.maxValue)
                }
                Text(
                    modifier = Modifier.padding(16.dp),
                    text = text,
                    style = MaterialTheme.typography.bodyMedium,
                    fontFamily = FontFamily.Monospace,
                    color = MaterialTheme.colorScheme.onSurface
                )
            }
        }
    }
}
