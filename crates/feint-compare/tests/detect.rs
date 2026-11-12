use airhockey::analysis::Frame;
use airhockey::{Params, Vec2};
use feint_compare::detect::{Shot, shots};

const TOUCH: f64 = 0.12;
const WINDUP_BACK: f64 = 0.32;
const SHOT_SPEED: f64 = 3.0;

struct Film {
    frames: Vec<Frame>,
    flip: bool,
    score: [u32; 2],
}

impl Film {
    fn new(flip: bool) -> Self {
        Self {
            frames: Vec::new(),
            flip,
            score: [0, 0],
        }
    }

    fn put(&mut self, v: Vec2) -> Vec2 {
        if self.flip { Vec2::new(-v.x, -v.y) } else { v }
    }

    fn push(&mut self, puck: Vec2, mallet: Vec2, score: [u32; 2]) {
        self.score = score;
        let tick = self.frames.len() as u64;
        let far = self.put(Vec2::new(0.9, -0.4));
        let (puck, mallet) = (self.put(puck), self.put(mallet));
        let (mallets, score) = if self.flip {
            ([far, mallet], [score[1], score[0]])
        } else {
            ([mallet, far], score)
        };
        self.frames.push(Frame {
            tick,
            puck,
            mallets,
            score,
        });
    }

    fn hold(&mut self, ticks: usize, puck: Vec2, mallet: Vec2) {
        for _ in 0..ticks {
            self.push(puck, mallet, self.score);
        }
    }

    fn strike(&mut self, puck: Vec2, aim_y: f64, ticks: usize) -> Vec2 {
        let dir = unit(Vec2::new(1.0 - puck.x, aim_y - puck.y));
        let mallet = puck - dir * TOUCH;
        self.push(puck, mallet, self.score);
        let step = SHOT_SPEED / 240.0;
        let mut at = puck;
        for _ in 0..ticks {
            at += dir * step;
            self.push(at, mallet, self.score);
        }
        at
    }

    fn player(&self) -> u8 {
        u8::from(self.flip)
    }
}

fn unit(v: Vec2) -> Vec2 {
    v * (1.0 / v.length())
}

fn behind(puck: Vec2, aim_y: f64, back: f64) -> Vec2 {
    puck - unit(Vec2::new(1.0 - puck.x, aim_y - puck.y)) * back
}

fn found(film: &Film) -> Vec<Shot> {
    shots(&Params::default(), &film.frames)
}

const PUCK: Vec2 = Vec2::new(-0.5, 0.0);

fn fake_film(flip: bool, show_ticks: usize) -> Film {
    let mut f = Film::new(flip);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    f.hold(show_ticks, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.strike(PUCK, 0.084, 60);
    f
}

#[test]
fn a_straight_shot_is_honest() {
    let mut f = Film::new(false);
    f.hold(120, PUCK, behind(PUCK, 0.084, WINDUP_BACK));
    f.strike(PUCK, 0.084, 60);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].player, 0);
    assert_eq!(s[0].contact, 120);
    assert_eq!(s[0].shown, None);
    assert_eq!(s[0].realised, 2);
    assert!(!s[0].fake());
}

#[test]
fn a_wide_show_then_the_other_corner_is_a_fake() {
    for flip in [false, true] {
        let f = fake_film(flip, 96);
        let s = found(&f);
        assert_eq!(s.len(), 1, "flip {flip}: {s:?}");
        assert_eq!(s[0].player, f.player());
        assert_eq!(s[0].contact, 126);
        assert_eq!(
            s[0].shown,
            Some(0),
            "the aim line meets the goal line at y = -0.3"
        );
        assert_eq!(s[0].realised, 2, "the puck crosses at y = +0.084");
        assert!(s[0].fake());
    }
}

#[test]
fn a_show_to_the_side_the_puck_goes_is_not_a_fake() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.strike(PUCK, -0.084, 60);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!((s[0].shown, s[0].realised), (Some(0), 0));
    assert!(!s[0].fake());
}

#[test]
fn a_show_shorter_than_the_hold_is_not_a_fake_and_the_hold_itself_is() {
    let s = found(&fake_film(false, 29));
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].shown, None, "29 ticks is under the 0.125 s hold");
    let s = found(&fake_film(false, 30));
    assert_eq!(s[0].shown, Some(0), "30 ticks is the 0.125 s hold");
}

