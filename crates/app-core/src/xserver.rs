//! A local X server for SSH X11 forwarding on Windows (M-014, first step).
//! Windows has no X server; when none is running, Termalith starts an
//! installed VcXsrv or Xming. These listen on TCP on every interface, so
//! instead of the common `-ac` (no access control) they get a random
//! per-run MIT cookie in an `-auth` file, and only Termalith, which holds
//! the cookie, can connect. The file is removed when Termalith quits.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::AppError;

/// Display :0 listens on TCP port 6000.
pub const DISPLAY_PORT: u16 = 6000;

/// An X server Termalith can start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XServerProgram {
    pub name: &'static str,
    pub path: PathBuf,
}

/// Where the supported X servers install themselves.
const CANDIDATES: &[(&str, &str)] = &[
    ("VcXsrv", r"%ProgramFiles%\VcXsrv\vcxsrv.exe"),
    ("VcXsrv", r"%ProgramFiles(x86)%\VcXsrv\vcxsrv.exe"),
    ("Xming", r"%ProgramFiles(x86)%\Xming\Xming.exe"),
    ("Xming", r"%ProgramFiles%\Xming\Xming.exe"),
];

/// Replaces `%NAME%` with the variable's value; `None` when one is unset.
fn expand(location: &str, var: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = location;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        out.push_str(&var(&after[..end]).filter(|v| !v.is_empty())?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The first installed X server, in order of preference.
pub fn find_program(
    var: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<XServerProgram> {
    CANDIDATES.iter().find_map(|(name, location)| {
        let path = PathBuf::from(expand(location, &var)?);
        exists(&path).then_some(XServerProgram { name, path })
    })
}

/// An Xauthority file with one wildcard entry (any host) for display `:0`.
pub fn xauthority(cookie: &[u8]) -> Vec<u8> {
    fn field(out: &mut Vec<u8>, data: &[u8]) {
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&0xffffu16.to_be_bytes()); // FamilyWild
    field(&mut out, b"");
    field(&mut out, b"0");
    field(&mut out, b"MIT-MAGIC-COOKIE-1");
    field(&mut out, cookie);
    out
}

/// The arguments for display :0 with the given auth file: one window per X
/// application, shared clipboard.
pub fn arguments(program: &XServerProgram, auth: &Path) -> Vec<String> {
    let mut args = vec![
        ":0".to_owned(),
        "-multiwindow".to_owned(),
        "-clipboard".to_owned(),
    ];
    if program.name == "VcXsrv" {
        args.push("-wgl".to_owned());
    }
    args.push("-auth".to_owned());
    args.push(auth.display().to_string());
    args
}

/// Whether something accepts connections on display :0.
pub fn display_open() -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], DISPLAY_PORT)),
        Duration::from_millis(300),
    )
    .is_ok()
}

struct Started {
    child: Child,
    cookie: Vec<u8>,
    auth: PathBuf,
}

/// The one X server Termalith may start in this process.
pub static LOCAL_X_SERVER: std::sync::LazyLock<LocalXServer> =
    std::sync::LazyLock::new(LocalXServer::default);

/// The X server Termalith started, if any; stopped when dropped.
#[derive(Default)]
pub struct LocalXServer {
    started: Mutex<Option<Started>>,
}

/// What X11 forwarding should connect to on Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XTarget {
    /// The user's own X server on :0 (Termalith knows no cookie).
    Running,
    /// The one Termalith started, with its cookie.
    Started { cookie: Vec<u8> },
}

impl LocalXServer {
    /// Uses an X server on :0, or starts an installed one (blocking: call
    /// off the async runtime). `dir` holds the auth file.
    pub fn ensure(&self, dir: &Path) -> Result<XTarget, AppError> {
        let mut started = self.started.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = started.as_mut() {
            if matches!(s.child.try_wait(), Ok(None)) {
                return Ok(XTarget::Started {
                    cookie: s.cookie.clone(),
                });
            }
            let _ = std::fs::remove_file(&s.auth);
            *started = None;
        }
        if display_open() {
            return Ok(XTarget::Running);
        }
        let program = find_program(|v| std::env::var(v).ok(), Path::exists).ok_or_else(|| {
            AppError::invalid(
                "X11 forwarding",
                "no X server is running and none is installed; install VcXsrv (free) or X410 and try again",
            )
        })?;
        let mut cookie = vec![0u8; 16];
        getrandom::fill(&mut cookie).map_err(|e| AppError::Db(e.to_string()))?;
        std::fs::create_dir_all(dir).map_err(|e| AppError::Db(e.to_string()))?;
        let auth = dir.join("termalith-x11.auth");
        std::fs::write(&auth, xauthority(&cookie)).map_err(|e| AppError::Db(e.to_string()))?;
        let child = Command::new(&program.path)
            .args(arguments(&program, &auth))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                let _ = std::fs::remove_file(&auth);
                AppError::invalid(
                    "X11 forwarding",
                    format!("could not start {}: {e}", program.name),
                )
            })?;
        let mut s = Started {
            child,
            cookie,
            auth,
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !display_open() {
            if Instant::now() > deadline || !matches!(s.child.try_wait(), Ok(None)) {
                let _ = s.child.kill();
                let _ = std::fs::remove_file(&s.auth);
                return Err(AppError::invalid(
                    "X11 forwarding",
                    format!("{} did not start on display :0", program.name),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let target = XTarget::Started {
            cookie: s.cookie.clone(),
        };
        *started = Some(s);
        Ok(target)
    }

    /// Stops the X server Termalith started.
    pub fn stop(&self) {
        if let Some(mut s) = self
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_file(&s.auth);
        }
    }
}

impl Drop for LocalXServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_installed_server() {
        let var = |v: &str| match v {
            "ProgramFiles" => Some(r"C:\Program Files".to_owned()),
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
            _ => None,
        };
        let only_xming = |p: &Path| {
            p.to_string_lossy().ends_with(r"Xming\Xming.exe")
                && p.to_string_lossy().contains("(x86)")
        };
        let found = find_program(var, only_xming).unwrap();
        assert_eq!(found.name, "Xming");
        assert_eq!(
            found.path,
            PathBuf::from(r"C:\Program Files (x86)\Xming\Xming.exe")
        );
        let both = |_: &Path| true;
        assert_eq!(find_program(var, both).unwrap().name, "VcXsrv");
        assert!(find_program(var, |_| false).is_none());
        assert!(
            find_program(|_| None, both).is_none(),
            "unset variables match nothing"
        );
    }

    #[test]
    fn xauthority_has_one_wildcard_cookie_entry() {
        let cookie = [7u8; 16];
        let bytes = xauthority(&cookie);
        assert_eq!(&bytes[..2], &[0xff, 0xff]);
        assert_eq!(&bytes[2..4], &[0, 0]); // empty address
        assert_eq!(&bytes[4..7], &[0, 1, b'0']); // display number
        assert_eq!(&bytes[7..9], &[0, 18]);
        assert_eq!(&bytes[9..27], b"MIT-MAGIC-COOKIE-1");
        assert_eq!(&bytes[27..29], &[0, 16]);
        assert_eq!(&bytes[29..], &cookie);
    }

    #[test]
    fn servers_start_with_access_control() {
        let p = XServerProgram {
            name: "VcXsrv",
            path: PathBuf::from("vcxsrv.exe"),
        };
        let args = arguments(&p, Path::new("auth"));
        assert!(
            !args.iter().any(|a| a == "-ac"),
            "never without access control"
        );
        assert!(args.windows(2).any(|w| w == ["-auth", "auth"]));
        assert_eq!(args[0], ":0");
    }
}
