//! AZ-3869 AC-2 (hash-verified input, AC-72 worker side) and AC-3 (size limits, IT-09-7).

mod api_support;

use std::fs::File;
use std::time::Duration;

use api_support::{MIB, Peer, Reply, Scratch, WORKER_ID, client, config, file_hash, job, pattern};
use media_shared::types::{ClaimedJob, JobKind};
use sc_ids::FileHash;
use sunnycloud_worker::{ApiError, WorkerApiClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const VECTORS: &str = include_str!("../../shared/ids_hashing/tests/vectors/hashing.txt");

fn hex_hash(text: &str) -> FileHash {
    let mut bytes = [0u8; 32];
    for (slot, pair) in bytes.iter_mut().zip(text.as_bytes().chunks(2)) {
        let pair = std::str::from_utf8(pair).expect("hex is ascii");
        *slot = u8::from_str_radix(pair, 16).expect("hex digit");
    }
    FileHash::from_bytes(bytes)
}

fn limited_client(base_url: &str, photo: u64, video: u64) -> WorkerApiClient {
    let mut config = config(base_url);
    config.max_input_photo = photo;
    config.max_input_video = video;
    WorkerApiClient::new(config).expect("client")
}

#[tokio::test]
async fn bytes_that_match_the_hash_vectors_land_whole_in_the_destination() {
    // Arrange: 0 B, 1 B, 2 MiB and 2 MiB + 1 B of the shared known-answer vectors.
    let cases: Vec<(&str, usize, FileHash)> = VECTORS
        .lines()
        .filter(|line| line.starts_with("file "))
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            (
                fields[1],
                fields[2].parse().expect("length"),
                hex_hash(fields[3]),
            )
        })
        .collect();
    assert_eq!(cases.len(), 4, "the vector file carries four file cases");

    for (name, length, hash) in cases {
        let bytes = pattern(length);
        let served = bytes.clone();
        let peer = Peer::start(move |_| Reply::bytes(served.clone())).await;
        let scratch = Scratch::new(job(JobKind::Stills, hash).job_id);

        // Act
        let stored = client(&peer.base_url())
            .fetch_input(&job(JobKind::Stills, hash), scratch.source())
            .await;

        // Assert
        assert_eq!(stored.expect(name), length as u64, "{name}");
        assert_eq!(scratch.source_bytes(), bytes, "{name}");
        assert_eq!(file_hash(&scratch.source_bytes()), hash, "{name}");
    }
}

#[tokio::test]
async fn a_hash_that_does_not_match_the_served_bytes_is_refused_and_leaves_no_input() {
    // Arrange: same size, one byte differs, in the last block of a multi-block body.
    let claimed = pattern(2 * MIB as usize + 1);
    let mut served = claimed.clone();
    served[claimed.len() - 1] ^= 0xff;
    let peer = Peer::start(move |_| Reply::bytes(served.clone())).await;
    let scratch = Scratch::new(job(JobKind::Transcode, file_hash(&claimed)).job_id);

    // Act
    let result = client(&peer.base_url())
        .fetch_input(
            &job(JobKind::Transcode, file_hash(&claimed)),
            scratch.source(),
        )
        .await;

    // Assert
    assert!(matches!(result, Err(ApiError::HashMismatch)), "{result:?}");
    assert_eq!(scratch.source_len(), 0);
}

#[tokio::test]
async fn bytes_of_a_wrong_revision_never_reach_the_destination_as_usable_input() {
    // Arrange: the claim names one revision's hash, the API serves another revision's plaintext.
    let wanted = file_hash(&pattern(4096));
    let peer = Peer::start(|_| Reply::bytes(vec![7; 4096])).await;
    let scratch = Scratch::new(job(JobKind::Stills, wanted).job_id);

    // Act
    let result = client(&peer.base_url())
        .fetch_input(&job(JobKind::Stills, wanted), scratch.source())
        .await;

    // Assert
    assert!(matches!(result, Err(ApiError::HashMismatch)), "{result:?}");
    assert_eq!(scratch.source_len(), 0);
}

