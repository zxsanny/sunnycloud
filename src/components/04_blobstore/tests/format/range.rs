//! AC-5: unsatisfiable ranges are classified from the declared size alone; no file is involved.

use contracts::{BlobError, ByteRange};
use sunnycloud_blobstore::BlobLayout;

const SIZE: u64 = 3 * 1024 * 1024;

fn map(start: u64, end: u64) -> Result<(), BlobError> {
    BlobLayout::new(SIZE)
        .expect("layout")
        .map_range(ByteRange { start, end })
        .map(|_| ())
}

#[test]
fn a_start_at_the_size_is_not_satisfiable() {
    assert!(matches!(
        map(SIZE, SIZE),
        Err(BlobError::RangeNotSatisfiable)
    ));
}

#[test]
fn an_end_past_the_size_is_not_satisfiable() {
    assert!(matches!(
        map(0, SIZE + 1),
        Err(BlobError::RangeNotSatisfiable)
    ));
    assert!(matches!(
        map(10, u64::MAX),
        Err(BlobError::RangeNotSatisfiable)
    ));
}

#[test]
fn an_end_at_the_size_is_one_byte_past_the_last_byte_and_not_satisfiable() {
    assert!(matches!(map(0, SIZE), Err(BlobError::RangeNotSatisfiable)));
}

#[test]
fn an_inverted_range_is_not_satisfiable() {
    assert!(matches!(map(100, 99), Err(BlobError::RangeNotSatisfiable)));
}

#[test]
fn the_first_and_the_last_byte_are_satisfiable() {
    assert!(map(0, 0).is_ok());
    assert!(map(SIZE - 1, SIZE - 1).is_ok());
}

#[test]
fn every_range_of_an_empty_blob_is_not_satisfiable() {
    let layout = BlobLayout::new(0).expect("layout");

    assert!(matches!(
        layout.map_range(ByteRange { start: 0, end: 0 }),
        Err(BlobError::RangeNotSatisfiable)
    ));
}
