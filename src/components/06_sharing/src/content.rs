//! The visitor's content and derived-file requests: the same lookup by token hash as `open`, then one
//! `resolve_live_file` read of the target (the dummy target for a link that is not live), then the decision in code.
//! Both routes run the one `allowed_file` path, so a thumbnail or preview answers exactly as the file does. Nothing
//! is cached (owner decision 2026-11-14): every call re-reads the link and its file, so a revoke, an expiry or a
//! delete answers `NotFound` on the next call and a save in place returns the new current revision at once. Only a
//! content call that starts a download and is allowed writes `share_accessed`, best effort; a derived file is never
//! a download start. The invalid-token limit is `open`'s, on the same per-address counter (06 §3): checked before
//! the lookup, and every `NotFound` counts.

use std::net::IpAddr;

use contracts::{
    AccessGrant, LiveFile, LiveFileLookup, NodeKind, PublicShareError, ShareContent, ShareDerived,
    TreeReaderError,
};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, SubjectRef};
use sc_ids::{RawToken, sha256_token};
use uuid::Uuid;

use crate::lookup_repo::{self, LinkRow};
use crate::public::{DUMMY_TARGET, live_link, run_unexpected};
use crate::service::{PgShareService, SHARE_LINK};

/// A live file with its current revision: what a live link may serve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedFile {
    revision_id: Uuid,
    file: LiveFile,
}

/// A live link on a live file whose password, if any, a valid grant of its own has answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Allowed {
    link: LinkRow,
    target: SharedFile,
}

impl Allowed {
    fn content(&self) -> ShareContent {
        ShareContent {
            account_id: self.link.account_id,
            node_id: self.link.node_id,
            revision_id: self.target.revision_id,
        }
    }

    fn derived(self) -> ShareDerived {
        ShareDerived {
            account_id: self.link.account_id,
            file: self.target.file,
        }
    }
}

impl PgShareService {
    /// `PublicShareService::authorize_content`: the link's account, node and the node's current revision, with the
    /// answers of `allowed_file`. With `starts_download`, an allowed call writes one `share_accessed` event; its
    /// failure is logged at ERROR by the writer and never changes the answer.
    pub(crate) async fn authorize_link_content(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
        starts_download: bool,
    ) -> Result<ShareContent, PublicShareError> {
        let allowed = self.allowed_file(token, grant, client_ip).await?;
        if starts_download {
            self.audit
                .record_best_effort(access_event(&allowed.link, allowed.target.revision_id))
                .await;
        }
        Ok(allowed.content())
    }

