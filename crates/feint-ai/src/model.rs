pub const MIN_EVIDENCE: u32 = 3;
pub const COUNT_CAP: f64 = 1.0e6;
pub const ODDS_FLOOR: f64 = 0.1;

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
    pub theirs: Vec<f64>,
    pub theirs_events: Vec<u32>,
    pub bluffs: Vec<f64>,
    pub knowing: f64,
    pub reading: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    NoMemory,
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
    knowing: f64,
    reading: f64,
    theirs: Vec<f64>,
    theirs_events: Vec<u32>,
    bluffs: Vec<f64>,
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
            knowing: 0.0,
            reading: 0.0,
            theirs: vec![0.0; n * n],
            theirs_events: vec![0; n],
            bluffs: vec![0.0; n],
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
            .chain(self.theirs.iter_mut())
            .chain(self.bluffs.iter_mut())
        {
            *count *= keep;
        }
        self.fake_tries *= keep;
        self.fake_wins *= keep;
        self.knowing *= keep;
        self.reading *= keep;
    }

    pub fn record_attempt(
        &mut self,
        shown: Option<u8>,
        real: Option<u8>,
        covered: Option<u8>,
        deceptive: bool,
        won: bool,
    ) {
        let (shown, real, covered) = (self.class(shown), self.class(real), self.class(covered));
        let counted = shown.is_some() && (real.is_some() || covered.is_some());
        if !counted && !deceptive {
            return;
        }
        let named = shown
            .zip(covered)
            .and_then(|(s, c)| Some((u8::try_from(s).ok()?, u8::try_from(c).ok()?)))
            .map(|(s, c)| {
                (
                    self.likely_real(s) == Some(c),
                    self.likely_cover(s) == Some(c),
                )
            });
        self.decay();
        if let Some((knew, read)) = named {
            if knew {
                Self::bump(&mut self.knowing);
            }
            if read {
                Self::bump(&mut self.reading);
            }
        }
        if let (Some(s), Some(c)) = (shown, covered) {
            Self::bump(&mut self.bite[s * self.classes + c]);
            self.bite_events[s] = self.bite_events[s].saturating_add(1);
        }
        if let (Some(s), Some(r)) = (shown, real) {
            Self::bump(&mut self.mine[s * self.classes + r]);
            self.mine_events[s] = self.mine_events[s].saturating_add(1);
        }
        if deceptive {
            Self::bump(&mut self.fake_tries);
            if won {
                Self::bump(&mut self.fake_wins);
            }
        }
    }

    pub fn record_shot(&mut self, class: u8) {
        self.record_shot_after(class, &[]);
    }

    pub fn record_shot_after(&mut self, class: u8, shown: &[u8]) {
        let Some(c) = self.class(Some(class)) else {
            return;
        };
        self.decay();
        Self::bump(&mut self.shots[c]);
        self.shot_events = self.shot_events.saturating_add(1);
        let n = self.classes;
        for s in shown.iter().map(|&s| usize::from(s)).filter(|&s| s < n) {
            Self::bump(&mut self.theirs[s * n + c]);
            self.theirs_events[s] = self.theirs_events[s].saturating_add(1);
        }
    }

    fn bump(count: &mut f64) {
        *count = (*count + 1.0).min(COUNT_CAP);
    }

    fn row(table: &[f64], classes: usize, row: usize) -> &[f64] {
        &table[row * classes..(row + 1) * classes]
    }

    fn argmax(values: &[f64], prefer: usize) -> Option<u8> {
        if values.iter().sum::<f64>() <= 0.0 {
            return None;
        }
        let mut best: Option<(usize, f64)> = None;
        for (i, &v) in values.iter().enumerate() {
            let wins = best.is_none_or(|(b, bv)| v > bv || (v == bv && i == prefer && b != prefer));
            if wins {
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
            .then(|| Self::argmax(Self::row(&self.bite, self.classes, s), s))
            .flatten()
    }

    #[must_use]
    pub fn likely_real(&self, shown: u8) -> Option<u8> {
        let s = self.class(Some(shown))?;
        (self.mine_events[s] >= MIN_EVIDENCE)
            .then(|| Self::argmax(Self::row(&self.mine, self.classes, s), s))
            .flatten()
    }

    pub fn record_bluff(&mut self, shown: &[u8]) {
        let n = self.classes;
        let shown: Vec<usize> = shown
            .iter()
            .map(|&s| usize::from(s))
            .filter(|&s| s < n)
            .collect();
        if shown.is_empty() {
            return;
        }
        self.decay();
        for s in shown {
            Self::bump(&mut self.bluffs[s]);
            self.theirs_events[s] = self.theirs_events[s].saturating_add(1);
        }
    }

    #[must_use]
    pub fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        let s = self.class(Some(shown))?;
        if self.theirs_events[s] < MIN_EVIDENCE {
            return None;
        }
        let row = Self::row(&self.theirs, self.classes, s);
        let bluffs = self.bluffs[s];
        let total = row.iter().sum::<f64>() + bluffs + self.classes as f64 + 1.0;
        let kept: Vec<(Option<u8>, f64)> = row
            .iter()
            .enumerate()
            .filter_map(|(k, &v)| Some((Some(u8::try_from(k).ok()?), v)))
            .chain([(None, bluffs)])
            .filter(|&(_, v)| v > 0.0 || (v + 1.0) / total >= ODDS_FLOOR)
            .map(|(k, v)| (k, (v + 1.0) / total))
            .collect();
        let mass: f64 = kept.iter().map(|&(_, p)| p).sum();
        (mass > 0.0).then(|| kept.into_iter().map(|(k, p)| (k, p / mass)).collect())
    }

    #[must_use]
    pub fn likely_shot(&self) -> Option<u8> {
        (self.shot_events >= MIN_EVIDENCE)
            .then(|| Self::argmax(&self.shots, 0))
            .flatten()
    }

    #[must_use]
    pub fn knowing_beats_reading(&self) -> bool {
        self.knowing > self.reading
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
            theirs: self.theirs.clone(),
            theirs_events: self.theirs_events.clone(),
            bluffs: self.bluffs.clone(),
            knowing: self.knowing,
            reading: self.reading,
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
        let older = profile.theirs.is_empty() && profile.theirs_events.is_empty();
        let theirs_match =
            older || (profile.theirs.len() == n * n && profile.theirs_events.len() == n);
        let bluffs_match = profile.bluffs.is_empty() || profile.bluffs.len() == n;
        if !sizes_match || !theirs_match || !bluffs_match {
            return Err(ProfileError::WrongSize);
        }
        let valid = |x: &f64| x.is_finite() && (0.0..=COUNT_CAP).contains(x);
        let counts_ok = profile
            .bite
            .iter()
            .chain(&profile.shots)
            .chain(&profile.mine)
            .chain(&profile.theirs)
            .chain(&profile.bluffs)
            .chain([
                &profile.fake_tries,
                &profile.fake_wins,
                &profile.knowing,
                &profile.reading,
            ])
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
        if older {
            self.theirs.fill(0.0);
            self.theirs_events.fill(0);
        } else {
            self.theirs.clone_from(&profile.theirs);
            self.theirs_events.clone_from(&profile.theirs_events);
        }
        if profile.bluffs.is_empty() {
            self.bluffs.fill(0.0);
        } else {
            self.bluffs.clone_from(&profile.bluffs);
        }
        self.knowing = profile.knowing;
        self.reading = profile.reading;
        Ok(())
    }
}

