//! Master password for saved secrets (ADR-032): turning it on, off and
//! changing it, unlocking, and the reset when it is forgotten. The wrapped
//! data key is kept in the settings table; it is not secret on its own.

use std::sync::Arc;

use credential_store::{CredentialRef, DataKey, KdfCost, SecretStore, VaultStore, WrappedKey};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::AppError;
use crate::sessions::SessionStore;

const MASTER_KEY: &str = "master_key";
pub const MIN_MASTER_PASSWORD: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterStatus {
    /// A master password is set.
    pub protected: bool,
    /// Saved secrets cannot be read until it is entered.
    pub locked: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredKey {
    memory: u32,
    iterations: u32,
    lanes: u32,
    salt: String,
    sealed: String,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (s.len().is_multiple_of(2))
        .then(|| {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
                .collect()
        })
        .flatten()
}

impl SessionStore {
    pub fn master_key(&self) -> Result<Option<WrappedKey>, AppError> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [MASTER_KEY],
                |r| r.get(0),
            )
            .optional()?;
        let Some(raw) = raw else { return Ok(None) };
        let k: StoredKey = serde_json::from_str(&raw)
            .map_err(|_| AppError::Db("the master password record is unreadable".into()))?;
        let salt = unhex(&k.salt)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| AppError::Db("the master password record is unreadable".into()))?;
        Ok(Some(WrappedKey {
            cost: KdfCost {
                memory: k.memory,
                iterations: k.iterations,
                lanes: k.lanes,
            },
            salt,
            sealed: unhex(&k.sealed)
                .ok_or_else(|| AppError::Db("the master password record is unreadable".into()))?,
        }))
    }

    fn set_master_key(&self, key: Option<&WrappedKey>) -> Result<(), AppError> {
        match key {
            Some(k) => {
                let json = serde_json::to_string(&StoredKey {
                    memory: k.cost.memory,
                    iterations: k.cost.iterations,
                    lanes: k.cost.lanes,
                    salt: hex(&k.salt),
                    sealed: hex(&k.sealed),
                })
                .map_err(|e| AppError::Db(e.to_string()))?;
                self.conn().execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![MASTER_KEY, json],
                )?
            }
            None => self
                .conn()
                .execute("DELETE FROM settings WHERE key = ?1", [MASTER_KEY])?,
        };
        Ok(())
    }

    /// Every saved secret: sessions' and the proxy's.
    fn credential_refs(&self) -> Result<Vec<CredentialRef>, AppError> {
        let mut refs: Vec<CredentialRef> = self
            .list()?
            .into_iter()
            .filter_map(|s| s.credential)
            .collect();
        refs.extend(self.proxy_credential()?);
        Ok(refs)
    }
}

/// The master password operations, over the app's session store and the
/// vault in front of the OS credential store.
pub struct MasterPassword {
    pub store: Arc<SessionStore>,
    pub vault: Arc<VaultStore>,
    pub cost: KdfCost,
}

/// Argon2 takes a noticeable moment: off the async runtime.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Db(format!("background task failed: {e}")))?
}

fn check_new(password: &str) -> Result<(), AppError> {
    if password.chars().count() < MIN_MASTER_PASSWORD {
        return Err(AppError::invalid(
            "master password",
            format!("must be at least {MIN_MASTER_PASSWORD} characters"),
        ));
    }
    Ok(())
}

impl MasterPassword {
    pub fn status(&self) -> MasterStatus {
        MasterStatus {
            protected: self.vault.is_protected(),
            locked: self.vault.is_locked(),
        }
    }

    fn wrapped(&self) -> Result<WrappedKey, AppError> {
        self.store
            .master_key()?
            .ok_or_else(|| AppError::invalid("master password", "none is set"))
    }

    async fn open(&self, password: Zeroizing<String>) -> Result<DataKey, AppError> {
        let wrapped = self.wrapped()?;
        blocking(move || Ok(DataKey::unlock(&password, &wrapped)?)).await
    }

    /// Sets a master password and seals every saved secret with it.
    pub async fn enable(&self, password: Zeroizing<String>) -> Result<(), AppError> {
        if self.vault.is_protected() {
            return Err(AppError::invalid("master password", "is already set"));
        }
        check_new(&password)?;
        let cost = self.cost;
        let (key, wrapped) = blocking(move || Ok(DataKey::create(&password, cost)?)).await?;
        self.vault.protect(key, &self.store.credential_refs()?)?;
        self.store.set_master_key(Some(&wrapped))
    }

    pub async fn unlock(&self, password: Zeroizing<String>) -> Result<(), AppError> {
        let key = self.open(password).await?;
        self.vault.unlock(key);
        Ok(())
    }

