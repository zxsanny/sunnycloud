package org.sunnycloud.backup

import android.app.Application
import android.util.Log
import uniffi.mobile_backup.libraryVersion

/** The app runs in one process; the library is loaded and the trust store initialised here, before any request. */
class BackupApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        if (!TlsTrustBridge.nativeInit(this)) {
            Log.e(TAG, "the native TLS trust store could not be initialised; connections will be refused")
        }
        Log.i(TAG, "mobile_backup ${libraryVersion()} loaded")
    }

    private companion object {
        const val TAG = "BackupApplication"
    }
}
