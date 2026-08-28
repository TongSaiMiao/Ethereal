package me.ethereal.app

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import me.ethereal.app.ui.screen.BottomBarDestination
import me.ethereal.app.ui.screen.visibleBottomBarDestinations

class ManagerAccessContractTest {
    @Test
    fun moduleAbsentShowsOnlyUnprivilegedTabs() {
        val visible = visibleBottomBarDestinations(
            kernelReady = false,
            managerAccessReady = false,
            serviceReady = false,
        )

        assertEquals(
            setOf(BottomBarDestination.Home, BottomBarDestination.Settings),
            visible,
        )
    }

    @Test
    fun authenticationFailureKeepsSuperUserHiddenWhileKernelCanBeWorking() {
        val visible = visibleBottomBarDestinations(
            kernelReady = true,
            managerAccessReady = false,
            serviceReady = false,
        )

        assertTrue(BottomBarDestination.Home in visible)
        assertTrue(BottomBarDestination.Settings in visible)
        assertFalse(BottomBarDestination.SuperUser in visible)
        assertFalse(BottomBarDestination.Modules in visible)
    }

    @Test
    fun authenticatedKernelShowsSuperUserWithoutPretendingServiceIsInstalled() {
        val visible = visibleBottomBarDestinations(
            kernelReady = true,
            managerAccessReady = true,
            serviceReady = false,
        )

        assertTrue(BottomBarDestination.SuperUser in visible)
        assertFalse(BottomBarDestination.Modules in visible)
    }

    @Test
    fun fullyWorkingStateShowsEveryTab() {
        val visible = visibleBottomBarDestinations(
            kernelReady = true,
            managerAccessReady = true,
            serviceReady = true,
        )

        assertEquals(BottomBarDestination.entries.toSet(), visible)
    }
}
