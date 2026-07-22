//! Writes the wave 0 test pattern to a PPM file for visual inspection.
//!
//! Lets AC-5 (colour channels) and AC-6 (orientation, stride) be checked
//! without an RDP client — and, from wave 1 on, lets a captured frame be
//! compared against a known-good reference.
//!
//! ```text
//! cargo run --example dump_pattern -- /tmp/pattern.ppm
//! sips -s format png /tmp/pattern.ppm --out /tmp/pattern.png
//! ```

use std::io::Write as _;

use mac_rdp_server::capture::placeholder::{self, Placeholder};
use mac_rdp_server::capture::test_pattern;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "pattern.ppm".to_owned());
    let what = std::env::args().nth(2).unwrap_or_else(|| "pattern".to_owned());

    let (width, height) = (test_pattern::DEFAULT_WIDTH, test_pattern::DEFAULT_HEIGHT);
    let frame = match what.as_str() {
        "denied" => placeholder::render(width, height, Placeholder::PermissionDenied),
        "waiting" => placeholder::render(width, height, Placeholder::WaitingForFirstFrame),
        counter => test_pattern::render(width, height, counter.parse().unwrap_or(42)),
    };

    // PPM is RGB, the frame is BGRA. Doing the swap explicitly here is the point:
    // if the bars come out in the wrong order, the bug is in the pattern rather
    // than hidden inside a library.
    let mut out = Vec::with_capacity(frame.data.len());
    write!(out, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let [b, g, r, _a] = frame.pixel(x, y).expect("pixel within bounds");
            out.extend_from_slice(&[r, g, b]);
        }
    }

    std::fs::write(&path, out)?;
    println!("wrote {path} ({}x{}, {what})", frame.width, frame.height);
    Ok(())
}
