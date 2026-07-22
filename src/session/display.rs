//! Adapts our capture/encoder pipeline to `RdpServerDisplay`.

use std::collections::VecDeque;

use anyhow::Result;
use ironrdp_server::{DesktopSize, DisplayUpdate, RdpServerDisplay, RdpServerDisplayUpdates};
use tracing::debug;

use super::convert;
use crate::capture::FrameStream;
use crate::encoder::Encoder;

/// Builds a fresh encoder for each connection.
pub type EncoderFactory = Box<dyn Fn() -> Box<dyn Encoder> + Send>;

pub struct MacRdpDisplay {
    stream: FrameStream,
    size: DesktopSize,
    make_encoder: EncoderFactory,
}

impl MacRdpDisplay {
    pub fn new(stream: FrameStream, width: u16, height: u16, make_encoder: EncoderFactory) -> Self {
        Self {
            stream,
            size: DesktopSize { width, height },
            make_encoder,
        }
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for MacRdpDisplay {
    async fn size(&mut self) -> DesktopSize {
        self.size
    }

    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        // Each connection gets its own stream clone, so it receives the current
        // frame immediately instead of waiting for the next one.
        Ok(Box::new(MacRdpDisplayUpdates {
            stream: self.stream.clone(),
            encoder: (self.make_encoder)(),
            pending: VecDeque::new(),
        }))
    }

    // `request_layout` keeps its default (log-only) implementation: honouring a
    // client-requested resize is F-006, wave 3.
}

pub struct MacRdpDisplayUpdates {
    stream: FrameStream,
    encoder: Box<dyn Encoder>,
    pending: VecDeque<DisplayUpdate>,
}

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for MacRdpDisplayUpdates {
    /// # Cancel safety
    ///
    /// Required by the trait: this is awaited inside `tokio::select!`. The only
    /// await is `FrameStream::next`, which is cancel-safe, and updates already
    /// produced sit in `pending` rather than in a local, so nothing is lost.
    async fn next_update(&mut self) -> Result<Option<DisplayUpdate>> {
        loop {
            if let Some(update) = self.pending.pop_front() {
                return Ok(Some(update));
            }

            let frame = self.stream.next().await?;
            for update in self.encoder.encode(frame)? {
                self.pending.push_back(convert::display_update(update)?);
            }
            if self.pending.is_empty() {
                debug!("encoder produced no updates for this frame");
            }
        }
    }
}
