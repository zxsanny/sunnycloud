package org.sunnycloud.backup

import android.content.Context

/**
 * The one place that loads the `mobile_backup` library with `System.loadLibrary` (UniFFI reaches the same file through
 * JNA by its name). The JNI symbol `Java_org_sunnycloud_backup_TlsTrustBridge_nativeInit` lives in sync-core
 * (`ffi/android.rs`), so this class and its package must not be renamed.
 */
object TlsTrustBridge {
    init {
        System.loadLibrary("mobile_backup")
    }

    /** Gives the rustls platform verifier the JVM and the context; true once it holds them. Idempotent. */
    @JvmStatic
    external fun nativeInit(context: Context): Boolean
}
