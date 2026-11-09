use feint_protocol::{DecodeError, MatchHeaderT, MatchResultT, MessageT, decode, encode};

use crate::runner::{Record, RunHeader};

pub const MAX_RECORD_BYTES: usize = 64 << 20;

#[derive(Clone, Debug, PartialEq)]
pub enum RecordError {
    Frame,
    Message(DecodeError),
    Order,
    Missing,
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
