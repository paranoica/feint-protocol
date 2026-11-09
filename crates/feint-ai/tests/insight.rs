mod line_game;

use feint_ai::{Bot, Controller, Difficulty, LongGame, Memory};
use feint_protocol::{GameAdapter, ObservationT};
use line_game::{Line, LinePlaybook, line_spec};

fn difficulty(depth: u8, deception: f64) -> Difficulty {
    let mut d = Difficulty::preset(5).unwrap();
    d.depth = depth;
    d.deception = deception;
    d.memory = Memory::Match;
    d.adaptation = 0.0;
    d.spread = 0.0;
    d.long_game = LongGame::None;
    d.reaction_ms = 0;
    d.anticipate = false;
    d
}

fn bot(d: Difficulty, playbook: LinePlaybook) -> Bot<Line, LinePlaybook> {
    let mut bot = Bot::new(d, playbook, &line_spec(60)).unwrap();
    bot.new_match(&line_spec(60), 0, 1);
    bot
}

fn drive(game: &mut Line, bot: &mut Bot<Line, LinePlaybook>, ticks: u64) {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let mut obs = ObservationT::default();
        game.observe(&game.state, 0, &mut obs);
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).unwrap();
        }
        game.advance(&mut events);
    }
}

fn reading() -> LinePlaybook {
    let mut p = LinePlaybook::new(&[0.0], 600);
    p.reads = true;
    p
}

#[test]
fn a_slow_shift_is_read_against_the_state_at_commit() {
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 0.0), reading());
    drive(&mut game, &mut b, 1);
    for _ in 0..100 {
        game.state.pos[1] -= 0.002;
        game.state.target[1] = game.state.pos[1];
        drive(&mut game, &mut b, 1);
    }
    let last = *b.playbook().twitches.borrow().last().unwrap();
    assert_eq!(last, Some(0), "0.2 moved at 0.002 per tick was never read");
}

#[test]
fn a_bait_without_a_real_class_does_not_win_by_a_cover() {
    let mut p = reading();
    p.fakes = vec![0.0];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive(&mut game, &mut b, 1);
    assert!(
        b.current().is_some_and(|c| c.deceptive),
        "the bait was not committed"
    );
    game.state.pos[1] = 0.5;
    game.state.target[1] = 0.5;
    drive(&mut game, &mut b, 20);
    game.state.ball = -0.5;
    drive(&mut game, &mut b, 300);
    let profile = b.profile("line").unwrap();
    assert_eq!(profile.fake_tries, 1.0);
    assert_eq!(
        profile.fake_wins, 0.0,
        "covering a side is no win for a bait"
    );
}

#[test]
fn a_ball_hovering_on_the_line_is_one_possession() {
    let mut p = reading();
    p.fakes = vec![0.0];
    let mut game = Line::new(0.01, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    for i in 0..200 {
        game.state.ball = if (i / 4) % 2 == 0 { 0.01 } else { -0.01 };
        drive(&mut game, &mut b, 1);
    }
    game.state.ball = -0.5;
    drive(&mut game, &mut b, 40);
    let tries = b.profile("line").unwrap().fake_tries;
    assert!(
        tries <= 1.0,
        "{tries} fake attempts in one hovering possession"
    );
}

fn drive_scored(game: &mut Line, bot: &mut Bot<Line, LinePlaybook>, ticks: u64, them: i64) {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let mut obs = ObservationT::default();
        game.observe(&game.state, 0, &mut obs);
        obs.score = Some(vec![0, them]);
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).unwrap();
        }
        game.advance(&mut events);
    }
}

fn shown_attempt() -> (Line, Bot<Line, LinePlaybook>) {
    let mut p = reading();
    p.shown = true;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 0.0), p);
    drive_scored(&mut game, &mut b, 1, 0);
    assert!(b.current().is_some_and(|c| c.shown.is_some()));
    (game, b)
}

#[test]
fn an_attempt_ends_when_the_possession_ends() {
    let (mut game, mut b) = shown_attempt();
    game.state.ball = -0.5;
    drive_scored(&mut game, &mut b, 4, 0);
    game.state.pos[1] = 0.5;
    game.state.target[1] = 0.5;
    drive_scored(&mut game, &mut b, 300, 0);
    let bites: u32 = b.profile("line").unwrap().bite_events.iter().sum();
    assert_eq!(
        bites, 0,
        "a cover after the possession ended was charged to the attempt"
    );
}

#[test]
fn an_attempt_ends_on_the_opponents_goal() {
    let (mut game, mut b) = shown_attempt();
    drive_scored(&mut game, &mut b, 4, 1);
    game.state.pos[1] = 0.5;
    game.state.target[1] = 0.5;
    drive_scored(&mut game, &mut b, 8, 1);
    game.state.ball = -0.5;
    drive_scored(&mut game, &mut b, 40, 1);
    let bites: u32 = b.profile("line").unwrap().bite_events.iter().sum();
    assert_eq!(
        bites, 0,
        "a cover after the opponent scored was charged to the attempt"
    );
}

fn drive_score(game: &mut Line, bot: &mut Bot<Line, LinePlaybook>, ticks: u64, score: [i64; 2]) {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let mut obs = ObservationT::default();
        game.observe(&game.state, 0, &mut obs);
        obs.score = Some(score.to_vec());
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).unwrap();
        }
        game.advance(&mut events);
    }
}

