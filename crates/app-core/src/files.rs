//! Local side of file transfers: safe destination names for downloads and
//! a registry of files the user dropped onto the window.
//!
//! The UI never names a local path. Downloads go to a directory chosen by
//! the backend; uploads refer to dropped files by opaque token.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::AppError;

/// Validates a file name received from a server before using it locally.
/// Rejects anything that could escape the destination directory or confuse
/// the OS (separators, `..`, control characters, Windows reserved names).
pub fn safe_file_name(name: &str) -> Result<&str, AppError> {
    let bad = |why: &str| Err(AppError::invalid("file name", why.to_owned()));
    if name.is_empty() || name == "." || name == ".." {
        return bad("empty or a relative path component");
    }
    if name.len() > 255 {
        return bad("too long");
    }
    if name.contains(['/', '\\']) {
        return bad("contains a path separator");
    }
    if name.chars().any(|c| c.is_control()) {
        return bad("contains control characters");
    }
    // Windows rules are enforced everywhere so a session behaves the same on
    // every OS.
    if name.contains([':', '*', '?', '"', '<', '>', '|']) {
        return bad("contains characters not allowed in file names");
    }
    if name.ends_with(['.', ' ']) {
        return bad("ends with a dot or space");
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return bad("is a reserved device name");
    }
    Ok(name)
}

/// `dir/name`, or `dir/name (1).ext`, `(2)`, … if taken.
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFile {
    pub token: u64,
    pub name: String,
    pub size: u64,
}

/// Paths the user dropped onto the window, kept on the backend.
#[derive(Default)]
pub struct DroppedFiles {
    next: AtomicU64,
    files: Mutex<HashMap<u64, PathBuf>>,
}

impl DroppedFiles {
    /// Registers regular files (directories are skipped) and returns what
    /// the UI may know about them.
    pub fn register(&self, paths: &[PathBuf]) -> Vec<DroppedFile> {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        paths
            .iter()
            .filter_map(|p| {
                let meta = std::fs::metadata(p).ok()?;
                if !meta.is_file() {
                    return None;
                }
                let token = self.next.fetch_add(1, Ordering::Relaxed) + 1;
                files.insert(token, p.clone());
                Some(DroppedFile {
                    token,
                    name: p.file_name()?.to_string_lossy().into_owned(),
                    size: meta.len(),
                })
            })
            .collect()
    }

    /// Consumes a token. Each dropped file can be uploaded once.
    pub fn take(&self, token: u64) -> Result<PathBuf, AppError> {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&token)
            .ok_or(AppError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for n in [
            "report.pdf",
            "archive.tar.gz",
            ".bashrc",
            "naïve file 名前.txt",
            "a",
        ] {
            assert_eq!(safe_file_name(n).unwrap(), n);
        }
    }

    #[test]
    fn rejects_traversal_and_special_names() {
        for n in [
            "",
            ".",
            "..",
            "../etc/passwd",
            "a/b",
            "a\\b",
            "x\u{0}y",
            "line\nbreak",
            "C:evil",
            "what?",
            "trailing.",
            "trailing ",
            "CON",
            "con.txt",
            "COM1",
            "lpt9.log",
        ] {
            assert!(safe_file_name(n).is_err(), "{n:?} should be rejected");
        }
        assert!(safe_file_name(&"x".repeat(256)).is_err());
        assert!(safe_file_name("COMPUTE.txt").is_ok());
    }

    #[test]
    fn unique_destination_appends_counter_before_extension() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(unique_destination(d, "a.txt"), d.join("a.txt"));
        std::fs::write(d.join("a.txt"), b"").unwrap();
        assert_eq!(unique_destination(d, "a.txt"), d.join("a (1).txt"));
        std::fs::write(d.join("a (1).txt"), b"").unwrap();
        assert_eq!(unique_destination(d, "a.txt"), d.join("a (2).txt"));
        std::fs::write(d.join("noext"), b"").unwrap();
        assert_eq!(unique_destination(d, "noext"), d.join("noext (1)"));
        std::fs::write(d.join(".hidden"), b"").unwrap();
        assert_eq!(unique_destination(d, ".hidden"), d.join(".hidden (1)"));
    }

    #[test]
    fn dropped_files_are_tokens_used_once_and_skip_directories() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("up.bin");
        std::fs::write(&f, b"1234").unwrap();
        let reg = DroppedFiles::default();
        let got = reg.register(&[f.clone(), dir.path().to_path_buf()]);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].name.as_str(), got[0].size), ("up.bin", 4));
        assert_eq!(reg.take(got[0].token).unwrap(), f);
        assert!(matches!(reg.take(got[0].token), Err(AppError::NotFound)));
        assert!(matches!(reg.take(999), Err(AppError::NotFound)));
    }
}
