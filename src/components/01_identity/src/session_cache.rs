use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use contracts::{Principal, PrincipalKind};
use sc_clock::MonotonicInstant;
use sc_ids::TokenHash;
use uuid::Uuid;

struct Liveness {
    kind: PrincipalKind,
    alive: bool,
    stored_at: MonotonicInstant,
}

struct Authenticated {
    principal: Principal,
    stored_at: MonotonicInstant,
}

struct Entries {
    // `TokenHash` is not `Hash` (its equality is constant time); the digest bytes are the key. The map holds
    // SHA-256 of tokens, so its lookup timing tells an attacker nothing about a token.
    by_token: HashMap<[u8; 32], Authenticated>,
    liveness: HashMap<Uuid, Liveness>,
    swept_at: MonotonicInstant,
}

/// Results of the authenticator's database reads. An entry is served only while it is younger than `ttl`
/// and is never invalidated any other way, so a revocation is visible after at most `ttl`. Only live
/// credentials are stored by token (an attacker's random tokens must not grow the map); a zero `ttl`
/// stores nothing.
pub(crate) struct SessionCache {
    ttl: Duration,
    entries: Mutex<Entries>,
}

impl SessionCache {
    pub(crate) fn new(ttl: Duration, now: MonotonicInstant) -> Self {
        SessionCache {
            ttl,
            entries: Mutex::new(Entries {
                by_token: HashMap::new(),
                liveness: HashMap::new(),
                swept_at: now,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn fresh(&self, stored_at: MonotonicInstant, now: MonotonicInstant) -> bool {
        now.saturating_duration_since(stored_at) < self.ttl
    }

    /// The principal stored for `hash`, if it is of `kind` (a session token presented as a device token is
    /// a different credential) and still fresh.
    pub(crate) fn principal(
        &self,
        hash: &TokenHash,
        kind: PrincipalKind,
        now: MonotonicInstant,
    ) -> Option<Principal> {
        let entries = self.lock();
        let entry = entries.by_token.get(hash.as_bytes())?;
        (entry.principal.kind == kind && self.fresh(entry.stored_at, now))
            .then(|| entry.principal.clone())
    }

    pub(crate) fn alive(&self, principal: &Principal, now: MonotonicInstant) -> Option<bool> {
        let entries = self.lock();
        let entry = entries.liveness.get(&principal.session_or_device_id)?;
        (entry.kind == principal.kind && self.fresh(entry.stored_at, now)).then_some(entry.alive)
    }

    pub(crate) fn store_principal(
        &self,
        hash: TokenHash,
        principal: &Principal,
        now: MonotonicInstant,
    ) {
        if self.ttl.is_zero() {
            return;
        }
        let mut entries = self.lock();
        self.sweep(&mut entries, now);
        entries.by_token.insert(
            hash.into_bytes(),
            Authenticated {
                principal: principal.clone(),
                stored_at: now,
            },
        );
        Self::put_liveness(&mut entries, principal, true, now);
    }

    pub(crate) fn store_liveness(&self, principal: &Principal, alive: bool, now: MonotonicInstant) {
        if self.ttl.is_zero() {
            return;
        }
        let mut entries = self.lock();
        self.sweep(&mut entries, now);
        Self::put_liveness(&mut entries, principal, alive, now);
    }

    fn put_liveness(
        entries: &mut Entries,
        principal: &Principal,
        alive: bool,
        now: MonotonicInstant,
    ) {
        entries.liveness.insert(
            principal.session_or_device_id,
            Liveness {
                kind: principal.kind,
                alive,
                stored_at: now,
            },
        );
    }

    /// Drops the entries nobody asked for again, at most once per `ttl`, so the maps hold only the last
    /// `ttl` of traffic.
    fn sweep(&self, entries: &mut Entries, now: MonotonicInstant) {
        if now.saturating_duration_since(entries.swept_at) < self.ttl {
            return;
        }
        entries
            .by_token
            .retain(|_, entry| self.fresh(entry.stored_at, now));
        entries
            .liveness
            .retain(|_, entry| self.fresh(entry.stored_at, now));
        entries.swept_at = now;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use contracts::{PrincipalKind, Role};
    use sc_clock::{Clock, ManualClock, Timestamp};
    use sc_ids::sha256_token;

    use super::*;

    const TTL: Duration = Duration::from_millis(200);

    fn principal(kind: PrincipalKind) -> Principal {
        Principal {
            account_id: Uuid::from_u128(1),
            kind,
            session_or_device_id: Uuid::from_u128(2),
            role: Role::User,
            label: "test".to_owned(),
        }
    }

    fn clock() -> ManualClock {
        ManualClock::new(Timestamp::from_unix_micros(0))
    }

    #[test]
    fn entry_is_served_until_it_is_ttl_old_and_never_after() {
        // Arrange
        let clock = clock();
        let cache = SessionCache::new(TTL, clock.monotonic());
        let hash = sha256_token("token");
        let session = principal(PrincipalKind::Session);
        cache.store_principal(hash, &session, clock.monotonic());

        // Act
        clock.tick(TTL - Duration::from_millis(1));
        let before_expiry = cache.principal(&hash, PrincipalKind::Session, clock.monotonic());
        clock.tick(Duration::from_millis(1));
        let at_expiry = cache.principal(&hash, PrincipalKind::Session, clock.monotonic());

        // Assert
        assert_eq!(before_expiry, Some(session));
        assert_eq!(at_expiry, None);
    }

    #[test]
    fn liveness_entry_older_than_ttl_is_never_served() {
        // Arrange
        let clock = clock();
        let cache = SessionCache::new(TTL, clock.monotonic());
        let device = principal(PrincipalKind::Device);
        cache.store_liveness(&device, true, clock.monotonic());

        // Act
        clock.tick(TTL);
        let served = cache.alive(&device, clock.monotonic());

        // Assert
        assert_eq!(served, None);
    }

    #[test]
    fn token_stored_for_a_session_is_not_served_to_a_device_credential() {
        // Arrange
        let clock = clock();
        let cache = SessionCache::new(TTL, clock.monotonic());
        let hash = sha256_token("token");
        cache.store_principal(hash, &principal(PrincipalKind::Session), clock.monotonic());

        // Act
        let served = cache.principal(&hash, PrincipalKind::Device, clock.monotonic());

        // Assert
        assert_eq!(served, None);
    }

    #[test]
    fn zero_ttl_stores_nothing() {
        // Arrange
        let clock = clock();
        let cache = SessionCache::new(Duration::ZERO, clock.monotonic());
        let hash = sha256_token("token");
        let session = principal(PrincipalKind::Session);

        // Act
        cache.store_principal(hash, &session, clock.monotonic());
        let served = cache.principal(&hash, PrincipalKind::Session, clock.monotonic());

        // Assert
        assert_eq!(served, None);
        assert!(cache.lock().by_token.is_empty());
    }

    #[test]
    fn entries_nobody_asked_for_again_are_dropped_after_one_ttl() {
        // Arrange
        let clock = clock();
        let cache = SessionCache::new(TTL, clock.monotonic());
        cache.store_principal(
            sha256_token("first"),
            &principal(PrincipalKind::Session),
            clock.monotonic(),
        );
        clock.tick(TTL);

        // Act
        cache.store_liveness(&principal(PrincipalKind::Device), true, clock.monotonic());

        // Assert
        assert!(cache.lock().by_token.is_empty());
    }
}
