//! A test allocator that notes the largest single allocation a measured closure makes on its own
//! thread, so a test can tell a streamed read from one that holds a whole file.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static LARGEST: Cell<Option<usize>> = const { Cell::new(None) };
}

fn note(size: usize) {
    let _ = LARGEST.try_with(|largest| {
        if largest.get().is_some_and(|seen| size > seen) {
            largest.set(Some(size));
        }
    });
}

struct Tracking;

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        System.realloc(ptr, layout, new_size)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }
}

#[global_allocator]
static ALLOCATOR: Tracking = Tracking;

/// Runs `work` and returns its result with the size in bytes of the largest allocation this thread
/// made meanwhile.
pub fn largest_allocation<T>(work: impl FnOnce() -> T) -> (T, usize) {
    LARGEST.with(|largest| largest.set(Some(0)));
    let result = work();
    let seen = LARGEST.with(|largest| largest.replace(None)).unwrap_or(0);
    (result, seen)
}
