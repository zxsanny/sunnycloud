package org.sunnycloud.backup

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.mobile_backup.BackupCore
import uniffi.mobile_backup.libraryVersion

/** The packaged library of the device's ABI, loaded through JNA in the app process (AZ-4002 AC-1). */
@RunWith(AndroidJUnit4::class)
class BackupCoreInstrumentedTest {
    @Test
    fun the_packaged_library_answers_status_without_a_server() {
        // Arrange
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val directory = context.cacheDir.resolve("backup-core-instrumented").apply { mkdirs() }
        val core = BackupCore(directory.path, "1.0.0")

        // Act
        val status = core.status()

        // Assert
        assertTrue(libraryVersion().isNotEmpty())
        assertEquals(0UL, status.visible)
        core.close()
        directory.deleteRecursively()
    }

    @Test
    fun the_trust_store_initialises_in_the_app_process() {
        // Arrange
        val context = InstrumentationRegistry.getInstrumentation().targetContext

        // Act / Assert
        assertTrue(TlsTrustBridge.nativeInit(context))
    }
}
