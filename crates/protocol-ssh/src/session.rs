//! Connection, authentication and channels.

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle, Msg};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate, load_secret_key};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use russh_sftp::client::SftpSession;
use zeroize::Zeroizing;

use crate::host_key::{HostKeyDecider, HostKeyDecision, HostKeyInfo, HostKeyStatus};
use crate::kbd::{KbdInteractive, offers_kbd};
use crate::known_hosts::KnownHostsStore;
use crate::x11::X11Forward;
use crate::{SftpClient, SshError, algorithms};

/// Cap on output collected by [`SshSession::exec`]; longer output is
/// truncated, not buffered.
const MAX_EXEC_OUTPUT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// Credentials. Secrets are zeroized on drop where this crate owns them;
/// russh copies the password into its own buffers during authentication.
pub enum Auth {
    Password(Zeroizing<String>),
    KeyFile {
        path: PathBuf,
        passphrase: Option<Zeroizing<String>>,
    },
    /// Keys held by an SSH agent; the private keys never enter this
    /// process. `socket` is the agent's Unix socket or, on Windows, its
    /// named pipe (OpenSSH agent: `\\.\pipe\openssh-ssh-agent`).
    Agent {
        socket: PathBuf,
    },
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Auth::Password(_) => f.write_str("Auth::Password(<redacted>)"),
            Auth::KeyFile { path, passphrase } => f
                .debug_struct("Auth::KeyFile")
                .field("path", path)
                .field("passphrase", &passphrase.as_ref().map(|_| "<redacted>"))
                .finish(),
            Auth::Agent { socket } => f
                .debug_struct("Auth::Agent")
                .field("socket", socket)
                .finish(),
        }
    }
}

#[derive(Clone)]
pub struct ConnectOptions {
    pub known_hosts: Arc<dyn KnownHostsStore>,
    pub decider: Arc<dyn HostKeyDecider>,
    /// Covers TCP connect, key exchange, host-key decision and
    /// authentication.
    pub timeout: Duration,
    pub keepalive: Option<Duration>,
    /// Answers keyboard-interactive prompts (one-time codes etc.). Without
    /// one, only a plain password prompt can be answered.
    pub kbd: Option<Arc<dyn KbdInteractive>>,
    /// Forward this local agent (socket or pipe) to shells opened on the
    /// connection (`ssh -A`). Off by default: anyone with root on the
    /// server can use the agent's keys while the connection is open.
    pub agent_forward: Option<PathBuf>,
    /// Forward X11 to this local display (`ssh -X`).
    pub x11: Option<X11Forward>,
    /// Reach the server through this proxy (direct connections only; a
    /// jump host's tunnel does not use it).
    pub proxy: Option<crate::Proxy>,
}

impl ConnectOptions {
    pub fn new(known_hosts: Arc<dyn KnownHostsStore>, decider: Arc<dyn HostKeyDecider>) -> Self {
        Self {
            known_hosts,
            decider,
            timeout: Duration::from_secs(30),
            keepalive: Some(Duration::from_secs(30)),
            kbd: None,
            agent_forward: None,
            x11: None,
            proxy: None,
        }
    }
}

pub(crate) struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<dyn KnownHostsStore>,
    decider: Arc<dyn HostKeyDecider>,
    /// Why the host key was refused, so `connect` can return a typed error
    /// instead of russh's generic one.
    refusal: Arc<Mutex<Option<SshError>>>,
    remote_forwards: RemoteForwards,
    /// Local agent that server-opened agent channels are bridged to; only
    /// set when the user enabled agent forwarding for this connection.
    agent_forward: Option<PathBuf>,
    /// X11 forwarding target and the fake cookie given to the server.
    x11: Option<Arc<(X11Forward, [u8; 16])>>,
}

/// Refuses a channel the server opened without this client asking for it.
async fn refuse(reply: russh::client::ChannelOpenHandle) {
    reply
        .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
        .await;
}

/// Server port -> local `host:port` for active remote (`-R`) forwards.
type RemoteForwards = Arc<Mutex<std::collections::HashMap<u32, (String, u16)>>>;