#[test]
fn a_walk_round_the_puck_is_not_a_fake() {
    let mut f = Film::new(false);
    for k in 0..=96 {
        let t = 1.0 - f64::from(k) / 96.0;
        let d = 1.0 + t * t;
        let rel = Vec2::new(-(1.0 - t * t) / d, 2.0 * t / d) * WINDUP_BACK;
        f.push(PUCK, PUCK + rel, [0, 0]);
    }
    f.hold(30, PUCK, PUCK - Vec2::new(WINDUP_BACK, 0.0));
    f.strike(PUCK, 0.084, 60);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].shown, None,
        "the aim line sweeps the 0.15-0.45 m band in about 9 ticks"
    );
}

#[test]
fn a_bank_setup_beyond_the_rail_is_not_a_show() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.9, WINDUP_BACK));
    f.strike(PUCK, 0.084, 60);
    let s = found(&f);
    assert_eq!(
        s[0].shown, None,
        "an aim line meeting the goal line past the rail is a bank set-up"
    );
}

#[test]
fn a_bank_shot_crosses_on_the_folded_side() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    f.strike(PUCK, -1.2, 40);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].realised, 2,
        "aimed at y = -1.2 on the goal line: the rail at y = -0.45 is met at 3/8 of the run, the rest at 0.9 of the side speed crosses at y = +0.225"
    );
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    f.strike(PUCK, -0.97, 40);
    assert_eq!(
        found(&f)[0].realised,
        1,
        "aimed at y = -0.97: the rail takes a tenth of the side speed, so it crosses at y = 0.9 * 0.97 - 0.855 = +0.018, the centre (a rail without loss would give +0.07)"
    );
}

#[test]
fn a_slow_push_is_not_a_shot() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let dir = Vec2::new(1.0, 0.0);
    f.push(PUCK, PUCK - dir * TOUCH, [0, 0]);
    for k in 1..60 {
        f.push(
            PUCK + dir * (1.0 * f64::from(k) / 240.0),
            PUCK - dir * TOUCH,
            [0, 0],
        );
    }
    assert!(
        found(&f).is_empty(),
        "1 m/s is under the 1.4 m/s shot speed"
    );
}

#[test]
fn a_backward_hit_is_not_a_shot() {
    let mut f = Film::new(false);
    let dir = Vec2::new(-1.0, 0.0);
    f.hold(30, PUCK, Vec2::new(0.0, 0.3));
    f.push(PUCK, PUCK - dir * TOUCH, [0, 0]);
    for k in 1..60 {
        f.push(
            PUCK + dir * (3.0 * f64::from(k) / 240.0),
            PUCK - dir * TOUCH,
            [0, 0],
        );
    }
    assert!(found(&f).is_empty());
}

#[test]
fn a_goal_goes_to_the_last_shot_within_a_second() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let at = f.strike(PUCK, 0.084, 100);
    f.push(Vec2::ZERO, Vec2::new(-0.9, 0.0), [1, 0]);
    f.hold(10, Vec2::ZERO, Vec2::new(-0.9, 0.0));
    let s = found(&f);
    assert!(at.x > 0.7);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(
        s[0].scored,
        "the goal at tick 131 is 101 ticks after the contact"
    );
}

#[test]
fn a_goal_more_than_a_second_after_the_shot_credits_nobody() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let at = f.strike(PUCK, 0.084, 100);
    f.hold(140, at, Vec2::new(-0.9, 0.0));
    f.push(Vec2::ZERO, Vec2::new(-0.9, 0.0), [1, 0]);
    let s = found(&f);
    assert!(
        !s[0].scored,
        "the goal at tick 271 is 241 ticks after the contact"
    );
}

#[test]
fn a_puck_that_comes_back_credits_nobody() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let at = f.strike(PUCK, 0.084, 60);
    let back = Vec2::new(-SHOT_SPEED / 240.0, 0.0);
    let mut p = at;
    for _ in 0..20 {
        p += back;
        f.push(p, Vec2::new(-0.9, 0.0), [0, 0]);
    }
    f.push(Vec2::ZERO, Vec2::new(-0.9, 0.0), [1, 0]);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(!s[0].scored);
}

#[test]
fn a_goal_inside_the_read_window_keeps_the_shot_side() {
    let mut f = Film::new(false);
    let puck = Vec2::new(0.8, 0.0);
    f.hold(30, puck, Vec2::new(0.5, 0.0));
    f.strike(puck, 0.084, 4);
    f.push(Vec2::ZERO, Vec2::new(-0.9, 0.0), [1, 0]);
    f.hold(30, Vec2::ZERO, Vec2::new(-0.9, 0.0));
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].realised, 2,
        "the serve spot after the goal is not part of the shot"
    );
    assert!(s[0].scored);
}

