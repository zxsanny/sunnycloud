package org.sunnycloud.backup

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/** Settings are stored natively and survive a restart (AZ-4000 AC-4). */
class SettingsStoreTest {
    @get:Rule
    val folder = TemporaryFolder()

    private fun file() = File(folder.root, "settings.properties")

    @Test
    fun a_store_with_no_file_holds_the_defaults() {
        // Arrange
        val store = SettingsStore(file())

        // Act
        val settings = store.read()

        // Assert
        assertEquals(Settings(unmeteredOnly = true, chargingOnly = false, buckets = null), settings)
    }

    @Test
    fun written_settings_are_read_back_by_a_new_store_on_the_same_file() {
        // Arrange
        val written = Settings(unmeteredOnly = false, chargingOnly = true, buckets = listOf("Camera", "Screenshots"))
        SettingsStore(file()).write(written)

        // Act
        val afterRestart = SettingsStore(file()).read()

        // Assert
        assertEquals(written, afterRestart)
    }

    @Test
    fun an_empty_selection_and_no_selection_stay_different() {
        // Arrange
        val store = SettingsStore(file())

        // Act
        store.write(Settings(buckets = emptyList()))
        val none = store.read().buckets
        store.write(Settings(buckets = null))
        val all = store.read().buckets

        // Assert
        assertEquals(emptyList<String>(), none)
        assertEquals(null, all)
    }

    @Test
    fun bucket_names_with_separators_and_unicode_survive_a_round_trip() {
        // Arrange
        val names = listOf("Trip, 2026", "line\nbreak", "Фото = 100%", "a\\b")
        val store = SettingsStore(file())

        // Act
        store.write(Settings(buckets = names))

        // Assert
        assertEquals(names, SettingsStore(file()).read().buckets)
    }

    @Test
    fun a_second_write_replaces_the_buckets_of_the_first() {
        // Arrange
        val store = SettingsStore(file())
        store.write(Settings(buckets = listOf("A", "B", "C")))

        // Act
        store.write(Settings(buckets = listOf("A")))

        // Assert
        assertEquals(listOf("A"), store.read().buckets)
    }

    @Test
    fun parse_accepts_valid_arguments_and_dedupes_buckets() {
        // Arrange
        val arguments = mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to false, Fields.BUCKETS to listOf("A", "A", "B"))

        // Act
        val parsed = Settings.parse(arguments)

        // Assert
        assertEquals(Settings.Parsed.Valid(Settings(true, false, listOf("A", "B"))), parsed)
    }

    @Test
    fun parse_reads_a_missing_or_null_buckets_as_all_albums() {
        // Arrange
        val withoutKey = mapOf(Fields.UNMETERED_ONLY to true, Fields.CHARGING_ONLY to false)
        val withNull = withoutKey + (Fields.BUCKETS to null)

        // Act
        val parsed = listOf(Settings.parse(withoutKey), Settings.parse(withNull))

        // Assert
        assertEquals(List(2) { Settings.Parsed.Valid(Settings(true, false, null)) }, parsed)
    }

    @Test
    fun parse_rejects_arguments_that_are_not_a_map() {
        // Arrange / Act
        val parsed = Settings.parse("unmetered")

        // Assert
        assertEquals(Settings.Parsed.Invalid(Fields.UNMETERED_ONLY), parsed)
    }
}
