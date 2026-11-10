pub mod batch;
mod pilot;

pub use batch::{BatchError, BatchSpec, Episode, Fed, HockeyBatch, Outputs, Seat};
pub use pilot::{EXTRA_FEATURES, Pilot, Policy};
