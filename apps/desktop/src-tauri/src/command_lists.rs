//! Every app command is listed in three places: the handler list in
//! `lib.rs`, the permission list in `build.rs` and the grants in
//! `capabilities/default.json`. A command missing from one of them fails
//! only at run time ("command not allowed"), so this test keeps them equal.

use std::collections::BTreeSet;

const LIB: &str = include_str!("lib.rs");
const BUILD: &str = include_str!("../build.rs");
const CAPABILITIES: &str = include_str!("../capabilities/default.json");

/// The text between `start` and the next `end`.
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text.find(start).expect("start marker") + start.len();
    let len = text[from..].find(end).expect("end marker");
    &text[from..from + len]
}

/// `module::name,` entries of `generate_handler![…]`.
fn handlers() -> BTreeSet<String> {
    between(LIB, "generate_handler![", "]")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.rsplit("::").next().unwrap_or(s).to_owned())
        .collect()
}

/// Quoted names in `AppManifest::new().commands(&[…])`.
fn permissions() -> BTreeSet<String> {
    between(BUILD, ".commands(&[", "]")
        .split(',')
        .map(|s| s.trim().trim_matches('"'))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `"allow-name-with-dashes"` grants, as command names.
fn grants() -> BTreeSet<String> {
    CAPABILITIES
        .split('"')
        .filter_map(|s| s.strip_prefix("allow-"))
        .map(|s| s.replace('-', "_"))
        .collect()
}

fn diff(a: &BTreeSet<String>, b: &BTreeSet<String>) -> Vec<String> {
    a.difference(b).cloned().collect()
}

#[test]
fn command_lists_agree() {
    let (h, p, g) = (handlers(), permissions(), grants());
    assert!(
        h.len() > 50,
        "parsed {} handlers; the parser is off",
        h.len()
    );
    assert!(
        h == p && p == g,
        "command lists differ:\n  in lib.rs but not build.rs: {:?}\n  in build.rs but not lib.rs: {:?}\n  in build.rs but not granted: {:?}\n  granted but not in build.rs: {:?}",
        diff(&h, &p),
        diff(&p, &h),
        diff(&p, &g),
        diff(&g, &p),
    );
}