    pub fn lock(&self) {
        self.vault.lock();
    }

    /// A new master password for the same data key.
    pub async fn change(
        &self,
        old: Zeroizing<String>,
        new: Zeroizing<String>,
    ) -> Result<(), AppError> {
        check_new(&new)?;
        let key = self.open(old).await?;
        let cost = self.cost;
        let (key, wrapped) = blocking(move || {
            let wrapped = key.wrap(&new, cost)?;
            Ok((key, wrapped))
        })
        .await?;
        self.store.set_master_key(Some(&wrapped))?;
        self.vault.unlock(key);
        Ok(())
    }

    /// Removes the master password; saved secrets are stored plain again.
    pub async fn disable(&self, password: Zeroizing<String>) -> Result<(), AppError> {
        let key = self.open(password).await?;
        self.vault.unlock(key);
        self.vault.unprotect(&self.store.credential_refs()?)?;
        self.store.set_master_key(None)
    }

    /// For a forgotten master password: deletes every saved secret (they
    /// cannot be opened any more) and the master password itself.
    pub fn reset(&self) -> Result<(), AppError> {
        for session in self.store.list()? {
            if let Some(r) = &session.credential {
                self.vault.delete(r)?;
                self.store.set_credential(&session.id, None)?;
            }
        }
        if let Some(r) = self.store.proxy_credential()? {
            self.vault.delete(&r)?;
            self.store.set_proxy_credential(None)?;
        }
        self.store.set_master_key(None)?;
        self.vault.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use credential_store::MemorySecretStore;

    const FAST: KdfCost = KdfCost {
        memory: 64,
        iterations: 1,
        lanes: 1,
    };

    fn pw(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.to_owned())
    }

    /// A random password per run, so no secret is written in the source.
    fn random_password() -> String {
        let mut b = [0u8; 12];
        getrandom::fill(&mut b).unwrap();
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn rig() -> (MasterPassword, Arc<MemorySecretStore>, CredentialRef) {
        let store = Arc::new(SessionStore::open_in_memory().unwrap());
        let os = Arc::new(MemorySecretStore::default());
        let vault = Arc::new(VaultStore::new(os.clone(), false));
        let cred = CredentialRef::generate().unwrap();
        store.set_proxy_credential(Some(&cred)).unwrap();
        vault.put(&cred, b"proxy secret").unwrap();
        (
            MasterPassword {
                store,
                vault,
                cost: FAST,
            },
            os,
            cred,
        )
    }

    #[tokio::test]
    async fn enable_lock_unlock_change_disable() {
        let (m, os, cred) = rig();
        let (first, second, wrong) = (random_password(), random_password(), random_password());
        assert!(m.enable(pw("short")).await.is_err(), "too short");
        m.enable(pw(&first)).await.unwrap();
        assert!(m.store.master_key().unwrap().is_some());
        assert_ne!(os.get(&cred).unwrap().unwrap().as_slice(), b"proxy secret");

        m.lock();
        assert_eq!(
            m.status(),
            MasterStatus {
                protected: true,
                locked: true
            }
        );
        assert!(m.vault.get(&cred).is_err());
        assert!(matches!(
            m.unlock(pw(&wrong)).await,
            Err(AppError::Secret(
                credential_store::SecretError::WrongPassword
            ))
        ));
        m.unlock(pw(&first)).await.unwrap();
        assert_eq!(
            m.vault.get(&cred).unwrap().unwrap().as_slice(),
            b"proxy secret"
        );

        m.change(pw(&first), pw(&second)).await.unwrap();
        m.lock();
        assert!(m.unlock(pw(&first)).await.is_err());
        m.unlock(pw(&second)).await.unwrap();

        // A store reopened later starts locked and opens with the new password.
        let again = MasterPassword {
            store: Arc::clone(&m.store),
            vault: Arc::new(VaultStore::new(os.clone(), true)),
            cost: FAST,
        };
        again.unlock(pw(&second)).await.unwrap();

        m.disable(pw(&second)).await.unwrap();
        assert!(!m.status().protected);
        assert!(m.store.master_key().unwrap().is_none());
        assert_eq!(os.get(&cred).unwrap().unwrap().as_slice(), b"proxy secret");
    }

    #[tokio::test]
    async fn reset_forgets_secrets_and_the_password() {
        let (m, os, cred) = rig();
        m.enable(pw(&random_password())).await.unwrap();
        m.lock();
        m.reset().unwrap();
        assert!(!m.status().protected);
        assert!(m.store.master_key().unwrap().is_none());
        assert!(m.store.proxy_credential().unwrap().is_none());
        assert!(os.get(&cred).unwrap().is_none());
    }
}
