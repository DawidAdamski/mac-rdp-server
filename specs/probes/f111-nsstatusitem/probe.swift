// F-111 probe: can a plain CLI binary — no .app bundle, no Info.plist — own a
// menu bar item, and does its menu open? Nothing here is project code; this
// only answers the question the spec needs before it can be written.

import AppKit
import Foundation

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "."

func log(_ s: String) { print(s); fflush(stdout) }

func capture(_ name: String, _ region: String) {
    let p = Process()
    p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    p.arguments = ["-x", "-R", region, "\(outDir)/\(name).png"]
    try? p.run()
    p.waitUntilExit()
    log("screencapture \(name) region=\(region) exit=\(p.terminationStatus)")
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)   // no Dock icon, no menu bar of its own

let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
item.button?.title = "RDPPROBE"

let menu = NSMenu()
let state = NSMenuItem(title: "Server: off", action: nil, keyEquivalent: "")
state.isEnabled = false
menu.addItem(state)
menu.addItem(.separator())
menu.addItem(NSMenuItem(title: "Start server", action: nil, keyEquivalent: ""))
menu.addItem(NSMenuItem(title: "Quit", action: nil, keyEquivalent: "q"))
item.menu = menu

// "hold": just show the item and stay up, so another process can photograph it.
if CommandLine.arguments.contains("hold") {
    log("holding: statusItem.isVisible=\(item.isVisible)")
    log("AXIsProcessTrusted=\(AXIsProcessTrusted())")
    DispatchQueue.main.asyncAfter(deadline: .now() + 12) { exit(0) }
    app.run()
    exit(0)
}

Timer.scheduledTimer(withTimeInterval: 1.0, repeats: false) { _ in
    log("bundleIdentifier=\(Bundle.main.bundleIdentifier ?? "nil")")
    log("bundlePath=\(Bundle.main.bundlePath)")
    log("executablePath=\(Bundle.main.executablePath ?? "nil")")
    log("infoDictionaryKeys=\(Bundle.main.infoDictionary?.keys.sorted().joined(separator: ",") ?? "nil")")
    log("isRunning=\(app.isRunning) activationPolicy=\(app.activationPolicy().rawValue)")
    log("statusItem.isVisible=\(item.isVisible) length=\(item.length)")
    log("AXIsProcessTrusted=\(AXIsProcessTrusted())")
    if let b = item.button, let w = b.window {
        log("button.windowNumber=\(w.windowNumber)")
        log("button.screenFrame=\(w.convertToScreen(b.frame))")
    } else {
        log("button.window=nil  <-- no window server presence")
    }

    let screenW = Int(NSScreen.main?.frame.width ?? 1440)
    capture("1-menubar", "0,0,\(screenW),26")

    // A human opens this with the mouse, so the probe does too: a real HID
    // click at the coordinates the button just reported. The menu runs its own
    // tracking run loop, so the screenshot timer is added to .common.
    guard let button = item.button, let win = button.window else { exit(3) }
    let f = win.convertToScreen(button.frame)
    let screenH = NSScreen.main?.frame.height ?? 0
    // AppKit is bottom-left, CGEvent is top-left.
    let click = CGPoint(x: f.midX, y: screenH - f.midY)
    let before = CGEvent(source: nil)?.location ?? .zero
    log("clicking at \(click), cursor was at \(before)")

    let t = Timer(timeInterval: 0.9, repeats: false) { _ in
        capture("2-menu-open", "\(max(0, Int(f.origin.x) - 60)),0,520,300")
        log("menuIsOpen=\(menu.highlightedItem != nil || item.button?.isHighlighted == true)")
        menu.cancelTracking()
        CGWarpMouseCursorPosition(before)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { exit(0) }
    }
    RunLoop.main.add(t, forMode: .common)

    NSApp.activate(ignoringOtherApps: true)
    let src = CGEventSource(stateID: .hidSystemState)
    CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: click, mouseButton: .left)?.post(tap: .cghidEventTap)
    CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: click, mouseButton: .left)?.post(tap: .cghidEventTap)
    CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: click, mouseButton: .left)?.post(tap: .cghidEventTap)
}

// A probe that hangs is a probe that needs killing by hand.
DispatchQueue.main.asyncAfter(deadline: .now() + 15) {
    log("TIMEOUT: still alive after 15 s")
    exit(2)
}

app.run()
