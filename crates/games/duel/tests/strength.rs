use std::collections::VecDeque;

use duel::{DuelPlaybook, Params, QuickDraw};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, RunReport, run_match};
use feint_protocol::{ActionT, BodyParamsT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};

const DELAY_TICKS: usize = 72;
const MATCHES: u64 = 40;

struct Human {
    me: PlayerId,
    seen: VecDeque<ObservationT>,
    rng: ChaCha8Rng,
    cycle: f32,
    bites: bool,
    left: bool,
    fake: bool,
    wait: f32,
    noise: f32,
    loaded_for: u32,
}

impl Human {
    fn new() -> Self {
        Self {
            me: 0,
            seen: VecDeque::new(),
            rng: ChaCha8Rng::seed_from_u64(0),
            cycle: -1.0,
            bites: false,
            left: false,
            fake: false,
            wait: 0.0,
            noise: 0.0,
            loaded_for: 0,
        }
    }

    fn unit(&mut self) -> f32 {
        (self.rng.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

impl Controller<QuickDraw> for Human {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.me = me;
        self.seen.clear();
        self.rng = ChaCha8Rng::seed_from_u64(seed ^ 0x48_554d_414e);
        self.cycle = -1.0;
        self.loaded_for = 0;
    }

    fn on_tick(&mut self, _game: &QuickDraw, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push_back(obs.clone());
        while self.seen.len() > DELAY_TICKS + 1 {
            self.seen.pop_front();
        }
        let v = self.seen.front()?.state.clone()?;
        let (my_x, my_hand, my_loaded, my_recovery) = (v[0], v[1], v[3] >= 0.5, v[4]);
        let (their_x, their_hand, their_loaded) = (v[9], v[10], v[12] >= 0.5);
        let their_shots = v[16];
        let live = v[26] == 1.0;
        let cycle = v[28] * 1000.0 + their_shots;
        if cycle != self.cycle {
            self.cycle = cycle;
            self.bites = self.unit() < 0.7;
            self.left = self.unit() < 0.65;
            self.fake = self.unit() < 0.2;
            self.wait = 0.2 + 0.8 * self.unit();
            let guess = self.unit();
            let lead = if guess < 0.5 {
                0.0
            } else if guess < 0.75 {
                -1.0
            } else {
                1.0
            };
            self.noise = lead + (self.unit() - 0.5) * 0.3;
        }
        let threatened = their_loaded && their_hand > 0.3;
        let roll = if live && my_recovery == 0.0 && self.bites && threatened {
            let room_left = my_x > -1.4;
            let room_right = my_x < 1.4;
            if (self.left && room_left) || !room_right {
                -1.0
            } else {
                1.0
            }
        } else {
            0.0
        };
        if my_loaded && live {
            self.loaded_for += 1;
        } else {
            self.loaded_for = 0;
        }
        let waited = self.loaded_for as f32 / 240.0 > self.wait;
        let hold = if !my_loaded || !waited {
            false
        } else if self.fake {
            if my_hand >= 0.4 {
                self.fake = false;
                self.wait += 0.6;
                false
            } else {
                true
            }
        } else {
            true
        };
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        a.values = Some(vec![their_x + self.noise, f32::from(u8::from(hold)), roll]);
        Some(a)
    }
}

fn human_body(game: &QuickDraw) -> BodyParamsT {
    let spec = game.spec();
    let dims = spec.body_params.unwrap();
    let mut b = BodyParamsT::default();
    b.values = Some(dims.iter().map(|d| d.max).collect());
    b
}

fn against_human(d: Difficulty, seeds: u64) -> (u32, u32, u32) {
    let (mut won, mut bot_hits, mut human_hits) = (0, 0, 0);
    for seed in 0..seeds {
        let me = (seed % 2) as usize;
        let mut game = QuickDraw::new(Params::default(), 3);
        let spec = game.spec();
        let mut bot = Bot::new(d, DuelPlaybook::new(Params::default()), &spec).unwrap();
        let mut human = Human::new();
        let bodies = if me == 0 {
            vec![d.body(&spec), human_body(&game)]
        } else {
            vec![human_body(&game), d.body(&spec)]
        };
        let header = RunHeader {
            game_id: "duel".into(),
            game_version: duel::GAME_VERSION,
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
        bot_hits += b;
        human_hits += h;
    }
    (won, bot_hits, human_hits)
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
            "level {level}: won {} of {MATCHES}, hits {}:{}, share {:.2}",
            row.0,
            row.1,
            row.2,
            share(*row)
        );
    }
    assert!(ladder[0].0 <= 10, "the human model must beat level 1");
    assert!(ladder[1].0 >= 24, "level 2 must win most duels");
    assert!(
        share(ladder[1]) <= 0.85,
        "the human model must still hit level 2"
    );
    assert!(
        share(ladder[4]) >= share(ladder[1]),
        "level 5 falls below level 2"
    );
}
