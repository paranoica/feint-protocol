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
    drive(&mut game, &mut b, 40);
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
    drive_scored(&mut game, &mut b, 40, 0);
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
