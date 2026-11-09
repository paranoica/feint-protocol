#![allow(dead_code)]

use feint_ai::{Choice, Cue, Playbook, Read, Stance};
use feint_protocol::{
    ActionSpaceT, ActionT, AdapterError, BodyParamsT, ChaCha8Rng, DimT, EventT, GameAdapter,
    GameSpecT, MatchResultT, ObservationT, PlayerId, TimeMode,
};

pub const TICK_HZ: u32 = 240;

#[derive(Clone, Debug, PartialEq)]
pub struct LineState {
    pub tick: u64,
    pub ball: f64,
    pub ball_v: f64,
    pub pos: [f64; 2],
    pub target: [f64; 2],
    pub speed: [f64; 2],
}

pub struct Line {
    pub state: LineState,
    pub forbid_live: bool,
    pub end_at: Option<u64>,
    pub simulated: std::cell::Cell<u64>,
}

impl Line {
    pub fn new(ball: f64, ball_v: f64) -> Self {
        Self {
            state: LineState {
                tick: 0,
                ball,
                ball_v,
                pos: [0.0, 0.0],
                target: [0.0, 0.0],
                speed: [1.0, 1.0],
            },
            forbid_live: false,
            end_at: None,
            simulated: std::cell::Cell::new(0),
        }
    }
}

fn dim(min: f32, max: f32) -> DimT {
    let mut d = DimT::default();
    d.min = min;
    d.max = max;
    d
}

pub fn line_spec(decision_hz: u32) -> GameSpecT {
    let mut spec = GameSpecT::default();
    spec.game_id = Some("line".into());
    spec.time_mode = TimeMode::Realtime;
    spec.tick_hz = TICK_HZ;
    spec.decision_hz = decision_hz;
    let mut space = ActionSpaceT::default();
    space.continuous = Some(vec![dim(-1.0, 1.0)]);
    spec.action_space = Some(Box::new(space));
    spec.body_params = Some(vec![dim(0.0, 2.0)]);
    spec
}

fn step(state: &mut LineState) {
    let dt = 1.0 / f64::from(TICK_HZ);
    state.ball += state.ball_v * dt;
    for i in 0..2 {
        let gap = state.target[i] - state.pos[i];
        let reach = state.speed[i] * dt;
        state.pos[i] += gap.clamp(-reach, reach);
    }
    state.tick = state.tick.saturating_add(1);
}

fn set_target(state: &mut LineState, action: &ActionT) {
    if let (Some(&[x]), true) = (action.values.as_deref(), action.player < 2) {
        state.target[usize::from(action.player)] = f64::from(x);
    }
}

impl GameAdapter for Line {
    type State = LineState;

    fn spec(&self) -> GameSpecT {
        line_spec(60)
    }

    fn reset(&mut self, seed: u64, bodies: &[BodyParamsT]) -> Result<(), AdapterError> {
        if bodies.len() != 2 {
            return Err(AdapterError::BadBody("one per player"));
        }
        self.state.tick = 0;
        self.state.ball = -0.5 + (seed % 10) as f64 / 10.0;
        self.state.pos = [0.0, 0.0];
        self.state.target = [0.0, 0.0];
        for (i, body) in bodies.iter().take(2).enumerate() {
            if let Some(&[speed]) = body.values.as_deref() {
                self.state.speed[i] = f64::from(speed);
            }
        }
        Ok(())
    }

    fn live(&self) -> &LineState {
        assert!(!self.forbid_live, "the bot read the live state");
        &self.state
    }

    fn apply(&mut self, action: &ActionT) -> Result<(), AdapterError> {
        if action.tick != self.state.tick {
            return Err(AdapterError::WrongTick {
                expected: self.state.tick,
                got: action.tick,
            });
        }
        set_target(&mut self.state, action);
        Ok(())
    }

    fn advance(&mut self, _events: &mut Vec<EventT>) {
        step(&mut self.state);
    }

    fn observe(&self, state: &LineState, player: PlayerId, out: &mut ObservationT) {
        let me = usize::from(player.min(1));
        out.tick = state.tick;
        out.player = player;
        out.state = Some(
            [
                state.ball,
                state.ball_v,
                state.pos[me],
                state.pos[1 - me],
                state.target[me],
            ]
            .map(|v| v as f32)
            .to_vec(),
        );
    }

    fn state_from_observation(&self, obs: &ObservationT, _rng: &mut ChaCha8Rng) -> LineState {
        let v: Vec<f64> = obs.state.iter().flatten().map(|&x| f64::from(x)).collect();
        let me = usize::from(obs.player.min(1));
        let mut pos = [0.0; 2];
        pos[me] = v[2];
        pos[1 - me] = v[3];
        let mut target = pos;
        target[me] = v[4];
        LineState {
            tick: obs.tick,
            ball: v[0],
            ball_v: v[1],
            pos,
            target,
            speed: self.state.speed,
        }
    }

