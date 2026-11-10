use std::collections::VecDeque;

use feint_protocol::{ActionT, GameAdapter, GameSpecT, PlayerId};

use crate::playbook::{Playbook, Stance};

#[derive(Clone, Debug, Default)]
pub struct Anticipation {
    history: VecDeque<ActionT>,
    window: u64,
}

impl Anticipation {
    #[must_use]
    pub fn new(window: u64) -> Self {
        Self {
            history: VecDeque::new(),
            window,
        }
    }

    #[must_use]
    pub fn window(&self) -> u64 {
        self.window
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.history.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    #[must_use]
    pub fn last(&self) -> Option<&ActionT> {
        self.history.back()
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }

    pub fn remember(&mut self, action: &ActionT, now: u64) {
        let cutoff = now.saturating_sub(self.window);
        while self.history.len() > 1 && self.history[1].tick <= cutoff {
            self.history.pop_front();
        }
        if self.history.back().is_some_and(|last| last.tick == now) {
            self.history.pop_back();
        }
        self.history.push_back(action.clone());
    }

    pub fn roll_forward<A: GameAdapter, P: Playbook<A>>(
        &self,
        adapter: &A,
        playbook: &P,
        them: PlayerId,
        state: A::State,
        from: u64,
        to: u64,
    ) -> A::State {
        let step = |state: &A::State, own: Option<&ActionT>, ticks: u64| {
            let reply = playbook.opponent_move(state, them, Stance::Naive);
            let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
            match own {
                Some(own) => adapter.simulate(state, &[own.clone(), reply], ticks),
                None => adapter.simulate(state, std::slice::from_ref(&reply), ticks),
            }
        };
        let to = to.min(from.saturating_add(self.window));
        let mut state = state;
        let mut at = from;
        let mut own = self.history.iter().rev().find(|a| a.tick <= from);
        for action in self
            .history
            .iter()
            .filter(|a| a.tick > from && a.tick <= to)
        {
            if action.tick > at {
                state = step(&state, own, action.tick - at);
                at = action.tick;
            }
            own = Some(action);
        }
        if to > at {
            state = step(&state, own, to - at);
        }
        state
    }
}

pub fn clamp_to_space(values: &mut [f32], spec: &GameSpecT) {
    let space = spec.action_space.as_deref();
    if let Some(dims) = space.and_then(|s| s.continuous.as_ref()) {
        for (value, dim) in values.iter_mut().zip(dims) {
            *value = value.max(dim.min).min(dim.max);
        }
    }
}
