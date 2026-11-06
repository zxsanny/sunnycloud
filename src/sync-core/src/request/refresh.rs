//! Single-flight rotation of a mobile session's tokens (`sync_api.md` § Operations, `host_traits.md` `on_rotated`).
//! One rotation runs at a time; every 401 that arrives meanwhile waits for it and repeats its request.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use sc_config::Secret;

use super::path::RequestPath;
use crate::error::SyncError;
use crate::host::PersistFailed;
use crate::token_ops;

/// The first store of a rotated pair plus one retry; a pair the host cannot keep is a lost session (R05).
const PERSIST_ATTEMPTS: u32 = 2;

impl RequestPath {
    /// Rotates the tokens unless a rotation finished after `seen` (the generation the failed request started
    /// under). Ends `Ok` only when the host stored the new pair; the next `bearer()` then carries the new access
    /// token. `Unauthorized` stops all work: a revoked or expired session, a device token, or a pair that could not
    /// be stored.
    pub(crate) async fn renew(&self, seen: u64) -> Result<(), SyncError> {
        let _turn = tokio::select! {
            biased;
            () = self.cancel.cancelled() => return Err(SyncError::Cancelled),
            turn = self.refresh_gate.write() => turn,
        };
        self.check()?;
        if self.generation.load(Ordering::Acquire) != seen {
            return Ok(());
        }
        let Some(refresh) = self.credentials.bearer().refresh else {
            tracing::warn!("device token refused, stopping work");
            self.stop_auth();
            return Err(SyncError::Unauthorized);
        };
        // Boxed: `run` awaits `renew`, which sends the rotation through `run` again.
        let tokens = match Box::pin(token_ops::request_refresh(self, &refresh)).await {
            Ok(tokens) => tokens,
            Err(SyncError::Unauthorized) => {
                tracing::warn!("session refresh refused, stopping work");
                self.stop_auth();
                return Err(SyncError::Unauthorized);
            }
            Err(error) => return Err(error),
        };
        let Some(next) = tokens.refresh else {
            return Err(SyncError::ProtocolViolation {
                detail: "refresh answer without a refresh token",
            });
        };
        self.persist(Secret::new(tokens.access), Secret::new(next))
            .await?;
        self.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// The `refresh()` operation: a rotation now, joined with one already running.
    pub(crate) async fn refresh_now(&self) -> Result<(), SyncError> {
        self.renew(self.generation.load(Ordering::Acquire)).await
    }

    /// `on_rotated` blocks on the host's secure store, so it runs on the blocking pool.
    async fn persist(
        &self,
        access: Secret<String>,
        refresh: Secret<String>,
    ) -> Result<(), SyncError> {
        for attempt in 1..=PERSIST_ATTEMPTS {
            let host = Arc::clone(&self.credentials);
            let (access, refresh) = (access.clone(), refresh.clone());
            match tokio::task::spawn_blocking(move || host.on_rotated(access, refresh)).await {
                Ok(Ok(())) => return Ok(()),
                Ok(Err(PersistFailed)) => {
                    tracing::error!(attempt, "host could not store the rotated tokens");
                }
                Err(error) => tracing::error!(
                    attempt,
                    panicked = error.is_panic(),
                    "storing the rotated tokens did not finish"
                ),
            }
        }
        self.stop_auth();
        Err(SyncError::Unauthorized)
    }
}
