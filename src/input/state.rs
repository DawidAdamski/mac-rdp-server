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

    /// The modifier keys currently held, left before right. Caps Lock is a
    /// toggle rather than a held key, so it is never listed (F-003.22).
    #[must_use]
    pub fn held(self) -> Vec<Modifier> {
        [
            (self.shift_left, Modifier::ShiftLeft),
            (self.shift_right, Modifier::ShiftRight),
            (self.control_left, Modifier::ControlLeft),
            (self.control_right, Modifier::ControlRight),
            (self.alt_left, Modifier::AltLeft),
            (self.alt_right, Modifier::AltRight),
            (self.command_left, Modifier::CommandLeft),
            (self.command_right, Modifier::CommandRight),
        ]
        .into_iter()
        .filter_map(|(held, modifier)| held.then_some(modifier))
        .collect()
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
///
/// Every button is tracked on its own, not only "some other button": a reset
/// has to release exactly the buttons that are down (F-003.22), and releasing
/// Back must not forget that Middle is still held.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Buttons {
    left: bool,
    right: bool,
    middle: bool,
    back: bool,
    forward: bool,
}

impl Buttons {
    pub fn set(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => self.left = pressed,
            MouseButton::Right => self.right = pressed,
            MouseButton::Middle => self.middle = pressed,
            MouseButton::Back => self.back = pressed,
            MouseButton::Forward => self.forward = pressed,
        }
    }

    /// The buttons currently held, in a fixed order (F-003.22).
    #[must_use]
    pub fn held(self) -> Vec<MouseButton> {
        [
            (self.left, MouseButton::Left),
            (self.right, MouseButton::Right),
            (self.middle, MouseButton::Middle),
            (self.back, MouseButton::Back),
            (self.forward, MouseButton::Forward),
        ]
        .into_iter()
        .filter_map(|(held, button)| held.then_some(button))
        .collect()
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
        } else if self.middle || self.back || self.forward {
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

/// Why the input state is being reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetCause {
    /// The client sent a Synchronize event (MS-RDPBCGR 2.2.8.1.1.3.1.1.5): it
    /// resets the server's key state to "all up" and sets the toggle keys.
    Sync { caps_lock: bool },
    /// The RDP connection ended, cleanly or not (F-003.24). Toggle keys are
    /// left alone: nobody told us their state changed.
    SessionEnded,
}

impl ResetCause {
    /// Short name for the log line (F-003.26).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sync { .. } => "sync",
            Self::SessionEnded => "disconnect",
        }
    }
}

/// One step of a reset, in the order it has to be injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetAction {
    ReleaseModifier(Modifier),
    ReleaseButton(MouseButton),
    SetCapsLock(bool),
}

/// Decides what a reset has to do, without doing it (F-003.25).
///
/// Every held modifier and button is released, and only **then** is Caps Lock
/// set from the Synchronize event (F-003.22) — releases carry the flags of the
/// state they leave behind, so they must not already see the new lock state.
/// `cgevent` turns each action into a real event; [`apply`] replays them on the
/// state.
#[must_use]
pub fn plan_reset(modifiers: Modifiers, buttons: Buttons, cause: ResetCause) -> Vec<ResetAction> {
    let mut actions: Vec<ResetAction> = modifiers.held().into_iter().map(ResetAction::ReleaseModifier).collect();
    actions.extend(buttons.held().into_iter().map(ResetAction::ReleaseButton));
    if let ResetCause::Sync { caps_lock } = cause {
        actions.push(ResetAction::SetCapsLock(caps_lock));
    }
    actions
}

/// Applies one reset action to the state.
pub fn apply(modifiers: &mut Modifiers, buttons: &mut Buttons, action: ResetAction) {
    match action {
        ResetAction::ReleaseModifier(modifier) => {
            modifiers.set(modifier, false);
        }
        ResetAction::ReleaseButton(button) => buttons.set(button, false),
        ResetAction::SetCapsLock(on) => modifiers.set_caps_lock(on),
    }
}

