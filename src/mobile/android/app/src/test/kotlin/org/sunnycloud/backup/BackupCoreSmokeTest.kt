package org.sunnycloud.backup

import java.nio.file.Files
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.mobile_backup.AccessLevel
import uniffi.mobile_backup.BackupCore
import uniffi.mobile_backup.libraryVersion

/** Loads the host build of the Rust library through JNA (AZ-4002 AC-1) and calls it without a server. */
class BackupCoreSmokeTest {
    @Test
    fun the_library_loads_and_reports_its_version() {
        // Arrange / Act
        val version = libraryVersion()

        // Assert
        assertTrue("version was '$version'", version.matches(Regex("""\d+\.\d+\.\d+""")))
    }

    @Test
    fun status_of_a_new_checkpoint_is_empty_without_a_server() {
        // Arrange
        val directory = Files.createTempDirectory("backup-core-smoke").toFile()
        val core = BackupCore(directory.path, "1.0.0")

        // Act
        val status = core.status()

        // Assert
        assertEquals(0UL, status.visible)
        assertEquals(0UL, status.uploaded)
        assertEquals(0UL, status.pending)
        assertNull(status.paused)
        assertNull(status.lastError)
        assertEquals(AccessLevel.FULL, status.accessLevel)
        assertFalse(status.locationRedacted)
        core.close()
        directory.deleteRecursively()
    }
}
