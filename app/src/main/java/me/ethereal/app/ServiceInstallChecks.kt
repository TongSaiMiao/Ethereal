package me.ethereal.app

internal fun serviceInstallSucceeded(
    requiredArtifactsReady: Boolean,
    daemonProbeExitCode: Int?,
    sepolicyExitCode: Int?,
): Boolean {
    return requiredArtifactsReady && daemonProbeExitCode == 0 && sepolicyExitCode == 0
}

internal fun modulePathPresent(access: () -> Boolean): Boolean {
    return runCatching(access).getOrDefault(false)
}