#[tokio::test]
async fn a_photo_declared_over_the_photo_limit_is_refused_before_any_byte_is_written() {
    // Arrange
    let body = pattern(2 * MIB as usize);
    let hash = file_hash(&body);
    let peer = Peer::start(move |_| Reply::bytes(body.clone())).await;
    let scratch = Scratch::new(job(JobKind::Stills, hash).job_id);

    // Act
    let result = limited_client(&peer.base_url(), MIB, 10 * MIB)
        .fetch_input(&job(JobKind::Stills, hash), scratch.source())
        .await;

    // Assert
    assert!(matches!(result, Err(ApiError::TooLarge)), "{result:?}");
    assert_eq!(scratch.source_len(), 0);
}

#[tokio::test]
async fn video_kinds_are_held_to_the_video_limit_not_the_photo_limit() {
    // Arrange: 2 MiB fits `worker_max_input_video` (4 MiB) but not `worker_max_input_photo` (1 MiB).
    let body = pattern(2 * MIB as usize);
    let hash = file_hash(&body);
    let peer = Peer::start(move |_| Reply::bytes(body.clone())).await;
    let api = limited_client(&peer.base_url(), MIB, 4 * MIB);
    let poster = Scratch::new(job(JobKind::Poster, hash).job_id);
    let still = Scratch::new(job(JobKind::Stills, hash).job_id);

    // Act
    let poster_result = api
        .fetch_input(&job(JobKind::Poster, hash), poster.source())
        .await;
    let still_result = api
        .fetch_input(&job(JobKind::Stills, hash), still.source())
        .await;

    // Assert
    assert_eq!(poster_result.expect("poster fits"), 2 * MIB);
    assert!(
        matches!(still_result, Err(ApiError::TooLarge)),
        "{still_result:?}"
    );
}

/// Serves one chunked answer without `Content-Length` that never ends, and reports how it stopped.
async fn endless_peer() -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0u8; 2048];
        if stream.read(&mut request).await? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await?;
        let chunk = [b'x'; 4096];
        loop {
            stream.write_all(b"1000\r\n").await?;
            stream.write_all(&chunk).await?;
            stream.write_all(b"\r\n").await?;
        }
    });
    (url, task)
}

#[tokio::test]
async fn a_body_that_grows_past_the_limit_is_aborted_there() {
    // Arrange: no `Content-Length`, so only the streaming check can stop it.
    let (url, server) = endless_peer().await;
    let scratch = Scratch::new(job(JobKind::Stills, file_hash(b"")).job_id);

    // Act
    let result = limited_client(&url, MIB, 10 * MIB)
        .fetch_input(&job(JobKind::Stills, file_hash(b"")), scratch.source())
        .await;

    // Assert
    assert!(matches!(result, Err(ApiError::TooLarge)), "{result:?}");
    assert_eq!(scratch.source_len(), 0);
    let ended = tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("the peer saw the connection close")
        .expect("peer task");
    assert!(ended.is_err(), "the endless peer ended by a write error");
}

#[tokio::test]
async fn a_destination_that_cannot_be_written_is_reported_as_such() {
    // Arrange: a read-only handle.
    let body = pattern(1024);
    let hash = file_hash(&body);
    let peer = Peer::start(move |_| Reply::bytes(body.clone())).await;
    let scratch = Scratch::new(job(JobKind::Stills, hash).job_id);
    drop(scratch.source());
    let read_only = File::open(scratch.dir.source_path()).expect("open read-only");

    // Act
    let result = client(&peer.base_url())
        .fetch_input(&job(JobKind::Stills, hash), read_only)
        .await;

    // Assert
    assert!(
        matches!(result, Err(ApiError::Destination(_))),
        "{result:?}"
    );
}

#[tokio::test]
async fn the_fetch_asks_for_the_whole_input_of_the_claimed_job() {
    // Arrange
    let body = pattern(100);
    let claimed: ClaimedJob = job(JobKind::Stills, file_hash(&body));
    let peer = Peer::start(move |_| Reply::bytes(body.clone())).await;
    let scratch = Scratch::new(claimed.job_id);

    // Act
    let result = client(&peer.base_url())
        .fetch_input(&claimed, scratch.source())
        .await;

    // Assert
    assert_eq!(result.expect("fetched"), 100);
    let seen = peer.requests();
    assert_eq!(seen[0].method, "GET");
    assert_eq!(
        seen[0].path,
        format!("/api/v1/worker-jobs/{}/input", claimed.job_id)
    );
    assert_eq!(seen[0].header("range"), None);
    assert_eq!(seen[0].header("x-worker-id"), Some(WORKER_ID));
}
