//! The visitor's content request: the same lookup by token hash as `open`, then one `resolve_live_file` read of the
//! target (the dummy target for a link that is not live), then the decision in code. Nothing is cached (owner
//! decision 2026-11-14): every call re-reads the link and its file, so a revoke, an expiry or a delete answers
//! `NotFound` on the next call and a save in place returns the new current revision at once. Only a call that
//! starts a download and is allowed writes `share_accessed`, best effort.

use std::net::IpAddr;

use contracts::{
    AccessGrant, LiveFileLookup, NodeKind, PublicShareError, ShareContent, TreeReaderError,
};
use sc_audit::{AuditDetail, AuditEvent, AuditEventKind, SubjectRef};
use sc_ids::{RawToken, sha256_token};
use uuid::Uuid;

use crate::lookup_repo::{self, LinkRow};
use crate::public::{DUMMY_TARGET, live_link, run_unexpected};
use crate::service::{PgShareService, SHARE_LINK};

impl PgShareService {
    /// `PublicShareService::authorize_content` (the port is implemented once its other operations exist): the
    /// link's account, node and the node's current revision, or `NotFound` for every dead link and every gone or
    /// folder target, or `PasswordRequired` for a live password link on a live file. Grants are not verified yet
    /// (E-16/c05 adds them), so a presented grant counts as absent. `client_ip` is for the invalid-token limit
    /// (E-16/c07). With `starts_download`, an allowed call writes one `share_accessed` event; its failure is
    /// logged at ERROR by the writer and never changes the answer.
    pub async fn authorize_content(
        &self,
        token: &RawToken,
        _grant: Option<&AccessGrant>,
        _client_ip: IpAddr,
        starts_download: bool,
    ) -> Result<ShareContent, PublicShareError> {
        let now = self.clock.now_utc();
        let row = lookup_repo::by_token_hash(&self.db, &sha256_token(token.expose()))
            .await
            .map_err(run_unexpected)?;
        let link = live_link(row, now);
        let (account, node) = link.map_or(DUMMY_TARGET, |row| (row.account_id, row.node_id));
        let file = self.tree.resolve_live_file(account, node).await;
        let content = decide_content(link.as_ref(), current_revision(file)?)?;
        if let (true, Some(link)) = (starts_download, link) {
            self.audit
                .record_best_effort(access_event(&link, content.revision_id))
                .await;
        }
        Ok(content)
    }
}

/// The current revision of a live file; `None` for a gone node, a folder and the `NotFound` of the dummy read. A
/// failure of the read is `Unexpected`.
pub(crate) fn current_revision(
    file: Result<LiveFileLookup, TreeReaderError>,
) -> Result<Option<Uuid>, PublicShareError> {
    match file {
        Ok(LiveFileLookup::Found(found)) if found.kind == NodeKind::File => {
            Ok(found.revision.map(|revision| revision.id))
        }
        Ok(LiveFileLookup::Found(_) | LiveFileLookup::Gone) | Err(TreeReaderError::NotFound) => {
            Ok(None)
        }
        Err(other) => Err(PublicShareError::Unexpected(Box::new(other))),
    }
}

/// The answer from the live link (if any) and its target's current revision (if live).
pub(crate) fn decide_content(
    link: Option<&LinkRow>,
    revision: Option<Uuid>,
) -> Result<ShareContent, PublicShareError> {
    match (link, revision) {
        (Some(link), Some(_)) if link.has_password => Err(PublicShareError::PasswordRequired),
        (Some(link), Some(revision_id)) => Ok(ShareContent {
            account_id: link.account_id,
            node_id: link.node_id,
            revision_id,
        }),
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
    use contracts::{LiveFile, RevisionView};
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

    fn found(kind: NodeKind) -> LiveFileLookup {
        LiveFileLookup::Found(LiveFile {
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
            blob_ref: None,
        })
    }

    #[test]
    fn only_a_live_file_has_a_current_revision_and_a_failed_read_is_unexpected() {
        // Arrange
        let failure = TreeReaderError::Unexpected(Box::new(DbError::Unavailable));

        // Act
        let file = current_revision(Ok(found(NodeKind::File))).expect("read");
        let folder = current_revision(Ok(found(NodeKind::Folder))).expect("read");
        let gone = current_revision(Ok(LiveFileLookup::Gone)).expect("read");
        let missing = current_revision(Err(TreeReaderError::NotFound)).expect("read");
        let failed = current_revision(Err(failure));

        // Assert
        assert_eq!(file, Some(REVISION));
        assert_eq!((folder, gone, missing), (None, None, None));
        assert!(matches!(failed, Err(PublicShareError::Unexpected(_))));
    }

    #[test]
    fn a_live_link_on_a_live_file_is_allowed_or_asks_for_its_password_and_all_else_is_not_found() {
        // Arrange
        let (plain, protected) = (link(false), link(true));

        // Act
        let allowed = decide_content(Some(&plain), Some(REVISION));
        let password = decide_content(Some(&protected), Some(REVISION));
        let dead = [
            decide_content(None, Some(REVISION)),
            decide_content(None, None),
            decide_content(Some(&plain), None),
            decide_content(Some(&protected), None),
        ];

        // Assert
        assert_eq!(
            allowed.expect("allowed"),
            ShareContent {
                account_id: plain.account_id,
                node_id: plain.node_id,
                revision_id: REVISION,
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
