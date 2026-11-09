use airhockey::Params;
use feint_protocol::EventKind;
use wasm_bindgen::prelude::*;

use crate::session::Session;

#[wasm_bindgen]
pub struct WebMatch {
    session: Session,
}

#[wasm_bindgen]
impl WebMatch {
    #[wasm_bindgen(constructor)]
    pub fn new(level: u8, seed: u32) -> Result<WebMatch, JsValue> {
        Session::new(level, u64::from(seed))
            .map(|session| WebMatch { session })
            .map_err(|_| JsValue::from_str("level must be 1 to 5"))
    }

    pub fn set_pointer(&mut self, x: f32, y: f32) {
        self.session.set_pointer(x, y);
    }

    pub fn advance(&mut self, ticks: u32) -> u32 {
        let goals = self
            .session
            .advance(ticks)
            .iter()
            .filter(|e| e.kind == EventKind::Score)
            .count();
        u32::try_from(goals).unwrap_or(u32::MAX)
    }

    pub fn positions(&self) -> Vec<f32> {
        self.session.positions().to_vec()
    }

    pub fn score(&self) -> Vec<u32> {
        self.session.score().to_vec()
    }

    pub fn tick(&self) -> f64 {
        self.session.tick() as f64
    }

    pub fn tick_hz(&self) -> u32 {
        Params::default().tick_hz
    }

    pub fn finished(&self) -> bool {
        self.session.finished()
    }

    pub fn intent(&self) -> i32 {
        self.session
            .insight()
            .intent
            .map_or(-1, |(intent, deceptive)| {
                i32::from(intent) * 2 + i32::from(deceptive)
            })
    }

    pub fn memory(&self) -> bool {
        self.session.insight().memory
    }

    pub fn likely_shot(&self) -> i32 {
        self.session.insight().likely_shot.map_or(-1, i32::from)
    }

    pub fn likely_cover(&self) -> Vec<i32> {
        self.session
            .insight()
            .likely_cover
            .iter()
            .map(|c| c.map_or(-1, i32::from))
            .collect()
    }

    pub fn current_shown(&self) -> i32 {
        self.session
            .insight()
            .current
            .and_then(|c| c.shown)
            .map_or(-1, i32::from)
    }

    pub fn current_real(&self) -> i32 {
        self.session
            .insight()
            .current
            .and_then(|c| c.real)
            .map_or(-1, i32::from)
    }

    pub fn table(&self) -> Vec<f32> {
        let p = Params::default();
        [
            p.half_length,
            p.half_width,
            p.mouth_half_width,
            p.puck_radius,
            p.mallet_radius,
        ]
        .map(|v| v as f32)
        .to_vec()
    }

    pub fn class_point(&self, attacker: u8, class: u8) -> Vec<f32> {
        let p = self.session.playbook().class_point(attacker, class);
        vec![p.x as f32, p.y as f32]
    }

    pub fn record(&self) -> Vec<u8> {
        self.session.record()
    }

    pub fn digest_hex(&self) -> String {
        format!("{:016x}", self.session.digest())
    }
}
