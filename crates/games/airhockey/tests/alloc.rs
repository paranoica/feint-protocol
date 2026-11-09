use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use airhockey::{AirHockey, Body, Input, Match, Params};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, ObservationT};

struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
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
    let before = allocations();
    run(&mut m, &mut events, 10_000);
    let after = allocations();
    assert_eq!(
        after - before,
        0,
        "{} allocations in 10k ticks",
        after - before
    );
    assert!(m.tick() > 1_000, "the match stopped early");
}

#[test]
fn adapter_tick_does_not_allocate() {
    let mut game = AirHockey::new(Params::default(), 1);
    let mut body = BodyParamsT::default();
    body.values = Some(vec![5.0, 40.0]);
    game.reset(5, &[body.clone(), body]).unwrap();
    let mut events = Vec::with_capacity(8);
    let mut obs = [ObservationT::default(), ObservationT::default()];
    let mut action = ActionT::default();
    action.values = Some(vec![0.0, 0.0]);
    let mut run = |game: &mut AirHockey, ticks: u64| {
        for _ in 0..ticks {
            for player in 0..2u8 {
                game.observe(game.live(), player, &mut obs[usize::from(player)]);
                let seen = obs[usize::from(player)].state.as_ref().unwrap();
                let (x, y) = (seen[0].clamp(-1.0, 1.0), seen[1].clamp(-0.5, 0.5));
                action.tick = game.live().tick();
                action.player = player;
                let values = action.values.as_mut().unwrap();
                values[0] = x;
                values[1] = y;
                let _ = game.apply(&action);
            }
            events.clear();
            game.advance(&mut events);
        }
    };
    run(&mut game, 1_000);
    let before = allocations();
    run(&mut game, 10_000);
    let after = allocations();
    assert_eq!(
        after - before,
        0,
        "{} allocations in 10k adapter ticks",
        after - before
    );
}
