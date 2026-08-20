//! Two rules about doc comments, read off the workspace's own source.
//!
//! Both exist because #169 and #176 gave `Provider::cancel` a real
//! implementation at three adapters and left the pre-change paragraph in place
//! above the new one. Rustdoc concatenates them, so the summary line — the
//! sentence docs.rs shows in search results — said the method always refused
//! while the body released a payer's hold. A shop reading it either leaves a
//! hold outstanding until the provider expires it, or releases one it meant to
//! keep.
//!
//! # What these do not prove
//!
//! Neither says a doc comment is **true**. The first proves only that two
//! blocks were concatenated; a rewrite that replaces the summary with a wrong
//! one passes it. The second proves only that a method claiming to refuse
//! actually refuses — a method claiming to *do* something is not checked, and
//! `.await` is a proxy for "reaches the network" rather than the thing itself.
//!
//! That asymmetry is deliberate: the refusal claim is the one a caller builds
//! a fallback around, and it is the one that was wrong.

#![allow(
    clippy::expect_used,
    reason = "a source tree that cannot be read is a failed test"
)]
//!
//! Both were run against the tree before the change they came from was fixed:
//! the first found three, all real; the second found two of eighteen claims,
//! both real. A rule that had found fifty would have been the wrong rule.

use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under a crate's `src/`, which is where doc comments live.
fn adapter_sources() -> Vec<PathBuf> {
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

/// The text of a `///` line, or `None` for anything else.
fn doc_text(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("///").map(str::trim)
}

/// Two doc blocks concatenated onto one item.
///
/// The shape is a short sentence ending a block — the last line of the old
/// summary — followed straight away by the first line of a new one, with no
/// item between them for rustdoc to split on. Numbered lists open the same way
/// and are not it.
fn seams_in(text: &str) -> Vec<String> {
    let lines: Vec<Option<&str>> = text.lines().map(doc_text).collect();
    let mut fenced = false;
    let mut seams = Vec::new();
    for (index, pair) in lines.windows(2).enumerate() {
        let (Some(first), Some(second)) = (pair[0], pair[1]) else {
            continue;
        };
        if first.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced
            || !first.ends_with('.')
            || first.chars().count() > 60
            || first.starts_with(|c: char| c.is_ascii_digit())
            || !second.starts_with(char::is_uppercase)
        {
            continue;
        }
        seams.push(format!(
            "{}\n    ends a block: {first}\n    opens another: {second}",
            index + 1
        ));
    }
    seams
}

#[test]
fn no_item_carries_two_summaries() {
    let mut seams = Vec::new();
    for file in adapter_sources() {
        let text = fs::read_to_string(&file).expect("a readable source file");
        seams.extend(
            seams_in(&text)
                .into_iter()
                .map(|found| format!("{}:{found}", file.display())),
        );
    }
    assert!(
        seams.is_empty(),
        "{} doc comment(s) carry two summaries, so rustdoc shows the older one:\n{}",
        seams.len(),
        seams.join("\n")
    );
}

/// A method documented as always refusing does not reach the network.
///
/// `.await` stands in for "sends something". A method that only builds an
/// error has nothing to await, so a claim of refusal sitting above a body that
/// awaits is a claim that stopped being true.
fn refusals_that_reach_the_network(text: &str) -> Vec<String> {
    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut broken = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(doc) = doc_text(line) else { continue };
        if !doc.contains("Always refused") && !doc.contains("Always [`ErrorKind::Unsupported`]") {
            continue;
        }
        let mut item = index;
        while item < lines.len() {
            let trimmed = lines[item].trim_start();
            if trimmed.starts_with("///") || trimmed.starts_with("#[") {
                item += 1;
            } else {
                break;
            }
        }
        if item >= lines.len() || !lines[item].contains(" fn ") {
            broken.push(format!(
                "{} — the claim does not sit on a function",
                index + 1
            ));
            continue;
        }
        if body_awaits(&lines, item) {
            broken.push(format!(
                "{} — says it always refuses, and its body awaits something:\n    {doc}",
                index + 1
            ));
        }
    }
    broken
}

#[test]
fn a_method_documented_as_always_refusing_refuses() {
    let mut broken = Vec::new();
    for file in adapter_sources() {
        let text = fs::read_to_string(&file).expect("a readable source file");
        broken.extend(
            refusals_that_reach_the_network(&text)
                .into_iter()
                .map(|found| format!("{}:{found}", file.display())),
        );
    }
    assert!(
        broken.is_empty(),
        "{} refusal claim(s) no longer describe the body below them:\n{}",
        broken.len(),
        broken.join("\n")
    );
}

/// Whether the block opening at or after `start` contains an `.await`.
fn body_awaits(lines: &[String], start: usize) -> bool {
    let mut depth: i32 = 0;
    let mut opened = false;
    for line in &lines[start..] {
        depth += i32::try_from(line.matches('{').count()).expect("a sane line");
        depth -= i32::try_from(line.matches('}').count()).expect("a sane line");
        if line.contains('{') {
            opened = true;
        }
        if line.contains(".await") {
            return true;
        }
        if opened && depth <= 0 {
            break;
        }
    }
    false
}

/// A latch nobody has seen fail is a latch nobody knows works.
///
/// Both fixtures are the shape the two rules were written against: the first
/// is what #176's diff left behind at three adapters, the second is what it
/// left behind at two of them.
#[test]
fn both_detectors_go_red_on_the_shape_they_were_written_for() {
    let two_summaries = r"
    /// Always refused. There is no operation for it.
    /// Voids the hold, through `void_authorization`.
    async fn cancel(&self) -> Result<(), ()> {
        Ok(())
    }
";
    assert_eq!(
        seams_in(two_summaries).len(),
        1,
        "the seam detector did not see two summaries"
    );

    let one_summary = r"
    /// Voids the hold, through `void_authorization`.
    ///
    /// It reads the order first, the way `refund` does.
    async fn cancel(&self) -> Result<(), ()> {
        Ok(())
    }
";
    assert!(
        seams_in(one_summary).is_empty(),
        "the seam detector fired on an ordinary doc comment"
    );

    let claims_and_sends = r"
    /// Always refused. There is no operation for it.
    async fn cancel(&self) -> Result<(), ()> {
        self.send().await
    }
";
    assert_eq!(
        refusals_that_reach_the_network(claims_and_sends).len(),
        1,
        "a refusal claim above a body that sends was not caught"
    );

    let claims_and_refuses = r"
    /// Always refused. There is no operation for it.
    async fn cancel(&self) -> Result<(), ()> {
        Err(())
    }
";
    assert!(
        refusals_that_reach_the_network(claims_and_refuses).is_empty(),
        "a refusal claim above a body that refuses was reported"
    );
}
