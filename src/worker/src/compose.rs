//! The composition root of the supervisor (09 section 3, step 1): the settings become the one API client, the
//! two slots with their decoder client, scratch root and job loop, the status reporter and the `/readyz`
//! checks. Constructor injection only; every collaborator here is the production one.

use std::sync::Arc;
use std::time::Duration;

use sc_clock::Clock;
use sc_config::WorkerSettings;
use sc_observability::{Metrics, ReadinessRegistry};
use uuid::Uuid;

use crate::api::{ApiConfig, ApiConfigError, WorkerApiClient};
use crate::decoder_client::{Backoff, DecoderMetrics, Slot, SlotConfig, SlotName};
use crate::error::WorkerError;
use crate::scratch::ScratchRoot;
use crate::slot_loop::{ApiAccess, JobMetrics, LoopConfig, Shutdown, SlotLoop};
use crate::status::{SlotView, StatusReporter};

/// First and longest wait between attempts to reach a decoder. No key sets them yet (see `Backoff`).
const RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// Everything `run_worker` starts.
pub(crate) struct Assembled {
    pub(crate) slots: Vec<SlotLoop>,
    pub(crate) status: StatusReporter,
    pub(crate) roots: Vec<ScratchRoot>,
    pub(crate) readiness: ReadinessRegistry,
}

/// The keys that name one slot's decoder socket and scratch volume.
struct SlotKeys<'a> {
    name: SlotName,
    socket: Option<&'a str>,
    scratch: Option<&'a str>,
}

pub(crate) fn assemble(
    settings: &WorkerSettings,
    metrics: &Metrics,
    clock: Arc<dyn Clock>,
    shutdown: &Shutdown,
) -> Result<Assembled, WorkerError> {
    let gid = settings
        .decoder_gid
        .ok_or(WorkerError::Unset("decoder_gid"))?;
    let gid = u32::try_from(gid).map_err(|_| WorkerError::Unset("decoder_gid"))?;
    let api = Arc::new(WorkerApiClient::new(ApiConfig::from_settings(
        settings,
        worker_id(settings),
    ))?);
    let access = ApiAccess::new();
    let decoders = DecoderMetrics::register(metrics)?;
    let jobs = JobMetrics::register(metrics)?;
    let loop_config = LoopConfig::from_settings(settings);
    let forbidden = vec![api_target(&settings.worker_api_url)?];
    let backoff = Backoff::new(RECONNECT_INITIAL, RECONNECT_MAX)?;
    let mut readiness = ReadinessRegistry::new();
    readiness.add(Arc::new(access.clone()));
    let mut slots = Vec::new();
    let mut views = Vec::new();
    let mut roots = Vec::new();
    for keys in slot_keys(settings) {
        let socket = keys
            .socket
            .ok_or(WorkerError::Unset(socket_key(keys.name)))?;
        let scratch = keys
            .scratch
            .ok_or(WorkerError::Unset(scratch_key(keys.name)))?;
        let slot = Slot::new(
            SlotConfig {
                name: keys.name,
                socket: socket.into(),
                forbidden_targets: forbidden.clone(),
                handshake_timeout: settings.decoder_silence_timeout.get(),
                backoff,
            },
            Arc::clone(&decoders),
        );
        views.push(SlotView {
            name: keys.name,
            state: slot.subscribe(),
        });
        let root = ScratchRoot::new(scratch, gid);
        roots.push(root.clone());
        let slot_loop = SlotLoop::new(
            Arc::clone(&api),
            access.clone(),
            slot,
            root,
            loop_config,
            Arc::clone(&jobs),
            shutdown.clone(),
        );
        readiness.add(Arc::new(slot_loop.readiness()));
        slots.push(slot_loop);
    }
    let status = StatusReporter {
        api,
        access,
        clock,
        decoders,
        slots: views,
        every: settings.worker_status_interval.get(),
    };
    Ok(Assembled {
        slots,
        status,
        roots,
        readiness,
    })
}

fn slot_keys(settings: &WorkerSettings) -> [SlotKeys<'_>; 2] {
    [
        SlotKeys {
            name: SlotName::Interactive,
            socket: settings.worker_sock_interactive.as_deref(),
            scratch: settings.worker_scratch_interactive.as_deref(),
        },
        SlotKeys {
            name: SlotName::General,
            socket: settings.worker_sock_general.as_deref(),
            scratch: settings.worker_scratch_general.as_deref(),
        },
    ]
}

fn socket_key(name: SlotName) -> &'static str {
    match name {
        SlotName::Interactive => "worker_sock_interactive",
        SlotName::General => "worker_sock_general",
    }
}

fn scratch_key(name: SlotName) -> &'static str {
    match name {
        SlotName::Interactive => "worker_scratch_interactive",
        SlotName::General => "worker_scratch_general",
    }
}

/// `worker_id`, or the host name and a random suffix, so two supervisors on one host differ.
fn worker_id(settings: &WorkerSettings) -> String {
    settings.worker_id.clone().unwrap_or_else(|| {
        let host = rustix::system::uname()
            .nodename()
            .to_string_lossy()
            .into_owned();
        format!("{host}-{}", &Uuid::new_v4().simple().to_string()[..8])
    })
}

/// `host:port` of the API, the one address the isolated decoder must fail to reach (`self_check`).
fn api_target(url: &str) -> Result<String, ApiConfigError> {
    let url = reqwest::Url::parse(url).map_err(|_| ApiConfigError::Url)?;
    let host = url.host_str().ok_or(ApiConfigError::Url)?;
    let port = url.port_or_known_default().ok_or(ApiConfigError::Url)?;
    Ok(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_api_target_carries_the_port_of_the_url() {
        assert_eq!(
            api_target("http://api:8081").expect("target"),
            "api:8081".to_owned()
        );
    }

    #[test]
    fn a_url_without_a_port_gets_the_scheme_default() {
        assert_eq!(
            api_target("http://api/").expect("target"),
            "api:80".to_owned()
        );
    }

    #[test]
    fn a_url_that_does_not_parse_is_refused() {
        assert!(api_target("not a url").is_err());
    }
}
