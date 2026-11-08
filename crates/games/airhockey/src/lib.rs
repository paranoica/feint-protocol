//! Air hockey for Feint: deterministic 2D physics and its game adapter (FNT-D4, FNT-D5).

pub mod game;
pub mod mallet;
pub mod params;
pub mod puck;
pub mod vec2;
pub mod world;

pub use game::{Input, InputError, Match, MatchEvent};
pub use mallet::{Mallet, Region, Side};
pub use params::{BadBody, Body, Params};
pub use puck::Puck;
pub use vec2::Vec2;
pub use world::{World, goal_crossing, puck_mallet, regions};
