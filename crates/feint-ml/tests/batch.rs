use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty, RunHeader, run_match};
use feint_ml::batch::{FEATURES, OUTPUTS};
use feint_ml::{BatchError, BatchSpec, HockeyBatch, Outputs, Pilot, Policy, Seat};
use feint_protocol::{BodyParamsT, GameAdapter};

fn decide(features: &[f32], out: &mut [f32]) {
    out[0] = features[0] / 1.2 - 0.3;
    out[1] = features[1] / 0.6 + features[3] / 8.0;
}

struct Same;

impl Policy for Same {
    fn act(&mut self, features: &[f32], out: &mut [f32]) {
        decide(features, out);
    }
}

fn actions(out: &Outputs, envs: usize) -> Vec<f32> {
    let mut actions = vec![0.0; envs * 2 * OUTPUTS];
    for slot in 0..envs * 2 {
        if out.ready[slot] {
            decide(
                &out.obs[slot * FEATURES..(slot + 1) * FEATURES],
                &mut actions[slot * OUTPUTS..(slot + 1) * OUTPUTS],
            );
        }
    }
    actions
}

fn still(envs: usize, target: [f32; 2]) -> Vec<f32> {
    target.repeat(envs * 2)
}

fn mixed() -> Vec<[Seat; 2]> {
    [
        [Seat::Learner, Seat::Learner],
        [Seat::Learner, Seat::Level(3)],
        [Seat::Human, Seat::Learner],
        [Seat::Learner, Seat::Reader { anticipates: true }],
    ]
    .repeat(2)
}

fn trajectory(seed: u64, threads: usize, steps: usize) -> Vec<Outputs> {
    let mut spec = BatchSpec::new(mixed(), seed);
    spec.threads = threads;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let mut seen = vec![out.clone()];
    for _ in 0..steps {
        out = batch.step(&actions(&out, 8)).expect("steps").clone();
        seen.push(out.clone());
    }
    seen
}

#[test]
fn same_seed_same_trajectory_whatever_the_threads() {
    let one = trajectory(5, 1, 150);
    assert_eq!(one, trajectory(5, 4, 150));
    assert_ne!(one, trajectory(6, 4, 150));
}

#[test]
fn ready_seats_see_their_reaction_and_body_and_the_rest_see_nothing() {
    let mut batch = HockeyBatch::new(BatchSpec::new(mixed(), 2)).expect("a valid batch");
    let out = batch.reset().expect("resets").clone();
    let seeds: Vec<u64> = (0..8).map(|env| batch.episode(env).unwrap().seed).collect();
    assert_ne!(
        seeds[0], seeds[4],
        "two envs with the same seats play different matches"
    );
    for env in 0..8 {
        let episode = batch.episode(env).expect("an env");
        assert!(
            episode.seed >= 1 << 63,
            "training seeds stay off the evaluation seeds"
        );
        assert!(out.ready[env * 2] || out.ready[env * 2 + 1], "env {env}");
        for seat in 0..2 {
            let slot = env * 2 + seat;
            let features = &out.obs[slot * FEATURES..(slot + 1) * FEATURES];
            if out.ready[slot] {
                let reaction = episode.reaction_ms[seat];
                assert!((150..=400).contains(&reaction), "{reaction}");
                assert_eq!(features[16], reaction as f32 / 1000.0);
                assert_eq!(features[17], episode.physique[seat] as f32);
                assert!((0.4..=1.0).contains(&episode.physique[seat]));
            } else {
                assert!(features.iter().all(|&v| v == 0.0), "env {env} seat {seat}");
            }
        }
    }
}

#[test]
fn the_batch_plays_what_run_match_plays() {
    let mut spec = BatchSpec::new(vec![[Seat::Learner, Seat::Level(2)]], 9);
    spec.threads = 2;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let episode = batch.episode(0).expect("env 0");
    for _ in 0..400 {
        out = batch.step(&actions(&out, 1)).expect("steps").clone();
        assert!(!out.terminated[0] && !out.truncated[0]);
    }
    let ticks = batch.ticks(0).expect("env 0");
    assert!(ticks > 1600, "{ticks}");

    let params = Params::default();
    let mut game = AirHockey::new(params.clone(), 0);
    let spec = game.spec();
    let body = |physique: f64| -> BodyParamsT {
        let mut d = Difficulty::preset(5).unwrap();
        d.physique = physique;
        d.body(&spec)
    };
    let mut pilot = Pilot::new(
        HockeyPlaybook::new(params.clone()),
        Same,
        &spec,
        episode.reaction_ms[0],
        episode.physique[0],
    )
    .expect("a pilot");
    let level = Difficulty::preset(2).unwrap();
    assert_eq!(episode.reaction_ms[1], level.reaction_ms);
    let mut bot = Bot::new(level, HockeyPlaybook::new(params.clone()), &spec).expect("a bot");
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: GAME_VERSION,
        seed: episode.seed,
        bodies: vec![body(episode.physique[0]), level.body(&spec)],
        levels: [0, 0],
    };
    run_match(&mut game, [&mut pilot, &mut bot], header, ticks).expect("runs");
    assert_eq!(batch.digest(0), Some(game.digest(game.live())));
}

