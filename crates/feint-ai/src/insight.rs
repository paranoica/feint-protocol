use feint_protocol::{GameAdapter, PlayerId};

use crate::difficulty::{Difficulty, Memory};
use crate::model::{OpponentModel, Profile, ProfileError};
use crate::playbook::{Choice, Playbook, Stance};

const MIN_AWAY_SAMPLES: u32 = 6;
const ATTEMPT_AWAY_SAMPLES: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Attempt {
    shown: Option<u8>,
    real: Option<u8>,
    deceptive: bool,
    covered: Option<u8>,
    won: bool,
    fired: bool,
    premature: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) started: bool,
    pub(crate) rally: bool,
}

pub(crate) struct Insight<S> {
    memory: Memory,
    model: Option<OpponentModel>,
    prev: Option<S>,
    prev_score: Vec<i64>,
    attempt: Option<Attempt>,
    possession: Option<u64>,
    away: u32,
    since: Option<S>,
    own_prev: Option<S>,
    their_signals: Vec<u8>,
    pub(crate) gate_open: bool,
    pub(crate) fake_pending: bool,
    pub(crate) twitch: Option<u8>,
}

impl<S: Clone> Insight<S> {
    pub(crate) fn new(difficulty: &Difficulty, classes: u8) -> Self {
        let model = (difficulty.memory != Memory::None)
            .then(|| OpponentModel::new(classes, difficulty.adaptation));
        Self {
            memory: difficulty.memory,
            model,
            prev: None,
            prev_score: Vec::new(),
            attempt: None,
            possession: None,
            away: 0,
            since: None,
            own_prev: None,
            their_signals: Vec::new(),
            gate_open: false,
            fake_pending: false,
            twitch: None,
        }
    }

    pub(crate) fn new_match(&mut self) {
        if self.memory == Memory::Match
            && let Some(model) = &mut self.model
        {
            model.reset();
        }
        self.forget_moment();
    }

    pub(crate) fn forget_moment(&mut self) {
        self.prev = None;
        self.prev_score.clear();
        self.attempt = None;
        self.possession = None;
        self.away = 0;
        self.since = None;
        self.own_prev = None;
        self.their_signals.clear();
        self.gate_open = false;
        self.fake_pending = false;
        self.twitch = None;
    }

    pub(crate) fn observe<A, P>(
        &mut self,
        playbook: &P,
        state: &S,
        score: &[i64],
        me: PlayerId,
        them: PlayerId,
    ) -> Seen
    where
        A: GameAdapter<State = S>,
        P: Playbook<A>,
    {
        let possession = playbook.possession(state, me);
        let had = self.away == 0 && self.possession.is_some();
        let returned = self.away >= MIN_AWAY_SAMPLES;
        let mut started = possession.is_some() && (possession != self.possession || returned);
        let mut rally = false;
        if possession.is_some()
            && !started
            && let Some(attempt) = &mut self.attempt
        {
            attempt.premature = false;
        }
        if possession.is_some() && self.away > 0 {
            self.since = Some(state.clone());
        }
        if let Some(prev) = &self.prev {
            rally = score
                .iter()
                .zip(&self.prev_score)
                .any(|(now, before)| now > before);
            if let Some(shot) = playbook.read(prev, state, them).shot {
                if let Some(model) = &mut self.model {
                    model.record_shot_after(shot, &self.their_signals);
                }
                self.their_signals.clear();
            }
            let reference = self.since.as_ref().unwrap_or(prev);
            let covered = (had || possession.is_some())
                .then(|| playbook.read(reference, state, them).covered)
                .flatten();
            if let Some(covered) = covered {
                self.twitch = Some(covered);
                if let Some(attempt) = &mut self.attempt {
                    attempt.covered = Some(covered);
                }
            }
            let goals = |s: &[i64], p: PlayerId| s.get(usize::from(p)).copied().unwrap_or(0);
            if goals(score, me) > goals(&self.prev_score, me) {
                if let Some(attempt) = &mut self.attempt {
                    attempt.won = true;
                }
                self.end_attempt();
            }
            if goals(score, them) > goals(&self.prev_score, them) {
                self.end_attempt();
            }
        } else {
            started = possession.is_some();
        }
        if started {
            self.end_attempt();
            self.twitch = None;
            self.since = Some(state.clone());
            if let Some(model) = &mut self.model {
                model.record_bluff(&self.their_signals);
            }
            self.their_signals.clear();
        }
        if let Some(signal) = playbook.signal(state, them)
            && !self.their_signals.contains(&signal)
        {
            self.their_signals.push(signal);
        }
        if possession.is_some() {
            self.away = 0;
            self.possession = possession;
        } else {
            self.away = self.away.saturating_add(1);
            if self.away == ATTEMPT_AWAY_SAMPLES {
                self.end_attempt();
            }
        }
        self.prev = Some(state.clone());
        self.prev_score.clear();
        self.prev_score.extend_from_slice(score);
        Seen { started, rally }
    }

