//! NFR: `now_utc` and `monotonic` do not allocate. One test per binary, so no other thread allocates
//! while the counter is read.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sc_clock::{Clock, ManualClock, SystemClock, Timestamp};

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded unchanged to the system allocator; only a counter is added.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn allocations_during<T>(read: impl Fn() -> T) -> usize {
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    for _ in 0..1_000 {
        black_box(read());
    }
    ALLOCATIONS.load(Ordering::SeqCst) - before
}

#[test]
fn clock_reads_do_not_allocate() {
    let system = SystemClock::new();
    let manual = ManualClock::new(Timestamp::from_unix_micros(0));
    manual.tick(Duration::from_secs(1));

    assert_eq!(0, allocations_during(|| system.now_utc()));
    assert_eq!(0, allocations_during(|| system.monotonic()));
    assert_eq!(0, allocations_during(|| manual.now_utc()));
    assert_eq!(0, allocations_during(|| manual.monotonic()));

    #[cfg(feature = "test-clock")]
    {
        let offset = sc_clock::OffsetClock::new();
        offset.advance(Duration::from_secs(1)).expect("advance");
        assert_eq!(0, allocations_during(|| offset.now_utc()));
        assert_eq!(0, allocations_during(|| offset.monotonic()));
    }
}
