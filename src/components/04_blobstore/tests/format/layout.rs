//! AC-4: file length, chunk offsets and the 4 KiB range bound over the size corpus.

use contracts::ByteRange;
use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, HEADER_LEN, SEALED_CHUNK_LEN};

const MIB: u64 = 1024 * 1024;
const CORPUS: [u64; 7] = [0, 1, 65_535, 65_536, 65_537, 2 * MIB, 5 * MIB + 7];

fn expected_chunks(size: u64) -> u64 {
    if size == 0 { 1 } else { size.div_ceil(65_536) }
}

#[test]
fn the_constants_are_the_documented_format() {
    assert_eq!(CHUNK_SIZE, 65_536);
    assert_eq!(SEALED_CHUNK_LEN, 65_552);
    assert_eq!(HEADER_LEN, 64);
}

#[test]
fn file_length_is_header_plus_tags_plus_size() {
    for size in CORPUS {
        let layout = BlobLayout::new(size).expect("layout");
        let chunks = expected_chunks(size);

        assert_eq!(layout.chunk_count(), chunks, "size {size}");
        assert_eq!(layout.file_len(), 64 + chunks * 16 + size, "size {size}");
    }
}

#[test]
fn chunk_i_starts_at_64_plus_i_times_65552() {
    for size in CORPUS {
        let layout = BlobLayout::new(size).expect("layout");

        for index in 0..layout.chunk_count() {
            assert_eq!(
                layout.chunk_offset(index),
                Some(64 + index * 65_552),
                "size {size} chunk {index}"
            );
        }
        assert_eq!(
            layout.chunk_offset(layout.chunk_count()),
            None,
            "size {size}"
        );
    }
}

#[test]
fn chunks_tile_the_file_exactly() {
    for size in CORPUS {
        let layout = BlobLayout::new(size).expect("layout");
        let last = layout.chunk_count() - 1;

        let end =
            layout.chunk_offset(last).expect("offset") + layout.plain_len(last).expect("len") + 16;
        let plain: u64 = (0..layout.chunk_count())
            .map(|i| layout.plain_len(i).expect("len"))
            .sum();

        assert_eq!(end, layout.file_len(), "size {size}");
        assert_eq!(plain, size, "size {size}");
        assert!(layout.is_last(last));
        assert!(last == 0 || !layout.is_last(last - 1));
    }
}

#[test]
fn every_4_kib_range_maps_to_at_most_two_chunks() {
    for size in CORPUS.into_iter().filter(|size| *size >= 4096) {
        let layout = BlobLayout::new(size).expect("layout");
        let mut starts: Vec<u64> = (0..size - 4095).step_by(1021).collect();
        starts.extend([0, 65_534, 65_535, 65_536, size - 4096]);

        for start in starts.into_iter().filter(|start| start + 4095 < size) {
            let span = layout
                .map_range(ByteRange {
                    start,
                    end: start + 4095,
                })
                .expect("range");

            assert!(
                span.last_chunk - span.first_chunk <= 1,
                "size {size} start {start}"
            );
            assert_eq!(span.len, 4096);
            assert_eq!(span.first_chunk, start / 65_536);
            assert_eq!(span.skip as u64, start % 65_536);
        }
    }
}

#[test]
fn a_range_spanning_three_chunks_names_them() {
    let layout = BlobLayout::new(3 * MIB).expect("layout");

    let span = layout
        .map_range(ByteRange {
            start: 65_535,
            end: 2 * 65_536,
        })
        .expect("range");

    assert_eq!(
        (span.first_chunk, span.last_chunk, span.skip, span.len),
        (0, 2, 65_535, 65_538)
    );
}

#[test]
fn the_whole_file_maps_to_every_chunk() {
    let layout = BlobLayout::new(5 * MIB + 7).expect("layout");

    let span = layout
        .map_range(ByteRange {
            start: 0,
            end: 5 * MIB + 6,
        })
        .expect("range");

    assert_eq!(
        (span.first_chunk, span.last_chunk, span.skip),
        (0, layout.chunk_count() - 1, 0)
    );
    assert_eq!(span.len, 5 * MIB + 7);
}
