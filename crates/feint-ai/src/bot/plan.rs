use feint_protocol::{ActionT, GameAdapter};

use super::{Bot, SIMULATION_BUDGET};

const STYLE_MARGINS: f64 = 6.0;
use crate::playbook::{Choice, Cue, Playbook, Stance};

impl<A: GameAdapter, P: Playbook<A>> Bot<A, P> {
    pub(super) fn step(
        &self,
        adapter: &A,
        state: &A::State,
        own: Option<&ActionT>,
        ticks: u64,
    ) -> A::State {
        let reply = self
            .playbook
            .opponent_move(state, self.opponent(), Stance::Naive);
        let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
        match own {
            Some(own) => adapter.simulate(state, &[own.clone(), reply], ticks),
            None => adapter.simulate(state, std::slice::from_ref(&reply), ticks),
        }
    }

    pub(super) fn roll_forward(
        &self,
        adapter: &A,
        state: A::State,
        from: u64,
        to: u64,
    ) -> A::State {
        let to = to.min(from.saturating_add(self.delay as u64));
        let mut state = state;
        let mut at = from;
        let mut own = self.history.iter().rev().find(|a| a.tick <= from);
        for action in self
            .history
            .iter()
            .filter(|a| a.tick > from && a.tick <= to)
        {
            if action.tick > at {
                state = self.step(adapter, &state, own, action.tick - at);
                at = action.tick;
            }
            own = Some(action);
        }
        if to > at {
            state = self.step(adapter, &state, own, to - at);
        }
        state
    }

    fn rollout(
        &self,
        adapter: &A,
        state: &A::State,
        choice: &Choice,
        stance: Stance,
        cap: u32,
        elapsed: u64,
    ) -> f64 {
        let horizon = choice.horizon_ticks.clamp(1, cap);
        let mut state = state.clone();
        let mut theirs = self.playbook.opponent_move(&state, self.opponent(), stance);
        let mut next_reply = self.reply_every;
        let mut done = 0;
        while done < horizon {
            if done >= next_reply {
                theirs = self.playbook.opponent_move(&state, self.opponent(), stance);
                next_reply = next_reply.saturating_add(self.reply_every);
            }
            let ticks = self.stride.min(horizon - done).min(next_reply - done);
            let mine = self.playbook.act(
                &state,
                self.me,
                choice,
                elapsed.saturating_add(u64::from(done)),
                [0.0; 2],
            );
            state = adapter.simulate(&state, &[mine, theirs.clone()], ticks);
            done += ticks;
        }
        self.playbook.evaluate(&state, self.me)
    }

    pub(super) fn replan(&mut self, adapter: &A, state: &A::State, now: u64) {
        let mut options = std::mem::take(&mut self.options);
        options.clear();
        let cue = Cue {
            pace: self.plan.pace(),
            twitch: self.insight.twitch,
            held: self.current.map(|c| c.choice.id),
        };
        self.playbook.options(
            state,
            self.me,
            self.difficulty.repertoire,
            &cue,
            &mut options,
        );
        let importance = self.playbook.importance(state, self.me);
        let signal = self.playbook.signal(state, self.opponent());
        let current = self.current;
        let depth = self.difficulty.depth;
        let held = current.map(|c| c.choice.id);
        if self.insight.fake_pending && depth < 2 {
            let fakes: Vec<Choice> = options.iter().filter(|c| c.deceptive).copied().collect();
            if !fakes.is_empty() {
                self.insight.fake_pending = false;
                let pick = ((self.unit() * fakes.len() as f64) as usize).min(fakes.len() - 1);
                self.commit(fakes[pick], now, importance, signal);
                self.options = options;
                return;
            }
        }
        let gate_open = self.insight.gate_open && depth >= 2;
        let plan = &self.plan;
        options.retain(|c| {
            !c.deceptive || gate_open || held == Some(c.id) || plan.allows(c, importance)
        });
        let offered = |id: u16| options.iter().any(|c| c.id == id);
        if let Some(cur) = current
            && offered(cur.choice.id)
            && cur.signal == signal
            && now.saturating_sub(cur.since) < u64::from(cur.choice.horizon_ticks / 2)
        {
            self.options = options;
            return;
        }
        if options.is_empty() {
            self.current = None;
            self.options = options;
            return;
        }
        let odds = self.insight.odds(depth, signal);
        let outcomes = odds.as_ref().map_or(1, Vec::len).max(1);
        let rollouts = options.len().saturating_mul(outcomes);
        let cap = (SIMULATION_BUDGET / u32::try_from(rollouts).unwrap_or(u32::MAX)).max(1);
        let mut scored: Vec<(Choice, f64)> = Vec::with_capacity(options.len());
        for choice in &options {
            let elapsed = current
                .filter(|c| c.choice.id == choice.id)
                .map_or(0, |c| now.saturating_sub(c.since));
            let score = match &odds {
                Some(odds) => odds
                    .iter()
                    .map(|&(stance, p)| {
                        p * self.rollout(adapter, state, choice, stance, cap, elapsed)
                    })
                    .sum(),
                None => {
                    let stance = self.insight.stance(depth, choice);
                    self.rollout(adapter, state, choice, stance, cap, elapsed)
                }
            };
            let read = choice.shown.zip(choice.real).is_some_and(|(shown, real)| {
                self.insight.model().is_some_and(|m| {
                    m.likely_cover(shown) == Some(real) || m.likely_real(shown) == Some(real)
                })
            });
            let style = if choice.deceptive && choice.real.is_some() && gate_open && !read {
                STYLE_MARGINS * self.playbook.margin() * self.difficulty.deception
            } else {
                0.0
            };
            scored.push((*choice, score + style + self.plan.bonus(choice, importance)));
        }
        self.options = options;
        let best = scored
            .iter()
            .map(|&(_, s)| s)
            .fold(f64::NEG_INFINITY, f64::max);
        let margin = self.playbook.margin();
        let current_score =
            current.and_then(|cur| scored.iter().find(|(c, _)| c.id == cur.choice.id));
        if let (Some(cur), Some(&(offer, cur_score))) = (current, current_score)
            && cur_score >= best - margin
        {
            let attempt = u64::from(cur.choice.horizon_ticks.max(1));
            if now.saturating_sub(cur.since) >= attempt {
                self.commit(offer, now, importance, signal);
            } else if let Some(held) = &mut self.current {
                held.signal = signal;
            }
            return;
        }
        let pick = if depth == 0 {
            scored
                .iter()
                .filter(|&&(_, s)| s == best)
                .min_by_key(|(c, _)| c.id)
                .map(|&(c, _)| c)
        } else {
            let near: Vec<Choice> = scored
                .iter()
                .filter(|&&(_, s)| s >= best - margin)
                .map(|&(c, _)| c)
                .collect();
            let i = ((self.unit() * near.len() as f64) as usize).min(near.len().saturating_sub(1));
            near.get(i).copied()
        };
        if let Some(choice) = pick {
            self.commit(choice, now, importance, signal);
        }
    }
}
