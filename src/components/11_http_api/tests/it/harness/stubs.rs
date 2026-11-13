use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::*;
use sc_ids::{FileHash, Hash32, RawToken, UserCode};
use uuid::Uuid;

use super::script::{Script, unscripted};

/// Implements `$trait` for `$stub`: every method returns the next scripted result of
/// `"$trait::method"`, or `Err($error::Unexpected(UnscriptedCall))` when none is queued.
macro_rules! stub_port {
    ($stub:ident: $trait:ident, $error:ident, {
        $( async fn $method:ident(&self $(, $arg:ident: $ty:ty)* $(,)?) -> $ret:ty; )*
    }) => {
        pub struct $stub(Script);

        impl $stub {
            pub fn new(script: &Script) -> Arc<Self> {
                Arc::new(Self(script.clone()))
            }
        }

        #[contracts::async_trait]
        impl $trait for $stub {
            $(
                async fn $method(&self $(, $arg: $ty)*) -> $ret {
                    let _arguments = ($(&$arg,)*);
                    let key = concat!(stringify!($trait), "::", stringify!($method));
                    self.0
                        .take::<$ret>(key)
                        .unwrap_or_else(|| Err($error::Unexpected(unscripted(key))))
                }
            )*
        }
    };
}

stub_port!(PrincipalRevalidatorStub: PrincipalRevalidator, RevalidatorError, {
    async fn revalidate(&self, principal: &Principal) -> Result<bool, RevalidatorError>;
});

