//! AZ-3842 AC-1 (IT-08-9, AC-43) and AC-3: accepted outputs are stored encrypted through 04, registered committed
//! with their `derived` row, and close the job on the last expected output; no transaction of 08 is open while a
//! slow body streams.

use bytes::Bytes;
use contracts::{AcceptOutcome, DerivedKind, StreamError};
use media_shared::types::JobKind;
use tokio::sync::mpsc;

use crate::accept_support::{Accept, Derived, WORKER, meta, mp4};
use crate::blob_support::WEBP;

#[tokio::test]
async fn stills_and_transcode_outputs_are_stored_encrypted_and_close_their_jobs() {
    // Arrange
    let a = Accept::new().await;
    let stills = a.claimed(JobKind::Stills, 10).await;
    let transcode = a.claimed(JobKind::Transcode, 11).await;
    let quota_before = a.quota_used().await;
    let movie = mp4(4096);

    // Act
    let preview = a
        .post(
            stills,
            DerivedKind::Preview,
            meta("image/webp", Some(1440), Some(1080)),
            WEBP.to_vec(),
        )
        .await
        .expect("preview");
    let after_preview = a.q.row(stills).await;
    let thumb = a
        .post(
            stills,
            DerivedKind::Thumb,
            meta("image/webp", Some(320), Some(240)),
            WEBP.to_vec(),
        )
        .await
        .expect("thumb");
    let movie_meta = meta("video/mp4", Some(1920), Some(1080));
    let transcoded = a
        .post(transcode, DerivedKind::Transcode, movie_meta, movie.clone())
        .await
        .expect("transcode");

    // Assert
    assert_eq!(
        (preview, thumb, transcoded),
        (
            AcceptOutcome::Accepted,
            AcceptOutcome::Accepted,
            AcceptOutcome::Accepted
        )
    );
    assert_eq!(after_preview.status, "running");
    for job in [stills, transcode] {
        let row = a.q.row(job).await;
        assert_eq!(
            (row.status.as_str(), row.outcome.as_deref()),
            ("succeeded", Some("derived"))
        );
    }
    let stills_rows = a.derived(10).await;
    let shape: Vec<_> = stills_rows
        .iter()
        .map(|row| {
            (
                row.kind.as_str(),
                row.media_type.as_str(),
                row.size,
                row.width,
                row.height,
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("preview", "image/webp", 34, Some(1440), Some(1080)),
            ("thumb", "image/webp", 34, Some(320), Some(240)),
        ]
    );
    let movie_rows: Vec<Derived> = a.derived(11).await;
    assert_eq!(movie_rows.len(), 1);
    assert_eq!(
        (
            movie_rows[0].kind.as_str(),
            movie_rows[0].media_type.as_str()
        ),
        ("transcode", "video/mp4")
    );
    for row in stills_rows.iter().chain(&movie_rows) {
        let raw = a.blobs.raw(row.blob_id);
        assert!(
            !raw.starts_with(b"RIFF"),
            "WebP signature in the stored file"
        );
        assert_ne!(
            raw.get(4..8),
            Some(&b"ftyp"[..]),
            "MP4 signature in the stored file"
        );
    }
    let states: Vec<_> = a
        .blob_rows()
        .await
        .into_iter()
        .map(|row| (row.1, row.2))
        .collect();
    assert_eq!(states, vec![("committed".to_owned(), None); 3]);
    assert_eq!(a.quota_used().await, quota_before);
    a.finish().await;
}

/// Open transactions of the API role in the test database, sampled by the superuser.
async fn open_api_transactions(a: &Accept) -> i64 {
    a.q.fixture
        .db
        .admin_count(
            "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() \
             AND usename = 'sunnycloud' AND xact_start IS NOT NULL",
        )
        .await
}

#[tokio::test]
async fn no_transaction_is_open_while_a_slow_body_streams() {
    // Arrange
    let a = Accept::new().await;
    let job = a.claimed(JobKind::Transcode, 12).await;
    let movie = mp4(2 * 1024 * 1024 - 32);
    let length = u64::try_from(movie.len()).expect("length");
    let chunks: Vec<Bytes> = Bytes::from(movie)
        .chunks(128 * 1024)
        .map(Bytes::copy_from_slice)
        .collect();
    let (sender, receiver) = mpsc::channel::<Bytes>(1);
    let body = Box::pin(futures_util::stream::unfold(
        receiver,
        |mut receiver| async move {
            let chunk = receiver.recv().await?;
            Some((Ok::<_, StreamError>(chunk), receiver))
        },
    ));
    let store = a.store.clone();
    let accepting = tokio::spawn(async move {
        store
            .accept_output(
                job,
                WORKER,
                DerivedKind::Transcode,
                meta("video/mp4", None, None),
                body,
                length,
            )
            .await
    });

    // Act
    let mut samples = Vec::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        // With room for one chunk, a finished send means the body has taken the chunk before it.
        sender.send(chunk).await.expect("the body is read");
        if index >= 2 && index % 4 == 2 {
            samples.push(open_api_transactions(&a).await);
        }
    }
    drop(sender);
    let outcome = accepting.await.expect("task").expect("accepted");

    // Assert
    assert_eq!(outcome, AcceptOutcome::Accepted);
    assert!(!samples.is_empty());
    assert!(
        samples.iter().all(|open| *open == 0),
        "open transactions while streaming: {samples:?}"
    );
    assert_eq!(a.derived(12).await.len(), 1);
    assert_eq!(a.q.row(job).await.status, "succeeded");
    a.finish().await;
}
