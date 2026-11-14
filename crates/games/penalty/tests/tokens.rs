use penalty::context::{FOLLOW, GUESS, KickToken, SHOT, TELL};
use penalty::persons::{Advice, Habits, match_tokens};

const HONEST: i8 = 0;
const FAKE: i8 = 1;
const NO_LEAN: i8 = 2;

fn left_honest_guesser() -> Habits {
    Habits {
        corner: [1.0, 0.0, 0.0],
        honest: 1.0,
        fake: 0.0,
        straighten: 0.0,
        guess: 1.0,
        follows_lean: false,
        noise: 0.5,
    }
}

fn shooting(tokens: &[KickToken]) -> impl Iterator<Item = &KickToken> {
    tokens.iter().filter(|t| t.human_shoots())
}

#[test]
fn a_habitual_person_reads_back_from_the_tokens() {
    for seat in [0, 1] {
        let tokens = match_tokens(4, Advice::Blind, left_honest_guesser(), seat, 7);
        assert_eq!(tokens.len(), 10, "seat {seat}");
        for (k, t) in tokens.iter().enumerate() {
            assert_eq!(t.human_shoots(), (k % 2 == 0) == (seat == 1), "kick {k}");
            assert_eq!(t.features[1], k as f32 / 10.0);
        }
        for t in shooting(&tokens) {
            assert_eq!(t.target[SHOT], 0, "{t:?}");
            assert!(matches!(t.target[TELL], HONEST | NO_LEAN), "{t:?}");
            assert_eq!(t.target[GUESS], -1);
            assert_eq!(t.target[FOLLOW], -1);
        }
        assert!(
            shooting(&tokens).any(|t| t.target[TELL] == HONEST),
            "an honest leaner shows its lean at least once"
        );
        let keeping: Vec<_> = tokens.iter().filter(|t| !t.human_shoots()).collect();
        assert_eq!(keeping.len(), 5);
        for t in &keeping {
            assert_eq!((t.target[SHOT], t.target[TELL]), (-1, -1), "{t:?}");
            assert!(
                matches!(t.target[FOLLOW], -1 | 0),
                "dives against the lean: {t:?}"
            );
            assert!(t.target[FOLLOW] == -1 || t.target[GUESS] == 1, "{t:?}");
        }
    }
    let mut early = 0;
    for seed in 0..20 {
        let tokens = match_tokens(4, Advice::Blind, left_honest_guesser(), 0, seed);
        early += tokens
            .iter()
            .filter(|t| !t.human_shoots() && t.target[GUESS] == 1)
            .count();
    }
    assert!(
        early > 0,
        "a keeper who always guesses dives early when it sees the lean in time"
    );
}

#[test]
fn a_liar_who_never_guesses_reads_back_too() {
    let liar = Habits {
        corner: [0.0, 0.0, 1.0],
        honest: 0.0,
        fake: 1.0,
        guess: 0.0,
        ..left_honest_guesser()
    };
    let tokens = match_tokens(4, Advice::Blind, liar, 0, 3);
    for t in shooting(&tokens) {
        assert_eq!(t.target[SHOT], 2, "{t:?}");
        assert!(matches!(t.target[TELL], FAKE | NO_LEAN), "{t:?}");
    }
    assert!(shooting(&tokens).any(|t| t.target[TELL] == FAKE));
    for t in tokens.iter().filter(|t| !t.human_shoots()) {
        assert!(
            matches!(t.target[GUESS], -1 | 0),
            "never dives before the kick: {t:?}"
        );
        assert_eq!(t.target[FOLLOW], -1);
    }
}
