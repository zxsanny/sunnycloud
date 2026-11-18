package org.sunnycloud.backup

/** Channel and command names of `contracts/17_android_app/status_channel.md`; the Dart side keeps the same strings. */
object Channels {
    const val STATUS = "org.sunnycloud.backup/status"
    const val COMMANDS = "org.sunnycloud.backup/commands"
    const val SET_SERVER = "setServer"
    const val UPDATE_SETTINGS = "updateSettings"
}

/**
 * Every field name that crosses the channel, in events, command arguments, results and error details. Nothing outside
 * this list may be sent, and none of these may name a token, code or verifier (the schema check of AZ-4000 reads this
 * object by reflection).
 */
object Fields {
    const val SIGNED_IN = "signed_in"
    const val SERVER_URL = "server_url"
    const val SERVER_TRUST = "server_trust"
    const val DEVICE_LABEL = "device_label"
    const val RUNNING = "running"
    const val PAUSED = "paused"
    const val VISIBLE = "visible"
    const val UPLOADED = "uploaded"
    const val PENDING = "pending"
    const val SKIPPED = "skipped"
    const val GONE = "gone"
    const val NAME = "name"
    const val SIZE = "size"
    const val FAILED = "failed"
    const val LAST_ERROR = "last_error"
    const val MESSAGE = "message"
    const val CORRELATION_ID = "correlation_id"
    const val ACCESS_LEVEL = "access_level"
    const val LOCATION_REDACTED = "location_redacted"
    const val BATTERY_EXEMPT = "battery_exempt"
    const val SETTINGS = "settings"
    const val UNMETERED_ONLY = "unmetered_only"
    const val CHARGING_ONLY = "charging_only"
    const val BUCKETS = "buckets"
    const val URL = "url"
    const val TRUST = "trust"
    const val FIELD = "field"
}
