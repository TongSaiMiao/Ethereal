package me.ethereal.app

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ServiceInstallChecksTest {
    @Test
    fun `installed state requires artifacts daemon and sepolicy`() {
        assertTrue(serviceInstallSucceeded(true, 0, 0))
        assertFalse(serviceInstallSucceeded(false, 0, 0))
        assertFalse(serviceInstallSucceeded(true, 1, 0))
        assertFalse(serviceInstallSucceeded(true, 0, 1))
        assertFalse(serviceInstallSucceeded(true, null, 0))
        assertFalse(serviceInstallSucceeded(true, 0, null))
    }

    @Test
    fun `module detection preserves false and treats errors as absent`() {
        assertTrue(modulePathPresent { true })
        assertFalse(modulePathPresent { false })
        assertFalse(modulePathPresent { throw IllegalStateException("denied") })
    }
}
