package org.sunnycloud.backup

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.util.Log

/** What the status channel needs from Android that a JVM test cannot have. */
interface Platform {
    /** Runs `block` on the main thread, at once when already there; Flutter's channels are main-thread only. */
    fun runOnMain(block: () -> Unit)

    fun logError(message: String, cause: Throwable)
}

/**
 * The native state that is not in the checkpoint. Later tasks fill it: the credential store (AZ-3992) answers
 * `signedIn` and `deviceLabel`, the run entry points (AZ-3994) answer `running`. It never carries a token.
 */
interface NativeState {
    val signedIn: Boolean
    val deviceLabel: String?
    val running: Boolean
    val batteryExempt: Boolean
}

class AndroidPlatform : Platform {
    private val main = Handler(Looper.getMainLooper())

    override fun runOnMain(block: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) block() else main.post(block)
    }

    override fun logError(message: String, cause: Throwable) {
        Log.e(TAG, message, cause)
    }

    private companion object {
        const val TAG = "StatusChannel"
    }
}

/** No sign-in and no run exist in the app yet (AZ-3992, AZ-3994), so those three answers are fixed until they land. */
class AndroidNativeState(context: Context) : NativeState {
    private val power = context.getSystemService(PowerManager::class.java)
    private val packageName = context.packageName

    override val signedIn = false
    override val deviceLabel: String? = null
    override val running = false
    override val batteryExempt get() = power.isIgnoringBatteryOptimizations(packageName)
}
