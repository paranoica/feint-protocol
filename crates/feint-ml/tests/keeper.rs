use airhockey::{AirHockey, GAME_VERSION, Params};
use feint_ai::{Controller, Opponent, RunHeader, Runner, run_match};
use feint_ml::keeper::{BITE_TICKS, KEEPER_DELAY_TICKS, Keeper, Read};
use feint_ml::show::SHOW_HOLD_TICKS;
use feint_ml::{BatchSpec, HockeyBatch, Seat};
use feint_protocol::{
    ActionT, BodyParamsT, ChaCha8Rng, GameAdapter, GameSpecT, ObservationT, PlayerId,
};
use rand_chacha::rand_core::SeedableRng;

fn full_body() -> BodyParamsT {
    let p = Params::default();
    let mut body = BodyParamsT::default();
    body.values = Some(vec![p.max_body_speed as f32, p.max_body_accel as f32]);
    body
}

fn header(seed: u64) -> RunHeader {
    RunHeader {
        game_id: "airhockey".into(),
        game_version: GAME_VERSION,
        seed,
        bodies: vec![full_body(), full_body()],
        levels: [0, 0],
        opponent: Opponent::HandMade,
    }
}

fn aim(puck: [f64; 2], y: f64) -> [f64; 2] {
    let d = [1.0 - puck[0], y - puck[1]];
    let n = (d[0] * d[0] + d[1] * d[1]).sqrt();
    [d[0] / n, d[1] / n]
}

struct Shooter {
    me: PlayerId,
    show_y: Option<f64>,
    shot_y: f64,
    strike_at: u64,
}

impl Controller<AirHockey> for Shooter {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let s = obs.state.as_deref()?;
        let puck = [f64::from(s[0]), f64::from(s[1])];
        let at = |dir: [f64; 2], k: f64| [puck[0] + dir[0] * k, puck[1] + dir[1] * k];
        let shot = aim(puck, self.shot_y);
        let t = obs.tick;
        let target = if t < self.strike_at {
            match self.show_y {
                Some(y) => at(aim(puck, y), -0.3),
                None => at(shot, -0.15),
            }
        } else if self.show_y.is_some() && t < self.strike_at + 20 {
            at(shot, -0.15)
        } else {
            at(shot, 0.4)
        };
        let mut action = ActionT::default();
        action.tick = t;
        action.player = self.me;
        action.values = Some(vec![target[0] as f32, target[1] as f32]);
        Some(action)
    }
}

struct Aside;

impl Controller<AirHockey> for Aside {
    fn new_match(&mut self, _spec: &GameSpecT, _me: PlayerId, _seed: u64) {}

    fn on_tick(&mut self, _game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        let mut action = ActionT::default();
        action.tick = obs.tick;
        action.player = 1;
        action.values = Some(vec![-0.5, 0.4]);
        Some(action)
    }
}

fn shooter(show_y: Option<f64>, shot_y: f64) -> Shooter {
    Shooter {
        me: 0,
        show_y,
        shot_y,
        strike_at: 70,
    }
}

fn shooter_goals(shooter: &mut Shooter, keeper: &mut dyn Controller<AirHockey>, seed: u64) -> i64 {
    let mut game = AirHockey::new(Params::default(), 0);
    let report = run_match(&mut game, [shooter, keeper], header(seed), 400).unwrap();
    report.score[0]
}

#[test]
fn an_honest_corner_shot_is_saved_and_scores_against_an_absent_keeper() {
    for seed in (0..40).step_by(2) {
        let corner = if seed % 4 == 0 { 0.13 } else { -0.13 };
        let mut keeper = Keeper::new(Params::default(), 700);
        assert_eq!(
            shooter_goals(&mut shooter(None, corner), &mut keeper, seed),
            0,
            "seed {seed}"
        );
        assert_eq!(
            shooter_goals(&mut shooter(None, corner), &mut Aside, seed),
            1,
            "seed {seed}"
        );
    }
}

#[test]
fn a_held_show_then_the_other_corner_beats_only_a_biting_keeper() {
    for seed in (0..40).step_by(2) {
        let mut biting = Keeper::new(Params::default(), 1000);
        assert_eq!(
            shooter_goals(&mut shooter(Some(0.3), -0.13), &mut biting, seed),
            1,
            "seed {seed}"
        );
        assert_eq!(biting.counts().bites, 1);
        assert_eq!(biting.bite_ticks().len(), 1);
        assert_eq!(biting.counts().bite_goals, 1);
        let mut steady = Keeper::new(Params::default(), 0);
        assert_eq!(
            shooter_goals(&mut shooter(Some(0.3), -0.13), &mut steady, seed),
            0,
            "seed {seed}"
        );
        assert_eq!(steady.counts().reads, 1);
        assert_eq!(steady.counts().bites, 0);
    }
}

