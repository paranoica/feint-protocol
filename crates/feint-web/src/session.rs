use std::num::NonZeroU64;

use airhockey::human::{HUMAN_GOALS, HUMAN_NOISE, Human};
use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{
    Bot, Controller, Difficulty, InsightView, Memory, Opponent, Profile, RunHeader, RunReport,
    Runner, encode_record,
};
use feint_protocol::{ActionT, EventT, GameAdapter, GameSpecT, ObservationT, PlayerId};

use crate::hub::{HubGame, HumanInput};

pub const GAME_ID: &str = "airhockey";
pub const RERUN_CAP_TICKS: u64 = 240 * 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    Level,
    Profile,
    Checkpoint,
    Unsupported,
}

#[must_use]
pub fn parse_checkpoint(hex: &str) -> Option<u64> {
    if hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        None
    }
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

fn blank() -> InsightView {
    InsightView {
        current: None,
        intent: None,
        memory: false,
        likely_shot: None,
        likely_cover: Vec::new(),
    }
}

enum Seat<G: HubGame> {
    HandMade(Box<Bot<G, G::Book>>),
    Trained(Box<dyn Controller<G>>, G::Book),
}

impl<G: HubGame> Seat<G> {
    fn controller(&mut self) -> &mut dyn Controller<G> {
        match self {
            Self::HandMade(bot) => bot.as_mut(),
            Self::Trained(pilot, _) => pilot.as_mut(),
        }
    }

    fn insight(&self) -> InsightView {
        match self {
            Self::HandMade(bot) => bot.insight(),
            Self::Trained(..) => blank(),
        }
    }

