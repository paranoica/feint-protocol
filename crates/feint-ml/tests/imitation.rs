use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{Bot, Controller, Difficulty, Opponent, RunHeader, encode_record, run_match};
use feint_ml::imitation::{ImitationError, MAX_TICKS, imitation, pilot_match};
use feint_ml::mlp::Mlp;
use feint_ml::registry::CHECKPOINTS;
use feint_ml::{Pilot, Policy};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};

const FEATURES: usize = 18;

fn human_body() -> BodyParamsT {
    let p = Params::default();
    let mut body = BodyParamsT::default();
    body.values = Some(vec![p.max_body_speed as f32, p.max_body_accel as f32]);
    body
}

fn header(seed: u64, level: u8, game: &AirHockey) -> RunHeader {
    RunHeader {
        game_id: "airhockey".into(),
        game_version: GAME_VERSION,
        seed,
        bodies: vec![
            human_body(),
            Difficulty::preset(level).unwrap().body(&game.spec()),
        ],
        levels: [0, level],
        opponent: Opponent::HandMade,
    }
}

struct Logged {
    mlp: Mlp,
    features: Vec<Vec<f32>>,
    outs: Vec<[f32; 2]>,
}

impl Policy for Logged {
    fn act(&mut self, features: &[f32], out: &mut [f32]) {
        self.mlp.act(features, out);
        self.features.push(features.to_vec());
        self.outs
            .push([out[0].clamp(-1.0, 1.0), out[1].clamp(-1.0, 1.0)]);
    }
}

struct Script {
    me: PlayerId,
    plan: Vec<(u64, [f32; 2])>,
}

impl Controller<AirHockey> for Script {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let (_, target) = self.plan.iter().find(|(tick, _)| *tick == obs.tick)?;
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.values = Some(target.to_vec());
        Some(action)
    }
}

fn scripted(plan: Vec<(u64, [f32; 2])>, ticks: u64) -> Vec<u8> {
    let mut game = AirHockey::new(Params::default(), 1);
    let head = header(5, 1, &game);
    let mut human = Script { me: 0, plan };
    let mut still = Script {
        me: 1,
        plan: Vec::new(),
    };
    let report = run_match(&mut game, [&mut human, &mut still], head, ticks).unwrap();
    encode_record(&report.record())
}

fn decisions_from_the_spec(ticks: u64, delay_ticks: u64) -> usize {
    (0..ticks)
        .filter(|t| t % 4 == 0 && *t >= delay_ticks)
        .count()
}

fn logged_match(ticks: u64) -> (Vec<u8>, Logged) {
    let mut game = AirHockey::new(Params::default(), 1);
    let spec = game.spec();
    let fresh = || Logged {
        mlp: focus(),
        features: Vec::new(),
        outs: Vec::new(),
    };
    let mut pilot = Pilot::new(
        HockeyPlaybook::new(Params::default()),
        fresh(),
        &spec,
        150,
        1.0,
    )
    .unwrap();
    let mut bot = Bot::new(
        Difficulty::preset(1).unwrap(),
        HockeyPlaybook::new(Params::default()),
        &spec,
    )
    .unwrap();
    let head = header(21, 1, &game);
    let report = run_match(&mut game, [&mut pilot, &mut bot], head, ticks).unwrap();
    let log = std::mem::replace(pilot.policy_mut(), fresh());
    (encode_record(&report.record()), log)
}

#[test]
fn a_pilots_own_match_gives_back_its_features_and_targets() {
    let (bytes, log) = logged_match(240 * 40);
    let ticks = feint_ai::decode_record(&bytes).unwrap().ticks;
    let got = imitation(&bytes, 150).unwrap();
    assert_eq!(got.ticks, ticks);
    assert_eq!(got.decisions, log.features.len());
    assert_eq!(got.decisions, decisions_from_the_spec(ticks, 36));
    assert_eq!(got.features.len(), got.decisions * FEATURES);
    assert_eq!(got.targets.len(), got.decisions * 2);
    for (k, seen) in log.features.iter().enumerate() {
        assert_eq!(
            seen[..],
            got.features[k * FEATURES..(k + 1) * FEATURES],
            "decision {k}"
        );
        for (a, b) in log.outs[k].iter().zip(&got.targets[k * 2..k * 2 + 2]) {
            assert!((a - b).abs() <= 1e-6, "decision {k}: target {b}, pilot {a}");
        }
    }
    assert!(
        log.outs.windows(2).any(|w| w[0] != w[1]),
        "the pilot never changed its target, the test proves nothing"
    );
}

#[test]
fn decisions_follow_the_cadence_and_the_delay() {
    let bytes = scripted(vec![(0, [-0.5, 0.1])], 1001);
    let got = imitation(&bytes, 300).unwrap();
    assert_eq!(got.decisions, decisions_from_the_spec(1001, 72));
    assert_eq!(got.decisions, 233);
    assert_eq!(got.features[16], 0.3);
    assert_eq!(got.features[17], 1.0);
}

#[test]
fn a_constant_target_maps_to_its_unit_value() {
    let p = Params::default();
    let bytes = scripted(vec![(0, [-0.5, 0.1])], 400);
    let got = imitation(&bytes, 300).unwrap();
    let x = -0.5 / p.half_length as f32;
    let y = 0.1 / p.half_width as f32;
    assert!(got.decisions > 0);
    for pair in got.targets.chunks(2) {
        assert_eq!(pair, [x, y]);
    }
}

