use airhockey::human::{HUMAN_NOISE, Human, Reader};
use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, run_match};
use feint_ml::batch::{FEATURES, OUTPUTS};
use feint_ml::mlp::Mlp;
use feint_ml::registry::CHECKPOINTS;
use feint_ml::{BatchError, BatchSpec, Episode, HockeyBatch, Outputs, Pilot, Policy, Seat};
use feint_protocol::GameAdapter;

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
    assert_eq!(batch.seats(2), Some([Seat::Human, Seat::Learner]));
    assert_eq!(batch.seats(8), None);
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

fn replayed(seats: [Seat; 2], episode: Episode, ticks: u64) -> u64 {
    let params = Params::default();
    let mut game = AirHockey::new(params.clone(), 0);
    let spec = game.spec();
    let mut players: Vec<Box<dyn Controller<AirHockey>>> = Vec::new();
    let mut bodies = Vec::new();
    for (index, seat) in seats.into_iter().enumerate() {
        let mut d = Difficulty::preset(5).unwrap();
        d.physique = episode.physique[index];
        bodies.push(d.body(&spec));
        players.push(match seat {
            Seat::Learner => Box::new(
                Pilot::new(
                    HockeyPlaybook::new(params.clone()),
                    Same,
                    &spec,
                    episode.reaction_ms[index],
                    episode.physique[index],
                )
                .expect("a pilot"),
            ),
            Seat::Level(level) => {
                let d = Difficulty::preset(level).unwrap();
                assert_eq!(episode.reaction_ms[index], d.reaction_ms);
                Box::new(Bot::new(d, HockeyPlaybook::new(params.clone()), &spec).expect("a bot"))
            }
            Seat::Human => Box::new(Human::new(params.clone(), HUMAN_NOISE)),
            Seat::Reader { anticipates } => Box::new(Reader::new(params.clone(), anticipates)),
            Seat::Frozen(_) => Box::new(
                Pilot::new(
                    HockeyPlaybook::new(params.clone()),
                    Mlp::from_safetensors(CHECKPOINTS[0].bytes).expect("the checkpoint"),
                    &spec,
                    episode.reaction_ms[index],
                    episode.physique[index],
                )
                .expect("a pilot"),
            ),
        });
    }
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: GAME_VERSION,
        seed: episode.seed,
        bodies,
        levels: [0, 0],
        opponent: feint_ai::Opponent::HandMade,
    };
    let [a, b] = &mut players[..] else {
        unreachable!("two seats")
    };
    run_match(&mut game, [a.as_mut(), b.as_mut()], header, ticks).expect("runs");
    game.digest(game.live())
}

