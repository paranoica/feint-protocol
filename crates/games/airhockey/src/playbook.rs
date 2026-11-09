use feint_ai::{Choice, Playbook};
use feint_protocol::{ActionT, PlayerId};

use crate::adapter::AirHockey;
use crate::game::Match;
use crate::params::Params;
use crate::vec2::Vec2;

/// Aim points on the opponent's goal line, in the striker's frame: right, centre, left.
/// At ±0.07 the puck's edge stays 1.5 cm clear of the posts.
const AIM_Y: [f64; 3] = [-0.07, 0.0, 0.07];
/// Run-up behind the puck before a strike, beyond contact distance.
const RUN_UP: f64 = 0.1;
/// How close to the aim line the mallet must be before it strikes.
const LINE_TOLERANCE: f64 = 0.01;
/// How far in front of its goal's centre a defender stands, towards the puck.
const DEFENCE_DEPTH: f64 = 0.15;
/// Metres of aim error per unit of the core's aim noise.
const AIM_SCALE: f64 = 2.0;
/// A chase only needs to reach the puck: a quarter of the aim error.
const CHASE_SCALE: f64 = 0.5;

/// +1 when `player` attacks towards +x (player 0), −1 otherwise.
fn attack(player: PlayerId) -> f64 {
    if player == 0 { 1.0 } else { -1.0 }
}

/// World and player frames differ by a 180° rotation for player 1.
fn frame(player: PlayerId, v: Vec2) -> Vec2 {
    if player == 0 { v } else { -v }
}

fn unit(v: Vec2, fallback: Vec2) -> Vec2 {
    let length = v.length();
    if length > 0.0 {
        v * (1.0 / length)
    } else {
        fallback
    }
}

/// Hand-made hockey tactics for levels 1–2: chase, defend, strike a corner (FNT-D7).
pub struct HockeyPlaybook {
    params: Params,
}

impl HockeyPlaybook {
    pub const CHASE: u16 = 0;
    pub const DEFEND: u16 = 1;
    /// Strikes at the right corner, the centre and the left corner of the opponent's goal,
    /// as the striker sees it.
    pub const STRIKES: [u16; 3] = [2, 3, 4];

    #[must_use]
    pub fn new(params: Params) -> Self {
        Self { params }
    }

    /// The point between `player`'s goal and the puck where its mallet defends.
    fn defence_spot(&self, state: &Match, player: PlayerId) -> Vec2 {
        let goal = Vec2::new(-attack(player) * self.params.half_length, 0.0);
        let towards = unit(state.world.puck.pos - goal, Vec2::new(attack(player), 0.0));
        goal + towards * DEFENCE_DEPTH
    }

    /// A strike in two phases: get behind the puck exactly on the aim line, then drive
    /// through it, so the contact normal is the aim line itself.
    fn strike_target(&self, state: &Match, player: PlayerId, aim: Vec2) -> Vec2 {
        let puck = state.world.puck.pos;
        let mallet = state.world.mallets[usize::from(player.min(1))].pos;
        let line = unit(aim - puck, Vec2::new(attack(player), 0.0));
        let reach = self.params.puck_radius + self.params.mallet_radius;
        let rel = mallet - puck;
        let along = rel.dot(line);
        let lateral = (rel.x * line.y - rel.y * line.x).abs();
        if along < -reach && lateral < LINE_TOLERANCE {
            puck + line * 0.15
        } else {
            puck - line * (reach + RUN_UP)
        }
    }

    fn puck_in_half_of(state: &Match, player: PlayerId) -> bool {
        attack(player) * state.world.puck.pos.x < 0.0
    }

    /// An action sending `player`'s mallet to a world point, inside the action space.
    fn go_to(&self, player: PlayerId, world: Vec2) -> ActionT {
        let seen = frame(player, world);
        let (l, w) = (
            self.params.half_length as f32,
            self.params.half_width as f32,
        );
        let mut action = ActionT::default();
        action.player = player;
        action.values = Some(vec![
            (seen.x as f32).clamp(-l, l),
            (seen.y as f32).clamp(-w, w),
        ]);
        action
    }
}

impl Playbook<AirHockey> for HockeyPlaybook {
    fn options(&self, state: &Match, me: PlayerId, repertoire: u8, out: &mut Vec<Choice>) {
        out.clear();
        let choice = |id: u16, intent: u8, horizon_ticks: u32, k: f32| Choice {
            id,
            intent,
            deceptive: false,
            horizon_ticks,
            param: [k, 0.0],
        };
        if repertoire == 0 {
            out.push(choice(Self::CHASE, 1, 60, 0.0));
            return;
        }
        out.push(choice(Self::DEFEND, 2, 120, 0.0));
        // A strike needs the puck within reach: in the striker's own half.
        if Self::puck_in_half_of(state, me) {
            for (k, &id) in Self::STRIKES.iter().enumerate() {
                out.push(choice(id, 1, 240, k as f32));
            }
        }
    }

    fn act(&self, s: &Match, me: PlayerId, c: &Choice, _elapsed: u64, aim: [f64; 2]) -> ActionT {
        let noise = frame(me, Vec2::new(aim[0], aim[1]));
        let target = match c.id {
            Self::CHASE => s.world.puck.pos + noise * CHASE_SCALE,
            Self::DEFEND => self.defence_spot(s, me),
            _ => {
                let k = (c.param[0].max(0.0) as usize).min(AIM_Y.len() - 1);
                let seen = Vec2::new(self.params.half_length, AIM_Y[k] + aim[1] * AIM_SCALE);
                self.strike_target(s, me, frame(me, seen))
            }
        };
        self.go_to(me, target)
    }

    fn reply(&self, s: &Match, them: PlayerId) -> ActionT {
        let target = if Self::puck_in_half_of(s, them) {
            self.defence_spot(s, them)
        } else {
            s.world.mallets[usize::from(them.min(1))].pos
        };
        self.go_to(them, target)
    }

    /// Goals dominate (10 each); then the puck's position and speed towards the opponent,
    /// and danger near the own goal. Defending while the puck is away needs no term: then
    /// `defend` is the only option.
    fn evaluate(&self, s: &Match, me: PlayerId) -> f64 {
        let me_i = usize::from(me.min(1));
        let score = s.score();
        let goals = f64::from(score[me_i]) - f64::from(score[1 - me_i]);
        let towards = attack(me);
        let puck = s.world.puck;
        let length = self.params.half_length;
        let progress = 0.3 * towards * puck.pos.x / length;
        let speed = towards * puck.vel.x / self.params.puck_speed_cap;
        let closeness_to_own_goal = 1.0 - (towards * puck.pos.x + length) / (2.0 * length);
        let danger = if speed < 0.0 {
            -speed * 0.5 * closeness_to_own_goal
        } else {
            0.0
        };
        10.0 * goals + progress + 0.3 * speed - danger
    }

    fn margin(&self) -> f64 {
        0.1
    }
}
