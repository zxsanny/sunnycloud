//! AC-2: the 32-bit chunk counter is never wrapped and its maximum is never used.

use sunnycloud_blobstore::{BlobLayout, CHUNK_SIZE, COUNTER_LIMIT, FormatError};

use crate::support::cipher;

#[test]
fn sealing_at_the_counter_limit_is_refused() {
    let mut buffer = vec![1, 2, 3];

    let result = cipher().seal(u64::from(u32::MAX), false, &mut buffer);

    assert_eq!(result, Err(FormatError::CounterLimit));
    assert_eq!(buffer, [1, 2, 3], "the buffer is left as it was");
}

#[test]
fn sealing_the_last_chunk_at_the_counter_limit_is_refused_too() {
    let result = cipher().seal(u64::from(u32::MAX), true, &mut vec![0; 8]);

    assert_eq!(result, Err(FormatError::CounterLimit));
}

#[test]
fn an_index_that_would_wrap_to_a_small_counter_is_refused() {
    let wrapping = u64::from(u32::MAX) + 1;

    assert_eq!(
        cipher().seal(wrapping, false, &mut vec![0; 8]),
        Err(FormatError::CounterLimit)
    );
    assert_eq!(
        cipher().seal(u64::MAX, false, &mut vec![0; 8]),
        Err(FormatError::CounterLimit)
    );
}

#[test]
fn the_highest_usable_index_is_below_the_limit() {
    let mut buffer = vec![0; 8];

    let result = cipher().seal(COUNTER_LIMIT - 1, true, &mut buffer);

    assert_eq!(result, Ok(()));
    assert_eq!(buffer.len(), 8 + 16);
}

#[test]
fn opening_at_the_counter_limit_is_out_of_range_not_a_wrapped_counter() {
    let result = cipher().open(u64::from(u32::MAX), false, &mut vec![0; 32]);

    assert!(matches!(result, Err(contracts::BlobError::OutOfRange)));
}

#[test]
fn a_size_whose_last_chunk_would_reach_the_limit_is_refused() {
    let largest = COUNTER_LIMIT * CHUNK_SIZE;

    assert!(BlobLayout::new(largest).is_ok());
    assert_eq!(BlobLayout::new(largest + 1), Err(FormatError::CounterLimit));
    assert_eq!(BlobLayout::new(u64::MAX), Err(FormatError::CounterLimit));
}
