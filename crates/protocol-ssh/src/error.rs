/// Errors from the SSH adapter. Messages never contain passwords, key
/// material or session data.
#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("connection to {host}:{port} timed out")]
    Timeout { host: String, port: u16 },

    #[error("could not connect: {0}")]
    Connect(String),

    #[error("host key for {host}:{port} was not trusted ({fingerprint})")]
    HostKeyRejected {
        host: String,
        port: u16,
        fingerprint: String,
    },

    /// The server presented a key that differs from every recorded key for
    /// this host. Possible man-in-the-middle; the caller must not offer a
    /// one-click "accept" here.
    #[error("HOST KEY CHANGED for {host}:{port}: got {presented}, known {known:?}")]
    HostKeyChanged {
        host: String,
        port: u16,
        presented: String,
        known: Vec<String>,
    },

    #[error("server presented a host certificate, which is not supported yet")]
    HostCertificateUnsupported,

    #[error("authentication failed")]
    AuthFailed,

    #[error("could not load private key: {0}")]
    Key(String),

    #[error("ssh-agent: {0}")]
    Agent(String),

    #[error("known-hosts store error: {0}")]
    KnownHosts(String),

    #[error("SSH protocol error: {0}")]
    Protocol(String),

    #[error("SFTP error: {0}")]
    Sftp(String),

    #[error("destination already exists: {0}")]
    DestinationExists(String),

    #[error("transfer cancelled")]
    Cancelled,

    #[error("file is larger than {limit} bytes")]
    TooLarge { limit: u64 },

    /// The remote file changed since it was read (conflict on save).
    #[error("{0} was changed on the server since it was opened")]
    Modified(String),

    #[error("{0} is not a regular file")]
    NotAFile(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> Self {
        SshError::Protocol(e.to_string())
    }
}

impl From<russh_sftp::client::error::Error> for SshError {
    fn from(e: russh_sftp::client::error::Error) -> Self {
        SshError::Sftp(e.to_string())
    }
}
