//! F-111 probe: can Rust own the menu bar with objc2 while tokio owns the
//! server? Measured, not assumed. Not part of the build.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusBarButton,
};
use objc2_foundation::{MainThreadMarker, NSString};

struct Ivars {
    running: Arc<AtomicBool>,
    button: Retained<NSStatusBarButton>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Toggle has no Drop.
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "F111Toggle"]
    #[ivars = Ivars]
    struct Toggle;

    impl Toggle {
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: Option<&AnyObject>) {
            let running = &self.ivars().running;
            let now = !running.load(Ordering::SeqCst);
            running.store(now, Ordering::SeqCst);
            println!("[main-thread callback] toggle: -> running={now}");
            let title = if now { "RDP*ON" } else { "RDP*OFF" };
            self.ivars().button.setTitle(&NSString::from_str(title));
        }
    }
);

impl Toggle {
    fn new(
        mtm: MainThreadMarker,
        running: Arc<AtomicBool>,
        button: Retained<NSStatusBarButton>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { running, button });
        unsafe { msg_send![super(this), init] }
    }
}

fn main() {
    let mtm = MainThreadMarker::new().expect("main() runs on the main thread");
    let running = Arc::new(AtomicBool::new(false));

    // The async side: a real tokio runtime on its own threads, started BEFORE
    // AppKit takes the main thread. This is the arrangement F-111 would need.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .expect("tokio runtime");
    let flag = Arc::clone(&running);
    rt.spawn(async move {
        let mut ticks = 0u32;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            ticks += 1;
            if ticks % 4 == 0 {
                println!(
                    "[tokio worker {:?}] alive, tick {ticks}, running={}",
                    std::thread::current().id(),
                    flag.load(Ordering::SeqCst)
                );
            }
        }
    });

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let bar = NSStatusBar::systemStatusBar();
    let item = bar.statusItemWithLength(-1.0); // NSVariableStatusItemLength
    let button = item.button(mtm).expect("a status item has a button");
    button.setTitle(&NSString::from_str("RDP*OFF"));

    let target = Toggle::new(mtm, Arc::clone(&running), button.clone());

    let menu = NSMenu::new(mtm);
    let toggle_item = NSMenuItem::new(mtm);
    toggle_item.setTitle(&NSString::from_str("Start server"));
    unsafe {
        toggle_item.setAction(Some(sel!(toggle:)));
        toggle_item.setTarget(Some(&target));
    }
    menu.addItem(&toggle_item);
    let quit = NSMenuItem::new(mtm);
    quit.setTitle(&NSString::from_str("Quit"));
    unsafe { quit.setAction(Some(sel!(terminate:))) };
    menu.addItem(&quit);
    item.setMenu(Some(&menu));

    // Where is the button, in screen coordinates? Needed to aim a screenshot.
    let frame = button.frame();
    if let Some(window) = button.window() {
        let on_screen = window.convertRectToScreen(frame);
        println!(
            "[probe] button on screen: x={} y={} w={} h={}, windowNumber={}",
            on_screen.origin.x,
            on_screen.origin.y,
            on_screen.size.width,
            on_screen.size.height,
            window.windowNumber()
        );
    }
    println!("[probe] isVisible={}", item.isVisible());

    // Prove the wiring without a human: dispatch the first menu item's action.
    menu.performActionForItemAtIndex(0);
    println!("[probe] after programmatic action, running={}", running.load(Ordering::SeqCst));

    // Quit by itself so the probe cannot hang a session.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(25));
        println!("[probe] 25 s elapsed, exiting");
        std::process::exit(0);
    });

    app.run();
}
