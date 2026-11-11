use airhockey::AirHockey;
use duel::QuickDraw;
use feint_ai::{
    Bot, Controller, Difficulty, Memory, Opponent, Profile, Record, RecordError, Runner,
    decode_record, encode_record,
};
use feint_protocol::ActionT;
use feint_web::{Column, HubGame, Play, Replayer, SessionError};
use penalty::Penalty;

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
    Checkpoint,
}

enum Seat<G: HubGame> {
    HandMade(Box<Bot<G, G::Book>>),
    Trained(Box<dyn Controller<G>>),
}

impl<G: HubGame> Seat<G> {
    fn controller(&mut self) -> &mut dyn Controller<G> {
        match self {
            Self::HandMade(bot) => bot.as_mut(),
            Self::Trained(pilot) => pilot.as_mut(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub tick: u64,
    pub values: Vec<Option<f64>>,
    pub events: Vec<(i8, i8)>,
    pub intent: Option<(u8, bool)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verified {
    pub game: &'static str,
    pub columns: &'static [Column],
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
    match record.header.game_id.as_str() {
        AirHockey::ID => verify_game::<AirHockey>(record, bytes, profile),
        Penalty::ID => verify_game::<Penalty>(record, bytes, profile),
        QuickDraw::ID => verify_game::<QuickDraw>(record, bytes, profile),
        _ => Err(VerifyError::Game),
    }
}

fn verify_game<G: HubGame>(
    record: Record,
    bytes: &[u8],
    profile: Option<&Profile>,
) -> Result<Verified, VerifyError> {
    if record.header.game_version != G::VERSION {
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
    let mut game = G::build(level);
    let header = Play::<G>::header(level, record.header.seed, record.header.opponent, &game)
        .ok_or(VerifyError::Level)?;
    if header != record.header {
        return Err(VerifyError::Bodies);
    }
    let mut human = Replayer::new(human_actions(&record)?);
    let mut bot = match record.header.opponent {
        Opponent::HandMade => {
            let mut bot =
                Bot::new(difficulty, G::book(), &game.spec()).map_err(|_| VerifyError::Level)?;
            if let Some(profile) = profile {
                bot.load_profile(profile, G::ID)
                    .map_err(|_| VerifyError::Profile)?;
            }
            Seat::HandMade(Box::new(bot))
        }
        Opponent::Trained(checkpoint) => {
            let pilot = G::trained(level, checkpoint, &game.spec()).map_err(|e| match e {
                SessionError::Unsupported => VerifyError::Game,
                SessionError::Level => VerifyError::Level,
                _ => VerifyError::Checkpoint,
            })?;
            if profile.is_some() {
                return Err(VerifyError::Profile);
            }
            Seat::Trained(pilot)
        }
    };
    let trained = matches!(bot, Seat::Trained(_));
    let mut runner = Runner::start(&mut game, [&mut human, bot.controller()], header)
        .map_err(|_| VerifyError::Bodies)?;
    let mut frames = Vec::with_capacity(usize::try_from(record.ticks.min(1 << 14)).unwrap_or(0));
    let mut intent = None;
    let mut seen = 0;
    while runner.ticks() < record.ticks
        && runner.step(
            &mut game,
            [&mut human as &mut dyn Controller<G>, bot.controller()],
        )
    {
        let fresh = &runner.actions()[seen..];
        if record.actions.get(seen..seen + fresh.len()) != Some(fresh) {
            return Err(VerifyError::Mismatch);
        }
        for action in fresh {
            if action.player == 1 && !trained {
                intent = Some((action.intent, action.deceptive));
            }
        }
        seen = runner.actions().len();
        frames.push(Frame {
            tick: runner.ticks() - 1,
            values: game.frame(),
            events: runner
                .events()
                .iter()
                .map(|e| (e.kind.0, e.player))
                .collect(),
            intent,
        });
    }
    let report = runner.finish(&mut game, [&mut human, bot.controller()]);
    if encode_record(&report.record()) != bytes {
        return Err(VerifyError::Mismatch);
    }
    Ok(Verified {
        game: G::ID,
        columns: G::COLUMNS,
        record,
        frames,
        finished: report.finished,
        score: report.score,
        digest: report.digest,
        profile: match &bot {
            Seat::HandMade(bot) => bot
                .profile(G::ID)
                .filter(|_| difficulty_keeps_profiles(level)),
            Seat::Trained(_) => None,
        },
    })
}

fn difficulty_keeps_profiles(level: u8) -> bool {
    Difficulty::preset(level).is_some_and(|d| d.memory == Memory::MatchAndProfile)
}
