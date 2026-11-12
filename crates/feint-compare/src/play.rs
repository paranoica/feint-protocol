use airhockey::analysis::Frame;
use airhockey::human::{HUMAN_GOALS, HUMAN_NOISE, Human, Recorder};
use airhockey::{AirHockey, Params};
use feint_ai::{Controller, Difficulty, Opponent, RunHeader, RunReport, run_match};
use feint_protocol::{BodyParamsT, GameAdapter, GameSpecT};

pub const CAP_TICKS: u64 = 240 * 600;

#[must_use]
pub fn human_params() -> Params {
    Params {
        goals_to_win: HUMAN_GOALS,
        ..Params::default()
    }
}

#[must_use]
pub fn human_body(spec: &GameSpecT) -> BodyParamsT {
    Difficulty::preset(5)
        .unwrap_or_else(|| panic!("level 5 exists"))
        .body(spec)
}

#[must_use]
pub fn header(
    seed: u64,
    seat: u8,
    bot: BodyParamsT,
    human: BodyParamsT,
    opponent: Opponent,
) -> RunHeader {
    RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: if seat == 0 {
            vec![bot, human]
        } else {
            vec![human, bot]
        },
        levels: [0, 0],
        opponent,
    }
}

pub struct Played<C> {
    pub seat: u8,
    pub frames: Vec<Frame>,
    pub report: RunReport,
    pub bot: C,
}

fn frame(game: &AirHockey) -> Frame {
    let live = game.live();
    Frame {
        tick: live.tick(),
        puck: live.world.puck.pos,
        mallets: live.world.mallets.map(|m| m.pos),
        score: live.score(),
    }
}

pub fn play_against_human<C: Controller<AirHockey>>(
    make: impl FnOnce(&GameSpecT, &Params) -> (C, BodyParamsT, Opponent),
    seed: u64,
    seat: u8,
) -> Played<C> {
    let params = human_params();
    let mut game = AirHockey::new(params.clone(), 1);
    let spec = game.spec();
    let (mut bot, body, opponent) = make(&spec, &params);
    let mut human = Human::new(params, HUMAN_NOISE);
    let h = header(seed, seat, body, human_body(&spec), opponent);
    let mut recorder = Recorder {
        inner: &mut bot,
        frames: Vec::new(),
    };
    let report = if seat == 0 {
        run_match(&mut game, [&mut recorder, &mut human], h, CAP_TICKS)
    } else {
        run_match(&mut game, [&mut human, &mut recorder], h, CAP_TICKS)
    }
    .unwrap_or_else(|e| panic!("a match against the human model runs: {e:?}"));
    let mut frames = recorder.frames;
    let last = frame(&game);
    if frames.last().is_none_or(|f| f.tick < last.tick) {
        frames.push(last);
    }
    Played {
        seat,
        frames,
        report,
        bot,
    }
}
