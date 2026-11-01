//! Turns a saved session (or a quick-connect target) into an authenticated
//! SSH connection: resolves the secret, runs host-key verification through
//! the prompt broker, and saves a new password only after it worked.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use credential_store::{CredentialRef, SecretStore};
use protocol_ssh::{
    Auth, ConnectOptions, KbdInteractive, KnownHostsStore, SshError, SshSession, Target,
};
use zeroize::Zeroizing;

use crate::sessions::{
    AuthMethod, MAX_JUMP_DEPTH, SessionId, SessionInput, SessionProtocol, SessionStore,
};
use crate::{AppError, PromptBroker};

/// A secret typed by the user for this connection.
pub enum PasswordInput {
    None,
    Provided {
        secret: Zeroizing<String>,
        /// Save to the OS credential store if the connection succeeds
        /// (saved sessions only).
        save: bool,
    },
}

pub enum ConnectRequest {
    Saved {
        id: SessionId,
        secret: PasswordInput,
    },
    Quick {
        host: String,
        port: u16,
        username: String,
        auth: AuthMethod,
        secret: PasswordInput,
    },
}

pub struct Connector {
    pub sessions: Arc<SessionStore>,
    pub secrets: Arc<dyn SecretStore>,
    pub known_hosts: Arc<dyn KnownHostsStore>,
    pub broker: Arc<PromptBroker>,
    pub timeout: Duration,
    /// SSH agent socket (Unix) or named pipe (Windows), if one is available.
    pub agent: Option<PathBuf>,
    /// Asks the user keyboard-interactive questions (one-time codes).
    pub kbd: Option<Arc<dyn KbdInteractive>>,
}

/// Result of a successful connection.
pub struct Connected {
    pub session: Arc<SshSession>,
    /// Tab title, e.g. the session name or `user@host`.
    pub label: String,
}

impl Connector {
    pub async fn connect(&self, req: ConnectRequest) -> Result<Connected, AppError> {
        match req {
            ConnectRequest::Saved { id, secret } => self.connect_saved(&id, secret).await,
            ConnectRequest::Quick {
                host,
                port,
                username,
                auth,
                secret,
            } => {
                let input = SessionInput {
                    name: format!("{username}@{host}"),
                    host,
                    port,
                    username,
                    auth,
                    folder: None,
                    jump_via: None,
                    protocol: SessionProtocol::Ssh,
                    domain: None,
                    tags: Vec::new(),
                };
                input.validate()?;
                let typed = match secret {
                    PasswordInput::Provided { secret, .. } => Some(secret),
                    PasswordInput::None => None,
                };
                if matches!(input.auth, AuthMethod::Password) && typed.is_none() {
                    return Err(AppError::PasswordRequired);
                }
                let session = self.dial(&input, typed, 0).await?;
                Ok(Connected {
                    session: Arc::new(session),
                    label: input.name,
                })
            }
        }
    }

    async fn connect_saved(
        &self,
        id: &SessionId,
        secret: PasswordInput,
    ) -> Result<Connected, AppError> {
        let rec = self.sessions.get(id)?;
        if rec.input.protocol != SessionProtocol::Ssh {
            return Err(AppError::invalid("session", "not an SSH session"));
        }
        let (typed, save) = match secret {
            PasswordInput::Provided { secret, save } => (Some(secret), save),
            PasswordInput::None => (None, false),
        };
        let used_saved = typed.is_none() && rec.credential.is_some();
        let secret = match typed.clone() {
            Some(s) => Some(s),
            None => match &rec.credential {
                Some(c) => self.load_secret(c.clone()).await?,
                None => None,
            },
        };
        if matches!(rec.input.auth, AuthMethod::Password) && secret.is_none() {
            return Err(AppError::PasswordRequired);
        }

        let session = match self.dial(&rec.input, secret, 0).await {
            Ok(s) => s,
            // A saved password that no longer works: ask the user again.
            Err(AppError::Ssh(SshError::AuthFailed))
                if used_saved && matches!(rec.input.auth, AuthMethod::Password) =>
            {
                return Err(AppError::PasswordRequired);
            }
            Err(e) => return Err(e),
        };

        if save && let Some(secret) = typed {
            let cred = match &rec.credential {
                Some(c) => c.clone(),
                None => CredentialRef::generate()?,
            };
            self.store_secret(cred.clone(), secret).await?;
            if rec.credential.is_none() {
                self.sessions.set_credential(id, Some(&cred))?;
            }
        }
        Ok(Connected {
            session: Arc::new(session),
            label: rec.input.name.clone(),
        })
    }

