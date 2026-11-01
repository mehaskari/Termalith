//! Bridges keyboard-interactive prompts (one-time codes, PAM questions) to a
//! UI dialog, like [`crate::PromptBroker`] does for host keys. No answer
//! within the timeout cancels the login.
//!
//! Prompt text comes from the server: control characters are removed and
//! lengths are capped before it reaches the UI. Answers are zeroized.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use protocol_ssh::{KbdInteractive, KbdRequest};
use serde::Serialize;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

const MAX_PROMPTS: usize = 10;
const MAX_PROMPT_CHARS: usize = 200;
const MAX_TEXT_CHARS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPromptField {
    pub text: String,
    /// Show what is typed (false: a secret).
    pub echo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub request_id: u64,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub name: String,
    pub instructions: String,
    pub fields: Vec<AuthPromptField>,
}

type Answers = Option<Vec<Zeroizing<String>>>;
type Notifier = Box<dyn Fn(AuthPrompt) + Send + Sync>;

pub struct AuthPromptBroker {
    pending: Mutex<HashMap<u64, oneshot::Sender<Answers>>>,
    next: AtomicU64,
    notify: Notifier,
    timeout: Duration,
}

/// Removes control characters (keeps newlines in instructions) and caps
/// the length.
fn clean(s: &str, max: usize, keep_newlines: bool) -> String {
    s.chars()
        .filter(|c| !c.is_control() || (keep_newlines && *c == '\n'))
        .take(max)
        .collect()
}

impl AuthPromptBroker {
    pub fn new(notify: impl Fn(AuthPrompt) + Send + Sync + 'static, timeout: Duration) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            notify: Box::new(notify),
            timeout,
        }
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashMap<u64, oneshot::Sender<Answers>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Delivers the user's answers (`None`: cancelled). `false` if the
    /// request is unknown (answered, timed out or never issued).
    pub fn respond(&self, request_id: u64, answers: Answers) -> bool {
        self.pending()
            .remove(&request_id)
            .is_some_and(|tx| tx.send(answers).is_ok())
    }

    pub fn pending_count(&self) -> usize {
        self.pending().len()
    }
}

impl KbdInteractive for AuthPromptBroker {
    fn answer<'a>(
        &'a self,
        request: &'a KbdRequest,
    ) -> Pin<Box<dyn Future<Output = Answers> + Send + 'a>> {
        Box::pin(async move {
            if request.prompts.len() > MAX_PROMPTS {
                return None;
            }
            let id = self.next.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.pending().insert(id, tx);
            (self.notify)(AuthPrompt {
                request_id: id,
                host: request.host.clone(),
                port: request.port,
                user: clean(&request.user, MAX_PROMPT_CHARS, false),
                name: clean(&request.name, MAX_PROMPT_CHARS, false),
                instructions: clean(&request.instructions, MAX_TEXT_CHARS, true),
                fields: request
                    .prompts
                    .iter()
                    .map(|p| AuthPromptField {
                        text: clean(&p.text, MAX_PROMPT_CHARS, false),
                        echo: p.echo,
                    })
                    .collect(),
            });
            let answers = match tokio::time::timeout(self.timeout, rx).await {
                Ok(Ok(a)) => a,
                _ => None,
            };
            self.pending().remove(&id);
            answers.filter(|a| a.len() == request.prompts.len())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol_ssh::KbdPrompt;
    use std::sync::Arc;

    fn request(n: usize) -> KbdRequest {
        KbdRequest {
            host: "h".into(),
            port: 22,
            user: "u".into(),
            name: "2FA\x1b[31m".into(),
            instructions: "line 1\nline 2\x07".into(),
            prompts: (0..n)
                .map(|i| KbdPrompt {
                    text: format!("Code {i}:\r"),
                    echo: true,
                })
                .collect(),
        }
    }

    fn broker(timeout: Duration) -> (Arc<AuthPromptBroker>, Arc<Mutex<Vec<AuthPrompt>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = Arc::clone(&seen);
        (
            Arc::new(AuthPromptBroker::new(
                move |p| s.lock().unwrap().push(p),
                timeout,
            )),
            seen,
        )
    }

    #[tokio::test]
    async fn answers_reach_the_connection_and_text_is_sanitized() {
        let (b, seen) = broker(Duration::from_secs(5));
        let b2 = Arc::clone(&b);
        let task = tokio::spawn(async move { b2.answer(&request(1)).await });
        while seen.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let p = seen.lock().unwrap()[0].clone();
        assert_eq!(p.name, "2FA[31m");
        assert_eq!(p.instructions, "line 1\nline 2");
        assert_eq!(p.fields[0].text, "Code 0:");
        assert!(b.respond(p.request_id, Some(vec![Zeroizing::new("42".into())])));
        let got = task.await.unwrap().unwrap();
        assert_eq!(got[0].as_str(), "42");
        assert!(!b.respond(p.request_id, None), "answered once");
        assert_eq!(b.pending_count(), 0);
    }

    #[tokio::test]
    async fn timeout_wrong_count_and_too_many_prompts_cancel() {
        let (b, seen) = broker(Duration::from_millis(20));
        assert!(b.answer(&request(1)).await.is_none(), "timed out");
        assert!(b.answer(&request(MAX_PROMPTS + 1)).await.is_none());
        assert_eq!(
            seen.lock().unwrap().len(),
            1,
            "too many prompts never reach the UI"
        );

        let (b, seen) = broker(Duration::from_secs(5));
        let b2 = Arc::clone(&b);
        let task = tokio::spawn(async move { b2.answer(&request(2)).await });
        while seen.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
        let id = seen.lock().unwrap()[0].request_id;
        b.respond(id, Some(vec![Zeroizing::new("only one".into())]));
        assert!(task.await.unwrap().is_none());
    }
}
