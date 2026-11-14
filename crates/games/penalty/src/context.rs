use feint_protocol::PlayerId;

use crate::{PenaltyPlaybook, Phase, Shootout};

pub const TOKEN_WIDTH: usize = 20;
pub const TARGETS: usize = 4;
pub const SHOT: usize = 0;
pub const TELL: usize = 1;
pub const GUESS: usize = 2;
pub const FOLLOW: usize = 3;
pub const KICKS: usize = 10;
pub const EARLY_DIVE_TICKS: u32 = crate::human::DELAY_TICKS as u32;

#[derive(Clone, Debug, PartialEq)]
pub struct KickToken {
    pub features: [f32; TOKEN_WIDTH],
    pub target: [i8; TARGETS],
}

impl KickToken {
    #[must_use]
    pub fn human_shoots(&self) -> bool {
        self.features[0] == 1.0
    }
}

#[derive(Clone, Debug, Default)]
struct Partial {
    kick: u32,
    human_shoots: bool,
    first_lean: Option<u8>,
    strike_lean: Option<u8>,
    aim: Option<f64>,
    flight: bool,
    early_dive: Option<u8>,
    dive_seen: bool,
    score: [u32; 2],
    shooter: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Tokenizer {
    open: Option<Partial>,
    done: Vec<KickToken>,
}

fn lean_class(lean: f64) -> Option<u8> {
    PenaltyPlaybook::lean_class(lean)
}

fn hot(out: &mut [f32], class: Option<u8>) {
    let i = class.map_or(0, |c| if c == 0 { 1 } else { 2 });
    out[i] = 1.0;
}

impl Partial {
    fn token(&self, scored: bool) -> KickToken {
        let mut f = [0.0; TOKEN_WIDTH];
        f[0] = f32::from(u8::from(self.human_shoots));
        f[1] = self.kick as f32 / KICKS as f32;
        let shot = self.aim.map(PenaltyPlaybook::class_of);
        let mut target = [-1i8; TARGETS];
        if self.human_shoots {
            hot(&mut f[2..5], self.first_lean);
            hot(&mut f[5..8], self.strike_lean);
            if let Some(c) = shot {
                f[8 + usize::from(c)] = 1.0;
                target[SHOT] = c as i8;
                target[TELL] = match self.first_lean {
                    None => 2,
                    Some(l) if l == c => 0,
                    Some(l) if c != 1 && l == 2 - c => 1,
                    Some(_) => -1,
                };
            }
        } else {
            f[11] = f32::from(u8::from(self.early_dive.is_some()));
            if let Some(d) = self.early_dive {
                f[12 + usize::from(d)] = 1.0;
            }
            hot(&mut f[15..18], self.strike_lean);
            if let Some(lean) = self.strike_lean {
                target[GUESS] = i8::from(self.early_dive.is_some());
                if let Some(d) = self.early_dive {
                    target[FOLLOW] = i8::from(d == lean);
                }
            }
        }
        f[18] = f32::from(u8::from(scored));
        f[19] = 1.0;
        KickToken {
            features: f,
            target,
        }
    }
}

impl Tokenizer {
    pub fn see(&mut self, s: &Shootout, me: PlayerId) {
        let fresh = self.open.as_ref().is_none_or(|p| p.kick != s.kicks);
        if fresh {
            if let Some(p) = self.open.take()
                && s.kicks > p.kick
            {
                let i = usize::from(p.shooter);
                self.done.push(p.token(s.score[i] > p.score[i]));
            }
            if s.phase == Phase::Over {
                return;
            }
            self.open = Some(Partial {
                kick: s.kicks,
                human_shoots: s.shooter() != me,
                score: s.score,
                shooter: s.shooter(),
                ..Partial::default()
            });
        }
        let Some(p) = self.open.as_mut() else {
            return;
        };
        if !p.human_shoots && s.dived && !p.dive_seen {
            p.dive_seen = true;
            let early = matches!(s.phase, Phase::RunUp | Phase::AtBall | Phase::Strike)
                || (s.phase == Phase::Flight && s.t < EARLY_DIVE_TICKS);
            if early {
                p.early_dive = s.dive.map(|d| PenaltyPlaybook::class_of(d.target));
            }
        }
        match s.phase {
            Phase::RunUp | Phase::AtBall | Phase::Strike => {
                if p.human_shoots && p.first_lean.is_none() {
                    p.first_lean = lean_class(s.lean);
                }
            }
            Phase::Flight => {
                if !p.flight {
                    p.flight = true;
                    p.strike_lean = lean_class(s.lean);
                }
                if s.t > 0 && s.aim.is_some() {
                    p.aim = s.aim;
                }
            }
            Phase::Settle | Phase::Over => {}
        }
    }

    #[must_use]
    pub fn tokens(&self) -> &[KickToken] {
        &self.done
    }

