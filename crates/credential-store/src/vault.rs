//! Master password for saved secrets (ADR-032). Secrets stay in the OS
//! credential store, but are sealed first with a random data key; that key
//! is stored wrapped with a key derived from the master password
//! (Argon2id), so changing the password rewraps one key. The data key lives
//! only in memory while unlocked. A forgotten master password cannot be
//! recovered: the sealed secrets are lost.

use std::sync::{Arc, RwLock};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

use crate::{CredentialRef, SecretError, SecretStore};

/// Marks sealed values; anything else in the store is a plain legacy value.
const SEALED: &[u8; 4] = b"TMV1";
const NONCE: usize = 24;
const KEY: usize = 32;

/// Argon2id cost, kept with the wrapped key so it can be raised later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfCost {
    /// Memory in KiB.
    pub memory: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl KdfCost {
    /// OWASP's first Argon2id recommendation (19 MiB, 2 passes): well under
    /// a second on current hardware.
    pub const DEFAULT: Self = Self {
        memory: 19 * 1024,
        iterations: 2,
        lanes: 1,
    };
}

/// The data key wrapped with the master password. Stored by the app; holds
/// no secret on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedKey {
    pub cost: KdfCost,
    pub salt: [u8; 16],
    pub sealed: Vec<u8>,
}

/// The random key secrets are sealed with. Wiped when dropped.
pub struct DataKey(Zeroizing<[u8; KEY]>);

fn random<const N: usize>() -> Result<[u8; N], SecretError> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| SecretError::Platform(e.to_string()))?;
    Ok(b)
}

fn derive(
    password: &str,
    salt: &[u8; 16],
    cost: KdfCost,
) -> Result<Zeroizing<[u8; KEY]>, SecretError> {
    let params = Params::new(cost.memory, cost.iterations, cost.lanes, Some(KEY))
        .map_err(|e| SecretError::Platform(e.to_string()))?;
    let mut out = Zeroizing::new([0u8; KEY]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, out.as_mut())
        .map_err(|e| SecretError::Platform(e.to_string()))?;
    Ok(out)
}

/// nonce || ciphertext, authenticated together with `aad`.
fn seal(key: &[u8; KEY], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>, SecretError> {
    let nonce = random::<NONCE>()?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad })
            .map_err(|_| SecretError::Platform("encryption failed".into()))?,
    );
    Ok(out)
}

fn open(key: &[u8; KEY], sealed: &[u8], aad: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    if sealed.len() < NONCE {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE);
    let nonce = XNonce::try_from(nonce).ok()?;
    XChaCha20Poly1305::new(key.into())
        .decrypt(&nonce, Payload { msg: body, aad })
        .ok()
        .map(Zeroizing::new)
}

const WRAP_AAD: &[u8] = b"termalith data key";

impl DataKey {
    /// A new data key, wrapped with `password`.
    pub fn create(password: &str, cost: KdfCost) -> Result<(Self, WrappedKey), SecretError> {
        let key = Self(Zeroizing::new(random::<KEY>()?));
        let wrapped = key.wrap(password, cost)?;
        Ok((key, wrapped))
    }

    /// The data key, or [`SecretError::WrongPassword`].
    pub fn unlock(password: &str, wrapped: &WrappedKey) -> Result<Self, SecretError> {
        let kek = derive(password, &wrapped.salt, wrapped.cost)?;
        let plain = open(&kek, &wrapped.sealed, WRAP_AAD).ok_or(SecretError::WrongPassword)?;
        let key: [u8; KEY] = plain
            .as_slice()
            .try_into()
            .map_err(|_| SecretError::WrongPassword)?;
        Ok(Self(Zeroizing::new(key)))
    }

    /// The same key under a new password (and fresh salt).
    pub fn wrap(&self, password: &str, cost: KdfCost) -> Result<WrappedKey, SecretError> {
        let salt = random::<16>()?;
        let kek = derive(password, &salt, cost)?;
        Ok(WrappedKey {
            cost,
            salt,
            sealed: seal(&kek, self.0.as_ref(), WRAP_AAD)?,
        })
    }

    /// Bound to the entry's reference, so sealed values cannot be swapped.
    fn seal_secret(&self, r: &CredentialRef, secret: &[u8]) -> Result<Vec<u8>, SecretError> {
        let mut out = SEALED.to_vec();
        out.extend(seal(&self.0, secret, r.as_str().as_bytes())?);
        Ok(out)
    }

