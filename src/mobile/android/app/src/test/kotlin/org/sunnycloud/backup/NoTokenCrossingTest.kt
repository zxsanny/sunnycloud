package org.sunnycloud.backup

import java.io.File
import java.lang.reflect.Modifier
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.mobile_backup.BackupException
import uniffi.mobile_backup.ServerCheck
import uniffi.mobile_backup.ServerTrust

/** The channel-side leg of ST-17-1 (AZ-4000 AC-3): no token, one-time code or PKCE verifier can reach Dart. */
class NoTokenCrossingTest {
    @get:Rule
    val folder = TemporaryFolder()

    private val tokenLike = Regex("token|code|verifier|secret|password|credential|authori[sz]ation|bearer|refresh|jwt", RegexOption.IGNORE_CASE)

    private fun declaredNames(): List<String> =
        (Fields::class.java.declaredFields + Channels::class.java.declaredFields)
            .filter { Modifier.isStatic(it.modifiers) && it.type == String::class.java }
            .map { it.get(null) as String }

    private fun tokenLikeNames(names: Collection<String>) = names.filter { tokenLike.containsMatchIn(it) }

    private fun undeclaredKeys(capture: Capture) = capture.keys() - declaredNames().toSet()

    /** The native side: signed in, and holding secrets that must never be read into a payload. */
    private class SecretHolder : FakeState(signedIn = true, deviceLabel = "Pixel") {
        val accessToken = "sentinel-access-5b1e"
        val refreshToken = "sentinel-refresh-9c42"
        val authorizationCode = "sentinel-code-77d0"
        val pkceVerifier = "sentinel-verifier-3a8f"
        val all get() = listOf(accessToken, refreshToken, authorizationCode, pkceVerifier)
    }

    private fun sentinelHarness(secrets: SecretHolder, core: FakeCore): Harness =
        Harness(core, secrets, FakePlatform(), SettingsStore(File(folder.root, "settings.properties")))

    @Test
    fun the_real_schema_has_no_field_or_command_named_like_a_token_code_or_verifier() {
        // Arrange
        val names = declaredNames()

        // Act
        val offending = tokenLikeNames(names)

        // Assert
        assertTrue("schema names were $names", names.size > 20)
        assertEquals(emptyList<String>(), offending)
    }

    @Test
    fun the_native_state_the_snapshot_reads_has_no_member_named_like_a_token() {
        // Arrange
        val members = NativeState::class.java.methods.map { it.name } + Settings::class.java.declaredFields.map { it.name }

        // Act
        val offending = tokenLikeNames(members)

        // Assert
        assertEquals(emptyList<String>(), offending)
    }

    @Test
    fun a_seeded_token_field_makes_the_schema_check_fail() {
        // Arrange
        val seeded = declaredNames() + "access_token"

        // Act
        val offending = tokenLikeNames(seeded)

        // Assert
        assertEquals(listOf("access_token"), offending)
    }

    @Test
    fun a_seeded_token_field_in_a_sent_event_makes_the_runtime_key_check_fail() {
        // Arrange
        val capture = Capture()
        capture.events += mapOf(Fields.SIGNED_IN to true, "access_token" to "x")

        // Act
        val undeclared = undeclaredKeys(capture)

        // Assert
        assertEquals(setOf("access_token"), undeclared)
    }

    @Test
    fun a_sentinel_in_a_sent_payload_is_found_by_the_scan() {
        // Arrange
        val capture = Capture()
        capture.results += mapOf(Fields.SETTINGS to listOf(mapOf(Fields.MESSAGE to "x sentinel-access-5b1e y")))

        // Act
        val text = capture.everything()

        // Assert
        assertTrue(text.contains("sentinel-access-5b1e"))
    }

    @Test
    fun a_signed_in_session_with_commands_and_failures_sends_no_sentinel_and_only_declared_keys() {
        // Arrange
        val secrets = SecretHolder()
        var failure: Throwable? = null
        val core = FakeCore(onSetServer = { _ -> failure?.let { throw it } ?: ServerCheck(ServerTrust.SYSTEM) })
        val h = sentinelHarness(secrets, core)

        // Act
        h.subscribe()
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))
        failure = IllegalStateException("Bearer ${secrets.accessToken}", RuntimeException(secrets.refreshToken))
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))
        failure = BackupException.UpgradeRequired("Update the app")
        h.send(Channels.SET_SERVER, mapOf(Fields.URL to "https://cloud.example.org"))
        h.send(
            Channels.UPDATE_SETTINGS,
            mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to false, "refresh_token" to secrets.refreshToken, "code" to secrets.authorizationCode),
        )
        h.send("signIn", mapOf(Fields.DEVICE_LABEL to "Pixel", "code_verifier" to secrets.pkceVerifier))
        core.onStatus = { throw IllegalStateException(secrets.pkceVerifier) }
        h.channel.publish()

        // Assert
        val sent = h.capture.everything()
        val leaked = secrets.all.filter { sent.contains(it) }
        assertEquals(emptyList<String>(), leaked)
        assertEquals(emptySet<String>(), undeclaredKeys(h.capture))
        assertEquals(3, h.capture.events.size)
        assertEquals(listOf("Unexpected", "UpgradeRequired", "Unexpected"), h.capture.errors.map { it.first })
    }

    @Test
    fun arguments_that_carry_a_token_are_neither_stored_nor_echoed() {
        // Arrange
        val secrets = SecretHolder()
        val h = sentinelHarness(secrets, FakeCore())

        // Act
        h.send(
            Channels.UPDATE_SETTINGS,
            mapOf(Fields.UNMETERED_ONLY to false, Fields.CHARGING_ONLY to false, "access_token" to secrets.accessToken),
        )

        // Assert
        val stored = File(folder.root, "settings.properties").readText()
        assertFalse(stored.contains(secrets.accessToken))
        assertFalse(h.capture.everything().contains(secrets.accessToken))
    }
}
