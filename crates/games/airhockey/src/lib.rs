//! Air hockey for Feint: deterministic 2D physics and its game adapter (FNT-D4, FNT-D5).

pub mod mallet;
pub mod params;
pub mod vec2;

pub use mallet::{Mallet, Region, Side};
pub use params::{BadBody, Body, Params};
pub use vec2::Vec2;
