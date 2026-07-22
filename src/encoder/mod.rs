//! Turns captured frames into display updates.
//!
//! Kept separate from `session` because the interesting logic here — damage
//! tracking in wave 3 — is pure and unit-testable, while `session` is not
//! (`specs/01-architecture.md` §2).

pub mod full_frame;

use anyhow::Result;

use crate::capture::Frame;

/// One update to be sent to the client.
///
/// Wave 0 only produces whole frames. `Region` arrives with damage tracking
/// (F-004) and `Resize` with session resize (F-006); neither requirement is
/// approved yet, so neither variant exists yet.
#[derive(Debug, Clone)]
pub enum FrameUpdate {
    Full { frame: Frame },
}

pub trait Encoder: Send {
    fn encode(&mut self, frame: Frame) -> Result<Vec<FrameUpdate>>;
}
