use feint_protocol::ObservationT;

#[must_use]
pub fn delay_ticks(reaction_ms: u32, tick_hz: u32) -> usize {
    let product = u64::from(reaction_ms) * u64::from(tick_hz);
    usize::try_from((product + 500) / 1000).unwrap_or(usize::MAX)
}

#[derive(Clone, Debug)]
pub struct ReactionBuffer {
    slots: Vec<ObservationT>,
    delay: usize,
    pushed: u64,
}

impl ReactionBuffer {
    #[must_use]
    pub fn new(delay: usize) -> Self {
        Self {
            slots: Vec::new(),
            delay,
            pushed: 0,
        }
    }

    pub fn push(&mut self, obs: &ObservationT) {
        let capacity = self.delay.saturating_add(1);
        let slot = (self.pushed % capacity as u64) as usize;
        if slot < self.slots.len() {
            self.slots[slot].clone_from(obs);
        } else {
            self.slots.push(obs.clone());
        }
        self.pushed += 1;
    }

    #[must_use]
    pub fn delayed(&self, now: u64) -> Option<&ObservationT> {
        let cutoff = now.checked_sub(self.delay as u64)?;
        self.recent()
            .filter(|obs| obs.tick <= cutoff)
            .max_by_key(|obs| obs.tick)
    }

    fn recent(&self) -> impl Iterator<Item = &ObservationT> {
        let held = self.pushed.min(self.slots.len() as u64);
        (1..=held).map(move |back| self.slot(self.pushed - back))
    }

    #[must_use]
    pub fn newest(&self) -> Option<&ObservationT> {
        (self.pushed > 0).then(|| self.slot(self.pushed - 1))
    }

    fn slot(&self, index: u64) -> &ObservationT {
        &self.slots[(index % self.slots.len() as u64) as usize]
    }

    pub fn clear(&mut self) {
        self.pushed = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    tick_hz: u32,
    decision_hz: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadCadence;

impl Cadence {
    pub fn new(tick_hz: u32, decision_hz: u32) -> Result<Self, BadCadence> {
        if decision_hz == 0 || decision_hz > tick_hz {
            return Err(BadCadence);
        }
        Ok(Self {
            tick_hz,
            decision_hz,
        })
    }

    #[must_use]
    pub fn is_decision(&self, tick: u64) -> bool {
        let (d, t) = (u128::from(self.decision_hz), u128::from(self.tick_hz));
        u128::from(tick) * d % t < d
    }
}
