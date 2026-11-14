use boxing::human::tally;
use feint_ai::Difficulty;

const MATCHES: u64 = 200;

#[test]
#[ignore = "release only: bedrock cmd strength"]
fn cancelled_wind_ups_against_the_human_model() {
    let t = tally(Difficulty::preset(5).unwrap(), MATCHES);
    let b = t.bites;
    let o = t.outcomes;
    let rate = |row: [u32; 2]| f64::from(row[1]) / f64::from(row[0].max(1));
    eprintln!(
        "level 5 over {MATCHES} matches: {} cancelled wind-ups, the human bit on {} ({:.2}); \
         strikes after a feint landed {} of {} ({:.2}), honest {} of {} ({:.2}); \
         the human scored {} points while the bot recovered from a cancel",
        t.feints,
        b.feint_bit,
        f64::from(b.feint_bit) / f64::from(b.feints.max(1)),
        o.feint[1],
        o.feint[0],
        rate(o.feint),
        o.honest[1],
        o.honest[0],
        rate(o.honest),
        o.conceded_open,
    );
    assert!(t.feints > 0, "level 5 feints");
    assert_eq!(b.feints, t.feints, "the human sees every cancel");
    assert!(
        rate(o.feint) > rate(o.honest),
        "a feint must open the guard"
    );
}
