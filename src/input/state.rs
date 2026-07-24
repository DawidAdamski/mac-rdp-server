//! Pure input state: modifier flags, button state, coordinate mapping.
//!
//! Everything here is testable without macOS. The actual `CGEvent` calls live
//! in `cgevent.rs`; this module decides *what* to inject, that one decides
//! *how*.

use super::MouseButton;
use super::keymap::Modifier;

/// The macOS event flag bits we set. Values mirror `CGEventFlags`; a test in
/// `cgevent.rs` asserts they still match the crate.
pub mod flag {
    pub const ALPHA_SHIFT: u64 = 0x0001_0000; // Caps Lock
    pub const SHIFT: u64 = 0x0002_0000;
    pub const CONTROL: u64 = 0x0004_0000;
    pub const ALTERNATE: u64 = 0x0008_0000; // Option / Alt
    pub const COMMAND: u64 = 0x0010_0000;
}

/// Tracks which modifiers are held, so `CGEventFlags` can be rebuilt for every
/// event (F-003.12) — macOS does not remember this for synthetic events.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    shift_left: bool,
    shift_right: bool,
    control_left: bool,
    control_right: bool,
    alt_left: bool,
    alt_right: bool,
    command_left: bool,
    command_right: bool,
    caps_lock: bool,
}

impl Modifiers {
    /// Records a modifier key going down or up. Returns `true` if this changed
    /// the state (so the caller can skip emitting a redundant FlagsChanged).
    pub fn set(&mut self, modifier: Modifier, pressed: bool) -> bool {
        let slot = match modifier {
            Modifier::ShiftLeft => &mut self.shift_left,
            Modifier::ShiftRight => &mut self.shift_right,
            Modifier::ControlLeft => &mut self.control_left,
            Modifier::ControlRight => &mut self.control_right,
            Modifier::AltLeft => &mut self.alt_left,
            Modifier::AltRight => &mut self.alt_right,
            Modifier::CommandLeft => &mut self.command_left,
            Modifier::CommandRight => &mut self.command_right,
            // Caps Lock is a toggle: a press flips it, a release does nothing.
            Modifier::CapsLock => {
                if pressed {
                    self.caps_lock = !self.caps_lock;
                    return true;
                }
                return false;
            }
        };
        let changed = *slot != pressed;
        *slot = pressed;
        changed
    }

    /// Forces Caps Lock to a known state (client focus sync, F-003.13).
    pub fn set_caps_lock(&mut self, on: bool) {
        self.caps_lock = on;
    }

    /// The `CGEventFlags` bits for the current state, to stamp on every event.
    #[must_use]
    pub fn cg_flags(self) -> u64 {
        let mut bits = 0;
        if self.shift_left || self.shift_right {
            bits |= flag::SHIFT;
        }
        if self.control_left || self.control_right {
            bits |= flag::CONTROL;
        }
        if self.alt_left || self.alt_right {
            bits |= flag::ALTERNATE;
        }
        if self.command_left || self.command_right {
            bits |= flag::COMMAND;
        }
        if self.caps_lock {
            bits |= flag::ALPHA_SHIFT;
        }
        bits
    }
}

/// Which mouse buttons are currently held, so a move becomes a drag (F-003.8).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Buttons {
    left: bool,
    right: bool,
    other: bool,
}

impl Buttons {
    pub fn set(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => self.left = pressed,
            MouseButton::Right => self.right = pressed,
            MouseButton::Middle | MouseButton::Back | MouseButton::Forward => self.other = pressed,
        }
    }

    /// Which kind of move event a bare pointer move should become, given what
    /// is held. Left drag wins over right, matching how macOS reports a move
    /// while several buttons are down.
    #[must_use]
    pub fn move_kind(self) -> MoveKind {
        if self.left {
            MoveKind::LeftDrag
        } else if self.right {
            MoveKind::RightDrag
        } else if self.other {
            MoveKind::OtherDrag
        } else {
            MoveKind::Move
        }
    }
}

/// The move-event flavour to synthesize (maps to a `CGEventType` in `cgevent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    Move,
    LeftDrag,
    RightDrag,
    OtherDrag,
}

/// Geometry of the captured display, for turning RDP desktop pixels into the
/// global macOS point coordinates `CGEvent` wants (F-003.6).
///
/// The RDP desktop is the display's backing store in **pixels**; macOS events
/// use **points**, and the display sits at `origin` in the global point space.
/// So: `point = origin + pixel / scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenGeometry {
    pub origin_x: f64,
    pub origin_y: f64,
    pub scale: f64,
}

impl ScreenGeometry {
    #[must_use]
    pub fn new(origin_x: f64, origin_y: f64, scale: f64) -> Self {
        // A zero scale would map everything onto the origin; guard it.
        let scale = if scale > 0.0 { scale } else { 1.0 };
        Self {
            origin_x,
            origin_y,
            scale,
        }
    }

