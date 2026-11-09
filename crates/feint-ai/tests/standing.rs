use feint_ai::{RunHeader, RunReport, Standing};
use feint_protocol::ActionT;

fn action(player: u8, deceptive: bool) -> ActionT {
    let mut a = ActionT::default();
    a.player = player;
    a.deceptive = deceptive;
    a
}

fn report(score: [i64; 2], finished: bool, actions: Vec<ActionT>) -> RunReport {
    RunReport {
        header: RunHeader {
            game_id: "test".into(),
            game_version: 1,
            seed: 0,
            bodies: vec![],
            levels: [1, 1],
        },
        score: score.to_vec(),
        ticks: 10,
        finished,
        actions,
        digest: 0,
    }
}

#[test]
fn a_standing_counts_wins_points_and_each_fake_once_for_its_own_seat() {
    let mut s = Standing::default();
    let fakes = vec![
        action(1, true),
        action(0, true),
        action(1, true),
        action(1, false),
        action(0, false),
        action(1, true),
    ];
    s.add(&report([3, 7], true, fakes), 1);
    assert_eq!(
        (s.matches, s.won, s.mine, s.theirs, s.fakes),
        (1, 1, 7, 3, 2)
    );
    s.add(&report([5, 5], true, vec![]), 0);
    s.add(&report([9, 1], false, vec![]), 0);
    assert_eq!(
        (s.matches, s.won, s.mine, s.theirs),
        (3, 1, 21, 9),
        "a level or unfinished match is no win"
    );
    assert!((s.share() - 21.0 / 30.0).abs() < 1e-12);
    assert!((s.fakes_per_match() - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(Standing::default().share(), 0.0);
    assert_eq!(Standing::default().fakes_per_match(), 0.0);
}
