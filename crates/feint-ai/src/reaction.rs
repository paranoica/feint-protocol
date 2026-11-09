use feint_protocol::ObservationT;

/// Ticks of delay for a reaction time: `round(reaction_ms · tick_hz / 1000)`.
#[must_use]
pub fn delay_ticks(reaction_ms: u32, tick_hz: u32) -> usize {
    let product = u64::from(reaction_ms) * u64::from(tick_hz);
    // At most (2³² − 1)², so adding 500 cannot overflow.
    usize::try_from((product + 500) / 1000).unwrap_or(usize::MAX)
}

/// The last `delay + 1` observations; the bot sees only the oldest (FNT-D9).
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

    /// Store this tick's observation; its vectors reuse the slot's capacity.
    pub fn push(&mut self, obs: &ObservationT) {
        let capacity = self.delay + 1;
        let slot = (self.pushed % capacity as u64) as usize;
        if slot < self.slots.len() {
            self.slots[slot].clone_from(obs);
        } else {
            self.slots.push(obs.clone());
        }
        self.pushed += 1;
    }

    /// The observation from `delay` pushes ago; none until that many exist.
    #[must_use]
    pub fn delayed(&self) -> Option<&ObservationT> {
        let back = self.delay as u64;
        (self.pushed > back).then(|| self.slot(self.pushed - 1 - back))
    }

    /// The observation pushed last.
    #[must_use]
    pub fn newest(&self) -> Option<&ObservationT> {
        (self.pushed > 0).then(|| self.slot(self.pushed - 1))
    }

    fn slot(&self, index: u64) -> &ObservationT {
        &self.slots[(index % (self.delay as u64 + 1)) as usize]
    }

    /// Forget everything, for a new match.
    pub fn clear(&mut self) {
        self.pushed = 0;
    }
}

/// Which ticks are decision ticks when deciding `decision_hz` times per `tick_hz` ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cadence {
    tick_hz: u32,
    decision_hz: u32,
}

/// A decision rate that cannot be kept: zero, or above the tick rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadCadence;

impl Cadence {
    /// # Errors
    /// [`BadCadence`] unless `0 < decision_hz <= tick_hz`.
    pub fn new(tick_hz: u32, decision_hz: u32) -> Result<Self, BadCadence> {
        if decision_hz == 0 || decision_hz > tick_hz {
            return Err(BadCadence);
        }
        Ok(Self {
            tick_hz,
            decision_hz,
        })
    }

    /// `(tick · decision_hz) % tick_hz < decision_hz`: evenly spread, tick 0 included.
    #[must_use]
    pub fn is_decision(&self, tick: u64) -> bool {
        let (d, t) = (u128::from(self.decision_hz), u128::from(self.tick_hz));
        u128::from(tick) * d % t < d
    }
}
