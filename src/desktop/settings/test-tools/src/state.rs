//! The default answers of the stub: a small in-memory daemon that follows
//! `contracts/14_desktop_daemon/daemon_control.md` for everything that does not need the
//! server, the file system or the rules grammar (those cases are scripted).

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use ipc_schema::{
    ControlError, ControlResult, DaemonControl, DaemonStatus, EscapedEntry, EscapedPage,
    FolderEntry, FolderPage, ListEntriesParams, ListServerFoldersParams, LoginPrompt,
    LoginStartParams, LoginState, LogoutOutcome, NodeId, PageToken, PairId, PairIdParams, PairSpec,
    PairState, PairTarget, PairView, ResolveHoldParams, SetServerParams, Settings, SettingsPatch,
    SkippedEntry, SkippedPage, StatusEvent, StatusSink, UpdatePairParams,
};

use crate::script::{Pages, Script};

const PAGE_PREFIX: &str = "page-";

struct Inner {
    status: DaemonStatus,
    settings: Settings,
    login: LoginState,
    prompt: LoginPrompt,
    next_pair: i64,
    folder_pages: Pages<FolderEntry>,
    skipped_pages: Pages<SkippedEntry>,
    escaped_pages: Pages<EscapedEntry>,
    sinks: Vec<Arc<dyn StatusSink>>,
}

pub(crate) struct DefaultDaemon {
    inner: Mutex<Inner>,
}

fn empty_status() -> DaemonStatus {
    DaemonStatus {
        server_url: None,
        account: None,
        pairs: Vec::new(),
        skipped_count: 0,
        escaped_count: 0,
        holds: Vec::new(),
        update_available: None,
        access_denied: Vec::new(),
        watcher_degraded: Vec::new(),
    }
}

impl DefaultDaemon {
    pub(crate) fn new(script: &Script) -> Self {
        let status = script.status.clone().unwrap_or_else(empty_status);
        let next_pair = status.pairs.iter().map(|p| p.pair_id.0).max().unwrap_or(0) + 1;
        let login = if status.account.is_some() {
            LoginState::Approved
        } else {
            LoginState::SignedOut
        };
        Self {
            inner: Mutex::new(Inner {
                status,
                settings: script.settings.clone().unwrap_or(Settings {
                    bandwidth_limit: 0,
                    hash_throttle: 0.0,
                    autostart: false,
                    tray: true,
                }),
                login,
                prompt: script.login_prompt.clone().unwrap_or_else(|| LoginPrompt {
                    verification_url: "https://stub.example/device".to_owned(),
                    user_code: "WDJB-MJHT".to_owned(),
                    expires_at: "2099-01-01T00:00:00Z".to_owned(),
                }),
                next_pair,
                folder_pages: script.folder_pages.clone(),
                skipped_pages: script.skipped_pages.clone(),
                escaped_pages: script.escaped_pages.clone(),
                sinks: Vec::new(),
            }),
        }
    }

