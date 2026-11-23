//! Mode S case data: the success a port answers when an operation's request is allowed, keyed by the owner port
//! method the document names (`x-owner-method`). The control and read cases script it before each request; an
//! owner with no entry has no case data, and the generators name it.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use contracts::*;
use sc_clock::Timestamp;
use sc_ids::{RawToken, TokenKind};
use uuid::Uuid;

use crate::harness::Script;
use crate::sharing::support::{allowed, derived, full, grant, shared_file};

const WHEN: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);
const NODE: Uuid = Uuid::from_u128(0xD1);
const REVISION: Uuid = Uuid::from_u128(0xD2);
const VAULT: Uuid = Uuid::from_u128(0xD3);

/// SHA-256 of one zero byte, the header the generated requests send.
fn vault_etag() -> ETag {
    ETag("6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d".to_owned())
}

/// Owners whose operation calls no port: nothing to script.
const PORTLESS: [&str; 2] = ["HttpServer::openapi", "ServerInfo::value"];

fn node() -> NodeView {
    NodeView {
        id: NODE,
        parent_id: None,
        kind: NodeKind::Folder,
        name: "Documents".to_owned(),
        state: NodeState::Live,
        current_revision_id: None,
        size: None,
        content_hash: None,
        media_type: None,
        client_mtime: None,
        created_at: WHEN,
        updated_at: WHEN,
        seq: 1,
        thumb_state: ThumbState::None,
        vault: false,
    }
}

fn quota() -> QuotaView {
    QuotaView {
        limit_bytes: None,
        effective_limit: 1_000_000,
        used: 0,
        reserved: 0,
    }
}

fn token(kind: TokenKind, value: &str) -> RawToken {
    RawToken::from_presented(kind, value.to_owned())
}

fn cursor() -> Cursor {
    Cursor { epoch: 1, seq: 1 }
}

fn download(script: &Script, key: &'static str) {
    script.push(key, Ok::<_, DownloadError>(full("image/png", 4)));
}

/// Scripts the success of every port call the operation owned by `owner` makes; `false` when there is no case
/// data for that owner.
pub fn prepare(script: &Script, owner: &str) -> bool {
    match owner {
        "AccountDeletionService::request" => script.push(
            "AccountDeletionService::request",
            Ok::<_, DeletionError>(DeletionAccepted { deadline: WHEN }),
        ),
        "AccountService::view" => script.push(
            "AccountService::view",
            Ok::<_, AccountServiceError>(AccountView {
                id: Uuid::from_u128(0xA1),
                display_name: "member".to_owned(),
                role: Role::User,
                identities: Vec::new(),
                quota: quota(),
                root_node_id: NODE,
            }),
        ),
        "AccountService::quota_view" => script.push(
            "AccountService::quota_view",
            Ok::<_, AccountServiceError>(quota()),
        ),
        "AdminService::health" => {
            script.push(
                "AdminService::health",
                Ok::<HealthReport, AdminError>(HealthReport { items: Vec::new() }),
            );
        }
        "AdminService::list_users" => script.push(
            "AdminService::list_users",
            Ok::<AdminUserPage, AdminError>(AdminUserPage {
                items: Vec::new(),
                default_quota_bytes: 1 << 30,
            }),
        ),
        "AdminService::update_user" | "AdminService::set_quota" => {
            script.push(static_key(owner), Ok::<(), AdminError>(()));
        }
        "InviteService::create_invite" => script.push(
            "InviteService::create_invite",
            Ok::<_, InviteError>(CreatedInvite {
                token: InviteToken(token(TokenKind::Invite, "inv-token")),
                expires_at: WHEN,
            }),
        ),
        "InviteService::list" => script.push(
            "InviteService::list",
            Ok::<Vec<InviteInfo>, InviteError>(Vec::new()),
        ),
        "InviteService::revoke" => script.push("InviteService::revoke", Ok::<(), InviteError>(())),
        "SignInService::list_providers" => script.push(
            "SignInService::list_providers",
            Ok::<_, SignInError>(ProviderList {
                providers: Vec::new(),
                setup_required: false,
            }),
        ),
        "SignInService::begin_link" => script.push(
            "SignInService::begin_link",
            Ok::<_, SignInError>(AuthorizationRedirect {
                url: "https://idp.test/authorize".to_owned(),
                browser_binding: BrowserBinding("binding".to_owned()),
            }),
        ),
        "SessionService::list" => script.push(
            "SessionService::list",
            Ok::<Vec<SessionInfo>, SessionError>(Vec::new()),
        ),
        "SessionService::revoke" => {
            script.push("SessionService::revoke", Ok::<(), SessionError>(()))
        }
        "DeviceGrantService::lookup_for_approval" => script.push(
            "DeviceGrantService::lookup_for_approval",
            Ok::<_, DeviceGrantError>(ApprovalView {
                grant_id: Uuid::from_u128(0xC1),
                expires_at: WHEN,
                device: "laptop".to_owned(),
                platform: "linux".to_owned(),
                requester_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                requester_location: None,
                approver_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                approver_location: None,
                mismatch: false,
            }),
        ),
        "DeviceGrantService::list_devices" => script.push(
            "DeviceGrantService::list_devices",
            Ok::<Vec<DeviceInfo>, DeviceGrantError>(Vec::new()),
        ),
        "DeviceGrantService::decide" | "DeviceGrantService::revoke_device" => {
            script.push(static_key(owner), Ok::<(), DeviceGrantError>(()));
        }
        "ChangeFeed::read" => script.push(
            "ChangeFeed::read",
            Ok::<_, ChangeFeedError>(ChangePage {
                items: Vec::new(),
                next_cursor: cursor(),
                has_more: false,
            }),
        ),
        "ChangeFeed::head" => script.push("ChangeFeed::head", Ok::<_, ChangeFeedError>(cursor())),
        _ => return prepare_files(script, owner),
    }
    true
}

