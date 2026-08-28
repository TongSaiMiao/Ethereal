package me.ethereal.app.ui.screen

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class BootImageSelectionTest {
    @Test
    fun initBootSelectionUpdatesOnlyInitBoot() {
        val selected = updateBootImageSelection(
            current = null,
            kind = BootImageKind.INIT_BOOT,
            uri = "content://images/init_boot.img",
            gki2 = true,
            summary = "Select both images",
        )

        assertEquals("content://images/init_boot.img", selected.initBootUri)
        assertNull(selected.bootUri)
    }

    @Test
    fun bootSelectionPreservesPreviouslySelectedInitBoot() {
        val initBoot = InstallMethod.SelectFile(
            initBootUri = "content://images/init_boot.img",
            gki2 = true,
            summary = "Select both images",
        )

        val selected = updateBootImageSelection(
            current = initBoot,
            kind = BootImageKind.BOOT,
            uri = "content://images/boot.img",
            gki2 = true,
            summary = "Select both images",
        )

        assertEquals("content://images/init_boot.img", selected.initBootUri)
        assertEquals("content://images/boot.img", selected.bootUri)
        assertTrue(hasRequiredBootImages(selected))
    }

    @Test
    fun gki1BootSelectionDoesNotRequireInitBoot() {
        val selected = updateBootImageSelection(
            current = null,
            kind = BootImageKind.BOOT,
            uri = "content://images/boot.img",
            gki2 = false,
            summary = "Select boot image",
        )

        assertEquals("content://images/boot.img", selected.bootUri)
        assertNull(selected.initBootUri)
        assertTrue(hasRequiredBootImages(selected))
    }

    @Test
    fun gki2RejectsMissingOrDuplicateImages() {
        val missingBoot = InstallMethod.SelectFile(
            initBootUri = "content://images/init_boot.img",
            gki2 = true,
            summary = "Select both images",
        )
        val duplicate = missingBoot.copy(bootUri = missingBoot.initBootUri)

        assertFalse(hasRequiredBootImages(missingBoot))
        assertFalse(hasRequiredBootImages(duplicate))
    }
}