#[test]
fn the_batch_plays_what_run_match_plays() {
    for seats in [
        [Seat::Learner, Seat::Level(2)],
        [Seat::Human, Seat::Learner],
        [Seat::Learner, Seat::Learner],
    ] {
        let mut spec = BatchSpec::new(vec![seats], 9);
        spec.threads = 2;
        spec.cap_ticks = 1_200;
        let mut batch = HockeyBatch::new(spec).expect("a valid batch");
        let mut out = batch.reset().expect("resets").clone();
        let first = batch.episode(0).expect("env 0");
        let ticks = batch.ticks(0).expect("env 0");
        assert_eq!(
            batch.digest(0),
            Some(replayed(seats, first, ticks)),
            "{seats:?} start"
        );
        while !out.truncated[0] {
            out = batch.step(&actions(&out, 1)).expect("steps").clone();
            assert!(!out.terminated[0], "{seats:?}");
        }
        let second = batch.episode(0).expect("env 0");
        assert_ne!(second.seed, first.seed);
        for _ in 0..200 {
            out = batch.step(&actions(&out, 1)).expect("steps").clone();
            assert!(!out.terminated[0] && !out.truncated[0], "{seats:?}");
        }
        let ticks = batch.ticks(0).expect("env 0");
        assert!(ticks > 800, "{ticks}");
        assert_eq!(
            batch.digest(0),
            Some(replayed(seats, second, ticks)),
            "{seats:?}"
        );
    }
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
    assert!(out.obs.iter().all(|&v| v == 0.0));
    assert_eq!(out.ready, [false, false]);
    assert_eq!(batch.ticks(0), Some(0));
    assert_ne!(batch.episode(0).expect("env 0").seed, first.seed);
    out = batch.step(&still(1, [-0.9, 0.9])).expect("steps").clone();
    assert!(out.ready[0] && !out.truncated[0]);
    assert!(out.final_obs.iter().all(|&v| v == 0.0));
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
    assert_eq!(batch.ticks(0), Some(0));
    assert_eq!(out.ready, [false, false]);
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
    for (reaction, cap, least) in [
        (400, 0, 100),
        (400, 96, 100),
        (400, 102, 100),
        (150, 36, 40),
    ] {
        let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
        s.reaction_ms = [150, reaction];
        s.cap_ticks = cap;
        assert_eq!(refused(s), Some(BatchError::BadCap { cap, least }));
    }
    let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
    s.reaction_ms = [400, 400];
    s.cap_ticks = 100;
    let mut tight = HockeyBatch::new(s).expect("the least cap is valid");
    let mut out = tight.reset().expect("resets").clone();
    for _ in 0..30 {
        assert!(out.ready[0] != out.truncated[0]);
        out = tight.step(&still(1, [0.0, 0.0])).expect("steps").clone();
    }
    let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
    s.params.goals_to_win = 0;
    assert!(matches!(refused(s), Some(BatchError::BadParams(_))));
    let mut s = spec(vec![[Seat::Learner, Seat::Human]]);
    s.params.decision_hz = 70;
    assert!(matches!(refused(s), Some(BatchError::BadParams(_))));
    for (reaction, physique) in [
        ([400, 150], [0.4, 1.0]),
        ([150, 400], [0.0, 1.0]),
        ([150, 400], [0.9, 0.5]),
        ([150, 400], [0.5, 1.1]),
        ([150, 300_000], [0.4, 1.0]),
        ([150, 400], [1e-50, 2e-45]),
        ([150, 400], [1e-300, 1e-300]),
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

#[test]
fn a_match_that_ends_before_the_learner_decides_comes_back_as_a_bare_terminal() {
    let mut spec = BatchSpec::new(vec![[Seat::Level(5), Seat::Learner]], 3);
    spec.reaction_ms = [60_000, 60_000];
    spec.params.goals_to_win = 1;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let mut bare = 0;
    for _ in 0..4 {
        if out.terminated[0] {
            bare += 1;
            assert_eq!(out.ready, [false, false]);
            assert!(out.obs.iter().all(|&v| v == 0.0));
            assert_eq!([out.score[0], out.score[1]], [1, 0]);
            assert_eq!(out.reward, [1.0, -1.0]);
            assert!(batch.ticks(0).expect("env 0") < 10);
        }
        out = batch.step(&still(1, [0.0, 0.0])).expect("steps").clone();
    }
    assert!(
        bare >= 3,
        "{bare} matches ended before a 60 s reaction passed"
    );
}

#[test]
fn every_terminal_carries_exactly_its_own_matchs_goals_and_score() {
    let mut spec = BatchSpec::new(vec![[Seat::Level(5), Seat::Learner]; 8], 6);
    spec.reaction_ms = [0, 4_000];
    spec.params.goals_to_win = 1;
    spec.threads = 4;
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    let mut out = batch.reset().expect("resets").clone();
    let mut since = [0.0_f32; 8];
    let mut after_decision = 0;
    let mut decided = [false; 8];
    for _ in 0..600 {
        for env in 0..8 {
            since[env] += out.reward[env * 2 + 1];
            decided[env] |= out.ready[env * 2 + 1];
            if out.terminated[env] || out.truncated[env] {
                let difference = (out.score[env * 2 + 1] - out.score[env * 2]) as f32;
                assert_eq!(since[env], difference, "env {env}");
                assert!(out.score[env * 2].max(out.score[env * 2 + 1]) <= 1);
                assert!(!out.ready[env * 2] && !out.ready[env * 2 + 1]);
                after_decision += usize::from(decided[env]);
                since[env] = 0.0;
                decided[env] = false;
            }
        }
        out = batch.step(&still(8, [0.0, 0.0])).expect("steps").clone();
    }
    assert!(
        after_decision >= 10,
        "{after_decision} terminals after a decision"
    );
}

#[test]
fn feature_scales_are_the_spec_maxima_and_one_for_reaction_and_body() {
    let params = Params::default();
    let scales = feint_ml::batch::feature_scales(&params);
    let (l, w) = (params.half_length as f32, params.half_width as f32);
    let (cap, mallet) = (params.puck_speed_cap as f32, params.max_body_speed as f32);
    assert_eq!(
        scales,
        [
            l,
            w,
            cap,
            cap,
            l,
            w,
            mallet,
            mallet,
            l,
            w,
            l,
            w,
            mallet,
            mallet,
            params.stall_seconds as f32,
            params.serve_pause_seconds as f32,
            1.0,
            1.0
        ]
    );
}

fn frozen_spec(seats: Vec<[Seat; 2]>, seed: u64) -> BatchSpec {
    let mut spec = BatchSpec::new(seats, seed);
    spec.frozen = vec![CHECKPOINTS[0].bytes.to_vec()];
    spec
}

#[test]
fn a_frozen_seat_plays_what_run_match_plays_with_the_same_pilot() {
    for seats in [
        [Seat::Learner, Seat::Frozen(0)],
        [Seat::Frozen(0), Seat::Learner],
    ] {
        let mut spec = frozen_spec(vec![seats], 13);
        spec.cap_ticks = 1_200;
        let mut batch = HockeyBatch::new(spec).expect("a valid batch");
        let mut out = batch.reset().expect("resets").clone();
        let first = batch.episode(0).expect("env 0");
        while !out.truncated[0] {
            out = batch.step(&actions(&out, 1)).expect("steps").clone();
        }
        for _ in 0..150 {
            out = batch.step(&actions(&out, 1)).expect("steps").clone();
        }
        let second = batch.episode(0).expect("env 0");
        assert_ne!(first.seed, second.seed);
        let ticks = batch.ticks(0).expect("env 0");
        assert!(ticks > 500, "{ticks}");
        assert_eq!(
            batch.digest(0),
            Some(replayed(seats, second, ticks)),
            "{seats:?}"
        );
    }
}

#[test]
fn a_frozen_seat_takes_the_episodes_reaction_and_body() {
    let spec = frozen_spec(vec![[Seat::Learner, Seat::Frozen(0)]; 64], 5);
    let (reaction, physique) = (spec.reaction_ms, spec.physique);
    let mut batch = HockeyBatch::new(spec).expect("a valid batch");
    batch.reset().expect("resets");
    let mut seen = Vec::new();
    for env in 0..64 {
        let e = batch.episode(env).unwrap();
        assert!(
            (reaction[0]..=reaction[1]).contains(&e.reaction_ms[1]),
            "{e:?}"
        );
        assert!(
            e.physique[1] >= physique[0] && e.physique[1] <= physique[1],
            "{e:?}"
        );
        seen.push(e.reaction_ms[1]);
    }
    seen.sort_unstable();
    seen.dedup();
    assert!(seen.len() > 20, "{} distinct reactions", seen.len());
}

#[test]
fn bad_frozen_bytes_or_index_are_refused() {
    let spec = frozen_spec(vec![[Seat::Learner, Seat::Frozen(1)]], 1);
    assert_eq!(HockeyBatch::new(spec).err(), Some(BatchError::Frozen(1)));
    let mut spec = BatchSpec::new(vec![[Seat::Learner, Seat::Frozen(0)]], 1);
    assert_eq!(
        HockeyBatch::new(spec.clone()).err(),
        Some(BatchError::Frozen(0))
    );
    spec.frozen = vec![b"not a checkpoint".to_vec()];
    assert_eq!(
        HockeyBatch::new(spec.clone()).err(),
        Some(BatchError::Frozen(0))
    );
    spec.frozen = vec![CHECKPOINTS[0].bytes.to_vec(), b"junk".to_vec()];
    assert_eq!(HockeyBatch::new(spec).err(), Some(BatchError::Frozen(1)));
    let mut spec = BatchSpec::new(vec![[Seat::Frozen(0), Seat::Frozen(0)]], 1);
    spec.frozen = vec![CHECKPOINTS[0].bytes.to_vec()];
    assert_eq!(HockeyBatch::new(spec).err(), Some(BatchError::NoLearner(0)));
}
