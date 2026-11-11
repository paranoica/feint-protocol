use airhockey::{AirHockey, HockeyPlaybook, Mallet, Params, Puck, Vec2};
use feint_ai::{Bot, Controller, Difficulty, RunHeader, replay, run_match};
use feint_protocol::{BodyParamsT, GameAdapter, ObservationT};

type HockeyBot = Bot<AirHockey, HockeyPlaybook>;

fn bot(level: u8, game: &AirHockey, me: u8, seed: u64) -> HockeyBot {
    let d = Difficulty::preset(level).unwrap();
    let mut bot = Bot::new(d, HockeyPlaybook::new(Params::default()), &game.spec()).unwrap();
    bot.new_match(&game.spec(), me, seed);
    bot
}

fn bodies_for(game: &AirHockey, levels: [u8; 2]) -> Vec<BodyParamsT> {
    levels
        .iter()
        .map(|&l| Difficulty::preset(l).unwrap().body(&game.spec()))
        .collect()
}

fn game(seed: u64, levels: [u8; 2]) -> AirHockey {
    let mut game = AirHockey::new(Params::default(), levels[0]);
    let bodies = bodies_for(&game, levels);
    game.reset(seed, &bodies).unwrap();
    game
}

fn play(game: &mut AirHockey, bot: &mut HockeyBot, me: u8, ticks: u32) {
    let mut obs = ObservationT::default();
    let mut events = Vec::new();
    for _ in 0..ticks {
        game.observe(game.live(), me, &mut obs);
        if let Some(action) = bot.on_tick(game, &obs) {
            game.apply(&action).expect("the bot's action is legal");
        }
        game.advance(&mut events);
    }
}

fn side(me: u8, p: Vec2) -> Vec2 {
    if me == 0 { p } else { -p }
}

#[test]
fn level_one_drives_at_the_puck_and_touches_it() {
    for me in [0u8, 1] {
        let mut game = game(u64::from(me), [1, 1]);
        let mut level_one = bot(1, &game, me, 7);
        assert_eq!(game.live().world.puck.vel, Vec2::ZERO);
        play(&mut game, &mut level_one, me, 480);
        assert_ne!(
            game.live().world.puck.vel,
            Vec2::ZERO,
            "side {me}: puck never touched"
        );
    }
}

fn game_with_puck(puck: Vec2) -> AirHockey {
    let mut g = game(0, [2, 2]);
    g.game_mut().world.puck = Puck::at_rest(puck);
    g
}

#[test]
fn level_two_defends_when_the_puck_is_away() {
    for me in [0u8, 1] {
        let puck = side(me, Vec2::new(0.5, 0.2));
        let goal = side(me, Vec2::new(-1.0, 0.0));
        let dir = (puck - goal) * (1.0 / (puck - goal).length());
        let spot = goal + dir * 0.15;
        let preset = Difficulty::preset(2).unwrap();
        let error = 2.0 * 2.0f64.sqrt() * preset.spread;
        for (spread, slack) in [(0.0, 0.05), (preset.spread, 0.05 + error)] {
            let mut g = game_with_puck(puck);
            let d = Difficulty { spread, ..preset };
            let mut level_two =
                Bot::new(d, HockeyPlaybook::new(Params::default()), &g.spec()).unwrap();
            level_two.new_match(&g.spec(), me, 3);
            play(&mut g, &mut level_two, me, 480);
            let mallet = g.live().world.mallets[usize::from(me)].pos;
            assert!(
                (mallet - spot).length() < slack,
                "side {me}, spread {spread}: {mallet:?} vs {spot:?}"
            );
            assert_eq!(
                level_two.current().map(|c| c.id),
                Some(HockeyPlaybook::DEFEND)
            );
        }
    }
}

#[test]
fn level_two_picks_whichever_corner_is_open() {
    for me in [0u8, 1] {
        let mut game = game(0, [2, 2]);
        {
            let world = &mut game.game_mut().world;
            world.puck = Puck::at_rest(side(me, Vec2::new(-0.4, 0.0)));
            world.mallets[usize::from(me)] = Mallet::at_rest(side(me, Vec2::new(-0.8, 0.0)));
            world.mallets[usize::from(1 - me)] = Mallet::at_rest(side(me, Vec2::new(0.92, -0.08)));
        }
        let mut level_two = bot(2, &game, me, 5);
        play(&mut game, &mut level_two, me, 73);
        assert_eq!(
            level_two.current().map(|c| c.id),
            Some(HockeyPlaybook::STRIKES[2]),
            "side {me}: should aim at the open left corner"
        );
    }
}

