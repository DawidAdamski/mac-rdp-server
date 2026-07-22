//! Points the linker at the Swift runtime's compatibility shims.
//!
//! `screencapturekit` (and its `apple-cf` / `apple-metal` dependencies) compile
//! a Swift bridge, which auto-links `swiftCompatibility56` and
//! `swiftCompatibilityConcurrency`. Their build scripts emit the search path
//! for a **full Xcode** install:
//!
//! ```text
//! $(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx
//! ```
//!
//! With only the Command Line Tools installed that directory does not exist —
//! the shims live in `$(xcode-select -p)/usr/lib/swift/macosx` instead — and
//! the link fails with `__swift_FORCE_LOAD_$_swiftCompatibility56` undefined.
//!
//! So: ask `xcode-select` where the developer directory is and emit whichever
//! layout is actually present. Works with both Xcode and Command Line Tools,
//! which matters because CI runners have the former and this machine has the
//! latter.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if !cfg!(target_os = "macos") {
        return;
    }

    let Some(developer_dir) = developer_dir() else {
        // Nothing to do; if the Swift bridge is needed the link error will say so.
        println!("cargo:warning=could not determine the Xcode developer directory");
        return;
    };

    let candidates = [
        developer_dir.join("Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"),
        developer_dir.join("usr/lib/swift/macosx"),
    ];

    for path in &candidates {
        if path.is_dir() {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    }

    // Link-time shims are only half of it: the Swift *runtime* (libswift_Concurrency
    // and friends) ships with macOS in /usr/lib/swift, and the produced binary has
    // no rpath pointing there. Without this the build links but every run aborts
    // with `Library not loaded: @rpath/libswift_Concurrency.dylib`.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn developer_dir() -> Option<PathBuf> {
    let output = Command::new("xcode-select").arg("-p").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = PathBuf::from(path.trim());
    path.is_dir().then_some(path)
}
