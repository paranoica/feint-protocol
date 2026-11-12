use feint_compare::detect::shots;
use feint_compare::play::human_params;
use feint_compare::trained::{Link, TrainedReport, pick, play_trained, trained_report, wilson};

#[test]
fn wilson_matches_a_worked_example() {
    let (lo, hi) = wilson(115, 200).unwrap();
    assert!((lo - 0.5057).abs() < 1e-4, "{lo}");
    assert!((hi - 0.6415).abs() < 1e-4, "{hi}");
    assert_eq!(wilson(0, 0), None);
    let (lo, _) = wilson(0, 10).unwrap();
    assert!(lo.abs() < 1e-12, "{lo}");
    let (_, hi) = wilson(10, 10).unwrap();
    assert!((hi - 1.0).abs() < 1e-12, "{hi}");
    for n in 1..=60 {
        for k in 0..=n {
            let (lo, hi) = wilson(k, n).unwrap();
            assert!(
                (0.0..=1.0).contains(&lo) && (0.0..=1.0).contains(&hi),
                "{k}/{n}: {lo} {hi}"
            );
        }
    }
}

#[test]
fn links_are_three_scored_first_then_by_seed_and_tick() {
    let l = |seed, contact, scored| Link {
        seed,
        seat: (seed % 2) as u8,
        contact,
        scored,
    };
    let picked = pick(vec![
        l(5, 10, false),
        l(2, 90, false),
        l(9, 1, true),
        l(2, 30, false),
        l(7, 4, true),
    ]);
    assert_eq!(picked, vec![l(7, 4, true), l(9, 1, true), l(2, 30, false)]);
    assert!(pick(Vec::new()).is_empty());
}

#[test]
fn trained_report_names_three_links_or_none() {
    let seeds = 30_029..30_032;
    let r = trained_report(5, seeds.clone());
    assert_eq!(r.matches, 3);
    assert_eq!(r.shots, r.fakes + r.honest);
    assert!(r.fake_scored <= r.fakes && r.honest_scored <= r.honest);
    assert_eq!(r.links.len(), r.fakes.min(3) as usize, "{r:?}");
    assert!(
        !r.links.is_empty(),
        "this seed range holds a detected fake: {r:?}"
    );
    let mine: u32 = seeds
        .clone()
        .map(|seed| {
            let seat = (seed % 2) as u8;
            let played = play_trained(5, seed, seat);
            shots(&human_params(), &played.frames)
                .iter()
                .filter(|s| s.player == seat)
                .count() as u32
        })
        .sum();
    assert_eq!(r.shots, mine, "only the trained bot's shots count");
    let mut seen_unscored = false;
    for link in &r.links {
        assert!(seeds.contains(&link.seed));
        assert_eq!(u64::from(link.seat), link.seed % 2);
        assert!(
            !(seen_unscored && link.scored),
            "scored fakes come first: {r:?}"
        );
        seen_unscored |= !link.scored;
        let played = play_trained(5, link.seed, link.seat);
        let shot = shots(&human_params(), &played.frames)
            .into_iter()
            .find(|s| s.player == link.seat && s.contact == link.contact)
            .unwrap_or_else(|| panic!("no shot at the link {link:?}"));
        assert!(shot.fake(), "{shot:?}");
        assert_eq!(shot.scored, link.scored);
    }
}

#[test]
fn json_names_counts_rates_and_links() {
    let r = TrainedReport {
        level: 5,
        checkpoint: 0x1517_1d6c_57a0_caa7,
        matches: 200,
        noise: 0.49,
        shots: 30,
        fakes: 10,
        fake_scored: 0,
        honest: 20,
        honest_scored: 20,
        links: vec![Link {
            seed: 30_001,
            seat: 1,
            contact: 4_321,
            scored: false,
        }],
    };
    assert_eq!(
        r.to_json(),
        "{\"level\":5,\"checkpoint\":\"15171d6c57a0caa7\",\"matches\":200,\"noise\":0.4900,\"shots\":30,\
\"fake\":{\"count\":10,\"scored\":0,\"low\":0.0000,\"high\":0.2775},\
\"honest\":{\"count\":20,\"scored\":20,\"low\":0.8389,\"high\":1.0000},\
\"links\":[{\"seed\":30001,\"seat\":1,\"from\":4321,\"scored\":false}]}"
    );
}

#[test]
fn json_of_a_bot_without_fakes_has_no_rate_and_no_links() {
    let r = TrainedReport {
        level: 5,
        checkpoint: 1,
        matches: 2,
        noise: 0.0,
        shots: 0,
        fakes: 0,
        fake_scored: 0,
        honest: 0,
        honest_scored: 0,
        links: Vec::new(),
    };
    assert_eq!(
        r.to_json(),
        "{\"level\":5,\"checkpoint\":\"0000000000000001\",\"matches\":2,\"noise\":0.0000,\"shots\":0,\
\"fake\":{\"count\":0,\"scored\":0,\"low\":null,\"high\":null},\
\"honest\":{\"count\":0,\"scored\":0,\"low\":null,\"high\":null},\
\"links\":[]}"
    );
}
