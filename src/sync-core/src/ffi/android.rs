//! The Android trust-store initialisation (owner decision 2026-11-13: an exported JNI function, not a UniFFI
//! call). The app declares, in the class that loads the native library:
//!
//! ```kotlin
//! package org.sunnycloud.backup
//!
//! object TlsTrustBridge {
//!     /** Call once per process, before the first request, after `System.loadLibrary`. */
//!     @JvmStatic external fun nativeInit(context: android.content.Context): Boolean
//! }
//! ```
//!
//! and calls `TlsTrustBridge.nativeInit(applicationContext)` at process start. It answers `true` once the
//! verifier holds the JVM, the context and its class loader; a second call is a no-op answering `true`.
//! `false` means the init failed (logged); `connect` then keeps answering `TrustStoreUninitialized`.

use jni::objects::{JClass, JObject};
use jni::sys::jboolean;
use jni::{EnvUnowned, Outcome};

use crate::trust_store;

/// JNI symbol of `org.sunnycloud.backup.TlsTrustBridge.nativeInit(Context): Boolean` (static).
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_sunnycloud_backup_TlsTrustBridge_nativeInit<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) -> jboolean {
    trust_store::init_once(trust_store::process_flag(), || {
        // `with_env` catches a panic of the closure, so nothing unwinds into the JVM.
        let outcome = env
            .with_env(|env| -> Result<(), jni::errors::Error> {
                rustls_platform_verifier::android::init_with_env(env, context)
            })
            .into_outcome();
        match outcome {
            Outcome::Ok(()) => true,
            Outcome::Err(error) => {
                tracing::error!(%error, "trust store initialisation failed");
                false
            }
            Outcome::Panic(_) => {
                tracing::error!("trust store initialisation panicked");
                false
            }
        }
    })
}
