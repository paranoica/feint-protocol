use feint_ai::{Controller, Difficulty, Opponent, RunHeader, Runner};
use feint_protocol::{ActionT, GameAdapter, GameSpecT, ObservationT, PlayerId};
use penalty::human::{Human, human_body};
use penalty::persons::{Habits, measured_params};
use penalty::styles::{KeeperStyle, KickSeen, Mind, ShooterStyle};
use penalty::{Penalty, PenaltyPlaybook, Phase, Shootout};

struct Script {
    me: PlayerId,
    plan: Vec<(f32, f32)>,
}

impl Controller<Penalty> for Script {
    fn new_match(&mut self, _spec: &GameSpecT, me: PlayerId, _seed: u64) {
        self.me = me;
    }

    fn on_tick(&mut self, _game: &Penalty, obs: &ObservationT) -> Option<ActionT> {
        let v = obs.state.as_ref()?;
        let (keeper, phase, dist, kicks) = (v[0] >= 0.5, v[1], v[3], v[13] as usize);
        let mut a = ActionT::default();
        a.tick = obs.tick;
        a.player = self.me;
        let values = if keeper {
            vec![0.0, 0.0, 0.0, 0.0]
        } else {
            let (lean, aim) = self.plan[(kicks / 2).min(self.plan.len() - 1)];
            let fire = phase == 1.0 || dist < 0.2;
            vec![lean, 1.0, f32::from(u8::from(fire)), aim]
        };
        a.values = Some(values);
        Some(a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Truth {
    bot_shot: bool,
    lean: Option<u8>,
    shot: Option<u8>,
    dive: Option<u8>,
    goal: bool,
}

fn lean_class(lean: f64) -> Option<u8> {
    (lean.abs() > 0.3).then_some(if lean < 0.0 { 0 } else { 2 })
}

fn play(mind: Mind, plan: Vec<(f32, f32)>, seed: u64) -> (Vec<KickSeen>, Vec<Truth>) {
    let params = measured_params();
    let mut game = Penalty::new(params, 4);
    let spec = game.spec();
    let d = Difficulty::preset(4).unwrap_or_else(|| panic!("level 4"));
    let head = RunHeader {
        game_id: "penalty".into(),
        game_version: penalty::GAME_VERSION,
        seed,
        bodies: vec![d.body(&spec), human_body(&game)],
        levels: [4, 0],
        opponent: Opponent::HandMade,
    };
    let mut bot = Script { me: 0, plan };
    let mut human = Human::with_mind(mind);
    let mut runner = Runner::start(&mut game, [&mut bot, &mut human], head)
        .unwrap_or_else(|e| panic!("a match starts: {e:?}"));
    let mut truths = Vec::new();
    let mut start: Shootout = game.live().clone();
    let mut lean = None;
    let mut struck = false;
    let mut shot = None;
    let mut dive = None;
    for _ in 0..240 * 900 {
        if !runner.step(&mut game, [&mut bot, &mut human]) {
            break;
        }
        let s = game.live();
        if s.kicks == start.kicks {
            if s.phase == Phase::Strike && !struck {
                struck = true;
                lean = lean_class(s.lean);
            }
            if let Some(aim) = s.aim {
                shot = Some(PenaltyPlaybook::class_of(aim));
            }
            if s.dived && dive.is_none() {
                dive = s.dive.map(|d| PenaltyPlaybook::class_of(d.target));
            }
            continue;
        }
        let before = game.live().clone();
        let shooter = usize::from(start.shooter());
        truths.push(Truth {
            bot_shot: shooter == 0,
            lean,
            shot,
            dive,
            goal: before.score[shooter] > start.score[shooter],
        });
        start = before;
        lean = None;
        struck = false;
        shot = None;
        dive = None;
    }
    (human.seen().to_vec(), truths)
}

fn trusting() -> Mind {
    Mind::new(
        KeeperStyle::Trust {
            rate: 0.4,
            guess: 1.0,
            start: 0.5,
        },
        ShooterStyle::Static,
        Habits::default(),
    )
}

#[test]
fn the_person_sees_the_lean_and_the_outcome_of_every_kick_it_learns_from() {
    let plan = vec![
        (-0.9, -2.8),
        (0.9, 2.8),
        (-0.9, 2.8),
        (0.0, 0.0),
        (0.9, -2.8),
    ];
    for seed in 0..3 {
        let (seen, truths) = play(trusting(), plan.clone(), seed);
        assert!(seen.len() >= 9, "{seen:?}");
        for (s, t) in seen.iter().zip(&truths) {
            assert_eq!(s.person_shot, !t.bot_shot, "{seen:?} {truths:?}");
            assert_eq!(s.goal, t.goal, "{seen:?} {truths:?}");
            assert_eq!(s.shot, t.shot, "{seen:?} {truths:?}");
            assert_eq!(s.dive, t.dive, "{seen:?} {truths:?}");
            if t.bot_shot {
                assert_eq!(s.lean, t.lean, "{seen:?} {truths:?}");
            }
        }
        let bot: Vec<&KickSeen> = seen.iter().filter(|k| !k.person_shot).collect();
        let expect = [
            (Some(0), 0),
            (Some(2), 2),
            (Some(0), 2),
            (None, 1),
            (Some(2), 0),
        ];
        for (k, (lean, shot)) in bot.iter().zip(expect) {
            assert_eq!(k.lean, lean, "{bot:?}");
            assert_eq!(k.shot, Some(shot), "{bot:?}");
        }
    }
}

fn with_lean_on_the_fifth(plan: Vec<(f32, f32)>, matches: u64) -> f64 {
    let mut with = 0u32;
    for seed in 0..matches {
        let (seen, _) = play(trusting(), plan.clone(), 1_000 + seed);
        let fifth = seen.iter().filter(|k| !k.person_shot).nth(4);
        if fifth.is_some_and(|k| k.dive == Some(0)) {
            with += 1;
        }
    }
    f64::from(with) / matches as f64
}

#[test]
fn a_trusting_keeper_dives_with_the_lean_after_honest_kicks_and_against_it_after_fakes() {
    let honest = vec![
        (-0.9, -2.8),
        (0.9, 2.8),
        (-0.9, -2.8),
        (0.9, 2.8),
        (-0.9, 0.0),
    ];
    let fakes = vec![
        (-0.9, 2.8),
        (0.9, -2.8),
        (-0.9, 2.8),
        (0.9, -2.8),
        (-0.9, 0.0),
    ];
    let n = 120;
    let a = with_lean_on_the_fifth(honest, n);
    let b = with_lean_on_the_fifth(fakes, n);
    let up = 1.0 - 0.5 * 0.6f64 * 0.6 * 0.6 * 0.6;
    let down = 0.5 * 0.6f64 * 0.6 * 0.6 * 0.6;
    let se = |p: f64| (p * (1.0 - p) / n as f64).sqrt().max(0.01);
    assert!(
        (a - up * up).abs() < 4.0 * se(up * up),
        "{a} vs {}",
        up * up
    );
    assert!(
        (b - down * down).abs() < 4.0 * se(down * down),
        "{b} vs {}",
        down * down
    );
}
