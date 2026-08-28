package me.ethereal.app.ui.screen

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class BootImageSelectionTest {
    @Test
    fun bootImageSelectionEnablesOfflinePatch() {
        val selected = updateBootImageSelection(
            current = null,
            uri = "content://images/boot.img",
            summary = "Select one image",
        )

        assertEquals("content://images/boot.img", selected.imageUri)
        assertTrue(hasSelectedBootImage(selected))
    }

    @Test
    fun initBootImageSelectionEnablesOfflinePatch() {
        val selected = updateBootImageSelection(
            current = null,
            uri = "content://images/init_boot.img",
            summary = "Select one image",
        )

        assertEquals("content://images/init_boot.img", selected.imageUri)
        assertTrue(hasSelectedBootImage(selected))
    }

    @Test
    fun reselectionReplacesPreviousImage() {
        val boot = InstallMethod.SelectFile(
            imageUri = "content://images/boot.img",
            summary = "Select one image",
        )

        val selected = updateBootImageSelection(
            current = boot,
            uri = "content://images/init_boot.img",
            summary = "Select one image",
        )

        assertEquals("content://images/init_boot.img", selected.imageUri)
        assertTrue(hasSelectedBootImage(selected))
    }

    @Test
    fun missingOrBlankImageCannotContinue() {
        val missing = InstallMethod.SelectFile(summary = "Select one image")
        val blank = missing.copy(imageUri = "   ")

        assertFalse(hasSelectedBootImage(missing))
        assertFalse(hasSelectedBootImage(blank))
    }

    @Test
    fun unknownFileNameDoesNotBlockOfflinePatch() {
        val selected = updateBootImageSelection(
            current = null,
            uri = "content://images/payload.bin",
            summary = "Select one image",
        )

        assertTrue(hasSelectedBootImage(selected))
    }
}
