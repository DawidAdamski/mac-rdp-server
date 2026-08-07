//! Wave 0/1 encoder: every frame is sent whole.
//!
//! Wasteful by design — `ironrdp-server` still applies RDP 6.0 compression
//! downstream. Damage tracking is F-004 (wave 3).

use anyhow::Result;

use super::{Encoder, FrameUpdate};
use crate::capture::Frame;

#[derive(Debug, Default)]
pub struct FullFrameEncoder;

impl FullFrameEncoder {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Encoder for FullFrameEncoder {
    fn encode(&mut self, frame: Frame) -> Result<Vec<FrameUpdate>> {
        Ok(vec![FrameUpdate::Full { frame }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::test_pattern;

    #[test]
    fn every_frame_becomes_one_full_update() {
        let mut encoder = FullFrameEncoder::new();
        let frame = test_pattern::render(64, 64, 0);
        let updates = encoder.encode(frame.clone()).expect("encode");

        assert_eq!(updates.len(), 1);
        let [FrameUpdate::Full { frame: encoded }] = updates.as_slice() else {
            panic!("the full-frame encoder must never produce a region, got {updates:?}");
        };
        assert_eq!(encoded.width, frame.width);
        assert_eq!(encoded.height, frame.height);
        assert_eq!(encoded.stride, frame.stride);
        assert_eq!(encoded.data, frame.data);
    }
}
