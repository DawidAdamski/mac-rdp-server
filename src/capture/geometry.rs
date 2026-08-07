//! Live display geometry, cheap enough to poll (F-007.17).
//!
//! Input maps RDP desktop pixels onto global macOS points, and that mapping
//! depends on where the captured display sits in the global point space. That
//! position **moves**: attaching a display (opening the lid) makes macOS
//! re-lay-out every screen, and an origin read once at startup is then wrong.
//! Verified 2026-08-07 — the cursor led into the left of the RDP desktop
//! crossed onto the laptop screen (F-007 §8a).
//!
//! Re-enumerating `SCShareableContent` for this would be far too heavy to run
//! on a timer, so the geometry comes straight from Quartz instead:
//! `CGDisplayBounds` plus the current display mode. Both are cheap, synchronous
//! and need no permission.
//!
//! This module knows nothing about input — it publishes [`DisplayInfo`], our
//! own type, and `main` is what turns that into an input-side geometry
//! (F-007.17a, `01-architecture.md` §1).

use core_graphics::display::CGDisplay;

use super::{DisplayId, DisplayInfo};

/// Derives the point-to-pixel scale of a display mode.
///
/// Pure, so the Retina arithmetic is unit-tested without a second monitor.
/// Falls back to 1.0 for the nonsense values a disconnected display can report,
/// because a zero scale would collapse the whole desktop onto its origin.
#[must_use]
pub(crate) fn scale_from(points: u64, pixels: u64) -> f64 {
    if points == 0 || pixels == 0 {
        return 1.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "display dimensions are far below f64's exact-integer range"
    )]
    let scale = pixels as f64 / points as f64;
    if scale > 0.0 { scale } else { 1.0 }
}

/// Reads a display's current position and scale.
///
/// Returns `None` when the display is not currently active — the lid is shut,
/// the cable is out — so the caller keeps the last known geometry rather than
/// mapping input through a null rectangle.
#[must_use]
pub fn probe(id: DisplayId) -> Option<DisplayInfo> {
    let display = CGDisplay::new(id.0);
    if !display.is_active() {
        return None;
    }

    let bounds = display.bounds();
    let mode = display.display_mode()?;
    let scale = scale_from(mode.width(), mode.pixel_width());

    Some(DisplayInfo {
        id,
        width_px: u32::try_from(mode.pixel_width()).unwrap_or(u32::MAX),
        height_px: u32::try_from(mode.pixel_height()).unwrap_or(u32::MAX),
        scale_factor: scale,
        origin_x: bounds.origin.x,
        origin_y: bounds.origin.y,
        name: format!("Display {}", id.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_retina_mode_scales_one_to_one() {
        assert_eq!(scale_from(5120, 5120), 1.0);
    }

    #[test]
    fn a_retina_mode_scales_by_two() {
        assert_eq!(scale_from(1512, 3024), 2.0);
    }

    #[test]
    fn a_degenerate_mode_falls_back_to_one() {
        // A disconnected display reports zeroes; a zero scale would map every
        // pixel of the desktop onto the display's origin.
        assert_eq!(scale_from(0, 3024), 1.0);
        assert_eq!(scale_from(1512, 0), 1.0);
        assert_eq!(scale_from(0, 0), 1.0);
    }

    #[test]
    fn probing_a_display_does_not_panic() {
        // The result depends on the machine running the tests, so only the
        // absence of a crash is asserted; the real check is manual (F-007 AC-2).
        let _ = probe(DisplayId(CGDisplay::main().id));
        let _ = probe(DisplayId(u32::MAX));
    }
}
