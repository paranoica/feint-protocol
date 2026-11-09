mod line_game;
mod penalty_game;

use feint_ai::{
    Bot, Controller, Difficulty, Memory, ProfileError, RunHeader, RunReport, run_match,
};
use feint_protocol::{GameAdapter, ObservationT};
use line_game::{Line, LinePlaybook, line_spec};
use penalty_game::{
    HabitStriker, KEEPER, Keeper, KeeperRule, Penalty, PenaltyPlaybook, STRIKER, penalty_spec,
};

fn thinker(depth: u8, deception: f64, memory: Memory) -> Difficulty {
    let mut d = Difficulty::preset(5).unwrap();
    d.depth = depth;
    d.deception = deception;
    d.memory = memory;
    d.adaptation = 0.3;
    d.spread = 0.0;
    d.long_game = feint_ai::LongGame::None;
    d
}

fn header(seed: u64) -> RunHeader {
    RunHeader {
        game_id: "penalty".into(),
        game_version: 1,
        seed,
        bodies: vec![],
        levels: [0, 0],
    }
}

fn penalty_bot(d: Difficulty) -> Bot<Penalty, PenaltyPlaybook> {
    Bot::new(d, PenaltyPlaybook::default(), &penalty_spec()).unwrap()
}

fn shootout(d: Difficulty, rule: KeeperRule, rounds: u32, seed: u64) -> RunReport {
    let mut game = Penalty::new(rounds);
    let mut striker = penalty_bot(d);
    let mut keeper = Keeper::new(rule);
    run_match(
        &mut game,
        [&mut striker, &mut keeper],
        header(seed),
        u64::from(rounds) * 4,
    )
    .unwrap()
}

fn deceptive_kicks(report: &RunReport) -> Vec<i32> {
    report
        .actions
        .iter()
        .filter(|a| a.player == STRIKER && a.deceptive)
        .map(|a| a.discrete)
        .collect()
}

#[test]
fn depth_two_feints_against_a_biter() {
    let honest = shootout(thinker(0, 0.0, Memory::Match), KeeperRule::Biter, 50, 1);
    assert_eq!(
        honest.score[0], 0,
        "an honest kick into a biter never scores"
    );
    let feinting = shootout(thinker(2, 1.0, Memory::Match), KeeperRule::Biter, 50, 1);
    assert!(
        feinting.score[0] >= 45,
        "depth 2 scored {} of 50",
        feinting.score[0]
    );
}

#[test]
fn depth_three_beats_an_adaptive_keeper() {
    let two = shootout(thinker(2, 1.0, Memory::Match), KeeperRule::Adaptive, 60, 2);
    let three = shootout(thinker(3, 1.0, Memory::Match), KeeperRule::Adaptive, 60, 2);
    assert!(
        three.score[0] >= 42,
        "depth 3 scored {} of 60",
        three.score[0]
    );
    assert!(
        three.score[0] > two.score[0] + 5,
        "depth 3 ({}) is not clearly ahead of depth 2 ({})",
        three.score[0],
        two.score[0]
    );
}

#[test]
fn depth_below_two_feints_rarely_and_randomly() {
    let report = shootout(thinker(1, 0.1, Memory::Match), KeeperRule::Biter, 100, 3);
    let fakes = deceptive_kicks(&report);
    assert!(
        (5..=15).contains(&fakes.len()),
        "{} deceptive kicks of 100",
        fakes.len()
    );
    assert!(
        fakes.contains(&1) && fakes.contains(&2),
        "always the same fake: {fakes:?}"
    );
    let none = shootout(thinker(1, 0.0, Memory::Match), KeeperRule::Biter, 100, 3);
    assert!(deceptive_kicks(&none).is_empty());
}

#[test]
fn depth_one_defends_the_opponents_habit() {
    let keep = |depth: u8| {
        let mut game = Penalty::new(50);
        let mut striker = HabitStriker(1);
        let mut keeper = penalty_bot(thinker(depth, 0.0, Memory::Match));
        run_match(&mut game, [&mut striker, &mut keeper], header(4), 200).unwrap()
    };
    assert_eq!(
        keep(0).score[0],
        50,
        "a naive keeper expects the default kick"
    );
    let reading = keep(1);
    assert!(
        reading.score[0] <= 5,
        "the habit got through {} times",
        reading.score[0]
    );
    assert_eq!(KEEPER, 1);
}

#[test]
fn the_model_counts_each_kick_once() {
    let mut game = Penalty::new(10);
    let mut striker = penalty_bot(thinker(2, 1.0, Memory::Match));
    let mut keeper = Keeper::new(KeeperRule::Biter);
    let report = run_match(&mut game, [&mut striker, &mut keeper], header(5), 40).unwrap();
    let profile = striker.profile("penalty").expect("level with memory");
    assert_eq!(profile.bite_events.iter().sum::<u32>(), 10);
    assert_eq!(profile.mine_events.iter().sum::<u32>(), 10);
    let fakes = deceptive_kicks(&report).len() as f64;
    assert!(profile.fake_tries > 0.0 && profile.fake_tries <= fakes);
}

