use std::sync::Arc;
use std::time::Duration;

use std::path::PathBuf;

use app_core::{
    Connector, DroppedFiles, PromptBroker, SessionStore, TerminalHub, Transfers, Tunnels,
};
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
    pub transfers: Arc<Transfers>,
    pub tunnels: Arc<Tunnels>,
    pub probes: Arc<crate::nettools::Probes>,
    /// Files dropped onto the window; uploads refer to them by token.
    pub dropped: Arc<DroppedFiles>,
    /// Where downloads are saved (chosen by the backend, not the UI).
    pub downloads_dir: PathBuf,
    pub home_dir: Option<PathBuf>,
    pub agent_available: bool,
}

/// The user's SSH agent: `SSH_AUTH_SOCK` on Unix (set for GUI apps by
/// launchd on macOS and by most Linux desktops), the OpenSSH agent pipe on
/// Windows.
fn agent_socket() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var_os("SSH_AUTH_SOCK")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute() && p.exists())
    }
    #[cfg(windows)]
    {
        Some(PathBuf::from(r"\\.\pipe\openssh-ssh-agent"))
    }
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

        let agent = agent_socket();
        let agent_available = agent.is_some();
        let connector = Arc::new(Connector {
            sessions,
            secrets,
            known_hosts: Arc::new(known_hosts),
            broker: broker.clone(),
            timeout: CONNECT_TIMEOUT + HOST_KEY_PROMPT_TIMEOUT,
            agent,
        });

        let downloads_dir = app
            .path()
            .download_dir()
            .or_else(|_| app.path().home_dir())
            .unwrap_or_else(|_| data_dir.clone());

        Ok(Self {
            hub: Arc::new(TerminalHub::with_default_flow()),
            connector,
            broker,
            secrets_available,
            transfers: Arc::new(Transfers::default()),
            tunnels: Arc::new(Tunnels::default()),
            probes: Arc::new(crate::nettools::Probes::default()),
            dropped: Arc::new(DroppedFiles::default()),
            downloads_dir,
            home_dir: app.path().home_dir().ok(),
            agent_available,
        })
    }
}
