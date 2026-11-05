//! NFR: building a tier-2 error allocates nothing beyond the value itself.

use sc_error::{BusinessError, BusinessParams, ErrorCode};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: forwards every call unchanged to the system allocator; the counter is a const-initialised thread-local.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations_during<T>(build: impl FnOnce() -> T) -> (usize, T) {
    let before = ALLOCATIONS.with(Cell::get);
    let value = build();
    (ALLOCATIONS.with(Cell::get) - before, value)
}

#[test]
fn tier_two_construction_does_not_allocate() {
    let (count, error) = allocations_during(|| {
        BusinessError::new(
            ErrorCode::RateLimited,
            BusinessParams {
                retry_after: Some(Duration::from_secs(3)),
                ..BusinessParams::default()
            },
        )
    });
    assert_eq!(count, 0);
    assert_eq!(error.code, ErrorCode::RateLimited);

    let (count, not_found) = allocations_during(BusinessError::not_found);
    assert_eq!(count, 0);
    assert_eq!(not_found.code, ErrorCode::NotFound);
}