impl ClientHandler {
    fn refuse(&self, e: SshError) -> Result<bool, russh::Error> {
        *self.refusal.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
        Ok(false)
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    /// A connection arrived on a port the server listens on for us. Only
    /// ports we asked for are accepted; the local side connects on a task
    /// so the SSH loop is never blocked.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let target = self
            .remote_forwards
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&connected_port)
            .cloned();
        let Some((host, port)) = target else {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut remote = channel.into_stream();
            match tokio::net::TcpStream::connect((host.as_str(), port)).await {
                Ok(mut local) => {
                    let _ = local.set_nodelay(true);
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Err(_) => {
                    let _ = tokio::io::AsyncWriteExt::shutdown(&mut remote).await;
                }
            }
        });
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(socket) = self.agent_forward.clone() else {
            refuse(reply).await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut remote = channel.into_stream();
            match agent::connect_raw(&socket).await {
                Ok(mut local) => {
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Err(_) => {
                    let _ = tokio::io::AsyncWriteExt::shutdown(&mut remote).await;
                }
            }
        });
        Ok(())
    }

    // Channels Termalith never requests. russh accepts these by default;
    // a client must not.
    async fn server_channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        refuse(reply).await;
        Ok(())
    }

    async fn server_channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        refuse(reply).await;
        Ok(())
    }

    async fn server_channel_open_direct_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        refuse(reply).await;
        Ok(())
    }

    async fn server_channel_open_forwarded_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        refuse(reply).await;
        Ok(())
    }

    async fn server_channel_open_x11(
        &mut self,
        channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(x11) = self.x11.clone() else {
            refuse(reply).await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            crate::x11::proxy(channel.into_stream(), &x11.0, &x11.1).await;
        });
        Ok(())
    }

    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match presented {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            PublicKeyOrCertificate::Certificate(_) => {
                return self.refuse(SshError::HostCertificateUnsupported);
            }
        };
        let info = HostKeyInfo::new(&self.host, self.port, key);
        match self.known_hosts.status(&self.host, self.port, key) {
            Err(e) => self.refuse(e),
            Ok(HostKeyStatus::Trusted) => Ok(true),
            Ok(HostKeyStatus::Changed { known_fingerprints }) => {
                self.refuse(SshError::HostKeyChanged {
                    host: info.host,
                    port: info.port,
                    presented: info.fingerprint,
                    known: known_fingerprints,
                })
            }
            Ok(HostKeyStatus::Unknown) => match self.decider.decide(&info).await {
                HostKeyDecision::Reject => self.refuse(SshError::HostKeyRejected {
                    host: info.host,
                    port: info.port,
                    fingerprint: info.fingerprint,
                }),
                HostKeyDecision::AcceptOnce => Ok(true),
                HostKeyDecision::AcceptAndSave => {
                    match self.known_hosts.add(&self.host, self.port, key) {
                        Ok(()) => Ok(true),
                        Err(e) => self.refuse(e),
                    }
                }
            },
        }
    }
}

pub struct SshSession {
    handle: Handle<ClientHandler>,
    /// Jump host this connection is tunnelled through, kept alive for as
    /// long as this session exists.
    _via: Option<Arc<SshSession>>,
    remote_forwards: RemoteForwards,
    agent_forward: bool,
    /// Fake cookie and screen sent with X11 requests.
    x11: Option<([u8; 16], u32)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_status: Option<u32>,
    pub truncated: bool,
}

impl SshSession {
    pub async fn connect(
        target: &Target,
        auth: &Auth,
        opts: &ConnectOptions,
    ) -> Result<Self, SshError> {
        Self::connect_with(target, auth, opts, None).await
    }

    /// Connects to `target` through an `direct-tcpip` tunnel on an already
    /// authenticated jump host (OpenSSH `ProxyJump`). The target's host key
    /// and authentication are verified independently of the jump host's.
    pub async fn connect_via(
        jump: Arc<SshSession>,
        target: &Target,
        auth: &Auth,
        opts: &ConnectOptions,
    ) -> Result<Self, SshError> {
        Self::connect_with(target, auth, opts, Some(jump)).await
    }

