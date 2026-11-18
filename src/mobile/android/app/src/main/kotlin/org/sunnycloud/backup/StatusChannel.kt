package org.sunnycloud.backup

import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import kotlin.coroutines.Continuation
import kotlin.coroutines.EmptyCoroutineContext
import kotlin.coroutines.startCoroutine
import uniffi.mobile_backup.BackupCoreInterface
import uniffi.mobile_backup.BackupException
import uniffi.mobile_backup.BackupStatus
import uniffi.mobile_backup.ServerTrust

/**
 * The Kotlin half of `contracts/17_android_app/status_channel.md`: the one holder of the `BackupCore` handle that
 * Flutter talks to. Every event is a full snapshot read from the core when it is sent; whoever changes the status
 * (a run, a sign-in, a setting) calls [publish]. No token, code or verifier is read here: the snapshot is built from
 * `BackupStatus`, the settings and [NativeState], and an unexpected failure reaches Dart as a code with no text.
 */
class StatusChannel(
    private val core: BackupCoreInterface,
    private val settings: SettingsStore,
    private val state: NativeState,
    private val platform: Platform,
) : MethodChannel.MethodCallHandler, EventChannel.StreamHandler {
    // Both are touched on the main thread only (see Platform.runOnMain).
    private var sink: EventChannel.EventSink? = null
    private var server: Pair<String, ServerTrust>? = null
    private var channels: Pair<MethodChannel, EventChannel>? = null

    fun register(messenger: BinaryMessenger) {
        val methods = MethodChannel(messenger, Channels.COMMANDS).also { it.setMethodCallHandler(this) }
        val events = EventChannel(messenger, Channels.STATUS).also { it.setStreamHandler(this) }
        channels = methods to events
    }

    fun unregister() {
        channels?.let { (methods, events) ->
            methods.setMethodCallHandler(null)
            events.setStreamHandler(null)
        }
        channels = null
        sink = null
    }

    /** Sends one full snapshot to the subscriber, if there is one. Safe from any thread. */
    fun publish() = platform.runOnMain {
        val events = sink ?: return@runOnMain
        try {
            events.success(snapshot())
        } catch (e: Exception) {
            val failure = failureOf(e)
            events.error(failure.code, failure.message, failure.details)
        }
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        sink = events
        publish()
    }

    override fun onCancel(arguments: Any?) {
        sink = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            Channels.SET_SERVER -> setServer(call.arguments, result)
            Channels.UPDATE_SETTINGS -> updateSettings(call.arguments, result)
            else -> result.notImplemented()
        }
    }

    private fun setServer(arguments: Any?, result: MethodChannel.Result) {
        val url = (arguments as? Map<*, *>)?.get(Fields.URL) as? String
        if (url == null) {
            reply(result, Failure("Invalid", details = mapOf(Fields.FIELD to Fields.URL)))
            return
        }
        // setServer is async; its continuation resumes on a Rust thread, so the reply goes back through the main thread.
        suspend { core.setServer(url) }.startCoroutine(Continuation(EmptyCoroutineContext) { outcome ->
            platform.runOnMain {
                outcome.fold(
                    onSuccess = { check ->
                        server = url to check.trust
                        result.success(mapOf(Fields.TRUST to check.trust.toWire()))
                        publish()
                    },
                    onFailure = { reply(result, failureOf(it)) },
                )
            }
        })
    }

    private fun updateSettings(arguments: Any?, result: MethodChannel.Result) {
        when (val parsed = Settings.parse(arguments)) {
            is Settings.Parsed.Invalid -> reply(result, Failure("Invalid", details = mapOf(Fields.FIELD to parsed.field)))
            is Settings.Parsed.Valid -> try {
                settings.write(parsed.settings)
                result.success(settings.read().toWire())
                publish()
            } catch (e: Exception) {
                reply(result, failureOf(e))
            }
        }
    }

    private fun snapshot(): Map<String, Any?> {
        val status = core.status()
        return mapOf(
            Fields.SIGNED_IN to state.signedIn,
            Fields.SERVER_URL to server?.first,
            Fields.SERVER_TRUST to server?.second?.toWire(),
            Fields.DEVICE_LABEL to state.deviceLabel,
            Fields.RUNNING to state.running,
            Fields.PAUSED to status.paused?.name?.toPascalCase(),
            Fields.VISIBLE to status.visible.toLong(),
            Fields.UPLOADED to status.uploaded.toLong(),
            Fields.PENDING to status.pending.toLong(),
            Fields.SKIPPED to skipped(status),
            Fields.LAST_ERROR to status.lastError?.let {
                mapOf(Fields.MESSAGE to it.message, Fields.CORRELATION_ID to it.correlationId)
            },
            Fields.ACCESS_LEVEL to status.accessLevel.name.lowercase(),
            Fields.LOCATION_REDACTED to status.locationRedacted,
            Fields.BATTERY_EXEMPT to state.batteryExempt,
            Fields.SETTINGS to settings.read().toWire(),
        )
    }

    private fun skipped(status: BackupStatus) = mapOf(
        Fields.GONE to status.skipped.gone.toLong(),
        Fields.NAME to status.skipped.name.toLong(),
        Fields.SIZE to status.skipped.size.toLong(),
        Fields.FAILED to status.skipped.failed.toLong(),
    )

    private fun reply(result: MethodChannel.Result, failure: Failure) =
        result.error(failure.code, failure.message, failure.details)

    /** The error names of the contract; `message` is set only where the user is shown server text. */
    private class Failure(val code: String, val message: String? = null, val details: Map<String, Any?>? = null)

    private fun failureOf(e: Throwable): Failure = when (e) {
        is BackupException -> when (e) {
            is BackupException.NotSignedIn -> Failure("NotSignedIn")
            is BackupException.ServerUnreachable -> Failure("ServerUnreachable")
            is BackupException.UpgradeRequired ->
                Failure("UpgradeRequired", e.userMessage, mapOf(Fields.MESSAGE to e.userMessage))
            is BackupException.TokenRejected -> Failure("TokenRejected")
            is BackupException.InvalidCode -> Failure("InvalidCode")
            is BackupException.HttpsRequired -> Failure("HttpsRequired")
            is BackupException.InvalidServerUrl -> Failure("InvalidServerUrl")
            is BackupException.CheckpointUnavailable -> Failure("CheckpointUnavailable")
        }
        else -> {
            platform.logError("unexpected failure on the status channel", e)
            Failure("Unexpected")
        }
    }
}

/** `QUOTA_EXCEEDED` is sent as `QuotaExceeded`, the variant name of the contract. */
private fun String.toPascalCase() = split('_').joinToString("") { it.lowercase().replaceFirstChar(Char::uppercase) }

/** `USER_ADDED` is sent as `user_added`. */
private fun ServerTrust.toWire() = name.lowercase()
