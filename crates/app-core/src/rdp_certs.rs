//! RDP server certificates the user trusts. Used for clients that cannot
//! show their own certificate prompt (FreeRDP without a terminal): the
//! certificate is read first, checked here or shown in the same dialog as
//! SSH host keys, and the client is then told to accept only that one.

use std::sync::Arc;

use protocol_ssh::HostKeyDecision;
use remote_desktop::{CertFingerprint, DesktopError};
use rusqlite::{OptionalExtension, params};

use crate::sessions::SessionStore;
use crate::{AppError, HostKeyPrompt, PromptBroker};

/// Shown as the key type in the dialog.
pub const RDP_CERT_ALGORITHM: &str = "RDP TLS certificate (SHA-256)";
pub const FTPS_CERT_ALGORITHM: &str = "FTPS TLS certificate (SHA-256)";

impl SessionStore {
    /// The trusted certificate for `host:port`, if any.
    pub fn rdp_certificate(
        &self,
        host: &str,
        port: u16,
    ) -> Result<Option<CertFingerprint>, AppError> {
        let hex: Option<String> = self
            .conn()
            .query_row(
                "SELECT sha256 FROM rdp_certificates WHERE host = ?1 AND port = ?2",
                params![host.trim(), port],
                |r| r.get(0),
            )
            .optional()?;
        Ok(hex.as_deref().and_then(CertFingerprint::parse))
    }

    /// Trusts `fp` for `host:port`, replacing an earlier certificate.
    pub fn trust_rdp_certificate(
        &self,
        host: &str,
        port: u16,
        fp: CertFingerprint,
    ) -> Result<(), AppError> {
        self.conn().execute(
            "INSERT INTO rdp_certificates (host, port, sha256, trusted_at)
             VALUES (?1, ?2, ?3, strftime('%s', 'now'))
             ON CONFLICT (host, port) DO UPDATE SET
                sha256 = excluded.sha256, trusted_at = excluded.trusted_at",
            params![host.trim(), port, fp.hex()],
        )?;
        Ok(())
    }
}

/// Where trusted certificates are kept and how the user is asked.
pub struct CertificateCheck {
    pub store: Arc<SessionStore>,
    pub broker: Arc<PromptBroker>,
}

impl CertificateCheck {
    /// Succeeds when `fp` is trusted for `host:port` or the user accepts it
    /// now. A different certificate than the trusted one is shown with the
    /// old fingerprint (servers renew self-signed certificates).
    pub async fn verify(&self, host: &str, port: u16, fp: CertFingerprint) -> Result<(), AppError> {
        self.verify_as(RDP_CERT_ALGORITHM, host, port, fp).await
    }

    /// [`Self::verify`] with the dialog's key type. Trust is per host and
    /// port, so FTPS shares the store with RDP.
    pub async fn verify_as(
        &self,
        algorithm: &str,
        host: &str,
        port: u16,
        fp: CertFingerprint,
    ) -> Result<(), AppError> {
        let host = host.trim();
        let known = self.store.rdp_certificate(host, port)?;
        if known == Some(fp) {
            return Ok(());
        }
        let decision = self
            .broker
            .ask(HostKeyPrompt {
                request_id: 0,
                host: host.to_owned(),
                port,
                algorithm: algorithm.to_owned(),
                fingerprint: fp.display(),
                previous_fingerprint: known.map(|k| k.display()),
            })
            .await;
        match decision {
            HostKeyDecision::Reject => Err(AppError::Desktop(DesktopError::Certificate(
                "the server certificate was not trusted".into(),
            ))),
            HostKeyDecision::AcceptOnce => Ok(()),
            HostKeyDecision::AcceptAndSave => self.store.trust_rdp_certificate(host, port, fp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::Duration;

    fn store() -> (tempfile::TempDir, Arc<SessionStore>) {
        let dir = tempfile::tempdir().unwrap();
        let s = SessionStore::open(&dir.path().join("t.db")).unwrap();
        (dir, Arc::new(s))
    }

    #[test]
    fn trust_is_stored_per_host_and_port() {
        let (_d, s) = store();
        let a = CertFingerprint::of(b"a");
        let b = CertFingerprint::of(b"b");
        assert_eq!(s.rdp_certificate("srv", 3389).unwrap(), None);
        s.trust_rdp_certificate("srv", 3389, a).unwrap();
        assert_eq!(s.rdp_certificate("SRV", 3389).unwrap(), Some(a));
        assert_eq!(s.rdp_certificate("srv", 3390).unwrap(), None);
        s.trust_rdp_certificate("srv", 3389, b).unwrap();
        assert_eq!(s.rdp_certificate("srv", 3389).unwrap(), Some(b));
    }

    /// Answers every prompt with `answer` and records it.
    fn check(
        store: Arc<SessionStore>,
        answer: HostKeyDecision,
    ) -> (Arc<CertificateCheck>, Arc<Mutex<Vec<HostKeyPrompt>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let broker_slot: Arc<Mutex<Option<Arc<PromptBroker>>>> = Arc::new(Mutex::new(None));
        let (s, slot) = (Arc::clone(&seen), Arc::clone(&broker_slot));
        let broker = Arc::new(PromptBroker::new(
            move |p: HostKeyPrompt| {
                s.lock().unwrap().push(p.clone());
                let slot = Arc::clone(&slot);
                let answer = answer;
                tokio::spawn(async move {
                    let b = slot.lock().unwrap().clone().unwrap();
                    b.respond(p.request_id, answer);
                });
            },
            Duration::from_secs(5),
        ));
        *broker_slot.lock().unwrap() = Some(Arc::clone(&broker));
        (Arc::new(CertificateCheck { store, broker }), seen)
    }

    #[tokio::test]
    async fn unknown_certificate_prompts_and_saves_on_trust() {
        let (_d, s) = store();
        let fp = CertFingerprint::of(b"a");
        let (c, seen) = check(Arc::clone(&s), HostKeyDecision::AcceptAndSave);
        c.verify("srv", 3389, fp).await.unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(seen.lock().unwrap()[0].previous_fingerprint, None);
        assert_eq!(s.rdp_certificate("srv", 3389).unwrap(), Some(fp));
        // Trusted now: no second prompt.
        c.verify("srv", 3389, fp).await.unwrap();
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn changed_certificate_shows_the_old_one_and_reject_fails() {
        let (_d, s) = store();
        let old = CertFingerprint::of(b"old");
        s.trust_rdp_certificate("srv", 3389, old).unwrap();
        let (c, seen) = check(Arc::clone(&s), HostKeyDecision::Reject);
        let err = c
            .verify("srv", 3389, CertFingerprint::of(b"new"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not trusted"), "{err}");
        assert_eq!(
            seen.lock().unwrap()[0].previous_fingerprint,
            Some(old.display())
        );
        assert_eq!(s.rdp_certificate("srv", 3389).unwrap(), Some(old));
    }

    #[tokio::test]
    async fn accept_once_does_not_save() {
        let (_d, s) = store();
        let (c, _) = check(Arc::clone(&s), HostKeyDecision::AcceptOnce);
        c.verify("srv", 3389, CertFingerprint::of(b"a"))
            .await
            .unwrap();
        assert_eq!(s.rdp_certificate("srv", 3389).unwrap(), None);
    }
}
