mod line_game;
mod pick_game;

use feint_ai::{Bot, BotError, Controller, Difficulty};
use feint_protocol::{ActionT, GameAdapter, ObservationT};
use line_game::{Line, LinePlaybook, line_spec};
use pick_game::{LEGAL, Pick, PickPlaybook, PickState, pick_spec};

/// Level 2 with the reaction, anticipation and spread a test needs.
fn difficulty(reaction_ms: u32, anticipate: bool, spread: f64) -> Difficulty {
    let mut d = Difficulty::preset(2).unwrap();
    d.reaction_ms = reaction_ms;
    d.anticipate = anticipate;
    d.spread = spread;
    d
}

fn line_bot(d: Difficulty, playbook: LinePlaybook, seed: u64) -> Bot<Line, LinePlaybook> {
    let mut bot = Bot::new(d, playbook, &line_spec(60)).expect("a valid spec");
    bot.new_match(&line_spec(60), 0, seed);
    bot
}

fn observe(game: &Line, player: u8) -> ObservationT {
    let mut obs = ObservationT::default();
    game.observe(&game.state, player, &mut obs);
    obs
}

/// Run the bot as player 0 for `ticks`, applying its actions; never calls `live()`.
fn drive(game: &mut Line, bot: &mut Bot<Line, LinePlaybook>, ticks: u64) -> Vec<ActionT> {
    let mut actions = Vec::new();
    let mut events = Vec::new();
    for _ in 0..ticks {
        let obs = observe(game, 0);
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).expect("the bot's action is accepted");
            actions.push(action);
        }
        game.advance(&mut events);
    }
    actions
}

#[test]
fn bot_new_refuses_a_realtime_cadence_it_cannot_keep() {
    let d = difficulty(0, false, 0.0);
    for hz in [0, 241] {
        let made = Bot::<Line, _>::new(d, LinePlaybook::new(&[0.0], 1), &line_spec(hz));
        assert!(
            matches!(made, Err(BotError::BadCadence)),
            "decision_hz {hz}"
        );
    }
    assert!(Bot::<Line, _>::new(d, LinePlaybook::new(&[0.0], 1), &line_spec(60)).is_ok());
    assert!(
        Bot::<Pick, _>::new(d, PickPlaybook, &pick_spec()).is_ok(),
        "turn-based has no rate"
    );
}

#[test]
fn best_option_wins() {
    // Ball still at 0.8; one second ahead the targets end 1.8, 0.8 and 0.2 from it.
    let mut game = Line::new(0.8, 0.0);
    let mut bot = line_bot(
        difficulty(0, false, 0.0),
        LinePlaybook::new(&[-1.0, 0.0, 1.0], 240),
        1,
    );
    let actions = drive(&mut game, &mut bot, 1);
    assert_eq!(bot.current().map(|c| c.id), Some(2));
    assert_eq!(actions[0].values, Some(vec![1.0]));
}

#[test]
fn ties_go_to_the_lower_id() {
    let mut game = Line::new(0.5, 0.0);
    let mut bot = line_bot(
        difficulty(0, false, 0.0),
        LinePlaybook::new(&[0.5, 0.5], 240),
        1,
    );
    drive(&mut game, &mut bot, 1);
    assert_eq!(bot.current().map(|c| c.id), Some(0));
}

#[test]
fn margin_keeps_the_current_option_until_another_is_clearly_better() {
    for (margin, expected) in [(0.5, 0), (0.0, 1)] {
        let mut game = Line::new(0.0, 0.0);
        let mut playbook = LinePlaybook::new(&[-0.2, 0.25], 240);
        playbook.margin = margin;
        let mut bot = line_bot(difficulty(0, false, 0.0), playbook, 1);
        drive(&mut game, &mut bot, 1);
        assert_eq!(
            bot.current().map(|c| c.id),
            Some(0),
            "0.2 from the ball beats 0.25"
        );
        // Now option 1 ends 0.15 from the ball and option 0 ends 0.3: better by 0.15.
        game.state.ball = 0.1;
        drive(&mut game, &mut bot, 48);
        assert_eq!(
            bot.current().map(|c| c.id),
            Some(expected),
            "margin {margin}"
        );
    }
}

