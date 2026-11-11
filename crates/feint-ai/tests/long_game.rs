use feint_ai::{Choice, DECISIVE, LongGame, PATTERN_BONUS, Plan};

fn strike(id: u16, shown: u8) -> Choice {
    Choice {
        id,
        intent: 1,
        deceptive: false,
        horizon_ticks: 60,
        param: [0.0; 2],
        shown: Some(shown),
        real: Some(shown),
    }
}

fn fake(id: u16, shown: u8, real: u8) -> Choice {
    Choice {
        deceptive: true,
        real: Some(real),
        ..strike(id, shown)
    }
}

fn defend(id: u16) -> Choice {
    Choice {
        shown: None,
        real: None,
        ..strike(id, 0)
    }
}

const CALM: f64 = 0.0;
const TENSE: f64 = 1.0;

#[test]
fn no_long_game_changes_nothing() {
    let mut plan = Plan::new(LongGame::None);
    plan.committed(&strike(1, 2), CALM);
    assert_eq!(plan.bonus(&strike(1, 2), CALM), 0.0);
    assert_eq!(plan.bonus(&fake(5, 2, 0), TENSE), 0.0);
    assert!(!plan.allows(&fake(5, 2, 0), TENSE));
    plan.rally();
    assert_eq!(plan.pace(), 1.0);
}

#[test]
fn the_first_calm_strike_becomes_the_pattern_and_breaks_when_it_matters() {
    let mut plan = Plan::new(LongGame::Simple);
    plan.committed(&defend(0), CALM);
    assert_eq!(
        plan.bonus(&strike(1, 2), CALM),
        0.0,
        "a defence is no pattern"
    );
    plan.committed(&strike(1, 2), CALM);
    plan.committed(&strike(2, 0), CALM);
    assert_eq!(plan.bonus(&strike(1, 2), CALM), PATTERN_BONUS);
    assert_eq!(
        plan.bonus(&strike(2, 0), CALM),
        0.0,
        "only the first strike is the pattern"
    );
    assert!(!plan.allows(&fake(5, 2, 0), CALM));

    let decisive = DECISIVE;
    assert_eq!(plan.bonus(&strike(1, 2), decisive), -PATTERN_BONUS);
    assert_eq!(plan.bonus(&fake(5, 2, 0), decisive), PATTERN_BONUS);
    assert_eq!(
        plan.bonus(&fake(6, 0, 2), decisive),
        0.0,
        "it must show the pattern's side"
    );
    assert!(plan.allows(&fake(5, 2, 0), decisive));
    assert!(!plan.allows(&fake(6, 0, 2), decisive));

    plan.committed(&fake(5, 2, 0), TENSE);
    assert_eq!(
        plan.bonus(&strike(1, 2), CALM),
        PATTERN_BONUS,
        "a simple long game keeps its one pattern"
    );
    plan.reset();
    assert_eq!(
        plan.bonus(&strike(1, 2), CALM),
        0.0,
        "a new match has no pattern yet"
    );
}

#[test]
fn a_full_long_game_seeds_a_new_pattern_after_each_break_and_alternates_pace() {
    let mut plan = Plan::new(LongGame::Full);
    plan.committed(&strike(1, 2), CALM);
    plan.committed(&fake(5, 2, 0), TENSE);
    assert_eq!(
        plan.bonus(&strike(1, 2), CALM),
        0.0,
        "the broken pattern is dropped"
    );
    plan.committed(&strike(3, 0), CALM);
    assert_eq!(plan.bonus(&strike(3, 0), CALM), PATTERN_BONUS);

    let mut paces = vec![plan.pace()];
    for _ in 0..4 {
        plan.rally();
        paces.push(plan.pace());
    }
    assert_eq!(paces, vec![1.0, 0.0, 1.0, 0.0, 1.0]);
    plan.reset();
    assert_eq!(plan.pace(), 1.0);
}

mod line_game;
mod penalty_game;

use feint_ai::{Bot, Controller, Difficulty, Memory, RunHeader, run_match};
use feint_protocol::GameAdapter;
use line_game::{Line, LinePlaybook, line_spec};
use penalty_game::{Keeper, KeeperRule, Penalty, PenaltyPlaybook, STRIKER, penalty_spec};