    fn simulate(&self, state: &LineState, actions: &[ActionT], ticks: u32) -> LineState {
        let mut next = state.clone();
        for action in actions {
            set_target(&mut next, action);
        }
        for _ in 0..ticks {
            step(&mut next);
        }
        self.simulated.set(self.simulated.get() + u64::from(ticks));
        next
    }

    fn signals(&self, state: &LineState, action: &ActionT, out: &mut Vec<f32>) {
        let next = self.simulate(state, std::slice::from_ref(action), 1);
        out.clear();
        out.push(next.pos[usize::from(action.player.min(1))] as f32);
    }

    fn digest(&self, state: &LineState) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for word in [
            state.tick,
            state.ball.to_bits(),
            state.pos[0].to_bits(),
            state.pos[1].to_bits(),
        ] {
            for b in word.to_le_bytes() {
                h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    fn result(&self) -> Option<MatchResultT> {
        let end = self.end_at?;
        (self.state.tick >= end).then(|| {
            let mut result = MatchResultT::default();
            result.score = Some(vec![1, 0]);
            result.duration_ticks = self.state.tick;
            result
        })
    }
}

pub struct LinePlaybook {
    pub targets: Vec<f32>,
    pub fakes: Vec<f32>,
    pub shown: bool,
    pub flat: bool,
    pub horizon: u32,
    pub margin: f64,
    pub replies: std::cell::Cell<u32>,
    pub reads: bool,
    pub fake_real: Option<u8>,
    pub shoots: bool,
    pub twitches: std::cell::RefCell<Vec<Option<u8>>>,
}

impl LinePlaybook {
    pub fn new(targets: &[f32], horizon: u32) -> Self {
        Self {
            targets: targets.to_vec(),
            fakes: Vec::new(),
            shown: false,
            flat: false,
            horizon,
            margin: 0.0,
            replies: std::cell::Cell::new(0),
            reads: false,
            fake_real: Some(1),
            shoots: false,
            twitches: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl Playbook<Line> for LinePlaybook {
    fn options(&self, _s: &LineState, _me: PlayerId, _rep: u8, cue: &Cue, out: &mut Vec<Choice>) {
        out.clear();
        self.twitches.borrow_mut().push(cue.twitch);
        out.extend(self.targets.iter().enumerate().map(|(i, &x)| Choice {
            id: i as u16,
            intent: 1,
            deceptive: false,
            horizon_ticks: self.horizon,
            param: [x, 0.0],
            shown: self.shown.then_some(i as u8),
            real: self.shown.then_some(i as u8),
        }));
        let first = self.targets.len();
        out.extend(self.fakes.iter().enumerate().map(|(i, &x)| Choice {
            id: (first + i) as u16,
            intent: 2,
            deceptive: true,
            horizon_ticks: self.horizon,
            param: [x, 0.0],
            shown: Some(0),
            real: self.fake_real,
        }));
    }

    fn act(
        &self,
        _s: &LineState,
        me: PlayerId,
        c: &Choice,
        _elapsed: u64,
        aim: [f64; 2],
    ) -> ActionT {
        let mut a = ActionT::default();
        a.player = me;
        a.values = Some(vec![(f64::from(c.param[0]) + aim[0]) as f32]);
        a
    }

    fn opponent_move(&self, s: &LineState, them: PlayerId, _stance: Stance) -> ActionT {
        self.replies.set(self.replies.get() + 1);
        let mut a = ActionT::default();
        a.player = them;
        a.values = Some(vec![s.pos[usize::from(them.min(1))] as f32]);
        a
    }

    fn evaluate(&self, s: &LineState, me: PlayerId) -> f64 {
        if self.flat {
            return 0.0;
        }
        -(s.pos[usize::from(me.min(1))] - s.ball).abs()
    }

    fn margin(&self) -> f64 {
        self.margin
    }

    fn read(&self, before: &LineState, after: &LineState, them: PlayerId) -> Read {
        let i = usize::from(them.min(1));
        let (from, to) = (before.pos[i], after.pos[i]);
        let covered = (self.reads && (to - from).abs() > 0.01).then_some(if to > 0.0 {
            2
        } else if to < 0.0 {
            0
        } else {
            1
        });
        Read {
            covered,
            shot: (self.shoots && before.ball_v == 0.0 && after.ball_v != 0.0).then_some(1),
        }
    }

    fn possession(&self, s: &LineState, _me: PlayerId) -> Option<u64> {
        (s.ball >= 0.0).then_some(0)
    }
}
