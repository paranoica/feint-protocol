mod line_game;
mod pick_game;

use feint_ai::{Bot, BotError, Controller, Difficulty};
use feint_protocol::{ActionT, GameAdapter, ObservationT};
use line_game::{Line, LinePlaybook, line_spec};
use pick_game::{LEGAL, Pick, PickPlaybook, PickState, pick_spec};

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
        game.state.ball = 0.1;
        drive(&mut game, &mut bot, 144);
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
    let game = Line::new(0.0, 0.0);
    let mut bot = line_bot(difficulty(0, false, 0.0), LinePlaybook::new(&[0.5], 240), 1);
    bot.on_tick(&game, &observe(&game, 0))
        .expect("acts at once");
    assert_eq!(feint_ai::OPPONENT_REACTION_MS, 250);
    assert_eq!(bot.playbook().replies.get(), 4);
}

#[test]
fn commitment_holds_for_half_an_attempt() {
    let mut game = Line::new(0.0, 0.0);
    let mut bot = line_bot(
        difficulty(0, false, 0.0),
        LinePlaybook::new(&[-0.2, 0.8], 240),
        1,
    );
    drive(&mut game, &mut bot, 1);
    assert_eq!(bot.current().map(|c| c.id), Some(0));
    game.state.ball = 0.8;
    drive(&mut game, &mut bot, 100);
    assert_eq!(
        bot.current().map(|c| c.id),
        Some(0),
        "dropped its option within half an attempt"
    );
    drive(&mut game, &mut bot, 48);
    assert_eq!(bot.current().map(|c| c.id), Some(1));
}

#[test]
fn each_new_attempt_draws_new_aim_noise() {
    let mut game = Line::new(0.0, 0.0);
    let mut bot = line_bot(difficulty(0, false, 0.08), LinePlaybook::new(&[0.0], 60), 4);
    let actions = drive(&mut game, &mut bot, 600);
    let aims: Vec<f32> = actions
        .iter()
        .map(|a| a.values.as_ref().unwrap()[0])
        .collect();
    assert!(
        aims[..12].windows(2).all(|w| w[0] == w[1]),
        "aim changed within an attempt"
    );
    let distinct = {
        let mut v = aims.clone();
        v.sort_by(f32::total_cmp);
        v.dedup();
        v.len()
    };
    assert!(distinct >= 5, "only {distinct} aims over 10 attempts");
}

#[test]
fn a_tick_gap_rolls_forward_at_most_the_delay() {
    let game = Line::new(0.0, 0.0);
    let mut bot = line_bot(difficulty(300, true, 0.0), LinePlaybook::new(&[0.0], 1), 1);
    let mut obs = observe(&game, 0);
    for t in 0..72 {
        obs.tick = t;
        bot.on_tick(&game, &obs);
    }
    game.simulated.set(0);
    obs.tick = 1_000_000;
    bot.on_tick(&game, &obs).expect("acts on a decision tick");
    let simulated = game.simulated.get();
    assert!(
        simulated <= 72 + u64::from(feint_ai::SIMULATION_BUDGET),
        "simulated {simulated} ticks"
    );
}

#[test]
fn absurd_ticks_neither_panic_nor_freeze_the_bot() {
    let mut game = Line::new(0.0, 0.0);
    let mut bot = line_bot(
        difficulty(0, false, 0.0),
        LinePlaybook::new(&[-0.5, 0.5], 60),
        1,
    );
    drive(&mut game, &mut bot, 100);
    let mut forged = observe(&game, 0);
    forged.tick = u64::MAX - 3;
    let _ = bot.on_tick(&game, &forged);
    game.state.ball = -0.8;
    let actions = drive(&mut game, &mut bot, 200);
    assert!(
        actions.len() >= 45,
        "the bot froze after a forged tick: {} actions",
        actions.len()
    );
    assert_eq!(
        bot.current().map(|c| c.id),
        Some(0),
        "it never replanned towards the ball"
    );
    assert!(
        bot.memory_len() <= 30,
        "history kept growing: {}",
        bot.memory_len()
    );
}

#[test]
fn bot_new_refuses_a_reaction_longer_than_it_can_buffer() {
    let mut d = difficulty(0, false, 0.0);
    d.reaction_ms = u32::MAX;
    let made = Bot::<Line, _>::new(d, LinePlaybook::new(&[0.0], 1), &line_spec(60));
    assert!(matches!(made, Err(BotError::BadReaction)));
}

#[test]
fn after_time_goes_back_the_old_timeline_is_never_seen() {
    let game = Line::new(0.0, 0.0);
    let mut bot = line_bot(
        difficulty(17, false, 0.0),
        LinePlaybook::new(&[-0.5, 0.5], 1),
        1,
    );
    let seen = |tick: u64, ball: f64| {
        let mut g = Line::new(ball, 0.0);
        g.state.tick = tick;
        observe(&g, 0)
    };
    for t in 101..=105 {
        bot.on_tick(&game, &seen(t, 0.9));
    }
    for t in [100, 108] {
        bot.on_tick(&game, &seen(t, -0.9));
    }
    assert_eq!(
        bot.current().map(|c| c.id),
        Some(0),
        "planned from the abandoned timeline"
    );
}
