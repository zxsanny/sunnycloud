//! ST-04-1 (AC-5): a stolen volume yields no plaintext and no master key. Files carrying the DS-09 marker and a
//! known string go through every write path (block uploads, `write_complete`, a discarded blob), then every byte
//! of the volume is scanned. The plain-root case scans the file tree, `.keycheck` and `.epoch`; the image case
//! also scans free space and metadata of the whole loop-mounted XFS image and needs the owner's volume.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use contracts::{BlobWriter, ByteStream, EpochMarker, StreamError};
use futures_core::Stream;
use sunnycloud_blobstore::FileEpochMarker;

use crate::kit::{KNOWN, hits, key_forms, marked, marker, store};
use crate::support::{KEY, MIB, Rig};

const XFS_MAGIC: u64 = 0x5846_5342;
const SEEDS: [u64; 4] = [20_261_103, 1, 2, 3];

struct Items(VecDeque<Bytes>);

impl Stream for Items {
    type Item = Result<Bytes, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front().map(Ok))
    }
}

fn stream_of(bytes: &[u8]) -> ByteStream {
    Box::pin(Items(
        bytes.chunks(100_000).map(Bytes::copy_from_slice).collect(),
    ))
}

/// Everything a thief would look for: the markers, the known string, the key bytes and their text forms.
fn needles() -> Vec<Vec<u8>> {
    let mut needles: Vec<Vec<u8>> = SEEDS.iter().map(|seed| marker(*seed)).collect();
    needles.push(KNOWN.as_bytes().to_vec());
    needles.push(KEY.to_vec());
    needles.extend(key_forms(&KEY).into_iter().map(String::into_bytes));
    needles
}

/// Uploads marked plaintext through every write path of the volume and writes the epoch marker.
async fn fill(rig: &Rig) -> Vec<Vec<u8>> {
    let sources = vec![
        marked(SEEDS[0], 5 * MIB + 7),
        marked(SEEDS[1], 2 * MIB),
        marked(SEEDS[2], 65_537),
    ];
    for source in &sources {
        store(rig, rig.account, source).await;
    }
    let small = marked(SEEDS[3], 70_000);
    rig.writer
        .write_complete(rig.account, small.len() as i64, stream_of(&small))
        .await
        .expect("write_complete");
    let discarded = store(rig, rig.account, &marked(SEEDS[1], 3 * MIB)).await;
    rig.writer
        .discard(discarded.blob_id)
        .await
        .expect("discard");
    FileEpochMarker::new(&rig.config)
        .expect("epoch marker")
        .write(7)
        .await
        .expect("epoch written");
    let mut all: Vec<Vec<u8>> = sources.iter().map(|source| source.to_vec()).collect();
    all.push(small.to_vec());
    all
}

fn files_under(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("list volume") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

/// Hits of every needle over a reader, in windows that overlap by the longest needle.
fn scan(mut source: impl std::io::Read, needles: &[Vec<u8>]) -> usize {
    let overlap = needles.iter().map(Vec::len).max().unwrap_or(1) - 1;
    let mut window = vec![0; 64 * MIB];
    let mut kept = 0;
    let mut total = 0;
    loop {
        let read = source.read(&mut window[kept..]).expect("read volume bytes");
        if read == 0 {
            return total
                + needles
                    .iter()
                    .map(|needle| hits(&window[..kept], needle))
                    .sum::<usize>();
        }
        let filled = kept + read;
        total += needles
            .iter()
            .map(|needle| hits(&window[..filled], needle))
            .sum::<usize>();
        kept = overlap.min(filled);
        window.copy_within(filled - kept..filled, 0);
        // A needle wholly inside the kept tail is counted again with the next window, so it is taken off here.
        total -= needles
            .iter()
            .map(|needle| hits(&window[..kept], needle))
            .sum::<usize>();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_file_tree_key_check_and_epoch_hold_no_marker_no_known_string_and_no_key() {
    // Arrange
    let rig = Rig::new().await;
    let sources = fill(&rig).await;
    let needles = needles();
    let mut files = Vec::new();
    files_under(&rig.volume, &mut files);

    // Act
    let found: Vec<(PathBuf, usize)> = files
        .iter()
        .map(|path| {
            let file = File::open(path).expect("volume file");
            (path.clone(), scan(file, &needles))
        })
        .filter(|(_, count)| *count > 0)
        .collect();

    // Assert
    let names: Vec<String> = files
        .iter()
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    assert!(names.contains(&".keycheck".to_owned()), "{names:?}");
    assert!(names.contains(&".epoch".to_owned()), "{names:?}");
    assert_eq!(files.len(), 6, "4 blobs, .keycheck, .epoch: {names:?}");
    assert!(
        sources.iter().all(|source| scan(&source[..], &needles) > 0),
        "the scanner finds the needles in the plaintext"
    );
    assert!(
        found.is_empty(),
        "plaintext or key on the volume: {found:?}"
    );
    rig.finish().await;
}

/// ST-04-1 on the whole image, free space included. Run by the owner on a loop-mounted XFS volume:
/// `TMPDIR=<writable dir on the mounted volume> ST_04_1_IMAGE=<the image file> cargo test -p sunnycloud-blobstore
/// --test security -- --ignored --exact stolen_disk::the_whole_loop_image_holds_no_marker_no_known_string_and_no_key`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "NOT-RUN(C-priv): needs a loop-mounted XFS volume (TMPDIR on it) and its image (ST_04_1_IMAGE)"]
async fn the_whole_loop_image_holds_no_marker_no_known_string_and_no_key() {
    // Arrange
    let image = std::env::var("ST_04_1_IMAGE").expect("ST_04_1_IMAGE names the loop image file");
    let filesystem = rustix::fs::statfs(std::env::temp_dir()).expect("statfs of TMPDIR");
    assert_eq!(
        filesystem.f_type as u64, XFS_MAGIC,
        "TMPDIR must be on the loop-mounted XFS volume"
    );
    let rig = Rig::new().await;
    fill(&rig).await;
    let root = File::open(&rig.volume).expect("volume root");
    rustix::fs::syncfs(&root).expect("syncfs");

    // Act
    let found = scan(File::open(&image).expect("image file"), &needles());

    // Assert
    assert_eq!(found, 0, "plaintext or key bytes in the image {image}");
    rig.finish().await;
}
