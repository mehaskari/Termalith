//! Local side of file transfers: safe destination names for downloads, a
//! registry of files and folders the user dropped onto the window, and the
//! walk of a dropped folder.
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
/// Largest remote file the built-in editor opens.
pub const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

/// Text for the editor: UTF-8 without NUL bytes (a NUL means binary).
/// A UTF-8 byte-order mark is kept as part of the text so saving
/// round-trips it.
pub fn decode_text(bytes: Vec<u8>) -> Result<String, AppError> {
    if bytes.contains(&0) {
        return Err(AppError::invalid("file", "looks like a binary file"));
    }
    String::from_utf8(bytes)
        .map_err(|_| AppError::invalid("file", "is not UTF-8 text; download it to edit locally"))
}

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
    /// Bytes; for a folder, the total of the files that will be uploaded.
    pub size: u64,
    pub folder: bool,
}

/// Most entries (files and folders) a dropped folder may contain.
pub const MAX_FOLDER_ENTRIES: usize = 10_000;
const MAX_FOLDER_LEVELS: usize = 32;

/// A local folder to upload, as paths relative to it. Folders come before
/// their contents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LocalTree {
    pub dirs: Vec<Vec<String>>,
    pub files: Vec<LocalTreeFile>,
    /// Symbolic links, special files and names that are not valid on every
    /// OS; they are not uploaded.
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTreeFile {
    pub rel: Vec<String>,
    pub path: PathBuf,
    pub size: u64,
}

impl LocalTree {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Walks `root` without following symbolic links. Errors when the folder
/// is too large or too deep, or cannot be read.
pub fn walk_local_folder(root: &Path) -> Result<LocalTree, AppError> {
    fn walk(
        dir: &Path,
        rel: &[String],
        tree: &mut LocalTree,
        entries: &mut usize,
    ) -> Result<(), AppError> {
        if rel.len() > MAX_FOLDER_LEVELS {
            return Err(AppError::invalid("folder", "is nested too deeply"));
        }
        let read = std::fs::read_dir(dir)
            .map_err(|e| AppError::invalid("folder", format!("cannot be read: {e}")))?;
        let mut items: Vec<_> = read.filter_map(Result::ok).collect();
        items.sort_by_key(|e| e.file_name());
        for item in items {
            *entries += 1;
            if *entries > MAX_FOLDER_ENTRIES {
                return Err(AppError::invalid(
                    "folder",
                    format!("has more than {MAX_FOLDER_ENTRIES} entries"),
                ));
            }
            let name = item.file_name();
            let Some(name) = name.to_str().filter(|n| safe_file_name(n).is_ok()) else {
                tree.skipped += 1;
                continue;
            };
            let Ok(meta) = std::fs::symlink_metadata(item.path()) else {
                tree.skipped += 1;
                continue;
            };
            let mut child = rel.to_vec();
            child.push(name.to_owned());
            if meta.is_dir() {
                tree.dirs.push(child.clone());
                walk(&item.path(), &child, tree, entries)?;
            } else if meta.is_file() {
                tree.files.push(LocalTreeFile {
                    rel: child,
                    path: item.path(),
                    size: meta.len(),
                });
            } else {
                tree.skipped += 1;
            }
        }
        Ok(())
    }
    let mut tree = LocalTree::default();
    walk(root, &[], &mut tree, &mut 0)?;
    Ok(tree)
}

/// Paths the user dropped onto the window, kept on the backend.
#[derive(Default)]
pub struct DroppedFiles {
    next: AtomicU64,
    files: Mutex<HashMap<u64, PathBuf>>,
}

impl DroppedFiles {
    /// Registers regular files and folders and returns what the UI may
    /// know about them. Anything else, and folders that cannot be walked,
    /// are left out. Walks folders, so call it off the UI thread.
    pub fn register(&self, paths: &[PathBuf]) -> Vec<DroppedFile> {
        paths
            .iter()
            .filter_map(|p| {
                let meta = std::fs::metadata(p).ok()?;
                let name = p.file_name()?.to_string_lossy().into_owned();
                let (size, folder) = if meta.is_file() {
                    (meta.len(), false)
                } else if meta.is_dir() {
                    (walk_local_folder(p).ok()?.total_bytes(), true)
                } else {
                    return None;
                };
                let token = self.next.fetch_add(1, Ordering::Relaxed) + 1;
                self.files
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(token, p.clone());
                Some(DroppedFile {
                    token,
                    name,
                    size,
                    folder,
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

    #[test]
    fn decode_text_accepts_utf8_and_rejects_binary() {
        assert_eq!(decode_text(b"a\r\nb".to_vec()).unwrap(), "a\r\nb");
        assert_eq!(
            decode_text("\u{feff}ü".as_bytes().to_vec()).unwrap(),
            "\u{feff}ü"
        );
        assert!(decode_text(b"a\0b".to_vec()).is_err());
        assert!(decode_text(vec![0xff, 0xfe, 0x41]).is_err());
    }

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
    fn dropped_files_and_folders_are_tokens_used_once() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("up.bin");
        std::fs::write(&f, b"1234").unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("a.txt"), b"abc").unwrap();
        let reg = DroppedFiles::default();
        let got = reg.register(&[f.clone(), sub.clone(), dir.path().join("missing")]);
        assert_eq!(got.len(), 2);
        assert_eq!(
            (got[0].name.as_str(), got[0].size, got[0].folder),
            ("up.bin", 4, false)
        );
        assert_eq!(
            (got[1].name.as_str(), got[1].size, got[1].folder),
            ("sub", 3, true)
        );
        assert_eq!(reg.take(got[1].token).unwrap(), sub);
        assert_eq!(reg.take(got[0].token).unwrap(), f);
        assert!(matches!(reg.take(got[0].token), Err(AppError::NotFound)));
        assert!(matches!(reg.take(999), Err(AppError::NotFound)));
    }

    #[test]
    fn folder_walk_lists_folders_first_and_skips_links() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("b/c")).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        std::fs::write(root.join("a.txt"), b"1").unwrap();
        std::fs::write(root.join("b/c/d.txt"), b"22").unwrap();
        // Names Windows cannot store are skipped (only creatable on Unix).
        #[cfg(unix)]
        {
            std::fs::write(root.join("bad:name"), b"x").unwrap();
            std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
        }
        let tree = walk_local_folder(root).unwrap();
        let v = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(tree.dirs, vec![v(&["b"]), v(&["b", "c"]), v(&["empty"])]);
        let files: Vec<_> = tree.files.iter().map(|f| (f.rel.clone(), f.size)).collect();
        assert_eq!(
            files,
            vec![(v(&["a.txt"]), 1), (v(&["b", "c", "d.txt"]), 2)]
        );
        assert_eq!(tree.total_bytes(), 3);
        assert_eq!(tree.skipped, if cfg!(unix) { 2 } else { 0 });
    }
}
