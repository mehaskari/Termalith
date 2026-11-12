//! Command-line switches: open a saved session, a folder's sessions, or a
//! connection (`ssh://…`, `telnet host`) when Termalith starts, e.g. from
//! a desktop shortcut or a script. The UI receives names and text only and
//! resolves them itself; nothing here runs a program.

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;

/// What the command line asked to open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchRequest {
    /// `--session NAME`: a saved session, by name.
    Session { name: String },
    /// `--folder PATH`: every session in a folder.
    Folder { path: String },
    /// `ssh://user@host`, `telnet://host`… or `--connect "ssh -p 2222 me@h"`.
    Connect { text: String },
}

/// Longest argument kept (names and URLs are short).
const MAX_ARG: usize = 512;

/// Reads the switches; unknown ones (macOS adds `-psn_…`) are ignored.
pub fn parse(args: impl IntoIterator<Item = String>) -> Vec<LaunchRequest> {
    let mut out = Vec::new();
    let mut it = args.into_iter().skip(1);
    while let Some(arg) = it.next() {
        let value = |it: &mut dyn Iterator<Item = String>| it.next().filter(|v| v.len() <= MAX_ARG);
        match arg.as_str() {
            "--session" | "-s" => {
                if let Some(name) = value(&mut it) {
                    out.push(LaunchRequest::Session { name });
                }
            }
            "--folder" => {
                if let Some(path) = value(&mut it) {
                    out.push(LaunchRequest::Folder { path });
                }
            }
            "--connect" => {
                if let Some(text) = value(&mut it) {
                    out.push(LaunchRequest::Connect { text });
                }
            }
            a if a.len() <= MAX_ARG && a.contains("://") && !a.starts_with('-') => {
                out.push(LaunchRequest::Connect { text: a.to_owned() });
            }
            _ => {}
        }
    }
    out
}

/// The startup requests, handed out once.
#[derive(Default)]
pub struct LaunchRequests(pub Mutex<Option<Vec<LaunchRequest>>>);

#[tauri::command]
pub fn launch_requests(state: State<'_, AppState>) -> Vec<LaunchRequest> {
    state
        .launch
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("termalith")
            .chain(list.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn reads_switches_and_urls() {
        assert_eq!(
            parse(args(&[
                "--session",
                "web-01",
                "--folder",
                "Production/DB",
                "ssh://me@10.0.0.5:2222",
                "--connect",
                "telnet 10.0.0.254 23",
            ])),
            vec![
                LaunchRequest::Session {
                    name: "web-01".into()
                },
                LaunchRequest::Folder {
                    path: "Production/DB".into()
                },
                LaunchRequest::Connect {
                    text: "ssh://me@10.0.0.5:2222".into()
                },
                LaunchRequest::Connect {
                    text: "telnet 10.0.0.254 23".into()
                },
            ]
        );
    }

    #[test]
    fn ignores_unknown_and_incomplete_switches() {
        assert!(parse(args(&["-psn_0_12345", "--unknown", "x", "--session"])).is_empty());
        assert!(parse(args(&["plain-word"])).is_empty());
        let long = format!("ssh://{}", "a".repeat(600));
        assert!(parse(args(&[long.as_str()])).is_empty());
    }
}