#[test]
fn memory_none_match_and_profile_behave_as_specified() {
    let trained = |memory: Memory| {
        let mut game = Penalty::new(12);
        let mut striker = penalty_bot(thinker(2, 1.0, memory));
        let mut keeper = Keeper::new(KeeperRule::Biter);
        run_match(&mut game, [&mut striker, &mut keeper], header(6), 48).unwrap();
        striker
    };
    let none = trained(Memory::None);
    assert!(none.profile("penalty").is_none());

    let mut per_match = trained(Memory::Match);
    assert!(
        per_match
            .profile("penalty")
            .unwrap()
            .bite_events
            .iter()
            .sum::<u32>()
            > 0
    );
    per_match.new_match(&penalty_spec(), STRIKER, 7);
    assert_eq!(
        per_match
            .profile("penalty")
            .unwrap()
            .bite_events
            .iter()
            .sum::<u32>(),
        0
    );

    let mut lasting = trained(Memory::MatchAndProfile);
    let before = lasting.profile("penalty").unwrap();
    lasting.new_match(&penalty_spec(), STRIKER, 7);
    assert_eq!(lasting.profile("penalty").unwrap(), before);

    let mut fresh = penalty_bot(thinker(2, 1.0, Memory::MatchAndProfile));
    fresh.load_profile(&before, "penalty").unwrap();
    assert_eq!(fresh.profile("penalty").unwrap(), before);
    let mut forgetful = penalty_bot(thinker(2, 1.0, Memory::None));
    assert_eq!(
        forgetful.load_profile(&before, "penalty"),
        Err(ProfileError::NoMemory)
    );
}

fn line_bot(d: Difficulty, playbook: LinePlaybook, seed: u64) -> Bot<Line, LinePlaybook> {
    let mut bot = Bot::new(d, playbook, &line_spec(60)).unwrap();
    bot.new_match(&line_spec(60), 0, seed);
    bot
}

fn observe(game: &Line) -> ObservationT {
    let mut obs = ObservationT::default();
    game.observe(&game.state, 0, &mut obs);
    obs
}

fn drive(game: &mut Line, bot: &mut Bot<Line, LinePlaybook>, ticks: u64) {
    let mut events = Vec::new();
    for _ in 0..ticks {
        let obs = observe(game);
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).unwrap();
        }
        game.advance(&mut events);
    }
}

fn realtime(depth: u8, deception: f64) -> Difficulty {
    let mut d = thinker(depth, deception, Memory::Match);
    d.reaction_ms = 0;
    d.anticipate = false;
    d
}

#[test]
fn a_fake_committed_with_the_gate_shut_is_held_for_ten_replans() {
    let mut playbook = LinePlaybook::new(&[0.0], 240);
    playbook.fakes = vec![0.5];
    playbook.flat = true;
    let mut game = Line::new(0.5, 0.0);
    let mut bot = line_bot(realtime(1, 1.0), playbook, 3);
    drive(&mut game, &mut bot, 1);
    let fake = bot
        .current()
        .filter(|c| c.deceptive)
        .expect("depth 1 commits its one fake")
        .id;
    for replan in 0..12 {
        drive(&mut game, &mut bot, 24);
        assert_eq!(
            bot.current().map(|c| c.id),
            Some(fake),
            "dropped at replan {replan}"
        );
    }
}

#[test]
fn the_gate_is_rolled_once_per_possession() {
    let mut outcomes = Vec::new();
    for seed in 0..40 {
        let mut playbook = LinePlaybook::new(&[0.0], 24);
        playbook.fakes = vec![0.5];
        let mut game = Line::new(0.5, 0.0);
        let mut bot = line_bot(realtime(2, 0.5), playbook, seed);
        let mut seen = Vec::new();
        for _ in 0..12 {
            drive(&mut game, &mut bot, 24);
            seen.push(bot.current().is_some_and(|c| c.deceptive));
        }
        assert!(
            seen.windows(2).all(|w| w[0] == w[1]),
            "seed {seed}: {seen:?}"
        );
        outcomes.push(seen[0]);
    }
    assert!(
        outcomes.contains(&true) && outcomes.contains(&false),
        "{outcomes:?}"
    );
}

#[test]
fn near_ties_are_random_from_depth_one() {
    let picks = |depth: u8| {
        (0..20)
            .map(|seed| {
                let mut game = Line::new(0.5, 0.0);
                let mut bot = line_bot(
                    realtime(depth, 0.0),
                    LinePlaybook::new(&[0.5, 0.5], 60),
                    seed,
                );
                drive(&mut game, &mut bot, 1);
                bot.current().unwrap().id
            })
            .collect::<Vec<_>>()
    };
    assert!(picks(0).iter().all(|&id| id == 0));
    let random = picks(1);
    assert!(random.contains(&0) && random.contains(&1), "{random:?}");
}

