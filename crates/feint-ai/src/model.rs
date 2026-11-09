pub const MIN_EVIDENCE: u32 = 3;
pub const COUNT_CAP: f64 = 1.0e6;

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub game_id: String,
    pub classes: u8,
    pub bite: Vec<f64>,
    pub bite_events: Vec<u32>,
    pub shots: Vec<f64>,
    pub shot_events: u32,
    pub mine: Vec<f64>,
    pub mine_events: Vec<u32>,
    pub fake_tries: f64,
    pub fake_wins: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    OtherGame,
    WrongSize,
    BadCount,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpponentModel {
    classes: usize,
    keep: f64,
    bite: Vec<f64>,
    bite_events: Vec<u32>,
    shots: Vec<f64>,
    shot_events: u32,
    mine: Vec<f64>,
    mine_events: Vec<u32>,
    fake_tries: f64,
    fake_wins: f64,
}

impl OpponentModel {
    #[must_use]
    pub fn new(classes: u8, adaptation: f64) -> Self {
        let n = usize::from(classes);
        Self {
            classes: n,
            keep: (1.0 - 0.1 * adaptation.clamp(0.0, 1.0)).clamp(0.0, 1.0),
            bite: vec![0.0; n * n],
            bite_events: vec![0; n],
            shots: vec![0.0; n],
            shot_events: 0,
            mine: vec![0.0; n * n],
            mine_events: vec![0; n],
            fake_tries: 0.0,
            fake_wins: 0.0,
        }
    }

    fn class(&self, class: Option<u8>) -> Option<usize> {
        class.map(usize::from).filter(|&c| c < self.classes)
    }

    fn decay(&mut self) {
        let keep = self.keep;
        for count in self
            .bite
            .iter_mut()
            .chain(self.shots.iter_mut())
            .chain(self.mine.iter_mut())
        {
            *count *= keep;
        }
        self.fake_tries *= keep;
        self.fake_wins *= keep;
    }

    pub fn record_attempt(
        &mut self,
        shown: Option<u8>,
        real: Option<u8>,
        covered: Option<u8>,
        deceptive: bool,
        won: bool,
    ) {
        self.decay();
        let (shown, real, covered) = (self.class(shown), self.class(real), self.class(covered));
        if let (Some(s), Some(c)) = (shown, covered) {
            self.bite[s * self.classes + c] += 1.0;
            self.bite_events[s] = self.bite_events[s].saturating_add(1);
        }
        if let (Some(s), Some(r)) = (shown, real) {
            self.mine[s * self.classes + r] += 1.0;
            self.mine_events[s] = self.mine_events[s].saturating_add(1);
        }
        if deceptive {
            self.fake_tries += 1.0;
            if won {
                self.fake_wins += 1.0;
            }
        }
    }

    pub fn record_shot(&mut self, class: u8) {
        self.decay();
        if let Some(c) = self.class(Some(class)) {
            self.shots[c] += 1.0;
            self.shot_events = self.shot_events.saturating_add(1);
        }
    }

    fn row(table: &[f64], classes: usize, row: usize) -> &[f64] {
        &table[row * classes..(row + 1) * classes]
    }

    fn argmax(values: &[f64]) -> Option<u8> {
        let mut best: Option<(usize, f64)> = None;
        for (i, &v) in values.iter().enumerate() {
            if best.is_none_or(|(_, b)| v > b) {
                best = Some((i, v));
            }
        }
        best.and_then(|(i, _)| u8::try_from(i).ok())
    }

    #[must_use]
    pub fn bite_probability(&self, shown: u8, covered: u8) -> f64 {
        let (Some(s), Some(c)) = (self.class(Some(shown)), self.class(Some(covered))) else {
            return 0.0;
        };
        let row = Self::row(&self.bite, self.classes, s);
        (row[c] + 1.0) / (row.iter().sum::<f64>() + self.classes as f64)
    }

    #[must_use]
    pub fn likely_cover(&self, shown: u8) -> Option<u8> {
        let s = self.class(Some(shown))?;
        (self.bite_events[s] >= MIN_EVIDENCE)
            .then(|| Self::argmax(Self::row(&self.bite, self.classes, s)))
            .flatten()
    }

    #[must_use]
    pub fn likely_real(&self, shown: u8) -> Option<u8> {
        let s = self.class(Some(shown))?;
        (self.mine_events[s] >= MIN_EVIDENCE)
            .then(|| Self::argmax(Self::row(&self.mine, self.classes, s)))
            .flatten()
    }

    #[must_use]
    pub fn likely_shot(&self) -> Option<u8> {
        (self.shot_events >= MIN_EVIDENCE)
            .then(|| Self::argmax(&self.shots))
            .flatten()
    }

    #[must_use]
    pub fn fake_win_rate(&self) -> Option<f64> {
        (self.fake_tries > 0.0).then(|| self.fake_wins / self.fake_tries)
    }

    pub fn reset(&mut self) {
        let keep = self.keep;
        *self = Self {
            keep,
            ..Self::new(u8::try_from(self.classes).unwrap_or(u8::MAX), 0.0)
        };
    }

    #[must_use]
    pub fn export(&self, game_id: &str) -> Profile {
        Profile {
            game_id: game_id.to_owned(),
            classes: u8::try_from(self.classes).unwrap_or(u8::MAX),
            bite: self.bite.clone(),
            bite_events: self.bite_events.clone(),
            shots: self.shots.clone(),
            shot_events: self.shot_events,
            mine: self.mine.clone(),
            mine_events: self.mine_events.clone(),
            fake_tries: self.fake_tries,
            fake_wins: self.fake_wins,
        }
    }

    pub fn import(&mut self, profile: &Profile, game_id: &str) -> Result<(), ProfileError> {
        if profile.game_id != game_id {
            return Err(ProfileError::OtherGame);
        }
        let n = self.classes;
        let sizes_match = usize::from(profile.classes) == n
            && profile.bite.len() == n * n
            && profile.mine.len() == n * n
            && profile.shots.len() == n
            && profile.bite_events.len() == n
            && profile.mine_events.len() == n;
        if !sizes_match {
            return Err(ProfileError::WrongSize);
        }
        let valid = |x: &f64| x.is_finite() && (0.0..=COUNT_CAP).contains(x);
        let counts_ok = profile
            .bite
            .iter()
            .chain(&profile.shots)
            .chain(&profile.mine)
            .chain([&profile.fake_tries, &profile.fake_wins])
            .all(valid)
            && profile.fake_wins <= profile.fake_tries;
        if !counts_ok {
            return Err(ProfileError::BadCount);
        }
        self.bite.clone_from(&profile.bite);
        self.bite_events.clone_from(&profile.bite_events);
        self.shots.clone_from(&profile.shots);
        self.shot_events = profile.shot_events;
        self.mine.clone_from(&profile.mine);
        self.mine_events.clone_from(&profile.mine_events);
        self.fake_tries = profile.fake_tries;
        self.fake_wins = profile.fake_wins;
        Ok(())
    }
}