    fn open_secret(
        &self,
        r: &CredentialRef,
        stored: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, SecretError> {
        open(&self.0, &stored[SEALED.len()..], r.as_str().as_bytes())
            .ok_or_else(|| SecretError::Platform("a saved secret could not be opened".into()))
    }
}

pub fn is_sealed(stored: &[u8]) -> bool {
    stored.starts_with(SEALED)
}

/// Whether secrets are sealed, and the key while unlocked.
enum State {
    /// No master password: values pass through unchanged.
    Off,
    Locked,
    Unlocked(DataKey),
}

/// A [`SecretStore`] that seals values when a master password is set.
/// Reading a sealed value while locked fails with [`SecretError::Locked`];
/// plain values (saved before the master password) are still readable.
pub struct VaultStore {
    inner: Arc<dyn SecretStore>,
    state: RwLock<State>,
    /// Portable mode: plain values are never written (ADR-033).
    sealed_only: bool,
}

impl VaultStore {
    /// `protected`: a master password is set (the app keeps the wrapped key).
    pub fn new(inner: Arc<dyn SecretStore>, protected: bool) -> Self {
        Self {
            inner,
            state: RwLock::new(if protected { State::Locked } else { State::Off }),
            sealed_only: false,
        }
    }

    /// Writes only sealed values: without a master password nothing is saved.
    #[must_use]
    pub fn sealed_only(mut self) -> Self {
        self.sealed_only = true;
        self
    }

    fn state(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.state.read().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, s: State) {
        *self.state.write().unwrap_or_else(|e| e.into_inner()) = s;
    }

    pub fn is_locked(&self) -> bool {
        matches!(*self.state(), State::Locked)
    }

    pub fn is_protected(&self) -> bool {
        !matches!(*self.state(), State::Off)
    }

    pub fn unlock(&self, key: DataKey) {
        self.set(State::Unlocked(key));
    }

    /// Forgets the data key; protected secrets need the master password again.
    pub fn lock(&self) {
        if self.is_protected() {
            self.set(State::Locked);
        }
    }

    /// After the sealed secrets were deleted (a forgotten master password):
    /// no protection any more.
    pub fn reset(&self) {
        self.set(State::Off);
    }

    /// Turns protection on: every listed secret is read and sealed.
    pub fn protect(&self, key: DataKey, refs: &[CredentialRef]) -> Result<(), SecretError> {
        for r in refs {
            if let Some(v) = self.inner.get(r)?
                && !is_sealed(&v)
            {
                self.inner.put(r, &key.seal_secret(r, &v)?)?;
            }
        }
        self.unlock(key);
        Ok(())
    }

    /// Turns protection off: every listed secret is opened and stored plain.
    /// Needs the store unlocked.
    pub fn unprotect(&self, refs: &[CredentialRef]) -> Result<(), SecretError> {
        if self.sealed_only {
            return Err(SecretError::Unavailable(
                "portable mode keeps saved passwords sealed".into(),
            ));
        }
        {
            let state = self.state();
            let State::Unlocked(key) = &*state else {
                return Err(SecretError::Locked);
            };
            for r in refs {
                if let Some(v) = self.inner.get(r)?
                    && is_sealed(&v)
                {
                    self.inner.put(r, &key.open_secret(r, &v)?)?;
                }
            }
        }
        self.set(State::Off);
        Ok(())
    }
}

impl SecretStore for VaultStore {
    fn put(&self, r: &CredentialRef, secret: &[u8]) -> Result<(), SecretError> {
        match &*self.state() {
            State::Off if self.sealed_only => Err(SecretError::Unavailable(
                "set a master password to save passwords in portable mode".into(),
            )),
            State::Off => self.inner.put(r, secret),
            State::Locked => Err(SecretError::Locked),
            State::Unlocked(key) => self.inner.put(r, &key.seal_secret(r, secret)?),
        }
    }

    fn get(&self, r: &CredentialRef) -> Result<Option<Zeroizing<Vec<u8>>>, SecretError> {
        let Some(stored) = self.inner.get(r)? else {
            return Ok(None);
        };
        if !is_sealed(&stored) {
            return Ok(Some(stored));
        }
        match &*self.state() {
            State::Unlocked(key) => key.open_secret(r, &stored).map(Some),
            State::Off | State::Locked => Err(SecretError::Locked),
        }
    }

