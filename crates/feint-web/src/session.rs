use airhockey::{AirHockey, GAME_VERSION, HockeyPlaybook, Params};
use feint_ai::{
    Bot, Controller, Difficulty, InsightView, Memory, Profile, RunHeader, RunReport, Runner,
    encode_record,
};
use feint_protocol::{
    ActionT, BodyParamsT, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId,
};

pub const GAME_ID: &str = "airhockey";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    Level,
    Profile,
}

pub struct Pointer {
    target: [f32; 2],
    sent: Option<[f32; 2]>,
    bounds: [f32; 2],
    me: PlayerId,
}

impl Pointer {
    #[must_use]
    pub fn new() -> Self {
        let p = Params::default();
        Self {
            target: [0.0, 0.0],
            sent: None,
            bounds: [p.half_length as f32, p.half_width as f32],
            me: 0,
        }
    }

    pub fn aim(&mut self, x: f32, y: f32) {
        let clamp = |v: f32, limit: f32| {
            if v.is_finite() {
                v.clamp(-limit, limit)
            } else {
                0.0
            }
        };
        self.target = [clamp(x, self.bounds[0]), clamp(y, self.bounds[1])];
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller<AirHockey> for Pointer {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.sent = None;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        if self.sent == Some(self.target) {
            return None;
        }
        self.sent = Some(self.target);
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = self.me;
        action.values = Some(self.target.to_vec());
        Some(action)
    }
}

pub struct Session {
    game: AirHockey,
    bot: Bot<AirHockey, HockeyPlaybook>,
    left: Option<Bot<AirHockey, HockeyPlaybook>>,
    human: Pointer,
    runner: Option<Runner>,
    report: Option<RunReport>,
    events: Vec<EventT>,
}

fn body(values: [f64; 2]) -> BodyParamsT {
    let mut b = BodyParamsT::default();
    b.values = Some(values.map(|v| v as f32).to_vec());
    b
}

pub struct Replayer {
    actions: Vec<ActionT>,
    next: usize,
    me: PlayerId,
}

impl Replayer {
    #[must_use]
    pub fn new(actions: Vec<ActionT>) -> Self {
        Self {
            actions,
            next: 0,
            me: 0,
        }
    }
}

impl Controller<AirHockey> for Replayer {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.next = 0;
        self.me = me;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        while self
            .actions
            .get(self.next)
            .is_some_and(|a| a.tick < obs.tick)
        {
            self.next += 1;
        }
        let action = self.actions.get(self.next).filter(|a| a.tick == obs.tick)?;
        let mut action = action.clone();
        action.player = self.me;
        self.next += 1;
        Some(action)
    }
}

impl Session {
    pub fn new(level: u8, seed: u64) -> Result<Self, SessionError> {
        Self::with_profile(level, seed, None)
    }

    pub fn with_profile(
        level: u8,
        seed: u64,
        profile: Option<&Profile>,
    ) -> Result<Self, SessionError> {
        let difficulty = Difficulty::preset(level).ok_or(SessionError::Level)?;
        if profile.is_some() && difficulty.memory != Memory::MatchAndProfile {
            return Err(SessionError::Profile);
        }
        let params = Params::default();
        let mut game = AirHockey::new(params.clone(), level);
        let header = Self::header(level, seed, &game).ok_or(SessionError::Level)?;
        let mut bot = Bot::new(difficulty, HockeyPlaybook::new(params), &game.spec())
            .map_err(|_| SessionError::Level)?;
        if let Some(profile) = profile {
            bot.load_profile(profile, GAME_ID)
                .map_err(|_| SessionError::Profile)?;
        }
        let mut human = Pointer::new();
        let runner = Runner::start(&mut game, [&mut human, &mut bot], header)
            .map_err(|_| SessionError::Level)?;
        Ok(Self {
            game,
            bot,
            left: None,
            human,
            runner: Some(runner),
            report: None,
            events: Vec::new(),
        })
    }