fn prepare_files(script: &Script, owner: &str) -> bool {
    match owner {
        "TreeReader::get_node" => {
            script.push("TreeReader::get_node", Ok::<_, TreeReaderError>(node()));
        }
        "TreeReader::list_children" => script.push(
            "TreeReader::list_children",
            Ok::<_, TreeReaderError>(ListingPage {
                items: Vec::new(),
                next: None,
            }),
        ),
        "TreeReader::get_revision" => script.push(
            "TreeReader::get_revision",
            Ok::<_, TreeReaderError>(RevisionView {
                id: REVISION,
                node_id: NODE,
                blob_id: uuid::Uuid::from_u128(0xB10B),
                size: 0,
                content_hash: sc_ids::FileHash::from_bytes([0xAB; 32]),
                blocklist: None,
                media_type: None,
                client_mtime: None,
                client_meta: None,
                created_at: WHEN,
            }),
        ),
        "TreeReader::lookup_revisions" => script.push(
            "TreeReader::lookup_revisions",
            Ok::<Vec<RevisionMatch>, TreeReaderError>(Vec::new()),
        ),
        "TreeService::create_folder" | "TreeService::update_node" => {
            script.push(static_key(owner), Ok::<_, TreeServiceError>(node()));
        }
        "TreeService::delete_node" => {
            script.push("TreeService::delete_node", Ok::<(), TreeServiceError>(()));
        }
        "TreeService::find_revisions" => script.push(
            "TreeService::find_revisions",
            Ok::<Vec<RevisionRef>, TreeServiceError>(Vec::new()),
        ),
        "UploadService::start_upload" => script.push(
            "UploadService::start_upload",
            Ok::<_, UploadError>(StartUploadOutcome::Started(Started {
                upload_id: Uuid::from_u128(0xF4),
                need_blocks: Vec::new(),
                block_size: 4 << 20,
                lane_limit: 4,
                expires_at: WHEN,
            })),
        ),
        "UploadService::get_upload" => script.push(
            "UploadService::get_upload",
            Ok::<_, UploadError>(UploadState {
                need_blocks: Vec::new(),
                received_count: 0,
                expires_at: WHEN,
            }),
        ),
        "UploadService::abort_upload" => {
            script.push("UploadService::abort_upload", Ok::<(), UploadError>(()));
        }
        "UploadService::put_block" => {
            script.push("UploadService::put_block", Ok::<_, UploadError>(BlockAck));
        }
        "UploadService::stage_small" => script.push(
            "UploadService::stage_small",
            Ok::<_, UploadError>(Staged {
                staged_key: Uuid::from_u128(0xF5),
                size: 0,
            }),
        ),
        "CommitService::commit_upload" => {
            script.push(
                "CommitService::commit_upload",
                Ok::<_, CommitServiceError>(committed()),
            );
        }
        "CommitService::commit_batch" => script.push(
            "CommitService::commit_batch",
            Ok::<_, CommitServiceError>(BatchResult {
                per_entry: vec![BatchEntryResult::Committed(committed())],
            }),
        ),
        "DownloadService::open_revision_content" => {
            download(script, "DownloadService::open_revision_content");
        }
        "DerivedOpener::open_derived" => {
            script.push(
                "DerivedOpener::open_derived",
                Ok::<_, DerivedError>(derived("image/webp")),
            );
            download(script, "DownloadService::open_blob_content");
        }
        "ThumbnailStateSource::states" => script.push(
            "ThumbnailStateSource::states",
            Ok::<_, ThumbnailStateError>(HashMap::<sc_ids::FileHash, ThumbState>::new()),
        ),
        _ => return prepare_shares(script, owner),
    }
    true
}

