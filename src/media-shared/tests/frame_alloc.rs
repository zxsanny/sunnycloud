//! AC-2: no buffer above 64 KiB is allocated, measured with a counting allocator.
//!
//! One test only: the allocator watches the whole process while it is armed.

mod common;

use common::frame;
use media_shared::decoder_protocol::{FrameReader, MAX_PAYLOAD, Polled, ReadError, ViolationKind};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static ARMED: AtomicBool = AtomicBool::new(false);
static LARGEST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ARMED.load(Ordering::SeqCst) {
            LARGEST.fetch_max(layout.size(), Ordering::SeqCst);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Runs `work` and returns the largest single allocation made meanwhile.
fn largest_allocation<T>(work: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.store(0, Ordering::SeqCst);
    ARMED.store(true, Ordering::SeqCst);
    let result = work();
    ARMED.store(false, Ordering::SeqCst);
    (result, LARGEST.load(Ordering::SeqCst))
}

#[test]
fn no_frame_causes_an_allocation_above_the_cap() {
    let huge = u32::MAX.to_be_bytes().to_vec();
    let just_over = (u32::try_from(MAX_PAYLOAD).expect("fits") + 1)
        .to_be_bytes()
        .to_vec();
    let mut full = br#"{"request":"cancel"}"#.to_vec();
    full.resize(MAX_PAYLOAD, b' ');
    let full = frame(&full);

    for (name, wire) in [("4 GiB prefix", huge), ("65537 prefix", just_over)] {
        let (result, largest) =
            largest_allocation(|| FrameReader::new().poll_request(&mut wire.as_slice()));
        assert!(
            matches!(&result, Err(ReadError::Violation(v)) if v.0 == ViolationKind::Oversize),
            "{name}: {result:?}"
        );
        assert!(largest < 1024, "{name}: allocated {largest} bytes");
    }

    let (result, largest) =
        largest_allocation(|| FrameReader::new().poll_request(&mut full.as_slice()));
    assert!(matches!(result, Ok(Polled::Ready(_))), "{result:?}");
    assert!(
        largest <= MAX_PAYLOAD,
        "a 64 KiB frame allocated {largest} bytes"
    );

    let started = std::time::Instant::now();
    let parsed = FrameReader::new().poll_request(&mut full.as_slice());
    assert!(matches!(parsed, Ok(Polled::Ready(_))));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(40),
        "parsing a 64 KiB frame took {:?}",
        started.elapsed()
    );
}
