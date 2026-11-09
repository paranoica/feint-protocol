use feint_ai::Profile;
use feint_web::Play;
use penalty::Penalty;

pub fn shootout(level: u8, seed: u64, profile: Option<&Profile>) -> Play<Penalty> {
    let mut game = Play::<Penalty>::with_profile(level, seed, profile).unwrap();
    let mut guard = 0;
    while !game.finished() {
        let v = game.view();
        if v[0] == 0.0 {
            let corner = if game.score()[0].is_multiple_of(2) { 2.8 } else { -2.8 };
            game.set_input(&[0.8, 1.0, 0.0, corner]);
            if v[1] == 0.0 && v[2] < 0.6 {
                game.push_input(&[0.8, 1.0, 1.0, corner]);
            }
        } else {
            game.set_input(&[0.0; 4]);
        }
        game.advance(4);
        guard += 1;
        assert!(guard < 240 * 60 * 5, "the shootout never ended");
    }
    game
}
