use airhockey::{AirHockey, Params};
use feint_ai::{
    Bot, Controller, Difficulty, InsightView, Memory, Profile, RunHeader, RunReport, Runner,
    encode_record,
};
use feint_protocol::{ActionT, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::hub::{HubGame, HumanInput};

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

pub struct Play<G: HubGame> {
    game: G,
    bot: Bot<G, G::Book>,
    left: Option<Bot<G, G::Book>>,
    human: G::Human,
    runner: Option<Runner>,
    report: Option<RunReport>,
    events: Vec<EventT>,
}

pub type Session = Play<AirHockey>;

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

impl<A: GameAdapter> Controller<A> for Replayer {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.next = 0;
        self.me = me;
    }

    fn on_tick(&mut self, _game: &A, obs: &ObservationT) -> Option<ActionT> {
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

impl<G: HubGame> Play<G> {
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
        let mut game = G::build(level);
        let header = Self::header(level, seed, &game).ok_or(SessionError::Level)?;
        let mut bot =
            Bot::new(difficulty, G::book(), &game.spec()).map_err(|_| SessionError::Level)?;
        if let Some(profile) = profile {
            bot.load_profile(profile, G::ID)
                .map_err(|_| SessionError::Profile)?;
        }
        let mut human = G::Human::default();
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
        let mut game = G::build(right);
        let spec = game.spec();
        let make = |level: u8| {
            let difficulty = Difficulty::preset(level).ok_or(SessionError::Level)?;
            let bot = Bot::new(difficulty, G::book(), &spec).map_err(|_| SessionError::Level)?;
            Ok::<_, SessionError>((bot, difficulty.body(&spec)))
        };
        let (mut left_bot, left_body) = make(left)?;
        let (mut bot, right_body) = make(right)?;
        let header = RunHeader {
            game_id: G::ID.into(),
            game_version: G::VERSION,
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
            human: G::Human::default(),
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
        self.bot.profile(G::ID)
    }

    #[must_use]
    pub fn header(level: u8, seed: u64, game: &G) -> Option<RunHeader> {
        let difficulty = Difficulty::preset(level)?;
        Some(RunHeader {
            game_id: G::ID.into(),
            game_version: G::VERSION,
            seed,
            bodies: vec![G::human_body(), difficulty.body(&game.spec())],
            levels: [0, level],
        })
    }

    pub fn set_input(&mut self, values: &[f32]) {
        self.human.set(values);
    }

    pub fn push_input(&mut self, values: &[f32]) {
        self.human.push(values);
    }

    pub fn advance(&mut self, ticks: u32) -> &[EventT] {
        self.events.clear();
        for _ in 0..ticks {
            let Some(runner) = self.runner.as_mut() else {
                break;
            };
            let first: &mut dyn Controller<G> = match self.left.as_mut() {
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
            let first: &mut dyn Controller<G> = match self.left.as_mut() {
                Some(left) => left,
                None => &mut self.human,
            };
            self.report = Some(runner.finish(&mut self.game, [first, &mut self.bot]));
        }
        &self.events
    }

    #[must_use]
    pub fn view(&self) -> Vec<f32> {
        self.game.view()
    }

    #[must_use]
    pub fn layout(&self) -> Vec<f32> {
        self.game.layout()
    }

    #[must_use]
    pub fn serve_pause(&self) -> f32 {
        self.game.pause()
    }

    #[must_use]
    pub fn score(&self) -> [u32; 2] {
        self.game.score()
    }

    #[must_use]
    pub fn tick(&self) -> u64 {
        self.game.tick()
    }

    #[must_use]
    pub fn tick_hz(&self) -> u32 {
        self.game.tick_hz()
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
    pub fn playbook(&self) -> &G::Book {
        self.bot.playbook()
    }

    #[must_use]
    pub fn class_point(&self, attacker: u8, class: u8) -> [f32; 2] {
        G::class_point(self.bot.playbook(), attacker, class)
    }

    #[must_use]
    pub fn digest(&self) -> u64 {
        self.game.digest(self.game.live())
    }

    #[must_use]
    pub fn record(&self) -> Vec<u8> {
        if self.spectating() {
            return Vec::new();
        }
        let report = match (&self.report, &self.runner) {
            (Some(report), _) => report.clone(),
            (None, Some(runner)) => runner.report(&self.game),
            (None, None) => return Vec::new(),
        };
        encode_record(&report.record())
    }
}

impl Play<AirHockey> {
    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.human.aim(x, y);
    }

    #[must_use]
    pub fn positions(&self) -> [f32; 12] {
        let v = self.game.view();
        std::array::from_fn(|i| v[i])
    }
}
