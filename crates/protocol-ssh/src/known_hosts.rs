//! Known-hosts storage in OpenSSH `known_hosts` format.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use russh::keys::PublicKey;
use russh::keys::known_hosts::{known_host_keys_path, learn_known_hosts_path};

use crate::SshError;
use crate::host_key::{HostKeyStatus, fingerprint};

pub trait KnownHostsStore: Send + Sync {
    /// All keys recorded for `host:port`, from every source.
    fn keys_for(&self, host: &str, port: u16) -> Result<Vec<PublicKey>, SshError>;
    /// Record a key in the writable store.
    fn add(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), SshError>;

    fn status(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus, SshError> {
        let known = self.keys_for(host, port)?;
        if known.iter().any(|k| k == key) {
            Ok(HostKeyStatus::Trusted)
        } else if known.is_empty() {
            Ok(HostKeyStatus::Unknown)
        } else {
            // Unlike russh's `check_known_hosts`, a recorded key of a
            // different algorithm also counts as "changed": an attacker could
            // otherwise present a new key type and get an "unknown" prompt.
            Ok(HostKeyStatus::Changed {
                known_fingerprints: known.iter().map(fingerprint).collect(),
            })
        }
    }
}

/// App-owned known-hosts file (read/write), optionally consulting extra
/// read-only files such as the user's `~/.ssh/known_hosts`, which is never
/// modified.
#[derive(Debug)]
pub struct FileKnownHosts {
    path: PathBuf,
    read_only: Vec<PathBuf>,
    write_lock: Mutex<()>,
}

impl FileKnownHosts {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            read_only: Vec::new(),
            write_lock: Mutex::new(()),
        }
    }

    pub fn with_read_only(mut self, path: impl Into<PathBuf>) -> Self {
        self.read_only.push(path.into());
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn read(path: &Path, host: &str, port: u16) -> Result<Vec<PublicKey>, SshError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    known_host_keys_path(host, port, path)
        .map(|v| v.into_iter().map(|(_, k)| k).collect())
        .map_err(|e| SshError::KnownHosts(e.to_string()))
}

impl KnownHostsStore for FileKnownHosts {
    fn keys_for(&self, host: &str, port: u16) -> Result<Vec<PublicKey>, SshError> {
        let mut keys = read(&self.path, host, port)?;
        for p in &self.read_only {
            keys.extend(read(p, host, port)?);
        }
        Ok(keys)
    }

    fn add(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), SshError> {
        let _guard = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        learn_known_hosts_path(host, port, key, &self.path)
            .map_err(|e| SshError::KnownHosts(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::PrivateKey;

    fn ed25519() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone()
    }

    fn store() -> (tempfile::TempDir, FileKnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let s = FileKnownHosts::new(dir.path().join("known_hosts"));
        (dir, s)
    }

    #[test]
    fn missing_file_means_unknown() {
        let (_d, s) = store();
        assert_eq!(
            s.status("example.test", 22, &ed25519()).unwrap(),
            HostKeyStatus::Unknown
        );
    }

    #[test]
    fn added_key_is_trusted_and_port_scoped() {
        let (_d, s) = store();
        let k = ed25519();
        s.add("example.test", 2222, &k).unwrap();
        assert_eq!(
            s.status("example.test", 2222, &k).unwrap(),
            HostKeyStatus::Trusted
        );
        assert_eq!(
            s.status("example.test", 22, &k).unwrap(),
            HostKeyStatus::Unknown
        );
    }

    #[test]
    fn different_key_is_changed_even_with_other_algorithm() {
        let (_d, s) = store();
        let recorded = ed25519();
        s.add("example.test", 22, &recorded).unwrap();
        let presented = PrivateKey::random(
            &mut rand::rng(),
            russh::keys::Algorithm::Ecdsa {
                curve: russh::keys::EcdsaCurve::NistP256,
            },
        )
        .unwrap()
        .public_key()
        .clone();
        match s.status("example.test", 22, &presented).unwrap() {
            HostKeyStatus::Changed { known_fingerprints } => {
                assert_eq!(known_fingerprints, vec![fingerprint(&recorded)]);
            }
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn read_only_source_is_consulted_but_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let user_file = dir.path().join("user_known_hosts");
        let k = ed25519();
        learn_known_hosts_path("example.test", 22, &k, &user_file).unwrap();
        let before = std::fs::read(&user_file).unwrap();

        let s = FileKnownHosts::new(dir.path().join("app_known_hosts")).with_read_only(&user_file);
        assert_eq!(
            s.status("example.test", 22, &k).unwrap(),
            HostKeyStatus::Trusted
        );
        s.add("other.test", 22, &ed25519()).unwrap();
        assert_eq!(std::fs::read(&user_file).unwrap(), before);
    }
}
