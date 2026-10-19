//! How long does one frame cost the encoder? (F-004, F-007 §8d)
//!
//! Capture sustains 29.6 fps at 5120x1440 — measured, see F-007. The open
//! question is the other end of the pipe: the encoder and the protocol engine
//! consume frames from a `watch` channel that coalesces, so a consumer slower
//! than capture does not queue, it drops. The visible result is a picture that
//! updates in bursts and looks stuck in between.
//!
//! This probe times `Encoder::encode` alone, with no capture and no client, so
//! the number is the encoder's and nobody else's.
//!
//! Budget: at 30 fps a frame may cost 33.3 ms. At 60 fps, 16.7 ms.

use std::time::{Duration, Instant};

use bytes::Bytes;
use mac_rdp_server::capture::{Frame, PixelFormat};
use mac_rdp_server::encoder::Encoder;
use mac_rdp_server::encoder::damage::DamageEncoder;
use mac_rdp_server::encoder::full_frame::FullFrameEncoder;

/// The owner's desktop, which is where the symptom was reported.
const WIDTH: u32 = 5120;
const HEIGHT: u32 = 1440;
const BPP: usize = 4;

fn frame(seq: u64, fill: u8) -> Frame {
    let stride = WIDTH as usize * BPP;
    Frame {
        width: WIDTH,
        height: HEIGHT,
        stride,
        format: PixelFormat::Bgra8,
        data: Bytes::from(vec![fill; stride * HEIGHT as usize]),
        sequence: seq,
        captured_at: Instant::now(),
    }
}

/// Copies `base` and paints a `w` x `h` rectangle at (`x`, `y`).
///
/// Pixel arithmetic goes through `y * stride + x * bpp`, never
/// `y * width * bpp` (ADR-0005) — even here, where the stride happens to be
/// packed, because a probe that models the arithmetic wrongly measures the
/// wrong thing.
fn frame_with_rect(seq: u64, base: u8, x: u32, y: u32, w: u32, h: u32, ink: u8) -> Frame {
    let stride = WIDTH as usize * BPP;
    let mut data = vec![base; stride * HEIGHT as usize];
    for row in y..(y + h).min(HEIGHT) {
        let start = row as usize * stride + x as usize * BPP;
        let end = (start + w as usize * BPP).min(start + stride);
        data[start..end].fill(ink);
    }
    Frame {
        width: WIDTH,
        height: HEIGHT,
        stride,
        format: PixelFormat::Bgra8,
        data: Bytes::from(data),
        sequence: seq,
        captured_at: Instant::now(),
    }
}

/// A fill byte that really differs from frame to frame.
///
/// The first cut of this probe used `n % 251 | 1`, which collapses consecutive
/// pairs onto one value (10|1 and 11|1 are both 11), so half the "changed"
/// frames were identical and the probe measured the wrong thing. Odd steps
/// through a 256-wide space never repeat inside one run.
fn ink(n: u64) -> u8 {
    u8::try_from(n.wrapping_mul(37).wrapping_add(1) % 256).unwrap_or(1)
}

fn time_it(label: &str, encoder: &mut dyn Encoder, frames: Vec<Frame>) {
    // First frame primes the baseline; it is always a full send and is not
    // what we are measuring.
    let mut costs: Vec<Duration> = Vec::new();
    let mut updates_last = 0;
    for (n, f) in frames.into_iter().enumerate() {
        let t = Instant::now();
        let updates = encoder.encode(f).expect("encode");
        let cost = t.elapsed();
        if n > 0 {
            costs.push(cost);
            updates_last = updates.len();
        }
    }
    if costs.is_empty() {
        println!("{label:<34} (nothing measured)");
        return;
    }
    let total: Duration = costs.iter().sum();
    let avg = total / costs.len() as u32;
    let worst = costs.iter().max().copied().unwrap_or_default();
    let fps = if avg.is_zero() {
        f64::INFINITY
    } else {
        1.0 / avg.as_secs_f64()
    };
    println!(
        "{label:<34} avg {:>8.2} ms   worst {:>8.2} ms   ceiling {:>7.1} fps   updates {updates_last}",
        avg.as_secs_f64() * 1000.0,
        worst.as_secs_f64() * 1000.0,
        fps
    );
}

fn main() {
    println!("desktop {WIDTH}x{HEIGHT}, {} MB per frame", (WIDTH as usize * HEIGHT as usize * BPP) >> 20);
    println!("budget: 33.3 ms at 30 fps, 16.7 ms at 60 fps\n");

    const ROUNDS: u64 = 12;

    // An idle desktop: nothing changes between frames. The cheapest case, and
    // the one a static screen spends its time in.
    let mut enc = DamageEncoder::new();
    time_it(
        "damage, nothing changed",
        &mut enc,
        (0..ROUNDS).map(|n| frame(n, 0x20)).collect(),
    );

    // A cursor-sized change: the smallest thing worth sending.
    let mut enc = DamageEncoder::new();
    time_it(
        "damage, 32x32 changed",
        &mut enc,
        (0..ROUNDS)
            .map(|n| frame_with_rect(n, 0x20, 100, 100, 32, 32, ink(n)))
            .collect(),
    );

    // A window-sized change: typing in an editor, scrolling a page.
    let mut enc = DamageEncoder::new();
    time_it(
        "damage, 1280x720 changed",
        &mut enc,
        (0..ROUNDS)
            .map(|n| frame_with_rect(n, 0x20, 64, 64, 1280, 720, ink(n)))
            .collect(),
    );

    // Everything changed: Cmd+Tab, a full repaint, a video.
    let mut enc = DamageEncoder::new();
    time_it(
        "damage, whole screen changed",
        &mut enc,
        (0..ROUNDS)
            .map(|n| frame(n, ink(n)))
            .collect(),
    );

    // The other encoder, for reference: it never diffs, so its cost is the
    // floor for "send the whole screen".
    let mut enc = FullFrameEncoder::new();
    time_it(
        "full-frame, whole screen",
        &mut enc,
        (0..ROUNDS)
            .map(|n| frame(n, ink(n)))
            .collect(),
    );
}
