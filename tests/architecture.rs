//! Enforces the architectural boundary from F-001.12 / AC-11.
//!
//! Only `src/session/` may reference `ironrdp-server`. This is not style
//! policing: replacing IronRDP with our own MS-RDPBCGR implementation
//! (ADR-0001) is only a module swap for as long as this holds.

use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

#[test]
fn only_the_session_module_references_ironrdp() {
    let src = src_dir();
    let session = src.join("session");

    let mut files = Vec::new();
    rust_sources(&src, &mut files);
    assert!(!files.is_empty(), "found no sources to check");

    let mut offenders = Vec::new();
    for file in &files {
        if file.starts_with(&session) {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("reading {}: {e}", file.display()));
        for (number, line) in text.lines().enumerate() {
            // Doc comments and ordinary comments may name IronRDP; code may not.
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if code.contains("ironrdp") {
                offenders.push(format!("{}:{}: {}", file.display(), number + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "ironrdp must only be referenced from src/session/ (see F-001.12 and ADR-0001):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_session_module_actually_exists_and_uses_ironrdp() {
    // Guards against the test above passing vacuously if session/ is renamed
    // or the dependency is dropped.
    let mut files = Vec::new();
    rust_sources(&src_dir().join("session"), &mut files);
    assert!(!files.is_empty(), "src/session/ has no sources");

    let uses_ironrdp = files.iter().any(|file| {
        std::fs::read_to_string(file)
            .expect("reading session source")
            .contains("ironrdp_server::")
    });
    assert!(uses_ironrdp, "src/session/ should be the module that speaks IronRDP");
}