#[test]
fn a_goal_scored_after_the_puck_left_still_wins_the_attempt() {
    let mut p = reading();
    p.fakes = vec![0.0];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    assert!(b.current().is_some_and(|c| c.deceptive));
    game.state.ball = -0.5;
    drive_score(&mut game, &mut b, 20, [0, 0]);
    drive_score(&mut game, &mut b, 4, [1, 0]);
    let profile = b.profile("line").unwrap();
    assert_eq!(
        (profile.fake_tries, profile.fake_wins),
        (1.0, 1.0),
        "the bait's goal came while the puck was away"
    );
}

#[test]
fn a_twitch_is_not_read_while_the_opponent_has_the_ball() {
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 0.0), reading());
    drive(&mut game, &mut b, 1);
    game.state.ball = -0.5;
    drive(&mut game, &mut b, 4);
    game.state.pos[1] = -0.5;
    game.state.target[1] = -0.5;
    drive(&mut game, &mut b, 8);
    game.state.ball = 0.5;
    drive(&mut game, &mut b, 30);
    assert_eq!(*b.playbook().twitches.borrow().last().unwrap(), None);
}

#[test]
fn a_frozen_tick_is_one_sample() {
    let mut p = reading();
    p.fakes = vec![0.0];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    game.state.ball = -0.5;
    drive_score(&mut game, &mut b, 3, [0, 0]);
    assert_eq!(game.state.tick % 4, 0, "freeze on a decision tick");
    let mut obs = ObservationT::default();
    game.observe(&game.state, 0, &mut obs);
    obs.score = Some(vec![0, 0]);
    for _ in 0..100 {
        b.on_tick(&game, &obs);
    }
    drive_score(&mut game, &mut b, 8, [1, 0]);
    assert_eq!(
        b.profile("line").unwrap().fake_wins,
        1.0,
        "a paused host closed the attempt"
    );
}

#[test]
fn a_fired_attempt_is_not_replaced_by_the_next_commit() {
    let mut p = LinePlaybook::new(&[0.0], 24);
    p.reads = true;
    p.shoots = true;
    p.flat = true;
    p.fakes = vec![0.0];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    assert!(b.current().is_some_and(|c| c.deceptive));
    game.state.ball_v = 1e-9;
    drive_score(&mut game, &mut b, 100, [0, 0]);
    drive_score(&mut game, &mut b, 8, [1, 0]);
    let profile = b.profile("line").unwrap();
    assert_eq!(
        (profile.fake_tries, profile.fake_wins),
        (1.0, 1.0),
        "the fake had shot; later commits closed it as a loss"
    );
}

#[test]
fn a_bait_takes_the_real_class_of_the_strike_that_follows() {
    let mut p = LinePlaybook::new(&[-0.9, 0.5], 240);
    p.reads = true;
    p.shown = true;
    p.fakes = vec![-0.8];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    assert!(
        b.current().is_some_and(|c| c.deceptive),
        "the bait comes first"
    );
    drive_score(&mut game, &mut b, 200, [0, 0]);
    assert!(b.current().is_some_and(|c| !c.deceptive), "then the strike");
    drive_score(&mut game, &mut b, 8, [1, 0]);
    let profile = b.profile("line").unwrap();
    assert_eq!(
        (profile.fake_tries, profile.fake_wins),
        (1.0, 1.0),
        "bait and strike are one fake"
    );
    assert_eq!(profile.mine_events.iter().sum::<u32>(), 1);
}

#[test]
fn an_attempt_opened_before_the_possession_is_seen_is_not_recorded() {
    let mut p = LinePlaybook::new(&[-0.9], 24);
    p.reads = true;
    p.fakes = vec![-0.5];
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(2, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    assert!(b.current().is_some_and(|c| c.deceptive));
    game.state.ball = -0.5;
    drive_score(&mut game, &mut b, 120, [0, 0]);
    game.state.ball = 0.5;
    drive_score(&mut game, &mut b, 4, [0, 0]);
    let tries = b.profile("line").unwrap().fake_tries;
    assert_eq!(
        tries, 1.0,
        "fakes committed while the ball was away were recorded"
    );
}

#[test]
fn an_attempt_opened_during_a_short_trip_away_counts_once_the_ball_is_back() {
    let mut p = LinePlaybook::new(&[-0.9], 24);
    p.reads = true;
    p.fakes = vec![-0.5];
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(2, 1.0), p);
    drive_score(&mut game, &mut b, 20, [0, 0]);
    game.state.ball = -0.5;
    drive_score(&mut game, &mut b, 12, [0, 0]);
    game.state.ball = 0.5;
    drive_score(&mut game, &mut b, 8, [1, 0]);
    let profile = b.profile("line").unwrap();
    assert_eq!(
        profile.fake_wins, 1.0,
        "the attempt of the possession was dropped: {profile:?}"
    );
}

#[test]
fn a_bait_does_not_take_a_strike_at_the_class_it_showed() {
    let mut p = LinePlaybook::new(&[0.5], 240);
    p.reads = true;
    p.shown = true;
    p.fakes = vec![-0.9];
    p.fake_real = None;
    let mut game = Line::new(0.5, 0.0);
    let mut b = bot(difficulty(1, 1.0), p);
    drive_score(&mut game, &mut b, 1, [0, 0]);
    drive_score(&mut game, &mut b, 200, [0, 0]);
    assert_eq!(
        b.current().and_then(|c| c.real),
        Some(0),
        "the strike shows and plays class 0"
    );
    drive_score(&mut game, &mut b, 8, [1, 0]);
    let profile = b.profile("line").unwrap();
    assert_eq!(profile.fake_tries, 1.0);
    assert_eq!(
        profile.fake_wins, 0.0,
        "a strike where the bait pointed is no fake"
    );
}