stub_port!(SignInStub: SignInService, SignInError, {
    async fn begin(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError>;
    async fn complete(&self, provider: &str, callback_params: CallbackParams,
        browser_binding: &BrowserBinding, host: &str, client_ip: IpAddr)
        -> Result<SignInOutcome, SignInError>;
    async fn begin_link(&self, principal: &Principal, provider: &str)
        -> Result<AuthorizationRedirect, SignInError>;
    async fn complete_link(&self, principal: &Principal, provider: &str,
        callback_params: CallbackParams, browser_binding: &BrowserBinding, host: &str,
        client_ip: IpAddr) -> Result<Linked, SignInError>;
    async fn exchange_mobile_code(&self, code: &RawToken, code_verifier: &str,
        client_ip: IpAddr) -> Result<SessionTokens, SignInError>;
    async fn list_providers(&self) -> Result<ProviderList, SignInError>;
});

stub_port!(SessionStub: SessionService, SessionError, {
    async fn refresh(&self, refresh_token: &RawToken) -> Result<SessionTokens, SessionError>;
    async fn revoke(&self, account: Uuid, session_id: Uuid, reason: RevokeReason)
        -> Result<(), SessionError>;
    async fn revoke_all(&self, account: Uuid, reason: RevokeReason) -> Result<(), SessionError>;
    async fn list(&self, account: Uuid) -> Result<Vec<SessionInfo>, SessionError>;
    async fn touch(&self, principal: &Principal) -> Result<(), SessionError>;
});

stub_port!(DeviceGrantStub: DeviceGrantService, DeviceGrantError, {
    async fn request(&self, device_name: &str, platform: &str, requester_ip: IpAddr)
        -> Result<DeviceAuthorization, DeviceGrantError>;
    async fn poll(&self, device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError>;
    async fn lookup_for_approval(&self, principal: &Principal, user_code: &UserCode,
        approver_ip: IpAddr) -> Result<ApprovalView, DeviceGrantError>;
    async fn decide(&self, principal: &Principal, grant_id: Uuid, decision: Decision)
        -> Result<(), DeviceGrantError>;
    async fn list_devices(&self, account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError>;
    async fn revoke_device(&self, account: Uuid, device_id: Uuid) -> Result<(), DeviceGrantError>;
    async fn revoke_self(&self, principal: &Principal) -> Result<(), DeviceGrantError>;
});

stub_port!(InviteStub: InviteService, InviteError, {
    async fn create_invite(&self, admin_account: Uuid, note: &str)
        -> Result<InviteToken, InviteError>;
    async fn list(&self, admin_account: Uuid) -> Result<Vec<InviteInfo>, InviteError>;
    async fn revoke(&self, admin_account: Uuid, invite_id: Uuid) -> Result<(), InviteError>;
});

stub_port!(TreeStub: TreeService, TreeServiceError, {
    async fn create_folder(&self, account: Uuid, parent_id: Uuid, name: &str)
        -> Result<NodeView, TreeServiceError>;
    async fn update_node(&self, account: Uuid, node_id: Uuid, new_name: Option<&str>,
        new_parent: Option<Uuid>) -> Result<NodeView, TreeServiceError>;
    async fn delete_node(&self, account: Uuid, node_id: Uuid, base_revision: Option<Uuid>)
        -> Result<(), TreeServiceError>;
    async fn resolve_path(&self, account: Uuid, path: &str) -> Result<Uuid, TreeServiceError>;
    async fn find_revisions(&self, account: Uuid, client_device: Option<&str>, page: Page)
        -> Result<Vec<RevisionRef>, TreeServiceError>;
});

stub_port!(TreeReaderStub: TreeReader, TreeReaderError, {
    async fn get_node(&self, account: Uuid, selector: NodeSelector)
        -> Result<NodeView, TreeReaderError>;
    async fn list_children(&self, account: Uuid, folder_id: Uuid, after_name_key: Option<&str>,
        limit: u32) -> Result<ListingPage, TreeReaderError>;
    async fn find_live_node(&self, account: Uuid, parent_id: Uuid, name: &str)
        -> Result<Option<NodeRef>, TreeReaderError>;
    async fn resolve_live_file(&self, account: Uuid, node_id: Uuid)
        -> Result<LiveFileLookup, TreeReaderError>;
    async fn root_node(&self, account: Uuid) -> Result<NodeRef, TreeReaderError>;
    async fn get_revision(&self, account: Uuid, revision_id: Uuid)
        -> Result<RevisionView, TreeReaderError>;
    async fn is_current_or_retained(&self, account: Uuid, revision_id: Uuid)
        -> Result<bool, TreeReaderError>;
    async fn describe_blobs(&self, blob_ids: &[Uuid])
        -> Result<HashMap<Uuid, Vec<String>>, TreeReaderError>;
    async fn lookup_revisions(&self, account: Uuid, hashes: &[FileHash])
        -> Result<Vec<RevisionMatch>, TreeReaderError>;
});

stub_port!(UploadStub: UploadService, UploadError, {
    async fn start_upload(&self, account: Uuid, request: StartUpload)
        -> Result<StartUploadOutcome, UploadError>;
    async fn get_upload(&self, account: Uuid, upload_id: Uuid) -> Result<UploadState, UploadError>;
    async fn put_block(&self, account: Uuid, upload_id: Uuid, index: u32, expected_hash: Hash32,
        body: ByteStream, length: u64) -> Result<BlockAck, UploadError>;
    async fn abort_upload(&self, account: Uuid, upload_id: Uuid) -> Result<(), UploadError>;
    async fn stage_small(&self, account: Uuid, staged_key: Uuid, expected_hash: Option<Hash32>,
        body: ByteStream, length: u64) -> Result<Staged, UploadError>;
});

stub_port!(CommitStub: CommitService, CommitServiceError, {
    async fn commit_upload(&self, actor: CommitActor, request: CommitUpload)
        -> Result<Committed, CommitServiceError>;
    async fn commit_batch(&self, actor: CommitActor, request: BatchCommit)
        -> Result<BatchResult, CommitServiceError>;
});

stub_port!(DownloadStub: DownloadService, DownloadError, {
    async fn open_revision_content(&self, account: Uuid, target: RevisionOrCurrent,
        options: DownloadOptions) -> Result<Download, DownloadError>;
    async fn open_blob_content(&self, account: Uuid, blob: BlobRef, media_type: Option<String>,
        options: DownloadOptions) -> Result<Download, DownloadError>;
});

stub_port!(ChangeFeedStub: ChangeFeed, ChangeFeedError, {
    async fn read(&self, principal: &Principal, revalidator: &dyn PrincipalRevalidator,
        cursor: Cursor, wait: Option<Duration>, limit: Option<u32>)
        -> Result<ChangePage, ChangeFeedError>;
    async fn head(&self, account: Uuid) -> Result<Cursor, ChangeFeedError>;
});

stub_port!(ShareStub: ShareService, ShareError, {
    async fn create(&self, account: Uuid, node_id: Uuid, expires_in: Option<Duration>,
        password: Option<&str>) -> Result<CreatedShare, ShareError>;
    async fn list(&self, account: Uuid, node_id: Option<Uuid>) -> Result<Vec<ShareInfo>, ShareError>;
    async fn revoke(&self, account: Uuid, share_id: Uuid) -> Result<(), ShareError>;
});

stub_port!(PublicShareStub: PublicShareService, PublicShareError, {
    async fn open(&self, token: &RawToken, client_ip: IpAddr)
        -> Result<ShareOpen, PublicShareError>;
    async fn verify_password(&self, token: &RawToken, password: &str, client_ip: IpAddr)
        -> Result<AccessGrant, PublicShareError>;
    async fn authorize_content(&self, token: &RawToken, grant: Option<&AccessGrant>,
        client_ip: IpAddr, starts_download: bool) -> Result<ShareContent, PublicShareError>;
    async fn authorize_derived(&self, token: &RawToken, grant: Option<&AccessGrant>,
        client_ip: IpAddr) -> Result<ShareDerived, PublicShareError>;
});

stub_port!(AccountStub: AccountService, AccountServiceError, {
    async fn view(&self, principal: &Principal) -> Result<AccountView, AccountServiceError>;
    async fn quota_view(&self, principal: &Principal) -> Result<QuotaView, AccountServiceError>;
});

stub_port!(AccountDeletionStub: AccountDeletionService, DeletionError, {
    async fn request(&self, principal: &Principal, confirmation: &str)
        -> Result<DeletionAccepted, DeletionError>;
});

stub_port!(ThumbnailStateStub: ThumbnailStateSource, ThumbnailStateError, {
    async fn states(&self, account: Uuid, hashes: &[FileHash])
        -> Result<HashMap<FileHash, ThumbState>, ThumbnailStateError>;
});

stub_port!(DerivedStub: DerivedOpener, DerivedError, {
    async fn open_derived(&self, account: Uuid, node_id: Uuid, kind: DerivedKind)
        -> Result<BlobRef, DerivedError>;
    async fn open_derived_for(&self, file: &LiveFile, kind: DerivedKind)
        -> Result<BlobRef, DerivedError>;
});

stub_port!(WorkerJobsStub: WorkerJobs, WorkerJobError, {
    async fn claim(&self, lane: Lane, worker_id: &str) -> Result<Option<ClaimedJob>, WorkerJobError>;
    async fn extend_lease(&self, job_id: Uuid, worker_id: &str)
        -> Result<LeaseExtended, WorkerJobError>;
    async fn release(&self, job_id: Uuid, worker_id: &str) -> Result<(), WorkerJobError>;
    async fn complete_skipped(&self, job_id: Uuid, worker_id: &str, outcome: SkipOutcome)
        -> Result<(), WorkerJobError>;
    async fn fail(&self, job_id: Uuid, worker_id: &str, code: &str, disposition: FailDisposition)
        -> Result<FailResult, WorkerJobError>;
    async fn open_input(&self, job_id: Uuid, worker_id: &str) -> Result<WorkerInput, WorkerJobError>;
    async fn accept_output(&self, job_id: Uuid, worker_id: &str, kind: DerivedKind,
        meta: OutputMeta, body: ByteStream, length: u64) -> Result<AcceptOutcome, WorkerJobError>;
    async fn record_status(&self, worker_id: &str, status: WorkerStatus)
        -> Result<(), WorkerJobError>;
});

stub_port!(VaultStub: VaultService, VaultError, {
    async fn create_vault(&self, account: Uuid, header: &[u8], root_node_id: Uuid)
        -> Result<CreatedVault, VaultError>;
    async fn get_header(&self, account: Uuid, vault_id: Uuid) -> Result<VaultHeader, VaultError>;
    async fn replace_header(&self, account: Uuid, vault_id: Uuid, header: &[u8], if_match: &ETag)
        -> Result<ETag, VaultError>;
    async fn report_unlock_failure(&self, account: Uuid, vault_id: Uuid) -> Result<(), VaultError>;
});

pub struct AuthenticatorStub(Script);

impl AuthenticatorStub {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }
}

#[contracts::async_trait]
impl Authenticator for AuthenticatorStub {
    async fn authenticate(&self, credential: Credential) -> Result<Principal, AuthenticatorError> {
        let _credential = &credential;
        let key = "Authenticator::authenticate";
        self.0
            .take(key)
            .unwrap_or_else(|| Err(AuthenticatorError::Unexpected(unscripted(key))))
    }

    fn require(&self, _principal: &Principal, _scope: Scope) -> Result<(), AuthenticatorError> {
        let key = "Authenticator::require";
        self.0
            .take(key)
            .unwrap_or_else(|| Err(AuthenticatorError::Unexpected(unscripted(key))))
    }
}

pub struct AdminStub(Script);

impl AdminStub {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }
}

#[contracts::async_trait]
impl AdminService for AdminStub {
    async fn list_users(
        &self,
        actor: &Actor,
        page: Page,
    ) -> Result<Vec<AdminUserView>, AdminError> {
        let _arguments = (actor, page);
        let key = "AdminService::list_users";
        self.0
            .take(key)
            .unwrap_or_else(|| Err(AdminError::Unexpected(unscripted(key))))
    }

    async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminError> {
        let _arguments = (actor, account, role, state);
        let key = "AdminService::update_user";
        self.0
            .take(key)
            .unwrap_or_else(|| Err(AdminError::Unexpected(unscripted(key))))
    }

    async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminError> {
        let _arguments = (actor, account, limit);
        let key = "AdminService::set_quota";
        self.0
            .take(key)
            .unwrap_or_else(|| Err(AdminError::Unexpected(unscripted(key))))
    }

    /// The report has no error channel, so an unscripted call reports a down item naming the call.
    async fn health(&self, actor: &Actor) -> HealthReport {
        let _actor = actor;
        let key = "AdminService::health";
        self.0.take(key).unwrap_or_else(|| HealthReport {
            items: vec![(
                key.to_owned(),
                HealthItem::new(HealthStatus::Down, "unscripted port call"),
            )],
        })
    }
}

pub struct WorkerAuthStub(Script);

impl WorkerAuthStub {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }
}

#[contracts::async_trait]
impl WorkerAuthenticator for WorkerAuthStub {
    /// `WorkerAuthError` has no unexpected-fault variant, so an unscripted call is refused as `Forbidden`.
    async fn authenticate(&self, bearer: &str) -> Result<WorkerIdentity, WorkerAuthError> {
        let _bearer = bearer;
        self.0
            .take("WorkerAuthenticator::authenticate")
            .unwrap_or(Err(WorkerAuthError::Forbidden))
    }
}
