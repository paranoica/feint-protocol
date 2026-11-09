mod session;
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod wasm;

pub use session::{Pointer, Session, SessionError};
