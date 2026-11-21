//! The announcement of previews (`ThumbNotifier`): ready when the job that makes a `thumb` closes with its outputs
//! stored, failed when such a job dies (by a failure report or by the reaper), nothing for other jobs, a retry or a
//! partial output.

use std::sync::{Arc, Mutex, PoisonError};

use contracts::{
    DerivedKind, ThumbEvent, ThumbNotifier, ThumbState, ThumbSubscription, ThumbWake, async_trait,
};
use media_api::Priority;
use media_shared::types::{Disposition, JobKind, Lane};

use crate::accept_support::{Accept, WORKER};
use crate::queue_support::{ATTEMPTS, LEASE, Queue, config, hash};
use crate::support::Fixture;

#[derive(Default)]
struct Recorder(Mutex<Vec<ThumbEvent>>);

impl Recorder {
    fn events(&self) -> Vec<ThumbEvent> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

struct Never;

#[async_trait]
impl ThumbSubscription for Never {
    async fn next(&mut self) -> ThumbWake {
        std::future::pending().await
    }
}

impl ThumbNotifier for Recorder {
    fn notify(&self, event: ThumbEvent) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }

    fn subscribe(&self) -> Box<dyn ThumbSubscription> {
        Box::new(Never)
    }
}

async fn accept(recorder: &Arc<Recorder>) -> Accept {
    let thumbs: Arc<dyn ThumbNotifier> = recorder.clone();
    Accept::with_thumbs(
        config(),
        |tree| tree,
        |blobs| blobs.registry.clone(),
        Some(thumbs),
    )
    .await
}

#[tokio::test]
async fn a_stills_job_announces_ready_when_its_last_output_closes_it_and_not_before() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let a = accept(&recorder).await;
    let job = a.claimed(JobKind::Stills, 1).await;

    // Act
    a.post_webp(job, DerivedKind::Thumb).await;
    let after_thumb = recorder.events();
    a.post_webp(job, DerivedKind::Preview).await;

    // Assert
    assert!(after_thumb.is_empty(), "{after_thumb:?}");
    assert_eq!(
        recorder.events(),
        [ThumbEvent {
            account: a.q.account,
            content_hash: hash(1),
            state: ThumbState::Ready
        }]
    );
}

#[tokio::test]
async fn a_poster_job_announces_ready_with_its_one_output() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let a = accept(&recorder).await;
    let job = a.claimed(JobKind::Poster, 2).await;

    // Act
    a.post_webp(job, DerivedKind::Thumb).await;

    // Assert
    let states: Vec<_> = recorder
        .events()
        .iter()
        .map(|e| (e.content_hash, e.state))
        .collect();
    assert_eq!(states, [(hash(2), ThumbState::Ready)]);
}

#[tokio::test]
async fn a_dead_stills_job_announces_failed_and_a_retry_and_a_dead_transcode_announce_nothing() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let thumbs: Arc<dyn ThumbNotifier> = recorder.clone();
    let q = Queue::with_thumbs(Fixture::new().await, Some(thumbs)).await;
    let stills = q.add(JobKind::Stills, 3, Priority::Interactive).await;
    let transcode = q.add(JobKind::Transcode, 4, Priority::Interactive).await;
    q.queue
        .claim(Lane::Image, WORKER)
        .await
        .expect("claim")
        .expect("stills");
    q.queue
        .claim(Lane::Video, WORKER)
        .await
        .expect("claim")
        .expect("transcode");

    // Act
    q.queue
        .fail(transcode, WORKER, "x", Disposition::Permanent)
        .await
        .expect("fail transcode");
    q.queue
        .fail(stills, WORKER, "x", Disposition::Retry)
        .await
        .expect("retry stills");
    let after_retry = recorder.events();
    q.clock.tick(std::time::Duration::from_secs(5));
    q.queue
        .claim(Lane::Image, WORKER)
        .await
        .expect("claim")
        .expect("stills again");
    q.queue
        .fail(stills, WORKER, "x", Disposition::Permanent)
        .await
        .expect("fail stills");

    // Assert
    assert!(after_retry.is_empty(), "{after_retry:?}");
    assert_eq!(
        recorder.events(),
        [ThumbEvent {
            account: q.account,
            content_hash: hash(3),
            state: ThumbState::Failed
        }]
    );
}

#[tokio::test]
async fn a_job_the_reaper_kills_announces_failed() {
    // Arrange
    let recorder = Arc::new(Recorder::default());
    let thumbs: Arc<dyn ThumbNotifier> = recorder.clone();
    let q = Queue::with_thumbs(Fixture::new().await, Some(thumbs)).await;
    q.add(JobKind::Stills, 5, Priority::Interactive).await;
    for _ in 0..ATTEMPTS {
        q.queue
            .claim(Lane::Image, WORKER)
            .await
            .expect("claim")
            .expect("job");
        q.clock.tick(LEASE + std::time::Duration::from_secs(1));
    }

    // Act
    q.queue.reap(q.clock_now()).await.expect("reap");

    // Assert
    let states: Vec<_> = recorder
        .events()
        .iter()
        .map(|e| (e.content_hash, e.state))
        .collect();
    assert_eq!(states, [(hash(5), ThumbState::Failed)]);
}