#[test]
fn the_cap_is_a_truncation_not_a_terminal() {
    let mut spec = BatchSpec::new(
        vec![[Seat::Learner, Seat::Reader { anticipates: false }]],
        3,
    );
    spec.cap_ticks = 2_400;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let first = batch.episode(0).expect("env 0");
    let mut steps = 0;
    while !out.truncated[0] {
        let before = batch.ticks(0).expect("env 0");
        out = batch.step(&still(1, [-0.9, 0.9])).expect("steps").clone();
        assert!(
            !out.terminated[0],
            "no match ends in ten seconds of a still learner"
        );
        steps += 1;
        assert!(
            steps <= 600,
            "the cap of 2400 ticks passed without a truncation"
        );
        if !out.truncated[0] {
            assert_eq!(batch.ticks(0), Some(before + 4));
        }
    }
    let last = &out.final_obs[..FEATURES];
    assert!(last.iter().any(|&v| v != 0.0));
    assert_ne!(last, &out.obs[..FEATURES]);
    assert!(batch.ticks(0).expect("env 0") < 200);
    assert_ne!(batch.episode(0).expect("env 0").seed, first.seed);
    assert!(out.ready[0]);
}

#[test]
fn a_finished_match_is_a_terminal_and_its_rewards_sum_to_the_score() {
    let mut spec = BatchSpec::new(vec![[Seat::Level(5), Seat::Learner]], 4);
    spec.params = Params {
        goals_to_win: 2,
        ..Params::default()
    };
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let first = batch.episode(0).expect("env 0").seed;
    let mut rewards = [0.0_f32; 2];
    for _ in 0..20_000 {
        out = batch.step(&still(1, [-0.9, 0.9])).expect("steps").clone();
        rewards[0] += out.reward[0];
        rewards[1] += out.reward[1];
        assert!(!out.truncated[0]);
        if out.terminated[0] {
            break;
        }
    }
    assert!(out.terminated[0], "level 5 beats a still learner to two");
    assert_eq!([out.score[0], out.score[1]], [2, 0]);
    assert_eq!(rewards, [2.0, -2.0]);
    assert!(out.final_obs.iter().all(|&v| v == 0.0));
    assert_ne!(batch.episode(0).expect("env 0").seed, first);
    assert!(batch.ticks(0).expect("env 0") < 200);
}

#[test]
fn malformed_batches_and_actions_are_refused() {
    let spec = |seats: Vec<[Seat; 2]>| BatchSpec::new(seats, 1);
    let refused = |s: BatchSpec| HockeyBatch::new(s).err();
    assert_eq!(refused(spec(vec![])), Some(BatchError::Empty));
    assert_eq!(
        refused(spec(vec![
            [Seat::Learner, Seat::Human],
            [Seat::Human, Seat::Level(2)]
        ])),
        Some(BatchError::NoLearner(1))
    );
    assert_eq!(
        refused(spec(vec![[Seat::Learner, Seat::Level(6)]])),
        Some(BatchError::BadLevel(6))
    );
    assert_eq!(
        refused(spec(vec![[Seat::Learner, Seat::Level(0)]])),
        Some(BatchError::BadLevel(0))
    );
    let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
    s.threads = 0;
    assert_eq!(refused(s), Some(BatchError::ZeroThreads));
    for cap in [0, 6] {
        let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
        s.cap_ticks = cap;
        assert_eq!(refused(s), Some(BatchError::BadCap(cap)));
    }
    for (reaction, physique) in [
        ([400, 150], [0.4, 1.0]),
        ([150, 400], [0.0, 1.0]),
        ([150, 400], [0.9, 0.5]),
        ([150, 400], [0.5, 1.1]),
    ] {
        let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
        s.reaction_ms = reaction;
        s.physique = physique;
        assert_eq!(
            refused(s),
            Some(BatchError::BadRange),
            "{reaction:?} {physique:?}"
        );
    }
    let mut batch = HockeyBatch::new(spec(vec![[Seat::Learner, Seat::Human]; 3])).expect("valid");
    batch.reset().expect("resets");
    assert_eq!(
        batch.step(&[0.0; 11]).err(),
        Some(BatchError::BadActions {
            expected: 12,
            got: 11
        })
    );
    assert!(batch.step(&[0.0; 12]).is_ok());
}

#[test]
fn a_seat_still_warming_up_after_a_new_episode_sees_nothing() {
    let mut spec = BatchSpec::new(vec![[Seat::Learner, Seat::Learner]; 4], 8);
    spec.cap_ticks = 400;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    batch.reset().expect("resets");
    let mut warming = 0;
    for _ in 0..2_000 {
        let out = batch.step(&still(4, [-0.5, 0.0])).expect("steps");
        for slot in 0..8 {
            let features = &out.obs[slot * FEATURES..(slot + 1) * FEATURES];
            if out.ready[slot] {
                assert!(features[16] > 0.0);
            } else {
                warming += 1;
                assert!(features.iter().all(|&v| v == 0.0), "slot {slot}");
            }
        }
    }
    assert!(warming > 20, "{warming} warming seats seen");
}