/// How many modifiers and buttons a plan releases, for the log (F-003.26).
#[must_use]
pub fn released_counts(actions: &[ResetAction]) -> (usize, usize) {
    actions.iter().fold((0, 0), |(mods, buttons), action| match action {
        ResetAction::ReleaseModifier(_) => (mods + 1, buttons),
        ResetAction::ReleaseButton(_) => (mods, buttons + 1),
        ResetAction::SetCapsLock(_) => (mods, buttons),
    })
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

    /// F-007.17b. The bug this guards: the origin is read once at startup, and
    /// attaching a display re-lays-out the global point space underneath it, so
    /// the same desktop pixel has to resolve to a different macOS point.
    #[test]
    fn the_same_pixel_maps_elsewhere_after_the_display_moves() {
        // Captured display starts at the origin; the laptop lid then opens and
        // macOS pushes the external screen to the right of the built-in one.
        let before = ScreenGeometry::new(0.0, 0.0, 1.0);
        let after = ScreenGeometry::new(1512.0, 0.0, 1.0);

        assert_eq!(before.pixel_to_point(0, 0), (0.0, 0.0));
        assert_eq!(
            after.pixel_to_point(0, 0),
            (1512.0, 0.0),
            "the left edge of the RDP desktop must follow the display, not stay at zero"
        );
        assert_ne!(before.pixel_to_point(100, 100), after.pixel_to_point(100, 100));
    }

    #[test]
    fn a_scale_change_alone_remaps_the_desktop() {
        // Same position, mirrored into a Retina mode: the point space halves.
        let before = ScreenGeometry::new(100.0, 50.0, 1.0);
        let after = ScreenGeometry::new(100.0, 50.0, 2.0);
        assert_eq!(before.pixel_to_point(400, 200), (500.0, 250.0));
        assert_eq!(after.pixel_to_point(400, 200), (300.0, 150.0));
    }

    #[test]
    fn zero_scale_is_treated_as_one() {
        let geom = ScreenGeometry::new(0.0, 0.0, 0.0);
        assert_eq!(geom.pixel_to_point(10, 10), (10.0, 10.0));
    }

    fn replay(mut modifiers: Modifiers, mut buttons: Buttons, actions: &[ResetAction]) -> (Modifiers, Buttons) {
        for &action in actions {
            apply(&mut modifiers, &mut buttons, action);
        }
        (modifiers, buttons)
    }

    /// F-003.22, F-003.25 (AC-11): everything held is released, exactly once.
    #[test]
    fn f003_22_sync_releases_every_held_modifier_and_button() {
        let mut modifiers = Modifiers::default();
        modifiers.set(Modifier::ShiftLeft, true);
        modifiers.set(Modifier::CommandRight, true);
        let mut buttons = Buttons::default();
        buttons.set(MouseButton::Left, true);

        let plan = plan_reset(modifiers, buttons, ResetCause::Sync { caps_lock: false });
        assert_eq!(
            plan,
            vec![
                ResetAction::ReleaseModifier(Modifier::ShiftLeft),
                ResetAction::ReleaseModifier(Modifier::CommandRight),
                ResetAction::ReleaseButton(MouseButton::Left),
                ResetAction::SetCapsLock(false),
            ]
        );

        let (modifiers, buttons) = replay(modifiers, buttons, &plan);
        assert_eq!(modifiers.cg_flags(), 0, "no modifier may survive the reset");
        assert_eq!(
            buttons.move_kind(),
            MoveKind::Move,
            "a move after the reset is not a drag"
        );
    }

    /// F-003.25 (AC-11): nothing held, nothing to release.
    #[test]
    fn f003_25_reset_of_an_idle_state_releases_nothing() {
        let plan = plan_reset(Modifiers::default(), Buttons::default(), ResetCause::SessionEnded);
        assert!(plan.is_empty());
    }

    /// F-003.22 (AC-11): releases come before the toggle keys are set.
    #[test]
    fn f003_22_releases_precede_the_caps_lock_update() {
        let mut modifiers = Modifiers::default();
        modifiers.set(Modifier::ControlLeft, true);
        let plan = plan_reset(modifiers, Buttons::default(), ResetCause::Sync { caps_lock: true });

        assert_eq!(
            plan.last(),
            Some(&ResetAction::SetCapsLock(true)),
            "the lock is set last"
        );
        let (modifiers, _) = replay(modifiers, Buttons::default(), &plan);
        assert_eq!(
            modifiers.cg_flags(),
            flag::ALPHA_SHIFT,
            "Caps Lock on, Control released"
        );
    }

    /// F-003.24 (AC-11): a disconnect releases keys but leaves Caps Lock alone.
    #[test]
    fn f003_24_disconnect_does_not_touch_caps_lock() {
        let mut modifiers = Modifiers::default();
        modifiers.set_caps_lock(true);
        modifiers.set(Modifier::CommandLeft, true);
        let mut buttons = Buttons::default();
        buttons.set(MouseButton::Right, true);

        let plan = plan_reset(modifiers, buttons, ResetCause::SessionEnded);
        assert!(!plan.iter().any(|a| matches!(a, ResetAction::SetCapsLock(_))));

        let (modifiers, buttons) = replay(modifiers, buttons, &plan);
        assert_eq!(modifiers.cg_flags(), flag::ALPHA_SHIFT, "Cmd released, Caps Lock kept");
        assert!(buttons.held().is_empty());
    }

    /// F-003.23 (AC-11): a key re-sent by the client after the reset counts.
    #[test]
    fn f003_23_a_press_after_the_reset_is_held_again() {
        let mut modifiers = Modifiers::default();
        modifiers.set(Modifier::ShiftLeft, true);
        let plan = plan_reset(modifiers, Buttons::default(), ResetCause::Sync { caps_lock: false });
        let (mut modifiers, _) = replay(modifiers, Buttons::default(), &plan);

        assert!(
            modifiers.set(Modifier::ShiftLeft, true),
            "the re-sent press must register"
        );
        assert_eq!(modifiers.cg_flags(), flag::SHIFT);
    }

    /// F-003.22: each extra button is released on its own, so one release
    /// cannot hide another button that is still down.
    #[test]
    fn f003_22_other_buttons_are_tracked_individually() {
        let mut buttons = Buttons::default();
        buttons.set(MouseButton::Middle, true);
        buttons.set(MouseButton::Back, true);
        buttons.set(MouseButton::Back, false);
        assert_eq!(buttons.held(), vec![MouseButton::Middle]);
        assert_eq!(buttons.move_kind(), MoveKind::OtherDrag, "Middle still drags");
    }

    /// F-003.26: the log line counts what was released.
    #[test]
    fn f003_26_released_counts_split_modifiers_and_buttons() {
        let actions = [
            ResetAction::ReleaseModifier(Modifier::ShiftLeft),
            ResetAction::ReleaseModifier(Modifier::AltRight),
            ResetAction::ReleaseButton(MouseButton::Left),
            ResetAction::SetCapsLock(true),
        ];
        assert_eq!(released_counts(&actions), (2, 1));
        assert_eq!(ResetCause::Sync { caps_lock: true }.label(), "sync");
        assert_eq!(ResetCause::SessionEnded.label(), "disconnect");
    }
}