    /// Converts an RDP desktop pixel to a global macOS point.
    #[must_use]
    pub fn pixel_to_point(self, x: u32, y: u32) -> (f64, f64) {
        (
            self.origin_x + f64::from(x) / self.scale,
            self.origin_y + f64::from(y) / self.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_combine_into_flags() {
        let mut mods = Modifiers::default();
        assert_eq!(mods.cg_flags(), 0);

        assert!(mods.set(Modifier::ShiftLeft, true));
        assert_eq!(mods.cg_flags(), flag::SHIFT);

        assert!(mods.set(Modifier::CommandLeft, true));
        assert_eq!(mods.cg_flags(), flag::SHIFT | flag::COMMAND);

        assert!(mods.set(Modifier::ShiftLeft, false));
        assert_eq!(mods.cg_flags(), flag::COMMAND);
    }

    #[test]
    fn either_shift_key_sets_the_same_flag() {
        let mut mods = Modifiers::default();
        mods.set(Modifier::ShiftRight, true);
        assert_eq!(mods.cg_flags(), flag::SHIFT);
        // Releasing the *other* shift must not clear it.
        mods.set(Modifier::ShiftLeft, false);
        assert_eq!(mods.cg_flags(), flag::SHIFT);
    }

    #[test]
    fn no_stuck_shift_after_release() {
        // Guards AC-10: a released modifier must not linger.
        let mut mods = Modifiers::default();
        mods.set(Modifier::ShiftLeft, true);
        mods.set(Modifier::ShiftLeft, false);
        assert_eq!(mods.cg_flags(), 0);
    }

    #[test]
    fn setting_a_held_modifier_again_reports_no_change() {
        let mut mods = Modifiers::default();
        assert!(mods.set(Modifier::ControlLeft, true));
        assert!(!mods.set(Modifier::ControlLeft, true), "repeat must report unchanged");
    }

    #[test]
    fn caps_lock_toggles_on_press_and_ignores_release() {
        let mut mods = Modifiers::default();
        assert!(mods.set(Modifier::CapsLock, true));
        assert_eq!(mods.cg_flags(), flag::ALPHA_SHIFT);
        assert!(!mods.set(Modifier::CapsLock, false), "release must not toggle");
        assert_eq!(mods.cg_flags(), flag::ALPHA_SHIFT, "still on after release");
        assert!(mods.set(Modifier::CapsLock, true), "next press toggles off");
        assert_eq!(mods.cg_flags(), 0);
    }

    #[test]
    fn caps_lock_can_be_forced_for_focus_sync() {
        let mut mods = Modifiers::default();
        mods.set_caps_lock(true);
        assert_eq!(mods.cg_flags(), flag::ALPHA_SHIFT);
        mods.set_caps_lock(false);
        assert_eq!(mods.cg_flags(), 0);
    }

    #[test]
    fn a_move_is_a_drag_only_while_a_button_is_held() {
        let mut buttons = Buttons::default();
        assert_eq!(buttons.move_kind(), MoveKind::Move);

        buttons.set(MouseButton::Left, true);
        assert_eq!(buttons.move_kind(), MoveKind::LeftDrag);

        buttons.set(MouseButton::Left, false);
        assert_eq!(buttons.move_kind(), MoveKind::Move, "no stuck drag after release");
    }

    #[test]
    fn left_drag_wins_over_right() {
        let mut buttons = Buttons::default();
        buttons.set(MouseButton::Right, true);
        buttons.set(MouseButton::Left, true);
        assert_eq!(buttons.move_kind(), MoveKind::LeftDrag);
    }

    #[test]
    fn extra_buttons_count_as_other_drag() {
        let mut buttons = Buttons::default();
        buttons.set(MouseButton::Middle, true);
        assert_eq!(buttons.move_kind(), MoveKind::OtherDrag);
    }

    #[test]
    fn non_retina_maps_pixels_one_to_one_from_the_origin() {
        let geom = ScreenGeometry::new(0.0, 0.0, 1.0);
        assert_eq!(geom.pixel_to_point(0, 0), (0.0, 0.0));
        assert_eq!(geom.pixel_to_point(1920, 1080), (1920.0, 1080.0));
    }

    #[test]
    fn retina_halves_pixels_into_points() {
        // A 2880x1800 backing store is a 1440x900 point space.
        let geom = ScreenGeometry::new(0.0, 0.0, 2.0);
        assert_eq!(geom.pixel_to_point(2880, 1800), (1440.0, 900.0));
        // The corner of the desktop must map to the corner, not the middle (AC-2).
        assert_eq!(geom.pixel_to_point(0, 0), (0.0, 0.0));
    }

    #[test]
    fn a_secondary_display_is_offset_by_its_origin() {
        // A second screen to the right of a 1920-wide primary.
        let geom = ScreenGeometry::new(1920.0, 0.0, 1.0);
        assert_eq!(geom.pixel_to_point(0, 0), (1920.0, 0.0));
        assert_eq!(geom.pixel_to_point(100, 50), (2020.0, 50.0));
    }

    #[test]
    fn zero_scale_is_treated_as_one() {
        let geom = ScreenGeometry::new(0.0, 0.0, 0.0);
        assert_eq!(geom.pixel_to_point(10, 10), (10.0, 10.0));
    }
}
