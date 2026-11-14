use std::sync::Arc;

use feint_context::{ContextError, ContextModel};

use crate::context::{ContextAdvisor, LAYOUT};

#[derive(Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub name: &'static str,
    pub id: u64,
    pub bytes: &'static [u8],
}

pub const SHIPPED: u64 = 0xb3d3_9ebc_d442_71df;

pub const CHECKPOINTS: &[Checkpoint] = &[Checkpoint {
    name: "context-2",
    id: SHIPPED,
    bytes: include_bytes!(concat!(env!("OUT_DIR"), "/context-2.safetensors")),
}];

#[derive(Debug, Clone, PartialEq)]
pub enum ContextCheckpointError {
    Unknown,
    Model(ContextError),
}

#[must_use]
pub fn lookup(id: u64) -> Option<&'static Checkpoint> {
    CHECKPOINTS.iter().find(|c| c.id == id)
}

pub fn model(id: u64) -> Result<ContextModel, ContextCheckpointError> {
    let c = lookup(id).ok_or(ContextCheckpointError::Unknown)?;
    ContextModel::from_safetensors(c.bytes, LAYOUT).map_err(ContextCheckpointError::Model)
}

pub fn advisor(id: u64) -> Result<ContextAdvisor, ContextCheckpointError> {
    Ok(ContextAdvisor::new(Arc::new(model(id)?)))
}