#[test]
fn level_two_strikes_away_from_the_covering_mallet() {
    for me in [0u8, 1] {
        let mut game = game(0, [2, 2]);
        let them = usize::from(1 - me);
        {
            let world = &mut game.game_mut().world;
            world.puck = Puck::at_rest(side(me, Vec2::new(-0.4, 0.0)));
            world.mallets[usize::from(me)] = Mallet::at_rest(side(me, Vec2::new(-0.8, 0.0)));
            world.mallets[them] = Mallet::at_rest(side(me, Vec2::new(0.92, 0.08)));
        }
        let mut level_two = bot(2, &game, me, 5);
        let mut accurate = {
            let mut d = Difficulty::preset(2).unwrap();
            d.spread = 0.0;
            let mut b = Bot::new(d, HockeyPlaybook::new(Params::default()), &game.spec()).unwrap();
            b.new_match(&game.spec(), me, 5);
            b
        };
        let mut twin = game.live().clone();
        play(&mut game, &mut level_two, me, 73);
        assert_eq!(
            level_two.current().map(|c| c.id),
            Some(HockeyPlaybook::STRIKES[0]),
            "side {me}: should aim at the open right corner"
        );
        std::mem::swap(game.game_mut(), &mut twin);
        play(&mut game, &mut accurate, me, 73 + 240);
        assert_eq!(
            game.live().score()[usize::from(me)],
            1,
            "side {me}: the open corner should score"
        );
    }
}

#[test]
fn bots_keep_their_level_body() {
    let mut game = game(0, [1, 5]);
    let bodies = game.live().bodies;
    let limits = Params::default();
    let low_speed = f64::from((0.4 * limits.max_body_speed) as f32);
    assert_eq!(
        (bodies[0].max_speed(), bodies[0].max_accel()),
        (low_speed, 0.4 * limits.max_body_accel)
    );
    assert_eq!((bodies[1].max_speed(), bodies[1].max_accel()), (7.0, 120.0));
    let mut low = bot(1, &game, 0, 1);
    let mut high = bot(5, &game, 1, 1);
    let mut obs = [ObservationT::default(), ObservationT::default()];
    let mut events = Vec::new();
    let mut top = [0.0f64; 2];
    let limits = [bodies[0].max_accel(), bodies[1].max_accel()];
    let reach = Params::default().puck_radius + Params::default().mallet_radius;
    let dt = 1.0 / 240.0;
    let mut near_until = [0u64; 2];
    for _ in 0..240 * 180 {
        if game.live().is_over() {
            break;
        }
        let before = game.live().world;
        for (me, bot) in [(0u8, &mut low), (1, &mut high)] {
            game.observe(game.live(), me, &mut obs[usize::from(me)]);
            if let Some(action) = bot.on_tick(&game, &obs[usize::from(me)]) {
                let _ = game.apply(&action);
            }
        }
        game.advance(&mut events);
        let after = game.live().world;
        for i in 0..2 {
            top[i] = top[i].max(after.mallets[i].vel.length());
            let clear =
                |w: &airhockey::World| (w.puck.pos - w.mallets[i].pos).length() > reach + 0.1;
            let tick = game.live().tick();
            if !clear(&before) || !clear(&after) {
                near_until[i] = tick + 24;
            }
            if tick > near_until[i] {
                let dv = (after.mallets[i].vel - before.mallets[i].vel).length();
                assert!(
                    dv <= limits[i] * dt * (1.0 + 1e-9),
                    "side {i}: {} m/s² away from the puck",
                    dv / dt
                );
            }
        }
    }
    assert!(
        top[0] <= low_speed + 1e-9,
        "level 1 mallet at {} m/s",
        top[0]
    );
    assert!(top[1] <= 7.0 + 1e-9, "level 5 mallet at {} m/s", top[1]);
    assert!(
        top[0] > 1.0 && top[1] > 1.0,
        "the bots barely moved: {top:?}"
    );
}

#[test]
fn the_human_gets_the_declared_maxima_by_default() {
    let game = AirHockey::new(Params::default(), 1);
    let human = game.live().bodies[0];
    assert_eq!((human.max_speed(), human.max_accel()), (7.0, 120.0));
}

#[test]
fn hockey_bot_match_replays_from_its_record() {
    let mut game = AirHockey::new(Params::default(), 2);
    let header = RunHeader {
        game_id: "airhockey".into(),
        game_version: airhockey::GAME_VERSION,
        seed: 11,
        bodies: bodies_for(&game, [2, 1]),
        levels: [2, 1],
        opponent: feint_ai::Opponent::HandMade,
    };
    let (mut a, mut b) = (bot(2, &game, 0, 0), bot(1, &game, 1, 0));
    let report = run_match(&mut game, [&mut a, &mut b], header, 240 * 20).unwrap();
    assert!(report.actions.len() > 1_000);
    let mut fresh = AirHockey::new(Params::default(), 2);
    assert_eq!(replay(&mut fresh, &report).unwrap(), report.digest);
}
