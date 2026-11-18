# JNA loads its native dispatcher by reflection and the UniFFI bindings are JNA Structures and Callbacks.
-dontwarn java.awt.*
-keep class com.sun.jna.* { *; }
-keepclassmembers class * extends com.sun.jna.* { public *; }
-keep class uniffi.** { *; }
# sync-core exports Java_org_sunnycloud_backup_TlsTrustBridge_nativeInit; renaming the class would unlink it.
-keep class org.sunnycloud.backup.TlsTrustBridge { *; }