    pub fn spectate(left: u8, right: u8, seed: u64) -> Result<Self, SessionError> {
        let params = Params::default();
        let mut game = AirHockey::new(params.clone(), right);
        let spec = game.spec();
        let make = |level: u8| {
            let difficulty = Difficulty::preset(level).ok_or(SessionError::Level)?;
            let bot = Bot::new(difficulty, HockeyPlaybook::new(params.clone()), &spec)
                .map_err(|_| SessionError::Level)?;
            Ok::<_, SessionError>((bot, difficulty.body(&spec)))
        };
        let (mut left_bot, left_body) = make(left)?;
        let (mut bot, right_body) = make(right)?;
        let header = RunHeader {
            game_id: GAME_ID.into(),
            game_version: GAME_VERSION,
            seed,
            bodies: vec![left_body, right_body],
            levels: [left, right],
        };
        let runner = Runner::start(&mut game, [&mut left_bot, &mut bot], header)
            .map_err(|_| SessionError::Level)?;
        Ok(Self {
            game,
            bot,
            left: Some(left_bot),
            human: Pointer::new(),
            runner: Some(runner),
            report: None,
            events: Vec::new(),
        })
    }

    #[must_use]
    pub fn spectating(&self) -> bool {
        self.left.is_some()
    }

    #[must_use]
    pub fn insight_of(&self, seat: PlayerId) -> InsightView {
        match (seat, &self.left) {
            (0, Some(left)) => left.insight(),
            _ => self.bot.insight(),
        }
    }

    #[must_use]
    pub fn profile(&self) -> Option<Profile> {
        self.bot.profile(GAME_ID)
    }

    #[must_use]
    pub fn header(level: u8, seed: u64, game: &AirHockey) -> Option<RunHeader> {
        let difficulty = Difficulty::preset(level)?;
        let spec = game.spec();
        let params = Params::default();
        Some(RunHeader {
            game_id: GAME_ID.into(),
            game_version: GAME_VERSION,
            seed,
            bodies: vec![
                body([params.max_body_speed, params.max_body_accel]),
                difficulty.body(&spec),
            ],
            levels: [0, level],
        })
    }

    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.human.aim(x, y);
    }

    pub fn advance(&mut self, ticks: u32) -> &[EventT] {
        self.events.clear();
        for _ in 0..ticks {
            let Some(runner) = self.runner.as_mut() else {
                break;
            };
            let first: &mut dyn Controller<AirHockey> = match self.left.as_mut() {
                Some(left) => left,
                None => &mut self.human,
            };
            if !runner.step(&mut self.game, [first, &mut self.bot]) {
                break;
            }
            self.events.extend_from_slice(runner.events());
        }
        if self.game.result().is_some()
            && let Some(runner) = self.runner.take()
        {
            let first: &mut dyn Controller<AirHockey> = match self.left.as_mut() {
                Some(left) => left,
                None => &mut self.human,
            };
            self.report = Some(runner.finish(&mut self.game, [first, &mut self.bot]));
        }
        &self.events
    }

    #[must_use]
    pub fn positions(&self) -> [f32; 12] {
        let world = &self.game.live().world;
        let [a, b] = world.mallets;
        [
            world.puck.pos.x,
            world.puck.pos.y,
            world.puck.vel.x,
            world.puck.vel.y,
            a.pos.x,
            a.pos.y,
            a.vel.x,
            a.vel.y,
            b.pos.x,
            b.pos.y,
            b.vel.x,
            b.vel.y,
        ]
        .map(|v| v as f32)
    }

    #[must_use]
    pub fn serve_pause(&self) -> f32 {
        let live = self.game.live();
        (f64::from(live.pause_ticks()) / f64::from(live.params.tick_hz)) as f32
    }

    #[must_use]
    pub fn score(&self) -> [u32; 2] {
        self.game.live().score()
    }

    #[must_use]
    pub fn tick(&self) -> u64 {
        self.game.live().tick()
    }

    #[must_use]
    pub fn finished(&self) -> bool {
        self.report.is_some()
    }

    #[must_use]
    pub fn insight(&self) -> InsightView {
        self.bot.insight()
    }

    #[must_use]
    pub fn playbook(&self) -> &HockeyPlaybook {
        self.bot.playbook()
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        self.game.digest(self.game.live())
    }

    #[must_use]
    pub fn record(&self) -> Vec<u8> {
        let report = match (&self.report, &self.runner) {
            (Some(report), _) => report.clone(),
            (None, Some(runner)) => runner.report(&self.game),
            (None, None) => return Vec::new(),
        };
        encode_record(&report.record())
    }
}