#[test]
fn no_options_means_no_action() {
    let mut game = Line::new(0.0, 0.0);
    let mut bot = line_bot(difficulty(0, false, 0.0), LinePlaybook::new(&[], 240), 1);
    assert!(drive(&mut game, &mut bot, 60).is_empty());
    assert_eq!(bot.current(), None);
}

#[test]
fn horizon_zero_is_clamped_to_one_tick() {
    let mut game = Line::new(0.8, 0.0);
    let mut bot = line_bot(
        difficulty(0, false, 0.0),
        LinePlaybook::new(&[-1.0, 1.0], 0),
        1,
    );
    drive(&mut game, &mut bot, 1);
    assert_eq!(bot.current().map(|c| c.id), Some(1));
}

#[test]
fn nothing_before_the_reaction_delay_has_passed() {
    let mut game = Line::new(0.8, 0.0);
    let mut bot = line_bot(
        difficulty(400, false, 0.0),
        LinePlaybook::new(&[1.0], 240),
        1,
    );
    assert!(
        drive(&mut game, &mut bot, 96).is_empty(),
        "acted before 96 ticks"
    );
    let actions = drive(&mut game, &mut bot, 1);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].tick, 96);
}

#[test]
fn anticipation_plans_from_now_not_from_the_delayed_state() {
    // 400 ms = 96 ticks: the ball, at 0.5/s from -0.15, is seen left of the player but is
    // now at +0.05, to its right. One-tick horizons: only the starting state decides.
    for (anticipate, expected) in [(false, 0), (true, 1)] {
        let mut game = Line::new(-0.15, 0.5);
        let mut bot = line_bot(
            difficulty(400, anticipate, 0.0),
            LinePlaybook::new(&[-0.3, 0.3], 1),
            1,
        );
        drive(&mut game, &mut bot, 97);
        assert_eq!(
            bot.current().map(|c| c.id),
            Some(expected),
            "anticipate {anticipate}"
        );
    }
}

#[test]
fn newest_observations_only_lend_their_tick() {
    for anticipate in [false, true] {
        let truth: Vec<ObservationT> = {
            let mut game = Line::new(-0.15, 0.5);
            let mut events = Vec::new();
            (0..=96)
                .map(|_| {
                    let obs = observe(&game, 0);
                    game.advance(&mut events);
                    obs
                })
                .collect()
        };
        let first_action = |stream: &[ObservationT]| {
            let game = Line::new(0.0, 0.0);
            let mut bot = line_bot(
                difficulty(400, anticipate, 0.0),
                LinePlaybook::new(&[-0.3, 0.3], 1),
                1,
            );
            stream.iter().find_map(|obs| bot.on_tick(&game, obs))
        };
        let mut scrambled = truth.clone();
        for obs in &mut scrambled[1..] {
            obs.state = Some(vec![99.0, -7.0, 0.5, -0.5, 0.9]);
        }
        let honest = first_action(&truth).expect("acts at tick 96");
        assert_eq!(
            first_action(&scrambled),
            Some(honest.clone()),
            "anticipate {anticipate}"
        );
        assert_eq!((honest.tick, honest.player), (96, 0));
    }
}

#[test]
fn aim_noise_stays_within_spread_and_zero_spread_is_exact() {
    let first_value = |spread: f64, seed: u64| {
        let game = Line::new(0.0, 0.0);
        let mut bot = line_bot(
            difficulty(0, false, spread),
            LinePlaybook::new(&[0.0], 1),
            seed,
        );
        let action = bot
            .on_tick(&game, &observe(&game, 0))
            .expect("acts at once");
        f64::from(action.values.unwrap()[0])
    };
    let noisy: Vec<f64> = (0..300).map(|seed| first_value(0.08, seed)).collect();
    assert!(noisy.iter().all(|v| v.abs() <= 0.08 + 1e-7), "{noisy:?}");
    assert!(noisy.iter().any(|&v| v > 0.04) && noisy.iter().any(|&v| v < -0.04));
    assert!((0..20).all(|seed| first_value(0.0, seed) == 0.0));
}

