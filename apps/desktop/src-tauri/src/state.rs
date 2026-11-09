use std::sync::Arc;
use std::time::Duration;

use std::path::PathBuf;

use app_core::{
    AuthPromptBroker, CertificateCheck, Connector, DesktopHub, DroppedFiles, MasterPassword,
    PromptBroker, ServerMonitor, SessionStore, TerminalHub, TerminalLogs, Transfers, Tunnels,
};
use credential_store::{
    FileSecretStore, KdfCost, MemorySecretStore, NativeSecretStore, SecretStore, VaultStore,
};
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
    pub auth_prompts: Arc<AuthPromptBroker>,
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
    pub desktops: Arc<DesktopHub>,
    pub logs: Arc<TerminalLogs>,
    pub monitor: Arc<ServerMonitor>,
    pub master: Arc<MasterPassword>,
    /// Running from a portable folder (ADR-033).
    pub portable: bool,
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
        // Portable: everything in the folder next to the app (ADR-033).
        let portable = crate::portable::data_root();
        let data_dir = match &portable {
            Some(root) => root.clone(),
            None => app.path().app_data_dir()?,
        };
        std::fs::create_dir_all(&data_dir)?;

        let sessions = Arc::new(SessionStore::open(&data_dir.join("termalith.db"))?);
        let sessions_for_master = Arc::clone(&sessions);

        let mut known_hosts = FileKnownHosts::new(data_dir.join("known_hosts"));
        if portable.is_none()
            && let Ok(home) = app.path().home_dir()
        {
            // Trust what the user already accepted in OpenSSH; never written.
            known_hosts = known_hosts.with_read_only(home.join(".ssh").join("known_hosts"));
        }

        // With a master password the vault starts locked (ADR-032).
        let protected = sessions.master_key()?.is_some();
        let (vault, secrets_available) = if let Some(root) = &portable {
            // This computer's credential store is not portable: a file next
            // to the data, holding only sealed values.
            let file: Arc<dyn SecretStore> = Arc::new(FileSecretStore::new(root.join("secrets")));
            (VaultStore::new(file, protected).sealed_only(), true)
        } else {
            match NativeSecretStore::new(CREDENTIAL_SERVICE) {
                Ok(s) => (VaultStore::new(Arc::new(s), protected), true),
                Err(_) => (
                    VaultStore::new(Arc::new(MemorySecretStore::default()), protected),
                    false,
                ),
            }
        };
        let vault = Arc::new(vault);
        let secrets: Arc<dyn SecretStore> = vault.clone();

        let emitter = app.clone();
        let broker = Arc::new(PromptBroker::new(
            move |prompt| {
                let _ = emitter.emit("host-key-prompt", prompt);
            },
            HOST_KEY_PROMPT_TIMEOUT,
        ));

        let emitter = app.clone();
        let auth_prompts = Arc::new(AuthPromptBroker::new(
            move |prompt| {
                let _ = emitter.emit("auth-prompt", prompt);
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
            kbd: Some(auth_prompts.clone()),
        });

        let downloads_dir = app
            .path()
            .download_dir()
            .or_else(|_| app.path().home_dir())
            .unwrap_or_else(|_| data_dir.clone());

        // Connection files for external RDP clients (no secrets in them).
        let desktop_dir = match &portable {
            Some(root) => root.join("cache"),
            None => app
                .path()
                .app_cache_dir()
                .unwrap_or_else(|_| data_dir.join("cache")),
        }
        .join("remote-desktop");
        std::fs::create_dir_all(&desktop_dir)?;

        // Session logs go where users look for documents.
        let logs_dir = match &portable {
            Some(root) => root.join("logs"),
            None => app
                .path()
                .document_dir()
                .map(|d| d.join("Termalith Logs"))
                .unwrap_or_else(|_| data_dir.join("logs")),
        };

        let transfers = Arc::new(Transfers::with_recorder(app_core::history_recorder(
            Arc::clone(&connector.sessions),
        )));

        let desktops = Arc::new(DesktopHub::new(desktop_dir).with_certificate_check(
            CertificateCheck {
                store: Arc::clone(&connector.sessions),
                broker: broker.clone(),
            },
        ));

        Ok(Self {
            hub: Arc::new(TerminalHub::with_default_flow()),
            connector,
            broker,
            auth_prompts,
            secrets_available,
            transfers,
            tunnels: Arc::new(Tunnels::default()),
            probes: Arc::new(crate::nettools::Probes::default()),
            dropped: Arc::new(DroppedFiles::default()),
            downloads_dir,
            home_dir: app.path().home_dir().ok(),
            agent_available,
            desktops,
            logs: Arc::new(TerminalLogs::new(logs_dir)),
            monitor: Arc::new(ServerMonitor::default()),
            portable: portable.is_some(),
            master: Arc::new(MasterPassword {
                store: sessions_for_master,
                vault,
                cost: KdfCost::DEFAULT,
            }),
        })
    }
}