#[test]
fn a_stall_serve_is_not_a_shot() {
    let mut f = Film::new(false);
    let mallet = PUCK - Vec2::new(TOUCH, 0.0);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    for k in 0..9 {
        f.push(PUCK + Vec2::new(f64::from(k) / 240.0, 0.0), mallet, [0, 0]);
    }
    f.hold(30, Vec2::new(0.5, 0.0), mallet);
    assert!(
        found(&f).is_empty(),
        "1 m/s before the puck is served to the far half; the jump is not flight"
    );
}

#[test]
fn a_contact_on_the_last_frames_is_not_read() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.strike(PUCK, 0.084, 2);
    assert!(
        found(&f).is_empty(),
        "one step after the puck leaves the mallet is too little"
    );
}

#[test]
fn no_frames_and_one_frame_give_no_shots() {
    assert!(shots(&Params::default(), &[]).is_empty());
    let mut f = Film::new(false);
    f.push(PUCK, PUCK - Vec2::new(TOUCH, 0.0), [0, 0]);
    assert!(found(&f).is_empty());
}

#[test]
fn the_opponent_touching_first_ends_the_read() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let dir = unit(Vec2::new(1.5, 0.084));
    f.push(PUCK, PUCK - dir * TOUCH, [0, 0]);
    let p1 = PUCK + dir * (SHOT_SPEED / 240.0);
    f.push(p1, PUCK - dir * TOUCH, [0, 0]);
    let mut frame = f.frames.last().copied().unwrap();
    frame.tick += 1;
    frame.mallets[1] = p1 + Vec2::new(TOUCH, 0.0);
    f.frames.push(frame);
    assert!(
        found(&f).is_empty(),
        "one step of flight is too little to read a side"
    );
}

#[test]
fn a_score_change_ends_the_read_even_without_a_jump() {
    let mut f = Film::new(false);
    let puck = Vec2::new(0.8, 0.0);
    f.hold(30, puck, Vec2::new(0.5, 0.0));
    let mut at = f.strike(puck, 0.084, 4);
    for _ in 0..8 {
        at += Vec2::new(0.0, -4.0 / 240.0);
        f.push(at, Vec2::new(-0.9, 0.0), [1, 0]);
    }
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].realised, 2,
        "frames after the goal do not bend the shot"
    );
}

#[test]
fn the_opponents_touch_ends_the_read() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    let at = f.strike(PUCK, 0.084, 3);
    let block = at + Vec2::new(TOUCH, 0.0);
    let mut p = at;
    for _ in 0..12 {
        let tick = f.frames.len() as u64;
        f.frames.push(Frame {
            tick,
            puck: p,
            mallets: [Vec2::new(-0.9, 0.0), block],
            score: [0, 0],
        });
        p += Vec2::new(-3.0 / 240.0, 0.0);
    }
    let s = found(&f);
    let mine: Vec<&Shot> = s.iter().filter(|s| s.player == 0).collect();
    assert_eq!(mine.len(), 1, "{s:?}");
    assert_eq!(
        mine[0].realised, 2,
        "the puck coming back off the block is not my shot"
    );
}

#[test]
fn a_fast_sideways_hit_is_not_a_shot() {
    let mut f = Film::new(false);
    let puck = Vec2::new(0.6, 0.0);
    let dir = unit(Vec2::new(0.4, 1.0));
    f.hold(30, puck, Vec2::new(0.3, 0.0));
    f.push(puck, puck - dir * TOUCH, [0, 0]);
    for k in 1..30 {
        f.push(
            puck + dir * (3.0 * f64::from(k) / 240.0),
            puck - dir * TOUCH,
            [0, 0],
        );
    }
    assert!(
        found(&f).is_empty(),
        "3 m/s mostly sideways is 1.11 m/s towards the goal, under the 1.4 m/s shot speed"
    );
}

#[test]
fn a_goal_from_a_contact_both_mallets_start_together_credits_nobody() {
    let mut f = Film::new(false);
    f.hold(30, PUCK, Vec2::new(-0.9, 0.0));
    f.strike(PUCK, 0.084, 100);
    f.push(Vec2::ZERO, Vec2::new(-0.9, 0.0), [1, 0]);
    f.frames[30].mallets[1] = PUCK + Vec2::new(0.0, -TOUCH);
    let s = found(&f);
    let mine: Vec<&Shot> = s.iter().filter(|s| s.player == 0).collect();
    assert_eq!(mine.len(), 1, "{s:?}");
    assert!(!mine[0].scored);
}

#[test]
fn a_mallet_beside_the_puck_shows_nothing() {
    let mut f = Film::new(false);
    let puck = Vec2::new(0.85, 0.0);
    f.hold(96, puck, puck + Vec2::new(-0.109, 0.301));
    f.strike(puck, 0.084, 10);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].shown, None,
        "70 degrees off the shot axis is beside, not behind, though its line meets the goal line at y = -0.41"
    );
}