fn schemer(long_game: LongGame) -> Bot<Penalty, PenaltyPlaybook> {
    let mut d = Difficulty::preset(5).unwrap();
    d.depth = 2;
    d.deception = 0.0;
    d.memory = Memory::Match;
    d.spread = 0.0;
    d.long_game = long_game;
    Bot::new(d, PenaltyPlaybook::default(), &penalty_spec()).unwrap()
}

fn play(bot: &mut Bot<Penalty, PenaltyPlaybook>, rounds: u32) -> feint_ai::RunReport {
    play_against(bot, rounds, KeeperRule::Biter)
}

fn play_against(
    bot: &mut Bot<Penalty, PenaltyPlaybook>,
    rounds: u32,
    rule: KeeperRule,
) -> feint_ai::RunReport {
    let mut game = Penalty::new(rounds);
    let mut keeper = Keeper::new(rule);
    let header = RunHeader {
        game_id: "penalty".into(),
        game_version: 1,
        seed: 3,
        bodies: vec![],
        levels: [4, 0],
        opponent: feint_ai::Opponent::HandMade,
    };
    run_match(&mut game, [bot, &mut keeper], header, u64::from(rounds) * 4).unwrap()
}

#[test]
fn the_pattern_breaks_on_the_decisive_kick_even_without_deception() {
    let mut simple = schemer(LongGame::Simple);
    let report = play(&mut simple, 12);
    let kicks: Vec<bool> = report
        .actions
        .iter()
        .filter(|a| a.player == STRIKER)
        .map(|a| a.deceptive)
        .collect();
    assert_eq!(kicks.len(), 12);
    assert_eq!(kicks.iter().filter(|&&d| d).count(), 1, "{kicks:?}");
    assert!(
        kicks[11],
        "the break must come on the last, decisive kick: {kicks:?}"
    );

    let mut plain = schemer(LongGame::None);
    let report = play(&mut plain, 12);
    assert!(
        report.actions.iter().all(|a| !a.deceptive),
        "no long game, no deception: no fakes"
    );
}

#[test]
fn a_full_long_game_alternates_the_pace_of_rallies() {
    let mut full = schemer(LongGame::Full);
    play(&mut full, 6);
    let paces = full.playbook().paces.borrow().clone();
    assert_eq!(paces, vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    let mut simple = schemer(LongGame::Simple);
    play(&mut simple, 6);
    assert!(simple.playbook().paces.borrow().iter().all(|&p| p == 1.0));
}

#[test]
fn the_pattern_is_repeated_while_the_score_is_calm() {
    let picks = |long_game: LongGame| {
        let mut d = Difficulty::preset(5).unwrap();
        d.depth = 1;
        d.deception = 0.0;
        d.reaction_ms = 0;
        d.anticipate = false;
        d.spread = 0.0;
        d.long_game = long_game;
        let mut playbook = LinePlaybook::new(&[-0.5, 0.5], 24);
        playbook.shown = true;
        playbook.flat = true;
        let mut bot = Bot::new(d, playbook, &line_spec(60)).unwrap();
        bot.new_match(&line_spec(60), 0, 11);
        let mut game = Line::new(0.2, 0.0);
        let mut events = Vec::new();
        let mut firsts = Vec::new();
        for _ in 0..20 {
            game.state.ball = 0.2;
            let mut first = None;
            for _ in 0..24 {
                let mut obs = feint_protocol::ObservationT::default();
                game.observe(&game.state, 0, &mut obs);
                if let Some(a) = bot.on_tick(&game, &obs) {
                    first.get_or_insert(a.values.clone().unwrap()[0]);
                    game.apply(&a).unwrap();
                }
                game.advance(&mut events);
            }
            firsts.push(first.expect("acts in every possession"));
            game.state.ball = -0.2;
            for _ in 0..40 {
                let mut obs = feint_protocol::ObservationT::default();
                game.observe(&game.state, 0, &mut obs);
                if let Some(a) = bot.on_tick(&game, &obs) {
                    game.apply(&a).unwrap();
                }
                game.advance(&mut events);
            }
        }
        firsts
    };
    let patterned = picks(LongGame::Simple);
    assert!(
        patterned.windows(2).all(|w| w[0] == w[1]),
        "the pattern was not repeated: {patterned:?}"
    );
    let free = picks(LongGame::None);
    assert!(
        free.windows(2).any(|w| w[0] != w[1]),
        "without a pattern each possession breaks the tie at random: {free:?}"
    );
}
