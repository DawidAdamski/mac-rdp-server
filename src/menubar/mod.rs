//! The menu-bar icon: the server without a terminal (F-111).
//!
//! Three rules shape this module, and all three come from the spec rather than
//! from taste:
//!
//! - **The main thread belongs to AppKit, and only in this mode** (F-111.16,
//!   ADR-0015). The tokio runtime is built before `NSApp.run()` takes the
//!   thread for good, and the server runs on its own threads throughout.
//! - **`menubar` knows macOS and does not know RDP** (F-111.14), the mirror of
//!   `session` knowing RDP and not macOS. Everything protocol-shaped is behind
//!   [`crate::service`]; `tests/architecture.rs` enforces the half of that
//!   which can be enforced.
//! - **Starting the program is not starting the service** (F-111.18). Nothing
//!   below starts anything: the first click does.
//!
//! The UI is a poll, not a push: a timer reads the shared state three times a
//! second and repaints if it changed. The alternative — waking the main thread
//! from a tokio task — buys invisible latency at the price of main-thread
//! dispatch in every state transition, and this is a menu, not a frame loop.

// Third unsafe site in the crate, after `perms` (four C functions) and one
// expression in `capture::screen` (ADR-0016). AppKit cannot be driven without
// it: defining an Objective-C class, pointing a menu item at a selector and
// scheduling a timer are all unsafe operations in `objc2`. ADR-0015 §Decyzja
// is where the choice to take that cost is argued.
#![allow(unsafe_code)]

pub mod state;
/// Public for the sake of being testable: the switch, the service threads and
/// the socket they open have nothing to do with AppKit, so `tests/menubar_service.rs`
/// drives them directly and leaves only the visual half of F-111 to a human
/// (AC-8 is checked at the socket level there, AC-4 … AC-12 by eye).
pub mod supervisor;

use std::cell::RefCell;
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{Context as _, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusBarButton,
    NSStatusItem, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSString, NSTimer, NSURL};
use tracing::{info, warn};

use self::state::{Icon, ServiceState, Toggle};
use self::supervisor::Supervisor;
use crate::config::Config;
use crate::perms::{self, Permission, PermissionState};

/// `NSVariableStatusItemLength`: the item is as wide as its content.
const VARIABLE_LENGTH: f64 = -1.0;

/// How often the menu re-reads the service state.
///
/// Fast enough that a click feels answered, slow enough to be free: the work
/// behind one tick is a mutex lock, two permission checks and — only when
/// something actually changed — a few `setTitle:` calls.
const REFRESH_INTERVAL: f64 = 0.33;

/// The permissions shown in the menu, in the order they appear (F-111.7).
const PERMISSIONS: [Permission; 2] = [Permission::ScreenRecording, Permission::Accessibility];

/// What the menu is currently showing, so a tick that changes nothing costs
/// nothing.
struct Rendered {
    state: ServiceState,
    permissions: Vec<(Permission, PermissionState)>,
}

struct Ivars {
    toggle: Arc<Mutex<Toggle>>,
    supervisor: Supervisor,
    /// Held for its lifetime: dropping the status item removes the icon from
    /// the bar, which looks exactly like a crash.
    #[expect(
        dead_code,
        reason = "holding the status item is what keeps the icon in the menu bar; nothing reads it"
    )]
    item: Retained<NSStatusItem>,
    button: Retained<NSStatusBarButton>,
    status_item: Retained<NSMenuItem>,
    address_item: Retained<NSMenuItem>,
    action_item: Retained<NSMenuItem>,
    permission_items: Vec<Retained<NSMenuItem>>,
    rendered: RefCell<Option<Rendered>>,
}

