use std::sync::Arc;
use std::time::Duration;

use app_core::{Connector, PromptBroker, SessionStore, TerminalHub};
use credential_store::{MemorySecretStore, NativeSecretStore, SecretStore};
use protocol_ssh::FileKnownHosts;
use tauri::{AppHandle, Emitter, Manager};

/// How long a host-key dialog may stay unanswered before the connection
/// attempt is rejected.
const HOST_KEY_PROMPT_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const CREDENTIAL_SERVICE: &str = "dev.termalith.credentials";

pub struct AppState {
    pub hub: Arc<TerminalHub>,
    pub connector: Arc<Connector>,
    pub broker: Arc<PromptBroker>,
    /// False when the OS credential store is unavailable; passwords are
    /// then never persisted.
    pub secrets_available: bool,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;

        let sessions = Arc::new(SessionStore::open(&data_dir.join("termalith.db"))?);

        let mut known_hosts = FileKnownHosts::new(data_dir.join("known_hosts"));
        if let Ok(home) = app.path().home_dir() {
            // Trust what the user already accepted in OpenSSH; never written.
            known_hosts = known_hosts.with_read_only(home.join(".ssh").join("known_hosts"));
        }

        let (secrets, secrets_available): (Arc<dyn SecretStore>, bool) =
            match NativeSecretStore::new(CREDENTIAL_SERVICE) {
                Ok(s) => (Arc::new(s), true),
                Err(_) => (Arc::new(MemorySecretStore::default()), false),
            };

        let emitter = app.clone();
        let broker = Arc::new(PromptBroker::new(
            move |prompt| {
                let _ = emitter.emit("host-key-prompt", prompt);
            },
            HOST_KEY_PROMPT_TIMEOUT,
        ));

        let connector = Arc::new(Connector {
            sessions,
            secrets,
            known_hosts: Arc::new(known_hosts),
            broker: broker.clone(),
            timeout: CONNECT_TIMEOUT + HOST_KEY_PROMPT_TIMEOUT,
        });

        Ok(Self {
            hub: Arc::new(TerminalHub::with_default_flow()),
            connector,
            broker,
            secrets_available,
        })
    }
}