#[test]
fn a_show_before_the_opponents_touch_does_not_count() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    let mut touch = f.frames.last().copied().unwrap();
    touch.tick += 1;
    touch.mallets[1] = PUCK + Vec2::new(TOUCH, 0.0);
    f.frames.push(touch);
    f.hold(20, PUCK, PUCK - Vec2::new(WINDUP_BACK, 0.0));
    f.strike(PUCK, 0.084, 30);
    let s = found(&f);
    let mine: Vec<&Shot> = s.iter().filter(|s| s.player == 0).collect();
    assert_eq!(mine.len(), 1, "{s:?}");
    assert_eq!(
        mine[0].shown, None,
        "the opponent's touch starts a new attack"
    );
    assert_eq!(mine[0].from, 96);
}

#[test]
fn a_glance_of_my_own_keeps_the_show() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.hold(1, PUCK, PUCK - Vec2::new(TOUCH, 0.0));
    f.hold(20, PUCK, PUCK - Vec2::new(WINDUP_BACK, 0.0));
    f.strike(PUCK, 0.084, 30);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].shown,
        Some(0),
        "a touch that is not a shot does not end my attack"
    );
    assert!(s[0].fake());
    assert_eq!(s[0].from, 0);
}

#[test]
fn my_previous_shot_ends_the_show_window() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    let at = f.strike(PUCK, -0.084, 20);
    f.hold(10, at, at - Vec2::new(WINDUP_BACK, 0.0));
    f.strike(at, 0.084, 30);
    let s = found(&f);
    assert_eq!(s.len(), 2, "{s:?}");
    assert_eq!(s[0].shown, Some(0));
    assert_eq!(
        s[1].shown, None,
        "the first shot closed the attack its show belonged to"
    );
    assert_eq!(s[1].from, 96);
}

#[test]
fn a_show_before_a_serve_does_not_count() {
    let mut f = Film::new(false);
    let elsewhere = Vec2::new(-0.2, 0.3);
    f.hold(96, elsewhere, behind(elsewhere, -0.3, WINDUP_BACK));
    f.hold(20, PUCK, PUCK - Vec2::new(WINDUP_BACK, 0.0));
    f.strike(PUCK, 0.084, 30);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].shown, None,
        "the puck jumped to the serve spot: a new point"
    );
    assert_eq!(s[0].from, 96);
}

#[test]
fn a_mallet_pressed_to_the_puck_shows_nothing() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, 0.21));
    f.strike(PUCK, 0.084, 30);
    assert_eq!(
        found(&f)[0].shown,
        None,
        "a show is a wind-up: 0.21 m is inside contact reach 0.12 plus 0.1"
    );
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.3, 0.23));
    f.strike(PUCK, 0.084, 30);
    assert_eq!(found(&f)[0].shown, Some(0), "0.23 m is a wind-up");
}

#[test]
fn the_serve_pause_is_not_a_show() {
    let mut f = Film::new(false);
    f.hold(10, Vec2::new(0.9, 0.3), Vec2::new(-0.9, 0.0));
    f.push(PUCK, behind(PUCK, -0.3, WINDUP_BACK), [0, 1]);
    f.hold(239, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.hold(29, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.strike(PUCK, 0.084, 30);
    let s = found(&f);
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(
        s[0].shown, None,
        "only 29 ticks of the show come after the 1 s pause"
    );
    let mut f = Film::new(false);
    f.hold(10, Vec2::new(0.9, 0.3), Vec2::new(-0.9, 0.0));
    f.push(PUCK, behind(PUCK, -0.3, WINDUP_BACK), [0, 1]);
    f.hold(239 + 30, PUCK, behind(PUCK, -0.3, WINDUP_BACK));
    f.strike(PUCK, 0.084, 30);
    assert_eq!(
        found(&f)[0].shown,
        Some(0),
        "30 ticks after the pause are a show"
    );
}

#[test]
fn a_show_is_an_aim_wider_than_the_mouth_edge() {
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.17, WINDUP_BACK));
    f.strike(PUCK, 0.084, 30);
    assert_eq!(
        found(&f)[0].shown,
        Some(0),
        "an aim at y = -0.17 is past the 0.15 line"
    );
    let mut f = Film::new(false);
    f.hold(96, PUCK, behind(PUCK, -0.14, WINDUP_BACK));
    f.strike(PUCK, 0.084, 30);
    assert_eq!(
        found(&f)[0].shown,
        None,
        "an aim at y = -0.14 is inside the mouth"
    );
}
