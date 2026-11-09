use std::collections::VecDeque;

use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use penalty::{Params, Penalty, PenaltyPlaybook};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

const DELAY_TICKS: usize = 72;
const MATCHES: u64 = 40;

struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    corner: f32,
    honest: bool,
    reads: bool,
    noise: f32,
    kick_seen: f32,
}

impl Human {
    fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            corner: 0.0,
            honest: true,
            reads: false,
            noise: 0.0,
            kick_seen: -1.0,
        }
    }

    fn unit(&mut self) -> f32 {
        (self.rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

impl Controller<Penalty> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x48_554d_414e);
        self.kick_seen = -1.0;
    }

    fn on_tick(&mut self, _game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let v = self.seen.front()?.state.clone()?;
        let (keeper, phase, t, dist, lean, kicks) = (v[0] >= 0.5, v[1], v[2], v[3], v[4], v[13]);
        if kicks != self.kick_seen {
            self.kick_seen = kicks;
            let pick = (self.unit() * 3.0).min(2.0).floor();
            self.corner = (pick - 1.0) * 2.7;
            self.honest = self.unit() < 0.6;
            self.reads = self.unit() < 0.5;
            self.noise = (self.unit() - 0.5) * 1.0;
        }
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        let values = if keeper {
            let near = phase == 1.0 || (phase == 0.0 && dist < 1.6);
            let flying = phase == 2.0 && t > 0.0;
            if self.reads && near && lean.abs() > 0.3 {
                [0.0, 0.0, 1.0, 2.8 * lean.signum() + self.noise]
            } else if flying {
                let f = 106.0 / 240.0;
                let aim = v[6] * f / t.min(f);
                [0.0, 0.0, 1.0, aim + self.noise]
            } else {
                [0.0, 0.0, 0.0, 0.0]
            }
        } else {
            let lean = if self.honest {
                self.corner.signum() * 0.8
            } else {
                0.0
            };
            let at_ball = phase == 1.0 || dist < 0.2;
            [
                lean,
                1.0,
                f32::from(u8::from(at_ball)),
                self.corner + self.noise,
            ]
        };
        a.values = Some(values.to_vec());
        Some(a)
    }
}

fn human_body(game: &Penalty) -> BodyParamsT {
    let spec = game.spec();
    let dims = spec.body_params.unwrap();
    let mut b = BodyParamsT::default();
    b.values = Some(dims.iter().map(|d| d.max).collect());
    b
}

fn against_human(d: Difficulty, seeds: u64) -> (u32, u32, u32) {
    let (mut won, mut bot_goals, mut human_goals) = (0, 0, 0);
    for seed in 0..seeds {
        let me = (seed % 2) as usize;
        let mut game = Penalty::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, PenaltyPlaybook::new(Params::default()), &spec).unwrap();
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "penalty".into(),
            game_version: penalty::GAME_VERSION,
            seed,
            bodies,
            levels: if me == 0 { [3, 0] } else { [0, 3] },
        };
        let report: RunReport = if me == 0 {
            run_match(&mut game, [&mut bot, &mut human], header, 240 * 900)
        } else {
            run_match(&mut game, [&mut human, &mut bot], header, 240 * 900)
        }
        .unwrap();
        let (b, h) = (report.score[me] as u32, report.score[1 - me] as u32);
        won += u32::from(b > h);
        bot_goals += b;
        human_goals += h;
    }
    (won, bot_goals, human_goals)
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn the_levels_climb_against_a_human_model() {
    let share = |(_, b, h): (u32, u32, u32)| f64::from(b) / f64::from((b + h).max(1));
    let ladder: Vec<(u32, u32, u32)> = (1..=5)
        .map(|l| against_human(Difficulty::preset(l).unwrap(), MATCHES))
        .collect();
    for (level, row) in (1..=5).zip(&ladder) {
        eprintln!(
            "level {level}: won {} of {MATCHES}, goals {}:{}, share {:.2}",
            row.0,
            row.1,
            row.2,
            share(*row)
        );
    }
    assert!(ladder[0].0 <= 10, "the human model must beat level 1");
    assert!(ladder[1].0 >= 24, "level 2 must win most shootouts");
    assert!(
        share(ladder[4]) >= share(ladder[1]) + 0.05,
        "level 5 is not clearly above level 2"
    );
}
