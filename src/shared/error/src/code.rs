//! The single code table: wire string, status and title of every error code.
//! Statuses are plain integers; the HTTP mapper in 11 reads this table and never picks one itself.

use serde::{Serialize, Serializer};

macro_rules! error_codes {
    ($($variant:ident => $wire:literal, $status:literal, $title:literal;)+) => {
        /// Stable snake_case error vocabulary shared by server and clients.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ErrorCode {
            $($variant,)+
        }

        impl ErrorCode {
            /// Every code, in table order.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant,)+];

            /// Wire code.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }

            /// Status of this code; the only place a status is chosen.
            pub const fn status(self) -> u16 {
                match self {
                    $(Self::$variant => $status,)+
                }
            }

            /// Short human summary for the problem body `title`; never carries request data.
            pub const fn title(self) -> &'static str {
                match self {
                    $(Self::$variant => $title,)+
                }
            }
        }
    };
}

error_codes! {
    InvalidRequest => "invalid_request", 400, "Invalid request";

    Unauthenticated => "unauthenticated", 401, "Authentication required";
    PasswordRequired => "password_required", 401, "Password required";

    AccountNotActive => "account_not_active", 403, "Account not active";
    Forbidden => "forbidden", 403, "Forbidden";

    ParentNotFound => "parent_not_found", 404, "Parent not found";
    NotFound => "not_found", 404, "Not found";
    ContentUnavailable => "content_unavailable", 404, "Content unavailable";
    RevisionObsolete => "revision_obsolete", 404, "Revision obsolete";
    ThumbnailNotReady => "thumbnail_not_ready", 404, "Thumbnail not ready";
    ThumbnailUnavailable => "thumbnail_unavailable", 404, "Thumbnail unavailable";

    NameConflict => "name_conflict", 409, "Name conflict";
    EditedSince => "edited_since", 409, "Edited since";
    Cycle => "cycle", 409, "Cycle";
    NeedBlocks => "need_blocks", 409, "Blocks needed";
    BlockConflict => "block_conflict", 409, "Block conflict";
    LastAdmin => "last_admin", 409, "Last admin";
    SessionCapExceeded => "session_cap_exceeded", 409, "Upload session cap exceeded";
    StagedCapReached => "staged_cap_reached", 409, "Staged file cap reached";
    LeaseLost => "lease_lost", 409, "Lease lost";

    UploadSessionGone => "upload_session_gone", 410, "Upload session gone";
    ParentDeleted => "parent_deleted", 410, "Parent deleted";
    ResyncRequired => "resync_required", 410, "Resync required";

    SizeTooLarge => "size_too_large", 413, "Size too large";
    OutputTooLarge => "output_too_large", 413, "Output too large";

    RangeNotSatisfiable => "range_not_satisfiable", 416, "Range not satisfiable";

    InvalidName => "invalid_name", 422, "Invalid name";
    InvalidCursor => "invalid_cursor", 422, "Invalid cursor";
    NotAFolder => "not_a_folder", 422, "Not a folder";
    BlocklistMismatch => "blocklist_mismatch", 422, "Blocklist mismatch";
    CommitIdReused => "commit_id_reused", 422, "Commit id reused";
    BatchTooLarge => "batch_too_large", 422, "Batch too large";
    TargetNotShareable => "target_not_shareable", 422, "Target not shareable";
    BlockHashMismatch => "block_hash_mismatch", 422, "Block hash mismatch";
    BlockLengthMismatch => "block_length_mismatch", 422, "Block length mismatch";
    OutputTypeRejected => "output_type_rejected", 422, "Output type rejected";
    OutputKindRejected => "output_kind_rejected", 422, "Output kind rejected";

    UpgradeRequired => "upgrade_required", 426, "Upgrade required";

    RateLimited => "rate_limited", 429, "Too many requests";

    Internal => "internal", 500, "Internal error";

    ServiceUnavailable => "service_unavailable", 503, "Service unavailable";
    Overloaded => "overloaded", 503, "Overloaded";

    QuotaExceeded => "quota_exceeded", 507, "Quota exceeded";
    FreeSpaceFloor => "free_space_floor", 507, "Free space floor reached";
}

impl ErrorCode {
    /// Maps a wire string to its code; `None` for a code this build does not know.
    pub fn from_wire(wire: &str) -> Option<ErrorCode> {
        Self::ALL.iter().copied().find(|code| code.as_str() == wire)
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
