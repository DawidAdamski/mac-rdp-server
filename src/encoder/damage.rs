//! Damage tracking: send the rectangles that changed, not the whole screen
//! (F-004, ADR-0009).
//!
//! # Why the diff, and not `dirty_rects()` from ScreenCaptureKit
//!
//! The system hands us damage rectangles, and using them directly is the
//! obvious design — but it is wrong here. Those rectangles describe the step
//! from frame N-1 to frame N, while our frames travel a `watch` channel that
//! **coalesces** (`01-architecture.md` §4b). A consumer that received N-3 and
//! then N never learns what changed in N-2 and N-1, so anything touched only
//! in those frames is never resent and stays on screen as a ghost — worse the
//! slower the link. Full reasoning in ADR-0009.
//!
//! So the source of truth is a diff against the frame **this encoder last
//! sent**, which is immune to coalescing by construction and is per-connection,
//! because `MacRdpDisplay::updates()` builds one encoder per client.
//!
//! # What the bitmap encoder downstream demands
//!
//! Two constraints read out of `ironrdp-server`'s `encoder/bitmap.rs`, both
//! load-bearing:
//!
//! * **Width must be a multiple of 4** (`:30`), or encoding fails outright.
//!   Rectangles are therefore widened, never narrowed — narrowing would drop
//!   pixels that changed.
//! * Rows are read as `&row[..width * bpp]` out of `data.chunks(stride)`
//!   (`:44`, `:56`), so a buffer that is not an exact multiple of its stride
//!   **panics** on the final short row. Regions are emitted as tight copies
//!   rather than zero-copy windows for exactly this reason (ADR-0009 §4).

use std::time::{Duration, Instant};

use anyhow::Result;
use bytes::{Bytes, BytesMut};
use tracing::{debug, warn};

use super::{Encoder, FrameUpdate};
use crate::capture::Frame;

/// Comparison granularity. Smaller finds tighter damage but produces more
/// rectangles, and every rectangle costs a bitmap header downstream. 64 px is
/// a starting point to be measured, not a tuned value (F-004 §8).
pub const DEFAULT_TILE: u32 = 64;

/// Above this share of dirty tiles, one whole-frame update beats many small
/// ones — the per-rectangle overhead stops paying for the pixels it saves.
pub const DEFAULT_FULL_FRAME_RATIO: f64 = 0.60;

/// Hard ceiling on rectangles per frame, so a pathological pattern (a dirty
/// checkerboard) degrades to a full frame instead of a storm of tiny updates.
pub const DEFAULT_MAX_REGIONS: usize = 32;

/// How often the damage counters are logged (F-004.17).
const STATS_INTERVAL: Duration = Duration::from_secs(10);

/// A rectangle of the desktop, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Why a frame was sent whole instead of as regions. Logged, and asserted on
/// in tests — the fallbacks are behaviour, not an implementation detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FullReason {
    FirstFrame,
    GeometryChanged,
    WidthNotAligned,
    MostOfScreenChanged,
    TooManyRegions,
    BaselineInvalidated,
}

impl FullReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::FirstFrame => "first frame of this connection",
            Self::BaselineInvalidated => "baseline invalidated (client resumed)",
            Self::GeometryChanged => "frame geometry changed",
            Self::WidthNotAligned => "desktop width is not a multiple of 4",
            Self::MostOfScreenChanged => "most of the screen changed",
            Self::TooManyRegions => "too many separate regions",
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    frames_in: u64,
    frames_unchanged: u64,
    full_frames: u64,
    regions: u64,
    bytes_sent: u64,
    bytes_if_full: u64,
}

/// Sends only what changed since the last frame this encoder emitted.
pub struct DamageEncoder {
    tile: u32,
    full_frame_ratio: f64,
    max_regions: usize,
    /// The frame this encoder last handed downstream — the baseline every
    /// comparison is made against (ADR-0009 §1).
    previous: Option<Frame>,
    /// Set by [`Encoder::invalidate_baseline`]: the next frame goes out whole
    /// and is labelled as such in the log (F-008.7v2).
    invalidated: bool,
    stats: Stats,
    last_log: Instant,
}

