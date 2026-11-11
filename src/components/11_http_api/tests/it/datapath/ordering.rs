//! AC-2 (AC-69): a commit stub that returns increasing `seq` values after random delays, called by 50
//! concurrent clients. The `seq` travels in the `revision_id` of `Committed` (the port has no `seq` field).

use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use contracts::{
    BatchCommit, BatchResult, CommitActor, CommitService, CommitServiceError, CommitUpload,
    Committed,
};
use serde_json::json;
use uuid::Uuid;

use super::support::*;
use crate::auth::support::MEMBER_TOKEN;
use crate::harness::script::unscripted;
use crate::worker::support::send;

const CLIENTS: u32 = 50;
const COMMITS_PER_CLIENT: u32 = 20;
const MAX_DELAY_MS: u64 = 50;
/// Two completions this far apart are ordered for the clients as well; closer ones may swap in the scheduler.
const ORDER_MARGIN: Duration = Duration::from_millis(40);

struct Completion {
    seq: u64,
    at: Instant,
}

/// Completes each call after a random delay and gives it the next `seq` at that moment.
struct DelayedCommits {
    next_seq: AtomicU64,
    random: Mutex<u64>,
    completed: Mutex<HashMap<String, Completion>>,
}

impl DelayedCommits {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            next_seq: AtomicU64::new(1),
            random: Mutex::new(0x9E37_79B9_7F4A_7C15),
            completed: Mutex::new(HashMap::new()),
        })
    }

    /// xorshift64: the delays are reproducible, so a failure can be replayed.
    fn delay(&self) -> Duration {
        let mut state = self.random.lock().unwrap_or_else(PoisonError::into_inner);
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        Duration::from_millis(*state % (MAX_DELAY_MS + 1))
    }
}

#[contracts::async_trait]
impl CommitService for DelayedCommits {
    async fn commit_upload(
        &self,
        _actor: CommitActor,
        request: CommitUpload,
    ) -> Result<Committed, CommitServiceError> {
        tokio::time::sleep(self.delay()).await;
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        self.completed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(
                request.name,
                Completion {
                    seq,
                    at: Instant::now(),
                },
            );
        Ok(Committed {
            node_id: NODE,
            revision_id: Uuid::from_u128(u128::from(seq)),
            conflict_copy: None,
        })
    }

    async fn commit_batch(
        &self,
        _actor: CommitActor,
        _request: BatchCommit,
    ) -> Result<BatchResult, CommitServiceError> {
        Err(CommitServiceError::Unexpected(unscripted(
            "CommitService::commit_batch",
        )))
    }
}

/// What one client saw of one commit.
struct Received {
    name: String,
    seq: u64,
    at: Instant,
}

async fn client(addr: SocketAddr, client: u32) -> Vec<Received> {
    let auth = bearer(MEMBER_TOKEN);
    let mut received = Vec::new();
    for index in 0..COMMITS_PER_CLIENT {
        let name = format!("c-{client}-{index}");
        let body = json!({
            "upload_id": UPLOAD,
            "parent_id": FOLDER,
            "name": name,
            "blocklist": [hex(1)]
        })
        .to_string();
        let reply = send(
            addr,
            "POST",
            "/api/v1/commits",
            &[("authorization", auth.as_str())],
            body.as_bytes(),
        )
        .await;
        let at = Instant::now();
        assert_eq!(reply.status, 200, "{name}");
        let revision = reply.json()["revision_id"]
            .as_str()
            .and_then(|text| Uuid::parse_str(text).ok())
            .expect("the answer names a revision");
        received.push(Received {
            name,
            seq: u64::try_from(revision.as_u128()).expect("a seq fits 64 bits"),
            at,
        });
    }
    received
}

#[tokio::test]
async fn ac2_a_thousand_commits_from_fifty_clients_each_get_their_own_seq_in_the_order_the_port_completed()
 {
    // Arrange
    let commits = DelayedCommits::new();
    let stack = Stack::start_with(|_, services| contracts_with(services, commits.clone())).await;

    // Act
    let clients: Vec<_> = (0..CLIENTS)
        .map(|number| tokio::spawn(client(stack.public, number)))
        .collect();
    let mut received = Vec::new();
    for task in clients {
        received.extend(task.await.expect("a client does not panic"));
    }

    stack.stop().await;

    // Assert
    let completed = commits
        .completed
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    assert_eq!(received.len(), 1000);
    assert_eq!(completed.len(), 1000);
    let mismatches = received
        .iter()
        .filter(|got| completed.get(&got.name).map(|done| done.seq) != Some(got.seq))
        .count();
    assert_eq!(mismatches, 0);
    let seqs: BTreeSet<u64> = received.iter().map(|got| got.seq).collect();
    assert_eq!(seqs, (1..=1000).collect::<BTreeSet<u64>>());
    received.sort_by_key(|got| got.at);
    let mut swapped = 0;
    for (index, earlier) in received.iter().enumerate() {
        let earlier_done = completed[&earlier.name].at;
        for later in &received[index + 1..] {
            if completed[&later.name].at + ORDER_MARGIN < earlier_done {
                swapped += 1;
            }
        }
    }
    assert_eq!(
        swapped, 0,
        "responses left in another order than the port completed"
    );
}

fn contracts_with(
    services: sunnycloud_http_api::ApiServices,
    commits: Arc<DelayedCommits>,
) -> sunnycloud_http_api::ApiServices {
    sunnycloud_http_api::ApiServices {
        commits,
        ..services
    }
}
