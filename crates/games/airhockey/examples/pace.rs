use airhockey::{AirHockey, HockeyPlaybook, Params};
use std::collections::VecDeque;

use feint_ai::{Bot, Controller, Difficulty, RunHeader, Runner};
use feint_protocol::{
    ActionT, BodyParamsT, EventKind, GameAdapter, GameSpecT, ObservationT, PlayerId,
};

const SEEDS: u64 = 6;
const CAP_SECONDS: u64 = 900;
const PAIRS: [[u8; 2]; 6] = [[2, 3], [2, 5], [3, 5], [0, 1], [0, 3], [0, 5]];
const CHASER_DELAY_TICKS: usize = 60;

struct Chaser {
    me: PlayerId,
    seen: VecDeque<[f32; 2]>,
}

impl Controller<AirHockey> for Chaser {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
        self.seen.clear();
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let v = obs.state.as_deref()?;
        self.seen.push_back([v[0], v[1]]);
        if self.seen.len() <= CHASER_DELAY_TICKS {
            return None;
        }
        let [x, y] = self.seen.pop_front()?;
        if !obs.tick.is_multiple_of(4) {
            return None;
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        a.values = Some(vec![x, y]);
        Some(a)
    }
}

fn params(args: &[String]) -> Params {
    let mut p = Params::default();
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            panic!("expected key=value, got {arg}");
        };
        let v: f64 = value.parse().expect("a number");
        match key {
            "cap" => p.puck_speed_cap = v,
            "speed" => p.max_body_speed = v,
            "accel" => p.max_body_accel = v,
            "puck" => p.puck_radius = v,
            "mallet" => p.mallet_radius = v,
            "mouth" => p.mouth_half_width = v,
            "damping" => p.damping = v,
            "restitution" => p.mallet_restitution = v,
            _ => panic!("unknown key {key}"),
        }
    }
    p.validate().expect("valid params");
    p
}

fn percentile(values: &mut [f64], p: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    let i = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[i]
}

struct Stats {
    seconds: Vec<f64>,
    goals: [u32; 2],
    across: Vec<f64>,
    speeds: Vec<f64>,
}

fn play(p: &Params, levels: [u8; 2], seed: u64, stats: &mut Stats) {
    let mut game = AirHockey::new(p.clone(), levels[1]);
    let spec = game.spec();
    let preset = |l: u8| Difficulty::preset(l).unwrap();
    let human = || {
        let mut b = BodyParamsT::default();
        b.values = Some(vec![p.max_body_speed as f32, p.max_body_accel as f32]);
        b
    };
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed,
        bodies: levels
            .iter()
            .map(|&l| {
                if l == 0 {
                    human()
                } else {
                    preset(l).body(&spec)
                }
            })
            .collect(),
        levels,
        opponent: feint_ai::Opponent::HandMade,
    };
    let side = |l: u8| -> Box<dyn Controller<AirHockey>> {
        if l == 0 {
            Box::new(Chaser {
                me: 0,
                seen: VecDeque::new(),
            })
        } else {
            Box::new(Bot::new(preset(l), HockeyPlaybook::new(p.clone()), &spec).unwrap())
        }
    };
    let (mut a, mut b) = (side(levels[0]), side(levels[1]));
    let mut runner = Runner::start(&mut game, [a.as_mut(), b.as_mut()], header).unwrap();
    let hz = f64::from(p.tick_hz);
    let mut last_x = 0.0;
    let cap = CAP_SECONDS * u64::from(p.tick_hz);
    while runner.ticks() < cap && runner.step(&mut game, [a.as_mut(), b.as_mut()]) {
        for e in runner.events() {
            if e.kind == EventKind::Score {
                stats.goals[usize::from(e.player.clamp(0, 1) as u8)] += 1;
            }
        }
        let puck = game.live().world.puck;
        let speed = puck.vel.length();
        if speed > 0.5 {
            stats.speeds.push(speed);
        }
        let x = puck.pos.x;
        if last_x != 0.0 && x != 0.0 && (last_x < 0.0) != (x < 0.0) && puck.vel.x.abs() > 0.3 {
            stats.across.push(2.0 * p.half_length / puck.vel.x.abs());
        }
        last_x = x;
    }
    stats.seconds.push(runner.ticks() as f64 / hz);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let p = params(&args);
    println!(
        "cap {} m/s, body {} m/s {} m/s2, puck r {}, mallet r {}, mouth {}",
        p.puck_speed_cap,
        p.max_body_speed,
        p.max_body_accel,
        p.puck_radius,
        p.mallet_radius,
        p.mouth_half_width
    );
    for levels in PAIRS {
        let mut stats = Stats {
            seconds: Vec::new(),
            goals: [0, 0],
            across: Vec::new(),
            speeds: Vec::new(),
        };
        for seed in 0..SEEDS {
            play(&p, levels, seed, &mut stats);
        }
        let total: f64 = stats.seconds.iter().sum();
        let goals = stats.goals[0] + stats.goals[1];
        let fast = stats.across.iter().filter(|&&t| t < 0.35).count() as f64
            / stats.across.len().max(1) as f64;
        let mut seconds = stats.seconds.clone();
        println!(
            "L{} vs L{}: match {:.0}s (median), {:.1} goals/min, goals {}:{}, across p50 {:.2}s p10 {:.2}s, <0.35s {:.0}%, puck p50 {:.1} p90 {:.1} m/s",
            levels[0],
            levels[1],
            percentile(&mut seconds, 0.5),
            f64::from(goals) / (total / 60.0),
            stats.goals[0],
            stats.goals[1],
            percentile(&mut stats.across, 0.5),
            percentile(&mut stats.across, 0.1),
            fast * 100.0,
            percentile(&mut stats.speeds, 0.5),
            percentile(&mut stats.speeds, 0.9),
        );
    }
}