impl Default for DamageEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl DamageEncoder {
    #[must_use]
    pub fn new() -> Self {
        Self::with_settings(DEFAULT_TILE, DEFAULT_FULL_FRAME_RATIO, DEFAULT_MAX_REGIONS)
    }

    #[must_use]
    pub fn with_settings(tile: u32, full_frame_ratio: f64, max_regions: usize) -> Self {
        Self {
            tile: tile.max(1),
            full_frame_ratio,
            max_regions,
            previous: None,
            invalidated: false,
            stats: Stats::default(),
            last_log: Instant::now(),
        }
    }

    /// The reason this frame cannot be sent as regions, if there is one.
    fn full_reason(&self, frame: &Frame) -> Option<FullReason> {
        // A width that is not a multiple of 4 cannot produce a legal region at
        // the right edge, and the whole-frame path has the same requirement —
        // `main` rejects such a desktop at startup (F-004.9), so in practice
        // this arm is unreachable. It exists so the encoder is total.
        if !frame.width.is_multiple_of(4) {
            return Some(FullReason::WidthNotAligned);
        }
        if self.invalidated {
            return Some(FullReason::BaselineInvalidated);
        }
        match &self.previous {
            None => Some(FullReason::FirstFrame),
            Some(previous) => (previous.width != frame.width
                || previous.height != frame.height
                || previous.stride != frame.stride
                || previous.format != frame.format)
                .then_some(FullReason::GeometryChanged),
        }
    }

    fn emit_full(&mut self, frame: Frame, reason: FullReason) -> Vec<FrameUpdate> {
        debug!(
            reason = reason.as_str(),
            width = frame.width,
            height = frame.height,
            "sending a full frame"
        );
        self.stats.full_frames += 1;
        self.stats.bytes_sent += frame.data.len() as u64;
        self.invalidated = false;
        self.previous = Some(frame.clone());
        vec![FrameUpdate::Full { frame }]
    }

    fn maybe_log(&mut self, now: Instant) {
        if now.duration_since(self.last_log) < STATS_INTERVAL {
            return;
        }
        self.last_log = now;
        let s = self.stats;
        debug!(
            frames_in = s.frames_in,
            unchanged = s.frames_unchanged,
            full_frames = s.full_frames,
            regions = s.regions,
            bytes_sent = s.bytes_sent,
            bytes_if_full = s.bytes_if_full,
            saved_pct = saved_percent(s.bytes_sent, s.bytes_if_full),
            "damage stats"
        );
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "byte counters are far below f64's exact-integer range in any realistic session"
)]
fn saved_percent(sent: u64, if_full: u64) -> u64 {
    if if_full == 0 {
        return 0;
    }
    let ratio = 1.0 - (sent as f64 / if_full as f64);
    (ratio.clamp(0.0, 1.0) * 100.0) as u64
}

