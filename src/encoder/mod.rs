//! Turns captured frames into display updates.
//!
//! Kept separate from `session` because the interesting logic here — damage
//! tracking in wave 3 — is pure and unit-testable, while `session` is not
//! (`specs/01-architecture.md` §2).

pub mod damage;
pub mod full_frame;

use anyhow::Result;

use crate::capture::Frame;

/// One update to be sent to the client.
///
/// `Resize` arrives with session resize (F-006), which is not approved yet.
#[derive(Debug, Clone)]
pub enum FrameUpdate {
    Full {
        frame: Frame,
    },
    /// A changed rectangle, placed at `(x, y)` of the RDP desktop (F-004).
    ///
    /// `frame` carries **only** that rectangle, packed tight
    /// (`stride == width * bpp`), not a window onto the full frame.
    Region {
        x: u32,
        y: u32,
        frame: Frame,
    },
}

pub trait Encoder: Send {
    fn encode(&mut self, frame: Frame) -> Result<Vec<FrameUpdate>>;
}
