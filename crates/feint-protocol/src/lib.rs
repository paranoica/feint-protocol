//! Feint protocol v0.1: the message schema shared by every game and the core, and the
//! [`GameAdapter`] trait a game implements to be played by the core.

#[allow(
    clippy::all,
    clippy::pedantic,
    unsafe_code,
    unused_imports,
    dead_code,
    deprecated,
    non_camel_case_types,
    mismatched_lifetime_syntaxes
)]
#[rustfmt::skip]
#[path = "generated/feint_generated.rs"]
mod feint_generated;

mod adapter;

pub use adapter::{AdapterError, GameAdapter, PlayerId};
pub use feint_generated::feint::protocol::*;
pub use rand_chacha::ChaCha8Rng;

/// The protocol version every envelope carries; v0.x may break between minor versions.
pub const PROTOCOL_VERSION: &str = "0.1";

/// The largest message `decode` accepts.
pub const MAX_MESSAGE_BYTES: usize = 1 << 20;

/// Why a buffer could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Larger than [`MAX_MESSAGE_BYTES`].
    TooLarge(usize),
    /// Not a Feint envelope (missing file identifier).
    NotFeint,
    /// The FlatBuffers verifier refused the buffer.
    Invalid(String),
    /// The envelope carries a protocol version other than [`PROTOCOL_VERSION`].
    Version(String),
    /// The envelope carries no message.
    Empty,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(n) => write!(f, "message of {n} bytes exceeds {MAX_MESSAGE_BYTES}"),
            Self::NotFeint => write!(f, "not a Feint envelope"),
            Self::Invalid(why) => write!(f, "invalid buffer: {why}"),
            Self::Version(v) => write!(f, "protocol version {v:?}, expected {PROTOCOL_VERSION}"),
            Self::Empty => write!(f, "envelope carries no message"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Wrap a message in a versioned envelope and serialise it.
pub fn encode(message: &MessageT) -> Vec<u8> {
    let mut fbb = flatbuffers::FlatBufferBuilder::new();
    let body = message.pack(&mut fbb);
    let version = fbb.create_string(PROTOCOL_VERSION);
    let root = Envelope::create(
        &mut fbb,
        &EnvelopeArgs {
            protocol_version: Some(version),
            message_type: message.message_type(),
            message: body,
        },
    );
    finish_envelope_buffer(&mut fbb, root);
    fbb.finished_data().to_vec()
}

/// Verify and deserialise an envelope from untrusted bytes.
///
/// # Errors
/// Any [`DecodeError`]; never panics on hostile input.
pub fn decode(bytes: &[u8]) -> Result<MessageT, DecodeError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(DecodeError::TooLarge(bytes.len()));
    }
    // The identifier sits at bytes 4..8; shorter buffers cannot hold it.
    if bytes.len() < 8 || !envelope_buffer_has_identifier(bytes) {
        return Err(DecodeError::NotFeint);
    }
    let options = flatbuffers::VerifierOptions {
        max_depth: 16,
        max_tables: 4096,
        max_apparent_size: MAX_MESSAGE_BYTES,
        ignore_missing_null_terminator: false,
    };
    let envelope = root_as_envelope_with_opts(&options, bytes)
        .map_err(|error| DecodeError::Invalid(error.to_string()))?;
    match envelope.protocol_version() {
        Some(PROTOCOL_VERSION) => {}
        other => {
            // Only a short prefix: the string is the client's and may be a megabyte long.
            let seen = other.unwrap_or_default().chars().take(16).collect();
            return Err(DecodeError::Version(seen));
        }
    }
    match envelope.unpack().message {
        MessageT::NONE => Err(DecodeError::Empty),
        message => Ok(message),
    }
}
