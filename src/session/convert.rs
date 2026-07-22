//! Translation between our types and `ironrdp-server`'s.
//!
//! Everything IronRDP-shaped is confined to this module tree so the protocol
//! engine can eventually be replaced (`specs/01-architecture.md` §1).

use core::num::{NonZeroU16, NonZeroUsize};

use anyhow::{Context as _, Result, bail};
use ironrdp_server::{BitmapUpdate, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat as RdpPixelFormat};

use crate::capture::{Frame, PixelFormat};
use crate::encoder::FrameUpdate;
use crate::input::{KeyEvent, MouseButton, PointerEvent};

// MS-RDPBCGR 2.2.8.1.2.2.5 (Fast-Path Synchronize Event). The flags type is not
// re-exported by ironrdp-server, so we read the raw bits.
const SYNC_SCROLL_LOCK: u8 = 0x01;
const SYNC_NUM_LOCK: u8 = 0x02;
const SYNC_CAPS_LOCK: u8 = 0x04;

/// Maps our pixel layout to IronRDP's.
///
/// `BgrA32` has channel offsets `[r=2, g=1, b=0, a=3]`, i.e. bytes in memory
/// are B, G, R, A — which is exactly `kCVPixelFormatType_32BGRA` (ADR-0005).
#[must_use]
pub fn pixel_format(format: PixelFormat) -> RdpPixelFormat {
    match format {
        PixelFormat::Bgra8 => RdpPixelFormat::BgrA32,
    }
}

/// Wraps a captured frame as a bitmap update covering `(x, y)` onwards.
pub fn bitmap_update(frame: &Frame, x: u16, y: u16) -> Result<BitmapUpdate> {
    let width = u16::try_from(frame.width)
        .ok()
        .and_then(NonZeroU16::new)
        .with_context(|| format!("frame width {} is not a valid RDP width", frame.width))?;
    let height = u16::try_from(frame.height)
        .ok()
        .and_then(NonZeroU16::new)
        .with_context(|| format!("frame height {} is not a valid RDP height", frame.height))?;
    let stride = NonZeroUsize::new(frame.stride).context("frame stride is zero")?;

    let needed = frame.required_len();
    if frame.data.len() < needed {
        bail!(
            "frame buffer is {} bytes but its geometry needs {needed}",
            frame.data.len()
        );
    }

    Ok(BitmapUpdate {
        x,
        y,
        width,
        height,
        format: pixel_format(frame.format),
        data: frame.data.clone(),
        stride,
    })
}

pub fn display_update(update: FrameUpdate) -> Result<DisplayUpdate> {
    match update {
        FrameUpdate::Full { frame } => Ok(DisplayUpdate::Bitmap(bitmap_update(&frame, 0, 0)?)),
    }
}

/// Converts a mouse event from the client.
///
/// Returns `None` for events wave 0 does not model.
#[must_use]
pub fn pointer_event(event: &MouseEvent) -> Option<PointerEvent> {
    let converted = match *event {
        MouseEvent::Move { x, y } => PointerEvent::MoveAbsolute {
            x: u32::from(x),
            y: u32::from(y),
        },
        MouseEvent::RelMove { x, y } => PointerEvent::MoveRelative { dx: x, dy: y },
        MouseEvent::LeftPressed => button(MouseButton::Left, true),
        MouseEvent::LeftReleased => button(MouseButton::Left, false),
        MouseEvent::RightPressed => button(MouseButton::Right, true),
        MouseEvent::RightReleased => button(MouseButton::Right, false),
        MouseEvent::MiddlePressed => button(MouseButton::Middle, true),
        MouseEvent::MiddleReleased => button(MouseButton::Middle, false),
        MouseEvent::Button4Pressed => button(MouseButton::Back, true),
        MouseEvent::Button4Released => button(MouseButton::Back, false),
        MouseEvent::Button5Pressed => button(MouseButton::Forward, true),
        MouseEvent::Button5Released => button(MouseButton::Forward, false),
        MouseEvent::VerticalScroll { value } => PointerEvent::Scroll {
            dx: 0,
            dy: i32::from(value),
        },
        MouseEvent::Scroll { x, y } => PointerEvent::Scroll { dx: x, dy: y },
    };
    Some(converted)
}

const fn button(button: MouseButton, pressed: bool) -> PointerEvent {
    PointerEvent::Button { button, pressed }
}

/// Converts a keyboard event from the client.
#[must_use]
pub fn key_event(event: &KeyboardEvent) -> Option<KeyEvent> {
    let converted = match *event {
        KeyboardEvent::Pressed { code, extended } => KeyEvent::Scancode {
            code,
            extended,
            pressed: true,
        },
        KeyboardEvent::Released { code, extended } => KeyEvent::Scancode {
            code,
            extended,
            pressed: false,
        },
        KeyboardEvent::UnicodePressed(ch) => KeyEvent::Unicode { ch, pressed: true },
        KeyboardEvent::UnicodeReleased(ch) => KeyEvent::Unicode { ch, pressed: false },
        KeyboardEvent::Synchronize(flags) => sync_locks(flags.bits()),
    };
    Some(converted)
}

