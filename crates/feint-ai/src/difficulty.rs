use feint_protocol::{BodyParamsT, GameSpecT};

/// What the bot remembers about its opponent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Memory {
    None,
    Match,
    MatchAndProfile,
}

/// How far the bot plays the long game: conditioning, breaking patterns, tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LongGame {
    None,
    Simple,
    Full,
}

/// The difficulty vector; levels are presets of it (FNT-D8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Difficulty {
    /// How old the world the bot sees is.
    pub reaction_ms: u32,
    /// Fraction of every declared body maximum the bot gets.
    pub physique: f64,
    /// Aim noise as a fraction of the play area; for discrete actions, the chance of a
    /// random legal action.
    pub spread: f64,
    /// Roll the delayed view forward to «now» before choosing.
    pub anticipate: bool,
    /// 0: a single option; 1: the full set of basic options.
    pub repertoire: u8,
    /// «He thinks that I think…» depth (from B3).
    pub depth: u8,
    /// Inclination to feint, 0..=1 (from B3).
    pub deception: f64,
    pub memory: Memory,
    /// How fast the bot adapts to its opponent, 0..=1 (from B3).
    pub adaptation: f64,
    pub long_game: LongGame,
}

impl Difficulty {
    /// The preset of level 1 («Лох») to 5 («Feint»).
    #[must_use]
    pub fn preset(level: u8) -> Option<Self> {
        // Columns of the spec's level table; numbers are starting values for playtests.
        let (reaction_ms, physique, spread, depth, deception, memory, adaptation, long_game) =
            match level {
                1 => (400, 0.4, 0.08, 0, 0.0, Memory::None, 0.0, LongGame::None),
                2 => (300, 0.6, 0.05, 0, 0.0, Memory::None, 0.0, LongGame::None),
                3 => (220, 0.75, 0.03, 1, 0.1, Memory::Match, 0.3, LongGame::None),
                4 => (170, 0.9, 0.02, 2, 0.4, Memory::Match, 0.6, LongGame::Simple),
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
            repertoire: u8::from(level >= 2),
            depth,
            deception,
            memory,
            adaptation,
            long_game,
        })
    }

    /// The bot's body: each declared body parameter at `physique` between its min and max.
    #[must_use]
    pub fn body(&self, spec: &GameSpecT) -> BodyParamsT {
        let p = self.physique;
        let values = spec
            .body_params
            .iter()
            .flatten()
            .map(|dim| {
                // Exactly `max` at p = 1 and `min` at p = 0.
                (f64::from(dim.max) * p + f64::from(dim.min) * (1.0 - p)) as f32
            })
            .collect();
        let mut body = BodyParamsT::default();
        body.values = Some(values);
        body
    }
}
