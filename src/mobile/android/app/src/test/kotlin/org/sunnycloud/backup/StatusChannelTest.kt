package org.sunnycloud.backup

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.mobile_backup.AccessLevel
import uniffi.mobile_backup.BackupCore
import uniffi.mobile_backup.BackupCoreInterface
import uniffi.mobile_backup.BackupException
import uniffi.mobile_backup.BackupStatus
import uniffi.mobile_backup.LastError
import uniffi.mobile_backup.PauseReason
import uniffi.mobile_backup.ServerCheck
import uniffi.mobile_backup.ServerTrust
import uniffi.mobile_backup.SkippedCounts

/** The event stream and the commands of status_channel.md over a fake core (AZ-4000 AC-1, AC-2, AC-4). */
class StatusChannelTest {
    @get:Rule
    val folder = TemporaryFolder()

    private fun harness(core: BackupCoreInterface = FakeCore(), state: NativeState = FakeState()) =
        Harness(core, state, FakePlatform(), SettingsStore(File(folder.root, "settings.properties")))

    private fun statusOf(uploaded: ULong, pending: ULong = 0UL) = emptyStatus().copy(visible = uploaded + pending, uploaded = uploaded, pending = pending)

    @Test
    fun subscribing_sends_one_full_snapshot_and_each_publish_sends_another() {
        // Arrange
        val core = FakeCore(status = statusOf(uploaded = 3UL))
        val h = harness(core)

        // Act
        h.subscribe()
        core.status = statusOf(uploaded = 4UL)
        h.channel.publish()

        // Assert
        val counts = h.capture.events.map { (it as Map<*, *>)[Fields.UPLOADED] }
        assertEquals(listOf<Any?>(3L, 4L), counts)
    }

    @Test
    fun a_snapshot_is_sent_once_per_publish_and_status_is_read_once_per_snapshot() {
        // Arrange
        var reads = 0
        val h = harness(FakeCore(onStatus = { reads++ }))
        h.subscribe()

        // Act
        h.channel.publish()
        h.channel.publish()

        // Assert
        assertEquals(3, h.capture.events.size)
        assertEquals(3, reads)
    }

    @Test
    fun nothing_is_sent_after_the_subscriber_cancels() {
        // Arrange
        val h = harness()
        h.subscribe()

        // Act
        h.channel.onCancel(null)
        h.channel.publish()

        // Assert
        assertEquals(1, h.capture.events.size)
    }

    @Test
    fun a_scripted_status_is_sent_as_the_contract_names_it_and_the_counts_add_up() {
        // Arrange
        val status = BackupStatus(
            visible = 19UL, uploaded = 10UL, pending = 5UL, paused = PauseReason.QUOTA_EXCEEDED,
            skipped = SkippedCounts(gone = 1UL, name = 2UL, size = 1UL, failed = 0UL),
            lastError = LastError("quota", "c-81"), accessLevel = AccessLevel.LIMITED, locationRedacted = true,
        )
        val h = harness(FakeCore(status = status), FakeState(signedIn = true, deviceLabel = "Pixel", running = true, batteryExempt = true))

        // Act
        h.subscribe()

        // Assert
        val event = h.capture.events.single() as Map<*, *>
        assertEquals("QuotaExceeded", event[Fields.PAUSED])
        assertEquals("limited", event[Fields.ACCESS_LEVEL])
        assertEquals(mapOf(Fields.MESSAGE to "quota", Fields.CORRELATION_ID to "c-81"), event[Fields.LAST_ERROR])
        assertEquals(mapOf(Fields.GONE to 1L, Fields.NAME to 2L, Fields.SIZE to 1L, Fields.FAILED to 0L), event[Fields.SKIPPED])
        assertEquals(true, event[Fields.LOCATION_REDACTED])
        assertEquals(true, event[Fields.SIGNED_IN])
        assertEquals("Pixel", event[Fields.DEVICE_LABEL])
        assertEquals(true, event[Fields.RUNNING])
        assertEquals(true, event[Fields.BATTERY_EXEMPT])
        val skipped = event[Fields.SKIPPED] as Map<*, *>
        assertEquals(event[Fields.VISIBLE], (event[Fields.UPLOADED] as Long) + (event[Fields.PENDING] as Long) + skipped.values.sumOf { it as Long })
    }

