package org.sunnycloud.backup

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    private val statusChannel get() = (application as BackupApplication).statusChannel

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        statusChannel.register(flutterEngine.dartExecutor.binaryMessenger)
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        statusChannel.unregister()
        super.cleanUpFlutterEngine(flutterEngine)
    }
}
