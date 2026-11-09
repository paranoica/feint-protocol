#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub half_length: f64,
    pub half_width: f64,
    pub mouth_half_width: f64,
    pub puck_radius: f64,
    pub mallet_radius: f64,
    pub rail_restitution: f64,
    pub post_restitution: f64,
    pub mallet_restitution: f64,
    pub damping: f64,
    pub puck_speed_cap: f64,
    pub tick_hz: u32,
    pub substeps: u32,
    pub decision_hz: u32,
    pub goals_to_win: u32,
    pub stall_seconds: f64,
    pub max_body_speed: f64,
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
    #[must_use]
    pub fn tick_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz)
    }

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

    pub fn body(&self, max_speed: f64, max_accel: f64) -> Result<Body, BadBody> {
        if max_speed <= self.max_body_speed && max_accel <= self.max_body_accel {
            Body::new(max_speed, max_accel)
        } else {
            Err(BadBody)
        }
    }

    #[must_use]
    pub fn substep_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz * self.substeps)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    max_speed: f64,
    max_accel: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadBody;

impl Body {
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
