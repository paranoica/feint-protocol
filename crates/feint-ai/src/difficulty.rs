use feint_protocol::{BodyParamsT, GameSpecT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Memory {
    None,
    Match,
    MatchAndProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongGame {
    None,
    Simple,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Difficulty {
    pub reaction_ms: u32,
    pub physique: f64,
    pub spread: f64,
    pub anticipate: bool,
    pub repertoire: u8,
    pub depth: u8,
    pub deception: f64,
    pub memory: Memory,
    pub adaptation: f64,
    pub long_game: LongGame,
}

impl Difficulty {
    #[must_use]
    pub fn preset(level: u8) -> Option<Self> {
        let (reaction_ms, physique, spread, depth, deception, memory, adaptation, long_game) =
            match level {
                1 => (400, 0.4, 0.12, 0, 0.0, Memory::None, 0.0, LongGame::None),
                2 => (300, 0.6, 0.07, 0, 0.0, Memory::None, 0.0, LongGame::None),
                3 => (220, 0.75, 0.045, 1, 0.1, Memory::Match, 0.3, LongGame::None),
                4 => (
                    170,
                    0.9,
                    0.025,
                    2,
                    0.4,
                    Memory::Match,
                    0.6,
                    LongGame::Simple,
                ),
                5 => (
                    150,
                    1.0,
                    0.01,
                    3,
                    0.7,
                    Memory::MatchAndProfile,
                    1.0,
                    LongGame::Full,
                ),
                _ => return None,
            };
        Some(Self {
            reaction_ms,
            physique,
            spread,
            anticipate: level >= 2,
            repertoire: match level {
                1 => 0,
                2 => 1,
                _ => 2,
            },
            depth,
            deception,
            memory,
            adaptation,
            long_game,
        })
    }

    #[must_use]
    pub fn body(&self, spec: &GameSpecT) -> BodyParamsT {
        let p = self.physique;
        let values = spec
            .body_params
            .iter()
            .flatten()
            .map(|dim| (f64::from(dim.max) * p + f64::from(dim.min) * (1.0 - p)) as f32)
            .collect();
        let mut body = BodyParamsT::default();
        body.values = Some(values);
        body
    }
}
