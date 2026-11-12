//! Mosh sessions through the installed `mosh` client (ADR-034). Mosh
//! libraries in Rust are GPL-3.0, so Termalith runs the client as a separate
//! program in a terminal tab. It signs in with the system's OpenSSH and
//! uses its known_hosts; Termalith's saved passwords are not used.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use terminal_pty::ShellSpec;

use crate::AppError;
use crate::sessions::{AuthMethod, SessionInput, SessionProtocol};

/// Where `mosh` usually is when a GUI app's PATH is short (macOS apps do
/// not get the shell's PATH).
const EXTRA_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/snap/bin",
];

/// The installed `mosh` client, if any.
pub fn find_mosh() -> Option<PathBuf> {
    let name = if cfg!(windows) { "mosh.exe" } else { "mosh" };
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(EXTRA_DIRS.iter().map(PathBuf::from))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// One word of mosh's `--ssh` option, which mosh splits like a shell does.
fn ssh_word(w: &str) -> Result<String, AppError> {
    if w.contains(['\'', '\0', '\n', '\r']) {
        return Err(AppError::invalid(
            "Mosh",
            "the key path contains a quote or line break",
        ));
    }
    Ok(
        if w.is_empty() || w.contains(|c: char| c.is_whitespace() || "\"\\$`".contains(c)) {
            format!("'{w}'")
        } else {
            w.to_owned()
        },
    )
}

/// The command line for a Mosh session: the program and its arguments,
/// never through a shell.
pub fn mosh_command(input: &SessionInput, mosh: &Path) -> Result<ShellSpec, AppError> {
    input.validate()?;
    if input.protocol != SessionProtocol::Ssh || !input.mosh {
        return Err(AppError::invalid("session", "not a Mosh session"));
    }
    let mut ssh = vec!["ssh".to_owned(), "-p".to_owned(), input.port.to_string()];
    if let AuthMethod::KeyFile { path } = &input.auth {
        ssh.push("-i".to_owned());
        ssh.push(ssh_word(&path.to_string_lossy())?);
    }
    let target = if input.username.is_empty() {
        input.host.trim().to_owned()
    } else {
        format!("{}@{}", input.username, input.host.trim())
    };
    let args: Vec<OsString> = vec![
        format!("--ssh={}", ssh.join(" ")).into(),
        // Nothing after this is read as an option.
        "--".into(),
        target.into(),
    ];
    Ok(args
        .into_iter()
        .fold(ShellSpec::new(mosh), ShellSpec::arg)
        .with_terminal_env())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionInput {
        SessionInput {
            name: "edge".into(),
            host: "edge.example.com".into(),
            port: 2222,
            username: "ops".into(),
            auth: AuthMethod::Password,
            folder: None,
            jump_via: None,
            protocol: SessionProtocol::Ssh,
            domain: None,
            tags: Vec::new(),
            forward_agent: false,
            x11_forward: false,
            startup_command: None,
            files_only: false,
            mosh: true,
            appearance: None,
            icon: None,
        }
    }

    fn args(spec: &ShellSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn builds_the_command_without_a_shell() {
        let spec = mosh_command(&session(), Path::new("/usr/bin/mosh")).unwrap();
        assert_eq!(spec.program, Path::new("/usr/bin/mosh"));
        assert_eq!(
            args(&spec),
            ["--ssh=ssh -p 2222", "--", "ops@edge.example.com"]
        );
        assert!(spec.env.iter().any(|(k, _)| k == "TERM"));
    }

    #[test]
    fn key_paths_are_quoted_for_mosh() {
        // An absolute path on every system, with a space in it.
        let key = std::env::temp_dir().join("My Keys").join("id_ed25519");
        let mut s = session();
        s.auth = AuthMethod::KeyFile { path: key.clone() };
        let spec = mosh_command(&s, Path::new("mosh")).unwrap();
        assert_eq!(
            args(&spec)[0],
            format!("--ssh=ssh -p 2222 -i '{}'", key.display())
        );
        s.auth = AuthMethod::KeyFile {
            path: std::env::temp_dir().join("it's"),
        };
        assert!(mosh_command(&s, Path::new("mosh")).is_err());
    }

    #[test]
    fn only_plain_ssh_sessions() {
        let mut s = session();
        s.mosh = false;
        assert!(mosh_command(&s, Path::new("mosh")).is_err());
        let mut s = session();
        s.x11_forward = true;
        assert!(mosh_command(&s, Path::new("mosh")).is_err(), "validated");
        let mut s = session();
        s.protocol = SessionProtocol::Telnet;
        s.auth = AuthMethod::Password;
        assert!(mosh_command(&s, Path::new("mosh")).is_err());
    }
}