#[test]
fn pruning_keeps_the_simulation_budget() {
    let targets: Vec<f32> = (0..12).map(|i| -0.9 + 0.15 * i as f32).collect();
    let game = Line::new(0.3, 0.0);
    let mut bot = line_bot(realtime(1, 0.0), LinePlaybook::new(&targets, 2_000), 1);
    game.simulated.set(0);
    bot.on_tick(&game, &observe(&game)).expect("acts at once");
    let simulated = game.simulated.get();
    assert!(
        simulated <= u64::from(feint_ai::SIMULATION_BUDGET),
        "{simulated} simulated ticks in one replan"
    );
    assert_eq!(
        bot.current().map(|c| c.id),
        Some(8),
        "the target nearest the ball should win"
    );
}

#[test]
fn the_cue_carries_the_opponents_twitch_until_the_possession_changes() {
    let mut playbook = LinePlaybook::new(&[0.0], 60);
    playbook.reads = true;
    let mut game = Line::new(0.5, 0.0);
    let mut bot = line_bot(realtime(1, 0.0), playbook, 1);
    let last = |bot: &Bot<Line, LinePlaybook>| *bot.playbook().twitches.borrow().last().unwrap();
    drive(&mut game, &mut bot, 1);
    assert_eq!(last(&bot), None);
    game.state.pos[1] = -0.5;
    game.state.target[1] = -0.5;
    drive(&mut game, &mut bot, 30);
    assert_eq!(last(&bot), Some(0), "the opponent moved to class 0");
    game.state.ball = -0.1;
    drive(&mut game, &mut bot, 30);
    game.state.ball = 0.1;
    drive(&mut game, &mut bot, 30);
    assert_eq!(last(&bot), None, "a new possession forgets the twitch");
}

#[test]
fn a_frozen_tick_does_not_grow_the_memory() {
    let game = Line::new(0.5, 0.0);
    let mut bot = line_bot(realtime(1, 0.0), LinePlaybook::new(&[0.0], 60), 1);
    let obs = observe(&game);
    for _ in 0..100 {
        bot.on_tick(&game, &obs);
    }
    assert!(
        bot.memory_len() <= 1,
        "{} actions kept for one tick",
        bot.memory_len()
    );
}

#[test]
fn a_thinking_bot_whose_gate_opens_prefers_a_fake_to_a_slightly_better_honest_option() {
    for depth in [2u8, 3] {
        let mut playbook = LinePlaybook::new(&[0.5], 240);
        playbook.fakes = vec![0.45, 0.4];
        playbook.margin = 0.1;
        let mut game = Line::new(0.5, 0.0);
        let mut bot = line_bot(realtime(depth, 1.0), playbook, 5);
        drive(&mut game, &mut bot, 1);
        let current = bot.current().expect("acts at once");
        assert!(current.deceptive, "depth {depth}: {current:?}");
        let mut shut = LinePlaybook::new(&[0.5], 240);
        shut.fakes = vec![0.45, 0.4];
        shut.margin = 0.1;
        let mut game = Line::new(0.5, 0.0);
        let mut bot = line_bot(realtime(depth, 0.0), shut, 5);
        drive(&mut game, &mut bot, 1);
        assert!(
            !bot.current().expect("acts at once").deceptive,
            "depth {depth}: a shut gate must not force a fake"
        );
    }
}

#[test]
fn the_style_bonus_does_not_push_a_fake_the_opponent_is_known_to_read() {
    let lean = |profile: Option<&feint_ai::Profile>| {
        let mut playbook = LinePlaybook::new(&[0.5], 240);
        playbook.fakes = vec![0.45];
        playbook.margin = 0.1;
        let mut d = realtime(3, 1.0);
        d.memory = Memory::MatchAndProfile;
        let mut bot = line_bot(d, playbook, 5);
        if let Some(p) = profile {
            bot.load_profile(p, "line").unwrap();
        }
        let mut game = Line::new(0.5, 0.0);
        drive(&mut game, &mut bot, 1);
        bot.current().expect("acts at once").deceptive
    };
    assert!(
        lean(None),
        "with no read of the opponent the style picks the fake"
    );
    let mut probe = line_bot(
        {
            let mut d = realtime(3, 1.0);
            d.memory = Memory::MatchAndProfile;
            d
        },
        LinePlaybook::new(&[0.5], 240),
        5,
    );
    let mut game = Line::new(0.5, 0.0);
    drive(&mut game, &mut probe, 1);
    let mut read = probe.profile("line").expect("a model");
    let k = usize::from(read.classes);
    read.bite[1] = 50.0;
    read.bite_events[0] = 50;
    assert_eq!(read.bite.len(), k * k);
    assert!(
        !lean(Some(&read)),
        "the opponent covers the real target when shown 0: no style bonus for that fake"
    );
}