#[test]
fn a_show_held_past_the_bite_is_not_drawn_again() {
    for seed in (0..20).step_by(2) {
        let mut keeper = Keeper::new(Params::default(), 1000);
        let mut held = Shooter {
            strike_at: 330,
            ..shooter(Some(0.3), -0.13)
        };
        let mut game = AirHockey::new(Params::default(), 0);
        run_match(&mut game, [&mut held, &mut keeper], header(seed), 320).unwrap();
        assert_eq!(keeper.counts().reads, 1, "seed {seed}");
        assert_eq!(keeper.counts().bites, 1, "seed {seed}");
    }
}

fn read(permille: u16) -> Read {
    Read::new(permille, ChaCha8Rng::seed_from_u64(9))
}

#[test]
fn a_show_is_read_once_it_is_held_as_long_as_the_detector_needs() {
    assert_eq!(SHOW_HOLD_TICKS, 30);
    let mut r = read(1000);
    for _ in 0..29 {
        assert_eq!(r.see(Some(2)), None);
    }
    assert_eq!(r.see(Some(2)), Some((2, true)));
    let mut r = read(1000);
    for _ in 0..29 {
        assert_eq!(r.see(Some(0)), None);
    }
    assert_eq!(r.see(None), None);
    assert_eq!(r.see(Some(0)), None);
}

#[test]
fn one_draw_per_show_however_long_it_is_held() {
    let mut r = read(1000);
    let draws = (0..300).filter_map(|_| r.see(Some(2))).count();
    assert_eq!(draws, 1);
    let switched = (0..30).filter_map(|_| r.see(Some(0))).count();
    assert_eq!(switched, 1);
    r.see(None);
    let again = (0..30).filter_map(|_| r.see(Some(0))).count();
    assert_eq!(again, 1);
}

#[test]
fn the_bite_rate_follows_the_setting() {
    for (permille, low, high) in [(0, 0, 0), (500, 170, 230), (1000, 400, 400)] {
        let mut r = read(permille);
        let mut bites = 0;
        for _ in 0..400 {
            for _ in 0..SHOW_HOLD_TICKS {
                if let Some((_, true)) = r.see(Some(2)) {
                    bites += 1;
                }
            }
            r.see(None);
        }
        assert!((low..=high).contains(&bites), "{permille}: {bites}");
    }
}

struct Tap {
    keeper: Keeper,
    seen: Vec<ObservationT>,
    actions: Vec<Option<ActionT>>,
    puck_speed: Vec<(u64, f64)>,
}

impl Controller<AirHockey> for Tap {
    fn new_match(&mut self, spec: &GameSpecT, me: PlayerId, seed: u64) {
        self.keeper.new_match(spec, me, seed);
    }

    fn on_tick(&mut self, game: &AirHockey, obs: &ObservationT) -> Option<ActionT> {
        self.seen.push(obs.clone());
        self.puck_speed
            .push((obs.tick, game.live().world.puck.vel.length()));
        let action = self.keeper.on_tick(game, obs);
        self.actions.push(action.clone());
        action
    }
}

fn tapped(seed: u64, ticks: u64) -> Tap {
    let mut game = AirHockey::new(Params::default(), 0);
    let mut tap = Tap {
        keeper: Keeper::new(Params::default(), 700),
        seen: Vec::new(),
        actions: Vec::new(),
        puck_speed: Vec::new(),
    };
    let mut shot = shooter(None, 0.13);
    run_match(&mut game, [&mut shot, &mut tap], header(seed), ticks).unwrap();
    tap
}

#[test]
fn the_keeper_reacts_100_ms_after_the_shot() {
    assert_eq!(KEEPER_DELAY_TICKS, 24);
    let tap = tapped(2, 300);
    let contact = tap.puck_speed.iter().find(|(_, v)| *v > 0.0).unwrap().0;
    let y = |a: &Option<ActionT>| a.as_ref().and_then(|a| a.values.clone()).map(|v| v[1]);
    let first = tap
        .actions
        .iter()
        .zip(&tap.seen)
        .find(|(a, _)| y(a).is_some_and(|v| v.abs() > 1e-6))
        .map(|(_, o)| o.tick)
        .unwrap();
    assert_eq!(first, contact + 24);
}

#[test]
fn the_keeper_acts_on_its_observations_alone() {
    let tap = tapped(4, 300);
    let spare = AirHockey::new(Params::default(), 3);
    let mut again = Keeper::new(Params::default(), 700);
    again.new_match(&spare.spec(), 1, 4);
    let replayed: Vec<Option<ActionT>> =
        tap.seen.iter().map(|o| again.on_tick(&spare, o)).collect();
    assert_eq!(replayed, tap.actions);
    assert!(tap.actions.iter().filter(|a| a.is_some()).count() > 200);
}

