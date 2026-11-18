package org.sunnycloud.backup

import android.app.Application
import android.util.Log
import java.io.File
import java.nio.file.Files
import uniffi.mobile_backup.BackupCore
import uniffi.mobile_backup.libraryVersion

/**
 * The app runs in one process; the library is loaded and the trust store initialised here, before any request.
 * The process also owns the one `BackupCore` handle (ADR-013) and the status channel that Flutter talks to.
 */
class BackupApplication : Application() {
    val core: BackupCore by lazy {
        val directory = Files.createDirectories(File(filesDir, "backup").toPath())
        val version = checkNotNull(packageManager.getPackageInfo(packageName, 0).versionName) { "versionName is not set" }
        BackupCore(directory.toString(), version)
    }

    val settings: SettingsStore by lazy { SettingsStore(File(filesDir, "settings.properties")) }

    val statusChannel: StatusChannel by lazy { StatusChannel(core, settings, AndroidNativeState(this), AndroidPlatform()) }

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
