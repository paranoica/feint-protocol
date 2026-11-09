use feint_protocol::{GameAdapter, PlayerId};

use crate::difficulty::{Difficulty, Memory};
use crate::model::{OpponentModel, Profile, ProfileError};
use crate::playbook::{Choice, Playbook, Stance};

const MIN_AWAY_SAMPLES: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Attempt {
    shown: Option<u8>,
    real: Option<u8>,
    deceptive: bool,
    covered: Option<u8>,
    won: bool,
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
        let returned = self.away >= MIN_AWAY_SAMPLES;
        let mut started = possession.is_some() && (possession != self.possession || returned);
        let mut rally = false;
        if let Some(prev) = &self.prev {
            rally = score
                .iter()
                .zip(&self.prev_score)
                .any(|(now, before)| now > before);
            if let (Some(model), Some(shot)) =
                (&mut self.model, playbook.read(prev, state, them).shot)
            {
                model.record_shot(shot);
            }
            let reference = self.since.as_ref().unwrap_or(prev);
            if let Some(covered) = playbook.read(reference, state, them).covered {
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
        }
        if possession.is_some() {
            self.away = 0;
            self.possession = possession;
        } else {
            self.away = self.away.saturating_add(1);
            self.end_attempt();
        }
        self.prev = Some(state.clone());
        self.prev_score.clear();
        self.prev_score.extend_from_slice(score);
        Seen { started, rally }
    }

    pub(crate) fn start_attempt(&mut self, choice: &Choice) {
        self.end_attempt();
        self.since.clone_from(&self.prev);
        if choice.shown.is_some() || choice.deceptive {
            self.attempt = Some(Attempt {
                shown: choice.shown,
                real: choice.real,
                deceptive: choice.deceptive,
                covered: None,
                won: false,
            });
        }
    }

    pub(crate) fn close(&mut self) {
        self.end_attempt();
    }

    fn end_attempt(&mut self) {
        if let (Some(a), Some(model)) = (self.attempt.take(), &mut self.model) {
            let fooled = a.real.is_some() && a.covered.is_some() && a.covered != a.real;
            let won = a.won || (a.deceptive && fooled);
            model.record_attempt(a.shown, a.real, a.covered, a.deceptive, won);
        }
    }

    pub(crate) fn stance(&self, depth: u8, choice: &Choice) -> Stance {
        let Some(model) = &self.model else {
            return Stance::Naive;
        };
        if depth == 0 {
            return Stance::Naive;
        }
        let habit = model.likely_shot().map_or(Stance::Naive, Stance::Shoot);
        let read_me = choice.shown.and_then(|s| model.likely_cover(s));
        let know_me = choice.shown.and_then(|s| model.likely_real(s));
        match depth {
            1 => habit,
            2 => read_me.map_or(habit, Stance::Cover),
            _ => know_me.or(read_me).map_or(habit, Stance::Cover),
        }
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