    async fn connect_with(
        target: &Target,
        auth: &Auth,
        opts: &ConnectOptions,
        via: Option<Arc<SshSession>>,
    ) -> Result<Self, SshError> {
        match tokio::time::timeout(opts.timeout, Self::connect_inner(target, auth, opts, via)).await
        {
            Ok(r) => r,
            Err(_) => Err(SshError::Timeout {
                host: target.host.clone(),
                port: target.port,
            }),
        }
    }

    async fn connect_inner(
        target: &Target,
        auth: &Auth,
        opts: &ConnectOptions,
        via: Option<Arc<SshSession>>,
    ) -> Result<Self, SshError> {
        let config = Arc::new(client::Config {
            preferred: algorithms::preferred(),
            keepalive_interval: opts.keepalive,
            ..Default::default()
        });
        let refusal = Arc::new(Mutex::new(None));
        let remote_forwards: RemoteForwards = Arc::default();
        let x11_state = opts
            .x11
            .clone()
            .map(|f| Arc::new((f, crate::x11::fake_cookie())));
        let handler = ClientHandler {
            host: target.host.clone(),
            port: target.port,
            known_hosts: Arc::clone(&opts.known_hosts),
            decider: Arc::clone(&opts.decider),
            refusal: Arc::clone(&refusal),
            remote_forwards: Arc::clone(&remote_forwards),
            agent_forward: opts.agent_forward.clone(),
            x11: x11_state.clone(),
        };
        let connected = match &via {
            None => match &opts.proxy {
                None => client::connect(config, (target.host.as_str(), target.port), handler).await,
                Some(p) => {
                    let stream = crate::proxy::connect(p, &target.host, target.port).await?;
                    client::connect_stream(config, stream, handler).await
                }
            },
            Some(jump) => {
                let channel = jump
                    .handle
                    .channel_open_direct_tcpip(
                        target.host.as_str(),
                        u32::from(target.port),
                        "127.0.0.1",
                        0,
                    )
                    .await
                    .map_err(|e| {
                        SshError::Connect(format!(
                            "the jump host could not open a tunnel to {}:{}: {e}",
                            target.host, target.port
                        ))
                    })?;
                client::connect_stream(config, channel.into_stream(), handler).await
            }
        };
        let mut handle = match connected {
            Ok(h) => h,
            Err(e) => {
                let typed = refusal.lock().unwrap_or_else(|p| p.into_inner()).take();
                return Err(typed.unwrap_or_else(|| SshError::Connect(e.to_string())));
            }
        };

        let mut result = match auth {
            Auth::Password(pw) => {
                handle
                    .authenticate_password(target.user.as_str(), pw.as_str())
                    .await?
            }
            Auth::KeyFile { path, passphrase } => {
                let key = load_secret_key(path, passphrase.as_ref().map(|p| p.as_str()))
                    .map_err(|e| SshError::Key(e.to_string()))?;
                let hash = if key.algorithm().is_rsa() {
                    // Never fall back to SHA-1 signatures.
                    handle
                        .best_supported_rsa_hash()
                        .await?
                        .flatten()
                        .or(Some(HashAlg::Sha256))
                } else {
                    None
                };
                handle
                    .authenticate_publickey(
                        target.user.as_str(),
                        PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await?
            }
            Auth::Agent { socket } => {
                agent::authenticate(&mut handle, &target.user, socket).await?
            }
        };
        if kbd_follows(auth, &result) {
            let password = match auth {
                Auth::Password(pw) => Some(pw),
                _ => None,
            };
            let ok = crate::kbd::authenticate(
                &mut handle,
                &target.host,
                target.port,
                &target.user,
                password,
                opts.kbd.as_deref(),
            )
            .await?;
            if ok {
                result = client::AuthResult::Success;
            }
        }
        if !result.success() {
            return Err(SshError::AuthFailed);
        }
        Ok(Self {
            handle,
            _via: via,
            remote_forwards,
            agent_forward: opts.agent_forward.is_some(),
            x11: x11_state.map(|s| (s.1, s.0.screen)),
        })
    }

    /// Asks the server to listen on `bind_address:port` (0 = any free port)
    /// and forward connections to `local_host:local_port` on this machine
    /// (`ssh -R`). Returns the port the server listens on.
    pub async fn forward_remote(
        &self,
        bind_address: &str,
        port: u16,
        local_host: &str,
        local_port: u16,
    ) -> Result<u16, SshError> {
        let assigned = self
            .handle
            .tcpip_forward(bind_address, u32::from(port))
            .await
            .map_err(|e| {
                SshError::Connect(format!(
                    "the server refused to listen on {bind_address}:{port}: {e}"
                ))
            })?;
        let server_port = if port == 0 { assigned } else { u32::from(port) };
        self.remote_forwards
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(server_port, (local_host.to_owned(), local_port));
        u16::try_from(server_port)
            .map_err(|_| SshError::Protocol("server returned an invalid port".into()))
    }

    /// Stops a remote forward started with [`forward_remote`](Self::forward_remote).
    pub async fn cancel_remote_forward(
        &self,
        bind_address: &str,
        port: u16,
    ) -> Result<(), SshError> {
        self.remote_forwards
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&u32::from(port));
        self.handle
            .cancel_tcpip_forward(bind_address, u32::from(port))
            .await?;
        Ok(())
    }

