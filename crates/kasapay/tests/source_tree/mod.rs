//! Reading the workspace's own source, for the checks that need to.
//!
//! Shared rather than copied: a rule with two copies is one file away from
//! having none, which is what #170 was about. Each test binary compiles its
//! own copy of this module and uses part of it, so `dead_code` is expected.

#![allow(dead_code, reason = "each test binary uses the part of this it needs")]
#![allow(
    clippy::expect_used,
    reason = "a source tree that cannot be read is a failed test"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under a crate's `src/`, sorted.
///
/// `src/` only: what lives under `tests/` and `examples/` is not the library,
/// and a fixture that deliberately shows a broken shape must not be read as
/// the workspace doing it.
pub(crate) fn files() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ is the parent of this crate");
    let mut found = Vec::new();
    for entry in fs::read_dir(crates).expect("crates/ is readable") {
        let src = entry.expect("a readable entry").path().join("src");
        if src.is_dir() {
            collect(&src, &mut found);
        }
    }
    assert!(
        found.len() > 50,
        "only {} source files found; the walk is looking in the wrong place",
        found.len()
    );
    found.sort();
    found
}

fn collect(dir: &Path, into: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a readable entry").path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

/// Where each `impl <trait> for …` in the workspace is written.
///
/// Names the site rather than the type: `Client` and `Webhooks` each name two
/// different things in two different crates, so a type name alone does not
/// identify an implementation.
pub(crate) fn implementations_of(name: &str) -> Vec<String> {
    let opening = format!("impl {name} for ");
    let mut found = Vec::new();
    for file in files() {
        let text = fs::read_to_string(&file).expect("a readable source file");
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with(&opening) {
                found.push(format!("{}:{}  {}", file.display(), index + 1, line.trim()));
            }
        }
    }
    found
}