pub trait Advisor<S>: Predictor {
    fn see(&mut self, state: &S, me: feint_protocol::PlayerId) {
        let _ = (state, me);
    }

    fn new_match(&mut self) {}
}

pub trait Predictor: Send {
    fn likely_shot(&self) -> Option<u8>;
    fn likely_cover(&self, shown: u8) -> Option<u8>;
    fn likely_real(&self, shown: u8) -> Option<u8>;
    fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>>;
    fn knowing_beats_reading(&self) -> bool;
    fn shot_odds(&self) -> Option<Vec<(u8, f64)>> {
        None
    }
}

impl Predictor for OpponentModel {
    fn likely_shot(&self) -> Option<u8> {
        OpponentModel::likely_shot(self)
    }

    fn likely_cover(&self, shown: u8) -> Option<u8> {
        OpponentModel::likely_cover(self, shown)
    }

    fn likely_real(&self, shown: u8) -> Option<u8> {
        OpponentModel::likely_real(self, shown)
    }

    fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        OpponentModel::their_odds(self, shown)
    }

    fn knowing_beats_reading(&self) -> bool {
        OpponentModel::knowing_beats_reading(self)
    }

    fn shot_odds(&self) -> Option<Vec<(u8, f64)>> {
        let total: f64 = self.shots.iter().map(|v| v + 1.0).sum();
        (self.shot_events >= MIN_EVIDENCE).then(|| {
            self.shots
                .iter()
                .enumerate()
                .filter_map(|(k, v)| Some((u8::try_from(k).ok()?, (v + 1.0) / total)))
                .collect()
        })
    }
}
