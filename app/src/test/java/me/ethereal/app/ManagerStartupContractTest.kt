package me.ethereal.app

import java.io.File
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ManagerStartupContractTest {
    private fun source(path: String): String = sequenceOf(File(path), File("app", path))
        .first { it.isFile }
        .readText()

    @Test
    fun applicationOnCreateStaysUnprivileged() {
        val application = source("src/main/java/me/ethereal/app/EtherealApplication.kt")
        val onCreate = application
            .substringAfter("override fun onCreate()")
            .substringBefore("fun getBackupWarningState()")

        listOf(
            "Natives.",
            "becomeRoot(",
            "ProcessBuilder(",
            "System.loadLibrary(",
            "Shell.build",
            "exitProcess(",
        ).forEach { forbidden ->
            assertFalse(onCreate.contains(forbidden), "Application.onCreate contains $forbidden")
        }
    }

    @Test
    fun firstFramePrecedesKernelAndNativeProbe() {
        val activity = source("src/main/java/me/ethereal/app/ui/MainActivity.kt")
        val probe = activity
            .substringAfter("LaunchedEffect(Unit)")
            .substringBefore("val portrait")

        val firstFrame = probe.indexOf("withFrameNanos")
        val secondFrame = probe.indexOf("withFrameNanos", firstFrame + 1)
        val refresh = probe.indexOf("EtherealApplication.refreshState()")
        assertTrue(firstFrame >= 0 && secondFrame > firstFrame && refresh > secondFrame)
    }

    @Test
    fun nativeProbeRequiresJavaModuleDetection() {
        val application = source("src/main/java/me/ethereal/app/EtherealApplication.kt")
        val refresh = application
            .substringAfter("fun refreshState()")
            .substringBefore("fun deployBundledSu()")

        val javaProbe = refresh.indexOf("kernelModulePresent()")
        val absentBranch = refresh.indexOf("if (!present)")
        val absentReturn = refresh.indexOf("return", absentBranch)
        val nativeProbe = refresh.indexOf("Natives.ready()")
        assertTrue(
            javaProbe >= 0 && absentBranch > javaProbe &&
                absentReturn > absentBranch && nativeProbe > absentReturn
        )
    }

    @Test
    fun managerAccessIsPublishedOnlyAfterNativeAuthentication() {
        val application = source("src/main/java/me/ethereal/app/EtherealApplication.kt")
        val refresh = application
            .substringAfter("fun refreshState()")
            .substringBefore("fun deployBundledSu()")

        val nativeProbe = refresh.indexOf("Natives.ready()")
        val failureBranch = refresh.indexOf("if (!ready)")
        val authenticated = refresh.indexOf(
            "publishManagerAccessState(ManagerAccessState.AUTHENTICATED)"
        )
        assertTrue(nativeProbe >= 0)
        assertTrue(failureBranch > nativeProbe)
        assertTrue(authenticated > failureBranch)
    }

    @Test
    fun authenticationFailureKeepsKernelUiAliveAndMarksServiceAbsent() {
        val application = source("src/main/java/me/ethereal/app/EtherealApplication.kt")
        val failure = application
            .substringAfter("if (!ready)")
            .substringBefore("return@thread")

        assertTrue(
            failure.contains("publishManagerAccessState(ManagerAccessState.UNAVAILABLE)")
        )
        assertTrue(failure.contains("publishServiceState(State.SERVICE_NOT_INSTALLED)"))
        assertFalse(failure.contains("exitProcess("))
        assertFalse(failure.contains("_kernelStateLiveData"))
    }

    @Test
    fun homeShowsKernelAndServiceAsNotInstalledBeforeProbe() {
        val home = source("src/main/java/me/ethereal/app/ui/screen/Home.kt")
        val absent = home
            .substringAfter("private fun KStatusCard")
            .substringAfter("else -> {")
            .substringBefore("private fun BackupWarningCard")

        assertTrue(absent.contains("R.string.module_service"))
        assertTrue(absent.windowed("R.string.home_not_installed".length)
            .count { it == "R.string.home_not_installed" } >= 2)
    }

    @Test
    fun jniLibraryLoadsOnlyThroughExplicitProbe() {
        val natives = source("src/main/java/me/ethereal/app/Natives.kt")
        val ensureLoaded = natives.indexOf("fun ensureLoaded()")
        val loadLibrary = natives.indexOf("System.loadLibrary")
        assertTrue(ensureLoaded >= 0 && loadLibrary > ensureLoaded)
        assertFalse(natives.substring(0, ensureLoaded).contains("System.loadLibrary"))
    }

    @Test
    fun superCallRebootRunsOnlyInForkedChildWithoutFastNative() {
        val supercall = source("src/main/cpp/supercall.h")
        val acquireFd = supercall
            .substringAfter("static inline int ethereal_fd")
            .substringBefore("static inline long ethereal_call")
        val fork = acquireFd.indexOf("pid = fork()")
        val child = acquireFd.indexOf("if (pid == 0)")
        val reboot = acquireFd.indexOf("syscall(__NR_reboot")
        val receive = acquireFd.indexOf("ethereal_receive_fd")
        val wait = acquireFd.indexOf("waitpid")

        assertTrue(fork >= 0 && child > fork && reboot > child)
        assertTrue(receive > reboot && wait > receive)
        assertTrue(acquireFd.windowed("syscall(__NR_reboot".length)
            .count { it == "syscall(__NR_reboot" } == 1)

        val kotlinSources = sequenceOf(File("src/main"), File("app/src/main"))
            .first { it.isDirectory }
            .walkTopDown()
            .filter { it.isFile && it.extension in setOf("kt", "java") }
            .joinToString("\n") { it.readText() }
        assertFalse(kotlinSources.contains("@FastNative"))
        assertFalse(kotlinSources.contains("dalvik.annotation.optimization.FastNative"))
    }

    @Test
    fun libsuIoCannotReintroduceImplicitSuExecution() {
        val sourceRoot = sequenceOf(File("src/main"), File("app/src/main"))
            .first { it.isDirectory }
        val sources = sourceRoot.walkTopDown()
            .filter { it.isFile && it.extension in setOf("kt", "java") }
            .joinToString("\n") { it.readText() }
        assertFalse(sources.contains("SuFile"))

        val build = sequenceOf(File("build.gradle.kts"), File("app/build.gradle.kts"))
            .first { it.isFile }
            .readText()
        assertFalse(build.contains("libsu.io"))
    }

    @Test
    fun libsuShellUsesOnlySystemShAfterSuperCallRoot() {
        val cli = source("src/main/java/me/ethereal/app/util/EtherealCli.kt")
        val openRootShell = cli
            .substringAfter("private fun openRootShell")
            .substringBefore("fun createRootShell")

        val becomeRoot = openRootShell.indexOf("if (!becomeRoot())")
        val builder = openRootShell.indexOf("Shell.Builder.create()")
        val systemSh = openRootShell.indexOf("tryBuildShell(builder, \"/system/bin/sh\")")
        assertTrue(becomeRoot >= 0 && builder > becomeRoot && systemSh > builder)
        assertFalse(openRootShell.contains("\"su\""))
        assertFalse(openRootShell.contains("su --mount-master"))
        assertFalse(cli.contains("com.topjohnwu.superuser.internal.MainShell"))
        assertTrue(cli.contains("Shell.setDefaultBuilder("))
    }

    @Test
    fun selectedBootImagesPatchWithoutExistingRoot() {
        val bootFlash = source("src/main/java/me/ethereal/app/util/BootFlash.kt")
        val filePatch = bootFlash
            .substringAfter("if (item.source !is FlashIt.BootSource.Direct)")
            .substringBefore("if (!becomeRoot()) error(\"SuperCall root required to flash a partition\")")

        assertTrue(filePatch.contains("runProcess(args, work, line)"))
        assertFalse(filePatch.contains("becomeRoot()"))

        val install = source("src/main/java/me/ethereal/app/ui/screen/BootInstall.kt")
        val options = install
            .substringAfter("val radioOptions = remember")
            .substringBefore("var selectedOption")
        val selectFile = options.indexOf("InstallMethod.SelectFile")
        val rootGate = options.indexOf("if (rootAvailable)")
        val direct = options.indexOf("InstallMethod.DirectInstall")
        assertTrue(selectFile >= 0 && rootGate > selectFile && direct > rootGate)
    }

    @Test
    fun fileProviderCanOnlyShareAppCache() {
        val paths = source("src/main/res/xml/file_paths.xml")

        assertTrue(paths.contains("<cache-path"))
        assertFalse(paths.contains("<root-path"))
        assertFalse(paths.contains("<external-path"))
        assertFalse(paths.contains("<external-files-path"))
        assertFalse(paths.contains("<external-cache-path"))
    }

    @Test
    fun webDebuggingCannotExposeTheRootJavascriptBridge() {
        val webUi = source("src/main/java/me/ethereal/app/ui/WebUIActivity.kt")
        assertTrue(
            webUi.contains(
                "BuildConfig.DEBUG && prefs.getBoolean(\"enable_web_debugging\", false)"
            )
        )
        val bridgeSetup = webUi
            .substringAfter("settings.safeBrowsingEnabled = true")
            .substringBefore("setWebViewClient(webViewClient)")
        assertTrue(bridgeSetup.contains("if (!webDebuggingEnabled)"))
        assertTrue(bridgeSetup.contains("addJavascriptInterface(webViewInterface, \"ksu\")"))

        val settings = source("src/main/java/me/ethereal/app/ui/screen/Settings.kt")
        val debugGate = settings.indexOf("if (BuildConfig.DEBUG)")
        val toggleTitle = settings.indexOf("R.string.enable_web_debugging)")
        assertTrue(debugGate >= 0 && toggleTitle > debugGate)
    }

    @Test
    fun userTriggeredRootActionsDispatchToIo() {
        val home = source("src/main/java/me/ethereal/app/ui/screen/Home.kt")
        val rebootItem = home
            .substringAfter("fun RebootDropdownItem")
            .substringBefore("private fun TopBar")
        assertTrue(
            rebootItem.indexOf("scope.launch(Dispatchers.IO)") in
                0 until rebootItem.indexOf("reboot(reason)")
        )

        val settings = source("src/main/java/me/ethereal/app/ui/screen/Settings.kt")
        val uninstall = settings
            .substringAfter("fun UninstallDialog")
            .substringBefore("fun ThemeChooseDialog")
        assertTrue(
            uninstall.indexOf("scope.launch(Dispatchers.IO)") in
                0 until uninstall.indexOf("uninstallEthereal()")
        )

        val resetPath = settings
            .substringAfter("fun ResetSUPathDialog")
            .substringBefore("fun SelinuxHideWarningDialog")
        val resetNative = resetPath.indexOf("Natives.resetSuPath")
        assertTrue(resetNative > 0)
        assertTrue(
            resetPath.lastIndexOf("withContext(Dispatchers.IO)", resetNative) in
                0 until resetNative
        )

        val superUser = source("src/main/java/me/ethereal/app/ui/viewmodel/SuperUserViewModel.kt")
        val grants = superUser.substringAfter("fun setRootGranted")
        val grantNative = grants.indexOf("Natives.grantSu")
        assertTrue(
            grants.indexOf("withContext(Dispatchers.IO)") in 0 until grantNative
        )
    }

    @Test
    fun installScreenKeepsLogsAndRebootOffWorkerUiState() {
        val install = source("src/main/java/me/ethereal/app/ui/screen/Install.kt")
        val installRun = install
            .substringAfter("LaunchedEffect(Unit)")
            .substringBefore("Scaffold(")
        val stdout = installRun
            .substringAfter("onStdout = { line ->")
            .substringBefore("onStderr = { line ->")
        val stderr = installRun
            .substringAfter("onStderr = { line ->")
            .substringBefore("})")
        assertTrue(stdout.contains("enqueueLog(line)"))
        assertTrue(stderr.contains("enqueueLog(line)"))
        assertFalse(stdout.contains("scope.launch"))
        assertFalse(stderr.contains("scope.launch"))

        val consumer = install
            .substringAfter("LaunchedEffect(logSignal)")
            .substringBefore("LaunchedEffect(Unit)")
        assertTrue(consumer.contains("installLog.drain()"))
        assertTrue(consumer.contains("text = snapshot.text"))

        val save = install
            .substringAfter("val content = logContent.toString()")
            .substringBefore("floatingActionButton")
        val saveIo = save.indexOf("withContext(Dispatchers.IO)")
        val write = save.indexOf("file.writeText(content)")
        assertTrue(saveIo >= 0 && write > saveIo)

        val reboot = install.substringAfter("ExtendedFloatingActionButton(")
        val rebootIo = reboot.indexOf("withContext(Dispatchers.IO)")
        val rebootCall = reboot.indexOf("reboot()")
        assertTrue(rebootIo >= 0 && rebootCall > rebootIo)
    }

    @Test
    fun webUiRootAndCanonicalPathsResolveOnIo() {
        val activity = source("src/main/java/me/ethereal/app/ui/WebUIActivity.kt")
        val setup = activity
            .substringAfter("private suspend fun setupWebView()")
            .substringBefore("if (Build.VERSION.SDK_INT")
        val io = setup.indexOf("withContext(Dispatchers.IO)")
        val root = setup.indexOf("becomeRoot()")
        val modulesCanonical = setup.indexOf("File(\"/data/adb/modules\").canonicalFile")
        val webCanonical = setup.indexOf("File(modulesRoot, \"\$moduleId/webroot\").canonicalFile")
        assertTrue(io >= 0 && root > io)
        assertTrue(modulesCanonical > root && webCanonical > modulesCanonical)
    }

    @Test
    fun webViewBridgeCannotOpenRootShellOnMainThread() {
        val bridge = source("src/main/java/me/ethereal/app/ui/webui/WebViewInterface.kt")
        val synchronousExec = bridge
            .substringAfter("fun exec(cmd: String): String")
            .substringBefore("@JavascriptInterface", missingDelimiterValue = bridge)
        val mainGuard = synchronousExec.indexOf("Looper.myLooper() == Looper.getMainLooper()")
        val rootShell = synchronousExec.indexOf("tryGetRootShell()")
        assertTrue(mainGuard >= 0 && rootShell > mainGuard)

        val callbackExec = bridge
            .substringAfter("fun exec(\n")
            .substringBefore("fun spawn(")
        val callbackExecutor = callbackExec.indexOf("submitRoot")
        val callbackRoot = callbackExec.indexOf("tryGetRootShell()")
        assertTrue(callbackExecutor >= 0 && callbackRoot > callbackExecutor)

        val spawn = bridge
            .substringAfter("fun spawn(")
            .substringBefore("fun close()")
        val spawnExecutor = spawn.indexOf("submitRoot")
        val spawnRoot = spawn.indexOf("tryGetRootShell()")
        assertTrue(spawnExecutor >= 0 && spawnRoot > spawnExecutor)
    }
}