/// Decodes the raw fast-path synchronize flags.
///
/// Split out because `SynchronizeFlags` is not re-exported by
/// `ironrdp-server`, so tests cannot construct one.
#[must_use]
const fn sync_locks(bits: u8) -> KeyEvent {
    KeyEvent::SyncLocks {
        caps: bits & SYNC_CAPS_LOCK != 0,
        num: bits & SYNC_NUM_LOCK != 0,
        scroll: bits & SYNC_SCROLL_LOCK != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::test_pattern;

    #[test]
    fn bgra_maps_to_the_format_whose_bytes_are_bgra() {
        let format = pixel_format(PixelFormat::Bgra8);
        assert_eq!(format, RdpPixelFormat::BgrA32);
        // Guards against an IronRDP rename flipping the meaning underneath us:
        // offsets are [red, green, blue, alpha] byte positions within a pixel.
        assert_eq!(format.channel_offsets(), [2, 1, 0, 3]);
        assert_eq!(format.bytes_per_pixel(), 4);
    }

    #[test]
    fn full_frame_becomes_a_bitmap_at_the_origin() {
        let frame = test_pattern::render(320, 240, 0);
        let update = display_update(FrameUpdate::Full { frame: frame.clone() }).expect("convert");

        let DisplayUpdate::Bitmap(bitmap) = update else {
            panic!("expected a bitmap update");
        };
        assert_eq!(bitmap.x, 0);
        assert_eq!(bitmap.y, 0);
        assert_eq!(bitmap.width.get(), 320);
        assert_eq!(bitmap.height.get(), 240);
        assert_eq!(bitmap.stride.get(), frame.stride);
        assert_eq!(bitmap.data.len(), frame.data.len());
        assert_eq!(bitmap.format, RdpPixelFormat::BgrA32);
    }

    #[test]
    fn stride_is_carried_through_untouched() {
        // The whole point of ADR-0005: no repacking between capture and the wire.
        let frame = test_pattern::render(1280, 720, 0);
        assert_ne!(frame.stride, 1280 * 4);
        let bitmap = bitmap_update(&frame, 0, 0).expect("convert");
        assert_eq!(bitmap.stride.get(), frame.stride);
    }

    #[test]
    fn short_buffer_is_rejected_rather_than_sent() {
        let mut frame = test_pattern::render(64, 64, 0);
        frame.data = frame.data.slice(0..frame.data.len() - 1);
        assert!(bitmap_update(&frame, 0, 0).is_err());
    }

    #[test]
    fn mouse_moves_convert() {
        assert_eq!(
            pointer_event(&MouseEvent::Move { x: 100, y: 200 }),
            Some(PointerEvent::MoveAbsolute { x: 100, y: 200 })
        );
        assert_eq!(
            pointer_event(&MouseEvent::RelMove { x: -3, y: 4 }),
            Some(PointerEvent::MoveRelative { dx: -3, dy: 4 })
        );
    }

    #[test]
    fn every_mouse_button_maps_to_a_distinct_button() {
        let cases = [
            (MouseEvent::LeftPressed, MouseButton::Left, true),
            (MouseEvent::LeftReleased, MouseButton::Left, false),
            (MouseEvent::RightPressed, MouseButton::Right, true),
            (MouseEvent::RightReleased, MouseButton::Right, false),
            (MouseEvent::MiddlePressed, MouseButton::Middle, true),
            (MouseEvent::MiddleReleased, MouseButton::Middle, false),
            (MouseEvent::Button4Pressed, MouseButton::Back, true),
            (MouseEvent::Button4Released, MouseButton::Back, false),
            (MouseEvent::Button5Pressed, MouseButton::Forward, true),
            (MouseEvent::Button5Released, MouseButton::Forward, false),
        ];
        for (event, expected_button, expected_pressed) in cases {
            assert_eq!(
                pointer_event(&event),
                Some(PointerEvent::Button {
                    button: expected_button,
                    pressed: expected_pressed
                }),
                "{event:?}"
            );
        }
    }

    #[test]
    fn scroll_events_convert() {
        assert_eq!(
            pointer_event(&MouseEvent::VerticalScroll { value: -120 }),
            Some(PointerEvent::Scroll { dx: 0, dy: -120 })
        );
        assert_eq!(
            pointer_event(&MouseEvent::Scroll { x: 5, y: -5 }),
            Some(PointerEvent::Scroll { dx: 5, dy: -5 })
        );
    }

    #[test]
    fn scancodes_keep_their_extended_flag() {
        assert_eq!(
            key_event(&KeyboardEvent::Pressed {
                code: 0x1E,
                extended: false
            }),
            Some(KeyEvent::Scancode {
                code: 0x1E,
                extended: false,
                pressed: true
            })
        );
        // Right Alt arrives as E0 38 — losing `extended` would make it Left Alt.
        assert_eq!(
            key_event(&KeyboardEvent::Released {
                code: 0x38,
                extended: true
            }),
            Some(KeyEvent::Scancode {
                code: 0x38,
                extended: true,
                pressed: false
            })
        );
    }

    #[test]
    fn lock_flags_decode_per_ms_rdpbcgr() {
        assert_eq!(
            sync_locks(0),
            KeyEvent::SyncLocks {
                caps: false,
                num: false,
                scroll: false
            }
        );
        assert_eq!(
            sync_locks(SYNC_CAPS_LOCK),
            KeyEvent::SyncLocks {
                caps: true,
                num: false,
                scroll: false
            }
        );
        assert_eq!(
            sync_locks(SYNC_NUM_LOCK | SYNC_SCROLL_LOCK),
            KeyEvent::SyncLocks {
                caps: false,
                num: true,
                scroll: true
            }
        );
        // KANA_LOCK (0x08) is defined by the spec but not modelled; it must not
        // bleed into another flag.
        assert_eq!(
            sync_locks(0x08),
            KeyEvent::SyncLocks {
                caps: false,
                num: false,
                scroll: false
            }
        );
    }

    #[test]
    fn unicode_events_convert() {
        assert_eq!(
            key_event(&KeyboardEvent::UnicodePressed(0x0119)),
            Some(KeyEvent::Unicode {
                ch: 0x0119,
                pressed: true
            })
        );
        assert_eq!(
            key_event(&KeyboardEvent::UnicodeReleased(0x0119)),
            Some(KeyEvent::Unicode {
                ch: 0x0119,
                pressed: false
            })
        );
    }
}