    pub fn clear(&mut self) {
        self.open = None;
        self.done.clear();
    }
}

pub fn npy_header(descr: &str, shape: &[usize]) -> Vec<u8> {
    let dims: Vec<String> = shape.iter().map(ToString::to_string).collect();
    let shape = if dims.len() == 1 {
        format!("({},)", dims[0])
    } else {
        format!("({})", dims.join(", "))
    };
    let mut dict = format!("{{'descr': '{descr}', 'fortran_order': False, 'shape': {shape}, }}");
    let unpadded = 10 + dict.len() + 1;
    dict.push_str(&" ".repeat((64 - unpadded % 64) % 64));
    dict.push('\n');
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&u16::try_from(dict.len()).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(dict.as_bytes());
    out
}

#[must_use]
pub fn npy_f32(shape: &[usize], values: &[f32]) -> Vec<u8> {
    let mut out = npy_header("<f4", shape);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[must_use]
pub fn npy_i8(shape: &[usize], values: &[i8]) -> Vec<u8> {
    let mut out = npy_header("|i1", shape);
    out.extend(values.iter().map(|v| v.to_le_bytes()[0]));
    out
}

pub const LAYOUT: &str = "context-v1:penalty:20:10:8";

#[derive(Clone, Copy, Debug)]
struct Answers {
    corner: [f64; 3],
    tell: [f64; 3],
    guess: f64,
    follow: f64,
}

pub const CONFIDENCE: f64 = 0.6;

pub struct ContextAdvisor {
    model: std::sync::Arc<feint_context::ContextModel>,
    tokens: Tokenizer,
    asked: usize,
    answers: Answers,
    confidence: f64,
}

fn ask(model: &feint_context::ContextModel, tokens: &[KickToken]) -> Answers {
    let features: Vec<[f32; TOKEN_WIDTH]> = tokens.iter().map(|t| t.features).collect();
    let out = model.predict(&features).map(f64::from);
    let corner = feint_context::softmax(&out[0..3]);
    let tell = feint_context::softmax(&out[3..6]);
    Answers {
        corner: [corner[0], corner[1], corner[2]],
        tell: [tell[0], tell[1], tell[2]],
        guess: feint_context::sigmoid(out[6]),
        follow: feint_context::sigmoid(out[7]),
    }
}

impl ContextAdvisor {
    #[must_use]
    pub fn new(model: std::sync::Arc<feint_context::ContextModel>) -> Self {
        Self::with_confidence(model, CONFIDENCE)
    }

    #[must_use]
    pub fn with_confidence(
        model: std::sync::Arc<feint_context::ContextModel>,
        confidence: f64,
    ) -> Self {
        let answers = ask(&model, &[]);
        Self {
            model,
            tokens: Tokenizer::default(),
            asked: 0,
            answers,
            confidence,
        }
    }
}

impl feint_ai::Predictor for ContextAdvisor {
    fn likely_shot(&self) -> Option<u8> {
        let c = self.answers.corner;
        let mut best = 0;
        for k in 1..3 {
            if c[k] > c[best] {
                best = k;
            }
        }
        u8::try_from(best).ok()
    }

    fn likely_cover(&self, shown: u8) -> Option<u8> {
        let a = self.answers;
        let sure = a.follow.max(1.0 - a.follow);
        if shown == 1 || shown > 2 || a.guess < self.confidence || sure < self.confidence {
            return None;
        }
        Some(if a.follow >= 0.5 { shown } else { 2 - shown })
    }

    fn likely_real(&self, _shown: u8) -> Option<u8> {
        None
    }

    fn their_odds(&self, shown: u8) -> Option<Vec<(Option<u8>, f64)>> {
        if shown == 1 || shown > 2 {
            return None;
        }
        let a = self.answers;
        let same = a.corner[usize::from(shown)] * a.tell[0];
        let other = a.corner[usize::from(2 - shown)] * a.tell[1];
        let total = same + other;
        (total > 0.0).then(|| {
            vec![
                (Some(shown), same / total),
                (Some(2 - shown), other / total),
            ]
        })
    }

    fn knowing_beats_reading(&self) -> bool {
        false
    }

    fn shot_odds(&self) -> Option<Vec<(u8, f64)>> {
        Some((0..3u8).zip(self.answers.corner).collect())
    }
}

impl feint_ai::Advisor<Shootout> for ContextAdvisor {
    fn see(&mut self, state: &Shootout, me: PlayerId) {
        self.tokens.see(state, me);
        let n = self.tokens.tokens().len();
        if n != self.asked {
            self.asked = n;
            self.answers = ask(&self.model, self.tokens.tokens());
        }
    }

    fn new_match(&mut self) {
        self.tokens.clear();
        self.asked = 0;
        self.answers = ask(&self.model, &[]);
    }
}