    /// Runs a command and collects its output (bounded).
    pub async fn exec(&self, command: &str) -> Result<ExecOutput, SshError> {
        let mut ch = self.handle.channel_open_session().await?;
        if self.agent_forward {
            let _ = ch.agent_forward(false).await;
        }
        ch.exec(true, command).await?;
        let mut out = ExecOutput::default();
        while let Some(msg) = ch.wait().await {
            match msg {
                ChannelMsg::Data { data } => append(&mut out.stdout, &data, &mut out.truncated),
                ChannelMsg::ExtendedData { data, ext: 1 } => {
                    append(&mut out.stderr, &data, &mut out.truncated)
                }
                ChannelMsg::ExitStatus { exit_status } => out.exit_status = Some(exit_status),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Opens an interactive shell on a remote PTY.
    pub async fn open_shell(&self, cols: u32, rows: u32) -> Result<ShellChannel, SshError> {
        let ch = self.handle.channel_open_session().await?;
        ch.request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
            .await?;
        if self.agent_forward {
            // Best effort: a server that disallows forwarding still gets a shell.
            let _ = ch.agent_forward(false).await;
        }
        if let Some((fake, screen)) = &self.x11 {
            let _ = ch
                .request_x11(
                    false,
                    false,
                    "MIT-MAGIC-COOKIE-1",
                    crate::x11::to_hex(fake),
                    *screen,
                )
                .await;
        }
        ch.request_shell(true).await?;
        let (reader, writer) = ch.split();
        Ok(ShellChannel {
            reader: ShellReader { inner: reader },
            writer: ShellWriter { inner: writer },
        })
    }

    /// Opens a `direct-tcpip` channel from the server to `host:port` and
    /// returns it as a byte stream (used for port forwarding).
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        originator: std::net::SocketAddr,
    ) -> Result<russh::ChannelStream<Msg>, SshError> {
        let ch = self
            .handle
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                originator.ip().to_string(),
                u32::from(originator.port()),
            )
            .await
            .map_err(|e| {
                SshError::Connect(format!("server refused a connection to {host}:{port}: {e}"))
            })?;
        Ok(ch.into_stream())
    }

    /// Starts the SFTP subsystem on a new channel of this connection.
    pub async fn sftp(&self) -> Result<SftpClient, SshError> {
        let ch = self.handle.channel_open_session().await?;
        ch.request_subsystem(true, "sftp").await?;
        let session = SftpSession::new(ch.into_stream()).await?;
        Ok(SftpClient::new(session))
    }

    /// Starts SFTP as root through `sudo` on a new channel, for files the
    /// login user may not read or change. Without a password this works
    /// only with passwordless sudo ([`SshError::SudoPasswordRequired`]
    /// otherwise). The password is written once to sudo's standard input
    /// and never kept; `-k` makes sudo always read it, so it can never
    /// reach the SFTP server instead.
    pub async fn sftp_sudo(&self, password: Option<&str>) -> Result<SftpClient, SshError> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Passwordless sudo (NOPASSWD), ignoring any cached credentials.
        let passwordless = self
            .exec("sudo -n -k true")
            .await
            .is_ok_and(|o| o.exit_status == Some(0));
        let command = match (passwordless, password) {
            (true, _) => format!("sudo -n -- sh -c {}", shell_quote(SUDO_SFTP_SCRIPT)),
            (false, Some(_)) => format!(
                "sudo -k -S -p '' -- sh -c {}",
                shell_quote(SUDO_SFTP_SCRIPT)
            ),
            (false, None) => return Err(SshError::SudoPasswordRequired),
        };
        let ch = self.handle.channel_open_session().await?;
        ch.exec(true, command).await?;
        let mut stream = ch.into_stream();
        if !passwordless && let Some(pw) = password {
            let mut line = Zeroizing::new(Vec::with_capacity(pw.len() + 1));
            line.extend_from_slice(pw.as_bytes());
            line.push(b'\n');
            stream
                .write_all(&line)
                .await
                .map_err(|e| SshError::SudoRefused(e.to_string()))?;
            stream
                .flush()
                .await
                .map_err(|e| SshError::SudoRefused(e.to_string()))?;
        }
        // The script prints a marker right before it starts the SFTP server;
        // until then nothing else is read, so the SFTP stream starts clean.
        let marker = SUDO_SFTP_MARKER.as_bytes();
        let mut seen = Vec::with_capacity(marker.len());
        let waited = tokio::time::timeout(SUDO_TIMEOUT, async {
            let mut byte = [0u8; 1];
            while !seen.ends_with(marker) {
                match stream.read(&mut byte).await {
                    Ok(0) | Err(_) => return false,
                    Ok(_) => {
                        seen.push(byte[0]);
                        if seen.len() > 4096 {
                            return false;
                        }
                    }
                }
            }
            true
        })
        .await;
        match waited {
            Ok(true) => {}
            Ok(false) => {
                return Err(SshError::SudoRefused(
                    "wrong password, no sudo rights, or no SFTP server on the host".into(),
                ));
            }
            Err(_) => {
                return Err(SshError::SudoRefused(
                    "no answer from sudo (wrong password?)".into(),
                ));
            }
        }
        let session = SftpSession::new(stream).await?;
        Ok(SftpClient::new(session))
    }

