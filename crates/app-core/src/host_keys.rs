//! Bridges russh's host-key callback to a UI dialog: each unknown key becomes
//! a [`HostKeyPrompt`] delivered to the UI, and the connection waits for
//! [`PromptBroker::respond`]. No answer within the timeout means "reject".

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use protocol_ssh::{HostKeyDecider, HostKeyDecision, HostKeyInfo};
use serde::Serialize;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyPrompt {
    pub request_id: u64,
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    /// Set when a different certificate was trusted before (RDP only; a
    /// changed SSH host key never prompts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_fingerprint: Option<String>,
}

type Notifier = Box<dyn Fn(HostKeyPrompt) + Send + Sync>;

pub struct PromptBroker {
    pending: Mutex<HashMap<u64, oneshot::Sender<HostKeyDecision>>>,
    next: AtomicU64,
    notify: Notifier,
    timeout: Duration,
}

impl PromptBroker {
    pub fn new(notify: impl Fn(HostKeyPrompt) + Send + Sync + 'static, timeout: Duration) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            notify: Box::new(notify),
            timeout,
        }
    }

    /// Delivers the user's answer. Returns `false` if the request is unknown
    /// (already answered, timed out, or never issued).
    pub fn respond(&self, request_id: u64, decision: HostKeyDecision) -> bool {
        let tx = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&request_id);
        tx.is_some_and(|tx| tx.send(decision).is_ok())
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl PromptBroker {
    /// Shows `prompt` (its `request_id` is assigned here) and waits for the
    /// answer; no answer within the timeout rejects.
    pub async fn ask(&self, mut prompt: HostKeyPrompt) -> HostKeyDecision {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        prompt.request_id = id;
        (self.notify)(prompt);
        let decision = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(d)) => d,
            _ => HostKeyDecision::Reject,
        };
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        decision
    }
}

impl HostKeyDecider for PromptBroker {
    fn decide<'a>(
        &'a self,
        info: &'a HostKeyInfo,
    ) -> Pin<Box<dyn Future<Output = HostKeyDecision> + Send + 'a>> {
        Box::pin(self.ask(HostKeyPrompt {
            request_id: 0,
            host: info.host.clone(),
            port: info.port,
            algorithm: info.algorithm.clone(),
            fingerprint: info.fingerprint.clone(),
            previous_fingerprint: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn info() -> HostKeyInfo {
        let key = protocol_ssh::PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2lES7/sb57Bd3YUeuSm4Hzfn0koMxFvyrAdqlrzQwa test",
        )
        .unwrap();
        HostKeyInfo::new("example.test", 22, &key)
    }

    #[tokio::test]
    async fn answer_is_delivered() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = Arc::clone(&seen);
        let broker = Arc::new(PromptBroker::new(
            move |p| s.lock().unwrap().push(p),
            Duration::from_secs(5),
        ));
        let b = Arc::clone(&broker);
        let info = info();
        let task = tokio::spawn(async move { b.decide(&info).await });
        while seen.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let prompt = seen.lock().unwrap()[0].clone();
        assert_eq!(prompt.host, "example.test");
        assert!(prompt.fingerprint.starts_with("SHA256:"));
        assert!(broker.respond(prompt.request_id, HostKeyDecision::AcceptOnce));
        assert_eq!(task.await.unwrap(), HostKeyDecision::AcceptOnce);
        assert!(!broker.respond(prompt.request_id, HostKeyDecision::AcceptOnce));
        assert_eq!(broker.pending_count(), 0);
    }

    #[tokio::test]
    async fn no_answer_means_reject() {
        let broker = PromptBroker::new(|_| {}, Duration::from_millis(50));
        assert_eq!(broker.decide(&info()).await, HostKeyDecision::Reject);
        assert_eq!(broker.pending_count(), 0);
    }

    #[tokio::test]
    async fn unknown_request_ids_are_ignored() {
        let broker = PromptBroker::new(|_| {}, Duration::from_secs(1));
        assert!(!broker.respond(42, HostKeyDecision::AcceptAndSave));
    }
}