define_class!(
    // SAFETY: NSObject imposes no subclassing requirements, and `Menu` has no
    // `Drop` — the status item it holds is released by AppKit when the process
    // ends.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MacRdpServerMenu"]
    #[ivars = Ivars]
    struct Menu;

    impl Menu {
        /// The switch (F-111.5).
        #[unsafe(method(toggleServer:))]
        fn toggle_server(&self, _sender: Option<&AnyObject>) {
            let request = lock(&self.ivars().toggle).click();
            match request {
                Some(request) => {
                    // F-111.17: a click is a state change, and a state change
                    // leaves a line in the log even when nobody is watching a
                    // terminal.
                    info!(?request, "menu clicked");
                    self.ivars().supervisor.request(request);
                }
                // Reachable only if AppKit sends an action for a disabled item.
                None => info!("menu clicked while the server was still starting or stopping; ignored"),
            }
            self.refresh();
        }

        /// Repaints from the shared state.
        #[unsafe(method(refreshFromState:))]
        fn refresh_from_state(&self, _sender: Option<&AnyObject>) {
            self.refresh();
        }

        /// F-111.8: ends the process as cleanly as `Ctrl-C` does.
        #[unsafe(method(quitServer:))]
        fn quit_server(&self, _sender: Option<&AnyObject>) {
            info!("quit chosen from the menu");
            // Waits for the service to come down, so a connected client gets a
            // closed socket instead of a process that simply vanished.
            self.ivars().supervisor.quit();
            // `exit` rather than `NSApplication::terminate`: the teardown that
            // matters has already happened above, and terminate would never
            // return here anyway.
            std::process::exit(0);
        }

        #[unsafe(method(openScreenRecordingSettings:))]
        fn open_screen_recording_settings(&self, _sender: Option<&AnyObject>) {
            open_settings(Permission::ScreenRecording);
        }

        #[unsafe(method(openAccessibilitySettings:))]
        fn open_accessibility_settings(&self, _sender: Option<&AnyObject>) {
            open_settings(Permission::Accessibility);
        }
    }
);

impl Menu {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: `init` on a direct NSObject subclass, called once on the
        // object just allocated.
        unsafe { msg_send![super(this), init] }
    }

    /// Brings the icon and every menu line in step with the service state.
    fn refresh(&self) {
        let ivars = self.ivars();
        // Cloned so the lock is released before any AppKit call: the service
        // reports its state from a tokio thread, and holding this across a
        // repaint would couple the two for no reason.
        let toggle = lock(&ivars.toggle).clone();
        let permissions = perms::survey();

        let unchanged = ivars
            .rendered
            .borrow()
            .as_ref()
            .is_some_and(|last| last.state == *toggle.state() && last.permissions == permissions);
        if unchanged {
            return;
        }

        let icon = toggle.icon();
        match symbol_image(icon) {
            Some(image) => {
                // F-111.9: a template image is the one that follows a light or
                // dark menu bar by itself.
                image.setTemplate(true);
                ivars.button.setImage(Some(&image));
                ivars.button.setTitle(&NSString::from_str(""));
            }
            None => {
                // An icon that failed to load leaves a blank clickable gap,
                // which is indistinguishable from the icon not being there.
                ivars.button.setImage(None);
                ivars.button.setTitle(&NSString::from_str(icon.fallback_text()));
            }
        }

        ivars.status_item.setTitle(&NSString::from_str(&toggle.status_line()));
        ivars.address_item.setTitle(&NSString::from_str(&toggle.address_line()));
        ivars.action_item.setTitle(&NSString::from_str(toggle.action_title()));
        ivars.action_item.setEnabled(toggle.is_actionable());

        for (item, &(permission, state)) in ivars.permission_items.iter().zip(permissions.iter()) {
            item.setTitle(&NSString::from_str(&permission_line(permission, state)));
        }

        *ivars.rendered.borrow_mut() = Some(Rendered {
            state: toggle.state().clone(),
            permissions,
        });
    }
}