    /// `PublicShareService::authorize_derived`: the link's account and its live file (the argument of
    /// `DerivedOpener::open_derived_for`), with the answers of `allowed_file`. Nothing is audited: a thumbnail or a
    /// preview is not a download start.
    pub(crate) async fn authorize_link_derived(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareDerived, PublicShareError> {
        Ok(self.allowed_file(token, grant, client_ip).await?.derived())
    }

    /// The live link and its live file, or `NotFound` for every dead link and every gone or folder target, or
    /// `PasswordRequired` for a live password link on a live file without a valid grant of its own (an expired,
    /// altered, empty, oversized or other link's grant counts as absent). A link without a password ignores grants.
    /// A `NotFound` counts against `client_ip`'s invalid tokens, and the answer is `RateLimited` before any read
    /// while that address is limited (AC-20).
    async fn allowed_file(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<Allowed, PublicShareError> {
        let ip = client_ip.to_canonical();
        self.check_token_ip(ip)?;
        let now = self.clock.now_utc();
        let token_hash = sha256_token(token.expose());
        let row = lookup_repo::by_token_hash(&self.db, &token_hash)
            .await
            .map_err(run_unexpected)?;
        let link = live_link(row, now);
        let (account, node) = link.map_or(DUMMY_TARGET, |row| (row.account_id, row.node_id));
        let file = self.tree.resolve_live_file(account, node).await;
        let granted = grant.is_some_and(|grant| self.grants.accepts(&token_hash, grant, now));
        let decided = decide_access(link, shared_file(file)?, granted);
        if let Err(PublicShareError::NotFound) = decided {
            self.count_invalid_token(ip).await;
        }
        decided
    }
}

/// A live file with its current revision; `None` for a gone node, a folder and the `NotFound` of the dummy read. A
/// failure of the read is `Unexpected`.
pub(crate) fn shared_file(
    file: Result<LiveFileLookup, TreeReaderError>,
) -> Result<Option<SharedFile>, PublicShareError> {
    match file {
        Ok(LiveFileLookup::Found(found)) if found.kind == NodeKind::File => {
            let revision_id = found.revision.as_ref().map(|revision| revision.id);
            Ok(revision_id.map(|revision_id| SharedFile {
                revision_id,
                file: found,
            }))
        }
        Ok(LiveFileLookup::Found(_) | LiveFileLookup::Gone) | Err(TreeReaderError::NotFound) => {
            Ok(None)
        }
        Err(other) => Err(PublicShareError::Unexpected(Box::new(other))),
    }
}

/// The answer from the live link (if any), its live target (if any) and whether a valid grant of this link was
/// presented.
pub(crate) fn decide_access(
    link: Option<LinkRow>,
    target: Option<SharedFile>,
    granted: bool,
) -> Result<Allowed, PublicShareError> {
    match (link, target) {
        (Some(link), Some(_)) if link.has_password && !granted => {
            Err(PublicShareError::PasswordRequired)
        }
        (Some(link), Some(target)) => Ok(Allowed { link, target }),
        _ => Err(PublicShareError::NotFound),
    }
}

/// Ids only: no token, file name, password or address. The visitor has no account, so there is no actor.
pub(crate) fn access_event(link: &LinkRow, revision: Uuid) -> AuditEvent {
    AuditEvent {
        subject: Some(SubjectRef {
            subject_type: SHARE_LINK,
            id: link.id,
        }),
        detail: AuditDetail::new()
            .with_id("account_id", link.account_id)
            .with_id("node_id", link.node_id)
            .with_id("revision_id", revision),
        ..AuditEvent::new(AuditEventKind::ShareAccessed)
    }
}

#[cfg(test)]
mod tests {
    use contracts::RevisionView;
    use sc_clock::Timestamp;
    use sc_db::DbError;
    use sc_ids::FileHash;

    use super::*;

    const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);
    const REVISION: Uuid = Uuid::from_u128(3);

    fn link(has_password: bool) -> LinkRow {
        LinkRow {
            id: Uuid::from_u128(8),
            account_id: Uuid::from_u128(1),
            node_id: Uuid::from_u128(2),
            has_password,
            expires_at: NOW,
            revoked: false,
        }
    }

    fn live_file(kind: NodeKind) -> LiveFile {
        LiveFile {
            kind,
            revision: (kind == NodeKind::File).then(|| RevisionView {
                id: REVISION,
                node_id: Uuid::from_u128(2),
                blob_id: Uuid::from_u128(4),
                size: 1,
                content_hash: FileHash::from_bytes([7; 32]),
                blocklist: None,
                media_type: None,
                client_mtime: None,
                client_meta: None,
                created_at: NOW,
            }),
            blob_ref: Some(Uuid::from_u128(4)),
            vault: false,
        }
    }

    fn target() -> Option<SharedFile> {
        shared_file(Ok(LiveFileLookup::Found(live_file(NodeKind::File)))).expect("read")
    }

    #[test]
    fn only_a_live_file_has_a_current_revision_and_a_failed_read_is_unexpected() {
        // Arrange
        let failure = TreeReaderError::Unexpected(Box::new(DbError::Unavailable));
        let found = |kind| Ok(LiveFileLookup::Found(live_file(kind)));

        // Act
        let file = shared_file(found(NodeKind::File)).expect("read");
        let folder = shared_file(found(NodeKind::Folder)).expect("read");
        let gone = shared_file(Ok(LiveFileLookup::Gone)).expect("read");
        let missing = shared_file(Err(TreeReaderError::NotFound)).expect("read");
        let failed = shared_file(Err(failure));

        // Assert
        assert_eq!(
            file,
            Some(SharedFile {
                revision_id: REVISION,
                file: live_file(NodeKind::File),
            })
        );
        assert_eq!((folder, gone, missing), (None, None, None));
        assert!(matches!(failed, Err(PublicShareError::Unexpected(_))));
    }

    #[test]
    fn a_live_link_on_a_live_file_is_allowed_or_asks_for_its_password_and_all_else_is_not_found() {
        // Arrange
        let (plain, protected) = (link(false), link(true));

        // Act
        let allowed = decide_access(Some(plain), target(), false);
        let password = decide_access(Some(protected), target(), false);
        let dead = [
            decide_access(None, target(), false),
            decide_access(None, None, false),
            decide_access(Some(plain), None, false),
            decide_access(Some(protected), None, false),
        ];

        // Assert
        let allowed = allowed.expect("allowed");
        assert_eq!(
            allowed.content(),
            ShareContent {
                account_id: plain.account_id,
                node_id: plain.node_id,
                revision_id: REVISION,
            }
        );
        assert_eq!(
            allowed.derived(),
            ShareDerived {
                account_id: plain.account_id,
                file: live_file(NodeKind::File),
            }
        );
        assert!(matches!(password, Err(PublicShareError::PasswordRequired)));
        for answer in dead {
            assert!(
                matches!(answer, Err(PublicShareError::NotFound)),
                "{answer:?}"
            );
        }
    }

    #[test]
    fn a_valid_grant_opens_a_password_link_only_while_it_and_its_file_are_live() {
        // Arrange
        let (plain, protected) = (link(false), link(true));

        // Act
        let granted = decide_access(Some(protected), target(), true);
        let plain_with_grant = decide_access(Some(plain), target(), true);
        let dead = [
            decide_access(None, target(), true),
            decide_access(Some(protected), None, true),
        ];

        // Assert
        let expected = ShareContent {
            account_id: protected.account_id,
            node_id: protected.node_id,
            revision_id: REVISION,
        };
        assert_eq!(granted.expect("granted").content(), expected);
        assert_eq!(plain_with_grant.expect("plain").content(), expected);
        for answer in dead {
            assert!(
                matches!(answer, Err(PublicShareError::NotFound)),
                "{answer:?}"
            );
        }
    }

    #[test]
    fn the_access_event_names_the_link_and_carries_ids_only() {
        // Arrange
        let row = link(false);

        // Act
        let event = access_event(&row, REVISION);

        // Assert
        assert_eq!(event.event, AuditEventKind::ShareAccessed);
        assert_eq!(
            event.subject,
            Some(SubjectRef {
                subject_type: SHARE_LINK,
                id: row.id
            })
        );
        assert_eq!((event.actor_account_id, event.ip), (None, None));
        let keys: Vec<&str> = event.detail.entries().iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, ["account_id", "node_id", "revision_id"]);
    }
}
