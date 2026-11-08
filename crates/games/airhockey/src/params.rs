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
        }
    }
}

impl Params {
    /// Length of one physics tick in seconds.
    #[must_use]
    pub fn tick_dt(&self) -> f64 {
        1.0 / f64::from(self.tick_hz)
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
