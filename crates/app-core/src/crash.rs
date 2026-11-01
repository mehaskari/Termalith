//! Crash reports: written when the backend panics or the UI hits an error,
//! shown on the next start, never sent anywhere automatically.
//!
//! A report holds the app version, OS, the error message and location and,
//! for the backend, a backtrace. Code never puts secrets into panic
//! messages; the UI still asks the user to review a report before sharing.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Reports kept; older ones are deleted.
pub const MAX_REPORTS: usize = 20;
/// Longest report body kept (a backtrace can be long).
const MAX_REPORT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CrashKind {
    /// The Rust backend panicked.
    Backend,
    /// The user interface hit an unhandled error.
    Ui,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    /// File name, used to dismiss it.
    pub id: String,
    pub kind: CrashKind,
    /// Seconds since the Unix epoch.
    pub time: u64,
    /// First line of the message.
    pub summary: String,
    pub text: String,
}

pub struct CrashReports {
    dir: PathBuf,
    version: String,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn kind_tag(kind: CrashKind) -> &'static str {
    match kind {
        CrashKind::Backend => "backend",
        CrashKind::Ui => "ui",
    }
}

impl CrashReports {
    pub fn new(dir: PathBuf, version: &str) -> Self {
        Self {
            dir,
            version: version.to_owned(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes a report and prunes old ones. Errors are returned, but callers
    /// in a panic hook ignore them (nothing else can be done there).
    pub fn write(&self, kind: CrashKind, message: &str, details: &str) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let now = unix_now();
        let mut text = format!(
            "Termalith {} crash report ({})\nOS: {} {}\nTime: {now}\n\n{}\n",
            self.version,
            kind_tag(kind),
            std::env::consts::OS,
            std::env::consts::ARCH,
            message.trim(),
        );
        if !details.trim().is_empty() {
            text.push('\n');
            text.push_str(details.trim());
            text.push('\n');
        }
        if text.len() > MAX_REPORT_BYTES {
            let mut cut = MAX_REPORT_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n[truncated]\n");
        }
        let mut n = 0;
        let path = loop {
            let p = self
                .dir
                .join(format!("crash-{now}-{n}-{}.txt", kind_tag(kind)));
            if !p.exists() {
                break p;
            }
            n += 1;
        };
        std::fs::write(&path, text)?;
        self.prune();
        Ok(path)
    }

    fn files(&self) -> Vec<(u64, u32, PathBuf)> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out: Vec<(u64, u32, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let rest = name.strip_prefix("crash-")?.strip_suffix(".txt")?;
                let mut parts = rest.splitn(3, '-');
                let time = parts.next()?.parse().ok()?;
                let seq = parts.next()?.parse().ok()?;
                Some((time, seq, e.path()))
            })
            .collect();
        out.sort_by_key(|f| std::cmp::Reverse((f.0, f.1)));
        out
    }

    fn prune(&self) {
        for (_, _, p) in self.files().into_iter().skip(MAX_REPORTS) {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Reports not yet dismissed, newest first.
    pub fn pending(&self) -> Vec<CrashReport> {
        self.files()
            .into_iter()
            .filter_map(|(time, _, path)| {
                let text = std::fs::read_to_string(&path).ok()?;
                let id = path.file_name()?.to_string_lossy().into_owned();
                let kind = if id.ends_with("-ui.txt") {
                    CrashKind::Ui
                } else {
                    CrashKind::Backend
                };
                let summary = text
                    .lines()
                    .skip(4)
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect();
                Some(CrashReport {
                    id,
                    kind,
                    time,
                    summary,
                    text,
                })
            })
            .collect()
    }

    /// Deletes reports by id (file names from [`CrashReports::pending`]);
    /// anything that is not a report file name is ignored.
    pub fn dismiss(&self, ids: &[String]) {
        for id in ids {
            let valid = id.starts_with("crash-")
                && id.ends_with(".txt")
                && !id.contains(['/', '\\'])
                && !id.contains("..");
            if valid {
                let _ = std::fs::remove_file(self.dir.join(id));
            }
        }
    }
}

/// Formats a backend panic.
pub fn panic_report(info: &std::panic::PanicHookInfo<'_>) -> (String, String) {
    let payload = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-text panic payload)".into());
    let location = info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_default();
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    (
        format!("panic in thread '{thread}' at {location}: {payload}"),
        format!("Backtrace:\n{}", std::backtrace::Backtrace::force_capture()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_lists_prunes_and_dismisses() {
        let dir = tempfile::tempdir().unwrap();
        let r = CrashReports::new(dir.path().join("crashes"), "0.1.0");
        assert!(r.pending().is_empty());
        r.write(
            CrashKind::Backend,
            "panic at src/x.rs:1: boom",
            "Backtrace:\n...",
        )
        .unwrap();
        r.write(CrashKind::Ui, "TypeError: x is undefined", "at App")
            .unwrap();
        let p = r.pending();
        assert_eq!(p.len(), 2);
        assert!(
            p.iter()
                .any(|c| c.kind == CrashKind::Ui && c.summary == "TypeError: x is undefined")
        );
        let b = p.iter().find(|c| c.kind == CrashKind::Backend).unwrap();
        assert!(
            b.text
                .starts_with("Termalith 0.1.0 crash report (backend)\nOS: ")
        );
        assert!(b.text.contains("Backtrace:"));

        for i in 0..MAX_REPORTS + 5 {
            r.write(CrashKind::Ui, &format!("e{i}"), "").unwrap();
        }
        assert_eq!(r.pending().len(), MAX_REPORTS);

        let ids: Vec<String> = r.pending().into_iter().map(|c| c.id).collect();
        r.dismiss(&["../../etc/passwd".into(), "notes.txt".into()]);
        assert_eq!(
            r.pending().len(),
            MAX_REPORTS,
            "only report files are deleted"
        );
        r.dismiss(&ids);
        assert!(r.pending().is_empty());
    }

    #[test]
    fn long_reports_are_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let r = CrashReports::new(dir.path().into(), "0.1.0");
        r.write(CrashKind::Backend, "x", &"ü".repeat(100_000))
            .unwrap();
        let t = &r.pending()[0].text;
        assert!(t.len() <= MAX_REPORT_BYTES + 20);
        assert!(t.ends_with("[truncated]\n"));
    }

    #[test]
    fn formats_a_panic() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(None));
        let s = seen.clone();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *s.lock().unwrap() = Some(panic_report(info));
        }));
        let _ = std::thread::Builder::new()
            .name("worker".into())
            .spawn(|| panic!("boom {}", 42))
            .unwrap()
            .join();
        std::panic::set_hook(prev);
        let (msg, details) = seen.lock().unwrap().take().unwrap();
        assert!(msg.starts_with("panic in thread 'worker' at "), "{msg}");
        assert!(msg.ends_with(": boom 42"), "{msg}");
        assert!(details.starts_with("Backtrace:"));
    }
}