#[test]
fn the_keeper_clears_a_slow_puck_on_its_half() {
    let mut game = AirHockey::new(Params::default(), 0);
    let mut keeper = Keeper::new(Params::default(), 700);
    let mut idle = Aside;
    let mut runner = Runner::start(&mut game, [&mut idle, &mut keeper], header(1)).unwrap();
    assert!(game.live().world.puck.pos.x > 0.0);
    let mut crossed = false;
    while runner.ticks() < 480 && runner.step(&mut game, [&mut idle, &mut keeper]) {
        crossed |= game.live().world.puck.pos.x < 0.0;
    }
    assert!(crossed, "the keeper never sent the puck back");
}

#[test]
fn a_keeper_seat_plays_at_full_body_and_reports_its_counts() {
    let mut spec = BatchSpec::new(
        vec![[Seat::Learner, Seat::Keeper { bite_permille: 700 }]; 2],
        5,
    );
    spec.cap_ticks = 2400;
    let mut batch = HockeyBatch::new(spec).unwrap();
    batch.reset().unwrap();
    assert_eq!(batch.episode(0).unwrap().physique[1], 1.0);
    assert_eq!(batch.episode(0).unwrap().reaction_ms[1], 0);
    for _ in 0..1200 {
        batch.step(&[0.0; 8]).unwrap();
    }
    let counts = batch.keeper_counts(0).unwrap();
    assert!(counts.goals <= 7 * 4);
    assert!(batch.keeper_counts(2).is_none());
    assert_eq!(BITE_TICKS, 120);
}

#[test]
fn a_bank_shot_into_the_far_corner_is_saved_and_scores_against_an_absent_keeper() {
    let limit = Params::default().half_width - Params::default().puck_radius;
    for seed in (0..40).step_by(2) {
        let corner = if seed % 4 == 0 { 0.11 } else { -0.11 };
        let mirrored = if corner > 0.0 {
            -2.0 * limit - corner
        } else {
            2.0 * limit - corner
        };
        let mut keeper = Keeper::new(Params::default(), 700);
        let mut absent = shooter(None, mirrored);
        assert_eq!(
            shooter_goals(&mut absent, &mut Aside, seed),
            1,
            "seed {seed} misses the goal"
        );
        assert_eq!(
            shooter_goals(&mut shooter(None, mirrored), &mut keeper, seed),
            0,
            "seed {seed}"
        );
    }
}

fn jitter(rng: &mut ChaCha8Rng) -> f64 {
    use rand_chacha::rand_core::RngCore;
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

#[test]
fn the_scripted_gate_pays_only_for_a_fake_timed_inside_the_commit() {
    let limit = Params::default().half_width - Params::default().puck_radius;
    let mut goals = [0u32; 4];
    let seeds: Vec<u64> = (60_000..60_080).step_by(2).collect();
    for &seed in &seeds {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let sign = if jitter(&mut rng) < 0.5 { 1.0 } else { -1.0 };
        let corner = sign * (0.10 + 0.04 * jitter(&mut rng));
        let show = -sign * (0.2 + 0.2 * jitter(&mut rng));
        let bank = if corner > 0.0 {
            -2.0 * limit - corner
        } else {
            2.0 * limit - corner
        };
        let cases = [
            Shooter {
                me: 0,
                show_y: Some(show),
                shot_y: corner,
                strike_at: 70,
            },
            Shooter {
                me: 0,
                show_y: Some(show),
                shot_y: corner,
                strike_at: 400,
            },
            Shooter {
                me: 0,
                show_y: None,
                shot_y: corner,
                strike_at: 70,
            },
            Shooter {
                me: 0,
                show_y: None,
                shot_y: bank,
                strike_at: 70,
            },
        ];
        for (k, mut case) in cases.into_iter().enumerate() {
            let mut keeper = Keeper::new(Params::default(), 1000);
            let mut game = AirHockey::new(Params::default(), 0);
            let ticks = case.strike_at + 300;
            let report =
                run_match(&mut game, [&mut case, &mut keeper], header(seed), ticks).unwrap();
            goals[k] += u32::from(report.score[0] > 0);
        }
    }
    let n = seeds.len() as u32;
    eprintln!(
        "of {n}: timed fakes {} late fakes {} honest {} bank {}",
        goals[0], goals[1], goals[2], goals[3]
    );
    assert!(goals[0] * 10 >= n * 8, "timed fakes {}", goals[0]);
    assert!(goals[1] * 10 <= n, "late fakes {}", goals[1]);
    assert!(goals[2] * 10 <= n, "honest {}", goals[2]);
    assert!(goals[3] * 10 <= n, "bank {}", goals[3]);
}
