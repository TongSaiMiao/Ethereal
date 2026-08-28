package me.ethereal.app.util

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class KmiInferenceTest {
    @Test
    fun unameTagTakesPriorityOverSdk() {
        assertEquals(
            "android12-5.10",
            inferKmi("5.10.218-android12-9-g123456789abc", 33),
        )
        assertEquals(
            "android13-5.15",
            inferKmi("5.15.153-android13-8-g123456789abc", 34),
        )
    }

    @Test
    fun ambiguousKernelLinesUseInitialSdk() {
        assertEquals("android12-5.10", inferKmi("5.10.218-oem", 31))
        assertEquals("android12-5.10", inferKmi("5.10.218-oem", 32))
        assertEquals("android13-5.10", inferKmi("5.10.218-oem", 33))
        assertEquals("android13-5.15", inferKmi("5.15.153-oem", 33))
        assertEquals("android14-5.15", inferKmi("5.15.153-oem", 34))
    }

    @Test
    fun ambiguousOrUnsupportedInputsDoNotGuess() {
        assertNull(inferKmi("5.10.218-oem", 34))
        assertNull(inferKmi("5.15.153-oem", 35))
        assertNull(inferKmi("5.4.210-oem", 30))
        assertNull(inferKmi("5.4.210-android11-0-g123456789abc", 31))
        assertNull(inferKmi("4.19.157-oem", 30))
    }

    @Test
    fun uniqueSupportedLinesRemainDeterministic() {
        assertEquals("android14-6.1", inferKmi("6.1.176-oem", 35))
        assertEquals("android15-6.6", inferKmi("6.6.142-oem", 35))
        assertEquals("android16-6.12", inferKmi("6.12.90-oem", 36))
    }
}
