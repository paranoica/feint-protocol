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

fn level_five(matches: u64, remember: bool) -> feint_ai::Standing {
    let d = Difficulty::preset(5).unwrap();
    against_human_with(
        matches,
        |spec: &GameSpecT, params: &Params, previous: Option<&HandMade>| {
            let mut bot = Bot::new(d, HockeyPlaybook::new(params.clone()), spec).unwrap();
            if let Some(profile) = previous
                .filter(|_| remember)
                .and_then(|p| p.profile("airhockey"))
            {
                bot.load_profile(&profile, "airhockey").unwrap();
            }
            (bot, d.body(spec))
        },
    )
}

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn level_five_against_the_human_model_carries_its_profile_from_match_to_match() {
    let with_profile = level_five(6, true);
    assert_ne!(
        with_profile,
        level_five(6, false),
        "the profile changes nothing in 6 matches"
    );
    assert_eq!(
        airhockey::human::against_human(Difficulty::preset(5).unwrap(), 6),
        with_profile
    );
}
