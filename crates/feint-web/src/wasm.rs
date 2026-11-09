use airhockey::AirHockey;
use feint_ai::{InsightView, Profile, decode_profile, encode_profile};
use feint_protocol::EventKind;
use wasm_bindgen::prelude::*;

use crate::hub::HubGame;
use crate::session::{Play, Session};

trait Live {
    fn advance(&mut self, ticks: u32) -> u32;
    fn set_input(&mut self, values: &[f32]);
    fn push_input(&mut self, values: &[f32]);
    fn view(&self) -> Vec<f32>;
    fn layout(&self) -> Vec<f32>;
    fn score(&self) -> [u32; 2];
    fn tick(&self) -> u64;
    fn tick_hz(&self) -> u32;
    fn finished(&self) -> bool;
    fn pause(&self) -> f32;
    fn insight_of(&self, seat: u8) -> InsightView;
    fn class_point(&self, attacker: u8, class: u8) -> [f32; 2];
    fn record(&self) -> Vec<u8>;
    fn digest(&self) -> u64;
    fn spectating(&self) -> bool;
    fn profile(&self) -> Option<Profile>;
    fn game_id(&self) -> &'static str;
    fn arena(&self) -> bool;
}

impl<G: HubGame> Live for Play<G> {
    fn advance(&mut self, ticks: u32) -> u32 {
        let goals = Play::advance(self, ticks)
            .iter()
            .filter(|e| e.kind == EventKind::Score)
            .count();
        u32::try_from(goals).unwrap_or(u32::MAX)
    }

    fn set_input(&mut self, values: &[f32]) {
        Play::set_input(self, values);
    }

    fn push_input(&mut self, values: &[f32]) {
        Play::push_input(self, values);
    }

    fn view(&self) -> Vec<f32> {
        Play::view(self)
    }

    fn layout(&self) -> Vec<f32> {
        Play::layout(self)
    }

    fn score(&self) -> [u32; 2] {
        Play::score(self)
    }

    fn tick(&self) -> u64 {
        Play::tick(self)
    }

    fn tick_hz(&self) -> u32 {
        Play::tick_hz(self)
    }

    fn finished(&self) -> bool {
        Play::finished(self)
    }

    fn pause(&self) -> f32 {
        Play::serve_pause(self)
    }

    fn insight_of(&self, seat: u8) -> InsightView {
        Play::insight_of(self, seat)
    }

    fn class_point(&self, attacker: u8, class: u8) -> [f32; 2] {
        Play::class_point(self, attacker, class)
    }

    fn record(&self) -> Vec<u8> {
        Play::record(self)
    }

    fn digest(&self) -> u64 {
        Play::digest(self)
    }

    fn spectating(&self) -> bool {
        Play::spectating(self)
    }

    fn profile(&self) -> Option<Profile> {
        Play::profile(self)
    }

    fn game_id(&self) -> &'static str {
        G::ID
    }

    fn arena(&self) -> bool {
        G::ARENA
    }
}

fn unknown(_: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str("unknown game or level")
}

fn start(game: &str, level: u8, seed: u32) -> Result<Box<dyn Live>, JsValue> {
    let seed = u64::from(seed);
    match game {
        <AirHockey as HubGame>::ID => Ok(Box::new(Session::new(level, seed).map_err(unknown)?)),
        _ => Err(unknown(game)),
    }
}

fn spectate(game: &str, left: u8, right: u8, seed: u32) -> Result<Box<dyn Live>, JsValue> {
    let seed = u64::from(seed);
    match game {
        <AirHockey as HubGame>::ID => Ok(Box::new(
            Session::spectate(left, right, seed).map_err(unknown)?,
        )),
        _ => Err(unknown(game)),
    }
}

#[must_use]
#[wasm_bindgen]
pub fn games() -> Vec<String> {
    vec![<AirHockey as HubGame>::ID.to_string()]
}

#[wasm_bindgen]
pub struct WebMatch {
    session: Box<dyn Live>,
}

#[wasm_bindgen]
impl WebMatch {
    #[wasm_bindgen(constructor)]
    pub fn new(level: u8, seed: u32) -> Result<WebMatch, JsValue> {
        Session::new(level, u64::from(seed))
            .map(|session| WebMatch {
                session: Box::new(session),
            })
            .map_err(|_| JsValue::from_str("level must be 1 to 5"))
    }

    pub fn new_with_profile(level: u8, seed: u32, profile: &[u8]) -> Result<WebMatch, JsValue> {
        let profile = decode_profile(profile).map_err(|_| JsValue::from_str("bad profile"))?;
        Session::with_profile(level, u64::from(seed), Some(&profile))
            .map(|session| WebMatch {
                session: Box::new(session),
            })
            .map_err(|_| JsValue::from_str("this level takes no profile"))
    }

