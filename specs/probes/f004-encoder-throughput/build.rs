//! The library links a Swift bridge, and link arguments from a dependency's
//! build script do not reach the binary that uses it — so this probe needs the
//! same rpath the main crate's `build.rs` adds, or every run aborts with
//! `Library not loaded: @rpath/libswift_Concurrency.dylib`.
fn main() {
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
