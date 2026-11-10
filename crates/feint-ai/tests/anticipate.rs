mod line_game;

use feint_ai::{Anticipation, clamp_to_space};
use feint_protocol::{ActionT, GameSpecT};
use line_game::{Line, LinePlaybook, LineState, TICK_HZ, line_spec};

const DT: f64 = 1.0 / TICK_HZ as f64;

fn state(tick: u64) -> LineState {
    LineState {
        tick,
        ball: 0.0,
        ball_v: 0.5,
        pos: [0.0, 0.0],
        target: [0.0, 0.0],
        speed: [1.0, 1.0],
    }
}

fn own(tick: u64, target: f32) -> ActionT {
    let mut a = ActionT::default();
    a.tick = tick;
    a.player = 0;
    a.values = Some(vec![target]);
    a
}

fn view(memory: &Anticipation, from: u64, to: u64) -> LineState {
    let playbook = LinePlaybook::new(&[0.0], 1);
    memory.roll_forward(&Line::new(0.0, 0.0), &playbook, 1, state(from), from, to)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-12
}

#[test]
fn with_no_own_action_the_view_is_the_delayed_state_played_forward() {
    let seen = view(&Anticipation::new(24), 100, 110);
    assert_eq!(seen.tick, 110);
    assert!(close(seen.ball, 10.0 * 0.5 * DT), "ball {}", seen.ball);
    assert_eq!(seen.pos, [0.0, 0.0]);
}

#[test]
fn an_own_action_inside_the_window_moves_the_view_from_its_tick() {
    let mut memory = Anticipation::new(24);
    memory.remember(&own(102, 0.5), 102);
    let seen = view(&memory, 100, 110);
    assert!(close(seen.pos[0], 8.0 * DT), "pos {}", seen.pos[0]);
    assert_eq!(seen.pos[1], 0.0);
}

#[test]
fn the_last_action_before_the_delayed_tick_holds_from_the_start() {
    let mut memory = Anticipation::new(24);
    memory.remember(&own(90, -0.5), 90);
    let seen = view(&memory, 100, 110);
    assert!(close(seen.pos[0], -10.0 * DT), "pos {}", seen.pos[0]);
}

#[test]
fn the_view_never_reaches_past_the_window() {
    let mut memory = Anticipation::new(24);
    memory.remember(&own(95, 0.5), 95);
    let seen = view(&memory, 100, 200);
    assert_eq!(seen.tick, 124);
    assert!(close(seen.pos[0], 24.0 * DT), "pos {}", seen.pos[0]);
}

#[test]
fn an_action_after_now_is_not_played() {
    let mut memory = Anticipation::new(24);
    memory.remember(&own(115, 0.5), 115);
    let seen = view(&memory, 100, 110);
    assert_eq!(seen.pos[0], 0.0);
}

#[test]
fn memory_keeps_the_last_action_at_the_window_edge_and_everything_after() {
    let mut memory = Anticipation::new(20);
    for tick in (0..=100).step_by(4) {
        memory.remember(&own(tick, tick as f32), tick);
    }
    assert_eq!(memory.len(), 6);
    assert_eq!(memory.last().map(|a| a.tick), Some(100));
    memory.remember(&own(100, -1.0), 100);
    assert_eq!(memory.len(), 6);
    assert_eq!(
        memory.last().and_then(|a| a.values.clone()),
        Some(vec![-1.0])
    );
    memory.clear();
    assert!(memory.is_empty());
}

#[test]
fn values_are_clamped_to_the_action_space_and_kept_inside_it() {
    let spec = line_spec(60);
    for (value, expected) in [(1.5, 1.0), (-3.0, -1.0), (0.25, 0.25), (1.0, 1.0)] {
        let mut values = [value];
        clamp_to_space(&mut values, &spec);
        assert_eq!(values, [expected], "{value}");
    }
    let mut values = [7.0];
    clamp_to_space(&mut values, &GameSpecT::default());
    assert_eq!(values, [7.0]);
}