    fn playbook(&self) -> &G::Book {
        match self {
            Self::HandMade(bot) => bot.playbook(),
            Self::Trained(_, book) => book,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rerun {
    Trained,
    HandMade,
}

fn order<'a, G: HubGame>(
    bot: &'a mut Seat<G>,
    left: &'a mut Option<Bot<G, G::Book>>,
    model: &'a mut Option<Box<dyn Controller<G>>>,
    human: &'a mut G::Human,
    bot_seat: PlayerId,
) -> [&'a mut dyn Controller<G>; 2] {
    let other: &mut dyn Controller<G> = match (left, model) {
        (Some(left), _) => left,
        (None, Some(model)) => model.as_mut(),
        (None, None) => human,
    };
    if bot_seat == 0 {
        [bot.controller(), other]
    } else {
        [other, bot.controller()]
    }
}

pub struct Play<G: HubGame> {
    game: G,
    bot: Seat<G>,
    bot_seat: PlayerId,
    left: Option<Bot<G, G::Book>>,
    model: Option<Box<dyn Controller<G>>>,
    cap: Option<u64>,
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
        let game = G::build(level);
        let advisor = G::shipped_advisor(level);
        let opponent = advisor
            .and_then(NonZeroU64::new)
            .map_or(Opponent::HandMade, Opponent::Advised);
        let header = Self::header(level, seed, opponent, &game).ok_or(SessionError::Level)?;
        let mut bot =
            Bot::new(difficulty, G::book(), &game.spec()).map_err(|_| SessionError::Level)?;
        if let Some(id) = advisor {
            bot.advise(G::advisor(level, id)?);
        }
        if let Some(profile) = profile {
            bot.load_profile(profile, G::ID)
                .map_err(|_| SessionError::Profile)?;
        }
        Self::start(game, Seat::HandMade(Box::new(bot)), header)
    }

    pub fn trained(level: u8, seed: u64, checkpoint: u64) -> Result<Self, SessionError> {
        Difficulty::preset(level).ok_or(SessionError::Level)?;
        let game = G::build(level);
        let pilot = G::trained(level, checkpoint, &game.spec())?;
        let id = NonZeroU64::new(checkpoint).ok_or(SessionError::Checkpoint)?;
        let header =
            Self::header(level, seed, Opponent::Trained(id), &game).ok_or(SessionError::Level)?;
        Self::start(game, Seat::Trained(pilot, G::book()), header)
    }

    fn start(mut game: G, mut bot: Seat<G>, header: RunHeader) -> Result<Self, SessionError> {
        let mut human = G::Human::default();
        let runner = Runner::start(&mut game, [&mut human, bot.controller()], header)
            .map_err(|_| SessionError::Level)?;
        Ok(Self {
            game,
            bot,
            bot_seat: 1,
            left: None,
            model: None,
            cap: None,
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
            let mut bot =
                Bot::new(difficulty, G::book(), &spec).map_err(|_| SessionError::Level)?;
            if let Some(id) = G::shipped_advisor(level) {
                bot.advise(G::advisor(level, id)?);
            }
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
            opponent: feint_ai::Opponent::HandMade,
        };
        let runner = Runner::start(&mut game, [&mut left_bot, &mut bot], header)
            .map_err(|_| SessionError::Level)?;
        Ok(Self {
            game,
            bot: Seat::HandMade(Box::new(bot)),
            bot_seat: 1,
            left: Some(left_bot),
            model: None,
            cap: None,
            human: G::Human::default(),
            runner: Some(runner),
            report: None,
            events: Vec::new(),
        })
    }

    #[must_use]
    pub fn spectating(&self) -> bool {
        self.left.is_some() || self.model.is_some()
    }

    #[must_use]
    pub fn insight_of(&self, seat: PlayerId) -> InsightView {
        match (seat, &self.left) {
            (0, Some(left)) => left.insight(),
            _ if self.model.is_some() && seat != self.bot_seat => blank(),
            _ => self.bot.insight(),
        }
    }

    #[must_use]
    pub fn profile(&self) -> Option<Profile> {
        match &self.bot {
            Seat::HandMade(bot) => bot.profile(G::ID),
            Seat::Trained(..) => None,
        }
    }

    #[must_use]
    pub fn header(level: u8, seed: u64, opponent: Opponent, game: &G) -> Option<RunHeader> {
        let difficulty = Difficulty::preset(level)?;
        Some(RunHeader {
            game_id: G::ID.into(),
            game_version: G::VERSION,
            seed,
            bodies: vec![G::human_body(), difficulty.body(&game.spec())],
            levels: [0, level],
            opponent,
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
            if self.cap.is_some_and(|cap| runner.ticks() >= cap) {
                break;
            }
            let pair = order(
                &mut self.bot,
                &mut self.left,
                &mut self.model,
                &mut self.human,
                self.bot_seat,
            );
            if !runner.step(&mut self.game, pair) {
                break;
            }
            self.events.extend_from_slice(runner.events());
        }
        let capped = self
            .runner
            .as_ref()
            .zip(self.cap)
            .is_some_and(|(runner, cap)| runner.ticks() >= cap);
        if (self.game.result().is_some() || capped)
            && let Some(runner) = self.runner.take()
        {
            let pair = order(
                &mut self.bot,
                &mut self.left,
                &mut self.model,
                &mut self.human,
                self.bot_seat,
            );
            self.report = Some(runner.finish(&mut self.game, pair));
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
    pub fn rerun_human(kind: Rerun, level: u8, seed: u64, seat: u8) -> Result<Self, SessionError> {
        let difficulty = Difficulty::preset(level).ok_or(SessionError::Level)?;
        let five = Difficulty::preset(5).ok_or(SessionError::Level)?;
        if seat > 1 {
            return Err(SessionError::Level);
        }
        let params = Params {
            goals_to_win: HUMAN_GOALS,
            ..Params::default()
        };
        let mut game = AirHockey::new(params.clone(), 1);
        let spec = game.spec();
        let (mut bot, opponent) = match kind {
            Rerun::Trained => {
                let checkpoint = feint_ml::registry::for_level(level)
                    .ok_or(SessionError::Checkpoint)?
                    .id;
                let id = NonZeroU64::new(checkpoint).ok_or(SessionError::Checkpoint)?;
                let pilot = <AirHockey as HubGame>::trained(level, checkpoint, &spec)?;
                (
                    Seat::Trained(pilot, HockeyPlaybook::new(params.clone())),
                    Opponent::Trained(id),
                )
            }
            Rerun::HandMade => {
                let bot = Bot::new(difficulty, HockeyPlaybook::new(params.clone()), &spec)
                    .map_err(|_| SessionError::Level)?;
                (Seat::HandMade(Box::new(bot)), Opponent::HandMade)
            }
        };
        let (mine, model_body) = (difficulty.body(&spec), five.body(&spec));
        let header = RunHeader {
            game_id: GAME_ID.into(),
            game_version: airhockey::GAME_VERSION,
            seed,
            bodies: if seat == 0 {
                vec![mine, model_body]
            } else {
                vec![model_body, mine]
            },
            levels: [0, 0],
            opponent,
        };
        let mut model: Option<Box<dyn Controller<AirHockey>>> =
            Some(Box::new(Human::new(params, HUMAN_NOISE)));
        let mut left = None;
        let mut human = Pointer::new();
        let runner = Runner::start(
            &mut game,
            order(&mut bot, &mut left, &mut model, &mut human, seat),
            header,
        )
        .map_err(|_| SessionError::Level)?;
        Ok(Self {
            game,
            bot,
            bot_seat: seat,
            left,
            model,
            cap: Some(RERUN_CAP_TICKS),
            human,
            runner: Some(runner),
            report: None,
            events: Vec::new(),
        })
    }

    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.human.aim(x, y);
    }

    #[must_use]
    pub fn positions(&self) -> [f32; 12] {
        let v = self.game.view();
        std::array::from_fn(|i| v[i])
    }
}