    pub(crate) fn watch_own<A, P>(&mut self, playbook: &P, state: &S, me: PlayerId)
    where
        A: GameAdapter<State = S>,
        P: Playbook<A>,
    {
        if let (Some(prev), Some(attempt)) = (&self.own_prev, &mut self.attempt)
            && playbook.read(prev, state, me).shot.is_some()
        {
            attempt.fired = true;
        }
        self.own_prev = Some(state.clone());
    }

    pub(crate) fn start_attempt(&mut self, choice: &Choice) {
        let quiet = choice.shown.is_none() && choice.real.is_none() && !choice.deceptive;
        if quiet && self.attempt.is_some() {
            return;
        }
        self.since.clone_from(&self.prev);
        if self.attempt.is_some_and(|a| a.fired) {
            return;
        }
        if let Some(bait) = &mut self.attempt
            && bait.deceptive
            && bait.real.is_none()
            && choice.real.is_some()
            && choice.real != bait.shown
        {
            bait.real = choice.real;
            return;
        }
        self.end_attempt();
        if choice.shown.is_some() || choice.deceptive {
            self.attempt = Some(Attempt {
                shown: choice.shown,
                real: choice.real,
                deceptive: choice.deceptive,
                covered: None,
                won: false,
                fired: false,
                premature: self.away > 0 || self.possession.is_none(),
            });
        }
    }

    pub(crate) fn close(&mut self) {
        self.end_attempt();
    }

    fn end_attempt(&mut self) {
        if let (Some(a), Some(model)) = (self.attempt.take(), &mut self.model)
            && !a.premature
        {
            let fooled = a.real.is_some() && a.covered.is_some() && a.covered != a.real;
            let won = a.won || (a.deceptive && fooled);
            model.record_attempt(a.shown, a.real, a.covered, a.deceptive, won);
        }
    }

    pub(crate) fn odds(&self, depth: u8, signal: Option<u8>) -> Option<Vec<(Stance, f64)>> {
        let odds = self.model.as_ref()?.their_odds(signal?)?;
        (depth >= 2).then(|| {
            odds.into_iter()
                .map(|(real, p)| (real.map_or(Stance::Still, Stance::Shoot), p))
                .collect()
        })
    }

    pub(crate) fn stance(&self, depth: u8, choice: &Choice) -> Stance {
        if depth == 0 {
            return Stance::Still;
        }
        let Some(model) = &self.model else {
            return Stance::Naive;
        };
        let habit = model.likely_shot().map_or(Stance::Naive, Stance::Shoot);
        let read_me = choice.shown.and_then(|s| model.likely_cover(s));
        let know_me = choice.shown.and_then(|s| model.likely_real(s));
        match depth {
            1 => habit,
            2 => read_me.map_or(habit, Stance::Cover),
            _ if model.knowing_beats_reading() => know_me.or(read_me).map_or(habit, Stance::Cover),
            _ => read_me.or(know_me).map_or(habit, Stance::Cover),
        }
    }

    pub(crate) fn model(&self) -> Option<&OpponentModel> {
        self.model.as_ref()
    }

    pub(crate) fn profile(&self, game_id: &str) -> Option<Profile> {
        self.model.as_ref().map(|m| m.export(game_id))
    }

    pub(crate) fn load_profile(
        &mut self,
        profile: &Profile,
        game_id: &str,
    ) -> Result<(), ProfileError> {
        self.model
            .as_mut()
            .ok_or(ProfileError::NoMemory)?
            .import(profile, game_id)
    }
}
