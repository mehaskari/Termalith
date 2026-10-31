//! Secrets (passwords, passphrases) live only in the OS credential store:
//! macOS Keychain, Windows Credential Manager, Linux Secret Service.
//! Application data stores an opaque [`CredentialRef`], never the secret.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use zeroize::Zeroizing;

const REF_PREFIX: &str = "cred-";

/// Opaque handle to a stored secret. Safe to persist and log.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CredentialRef(String);

impl CredentialRef {
    pub fn generate() -> Result<Self, SecretError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| SecretError::Platform(e.to_string()))?;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Ok(Self(format!("{REF_PREFIX}{hex}")))
    }

    /// Accepts only references produced by [`generate`](Self::generate).
    pub fn parse(s: &str) -> Result<Self, SecretError> {
        let hex = s.strip_prefix(REF_PREFIX).ok_or(SecretError::InvalidRef)?;
        if hex.len() == 32
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            Ok(Self(s.to_owned()))
        } else {
            Err(SecretError::InvalidRef)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CredentialRef({})", self.0)
    }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Errors never include secret values.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("the OS credential store is not available: {0}")]
    Unavailable(String),
    #[error("credential store error: {0}")]
    Platform(String),
    #[error("invalid credential reference")]
    InvalidRef,
}

pub trait SecretStore: Send + Sync {
    fn put(&self, r: &CredentialRef, secret: &[u8]) -> Result<(), SecretError>;
    fn get(&self, r: &CredentialRef) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError>;
    /// Idempotent: deleting a missing secret succeeds.
    fn delete(&self, r: &CredentialRef) -> Result<(), SecretError>;
}

/// In-memory store for tests and for running without an OS store.
#[derive(Default)]
pub struct MemorySecretStore {
    items: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
}

impl SecretStore for MemorySecretStore {
    fn put(&self, r: &CredentialRef, secret: &[u8]) -> Result<(), SecretError> {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        items.insert(r.0.clone(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn get(&self, r: &CredentialRef) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        Ok(items.get(&r.0).cloned())
    }

    fn delete(&self, r: &CredentialRef) -> Result<(), SecretError> {
        self.items
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&r.0);
        Ok(())
    }
}

/// The platform credential store, via keyring-core.
pub struct NativeSecretStore {
    service: String,
}

static DEFAULT_STORE: OnceLock<Result<(), String>> = OnceLock::new();

fn init_platform_store() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new();
    #[cfg(target_os = "windows")]
    let store = windows_native_keyring_store::Store::new();
    #[cfg(all(unix, not(target_os = "macos")))]
    let store = zbus_secret_service_keyring_store::Store::new();
    let store = store.map_err(describe)?;
    keyring_core::set_default_store(store);
    Ok(())
}

impl NativeSecretStore {
    /// `service` namespaces entries, e.g. `"dev.termalith.credentials"`.
    pub fn new(service: impl Into<String>) -> Result<Self, SecretError> {
        DEFAULT_STORE
            .get_or_init(init_platform_store)
            .clone()
            .map_err(SecretError::Unavailable)?;
        Ok(Self {
            service: service.into(),
        })
    }

    fn entry(&self, r: &CredentialRef) -> Result<keyring_core::Entry, SecretError> {
        keyring_core::Entry::new(&self.service, r.as_str()).map_err(map_err)
    }
}

impl SecretStore for NativeSecretStore {
    fn put(&self, r: &CredentialRef, secret: &[u8]) -> Result<(), SecretError> {
        self.entry(r)?.set_secret(secret).map_err(map_err)
    }

    fn get(&self, r: &CredentialRef) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        match self.entry(r)?.get_secret() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_err(e)),
        }
    }

    fn delete(&self, r: &CredentialRef) -> Result<(), SecretError> {
        match self.entry(r)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
}

/// Describes a keyring error without its payload: some variants carry the
/// stored bytes, which must never reach logs or the UI.
fn describe(e: keyring_core::Error) -> String {
    use keyring_core::Error as E;
    match e {
        E::PlatformFailure(p) => format!("platform failure: {p}"),
        E::NoStorageAccess(p) => format!("no storage access: {p}"),
        E::NoEntry => "no entry".into(),
        E::BadEncoding(_) => "stored value has an unexpected encoding".into(),
        E::BadDataFormat(_, _) => "stored value has an unexpected format".into(),
        E::BadStoreFormat(s) => format!("bad store format: {s}"),
        E::TooLong(name, max) => format!("{name} exceeds {max}"),
        E::Invalid(name, why) => format!("invalid {name}: {why}"),
        E::Ambiguous(_) => "ambiguous credential".into(),
        E::NoDefaultStore => "no default store".into(),
        E::NotSupportedByStore(s) => format!("not supported by store: {s}"),
        _ => "unknown credential store error".into(),
    }
}

fn map_err(e: keyring_core::Error) -> SecretError {
    match e {
        keyring_core::Error::NoStorageAccess(_) | keyring_core::Error::NoDefaultStore => {
            SecretError::Unavailable(describe(e))
        }
        other => SecretError::Platform(describe(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_unique_and_round_trip() {
        let a = CredentialRef::generate().unwrap();
        let b = CredentialRef::generate().unwrap();
        assert_ne!(a, b);
        assert_eq!(CredentialRef::parse(a.as_str()).unwrap(), a);
    }

    #[test]
    fn parse_rejects_foreign_strings() {
        for s in [
            "",
            "cred-",
            "cred-xyz",
            "cred-00112233445566778899AABBCCDDEEFF",
            "other-00112233445566778899aabbccddeeff",
            "cred-00112233445566778899aabbccddeeff0",
        ] {
            assert!(CredentialRef::parse(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn memory_store_round_trip_and_idempotent_delete() {
        let s = MemorySecretStore::default();
        let r = CredentialRef::generate().unwrap();
        assert!(s.get(&r).unwrap().is_none());
        s.put(&r, b"hunter2").unwrap();
        assert_eq!(s.get(&r).unwrap().unwrap().as_slice(), b"hunter2");
        s.delete(&r).unwrap();
        s.delete(&r).unwrap();
        assert!(s.get(&r).unwrap().is_none());
    }

    #[test]
    fn errors_never_contain_secret_bytes() {
        let msg = describe(keyring_core::Error::BadEncoding(b"hunter2".to_vec()));
        assert!(!msg.contains("hunter2"));
        let dbg = format!(
            "{:?}",
            map_err(keyring_core::Error::BadEncoding(b"hunter2".to_vec()))
        );
        assert!(!dbg.contains("hunter2") && !dbg.contains("104, 117"));
    }
}