impl Encoder for DamageEncoder {
    fn encode(&mut self, frame: Frame) -> Result<Vec<FrameUpdate>> {
        let now = Instant::now();
        self.stats.frames_in += 1;
        self.stats.bytes_if_full += frame.data.len() as u64;

        if let Some(reason) = self.full_reason(&frame) {
            let updates = self.emit_full(frame, reason);
            self.maybe_log(now);
            return Ok(updates);
        }

        let grid = {
            let previous = self.previous.as_ref().expect("full_reason covers the None case");
            dirty_tiles(previous, &frame, self.tile)
        };

        if !grid.any_dirty() {
            // Not an error and not a dropped frame: nothing on screen moved, so
            // there is nothing to send. `next_update` simply waits for the next
            // frame (`src/session/display.rs`).
            self.stats.frames_unchanged += 1;
            self.previous = Some(frame);
            self.maybe_log(now);
            return Ok(Vec::new());
        }

        if grid.dirty_ratio() > self.full_frame_ratio {
            let updates = self.emit_full(frame, FullReason::MostOfScreenChanged);
            self.maybe_log(now);
            return Ok(updates);
        }

        let rects = grid.merge();
        if rects.len() > self.max_regions {
            let updates = self.emit_full(frame, FullReason::TooManyRegions);
            self.maybe_log(now);
            return Ok(updates);
        }

        let mut updates = Vec::with_capacity(rects.len());
        for rect in rects {
            let Some(aligned) = align_to_four(rect, frame.width) else {
                // Only reachable for a width that is not a multiple of 4, which
                // `full_reason` already rejected; belt and braces.
                warn!(
                    ?rect,
                    width = frame.width,
                    "could not align a region; sending the frame whole"
                );
                let updates = self.emit_full(frame, FullReason::WidthNotAligned);
                self.maybe_log(now);
                return Ok(updates);
            };
            let region = extract(&frame, aligned);
            self.stats.regions += 1;
            self.stats.bytes_sent += region.data.len() as u64;
            updates.push(FrameUpdate::Region {
                x: aligned.x,
                y: aligned.y,
                frame: region,
            });
        }

        self.previous = Some(frame);
        self.maybe_log(now);
        Ok(updates)
    }

    /// F-008.7v2: forget the baseline, so the next frame goes out whole.
    fn invalidate_baseline(&mut self) {
        self.previous = None;
        self.invalidated = true;
    }
}

/// Which tiles of the frame differ, as a grid.
pub(crate) struct TileGrid {
    dirty: Vec<bool>,
    tiles_x: u32,
    tiles_y: u32,
    tile: u32,
    width: u32,
    height: u32,
}

impl TileGrid {
    fn any_dirty(&self) -> bool {
        self.dirty.iter().any(|d| *d)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "tile counts are small; the ratio only feeds a threshold comparison"
    )]
    fn dirty_ratio(&self) -> f64 {
        if self.dirty.is_empty() {
            return 0.0;
        }
        self.dirty.iter().filter(|d| **d).count() as f64 / self.dirty.len() as f64
    }

    /// Merges dirty tiles into rectangles: horizontal runs within a tile row,
    /// then a vertical join when a row's run sits exactly under the previous
    /// one.
    ///
    /// The vertical join only inspects the last rectangle produced, which is
    /// deliberate. It is exact for the shape that dominates in practice — a
    /// band spanning several tile rows, as scrolling and window drags produce —
    /// and merely leaves a few extra rectangles in patterns where it is not,
    /// which costs bandwidth rather than correctness.
    pub(crate) fn merge(&self) -> Vec<Rect> {
        let mut out: Vec<Rect> = Vec::new();
        for ty in 0..self.tiles_y {
            let mut tx = 0;
            while tx < self.tiles_x {
                if !self.dirty[(ty * self.tiles_x + tx) as usize] {
                    tx += 1;
                    continue;
                }
                let start = tx;
                while tx < self.tiles_x && self.dirty[(ty * self.tiles_x + tx) as usize] {
                    tx += 1;
                }

                let x = start * self.tile;
                let w = (tx * self.tile).min(self.width) - x;
                let y = ty * self.tile;
                let h = ((ty + 1) * self.tile).min(self.height) - y;

                match out.last_mut() {
                    Some(last) if last.x == x && last.w == w && last.y + last.h == y => last.h += h,
                    _ => out.push(Rect { x, y, w, h }),
                }
            }
        }
        out
    }
}

