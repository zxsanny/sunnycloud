package org.sunnycloud.backup

import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import uniffi.mobile_backup.AccessLevel
import uniffi.mobile_backup.BackupCoreInterface
import uniffi.mobile_backup.BackupStatus
import uniffi.mobile_backup.ServerCheck
import uniffi.mobile_backup.ServerTrust
import uniffi.mobile_backup.SkippedCounts

fun emptyStatus() = BackupStatus(
    visible = 0UL, uploaded = 0UL, pending = 0UL, paused = null,
    skipped = SkippedCounts(gone = 0UL, name = 0UL, size = 0UL, failed = 0UL),
    lastError = null, accessLevel = AccessLevel.FULL, locationRedacted = false,
)

/** A core whose status and `setServer` answer are set by the test. */
class FakeCore(
    var status: BackupStatus = emptyStatus(),
    var onSetServer: suspend (String) -> ServerCheck = { ServerCheck(ServerTrust.SYSTEM) },
    var onStatus: (() -> Unit)? = null,
) : BackupCoreInterface {
    override fun cancel() = Unit

    override suspend fun setServer(url: String): ServerCheck = onSetServer(url)

    override fun status(): BackupStatus {
        onStatus?.invoke()
        return status
    }
}

open class FakeState(
    override val signedIn: Boolean = false,
    override val deviceLabel: String? = null,
    override val running: Boolean = false,
    override val batteryExempt: Boolean = false,
) : NativeState

class FakePlatform : Platform {
    val logged = mutableListOf<Throwable>()

    override fun runOnMain(block: () -> Unit) = block()

    override fun logError(message: String, cause: Throwable) {
        logged += cause
    }
}

/** Everything sent to Dart, in order: events, successful results, and errors as `code`, `message`, `details`. */
class Capture {
    val events = mutableListOf<Any?>()
    val errors = mutableListOf<Triple<String, String?, Any?>>()
    val results = mutableListOf<Any?>()

    /** Every key and value sent, flattened to one text, for the sentinel scan. */
    fun everything(): String = (events + results + errors.map { listOf(it.first, it.second, it.third) }).joinToString("\n") { flatten(it) }

    /** Every map key sent, at any depth. */
    fun keys(): Set<String> = (events + results + errors.map { it.third }).flatMap { keysOf(it) }.toSet()

    private fun flatten(value: Any?): String = when (value) {
        is Map<*, *> -> value.entries.joinToString(" ") { "${it.key}=${flatten(it.value)}" }
        is Iterable<*> -> value.joinToString(" ") { flatten(it) }
        else -> value.toString()
    }

    private fun keysOf(value: Any?): List<String> = when (value) {
        is Map<*, *> -> value.entries.flatMap { listOf(it.key.toString()) + keysOf(it.value) }
        is Iterable<*> -> value.flatMap { keysOf(it) }
        else -> emptyList()
    }
}

class RecordingSink(private val capture: Capture) : EventChannel.EventSink {
    override fun success(event: Any?) {
        capture.events += event
    }

    override fun error(errorCode: String, errorMessage: String?, errorDetails: Any?) {
        capture.errors += Triple(errorCode, errorMessage, errorDetails)
    }

    override fun endOfStream() = Unit
}

/** A reply that can be awaited, because `setServer` answers from a Rust thread. */
class RecordingResult(private val capture: Capture) : MethodChannel.Result {
    private val done = CountDownLatch(1)
    var notImplemented = false
        private set

    override fun success(result: Any?) {
        capture.results += result
        done.countDown()
    }

    override fun error(errorCode: String, errorMessage: String?, errorDetails: Any?) {
        capture.errors += Triple(errorCode, errorMessage, errorDetails)
        done.countDown()
    }

    override fun notImplemented() {
        notImplemented = true
        done.countDown()
    }

    fun await() = check(done.await(10, TimeUnit.SECONDS)) { "no reply within 10 s" }
}

/** A channel over fakes, already subscribed; `capture` holds everything it sends. */
class Harness(
    val core: BackupCoreInterface = FakeCore(),
    val state: NativeState = FakeState(),
    val platform: FakePlatform = FakePlatform(),
    val settings: SettingsStore,
) {
    val capture = Capture()
    val channel = StatusChannel(core, settings, state, platform)

    fun subscribe() = channel.onListen(null, RecordingSink(capture))

    /** Sends a command and waits for its reply. */
    fun send(method: String, arguments: Any?): RecordingResult {
        val result = RecordingResult(capture)
        channel.onMethodCall(MethodCall(method, arguments), result)
        result.await()
        return result
    }
}