    fn delete(&self, r: &CredentialRef) -> Result<(), SecretError> {
        self.inner.delete(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemorySecretStore;

    /// A random password per run, so no secret is written in the source.
    fn password() -> String {
        random::<12>()
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Cheap enough for tests; real use takes [`KdfCost::DEFAULT`].
    const FAST: KdfCost = KdfCost {
        memory: 64,
        iterations: 1,
        lanes: 1,
    };

    #[test]
    fn the_right_password_unlocks_the_same_key() {
        let (first, second) = (password(), password());
        let (key, wrapped) = DataKey::create(&first, FAST).unwrap();
        let again = DataKey::unlock(&first, &wrapped).unwrap();
        assert_eq!(key.0.as_ref(), again.0.as_ref());
        assert!(matches!(
            DataKey::unlock(&second, &wrapped),
            Err(SecretError::WrongPassword)
        ));
        // A new password keeps the key; the old one stops working.
        let rewrapped = key.wrap(&second, FAST).unwrap();
        assert_ne!(rewrapped.salt, wrapped.salt);
        assert!(DataKey::unlock(&first, &rewrapped).is_err());
        assert_eq!(
            DataKey::unlock(&second, &rewrapped).unwrap().0.as_ref(),
            key.0.as_ref()
        );
    }

    #[test]
    fn sealed_values_need_the_key_and_their_own_entry() {
        let inner = Arc::new(MemorySecretStore::default());
        let vault = VaultStore::new(inner.clone(), false);
        let (a, b) = (
            CredentialRef::generate().unwrap(),
            CredentialRef::generate().unwrap(),
        );
        vault.put(&a, b"saved before").unwrap();

        let master = password();
        let (key, wrapped) = DataKey::create(&master, FAST).unwrap();
        vault.protect(key, std::slice::from_ref(&a)).unwrap();
        vault.put(&b, b"saved after").unwrap();
        // The OS store only sees sealed bytes.
        for r in [&a, &b] {
            let raw = inner.get(r).unwrap().unwrap();
            assert!(is_sealed(&raw));
            assert!(!raw.windows(5).any(|w| w == b"saved"));
        }
        assert_eq!(vault.get(&a).unwrap().unwrap().as_slice(), b"saved before");

        // Locked: nothing opens, nothing new is saved.
        vault.lock();
        assert!(matches!(vault.get(&b), Err(SecretError::Locked)));
        assert!(matches!(vault.put(&b, b"x"), Err(SecretError::Locked)));

        // Swapping two sealed values between entries is detected.
        vault.unlock(DataKey::unlock(&master, &wrapped).unwrap());
        let sealed_b = inner.get(&b).unwrap().unwrap();
        inner.put(&a, &sealed_b).unwrap();
        assert!(vault.get(&a).is_err());

        // Turning protection off stores plain values again.
        inner.put(&a, &vault_seal(&vault, &a, b"back")).unwrap();
        vault.unprotect(&[a.clone(), b.clone()]).unwrap();
        assert!(!vault.is_protected());
        assert_eq!(inner.get(&b).unwrap().unwrap().as_slice(), b"saved after");
        assert_eq!(inner.get(&a).unwrap().unwrap().as_slice(), b"back");
    }

    fn vault_seal(vault: &VaultStore, r: &CredentialRef, v: &[u8]) -> Vec<u8> {
        let State::Unlocked(key) = &*vault.state() else {
            unreachable!()
        };
        key.seal_secret(r, v).unwrap()
    }

    #[test]
    fn sealed_only_refuses_plain_values() {
        let inner = Arc::new(MemorySecretStore::default());
        let vault = VaultStore::new(inner.clone(), false).sealed_only();
        let r = CredentialRef::generate().unwrap();
        assert!(matches!(
            vault.put(&r, b"x"),
            Err(SecretError::Unavailable(_))
        ));
        let (key, _) = DataKey::create(&password(), FAST).unwrap();
        vault.protect(key, &[]).unwrap();
        vault.put(&r, b"x").unwrap();
        assert!(is_sealed(&inner.get(&r).unwrap().unwrap()));
        assert!(vault.unprotect(std::slice::from_ref(&r)).is_err());
    }

    #[test]
    fn the_default_cost_is_accepted() {
        let (_, wrapped) = DataKey::create(&password(), KdfCost::DEFAULT).unwrap();
        assert_eq!(wrapped.cost, KdfCost::DEFAULT);
    }
}
