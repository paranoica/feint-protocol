use std::num::NonZeroU64;

use feint_protocol::{
    BotKind, DecodeError, MatchHeaderT, MatchResultT, MessageT, ProfileT, decode, encode,
};

use crate::model::Profile;
use crate::runner::{Opponent, Record, RunHeader};

pub const MAX_RECORD_BYTES: usize = 64 << 20;

#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    Frame,
    Message(DecodeError),
    Order,
    Missing,
    Opponent,
}

fn frame(out: &mut Vec<u8>, message: &MessageT) {
    let body = encode(message);
    let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
}

#[must_use]
pub fn encode_record(record: &Record) -> Vec<u8> {
    let mut out = Vec::new();
    let mut header = MatchHeaderT::default();
    header.game_id = Some(record.header.game_id.clone());
    header.game_version = record.header.game_version;
    header.seed = record.header.seed;
    header.level = record.header.levels[1];
    header.bodies = Some(record.header.bodies.clone());
    if let Opponent::Trained(checkpoint) = record.header.opponent {
        header.bot = BotKind::Trained;
        header.checkpoint = checkpoint.get();
    }
    frame(&mut out, &MessageT::MatchHeader(Box::new(header)));
    for action in &record.actions {
        frame(&mut out, &MessageT::Action(Box::new(action.clone())));
    }
    let mut result = MatchResultT::default();
    result.score = Some(record.score.clone());
    result.duration_ticks = record.ticks;
    result.level = record.header.levels[1];
    result.seed = record.header.seed;
    frame(&mut out, &MessageT::MatchResult(Box::new(result)));
    out
}

fn opponent(bot: BotKind, checkpoint: u64) -> Result<Opponent, RecordError> {
    match (bot, checkpoint) {
        (BotKind::HandMade, 0) => Ok(Opponent::HandMade),
        (BotKind::Trained, c) => NonZeroU64::new(c)
            .map(Opponent::Trained)
            .ok_or(RecordError::Opponent),
        _ => Err(RecordError::Opponent),
    }
}

pub fn decode_record(bytes: &[u8]) -> Result<Record, RecordError> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(RecordError::Frame);
    }
    let mut header: Option<RunHeader> = None;
    let mut actions = Vec::new();
    let mut closing: Option<MatchResultT> = None;
    let mut rest = bytes;
    while !rest.is_empty() {
        let (len, body) = rest.split_at_checked(4).ok_or(RecordError::Frame)?;
        let len = u32::from_le_bytes(len.try_into().map_err(|_| RecordError::Frame)?) as usize;
        let (message, tail) = body.split_at_checked(len).ok_or(RecordError::Frame)?;
        rest = tail;
        if closing.is_some() {
            return Err(RecordError::Order);
        }
        match (decode(message).map_err(RecordError::Message)?, &header) {
            (MessageT::MatchHeader(h), None) => {
                header = Some(RunHeader {
                    game_id: h.game_id.unwrap_or_default(),
                    game_version: h.game_version,
                    seed: h.seed,
                    bodies: h.bodies.unwrap_or_default(),
                    levels: [0, h.level],
                    opponent: opponent(h.bot, h.checkpoint)?,
                });
            }
            (MessageT::Action(a), Some(_)) => {
                if actions
                    .last()
                    .is_some_and(|last: &feint_protocol::ActionT| last.tick > a.tick)
                {
                    return Err(RecordError::Order);
                }
                actions.push(*a);
            }
            (MessageT::MatchResult(r), Some(_)) => closing = Some(*r),
            _ => return Err(RecordError::Order),
        }
    }
    let (Some(header), Some(result)) = (header, closing) else {
        return Err(RecordError::Missing);
    };
    Ok(Record {
        header,
        actions,
        ticks: result.duration_ticks,
        score: result.score.unwrap_or_default(),
    })
}

#[must_use]
pub fn encode_profile(profile: &Profile) -> Vec<u8> {
    let mut p = ProfileT::default();
    p.game_id = Some(profile.game_id.clone());
    p.classes = profile.classes;
    p.bite = Some(profile.bite.clone());
    p.bite_events = Some(profile.bite_events.clone());
    p.shots = Some(profile.shots.clone());
    p.shot_events = profile.shot_events;
    p.mine = Some(profile.mine.clone());
    p.mine_events = Some(profile.mine_events.clone());
    p.fake_tries = profile.fake_tries;
    p.fake_wins = profile.fake_wins;
    p.theirs = Some(profile.theirs.clone());
    p.theirs_events = Some(profile.theirs_events.clone());
    p.bluffs = Some(profile.bluffs.clone());
    p.knowing = profile.knowing;
    p.reading = profile.reading;
    encode(&MessageT::Profile(Box::new(p)))
}

pub fn decode_profile(bytes: &[u8]) -> Result<Profile, RecordError> {
    match decode(bytes).map_err(RecordError::Message)? {
        MessageT::Profile(p) => Ok(Profile {
            game_id: p.game_id.unwrap_or_default(),
            classes: p.classes,
            bite: p.bite.unwrap_or_default(),
            bite_events: p.bite_events.unwrap_or_default(),
            shots: p.shots.unwrap_or_default(),
            shot_events: p.shot_events,
            mine: p.mine.unwrap_or_default(),
            mine_events: p.mine_events.unwrap_or_default(),
            fake_tries: p.fake_tries,
            fake_wins: p.fake_wins,
            theirs: p.theirs.unwrap_or_default(),
            theirs_events: p.theirs_events.unwrap_or_default(),
            bluffs: p.bluffs.unwrap_or_default(),
            knowing: p.knowing,
            reading: p.reading,
        }),
        _ => Err(RecordError::Order),
    }
}