    /// Runs `change` under the lock; when it changed the status, every subscriber gets one
    /// `StatusEvent` with the full new status before the caller answers.
    fn mutate<R>(&self, change: impl FnOnce(&mut Inner) -> ControlResult<R>) -> ControlResult<R> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let before = inner.status.clone();
        let outcome = change(&mut inner);
        if inner.status != before {
            for sink in &inner.sinks {
                sink.send(StatusEvent {
                    status: inner.status.clone(),
                });
            }
        }
        outcome
    }

    fn read<R>(&self, read: impl FnOnce(&Inner) -> R) -> R {
        read(&self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn signed_in(inner: &Inner) -> ControlResult<()> {
    if inner.status.account.is_some() {
        Ok(())
    } else {
        Err(ControlError::NotSignedIn)
    }
}

fn pair_mut(inner: &mut Inner, id: PairId) -> ControlResult<&mut PairView> {
    inner
        .status
        .pairs
        .iter_mut()
        .find(|p| p.pair_id == id)
        .ok_or(ControlError::UnknownPair)
}

/// Page `n` of a scripted listing is addressed by the token `page-n`; a listing without
/// pages is one empty page.
fn page_of<T: Clone>(
    pages: Option<&Vec<Vec<T>>>,
    token: Option<&PageToken>,
) -> ControlResult<(Vec<T>, Option<PageToken>)> {
    let Some(pages) = pages else {
        return Ok((Vec::new(), None));
    };
    let index = match token {
        None => 0,
        Some(PageToken(text)) => text
            .strip_prefix(PAGE_PREFIX)
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(bad_token)?,
    };
    let items = pages.get(index).ok_or_else(bad_token)?.clone();
    let next = (index + 1 < pages.len()).then(|| PageToken(format!("{PAGE_PREFIX}{}", index + 1)));
    Ok((items, next))
}

fn bad_token() -> ControlError {
    ControlError::Internal {
        correlation_id: "stub-unknown-page-token".to_owned(),
    }
}

fn entry_key(pair: Option<PairId>) -> String {
    pair.map(|p| p.0.to_string()).unwrap_or_default()
}

fn check_pair_filter(inner: &Inner, pair: Option<PairId>) -> ControlResult<()> {
    match pair {
        Some(id) if !inner.status.pairs.iter().any(|p| p.pair_id == id) => {
            Err(ControlError::UnknownPair)
        }
        _ => Ok(()),
    }
}

fn nested(a: &str, b: &str) -> bool {
    Path::new(a).starts_with(b) || Path::new(b).starts_with(a)
}

impl DaemonControl for DefaultDaemon {
    async fn status(&self) -> ControlResult<DaemonStatus> {
        Ok(self.read(|i| i.status.clone()))
    }

    async fn set_server(&self, params: SetServerParams) -> ControlResult<()> {
        if !params.url.to_ascii_lowercase().starts_with("https://") {
            return Err(ControlError::HttpsRequired);
        }
        self.mutate(|i| {
            if i.status.account.is_some() {
                return Err(ControlError::SignedIn);
            }
            i.status.server_url = Some(params.url);
            Ok(())
        })
    }

    async fn list_server_folders(
        &self,
        params: ListServerFoldersParams,
    ) -> ControlResult<FolderPage> {
        self.read(|i| {
            signed_in(i)?;
            let key = params
                .parent
                .as_ref()
                .map(NodeId::as_str)
                .unwrap_or_default();
            let pages = i.folder_pages.get(key);
            if pages.is_none() && params.parent.is_some() {
                return Err(ControlError::UnknownFolder);
            }
            let (items, next) = page_of(pages, params.page.as_ref())?;
            Ok(FolderPage { items, next })
        })
    }

    async fn list_pairs(&self) -> ControlResult<Vec<PairView>> {
        self.read(|i| {
            signed_in(i)?;
            Ok(i.status.pairs.clone())
        })
    }

    async fn add_pair(&self, spec: PairSpec) -> ControlResult<PairId> {
        self.mutate(|i| {
            signed_in(i)?;
            if !Path::new(&spec.local_root).is_absolute() {
                return Err(ControlError::RootInvalid);
            }
            if i.status
                .pairs
                .iter()
                .any(|p| nested(&p.local_root, &spec.local_root))
            {
                return Err(ControlError::RootNested);
            }
            let pair_id = PairId(i.next_pair);
            let server_node_id = match &spec.target {
                PairTarget::NodeId { node_id } => {
                    if i.status.pairs.iter().any(|p| &p.server_node_id == node_id) {
                        return Err(ControlError::AlreadyPaired);
                    }
                    node_id.clone()
                }
                PairTarget::Path { .. } => {
                    NodeId::parse(&format!("00000000-0000-4000-8000-{:012x}", pair_id.0))
                        .expect("a formatted canonical UUID parses")
                }
            };
            i.next_pair += 1;
            i.status.pairs.push(PairView {
                pair_id,
                local_root: spec.local_root,
                server_node_id,
                state: PairState::Active,
                rules_text: spec.rules_text,
            });
            Ok(pair_id)
        })
    }

    async fn update_pair(&self, params: UpdatePairParams) -> ControlResult<()> {
        self.mutate(|i| {
            let pair = pair_mut(i, params.pair_id)?;
            if let Some(rules) = params.patch.rules_text {
                pair.rules_text = rules;
            }
            Ok(())
        })
    }

    async fn remove_pair(&self, params: PairIdParams) -> ControlResult<()> {
        self.mutate(|i| {
            pair_mut(i, params.pair_id)?;
            let id = params.pair_id;
            i.status.pairs.retain(|p| p.pair_id != id);
            i.status.holds.retain(|h| h.pair_id != id);
            i.status.access_denied.retain(|p| *p != id);
            i.status.watcher_degraded.retain(|p| *p != id);
            Ok(())
        })
    }

    async fn pause(&self, params: PairIdParams) -> ControlResult<()> {
        self.mutate(|i| {
            let pair = pair_mut(i, params.pair_id)?;
            if pair.state == PairState::Active {
                pair.state = PairState::Paused;
            }
            Ok(())
        })
    }

    async fn resume(&self, params: PairIdParams) -> ControlResult<()> {
        self.mutate(|i| {
            let pair = pair_mut(i, params.pair_id)?;
            if pair.state == PairState::Paused {
                pair.state = PairState::Active;
            }
            Ok(())
        })
    }

    async fn list_skipped(&self, params: ListEntriesParams) -> ControlResult<SkippedPage> {
        self.read(|i| {
            check_pair_filter(i, params.pair_id)?;
            let pages = i.skipped_pages.get(&entry_key(params.pair_id));
            let (items, next) = page_of(pages, params.page.as_ref())?;
            Ok(SkippedPage { items, next })
        })
    }

    async fn list_escaped(&self, params: ListEntriesParams) -> ControlResult<EscapedPage> {
        self.read(|i| {
            check_pair_filter(i, params.pair_id)?;
            let pages = i.escaped_pages.get(&entry_key(params.pair_id));
            let (items, next) = page_of(pages, params.page.as_ref())?;
            Ok(EscapedPage { items, next })
        })
    }

    async fn get_settings(&self) -> ControlResult<Settings> {
        Ok(self.read(|i| i.settings.clone()))
    }

    async fn set_settings(&self, patch: SettingsPatch) -> ControlResult<Settings> {
        self.mutate(|i| {
            if patch
                .hash_throttle
                .is_some_and(|v| !v.is_finite() || v < 0.0)
            {
                return Err(ControlError::SettingInvalid {
                    key: "hash_throttle".to_owned(),
                });
            }
            let settings = &mut i.settings;
            settings.bandwidth_limit = patch.bandwidth_limit.unwrap_or(settings.bandwidth_limit);
            settings.hash_throttle = patch.hash_throttle.unwrap_or(settings.hash_throttle);
            settings.autostart = patch.autostart.unwrap_or(settings.autostart);
            settings.tray = patch.tray.unwrap_or(settings.tray);
            Ok(settings.clone())
        })
    }

    async fn resolve_hold(&self, params: ResolveHoldParams) -> ControlResult<()> {
        self.mutate(|i| {
            let id = params.pair_id;
            if !i.status.holds.iter().any(|h| h.pair_id == id) {
                return Err(ControlError::NoSuchHold);
            }
            i.status.holds.retain(|h| h.pair_id != id);
            pair_mut(i, id)?.state = PairState::Active;
            Ok(())
        })
    }

    async fn login_start(&self, _params: LoginStartParams) -> ControlResult<LoginPrompt> {
        self.mutate(|i| {
            if i.status.server_url.is_none() {
                return Err(ControlError::ServerNotSet);
            }
            i.login = LoginState::Pending;
            Ok(i.prompt.clone())
        })
    }

    async fn login_status(&self) -> ControlResult<LoginState> {
        Ok(self.read(|i| i.login))
    }

    async fn logout(&self) -> ControlResult<LogoutOutcome> {
        self.mutate(|i| {
            i.status.account = None;
            i.login = LoginState::SignedOut;
            Ok(LogoutOutcome {
                server_revoked: true,
            })
        })
    }

    async fn subscribe(&self, sink: Arc<dyn StatusSink>) -> ControlResult<()> {
        self.mutate(|i| {
            i.sinks.push(sink);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_tokens_address_scripted_pages_in_order() {
        let pages = vec![vec![1], vec![2], vec![3]];

        let first = page_of(Some(&pages), None).unwrap();
        let second = page_of(Some(&pages), first.1.as_ref()).unwrap();
        let third = page_of(Some(&pages), second.1.as_ref()).unwrap();

        assert_eq!(first, (vec![1], Some(PageToken("page-1".to_owned()))));
        assert_eq!(second, (vec![2], Some(PageToken("page-2".to_owned()))));
        assert_eq!(third, (vec![3], None));
    }

    #[test]
    fn unknown_page_token_is_an_error_not_an_empty_page() {
        let pages = vec![vec![1]];

        let outcome = page_of(Some(&pages), Some(&PageToken("page-9".to_owned())));

        assert!(matches!(outcome, Err(ControlError::Internal { .. })));
    }

    #[test]
    fn roots_nest_in_both_directions_but_siblings_do_not() {
        assert!(nested("/home/u/Sync", "/home/u/Sync/Docs"));
        assert!(nested("/home/u/Sync/Docs", "/home/u/Sync"));
        assert!(!nested("/home/u/Sync", "/home/u/Syncing"));
    }
}