/// Compares two frames tile by tile.
///
/// Both frames must share geometry; the caller guarantees that by falling back
/// to a full frame otherwise.
///
/// Only the **visible** bytes of each row are compared — `stride` padding is
/// not part of the picture, and comparing it would report damage for bytes no
/// client ever sees (ADR-0005).
pub(crate) fn dirty_tiles(previous: &Frame, current: &Frame, tile: u32) -> TileGrid {
    let bpp = current.format.bytes_per_pixel();
    let width = current.width;
    let height = current.height;
    let stride = current.stride;
    let tiles_x = width.div_ceil(tile);
    let tiles_y = height.div_ceil(tile);
    let mut dirty = vec![false; (tiles_x as usize) * (tiles_y as usize)];

    let row_bytes = (width as usize).saturating_mul(bpp);

    for ty in 0..tiles_y {
        let first_row = ty * tile;
        let last_row = ((ty + 1) * tile).min(height);
        let base = (ty * tiles_x) as usize;
        let mut remaining = tiles_x;

        for y in first_row..last_row {
            let offset = (y as usize) * stride;
            let (Some(prev_row), Some(cur_row)) = (
                previous.data.get(offset..offset + row_bytes),
                current.data.get(offset..offset + row_bytes),
            ) else {
                // A short buffer is a capture-side bug; treat the rest of this
                // tile row as changed rather than reading out of bounds.
                dirty[base..base + tiles_x as usize].fill(true);
                break;
            };

            // One comparison for the whole row first: on a static desktop this
            // is the only work done, and it is a single memcmp per row.
            if prev_row == cur_row {
                continue;
            }

            for tx in 0..tiles_x {
                if dirty[base + tx as usize] {
                    continue;
                }
                let from = (tx * tile) as usize * bpp;
                let to = (((tx + 1) * tile).min(width) as usize) * bpp;
                if prev_row[from..to] != cur_row[from..to] {
                    dirty[base + tx as usize] = true;
                    remaining -= 1;
                }
            }

            // Every tile in this row is already dirty; further rows cannot
            // teach us anything about it.
            if remaining == 0 {
                break;
            }
        }
    }

    TileGrid {
        dirty,
        tiles_x,
        tiles_y,
        tile,
        width,
        height,
    }
}

/// Widens a rectangle so its width is a multiple of 4, as the bitmap encoder
/// requires (F-004.6), clamped to the frame (F-004.7).
///
/// Widening only — narrowing would silently drop changed pixels. Returns
/// `None` when no legal widening exists, which can only happen for a frame
/// whose own width is not a multiple of 4.
pub(crate) fn align_to_four(rect: Rect, frame_width: u32) -> Option<Rect> {
    let mut left = rect.x & !3;
    let right = (rect.x + rect.w).div_ceil(4).saturating_mul(4).min(frame_width);
    if right <= left {
        return None;
    }

    // If the right edge had to be clamped to a frame width that is not itself a
    // multiple of 4, pull the left edge back instead.
    let overhang = (right - left) % 4;
    if overhang != 0 {
        left = left.checked_sub(4 - overhang)?;
    }

    let w = right - left;
    w.is_multiple_of(4).then_some(Rect { x: left, w, ..rect })
}