    pub async fn disconnect(&self) -> Result<(), SshError> {
        self.handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await?;
        Ok(())
    }
}

/// Printed by [`SUDO_SFTP_SCRIPT`] right before the SFTP server starts.
const SUDO_SFTP_MARKER: &str = "\u{1}TERMALITH-SUDO-SFTP\u{1}";
/// How long sudo may take to accept the password and start the server.
const SUDO_TIMEOUT: Duration = Duration::from_secs(15);
/// Finds the SFTP server in the usual places (Debian, RHEL, Arch, BSD,
/// macOS) and starts it after printing the marker.
const SUDO_SFTP_SCRIPT: &str = "for p in /usr/lib/openssh/sftp-server /usr/libexec/openssh/sftp-server \
/usr/lib/ssh/sftp-server /usr/libexec/sftp-server /usr/lib/sftp-server /usr/local/libexec/sftp-server; \
do if [ -x \"$p\" ]; then printf '\\001TERMALITH-SUDO-SFTP\\001'; exec \"$p\"; fi; done; \
echo 'sftp-server not found' >&2; exit 127";

/// Single-quotes a string for a POSIX shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

mod agent {
    use std::path::Path;

    use russh::client::{AuthResult, Handle};
    use russh::keys::HashAlg;
    use russh::keys::agent::AgentIdentity;
    use russh::keys::agent::client::AgentClient;
    use tokio::io::{AsyncRead, AsyncWrite};

    use super::ClientHandler;
    use crate::SshError;

