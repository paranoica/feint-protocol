use feint_protocol::{GameAdapter, PlayerId};

use crate::difficulty::{Difficulty, Memory};
use crate::model::{OpponentModel, Profile, ProfileError};
use crate::playbook::{Choice, Playbook, Stance};

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
    pub(crate) gate_open: bool,
    pub(crate) fake_pending: bool,
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
            gate_open: false,
            fake_pending: false,
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
        self.gate_open = false;
        self.fake_pending = false;
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
        let mut started = possession.is_some() && possession != self.possession;
        let mut rally = false;
        if let Some(prev) = &self.prev {
            rally = score
                .iter()
                .zip(&self.prev_score)
                .any(|(now, before)| now > before);
            let read = playbook.read(prev, state, them);
            if let (Some(model), Some(shot)) = (&mut self.model, read.shot) {
                model.record_shot(shot);
            }
            if let (Some(attempt), Some(covered)) = (&mut self.attempt, read.covered) {
                attempt.covered = Some(covered);
            }
            let me_i = usize::from(me);
            let goals = |s: &[i64], i: usize| s.get(i).copied().unwrap_or(0);
            if goals(score, me_i) > goals(&self.prev_score, me_i) {
                if let Some(attempt) = &mut self.attempt {
                    attempt.won = true;
                }
                self.end_attempt();
            }
        } else {
            started = possession.is_some();
        }
        if started {
            self.end_attempt();
        }
        self.prev = Some(state.clone());
        self.prev_score.clear();
        self.prev_score.extend_from_slice(score);
        self.possession = possession;
        Seen { started, rally }
    }

    pub(crate) fn start_attempt(&mut self, choice: &Choice) {
        self.end_attempt();
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
            let won = a.won || (a.deceptive && a.covered.is_some() && a.covered != a.real);
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