fn prepare_shares(script: &Script, owner: &str) -> bool {
    match owner {
        "ShareService::create" => script.push(
            "ShareService::create",
            Ok::<_, ShareError>(CreatedShare {
                id: Uuid::from_u128(0xE5),
                token: token(TokenKind::Share, "sc_shr_created"),
                url_path: "/s/sc_shr_created".to_owned(),
                expires_at: WHEN,
                has_password: false,
            }),
        ),
        "ShareService::list" => script.push(
            "ShareService::list",
            Ok::<Vec<ShareInfo>, ShareError>(Vec::new()),
        ),
        "ShareService::revoke" => script.push("ShareService::revoke", Ok::<(), ShareError>(())),
        "PublicShareService::open" => script.push(
            "PublicShareService::open",
            Ok::<_, PublicShareError>(ShareOpen::Available {
                name: "photo.png".to_owned(),
                size: 4,
                media_type: Some("image/png".to_owned()),
                content_hash: sc_ids::FileHash::from_bytes([0xAB; 32]),
                expires_at: WHEN,
            }),
        ),
        "PublicShareService::verify_password" => script.push(
            "PublicShareService::verify_password",
            Ok::<_, PublicShareError>(grant(600)),
        ),
        "PublicShareService::authorize_content" => {
            script.push("PublicShareService::authorize_content", allowed());
            download(script, "DownloadService::open_revision_content");
        }
        "PublicShareService::authorize_derived" => {
            script.push("PublicShareService::authorize_derived", shared_file());
            script.push(
                "DerivedOpener::open_derived_for",
                Ok::<_, DerivedError>(derived("image/webp")),
            );
            download(script, "DownloadService::open_blob_content");
        }
        "VaultService::create_vault" => script.push(
            "VaultService::create_vault",
            Ok::<_, VaultError>(CreatedVault {
                vault_id: VAULT,
                etag: vault_etag(),
            }),
        ),
        "VaultService::get_header" => script.push(
            "VaultService::get_header",
            Ok::<_, VaultError>(VaultHeader {
                header: vec![0],
                etag: vault_etag(),
                root_node_id: NODE,
            }),
        ),
        "VaultService::replace_header" => {
            script.push(
                "VaultService::replace_header",
                Ok::<_, VaultError>(vault_etag()),
            );
        }
        "VaultService::report_unlock_failure" => script.push(
            "VaultService::report_unlock_failure",
            Ok::<(), VaultError>(()),
        ),
        _ => return PORTLESS.contains(&owner),
    }
    true
}

fn committed() -> Committed {
    Committed {
        node_id: NODE,
        revision_id: REVISION,
        conflict_copy: None,
    }
}

/// The `'static` key of an owner the match above names; the script keys calls by `&'static str`.
fn static_key(owner: &str) -> &'static str {
    const KEYS: [&str; 6] = [
        "AdminService::update_user",
        "AdminService::set_quota",
        "DeviceGrantService::decide",
        "DeviceGrantService::revoke_device",
        "TreeService::create_folder",
        "TreeService::update_node",
    ];
    KEYS.into_iter()
        .find(|key| *key == owner)
        .expect("only keys listed here reach static_key")
}

/// Whether `owner` has Mode S case data.
pub fn has_case_data(owner: &str) -> bool {
    prepare(&Script::default(), owner)
}