    @Test
    fun a_new_install_snapshot_has_defaults_and_no_server() {
        // Arrange
        val h = harness()

        // Act
        h.subscribe()

        // Assert
        val event = h.capture.events.single() as Map<*, *>
        assertNull(event[Fields.PAUSED])
        assertNull(event[Fields.LAST_ERROR])
        assertNull(event[Fields.SERVER_URL])
        assertNull(event[Fields.SERVER_TRUST])
        assertEquals(mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to false, Fields.BUCKETS to null), event[Fields.SETTINGS])
    }

    @Test
    fun a_failed_status_read_is_sent_as_a_typed_error_event() {
        // Arrange
        val h = harness(FakeCore(onStatus = { throw BackupException.CheckpointUnavailable() }))

        // Act
        h.subscribe()

        // Assert
        assertEquals(0, h.capture.events.size)
        assertEquals("CheckpointUnavailable", h.capture.errors.single().first)
    }

    @Test
    fun an_unexpected_status_failure_is_logged_and_sent_as_a_code_without_text() {
        // Arrange
        val h = harness(FakeCore(onStatus = { throw IllegalStateException("internal detail") }))

        // Act
        h.subscribe()

        // Assert
        assertEquals(Triple("Unexpected", null, null), h.capture.errors.single())
        assertEquals("internal detail", h.platform.logged.single().message)
    }

    @Test
    fun set_server_returns_the_trust_and_the_next_snapshot_shows_the_server() {
        // Arrange
        val h = harness(FakeCore(onSetServer = { ServerCheck(ServerTrust.USER_ADDED) }))
        h.subscribe()

        // Act
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))

        // Assert
        assertEquals(mapOf(Fields.TRUST to "user_added"), h.capture.results.single())
        val event = h.capture.events.last() as Map<*, *>
        assertEquals("https://cloud.example.org", event[Fields.SERVER_URL])
        assertEquals("user_added", event[Fields.SERVER_TRUST])
    }

    @Test
    fun set_server_failures_carry_the_error_names_of_the_contract() {
        // Arrange
        val failures = mapOf<String, Throwable>(
            "HttpsRequired" to BackupException.HttpsRequired(),
            "ServerUnreachable" to BackupException.ServerUnreachable(),
            "InvalidServerUrl" to BackupException.InvalidServerUrl(),
            "UpgradeRequired" to BackupException.UpgradeRequired("Update the app"),
        )

        for ((code, failure) in failures) {
            val h = harness(FakeCore(onSetServer = { throw failure }))

            // Act
            h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))

            // Assert
            assertEquals(code, h.capture.errors.single().first)
        }
    }

    @Test
    fun an_upgrade_required_error_carries_the_server_text() {
        // Arrange
        val h = harness(FakeCore(onSetServer = { throw BackupException.UpgradeRequired("Update the app") }))

        // Act
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))

        // Assert
        assertEquals(Triple("UpgradeRequired", "Update the app", mapOf(Fields.MESSAGE to "Update the app")), h.capture.errors.single())
    }

    @Test
    fun the_real_core_refuses_a_plain_http_address_with_https_required() {
        // Arrange
        val directory = folder.newFolder("core")
        val core = BackupCore(directory.path, "1.0.0")
        val h = harness(core)

        // Act
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "http://cloud.example.org"))

        // Assert
        assertEquals("HttpsRequired", h.capture.errors.single().first)
        core.close()
    }

    @Test
    fun set_server_without_a_text_url_is_invalid_on_the_url_field() {
        // Arrange
        val h = harness()

        for (arguments in listOf(null, emptyMap<String, Any?>(), mapOf(Fields.URL to 5))) {
            // Act
            h.send(Channels.SET_SERVER, arguments)
        }

        // Assert
        val expected = Triple("Invalid", null, mapOf(Fields.FIELD to Fields.URL))
        assertEquals(listOf(expected, expected, expected), h.capture.errors)
    }

    @Test
    fun update_settings_stores_returns_and_publishes_the_new_values() {
        // Arrange
        val h = harness()
        h.subscribe()
        val settings = mapOf(Fields.UNMETERED_ONLY to false, Fields.CHARGING_ONLY to true, Fields.BUCKETS to listOf("Camera"))

        // Act
        h.send(Channels.UPDATE_SETTINGS, settings)

        // Assert
        assertEquals(settings, h.capture.results.single())
        assertEquals(settings, (h.capture.events.last() as Map<*, *>)[Fields.SETTINGS])
        assertEquals(Settings(unmeteredOnly = false, chargingOnly = true, buckets = listOf("Camera")), h.settings.read())
    }

    @Test
    fun invalid_settings_name_the_field_and_leave_the_stored_values_unchanged() {
        // Arrange
        val h = harness()
        val before = Settings(unmeteredOnly = false, chargingOnly = true, buckets = listOf("Camera"))
        h.settings.write(before)
        val calls = listOf(
            mapOf(Fields.UNMETERED_ONLY to "yes", Fields.CHARGING_ONLY to true) to Fields.UNMETERED_ONLY,
            mapOf(Fields.UNMETERED_ONLY to true) to Fields.CHARGING_ONLY,
            mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to true, Fields.BUCKETS to "Camera") to Fields.BUCKETS,
            mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to true, Fields.BUCKETS to listOf("Camera", " ")) to Fields.BUCKETS,
            mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to true, Fields.BUCKETS to listOf(7)) to Fields.BUCKETS,
        )

        for ((arguments, field) in calls) {
            // Act
            val errorsBefore = h.capture.errors.size
            h.send(Channels.UPDATE_SETTINGS, arguments)

            // Assert
            assertEquals(Triple("Invalid", null, mapOf(Fields.FIELD to field)), h.capture.errors[errorsBefore])
        }
        assertEquals(before, h.settings.read())
    }

    @Test
    fun a_command_without_a_native_implementation_is_not_implemented() {
        // Arrange
        val h = harness()

        // Act
        val result = h.send("signIn", mapOf(Fields.DEVICE_LABEL to "Pixel"))

        // Assert
        assertTrue(result.notImplemented)
        assertEquals(0, h.capture.errors.size)
    }
}
