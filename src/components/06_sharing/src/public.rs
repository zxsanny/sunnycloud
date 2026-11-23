//! The visitor's `open`: one lookup by token hash, then the same two target reads in every case, then the decision
//! in code. A link that is missing, expired or revoked (and every malformed token, which simply hashes to a value
//! no row holds) reads a fixed dummy target instead of its own, so the five not-found cases run the same statements
//! and differ from each other only in values (AC-12, IT-06-9). No password is checked here, so no Argon2 runs.
//! The address is checked against the invalid-token limit before the lookup, and every `NotFound` counts on it
//! (`abuse`), so the not-found cases also do the same limiter work. The `PublicShareService` implementation here is
//! the one public entry of the four visitor operations.

use std::net::IpAddr;

use contracts::{
    AccessGrant, LiveFileLookup, NodeKind, NodeSelector, NodeState, NodeView, PublicShareError,
    PublicShareService, ShareContent, ShareDerived, ShareOpen, TreeReaderError, async_trait,
};
use sc_clock::Timestamp;
use sc_db::{DbError, RunError};
use sc_ids::{FileHash, RawToken, sha256_token};
use uuid::Uuid;

use crate::lookup_repo::{self, LinkRow};
use crate::service::PgShareService;

/// The target read for a link that is not live: an account and a node that no row has, read by primary key
/// through the same `TreeReader` calls as a live link's target.
pub(crate) const DUMMY_TARGET: (Uuid, Uuid) = (Uuid::nil(), Uuid::nil());

/// What the two target reads found, reduced to what a visitor may see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Live {
        name: String,
        size: i64,
        media_type: Option<String>,
        content_hash: FileHash,
    },
    Gone,
}

