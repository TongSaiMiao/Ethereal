package me.ethereal.app.ui

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.ViewGroup
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.OnBackPressedCallback
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.lifecycle.lifecycleScope
import androidx.webkit.WebViewAssetLoader
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import me.ethereal.app.BuildConfig
import me.ethereal.app.EtherealApplication
import me.ethereal.app.ui.theme.EtherealTheme
import me.ethereal.app.ui.viewmodel.SuperUserViewModel
import me.ethereal.app.ui.webui.AppIconUtil
import me.ethereal.app.ui.webui.Insets
import me.ethereal.app.ui.webui.RootFilePathHandler
import me.ethereal.app.ui.webui.WebViewInterface
import me.ethereal.app.util.becomeRoot
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File

@SuppressLint("SetJavaScriptEnabled")
class WebUIActivity : ComponentActivity() {
    private companion object {
        const val WEB_UI_HOST = "mui.kernelsu.org"
        val MODULE_ID = Regex("^[A-Za-z][A-Za-z0-9._-]+$")
    }

    private lateinit var webViewInterface: WebViewInterface
    private var webView: WebView? = null
    private lateinit var container: FrameLayout
    private lateinit var insets: Insets
    private var insetsContinuation: CancellableContinuation<Unit>? = null
    private var isInsetsEnabled = false
    private var webCanGoBack = false
    private lateinit var fileChooserLauncher: ActivityResultLauncher<Intent>
    private var filePathCallback: ValueCallback<Array<Uri>>? = null

    override fun onCreate(savedInstanceState: Bundle?) {

        enableEdgeToEdge()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }

        super.onCreate(savedInstanceState)

        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (webCanGoBack) {
                    webView?.goBack()
                    return
                }
                isEnabled = false
                onBackPressedDispatcher.onBackPressed()
            }
        })

        setContent {
            EtherealTheme {
                Box(
                    modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background),
                    contentAlignment = Alignment.Center
                ) {
                    CircularProgressIndicator()
                }
            }
        }

        lifecycleScope.launch {
            if (SuperUserViewModel.apps.isEmpty()) {
                SuperUserViewModel().fetchAppList()
            }
            setupWebView()
        }

        fileChooserLauncher = registerForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            val uris: Array<Uri>? = when (result.resultCode) {
                RESULT_OK -> result.data?.let { data ->
                    when {
                        data.clipData != null -> {
                            Array(data.clipData!!.itemCount) { i ->
                                data.clipData!!.getItemAt(i).uri // Multiple files
                            }
                        }
                        data.data != null -> { arrayOf(data.data!!) } // Single file
                        else -> null
                    }
                }
                else -> null
            }
            filePathCallback?.onReceiveValue(uris)
            filePathCallback = null
        }
    }

    private suspend fun setupWebView() {
        val moduleId = intent.getStringExtra("id")?.trim()
        val name = intent.getStringExtra("name")
        if (moduleId == null || name == null || !MODULE_ID.matches(moduleId)) {
            finish()
            return
        }
        val roots = withContext(Dispatchers.IO) {
            if (!becomeRoot()) return@withContext null
            val modulesRoot = runCatching {
                File("/data/adb/modules").canonicalFile
            }.getOrNull() ?: return@withContext null
            val webRoot = runCatching {
                File(modulesRoot, "$moduleId/webroot").canonicalFile
            }.getOrNull() ?: return@withContext null
            if (!webRoot.toPath().startsWith(modulesRoot.toPath())) {
                return@withContext null
            }
            modulesRoot to webRoot
        }
        if (roots == null) {
            finish()
            return
        }
        val (_, webRoot) = roots
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            @Suppress("DEPRECATION")
            setTaskDescription(ActivityManager.TaskDescription("Ethereal - $name"))
        } else {
            val taskDescription = ActivityManager.TaskDescription.Builder().setLabel("Ethereal - $name").build()
            setTaskDescription(taskDescription)
        }

        val prefs = EtherealApplication.sharedPreferences
        val webDebuggingEnabled =
            BuildConfig.DEBUG && prefs.getBoolean("enable_web_debugging", false)
        WebView.setWebContentsDebuggingEnabled(webDebuggingEnabled)

        insets = Insets(0, 0, 0, 0)

        container = FrameLayout(this).apply {
            layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        }

        this.webView = WebView(this).apply {
            setBackgroundColor(Color.TRANSPARENT)
        }

        val density = resources.displayMetrics.density
        ViewCompat.setOnApplyWindowInsetsListener(container) { view, windowInsets ->
            val inset = windowInsets.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
            insets = Insets(
                top = (inset.top / density).toInt(),
                bottom = (inset.bottom / density).toInt(),
                left = (inset.left / density).toInt(),
                right = (inset.right / density).toInt()
            )
            if (isInsetsEnabled) {
                view.setPadding(0, 0, 0, 0)
            } else {
                view.setPadding(inset.left, inset.top, inset.right, inset.bottom)
            }
            insetsContinuation?.resumeWith(Result.success(Unit))
            insetsContinuation = null
            WindowInsetsCompat.CONSUMED
        }
        container.addView(this.webView)

        suspendCancellableCoroutine { cont ->
            insetsContinuation = cont
            cont.invokeOnCancellation {
                insetsContinuation = null
            }
            setContentView(container)

            if (insets != Insets(0, 0, 0, 0)) {
                cont.resumeWith(Result.success(Unit))
                insetsContinuation = null
            }
        }

        val webViewAssetLoader = WebViewAssetLoader.Builder()
            .setDomain(WEB_UI_HOST)
            .addPathHandler(
                "/",
                RootFilePathHandler(this, webRoot, { insets }, { enable -> enableInsets(enable) })
            )
            .build()

        val webViewClient = object : WebViewClient() {
            private fun response(code: Int, reason: String): WebResourceResponse {
                return WebResourceResponse(
                    "text/plain",
                    "utf-8",
                    code,
                    reason,
                    mapOf("Cache-Control" to "no-store"),
                    ByteArrayInputStream(ByteArray(0)),
                )
            }

            override fun shouldOverrideUrlLoading(
                view: WebView,
                request: WebResourceRequest,
            ): Boolean {
                if (!request.isForMainFrame) return false
                val url = request.url
                return url.scheme != "https" || url.host != WEB_UI_HOST
            }

            override fun shouldInterceptRequest(
                view: WebView,
                request: WebResourceRequest
            ): WebResourceResponse? {
                val url = request.url

                // Handle ksu://icon/[packageName] to serve app icon via WebView
                if (url.scheme.equals("ksu", ignoreCase = true) && url.host.equals("icon", ignoreCase = true)) {
                    val packageName = url.path?.substring(1)
                    if (!packageName.isNullOrEmpty()) {
                        val icon = AppIconUtil.loadAppIconSync(this@WebUIActivity, packageName, 512)
                        if (icon != null) {
                            val stream = ByteArrayOutputStream()
                            icon.compress(Bitmap.CompressFormat.PNG, 100, stream)
                            return WebResourceResponse(
                                "image/png", null, 200, "OK",
                                mapOf("Access-Control-Allow-Origin" to "*"),
                                ByteArrayInputStream(stream.toByteArray())
                            )
                        }
                    }
                }

                if (url.scheme == "data" || url.scheme == "blob") return null
                if (url.scheme != "https" || url.host != WEB_UI_HOST) {
                    return response(403, "Forbidden")
                }
                return webViewAssetLoader.shouldInterceptRequest(url)
                    ?: response(404, "Not Found")
            }

            override fun doUpdateVisitedHistory(view: WebView?, url: String?, isReload: Boolean) {
                webCanGoBack = view?.canGoBack() == true
                super.doUpdateVisitedHistory(view, url, isReload)
            }
        }

        webView?.apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            settings.blockNetworkLoads = true
            settings.javaScriptCanOpenWindowsAutomatically = false
            settings.setSupportMultipleWindows(false)
            settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            settings.safeBrowsingEnabled = true
            if (!webDebuggingEnabled) {
                webViewInterface = WebViewInterface(this@WebUIActivity, this)
                addJavascriptInterface(webViewInterface, "ksu")
            }
            setWebViewClient(webViewClient)
            webChromeClient = object : WebChromeClient() {
                override fun onShowFileChooser(
                    webView: WebView?,
                    filePathCallback: ValueCallback<Array<Uri>>?,
                    fileChooserParams: FileChooserParams?
                ): Boolean {
                    this@WebUIActivity.filePathCallback?.onReceiveValue(null)
                    this@WebUIActivity.filePathCallback = filePathCallback
                    val intent = fileChooserParams?.createIntent() ?: Intent(Intent.ACTION_GET_CONTENT).apply { type = "*/*" }
                    if (fileChooserParams?.mode == FileChooserParams.MODE_OPEN_MULTIPLE) {
                        intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
                    }
                    try {
                        fileChooserLauncher.launch(intent)
                    } catch (_: ActivityNotFoundException) {
                        filePathCallback?.onReceiveValue(null)
                        this@WebUIActivity.filePathCallback = null
                        return false
                    }
                    return true
                }
            }
            loadUrl("https://mui.kernelsu.org/index.html")
        }
    }

    fun enableInsets(enable: Boolean = true) {
        runOnUiThread {
            if (isInsetsEnabled != enable) {
                isInsetsEnabled = enable
                ViewCompat.requestApplyInsets(container)
            }
        }
    }

    override fun onDestroy() {
        if (::webViewInterface.isInitialized) webViewInterface.close()
        webView?.apply {
            removeJavascriptInterface("ksu")
            stopLoading()
            destroy()
        }
        webView = null
        super.onDestroy()
    }
}
