use airhockey::{AirHockey, HockeyPlaybook, Params, human::Reader};
use feint_ai::{Bot, Difficulty, RunHeader, delay_ticks, run_match};
use feint_ml::registry::CHECKPOINTS;
use feint_ml::trained::{TrainedError, trained_pilot};
use feint_protocol::GameAdapter;

fn header(level: u8, seed: u64, game: &AirHockey) -> RunHeader {
    let spec = game.spec();
    let body = Difficulty::preset(level).expect("a level").body(&spec);
    RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: vec![body.clone(), body],
        levels: [0, level],
    }
}

#[test]
fn the_trained_pilot_uses_the_levels_reaction_and_physique() {
    let id = CHECKPOINTS[0].id;
    for level in 1..=5 {
        let preset = Difficulty::preset(level).unwrap();
        let mut game = AirHockey::new(Params::default(), level);
        let spec = game.spec();
        let mut pilot = trained_pilot(level, id, &spec).unwrap();
        let mut other = Reader::new(Params::default(), true);
        let head = header(level, 3, &game);
        let report = run_match(&mut game, [&mut other, &mut pilot], head, 600).unwrap();
        let features = pilot.features();
        assert_eq!(
            features[16],
            preset.reaction_ms as f32 / 1000.0,
            "level {level}"
        );
        assert_eq!(features[17], preset.physique as f32, "level {level}");
        let delay = delay_ticks(preset.reaction_ms, 240) as u64;
        let first = report.actions.iter().find(|a| a.player == 1).unwrap().tick;
        assert_eq!(first, delay.div_ceil(4) * 4, "level {level}");
    }
}

#[test]
fn unknown_levels_and_checkpoints_are_refused() {
    let spec = AirHockey::new(Params::default(), 5).spec();
    let id = CHECKPOINTS[0].id;
    assert!(matches!(
        trained_pilot(0, id, &spec),
        Err(TrainedError::Level)
    ));
    assert!(matches!(
        trained_pilot(6, id, &spec),
        Err(TrainedError::Level)
    ));
    assert!(matches!(
        trained_pilot(5, 0, &spec),
        Err(TrainedError::Checkpoint)
    ));
    assert!(matches!(
        trained_pilot(5, id ^ 1, &spec),
        Err(TrainedError::Checkpoint)
    ));
}

#[test]
fn a_trained_pilot_plays_a_whole_match_through_run_match() {
    let id = CHECKPOINTS[0].id;
    let play = || {
        let mut game = AirHockey::new(Params::default(), 5);
        let spec = game.spec();
        let mut pilot = trained_pilot(5, id, &spec).unwrap();
        let mut bot = Bot::new(
            Difficulty::preset(5).unwrap(),
            HockeyPlaybook::new(Params::default()),
            &spec,
        )
        .unwrap();
        let head = header(5, 11, &game);
        run_match(&mut game, [&mut bot, &mut pilot], head, 240 * 60).unwrap()
    };
    let first = play();
    let second = play();
    assert_eq!(first.digest, second.digest);
    assert_eq!(first.actions, second.actions);
    let mine = first.actions.iter().filter(|a| a.player == 1).count() as u64;
    assert!(
        mine > (first.ticks - 36) / 4 / 2,
        "{mine} actions in {} ticks",
        first.ticks
    );
}