    pub fn create_with_profile(
        game: &str,
        level: u8,
        seed: u32,
        profile: &[u8],
    ) -> Result<WebMatch, JsValue> {
        let profile = decode_profile(profile).map_err(|_| JsValue::from_str("bad profile"))?;
        let seed = u64::from(seed);
        let session: Box<dyn Live> = match game {
            <AirHockey as HubGame>::ID => Box::new(
                Session::with_profile(level, seed, Some(&profile))
                    .map_err(|_| JsValue::from_str("this level takes no profile"))?,
            ),
            _ => return Err(unknown(game)),
        };
        Ok(WebMatch { session })
    }

    pub fn spectate(left: u8, right: u8, seed: u32) -> Result<WebMatch, JsValue> {
        spectate(<AirHockey as HubGame>::ID, left, right, seed).map(|session| WebMatch { session })
    }

    pub fn create(game: &str, level: u8, seed: u32) -> Result<WebMatch, JsValue> {
        start(game, level, seed).map(|session| WebMatch { session })
    }

    pub fn watch(game: &str, left: u8, right: u8, seed: u32) -> Result<WebMatch, JsValue> {
        spectate(game, left, right, seed).map(|session| WebMatch { session })
    }

    pub fn game_id(&self) -> String {
        self.session.game_id().to_string()
    }

    pub fn arena(&self) -> bool {
        self.session.arena()
    }

    pub fn spectating(&self) -> bool {
        self.session.spectating()
    }

    pub fn intent_of(&self, seat: u8) -> i32 {
        self.session
            .insight_of(seat)
            .intent
            .map_or(-1, |(intent, deceptive)| {
                i32::from(intent) * 2 + i32::from(deceptive)
            })
    }

    pub fn memory_of(&self, seat: u8) -> bool {
        self.session.insight_of(seat).memory
    }

    pub fn likely_shot_of(&self, seat: u8) -> i32 {
        self.session
            .insight_of(seat)
            .likely_shot
            .map_or(-1, i32::from)
    }

    pub fn likely_cover_of(&self, seat: u8) -> Vec<i32> {
        self.session
            .insight_of(seat)
            .likely_cover
            .iter()
            .map(|c| c.map_or(-1, i32::from))
            .collect()
    }

    pub fn shown_of(&self, seat: u8) -> i32 {
        self.session
            .insight_of(seat)
            .current
            .and_then(|c| c.shown)
            .map_or(-1, i32::from)
    }

    pub fn real_of(&self, seat: u8) -> i32 {
        self.session
            .insight_of(seat)
            .current
            .and_then(|c| c.real)
            .map_or(-1, i32::from)
    }

    pub fn profile(&self) -> Vec<u8> {
        self.session
            .profile()
            .map(|p| encode_profile(&p))
            .unwrap_or_default()
    }

    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.session.set_input(&[x, y]);
    }

    pub fn set_input(&mut self, values: &[f32]) {
        self.session.set_input(values);
    }

    pub fn push_input(&mut self, values: &[f32]) {
        self.session.push_input(values);
    }

    pub fn advance(&mut self, ticks: u32) -> u32 {
        self.session.advance(ticks)
    }

    pub fn positions(&self) -> Vec<f32> {
        self.session.view()
    }

    pub fn view(&self) -> Vec<f32> {
        self.session.view()
    }

    pub fn serve_pause(&self) -> f32 {
        self.session.pause()
    }

    pub fn score(&self) -> Vec<u32> {
        self.session.score().to_vec()
    }

    pub fn tick(&self) -> f64 {
        self.session.tick() as f64
    }

    pub fn tick_hz(&self) -> u32 {
        self.session.tick_hz()
    }

    pub fn finished(&self) -> bool {
        self.session.finished()
    }

    pub fn intent(&self) -> i32 {
        self.intent_of(1)
    }

    pub fn memory(&self) -> bool {
        self.memory_of(1)
    }

    pub fn likely_shot(&self) -> i32 {
        self.likely_shot_of(1)
    }

    pub fn likely_cover(&self) -> Vec<i32> {
        self.likely_cover_of(1)
    }

    pub fn current_shown(&self) -> i32 {
        self.shown_of(1)
    }

    pub fn current_real(&self) -> i32 {
        self.real_of(1)
    }

    pub fn table(&self) -> Vec<f32> {
        self.session.layout()
    }

    pub fn layout(&self) -> Vec<f32> {
        self.session.layout()
    }

    pub fn class_point(&self, attacker: u8, class: u8) -> Vec<f32> {
        self.session.class_point(attacker, class).to_vec()
    }

    pub fn record(&self) -> Vec<u8> {
        self.session.record()
    }

    pub fn digest_hex(&self) -> String {
        format!("{:016x}", self.session.digest())
    }
}