    async fn dial(
        &self,
        input: &SessionInput,
        secret: Option<Zeroizing<String>>,
        depth: usize,
    ) -> Result<SshSession, AppError> {
        let via = match &input.jump_via {
            Some(jump) => Some(Arc::new(self.jump(jump, depth + 1).await?)),
            None => None,
        };
        let auth = match &input.auth {
            AuthMethod::Password => Auth::Password(secret.ok_or(AppError::PasswordRequired)?),
            AuthMethod::KeyFile { path } => Auth::KeyFile {
                path: path.clone(),
                passphrase: secret,
            },
            AuthMethod::Agent => Auth::Agent {
                socket: self.agent.clone().ok_or_else(|| {
                    AppError::invalid(
                        "ssh-agent",
                        "no SSH agent is available (SSH_AUTH_SOCK is not set)",
                    )
                })?,
            },
        };
        let target = Target {
            host: input.host.trim().to_owned(),
            port: input.port,
            user: input.username.clone(),
        };
        let mut opts = ConnectOptions::new(Arc::clone(&self.known_hosts), self.broker.clone());
        opts.timeout = self.timeout;
        opts.kbd = self.kbd.clone();
        Ok(match via {
            Some(jump) => SshSession::connect_via(jump, &target, &auth, &opts).await?,
            None => SshSession::connect(&target, &auth, &opts).await?,
        })
    }

    /// Connects saved SSH session `id` non-interactively, for use as a
    /// tunnel (e.g. RDP through a bastion).
    pub async fn connect_tunnel(&self, id: &SessionId) -> Result<Arc<SshSession>, AppError> {
        Ok(Arc::new(self.jump(id, 1).await?))
    }

    /// Connects a jump host non-interactively: it must have a saved
    /// password, a key file or use the agent. Host-key prompts still apply.
    fn jump<'a>(
        &'a self,
        id: &'a SessionId,
        depth: usize,
    ) -> Pin<Box<dyn Future<Output = Result<SshSession, AppError>> + Send + 'a>> {
        Box::pin(async move {
            if depth > MAX_JUMP_DEPTH {
                return Err(AppError::invalid("jump host", "chain is too long"));
            }
            let rec = self.sessions.get(id).map_err(|e| match e {
                AppError::NotFound => {
                    AppError::invalid("jump host", "the jump session no longer exists")
                }
                other => other,
            })?;
            let name = rec.input.name.clone();
            let secret = match &rec.credential {
                Some(c) => self.load_secret(c.clone()).await?,
                None => None,
            };
            if matches!(rec.input.auth, AuthMethod::Password) && secret.is_none() {
                return Err(AppError::invalid(
                    "jump host",
                    format!("{name} needs a saved password, a key file or the SSH agent"),
                ));
            }
            self.dial(&rec.input, secret, depth)
                .await
                .map_err(|e| match e {
                    AppError::Ssh(SshError::AuthFailed) => {
                        AppError::invalid("jump host", format!("authentication to {name} failed"))
                    }
                    other => other,
                })
        })
    }

    async fn load_secret(
        &self,
        cred: CredentialRef,
    ) -> Result<Option<Zeroizing<String>>, AppError> {
        let secrets = Arc::clone(&self.secrets);
        // The OS store may block (e.g. a Keychain access prompt).
        let bytes = tokio::task::spawn_blocking(move || secrets.get(&cred))
            .await
            .map_err(|e| AppError::Db(e.to_string()))??;
        Ok(bytes.map(|b| Zeroizing::new(String::from_utf8_lossy(&b).into_owned())))
    }

    async fn store_secret(
        &self,
        cred: CredentialRef,
        secret: Zeroizing<String>,
    ) -> Result<(), AppError> {
        let secrets = Arc::clone(&self.secrets);
        tokio::task::spawn_blocking(move || secrets.put(&cred, secret.as_bytes()))
            .await
            .map_err(|e| AppError::Db(e.to_string()))??;
        Ok(())
    }

    /// Deletes a session and its saved secret.
    pub async fn delete_session(&self, id: &SessionId) -> Result<(), AppError> {
        if let Some(cred) = self.sessions.delete(id)? {
            let secrets = Arc::clone(&self.secrets);
            tokio::task::spawn_blocking(move || secrets.delete(&cred))
                .await
                .map_err(|e| AppError::Db(e.to_string()))??;
        }
        Ok(())
    }

    /// Removes the recorded host key(s) for `host:port` from the app's
    /// known-hosts file, after the user verified a changed key out of band.
    pub fn forget_host_key(
        &self,
        host: &str,
        port: u16,
    ) -> Result<protocol_ssh::RemovedHostKeys, AppError> {
        Ok(self.known_hosts.remove(host.trim(), port)?)
    }

    /// Forgets the saved secret of a session.
    pub async fn forget_secret(&self, id: &SessionId) -> Result<(), AppError> {
        let rec = self.sessions.get(id)?;
        if let Some(cred) = rec.credential {
            let secrets = Arc::clone(&self.secrets);
            tokio::task::spawn_blocking(move || secrets.delete(&cred))
                .await
                .map_err(|e| AppError::Db(e.to_string()))??;
            self.sessions.set_credential(id, None)?;
        }
        Ok(())
    }
}