impl PgShareService {
    /// `PublicShareService::open`: `Available` with the name, the current revision's size and media type and the
    /// expiry; `PasswordRequired` for a live password link on a live file without a valid grant of its own (the
    /// rule of `content::allowed_file`: an expired, altered, empty, oversized or other link's grant counts as
    /// absent), with no file data; a link without a password ignores grants; `NotFound` for everything else, counted against `client_ip`'s invalid tokens; `RateLimited` before any read while that
    /// address is limited (AC-20).
    pub(crate) async fn open_link(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareOpen, PublicShareError> {
        let ip = client_ip.to_canonical();
        self.check_token_ip(ip)?;
        let now = self.clock.now_utc();
        let token_hash = sha256_token(token.expose());
        let row = lookup_repo::by_token_hash(&self.db, &token_hash)
            .await
            .map_err(run_unexpected)?;
        let link = live_link(row, now);
        let (account, node) = link.map_or(DUMMY_TARGET, |row| (row.account_id, row.node_id));
        // Both reads run whatever the first answers, so a gone target costs what a live one does.
        let file = self.tree.resolve_live_file(account, node).await;
        let view = self.tree.get_node(account, NodeSelector::Id(node)).await;
        let target = target_of(file, view)?;
        let granted = grant.is_some_and(|grant| self.grants.accepts(&token_hash, grant, now));
        let answer = decide(link.as_ref(), target, granted);
        if answer == ShareOpen::NotFound {
            self.count_invalid_token(ip).await;
        }
        Ok(answer)
    }
}

/// The port is the one public path of the visitor operations; each method is its module's implementation (`open`
/// here, `verify_password` in `password`, `authorize_content` and `authorize_derived` in `content`).
#[async_trait]
impl PublicShareService for PgShareService {
    async fn open(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareOpen, PublicShareError> {
        self.open_link(token, grant, client_ip).await
    }

    async fn verify_password(
        &self,
        token: &RawToken,
        password: &str,
        client_ip: IpAddr,
    ) -> Result<AccessGrant, PublicShareError> {
        self.verify_link_password(token, password, client_ip).await
    }

    async fn authorize_content(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
        starts_download: bool,
    ) -> Result<ShareContent, PublicShareError> {
        self.authorize_link_content(token, grant, client_ip, starts_download)
            .await
    }

    async fn authorize_derived(
        &self,
        token: &RawToken,
        grant: Option<&AccessGrant>,
        client_ip: IpAddr,
    ) -> Result<ShareDerived, PublicShareError> {
        self.authorize_link_derived(token, grant, client_ip).await
    }
}

/// The link when it is live: not revoked and `expires_at > now` (G-06-3); `expires_at <= now` is gone.
pub(crate) fn live_link(row: Option<LinkRow>, now: Timestamp) -> Option<LinkRow> {
    row.filter(|row| !row.revoked && row.expires_at > now)
}

/// A live file whose current revision is known and whose node is live in `get_node` too; every other answer,
/// including the `NotFound` of the dummy read, is `Gone`. A failure of either read is `Unexpected`.
pub(crate) fn target_of(
    file: Result<LiveFileLookup, TreeReaderError>,
    view: Result<NodeView, TreeReaderError>,
) -> Result<Target, PublicShareError> {
    let file = match file {
        Ok(file) => Some(file),
        Err(TreeReaderError::NotFound) => None,
        Err(other) => return Err(PublicShareError::Unexpected(Box::new(other))),
    };
    let view = match view {
        Ok(view) => Some(view),
        Err(TreeReaderError::NotFound) => None,
        Err(other) => return Err(PublicShareError::Unexpected(Box::new(other))),
    };
    let revision = match file {
        Some(LiveFileLookup::Found(found)) if found.kind == NodeKind::File => found.revision,
        _ => None,
    };
    Ok(match (revision, view) {
        (Some(revision), Some(view))
            if view.kind == NodeKind::File && view.state == NodeState::Live =>
        {
            Target::Live {
                name: view.name,
                size: revision.size,
                media_type: revision.media_type,
                content_hash: revision.content_hash,
            }
        }
        _ => Target::Gone,
    })
}

/// The answer from the live link (if any), its target and whether a valid grant of this link was presented.
pub(crate) fn decide(link: Option<&LinkRow>, target: Target, granted: bool) -> ShareOpen {
    match (link, target) {
        (Some(link), Target::Live { .. }) if link.has_password && !granted => {
            ShareOpen::PasswordRequired
        }
        (
            Some(link),
            Target::Live {
                name,
                size,
                media_type,
                content_hash,
            },
        ) => ShareOpen::Available {
            name,
            size,
            media_type,
            content_hash,
            expires_at: link.expires_at,
        },
        _ => ShareOpen::NotFound,
    }
}

pub(crate) fn run_unexpected(error: RunError<DbError>) -> PublicShareError {
    match error {
        RunError::Closure(inner) | RunError::Db(inner) => {
            PublicShareError::Unexpected(Box::new(inner))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::{LiveFile, RevisionView, ThumbState};

    const NOW: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);
    const MICRO: std::time::Duration = std::time::Duration::from_micros(1);

    fn row(expires_at: Timestamp, revoked: bool, has_password: bool) -> LinkRow {
        LinkRow {
            id: Uuid::from_u128(8),
            account_id: Uuid::from_u128(1),
            node_id: Uuid::from_u128(2),
            has_password,
            expires_at,
            revoked,
        }
    }

    fn later() -> Timestamp {
        NOW.checked_add(MICRO).expect("in range")
    }

    fn earlier() -> Timestamp {
        NOW.checked_sub(MICRO).expect("in range")
    }

    fn live_target() -> Target {
        Target::Live {
            name: "f.jpg".to_owned(),
            size: 5,
            media_type: Some("image/jpeg".to_owned()),
            content_hash: FileHash::from_bytes([7; 32]),
        }
    }

    fn revision(size: i64) -> RevisionView {
        RevisionView {
            id: Uuid::from_u128(3),
            node_id: Uuid::from_u128(2),
            blob_id: Uuid::from_u128(4),
            size,
            content_hash: FileHash::from_bytes([7; 32]),
            blocklist: None,
            media_type: Some("image/png".to_owned()),
            client_mtime: None,
            client_meta: None,
            created_at: NOW,
        }
    }

    fn view(kind: NodeKind, state: NodeState) -> NodeView {
        NodeView {
            id: Uuid::from_u128(2),
            parent_id: Some(Uuid::from_u128(9)),
            kind,
            name: "f.png".to_owned(),
            state,
            current_revision_id: None,
            size: Some(1),
            content_hash: None,
            media_type: Some("text/plain".to_owned()),
            client_mtime: None,
            created_at: NOW,
            updated_at: NOW,
            seq: 1,
            thumb_state: ThumbState::None,
            vault: false,
        }
    }

    fn found(kind: NodeKind, size: i64) -> LiveFileLookup {
        LiveFileLookup::Found(LiveFile {
            kind,
            revision: (kind == NodeKind::File).then(|| revision(size)),
            blob_ref: None,
            vault: false,
        })
    }

    #[test]
    fn every_combination_but_a_live_link_on_a_live_file_is_not_found() {
        // Arrange
        let rows = [
            None,
            Some(row(later(), true, false)),
            Some(row(later(), true, true)),
            Some(row(NOW, false, false)),
            Some(row(NOW, false, true)),
            Some(row(earlier(), false, false)),
            Some(row(earlier(), true, true)),
        ];
        let mut cases: Vec<(Option<LinkRow>, Target)> = rows
            .iter()
            .flat_map(|row| [(*row, live_target()), (*row, Target::Gone)])
            .collect();
        cases.push((Some(row(later(), false, false)), Target::Gone));
        cases.push((Some(row(later(), false, true)), Target::Gone));

        // Act
        let answers: Vec<ShareOpen> = cases
            .into_iter()
            .map(|(row, target)| decide(live_link(row, NOW).as_ref(), target, true))
            .collect();

        // Assert
        assert_eq!(answers.len(), 16);
        for answer in answers {
            assert_eq!(answer, ShareOpen::NotFound);
        }
    }

    #[test]
    fn a_live_link_on_a_live_file_is_available_or_asks_for_its_password() {
        // Arrange
        let plain = row(later(), false, false);
        let protected = row(later(), false, true);

        // Act
        let available = decide(live_link(Some(plain), NOW).as_ref(), live_target(), false);
        let password = decide(
            live_link(Some(protected), NOW).as_ref(),
            live_target(),
            false,
        );

        // Assert
        assert_eq!(
            available,
            ShareOpen::Available {
                name: "f.jpg".to_owned(),
                size: 5,
                media_type: Some("image/jpeg".to_owned()),
                content_hash: FileHash::from_bytes([7; 32]),
                expires_at: later(),
            }
        );
        assert_eq!(password, ShareOpen::PasswordRequired);
    }

    #[test]
    fn a_valid_grant_opens_a_password_link_and_no_grant_does_not() {
        // Arrange
        let protected = row(later(), false, true);

        // Act
        let granted = decide(Some(&protected), live_target(), true);
        let not_granted = decide(Some(&protected), live_target(), false);

        // Assert
        assert!(matches!(granted, ShareOpen::Available { .. }));
        assert_eq!(not_granted, ShareOpen::PasswordRequired);
    }

    #[test]
    fn available_has_exactly_name_size_media_type_hash_and_expiry() {
        // Arrange
        let answer = decide(Some(&row(later(), false, false)), live_target(), false);

        // Act
        // The pattern names every field without `..`: a sixth field fails to compile.
        let ShareOpen::Available {
            name,
            size,
            media_type,
            content_hash,
            expires_at,
        } = answer
        else {
            panic!("expected Available, got {answer:?}");
        };

        // Assert
        assert_eq!(
            (
                name.as_str(),
                size,
                media_type.as_deref(),
                content_hash,
                expires_at
            ),
            (
                "f.jpg",
                5,
                Some("image/jpeg"),
                FileHash::from_bytes([7; 32]),
                later()
            )
        );
    }

    #[test]
    fn the_target_is_live_only_for_a_live_file_in_both_reads_and_takes_the_current_revision() {
        // Arrange
        let cases = [
            (
                Ok(found(NodeKind::File, 42)),
                Ok(view(NodeKind::File, NodeState::Live)),
            ),
            (
                Ok(LiveFileLookup::Gone),
                Ok(view(NodeKind::File, NodeState::Tombstone)),
            ),
            (Ok(LiveFileLookup::Gone), Err(TreeReaderError::NotFound)),
            (
                Ok(found(NodeKind::File, 42)),
                Ok(view(NodeKind::File, NodeState::Tombstone)),
            ),
            (
                Ok(found(NodeKind::Folder, 0)),
                Ok(view(NodeKind::Folder, NodeState::Live)),
            ),
            (
                Err(TreeReaderError::NotFound),
                Err(TreeReaderError::NotFound),
            ),
        ];

        // Act
        let targets: Vec<Target> = cases
            .into_iter()
            .map(|(file, node)| target_of(file, node).expect("no read failed"))
            .collect();

        // Assert
        assert_eq!(
            targets[0],
            Target::Live {
                name: "f.png".to_owned(),
                size: 42,
                media_type: Some("image/png".to_owned()),
                content_hash: FileHash::from_bytes([7; 32]),
            }
        );
        assert!(targets[1..].iter().all(|target| *target == Target::Gone));
    }

    #[test]
    fn a_failed_target_read_is_unexpected_not_a_gone_target() {
        // Arrange
        let failure = || TreeReaderError::Unexpected(Box::new(DbError::Unavailable));

        // Act
        let first = target_of(Err(failure()), Ok(view(NodeKind::File, NodeState::Live)));
        let second = target_of(Ok(found(NodeKind::File, 1)), Err(failure()));

        // Assert
        assert!(matches!(first, Err(PublicShareError::Unexpected(_))));
        assert!(matches!(second, Err(PublicShareError::Unexpected(_))));
    }
}
