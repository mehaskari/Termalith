//! Keyboard-interactive authentication (RFC 4256): PAM password prompts and
//! second factors such as one-time codes.
//!
//! A lone hidden prompt that asks for a password is answered with the
//! password the user already gave (servers that only offer
//! keyboard-interactive for passwords). Every other prompt goes to the
//! [`KbdInteractive`] handler, i.e. the UI.

use std::future::Future;
use std::pin::Pin;

use russh::MethodKind;
use russh::client::{AuthResult, Handle, KeyboardInteractiveAuthResponse};
use zeroize::Zeroizing;

use crate::SshError;
use crate::session::ClientHandler;

/// The most info-request rounds accepted before giving up.
const MAX_ROUNDS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbdPrompt {
    pub text: String,
    /// Whether the answer may be shown while typed (false for secrets).
    pub echo: bool,
}

/// Prompts from the server. All text is server-controlled and untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KbdRequest {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub name: String,
    pub instructions: String,
    pub prompts: Vec<KbdPrompt>,
}

/// Answers in prompt order; `None` cancels authentication.
pub type KbdAnswers = Option<Vec<Zeroizing<String>>>;

/// Answers keyboard-interactive prompts.
pub trait KbdInteractive: Send + Sync {
    fn answer<'a>(
        &'a self,
        request: &'a KbdRequest,
    ) -> Pin<Box<dyn Future<Output = KbdAnswers> + Send + 'a>>;
}

pub(crate) fn offers_kbd(result: &AuthResult) -> bool {
    matches!(result, AuthResult::Failure { remaining_methods, .. }
        if remaining_methods.contains(&MethodKind::KeyboardInteractive))
}

fn is_password_prompt(p: &KbdPrompt) -> bool {
    let t = p.text.to_lowercase();
    !p.echo && (t.contains("password") || t.contains("passphrase"))
}

/// Runs keyboard-interactive authentication. `password` answers at most one
/// password-looking prompt; anything else is asked through `handler`.
pub(crate) async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    host: &str,
    port: u16,
    user: &str,
    password: Option<&Zeroizing<String>>,
    handler: Option<&dyn KbdInteractive>,
) -> Result<bool, SshError> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(user, None)
        .await?;
    let mut password_used = false;
    for _ in 0..MAX_ROUNDS {
        let (name, instructions, prompts) = match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => (name, instructions, prompts),
        };
        let prompts: Vec<KbdPrompt> = prompts
            .into_iter()
            .map(|p| KbdPrompt {
                text: p.prompt,
                echo: p.echo,
            })
            .collect();
        let answers: Vec<Zeroizing<String>> = if prompts.is_empty() {
            Vec::new()
        } else if let (Some(pw), [p]) = (password, prompts.as_slice())
            && !password_used
            && is_password_prompt(p)
        {
            password_used = true;
            vec![pw.clone()]
        } else {
            let Some(h) = handler else {
                return Ok(false);
            };
            let request = KbdRequest {
                host: host.to_owned(),
                port,
                user: user.to_owned(),
                name,
                instructions,
                prompts,
            };
            match h.answer(&request).await {
                Some(a) if a.len() == request.prompts.len() => a,
                Some(_) => return Err(SshError::Protocol("wrong number of answers".into())),
                None => return Err(SshError::Cancelled),
            }
        };
        response = handle
            .authenticate_keyboard_interactive_respond(
                answers.iter().map(|a| a.as_str().to_owned()).collect(),
            )
            .await?;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_prompt_detection() {
        let p = |text: &str, echo| KbdPrompt {
            text: text.into(),
            echo,
        };
        assert!(is_password_prompt(&p("Password: ", false)));
        assert!(is_password_prompt(&p("admin@host's password:", false)));
        assert!(!is_password_prompt(&p("Verification code: ", false)));
        assert!(
            !is_password_prompt(&p("Password: ", true)),
            "echoed prompts are not secrets"
        );
    }
}