#[test]
fn players_and_seeds_get_their_own_noise() {
    let first_value = |me: u8, seed: u64| {
        let game = Line::new(0.0, 0.0);
        let mut bot = Bot::new(
            difficulty(0, false, 0.08),
            LinePlaybook::new(&[0.0], 1),
            &line_spec(60),
        )
        .unwrap();
        bot.new_match(&line_spec(60), me, seed);
        bot.on_tick(&game, &observe(&game, me))
            .unwrap()
            .values
            .unwrap()[0]
    };
    assert_ne!(first_value(0, 5), first_value(1, 5));
    assert_ne!(first_value(0, 5), first_value(0, 6));
    assert_eq!(first_value(0, 5), first_value(0, 5));
}

#[test]
fn same_seed_same_actions() {
    let run = || {
        let mut game = Line::new(-0.4, 0.3);
        let mut bot = line_bot(
            difficulty(150, true, 0.05),
            LinePlaybook::new(&[-0.5, 0.0, 0.5], 60),
            9,
        );
        drive(&mut game, &mut bot, 600)
    };
    let (a, b) = (run(), run());
    assert!(a.len() > 100);
    assert_eq!(a, b);
}

#[test]
fn bot_never_reads_live_state() {
    let mut game = Line::new(-0.4, 0.3);
    game.forbid_live = true;
    let mut bot = line_bot(
        difficulty(220, true, 0.03),
        LinePlaybook::new(&[-0.5, 0.0, 0.5], 60),
        3,
    );
    assert!(drive(&mut game, &mut bot, 600).len() > 100);
}

#[test]
fn turn_based_bot_acts_only_on_its_turn_and_picks_legal_actions() {
    let game = Pick {
        state: PickState {
            tick: 0,
            picks: vec![],
        },
    };
    let obs_for = |state: &PickState| {
        let mut obs = ObservationT::default();
        game.observe(state, 0, &mut obs);
        obs
    };
    let mut bot = Bot::new(difficulty(400, true, 0.0), PickPlaybook, &pick_spec()).unwrap();
    bot.new_match(&pick_spec(), 0, 1);
    let theirs = PickState {
        tick: 1,
        picks: vec![2],
    };
    assert_eq!(
        bot.on_tick(&game, &obs_for(&theirs)),
        None,
        "acted out of turn"
    );
    let mine = PickState {
        tick: 2,
        picks: vec![2, 0],
    };
    let action = bot
        .on_tick(&game, &obs_for(&mine))
        .expect("acts on its turn, no delay");
    assert_eq!((action.discrete, action.tick, action.player), (2, 2, 0));

    let mut seen = [0; 3];
    for seed in 0..100 {
        let mut random = Bot::new(difficulty(0, false, 1.0), PickPlaybook, &pick_spec()).unwrap();
        random.new_match(&pick_spec(), 0, seed);
        let a = random.on_tick(&game, &obs_for(&mine)).unwrap();
        seen[usize::try_from(a.discrete).unwrap()] += 1;
    }
    assert_eq!(seen[1], 0, "picked an illegal action");
    assert!(
        seen[0] > 0 && seen[2] > 0,
        "spread 1 should pick at random: {seen:?}"
    );
    assert_eq!(LEGAL[1], 0);
}

#[test]
fn rollouts_ask_the_opponent_once_per_its_reaction_time() {
    // One option, a 240-tick horizon at 240 Hz: an opponent reacting in 250 ms (60 ticks)
    // is asked at ticks 0, 60, 120 and 180 — four replies, not one per 4-tick stride.
    let game = Line::new(0.0, 0.0);
    let mut bot = line_bot(difficulty(0, false, 0.0), LinePlaybook::new(&[0.5], 240), 1);
    bot.on_tick(&game, &observe(&game, 0))
        .expect("acts at once");
    assert_eq!(feint_ai::OPPONENT_REACTION_MS, 250);
    assert_eq!(bot.playbook().replies.get(), 4);
}
