//! Host-key trust decisions.

use std::future::Future;
use std::pin::Pin;

use russh::keys::{HashAlg, PublicKey};

/// What the user is shown when asked to trust a host key.
#[derive(Debug, Clone)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    /// `SHA256:<base64>`, as printed by OpenSSH.
    pub fingerprint: String,
    pub key: PublicKey,
}

impl HostKeyInfo {
    pub fn new(host: &str, port: u16, key: &PublicKey) -> Self {
        Self {
            host: host.to_owned(),
            port,
            algorithm: key.algorithm().to_string(),
            fingerprint: fingerprint(key),
            key: key.clone(),
        }
    }
}

pub(crate) fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// How a presented key relates to what is recorded for the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    /// Exactly matches a recorded key.
    Trusted,
    /// Nothing recorded for this host.
    Unknown,
    /// Keys are recorded for this host but none matches, whatever their
    /// algorithm. Treated as a possible attack.
    Changed { known_fingerprints: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyDecision {
    Reject,
    /// Trust for this connection only.
    AcceptOnce,
    /// Trust and record in the app's known-hosts store.
    AcceptAndSave,
}

/// Asks the user (or a policy) whether to trust an **unknown** host key.
/// Never consulted for changed keys: those fail with
/// [`crate::SshError::HostKeyChanged`] and need an explicit remediation step.
pub trait HostKeyDecider: Send + Sync {
    fn decide<'a>(
        &'a self,
        info: &'a HostKeyInfo,
    ) -> Pin<Box<dyn Future<Output = HostKeyDecision> + Send + 'a>>;
}

/// Decider that rejects every unknown key. Safe default for non-interactive
/// use.
#[derive(Debug, Default, Clone, Copy)]
pub struct RejectUnknown;

impl HostKeyDecider for RejectUnknown {
    fn decide<'a>(
        &'a self,
        _info: &'a HostKeyInfo,
    ) -> Pin<Box<dyn Future<Output = HostKeyDecision> + Send + 'a>> {
        Box::pin(async { HostKeyDecision::Reject })
    }
}
