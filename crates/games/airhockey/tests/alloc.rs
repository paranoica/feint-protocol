//! Its own test binary: the counting allocator replaces the global one for everything here.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use airhockey::{Body, Input, Match, Params};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn tick_does_not_allocate() {
    let mut m = Match::new(Params::default(), [Body::default(); 2], 11);
    let mut events = Vec::with_capacity(8);
    let run = |m: &mut Match, events: &mut Vec<_>, ticks: u64| {
        for _ in 0..ticks {
            let t = m.tick();
            let puck = m.world.puck.pos;
            for player in 0..2 {
                let _ = m.apply(Input {
                    tick: t,
                    player,
                    target: [puck.x as f32, puck.y as f32],
                });
            }
            events.clear();
            m.step(events);
        }
    };
    run(&mut m, &mut events, 1_000);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    run(&mut m, &mut events, 10_000);
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(
        after - before,
        0,
        "{} allocations in 10k ticks",
        after - before
    );
    assert!(m.tick() > 1_000, "the match stopped early");
}