/// Copies a rectangle out of a frame into a tightly packed buffer.
///
/// Tight (`stride == width * bpp`) rather than a zero-copy `Bytes` window: a
/// window would have to be exactly `h * stride` bytes long, and for a rectangle
/// touching the bottom edge at `x > 0` those bytes run past the end of the
/// buffer. Copying a damage rectangle is cheap by definition — it is small, or
/// the full-frame fallback would have fired (ADR-0009 §4).
pub(crate) fn extract(frame: &Frame, rect: Rect) -> Frame {
    let bpp = frame.format.bytes_per_pixel();
    let row_bytes = (rect.w as usize) * bpp;
    let mut data = BytesMut::with_capacity(row_bytes * rect.h as usize);

    for row in 0..rect.h {
        // `y * stride + x * bpp`, never `y * width * bpp` (ADR-0005).
        let start = ((rect.y + row) as usize) * frame.stride + (rect.x as usize) * bpp;
        match frame.data.get(start..start + row_bytes) {
            Some(slice) => data.extend_from_slice(slice),
            // Unreachable for a rectangle derived from this frame's own grid;
            // pad rather than panic if it ever becomes reachable.
            None => data.resize(data.len() + row_bytes, 0),
        }
    }

    Frame {
        width: rect.w,
        height: rect.h,
        stride: row_bytes,
        format: frame.format,
        data: Bytes::from(data),
        sequence: frame.sequence,
        captured_at: frame.captured_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::PixelFormat;

    const BPP: usize = 4;

    /// Builds a frame with **padded** rows, so every test exercises the
    /// `stride > width * bpp` path that ADR-0005 warns about (F-004.4).
    fn frame(width: u32, height: u32, fill: u8) -> Frame {
        let stride = (width as usize) * BPP + 64;
        Frame {
            width,
            height,
            stride,
            format: PixelFormat::Bgra8,
            data: Bytes::from(vec![fill; stride * height as usize]),
            sequence: 0,
            captured_at: Instant::now(),
        }
    }

    /// Paints one pixel, in the frame's own byte order.
    fn put(frame: &mut Frame, x: u32, y: u32, value: u8) {
        let mut bytes = frame.data.to_vec();
        let offset = (y as usize) * frame.stride + (x as usize) * BPP;
        bytes[offset..offset + BPP].fill(value);
        frame.data = Bytes::from(bytes);
    }

    fn regions(updates: &[FrameUpdate]) -> Vec<(u32, u32, u32, u32)> {
        updates
            .iter()
            .filter_map(|u| match u {
                FrameUpdate::Region { x, y, frame } => Some((*x, *y, frame.width, frame.height)),
                FrameUpdate::Full { .. } => None,
            })
            .collect()
    }

    fn is_full(updates: &[FrameUpdate]) -> bool {
        matches!(updates, [FrameUpdate::Full { .. }])
    }

    #[test]
    fn the_first_frame_of_a_connection_is_sent_whole() {
        let mut encoder = DamageEncoder::new();
        let updates = encoder.encode(frame(256, 256, 0)).expect("encode");
        assert!(is_full(&updates), "a client with no picture yet needs the whole one");
    }

    #[test]
    fn an_unchanged_frame_produces_nothing() {
        let mut encoder = DamageEncoder::new();
        let first = frame(256, 256, 7);
        encoder.encode(first.clone()).expect("first");

        let updates = encoder.encode(first).expect("second");
        assert!(updates.is_empty(), "a static desktop must cost no bandwidth at all");
    }

    #[test]
    fn one_changed_pixel_yields_one_region_containing_it() {
        let mut encoder = DamageEncoder::with_settings(64, 0.6, 32);
        let first = frame(256, 256, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 100, 130, 0xFF);
        let updates = encoder.encode(second).expect("second");

        assert_eq!(regions(&updates), vec![(64, 128, 64, 64)], "the tile holding (100,130)");
    }

    #[test]
    fn two_distant_changes_yield_two_disjoint_regions() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(256, 256, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 10, 10, 0xFF);
        put(&mut second, 200, 200, 0xFF);
        let updates = encoder.encode(second).expect("second");

        assert_eq!(regions(&updates), vec![(0, 0, 64, 64), (192, 192, 64, 64)]);
    }

    #[test]
    fn adjacent_tiles_merge_horizontally() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(256, 64, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        for x in [10, 70, 130] {
            put(&mut second, x, 10, 0xFF);
        }
        let updates = encoder.encode(second).expect("second");

        assert_eq!(regions(&updates), vec![(0, 0, 192, 64)], "three tiles, one rectangle");
    }

    #[test]
    fn a_vertical_band_merges_across_tile_rows() {
        // 4x4 tiles, so one dirty column is a quarter of the screen and stays
        // well under the full-frame threshold.
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(256, 256, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        for y in [10, 70, 130, 200] {
            put(&mut second, 10, y, 0xFF);
        }
        let updates = encoder.encode(second).expect("second");

        assert_eq!(
            regions(&updates),
            vec![(0, 0, 64, 256)],
            "four stacked tiles must become one rectangle, not four"
        );
    }

    #[test]
    fn a_changed_geometry_forces_a_full_frame() {
        let mut encoder = DamageEncoder::new();
        encoder.encode(frame(256, 256, 0)).expect("first");

        let updates = encoder.encode(frame(320, 256, 0)).expect("resized");
        assert!(is_full(&updates), "there is nothing to diff a new size against");
    }

    #[test]
    fn changing_most_of_the_screen_falls_back_to_a_full_frame() {
        let mut encoder = DamageEncoder::with_settings(64, 0.5, 32);
        let first = frame(256, 256, 0);
        encoder.encode(first.clone()).expect("first");

        // Every tile differs.
        let second = frame(256, 256, 1);
        let updates = encoder.encode(second).expect("second");
        assert!(is_full(&updates));
    }

    #[test]
    fn too_many_separate_regions_falls_back_to_a_full_frame() {
        // Allow plenty of dirty area but only two rectangles.
        let mut encoder = DamageEncoder::with_settings(64, 0.99, 2);
        let first = frame(512, 128, 0);
        encoder.encode(first.clone()).expect("first");

        // Four separated tiles in one row: four runs, over the limit of two.
        let mut second = first.clone();
        for x in [10, 140, 270, 400] {
            put(&mut second, x, 10, 0xFF);
        }
        let updates = encoder.encode(second).expect("second");
        assert!(is_full(&updates), "a storm of tiny updates costs more than one frame");
    }

    /// F-004.4. The regression this guards: indexing with `y * width * bpp`
    /// instead of `y * stride + x * bpp` still finds *a* difference, just in the
    /// wrong place — so a test that only asserted "something changed" would pass.
    #[test]
    fn padded_rows_do_not_shift_the_reported_region() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(256, 256, 0);
        assert!(first.stride > first.width as usize * BPP, "the test needs padding");
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 200, 200, 0xFF);
        let updates = encoder.encode(second).expect("second");

        assert_eq!(regions(&updates), vec![(192, 192, 64, 64)]);
    }

    #[test]
    fn a_region_carries_the_pixels_of_its_rectangle_packed_tight() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(128, 128, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 70, 70, 0xAB);
        let updates = encoder.encode(second).expect("second");

        let [FrameUpdate::Region { x, y, frame: region }] = updates.as_slice() else {
            panic!("expected exactly one region, got {updates:?}");
        };
        assert_eq!((*x, *y), (64, 64));
        assert_eq!(region.stride, region.width as usize * BPP, "regions are packed tight");
        assert_eq!(region.data.len(), region.stride * region.height as usize);
        // The changed pixel sits at (70,70) globally, so (6,6) within the region.
        assert_eq!(region.pixel(6, 6), Some([0xAB; 4]));
        assert_eq!(region.pixel(0, 0), Some([0x00; 4]));
    }

    #[test]
    fn the_baseline_advances_so_a_change_is_reported_once() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(128, 128, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 10, 10, 0xFF);
        assert_eq!(regions(&encoder.encode(second.clone()).expect("second")).len(), 1);
        assert!(
            encoder.encode(second).expect("third").is_empty(),
            "the same content must not be resent"
        );
    }

    /// A pixel that changes and changes back must still be repainted: the
    /// baseline is what was *sent*, and the client is showing the intermediate
    /// state only if we sent it. Here we did, so the revert is a real change.
    #[test]
    fn a_reverted_pixel_is_repainted() {
        let mut encoder = DamageEncoder::with_settings(64, 0.9, 32);
        let first = frame(128, 128, 0);
        encoder.encode(first.clone()).expect("first");

        let mut changed = first.clone();
        put(&mut changed, 10, 10, 0xFF);
        encoder.encode(changed).expect("changed");

        let updates = encoder.encode(first).expect("reverted");
        assert_eq!(regions(&updates), vec![(0, 0, 64, 64)]);
    }

    #[test]
    fn alignment_widens_never_narrows() {
        // x=1..=6 must survive: left rounds down to 0, right up to 8.
        assert_eq!(
            align_to_four(
                Rect {
                    x: 1,
                    y: 0,
                    w: 5,
                    h: 10
                },
                256
            ),
            Some(Rect {
                x: 0,
                y: 0,
                w: 8,
                h: 10
            })
        );
    }

    #[test]
    fn alignment_leaves_an_already_aligned_rectangle_alone() {
        let rect = Rect {
            x: 64,
            y: 32,
            w: 128,
            h: 16,
        };
        assert_eq!(align_to_four(rect, 256), Some(rect));
    }

    /// F-004.7: widening must not run off the right edge of the frame.
    #[test]
    fn alignment_clamps_to_the_frame_and_stays_a_multiple_of_four() {
        let aligned = align_to_four(
            Rect {
                x: 250,
                y: 0,
                w: 6,
                h: 4,
            },
            256,
        )
        .expect("aligned");
        assert_eq!(aligned.x + aligned.w, 256, "clamped to the frame");
        assert!(aligned.w.is_multiple_of(4));
        assert!(aligned.x <= 250, "must still cover the original left edge");
    }

    #[test]
    fn alignment_pulls_the_left_edge_back_when_the_frame_width_is_odd() {
        // A 254-wide frame cannot be widened rightwards past 254, so the left
        // edge moves instead.
        let aligned = align_to_four(
            Rect {
                x: 252,
                y: 0,
                w: 2,
                h: 4,
            },
            254,
        )
        .expect("aligned");
        assert!(aligned.w.is_multiple_of(4));
        assert!(aligned.x <= 252 && aligned.x + aligned.w >= 254);
    }

    #[test]
    fn a_width_that_is_not_a_multiple_of_four_is_sent_whole() {
        let mut encoder = DamageEncoder::new();
        let first = frame(254, 64, 0);
        encoder.encode(first.clone()).expect("first");

        let mut second = first.clone();
        put(&mut second, 10, 10, 0xFF);
        let updates = encoder.encode(second).expect("second");
        assert!(
            is_full(&updates),
            "the bitmap encoder rejects such widths; main refuses this desktop at startup"
        );
    }

    /// F-008.7v2: after invalidation the next frame is full again, even when
    /// the screen has not changed at all — that is the point of a repaint on
    /// resume.
    #[test]
    fn f008_7v2_invalidating_the_baseline_forces_a_full_frame() {
        let mut encoder = DamageEncoder::new();
        // Several tiles wide, so a one-pixel change stays a region.
        let first = frame(256, 256, 0);
        assert!(is_full(&encoder.encode(first.clone()).expect("first")), "first frame");

        let mut second = first.clone();
        put(&mut second, 4, 4, 0xFF);
        assert!(
            !is_full(&encoder.encode(second.clone()).expect("second")),
            "a small change is a region"
        );

        encoder.invalidate_baseline();
        assert!(
            is_full(&encoder.encode(second.clone()).expect("after invalidation")),
            "an unchanged frame must still be sent whole after invalidation"
        );

        let mut third = second.clone();
        put(&mut third, 8, 8, 0xFF);
        assert!(
            !is_full(&encoder.encode(third).expect("third")),
            "and afterwards the encoder is back to diffing"
        );
    }

    /// F-008.7v2: invalidating twice costs one full frame, not two.
    #[test]
    fn f008_7v2_invalidation_is_consumed_by_the_next_frame() {
        let mut encoder = DamageEncoder::new();
        let first = frame(256, 256, 0);
        encoder.encode(first.clone()).expect("first");
        encoder.invalidate_baseline();
        encoder.invalidate_baseline();
        assert!(is_full(&encoder.encode(first.clone()).expect("full")), "one full frame");
        assert!(
            encoder.encode(first).expect("unchanged").is_empty(),
            "an unchanged frame after that sends nothing"
        );
    }

    #[test]
    fn saved_percentage_is_reported_sanely() {
        assert_eq!(saved_percent(0, 0), 0);
        assert_eq!(saved_percent(50, 100), 50);
        assert_eq!(saved_percent(0, 100), 100);
        assert_eq!(saved_percent(200, 100), 0, "never negative");
    }
}
