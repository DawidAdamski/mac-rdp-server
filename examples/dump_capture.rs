//! Captures one real frame and writes it to a PPM file (F-002.21).
//!
//! The cheapest possible answer to "the image looks wrong": it takes the same
//! path as the RDP pipeline — same stride handling, same pixel format check —
//! minus the protocol, so a broken picture here points at capture and a broken
//! picture only in the client points at `session`.
//!
//! ```text
//! cargo run --example dump_capture -- /tmp/frame.ppm
//! sips -s format png /tmp/frame.ppm --out /tmp/frame.png
//! ```
//!
//! Needs Screen Recording permission, exactly like the server does.

use std::io::Write as _;

use anyhow::{Context as _, Result, bail};
use mac_rdp_server::capture::FrameSource as _;
use mac_rdp_server::capture::screen::ScreenCaptureKitSource;
use mac_rdp_server::perms::{self, Permission, PermissionState};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).init();

    let path = std::env::args().nth(1).unwrap_or_else(|| "frame.ppm".to_owned());
    let display: u32 = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(0);

    if perms::ensure(Permission::ScreenRecording) == PermissionState::Denied {
        bail!("{}", perms::explain(Permission::ScreenRecording));
    }

    let mut source = ScreenCaptureKitSource::new(display, 30)
        .await
        .context("initialising capture")?;
    for screen in source.displays()? {
        println!(
            "display {} — {}x{} px, scale {}",
            screen.id.0, screen.width_px, screen.height_px, screen.scale_factor
        );
    }

    let mut stream = source.start().await.context("starting capture")?;
    let frame = stream.next().await.context("waiting for a frame")?;
    source.stop().await.ok();

    println!(
        "frame {}x{}, stride {} ({} bytes of padding per row)",
        frame.width,
        frame.height,
        frame.stride,
        frame.stride - frame.width as usize * 4
    );

    let mut out = Vec::with_capacity(frame.data.len());
    write!(out, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let [b, g, r, _a] = frame.pixel(x, y).context("pixel within bounds")?;
            out.extend_from_slice(&[r, g, b]);
        }
    }

    std::fs::write(&path, out)?;
    println!("wrote {path}");
    Ok(())
}
