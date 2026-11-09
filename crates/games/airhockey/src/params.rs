/// Table geometry and physics constants; starting values, tuned by playtest (FNT-D5).
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// Half the playing field's length; x runs over `[-half_length, half_length]`.
    pub half_length: f64,
    /// Half the field's width; y runs over `[-half_width, half_width]`.
    pub half_width: f64,
    /// Half the goal mouth's width.
    pub mouth_half_width: f64,
    pub puck_radius: f64,
    pub mallet_radius: f64,
    pub rail_restitution: f64,
    pub post_restitution: f64,
    pub mallet_restitution: f64,
    /// Linear puck damping per second.
    pub damping: f64,
    pub puck_speed_cap: f64,
    pub tick_hz: u32,
    pub substeps: u32,
    pub decision_hz: u32,
    pub goals_to_win: u32,
    /// Seconds the puck may stay in one half before it goes to the other player.
    pub stall_seconds: f64,
    /// The fastest mallet a body may have: the peak of a human hand.
    pub max_body_speed: f64,
    /// The hardest acceleration a body may have.
    pub max_body_accel: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            half_length: 1.0,
            half_width: 0.5,
            mouth_half_width: 0.125,
            puck_radius: 0.04,
            mallet_radius: 0.05,
            rail_restitution: 0.9,
            post_restitution: 0.9,
            mallet_restitution: 0.8,
            damping: 0.1,
            puck_speed_cap: 20.0,
            tick_hz: 240,
            substeps: 4,
            decision_hz: 60,
            goals_to_win: 7,
            stall_seconds: 7.0,
            max_body_speed: 12.0,
            max_body_accel: 200.0,
        }
    }
}

impl Params {
    /// Length of one physics tick in seconds.
    #[must_use]
    pub fn tick_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz)
    }

    /// # Errors
    /// What is wrong with these parameters.
    pub fn validate(&self) -> Result<(), &'static str> {
        let positive = |x: f64| x.is_finite() && x > 0.0;
        let sizes = [
            self.half_length,
            self.half_width,
            self.mouth_half_width,
            self.puck_radius,
            self.mallet_radius,
            self.puck_speed_cap,
            self.stall_seconds,
            self.max_body_speed,
            self.max_body_accel,
        ];
        if !sizes.into_iter().all(positive) {
            return Err("sizes, speeds and times must be positive and finite");
        }
        if self.half_width <= self.mallet_radius.max(self.puck_radius) {
            return Err("the table is narrower than a mallet or the puck");
        }
        if self.half_length <= 2.0 * self.mallet_radius {
            return Err("each half is shorter than a mallet");
        }
        if self.mouth_half_width >= self.half_width {
            return Err("the goal mouth is wider than the table");
        }
        let restitutions = [
            self.rail_restitution,
            self.post_restitution,
            self.mallet_restitution,
        ];
        if !restitutions.iter().all(|e| (0.0..=1.0).contains(e)) {
            return Err("restitution must be within [0, 1]");
        }
        if self.tick_hz == 0 || self.substeps == 0 || self.decision_hz == 0 {
            return Err("tick, substep and decision rates must be positive");
        }
        if self.tick_hz.checked_mul(self.substeps).is_none() {
            return Err("tick_hz × substeps overflows");
        }
        if self.decision_hz > self.tick_hz {
            return Err("decisions cannot be more frequent than ticks");
        }
        if self.goals_to_win == 0 {
            return Err("a match needs at least one goal to win");
        }
        if !(self.damping >= 0.0 && self.damping * self.tick_dt() < 1.0) {
            return Err("damping must be non-negative and below one per tick");
        }
        // The determinism argument of FNT-D4: per substep the puck moves less than its
        // radius, and puck and mallet together less than their combined reach.
        let dt = self.substep_dt();
        if self.puck_speed_cap * dt >= self.puck_radius {
            return Err("a substep moves the puck farther than its radius");
        }
        if (self.puck_speed_cap + self.max_body_speed) * dt >= self.puck_radius + self.mallet_radius
        {
            return Err("a substep can carry the puck through a mallet");
        }
        Ok(())
    }

    /// A body within the declared limits.
    ///
    /// # Errors
    /// [`BadBody`] unless both limits are positive and within the declared maximums.
    pub fn body(&self, max_speed: f64, max_accel: f64) -> Result<Body, BadBody> {
        if max_speed <= self.max_body_speed && max_accel <= self.max_body_accel {
            Body::new(max_speed, max_accel)
        } else {
            Err(BadBody)
        }
    }

    /// Length of one substep in seconds.
    #[must_use]
    pub fn substep_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz * self.substeps)
    }
}

/// A player's motor limits: how fast the mallet may move and speed up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    max_speed: f64,
    max_accel: f64,
}

/// Body limits that are not positive finite numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadBody;

impl Body {
    /// # Errors
    /// [`BadBody`] unless both limits are positive and finite.
    pub fn new(max_speed: f64, max_accel: f64) -> Result<Self, BadBody> {
        let valid = |x: f64| x.is_finite() && x > 0.0;
        if valid(max_speed) && valid(max_accel) {
            Ok(Self {
                max_speed,
                max_accel,
            })
        } else {
            Err(BadBody)
        }
    }

    #[must_use]
    pub fn max_speed(&self) -> f64 {
        self.max_speed
    }

    #[must_use]
    pub fn max_accel(&self) -> f64 {
        self.max_accel
    }
}

impl Default for Body {
    fn default() -> Self {
        Self {
            max_speed: 8.0,
            max_accel: 60.0,
        }
    }
}
