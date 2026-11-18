package org.sunnycloud.backup

import java.io.File
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.util.Properties

/** The user settings of the app. `buckets` is the list of selected albums; `null` means every album (the default). */
data class Settings(
    val unmeteredOnly: Boolean = true,
    val chargingOnly: Boolean = false,
    val buckets: List<String>? = null,
) {
    fun toWire(): Map<String, Any?> = mapOf(
        Fields.UNMETERED_ONLY to unmeteredOnly,
        Fields.CHARGING_ONLY to chargingOnly,
        Fields.BUCKETS to buckets,
    )

    companion object {
        /** The settings in the arguments of an `updateSettings` call, or the first invalid field. A missing `buckets` means all. */
        fun parse(arguments: Any?): Parsed {
            val map = arguments as? Map<*, *> ?: return Parsed.Invalid(Fields.UNMETERED_ONLY)
            val unmetered = map[Fields.UNMETERED_ONLY] as? Boolean ?: return Parsed.Invalid(Fields.UNMETERED_ONLY)
            val charging = map[Fields.CHARGING_ONLY] as? Boolean ?: return Parsed.Invalid(Fields.CHARGING_ONLY)
            val raw = map[Fields.BUCKETS]
            val buckets = when {
                raw == null -> null
                raw is List<*> && raw.all { it is String && it.isNotBlank() } -> raw.map { it as String }.distinct()
                else -> return Parsed.Invalid(Fields.BUCKETS)
            }
            return Parsed.Valid(Settings(unmetered, charging, buckets))
        }
    }

    sealed interface Parsed {
        data class Valid(val settings: Settings) : Parsed
        data class Invalid(val field: String) : Parsed
    }
}

/**
 * The settings, kept in one file of the app-private directory so that the channel, the WorkManager constraints (c10)
 * and the asset source (c09) read the same values and a restart keeps them. A write replaces the file whole.
 */
class SettingsStore(private val file: File) {
    @Synchronized
    fun read(): Settings {
        if (!file.exists()) return Settings()
        val properties = Properties()
        file.inputStream().use { properties.load(it) }
        val count = properties.getProperty(BUCKET_COUNT)?.toInt()
        return Settings(
            unmeteredOnly = properties.getProperty(Fields.UNMETERED_ONLY).toBoolean(),
            chargingOnly = properties.getProperty(Fields.CHARGING_ONLY).toBoolean(),
            buckets = count?.let { n -> (0 until n).map { properties.getProperty("$BUCKET_PREFIX$it") } },
        )
    }

    @Synchronized
    fun write(settings: Settings) {
        val properties = Properties()
        properties.setProperty(Fields.UNMETERED_ONLY, settings.unmeteredOnly.toString())
        properties.setProperty(Fields.CHARGING_ONLY, settings.chargingOnly.toString())
        settings.buckets?.let { buckets ->
            properties.setProperty(BUCKET_COUNT, buckets.size.toString())
            buckets.forEachIndexed { i, name -> properties.setProperty("$BUCKET_PREFIX$i", name) }
        }
        val temporary = File(file.parentFile, file.name + ".tmp")
        temporary.outputStream().use { properties.store(it, null) }
        Files.move(temporary.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
    }

    private companion object {
        const val BUCKET_COUNT = "bucket_count"
        const val BUCKET_PREFIX = "bucket."
    }
}