/// Runs the menu bar until the process ends.
///
/// Takes the main thread for AppKit and never gives it back, which is why it
/// returns only on an error setting up: `NSApplication::run` ends by exiting
/// the process (F-111.8).
pub fn run(config: &Config) -> Result<()> {
    let mtm = MainThreadMarker::new().context("the menu bar needs the main thread")?;

    let toggle = Arc::new(Mutex::new(Toggle::new(config.socket_addr())));
    let supervisor = supervisor::spawn(config.clone(), Arc::clone(&toggle));

    let app = NSApplication::sharedApplication(mtm);
    // F-111.2: no Dock icon, no stealing focus at startup.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(VARIABLE_LENGTH);
    let button = item.button(mtm).context("a status item without a button")?;

    let status_item = NSMenuItem::new(mtm);
    let address_item = NSMenuItem::new(mtm);
    let action_item = NSMenuItem::new(mtm);
    let permission_items: Vec<_> = PERMISSIONS.iter().map(|_| NSMenuItem::new(mtm)).collect();
    let quit_item = NSMenuItem::new(mtm);
    quit_item.setTitle(&NSString::from_str("Quit mac-rdp-server"));

    let menu = NSMenu::new(mtm);
    // Our own enabling, not AppKit's: the toggle item is greyed out while a
    // start or a stop is in flight, and automatic validation knows nothing
    // about that.
    menu.setAutoenablesItems(false);
    menu.addItem(&status_item);
    menu.addItem(&address_item);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&action_item);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    for permission_item in &permission_items {
        menu.addItem(permission_item);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&quit_item);
    item.setMenu(Some(&menu));

    let target = Menu::new(
        mtm,
        Ivars {
            toggle,
            supervisor,
            item: item.clone(),
            button,
            status_item,
            address_item,
            action_item: action_item.clone(),
            permission_items: permission_items.clone(),
            rendered: RefCell::new(None),
        },
    );

    // SAFETY: every selector below is defined on `Menu` above, and `target`
    // outlives the menu items because the items are retained by the menu, the
    // menu by the status item, and the status item by `target` itself.
    unsafe {
        action_item.setTarget(Some(&target));
        action_item.setAction(Some(sel!(toggleServer:)));
        quit_item.setTarget(Some(&target));
        quit_item.setAction(Some(sel!(quitServer:)));
        for (permission_item, permission) in permission_items.iter().zip(PERMISSIONS) {
            permission_item.setTarget(Some(&target));
            permission_item.setAction(Some(match permission {
                Permission::ScreenRecording => sel!(openScreenRecordingSettings:),
                Permission::Accessibility => sel!(openAccessibilitySettings:),
            }));
            // Clicking a granted permission opens the panel too: "show me
            // where this is" is a reasonable thing to want from the menu, and
            // a disabled row would answer it with silence.
            permission_item.setEnabled(true);
        }
        // The status lines are labels. Disabled is how a label looks.
        NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            REFRESH_INTERVAL,
            &target,
            sel!(refreshFromState:),
            None,
            true,
        );
    }

    // Paint once before the first tick, so the menu is never briefly empty.
    target.refresh();

    info!(
        addr = %config.socket_addr(),
        "menu bar ready; the server is NOT listening until you start it from the menu"
    );

    app.run();
    // `NSApplication::run` does not return; anything here would be unreachable
    // in practice, and `Ok(())` is the honest shape for "the app ended".
    Ok(())
}

/// Loads an SF Symbol for the icon, or nothing if this system has no such
/// symbol.
fn symbol_image(icon: Icon) -> Option<Retained<NSImage>> {
    let name = NSString::from_str(icon.symbol());
    let description = NSString::from_str("mac-rdp-server status");
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, Some(&description));
    if image.is_none() {
        warn!(symbol = icon.symbol(), "no such system symbol; using a text icon");
    }
    image
}

fn permission_line(permission: Permission, state: PermissionState) -> String {
    let mark = match state {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "NOT granted — click to open Settings",
    };
    format!("{}: {mark}", permission.name())
}

/// Opens the Settings panel that grants `permission` (F-111.7).
fn open_settings(permission: Permission) {
    let url = perms::settings_url(permission);
    match NSURL::URLWithString(&NSString::from_str(url)) {
        Some(parsed) => {
            let opened = NSWorkspace::sharedWorkspace().openURL(&parsed);
            info!(permission = permission.name(), opened, "opening the Settings panel");
        }
        // Unreachable with the constants in `perms`, but a panic in a menu
        // action would take the whole icon down with it.
        None => warn!(url, "could not build the Settings URL"),
    }
}

/// Recovers a poisoned lock rather than propagating the panic: see
/// [`supervisor`] for why this matters on this side too.
fn lock(toggle: &Mutex<Toggle>) -> MutexGuard<'_, Toggle> {
    toggle.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
