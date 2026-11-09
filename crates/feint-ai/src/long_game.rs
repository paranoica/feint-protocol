use crate::difficulty::LongGame;
use crate::playbook::Choice;

pub const PATTERN_BONUS: f64 = 0.2;
pub const DECISIVE: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pattern {
    id: u16,
    shown: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    mode: LongGame,
    pattern: Option<Pattern>,
    rallies: u32,
}

impl Plan {
    #[must_use]
    pub fn new(mode: LongGame) -> Self {
        Self {
            mode,
            pattern: None,
            rallies: 0,
        }
    }

    pub fn reset(&mut self) {
        self.pattern = None;
        self.rallies = 0;
    }

    fn breaks(pattern: Pattern, choice: &Choice) -> bool {
        choice.deceptive && choice.shown == Some(pattern.shown)
    }

    #[must_use]
    pub fn bonus(&self, choice: &Choice, importance: f64) -> f64 {
        let Some(pattern) = self.pattern.filter(|_| self.mode != LongGame::None) else {
            return 0.0;
        };
        let decisive = importance >= DECISIVE;
        if choice.id == pattern.id && !choice.deceptive {
            if decisive {
                -PATTERN_BONUS
            } else {
                PATTERN_BONUS
            }
        } else if decisive && Self::breaks(pattern, choice) {
            PATTERN_BONUS
        } else {
            0.0
        }
    }

    #[must_use]
    pub fn allows(&self, choice: &Choice, importance: f64) -> bool {
        self.mode != LongGame::None
            && importance >= DECISIVE
            && self.pattern.is_some_and(|p| Self::breaks(p, choice))
    }

    pub fn committed(&mut self, choice: &Choice, importance: f64) {
        if self.mode == LongGame::None {
            return;
        }
        match self.pattern {
            None => {
                if let (false, Some(shown), true) =
                    (choice.deceptive, choice.shown, importance < DECISIVE)
                {
                    self.pattern = Some(Pattern {
                        id: choice.id,
                        shown,
                    });
                }
            }
            Some(pattern) => {
                if self.mode == LongGame::Full && Self::breaks(pattern, choice) {
                    self.pattern = None;
                }
            }
        }
    }

    pub fn rally(&mut self) {
        self.rallies = self.rallies.saturating_add(1);
    }

    #[must_use]
    pub fn pace(&self) -> f32 {
        if self.mode == LongGame::Full && self.rallies % 2 == 1 {
            0.0
        } else {
            1.0
        }
    }
}