    /// A raw byte stream to the local agent, for forwarding.
    #[cfg(unix)]
    pub(super) async fn connect_raw(socket: &Path) -> std::io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(socket).await
    }

    #[cfg(windows)]
    pub(super) async fn connect_raw(
        socket: &Path,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        tokio::net::windows::named_pipe::ClientOptions::new().open(socket)
    }

    /// Tries each identity the agent offers until one is accepted.
    pub(super) async fn authenticate(
        handle: &mut Handle<ClientHandler>,
        user: &str,
        socket: &Path,
    ) -> Result<AuthResult, SshError> {
        #[cfg(unix)]
        let agent = AgentClient::connect_uds(socket).await;
        #[cfg(windows)]
        let agent = AgentClient::connect_named_pipe(socket).await;
        let agent = agent.map_err(|e| SshError::Agent(format!("cannot connect: {e}")))?;
        with_agent(handle, user, agent).await
    }

    async fn with_agent<R>(
        handle: &mut Handle<ClientHandler>,
        user: &str,
        mut agent: AgentClient<R>,
    ) -> Result<AuthResult, SshError>
    where
        R: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let identities = agent
            .request_identities()
            .await
            .map_err(|e| SshError::Agent(e.to_string()))?;
        let keys: Vec<_> = identities
            .iter()
            .filter_map(|i| match i {
                AgentIdentity::PublicKey { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect();
        if keys.is_empty() {
            return Err(SshError::Agent("the agent holds no keys".into()));
        }
        let mut last = None;
        for key in keys {
            let hash = if key.algorithm().is_rsa() {
                // Never SHA-1 signatures.
                handle
                    .best_supported_rsa_hash()
                    .await?
                    .flatten()
                    .or(Some(HashAlg::Sha256))
            } else {
                None
            };
            let r = handle
                .authenticate_publickey_with(user, key, hash, &mut agent)
                .await
                .map_err(|e| SshError::Agent(e.to_string()))?;
            if r.success() {
                return Ok(r);
            }
            last = Some(r);
        }
        Ok(last.expect("at least one key was tried"))
    }
}

fn append(buf: &mut Vec<u8>, data: &[u8], truncated: &mut bool) {
    let room = MAX_EXEC_OUTPUT.saturating_sub(buf.len());
    if data.len() > room {
        *truncated = true;
    }
    buf.extend_from_slice(&data[..data.len().min(room)]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Output(Vec<u8>),
    Exited(u32),
    Closed,
}

/// An interactive remote shell with a PTY. Use [`split`](Self::split) to
/// read and write from different tasks.
pub struct ShellChannel {
    reader: ShellReader,
    writer: ShellWriter,
}

impl ShellChannel {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.writer.write(data).await
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.writer.resize(cols, rows).await
    }

    /// Next event; `Closed` once the channel is gone.
    pub async fn next_event(&mut self) -> ShellEvent {
        self.reader.next_event().await
    }

    pub async fn close(&self) -> Result<(), SshError> {
        self.writer.close().await
    }

    pub fn split(self) -> (ShellReader, ShellWriter) {
        (self.reader, self.writer)
    }
}

pub struct ShellReader {
    inner: ChannelReadHalf,
}

impl ShellReader {
    /// Next event; `Closed` once the channel is gone.
    pub async fn next_event(&mut self) -> ShellEvent {
        loop {
            match self.inner.wait().await {
                None | Some(ChannelMsg::Close) => return ShellEvent::Closed,
                Some(ChannelMsg::Data { data }) => return ShellEvent::Output(data.to_vec()),
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    return ShellEvent::Output(data.to_vec());
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    return ShellEvent::Exited(exit_status);
                }
                Some(_) => {}
            }
        }
    }
}

pub struct ShellWriter {
    inner: ChannelWriteHalf<Msg>,
}

impl ShellWriter {
    pub async fn write(&self, data: &[u8]) -> Result<(), SshError> {
        self.inner.data(data).await?;
        Ok(())
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> Result<(), SshError> {
        self.inner.window_change(cols, rows, 0, 0).await?;
        Ok(())
    }

    pub async fn close(&self) -> Result<(), SshError> {
        self.inner.close().await?;
        Ok(())
    }
}

/// Whether to continue with keyboard-interactive after the first method:
/// a second factor was requested (partial success), or the server takes
/// passwords only through keyboard-interactive. A password the server
/// rejected outright is not retried this way.
fn kbd_follows(auth: &Auth, result: &client::AuthResult) -> bool {
    let client::AuthResult::Failure {
        remaining_methods,
        partial_success,
    } = result
    else {
        return false;
    };
    offers_kbd(result)
        && (*partial_success
            || (matches!(auth, Auth::Password(_))
                && !remaining_methods.contains(&russh::MethodKind::Password)))
}
