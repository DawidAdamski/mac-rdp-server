//! Wave 0 input sink: logs events, injects nothing (F-001.13).
//!
//! Actual injection via CGEvent lands in wave 2 (F-003, ADR-0003).

use anyhow::Result;
use tracing::info;

use super::{InputSink, KeyEvent, PointerEvent};

#[derive(Debug, Default)]
pub struct LoggingSink;

impl LoggingSink {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl InputSink for LoggingSink {
    fn pointer(&mut self, ev: PointerEvent) -> Result<()> {
        info!(?ev, "pointer");
        Ok(())
    }

    fn key(&mut self, ev: KeyEvent) -> Result<()> {
        info!(?ev, "key");
        Ok(())
    }
}
