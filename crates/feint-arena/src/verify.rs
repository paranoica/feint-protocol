use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{
    Bot, Controller, Difficulty, Memory, Profile, Record, RecordError, Runner, decode_record,
    encode_record,
};
use feint_protocol::{ActionT, GameAdapter};
use feint_web::{GAME_ID, Replayer, Session};

pub const MAX_TICKS: u64 = 240 * 60 * 30;

#[derive(Clone, Debug, PartialEq)]
pub enum VerifyError {
    Decode(RecordError),
    Game,
    Level,
    Bodies,
    Ticks,
    Seat,
    Twice,
    Profile,
    Mismatch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub tick: u64,
    pub puck: [f64; 4],
    pub mallets: [[f64; 4]; 2],
    pub events: Vec<(i8, i8)>,
    pub intent: Option<(u8, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verified {
    pub record: Record,
    pub frames: Vec<Frame>,
    pub finished: bool,
    pub score: Vec<i64>,
    pub digest: u64,
    pub profile: Option<Profile>,
}

fn human_actions(record: &Record) -> Result<Vec<ActionT>, VerifyError> {
    let mut human: Vec<ActionT> = Vec::new();
    for action in &record.actions {
        match action.player {
            0 => {
                if human.last().is_some_and(|last| last.tick == action.tick) {
                    return Err(VerifyError::Twice);
                }
                human.push(action.clone());
            }
            1 => {}
            _ => return Err(VerifyError::Seat),
        }
    }
    Ok(human)
}

pub fn verify(bytes: &[u8], profile: Option<&Profile>) -> Result<Verified, VerifyError> {
    let record = decode_record(bytes).map_err(VerifyError::Decode)?;
    if record.header.game_id != GAME_ID || record.header.game_version != GAME_VERSION {
        return Err(VerifyError::Game);
    }
    let level = record.header.levels[1];
    let difficulty = Difficulty::preset(level).ok_or(VerifyError::Level)?;
    if profile.is_some() && difficulty.memory != Memory::MatchAndProfile {
        return Err(VerifyError::Profile);
    }
    if record.ticks == 0 || record.ticks > MAX_TICKS {
        return Err(VerifyError::Ticks);
    }
    let params = Params::default();
    let mut game = AirHockey::new(params.clone(), level);
    let header = Session::header(level, record.header.seed, &game).ok_or(VerifyError::Level)?;
    if header != record.header {
        return Err(VerifyError::Bodies);
    }
    let mut human = Replayer::new(human_actions(&record)?);
    let mut bot = Bot::new(difficulty, HockeyPlaybook::new(params), &game.spec())
        .map_err(|_| VerifyError::Level)?;
    if let Some(profile) = profile {
        bot.load_profile(profile, GAME_ID)
            .map_err(|_| VerifyError::Profile)?;
    }
    let mut runner = Runner::start(&mut game, [&mut human, &mut bot], header)
        .map_err(|_| VerifyError::Bodies)?;
    let mut frames = Vec::with_capacity(usize::try_from(record.ticks.min(1 << 14)).unwrap_or(0));
    let mut intent = None;
    let mut seen = 0;
    while runner.ticks() < record.ticks
        && runner.step(
            &mut game,
            [&mut human as &mut dyn Controller<AirHockey>, &mut bot],
        )
    {
        let fresh = &runner.actions()[seen..];
        if record.actions.get(seen..seen + fresh.len()) != Some(fresh) {
            return Err(VerifyError::Mismatch);
        }
        for action in fresh {
            if action.player == 1 {
                intent = Some((action.intent, action.deceptive));
            }
        }
        seen = runner.actions().len();
        let world = &game.live().world;
        let body = |pos: airhockey::Vec2, vel: airhockey::Vec2| [pos.x, pos.y, vel.x, vel.y];
        frames.push(Frame {
            tick: runner.ticks() - 1,
            puck: body(world.puck.pos, world.puck.vel),
            mallets: world.mallets.map(|m| body(m.pos, m.vel)),
            events: runner
                .events()
                .iter()
                .map(|e| (e.kind.0, e.player))
                .collect(),
            intent,
        });
    }
    let report = runner.finish(&mut game, [&mut human, &mut bot]);
    if encode_record(&report.record()) != bytes {
        return Err(VerifyError::Mismatch);
    }
    Ok(Verified {
        record,
        frames,
        finished: report.finished,
        score: report.score,
        digest: report.digest,
        profile: bot
            .profile(GAME_ID)
            .filter(|_| difficulty_keeps_profiles(level)),
    })
}

fn difficulty_keeps_profiles(level: u8) -> bool {
    Difficulty::preset(level).is_some_and(|d| d.memory == Memory::MatchAndProfile)
}