#[test]
fn a_pointer_label_is_the_target_before_the_next_decision() {
    let p = Params::default();
    let (l, w) = (p.half_length as f32, p.half_width as f32);
    let a = [-0.5, 0.0];
    let b = [-0.4, 0.1];
    let c = [-0.3, -0.1];
    let d = [-0.2, 0.2];
    let e = [-0.6, -0.2];
    let plan = vec![(0, a), (74, b), (77, c), (81, d), (200, e)];
    let bytes = scripted(plan, 240);
    let got = imitation(&bytes, 300).unwrap();
    let unit = |v: [f32; 2]| [v[0] / l, v[1] / w];
    let mut expected = vec![unit(b), unit(c)];
    expected.extend(std::iter::repeat_n(unit(d), (196 - 80) / 4 + 1));
    expected.extend(std::iter::repeat_n(unit(e), (236 - 200) / 4 + 1));
    let labels: Vec<[f32; 2]> = got.targets.chunks(2).map(|c| [c[0], c[1]]).collect();
    assert_eq!(labels, expected);
    assert_ne!(labels[0], unit(a));
}

#[test]
fn before_the_first_move_the_label_is_where_the_mallet_starts() {
    let p = Params::default();
    let (l, w) = (p.half_length as f32, p.half_width as f32);
    let bytes = scripted(vec![(97, [-0.4, 0.1])], 120);
    let got = imitation(&bytes, 300).unwrap();
    let start = [(-(p.half_length * 0.8)) as f32 / l, 0.0];
    let labels: Vec<[f32; 2]> = got.targets.chunks(2).map(|c| [c[0], c[1]]).collect();
    let mut expected = vec![start; (92 - 72) / 4 + 1];
    expected.extend(std::iter::repeat_n([-0.4 / l, 0.1 / w], (116 - 96) / 4 + 1));
    assert_eq!(labels, expected);
}

fn edited(edit: impl FnOnce(&mut feint_ai::Record)) -> Vec<u8> {
    let bytes = scripted(vec![(0, [-0.5, 0.1]), (100, [-0.3, 0.0])], 400);
    let mut record = feint_ai::decode_record(&bytes).unwrap();
    edit(&mut record);
    encode_record(&record)
}

#[test]
fn a_wrong_score_is_refused() {
    let bytes = edited(|r| r.score = vec![3, 0]);
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Mismatch));
}

#[test]
fn a_truncated_record_is_refused() {
    let bytes = scripted(vec![(0, [-0.5, 0.1])], 400);
    assert!(matches!(
        imitation(&bytes[..bytes.len() - 7], 300),
        Err(ImitationError::Decode(_))
    ));
}

#[test]
fn another_game_or_version_is_refused() {
    let bytes = edited(|r| r.header.game_id = "penalty".into());
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Game));
    let bytes = edited(|r| r.header.game_version = GAME_VERSION + 1);
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Game));
}

#[test]
fn a_record_without_seat_zero_is_refused() {
    let bytes = edited(|r| r.actions.retain(|a| a.player != 0));
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::NoHuman));
}

#[test]
fn a_foreign_human_body_is_refused() {
    let spec = AirHockey::new(Params::default(), 1).spec();
    let slow = Difficulty::preset(1).unwrap().body(&spec);
    assert_ne!(slow, human_body());
    let bytes = edited(|r| r.header.bodies[0] = slow);
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Bodies));
}

#[test]
fn a_record_longer_than_the_arena_allows_is_refused() {
    assert_eq!(MAX_TICKS, 240 * 60 * 30);
    let bytes = edited(|r| r.ticks = MAX_TICKS + 1);
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Ticks));
    let bytes = edited(|r| r.ticks = 0);
    assert_eq!(imitation(&bytes, 300), Err(ImitationError::Ticks));
}

#[test]
fn a_reaction_past_the_limit_is_refused() {
    let bytes = scripted(vec![(0, [-0.5, 0.1])], 400);
    assert!(matches!(
        imitation(&bytes, u32::MAX),
        Err(ImitationError::Reaction(_))
    ));
}

fn focus() -> Mlp {
    let file = CHECKPOINTS
        .iter()
        .find(|c| c.name == "airhockey-focus")
        .unwrap();
    Mlp::from_safetensors(file.bytes).unwrap()
}

#[test]
fn a_pilot_match_seats_the_pilot_as_the_human_and_extracts() {
    let report = pilot_match(focus(), 1, 7, 150, 2400).unwrap();
    assert_eq!(report.ticks, 2400);
    assert_eq!(report.header.bodies[0], human_body());
    assert_eq!(report.header.levels, [0, 1]);
    assert_eq!(report.header.seed, 7);
    let first = report.actions.iter().find(|a| a.player == 0).unwrap().tick;
    assert_eq!(first, 36);
    let got = imitation(&encode_record(&report.record()), 150).unwrap();
    assert_eq!(got.decisions, decisions_from_the_spec(2400, 36));
}

#[test]
fn a_pilot_match_at_an_unknown_level_is_refused() {
    assert_eq!(
        pilot_match(focus(), 0, 7, 150, 2400).map(|r| r.ticks),
        Err(ImitationError::Level)
    );
    assert_eq!(
        pilot_match(focus(), 6, 7, 150, 2400).map(|r| r.ticks),
        Err(ImitationError::Level)
    );
}
