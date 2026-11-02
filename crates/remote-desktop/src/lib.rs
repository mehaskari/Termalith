//! Remote desktop through external clients (ADR-005 hybrid strategy,
//! "external" mode). The app builds a launch plan for an installed client
//! and starts it with an argument list (never a shell).
//!
//! Secrets are never put in arguments, URLs or files. Where a client can
//! read a password from stdin (FreeRDP `/from-stdin`) it is written there;
//! otherwise the client asks the user itself.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use zeroize::Zeroizing;

mod rdp_cert;
pub use rdp_cert::{CertFingerprint, read_certificate};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DesktopError {
    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("no {0} client found; install one (see the session's help text)")]
    NoClient(&'static str),
    #[error("could not start {client}: {reason}")]
    Launch { client: String, reason: String },
    #[error("could not check the server certificate: {0}")]
    Certificate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Rdp,
    Vnc,
}

impl Protocol {
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::Rdp => 3389,
            Protocol::Vnc => 5900,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Protocol::Rdp => "RDP",
            Protocol::Vnc => "VNC",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTarget {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub domain: Option<String>,
}

fn no_control(field: &'static str, v: &str) -> Result<(), DesktopError> {
    if v.chars().any(char::is_control) {
        return Err(DesktopError::Invalid {
            field,
            reason: "contains control characters".into(),
        });
    }
    Ok(())
}

impl DesktopTarget {
    pub fn validate(&self) -> Result<(), DesktopError> {
        let h = self.host.trim();
        if h.is_empty()
            || h.len() > 253
            || h.starts_with('-')
            || h.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(DesktopError::Invalid {
                field: "host",
                reason: "not a valid host name or address".into(),
            });
        }
        if self.port == 0 {
            return Err(DesktopError::Invalid {
                field: "port",
                reason: "must be 1-65535".into(),
            });
        }
        for (field, v) in [("username", &self.username), ("domain", &self.domain)] {
            if let Some(v) = v {
                no_control(field, v)?;
                if v.starts_with('-') || v.starts_with('/') {
                    return Err(DesktopError::Invalid {
                        field,
                        reason: "must not start with '-' or '/'".into(),
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// An installed client program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub id: &'static str,
    pub name: &'static str,
    pub protocol: Protocol,
    #[serde(skip)]
    pub program: PathBuf,
}

impl Client {
    /// Whether launching this client needs the password up front (it is
    /// written to the client's stdin). Other clients ask for it themselves.
    pub fn reads_password(&self) -> bool {
        self.id == "xfreerdp"
    }

    /// Whether Termalith must check the server certificate before launch
    /// (the client cannot show its own certificate prompt).
    pub fn needs_certificate(&self) -> bool {
        self.id == "xfreerdp"
    }
}

struct Candidate {
    id: &'static str,
    name: &'static str,
    protocol: Protocol,
    /// Absolute paths (app bundles / system tools) or bare names looked up
    /// on PATH.
    locations: &'static [&'static str],
}

const CANDIDATES: &[(Os, Candidate)] = &[
    (
        Os::MacOs,
        Candidate {
            id: "windows-app",
            name: "Windows App",
            protocol: Protocol::Rdp,
            locations: &["/Applications/Windows App.app"],
        },
    ),
    (
        Os::MacOs,
        Candidate {
            id: "ms-rdp",
            name: "Microsoft Remote Desktop",
            protocol: Protocol::Rdp,
            locations: &["/Applications/Microsoft Remote Desktop.app"],
        },
    ),
    (
        Os::MacOs,
        Candidate {
            id: "screen-sharing",
            name: "Screen Sharing",
            protocol: Protocol::Vnc,
            locations: &[
                "/System/Library/CoreServices/Applications/Screen Sharing.app",
                "/System/Applications/Utilities/Screen Sharing.app",
            ],
        },
    ),
    (
        Os::Windows,
        Candidate {
            id: "mstsc",
            name: "Remote Desktop Connection",
            protocol: Protocol::Rdp,
            locations: &[r"C:\Windows\System32\mstsc.exe"],
        },
    ),
    (
        Os::Windows,
        Candidate {
            id: "tigervnc",
            name: "TigerVNC Viewer",
            protocol: Protocol::Vnc,
            locations: &["vncviewer.exe", r"C:\Program Files\TigerVNC\vncviewer.exe"],
        },
    ),
    (
        Os::Linux,
        Candidate {
            id: "xfreerdp",
            name: "FreeRDP",
            protocol: Protocol::Rdp,
            locations: &["xfreerdp3", "xfreerdp", "sdl-freerdp3", "sdl-freerdp"],
        },
    ),
    (
        Os::Linux,
        Candidate {
            id: "vncviewer",
            name: "VNC Viewer",
            protocol: Protocol::Vnc,
            locations: &["vncviewer", "xtigervncviewer"],
        },
    ),
];

fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Clients installed on this machine for `protocol`, preferred first.
pub fn available_clients(protocol: Protocol) -> Vec<Client> {
    let os = Os::current();
    CANDIDATES
        .iter()
        .filter(|(o, c)| *o == os && c.protocol == protocol)
        .filter_map(|(_, c)| {
            let program = c.locations.iter().find_map(|l| {
                let p = Path::new(l);
                if p.is_absolute() {
                    p.exists().then(|| p.to_path_buf())
                } else {
                    find_on_path(l)
                }
            })?;
            Some(Client {
                id: c.id,
                name: c.name,
                protocol: c.protocol,
                program,
            })
        })
        .collect()
}

/// Exactly what will be run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// A password (if available) is written to the child's stdin.
    pub password_via_stdin: bool,
    /// A connection file to write before launching (no secrets in it).
    pub file: Option<(PathBuf, String)>,
    /// Whether the child process lives as long as the session (so its exit
    /// means the session ended). `open` on macOS hands off and exits.
    pub tracks_session: bool,
}

fn rdp_file(t: &DesktopTarget) -> String {
    // .rdp settings are one per line; validation rejected control chars.
    let mut s = format!(
        "full address:s:{}:{}\r\nprompt for credentials:i:1\r\n",
        t.host.trim(),
        t.port
    );
    if let Some(u) = &t.username {
        s.push_str(&format!("username:s:{u}\r\n"));
    }
    if let Some(d) = &t.domain {
        s.push_str(&format!("domain:s:{d}\r\n"));
    }
    s
}

/// Builds the launch plan for `client` on `os`. `temp_dir` holds the
/// connection file for clients that take one. `cert` is the server
/// certificate the user trusts, for clients that cannot ask themselves
/// (see [`Client::needs_certificate`]).
pub fn plan_for(
    os: Os,
    client: &Client,
    t: &DesktopTarget,
    temp_dir: &Path,
    cert: Option<CertFingerprint>,
) -> Result<LaunchPlan, DesktopError> {
    t.validate()?;
    let host = t.host.trim();
    let hostport = format!("{host}:{}", t.port);
    Ok(match (os, client.id) {
        (Os::MacOs, "windows-app" | "ms-rdp") => {
            let file = temp_dir.join(format!("termalith-{}-{}.rdp", sanitize(host), t.port));
            LaunchPlan {
                program: "/usr/bin/open".into(),
                args: vec![
                    "-a".into(),
                    client.program.to_string_lossy().into_owned(),
                    file.to_string_lossy().into_owned(),
                ],
                password_via_stdin: false,
                file: Some((file, rdp_file(t))),
                tracks_session: false,
            }
        }
        (Os::MacOs, "screen-sharing") => LaunchPlan {
            program: "/usr/bin/open".into(),
            // No credentials in the URL; Screen Sharing asks for them.
            args: vec![format!("vnc://{hostport}")],
            password_via_stdin: false,
            file: None,
            tracks_session: false,
        },
        (Os::Windows, "mstsc") => LaunchPlan {
            program: client.program.clone(),
            args: vec![format!("/v:{hostport}")],
            password_via_stdin: false,
            file: None,
            tracks_session: true,
        },
        (Os::Linux, "xfreerdp") => {
            let mut args = vec![format!("/v:{hostport}")];
            if let Some(u) = &t.username {
                args.push(format!("/u:{u}"));
            }
            if let Some(d) = &t.domain {
                args.push(format!("/d:{d}"));
            }
            // Password on stdin, never /p:. FreeRDP's own certificate
            // prompt needs a terminal, so Termalith checks the certificate
            // first and FreeRDP accepts exactly that one; anything else is
            // refused without asking. Both hex forms are listed because
            // FreeRDP versions compare with and without colons.
            args.push("/from-stdin:force".into());
            args.push(match cert {
                Some(fp) => format!(
                    "/cert:deny,fingerprint:sha256:{},fingerprint:sha256:{}",
                    fp.display(),
                    fp.hex()
                ),
                None => "/cert:deny".into(),
            });
            LaunchPlan {
                program: client.program.clone(),
                args,
                password_via_stdin: true,
                file: None,
                tracks_session: true,
            }
        }
        (_, "vncviewer" | "tigervnc") => LaunchPlan {
            program: client.program.clone(),
            args: vec![format!("{host}::{}", t.port)],
            password_via_stdin: false,
            file: None,
            tracks_session: true,
        },
        _ => return Err(DesktopError::NoClient(t.protocol.name())),
    })
}

fn sanitize(host: &str) -> String {
    host.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A started client.
pub struct Launched {
    pub child: Option<tokio::process::Child>,
    pub tracks_session: bool,
}

/// Starts the plan. The connection file (if any) is written first and
/// contains no secrets.
pub async fn launch(
    plan: &LaunchPlan,
    password: Option<Zeroizing<String>>,
    client_name: &str,
) -> Result<Launched, DesktopError> {
    let err = |reason: String| DesktopError::Launch {
        client: client_name.to_owned(),
        reason,
    };
    if let Some((path, contents)) = &plan.file {
        std::fs::write(path, contents).map_err(|e| err(e.to_string()))?;
    }
    let mut cmd = tokio::process::Command::new(&plan.program);
    cmd.args(&plan.args)
        .stdin(if plan.password_via_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| err(e.to_string()))?;
    if plan.password_via_stdin
        && let Some(mut stdin) = child.stdin.take()
    {
        let pw = password.unwrap_or_default();
        let mut line = Zeroizing::new(pw.as_bytes().to_vec());
        line.push(b'\n');
        stdin
            .write_all(&line)
            .await
            .map_err(|e| err(e.to_string()))?;
        drop(stdin); // EOF
    }
    Ok(Launched {
        child: Some(child),
        tracks_session: plan.tracks_session,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(p: Protocol) -> DesktopTarget {
        DesktopTarget {
            protocol: p,
            host: "srv.example.com".into(),
            port: p.default_port(),
            username: Some("admin".into()),
            domain: Some("CORP".into()),
        }
    }

    fn client(id: &'static str, p: Protocol) -> Client {
        Client {
            id,
            name: id,
            protocol: p,
            program: PathBuf::from(format!("/opt/{id}")),
        }
    }

    #[test]
    fn validation_blocks_option_injection() {
        let mut t = target(Protocol::Rdp);
        assert!(t.validate().is_ok());
        t.host = "-v:evil".into();
        assert!(t.validate().is_err());
        t = target(Protocol::Rdp);
        t.username = Some("/p:secret".into());
        assert!(t.validate().is_err());
        t = target(Protocol::Rdp);
        t.username = Some("a\nfull address:s:evil".into());
        assert!(t.validate().is_err(), "newlines could inject .rdp settings");
    }

    #[test]
    fn freerdp_takes_password_on_stdin_never_in_args() {
        let p = plan_for(
            Os::Linux,
            &client("xfreerdp", Protocol::Rdp),
            &target(Protocol::Rdp),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        assert!(p.password_via_stdin);
        assert!(p.args.contains(&"/from-stdin:force".to_string()));
        assert!(p.args.iter().all(|a| !a.starts_with("/p:")));
        assert_eq!(p.args[0], "/v:srv.example.com:3389");
        assert!(p.tracks_session);
        assert!(p.args.contains(&"/cert:deny".to_string()));
        assert!(!p.args.iter().any(|a| a.contains("tofu")));
    }

    #[test]
    fn freerdp_accepts_only_the_trusted_certificate() {
        let fp = CertFingerprint::of(b"cert");
        let p = plan_for(
            Os::Linux,
            &client("xfreerdp", Protocol::Rdp),
            &target(Protocol::Rdp),
            Path::new("/tmp"),
            Some(fp),
        )
        .unwrap();
        let cert = p.args.iter().find(|a| a.starts_with("/cert:")).unwrap();
        assert_eq!(
            cert,
            &format!(
                "/cert:deny,fingerprint:sha256:{},fingerprint:sha256:{}",
                fp.display(),
                fp.hex()
            )
        );
    }

    #[test]
    fn macos_rdp_uses_a_secret_free_connection_file() {
        let p = plan_for(
            Os::MacOs,
            &client("ms-rdp", Protocol::Rdp),
            &target(Protocol::Rdp),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        let (path, body) = p.file.unwrap();
        assert!(path.to_string_lossy().ends_with(".rdp"));
        assert!(body.contains("full address:s:srv.example.com:3389"));
        assert!(body.contains("username:s:admin"));
        assert!(!body.to_lowercase().contains("password"));
        assert!(!p.tracks_session);
    }

    #[test]
    fn vnc_urls_and_args_carry_no_credentials() {
        let p = plan_for(
            Os::MacOs,
            &client("screen-sharing", Protocol::Vnc),
            &target(Protocol::Vnc),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        assert_eq!(p.args, vec!["vnc://srv.example.com:5900".to_string()]);
        let l = plan_for(
            Os::Linux,
            &client("vncviewer", Protocol::Vnc),
            &target(Protocol::Vnc),
            Path::new("/tmp"),
            None,
        )
        .unwrap();
        assert_eq!(l.args, vec!["srv.example.com::5900".to_string()]);
    }

    #[test]
    fn windows_mstsc() {
        let p = plan_for(
            Os::Windows,
            &client("mstsc", Protocol::Rdp),
            &target(Protocol::Rdp),
            Path::new("C:/tmp"),
            None,
        )
        .unwrap();
        assert_eq!(p.args, vec!["/v:srv.example.com:3389".to_string()]);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn launch_writes_password_to_stdin_and_file_first() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("got");
        let plan = LaunchPlan {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!("read p; printf '%s' \"$p\" > '{}'", out.display()),
            ],
            password_via_stdin: true,
            file: Some((dir.path().join("c.rdp"), "full address:s:x:1\r\n".into())),
            tracks_session: true,
        };
        let mut l = launch(&plan, Some(Zeroizing::new("s3cret".into())), "test")
            .await
            .unwrap();
        l.child.as_mut().unwrap().wait().await.unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "s3cret");
        assert!(dir.path().join("c.rdp").exists());
    }
}
