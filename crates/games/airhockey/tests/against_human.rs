use airhockey::human::{HUMAN_GOALS, against_human_with};
use airhockey::{AirHockey, HockeyPlaybook, Params};
use feint_ai::{Bot, Difficulty};
use feint_protocol::GameSpecT;

type HandMade = Bot<AirHockey, HockeyPlaybook>;

#[test]
fn each_match_gets_a_fresh_bot_that_sees_the_one_before() {
    let d = Difficulty::preset(1).unwrap();
    let mut seen = Vec::new();
    let standing = against_human_with(
        3,
        |spec: &GameSpecT, params: &Params, previous: Option<&HandMade>| {
            assert_eq!(params.goals_to_win, HUMAN_GOALS);
            seen.push(previous.is_some());
            let bot = Bot::new(d, HockeyPlaybook::new(params.clone()), spec).unwrap();
            (bot, d.body(spec))
        },
    );
    assert_eq!(standing.matches, 3);
    assert_eq!(seen, vec![false, true, true]);
}
