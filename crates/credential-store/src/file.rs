//! Secrets in a file, for portable mode (ADR-033): the OS credential store
//! belongs to one computer, so a portable copy keeps its secrets next to
//! its data. Only sealed values are written there (the vault refuses plain
//! ones in portable mode), so the file is unreadable without the master
//! password.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use zeroize::Zeroizing;

use crate::{CredentialRef, SecretError, SecretStore};

pub struct FileSecretStore {
    path: PathBuf,
    lock: Mutex<()>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn io(e: impl std::fmt::Display) -> SecretError {
    SecretError::Platform(format!("secrets file: {e}"))
}

impl FileSecretStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, String>, SecretError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_lines(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(io(e)),
        }
    }

    /// Written to a temporary file and renamed, so a crash never leaves a
    /// half-written file.
    fn write(&self, items: &BTreeMap<String, String>) -> Result<(), SecretError> {
        let text: String = items.iter().map(|(k, v)| format!("{k} {v}\n")).collect();
        let tmp = self.path.with_extension("tmp");
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut f = options.open(&tmp).map_err(io)?;
            std::io::Write::write_all(&mut f, text.as_bytes()).map_err(io)?;
            f.sync_all().map_err(io)?;
        }
        std::fs::rename(&tmp, &self.path).map_err(io)
    }
}

/// One `reference hex-value` pair per line; unreadable lines are skipped.
fn serde_lines(text: &str) -> Result<BTreeMap<String, String>, SecretError> {
    Ok(text
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(' ')?;
            CredentialRef::parse(k).ok()?;
            unhex(v)?;
            Some((k.to_owned(), v.to_owned()))
        })
        .collect())
}

impl SecretStore for FileSecretStore {
    fn put(&self, r: &CredentialRef, secret: &[u8]) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut items = self.read()?;
        items.insert(r.as_str().to_owned(), hex(secret));
        self.write(&items)
    }

    fn get(&self, r: &CredentialRef) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self
            .read()?
            .get(r.as_str())
            .and_then(|v| unhex(v))
            .map(Zeroizing::new))
    }

    fn delete(&self, r: &CredentialRef) -> Result<(), SecretError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut items = self.read()?;
        if items.remove(r.as_str()).is_some() {
            self.write(&items)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_survives_a_new_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets");
        let (a, b) = (
            CredentialRef::generate().unwrap(),
            CredentialRef::generate().unwrap(),
        );
        let store = FileSecretStore::new(path.clone());
        assert_eq!(store.get(&a).unwrap(), None);
        store.put(&a, &[0, 1, 255]).unwrap();
        store.put(&b, b"two").unwrap();
        store.delete(&b).unwrap();
        store.delete(&b).unwrap();

        let again = FileSecretStore::new(path.clone());
        assert_eq!(again.get(&a).unwrap().unwrap().as_slice(), &[0, 1, 255]);
        assert_eq!(again.get(&b).unwrap(), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn garbage_lines_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets");
        let r = CredentialRef::generate().unwrap();
        std::fs::write(
            &path,
            format!("junk\n{} zz\n{} 6869\n", r.as_str(), r.as_str()),
        )
        .unwrap();
        let store = FileSecretStore::new(path);
        assert_eq!(store.get(&r).unwrap().unwrap().as_slice(), b"hi");
    }
}
