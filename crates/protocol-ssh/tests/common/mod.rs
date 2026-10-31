//! Test harness: an unprivileged OpenSSH `sshd` on a random localhost port,
//! with throwaway host and user keys. It can only authenticate the user
//! running the tests, by public key.
//!
//! If `sshd` is not available the tests are skipped, unless
//! `TERMALITH_REQUIRE_SSHD=1` is set (CI on macOS/Linux), which turns a
//! missing or failing server into a test failure.
#![allow(dead_code)]

use std::fmt::Write as _;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use protocol_ssh::Target;

pub struct TestSshd {
    pub dir: tempfile::TempDir,
    pub port: u16,
    pub user: String,
    /// Public host key in OpenSSH format.
    pub host_pub: String,
    pub user_key: PathBuf,
    child: Child,
}

#[derive(Default, Clone)]
pub struct SshdConfig {
    /// `ssh-keygen -t` value for the host key.
    pub host_key_type: Option<&'static str>,
    /// Extra `sshd_config` lines.
    pub extra: Vec<String>,
}

fn sshd_binary() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &[r"C:\Windows\System32\OpenSSH\sshd.exe"]
    } else {
        &[
            "/usr/sbin/sshd",
            "/usr/local/sbin/sshd",
            "/opt/homebrew/sbin/sshd",
        ]
    };
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

fn required() -> bool {
    std::env::var("TERMALITH_REQUIRE_SSHD").as_deref() == Ok("1")
}

/// Starts a server or explains why not. Returns `None` to skip the test.
pub fn start(cfg: SshdConfig) -> Option<TestSshd> {
    match try_start(&cfg) {
        Ok(s) => Some(s),
        Err(why) if required() => panic!("TERMALITH_REQUIRE_SSHD=1 but sshd failed: {why}"),
        Err(why) => {
            eprintln!("SKIPPED: no usable sshd ({why})");
            None
        }
    }
}

pub fn keygen(path: &Path, kind: &str, passphrase: &str) -> Result<(), String> {
    let mut cmd = Command::new("ssh-keygen");
    cmd.args(["-q", "-t", kind, "-N", passphrase, "-f"])
        .arg(path)
        .stdin(Stdio::null());
    if kind == "rsa" {
        cmd.args(["-b", "3072"]);
    }
    let out = cmd.output().map_err(|e| format!("ssh-keygen: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .expect("USER")
}

fn try_start(cfg: &SshdConfig) -> Result<TestSshd, String> {
    let sshd = sshd_binary().ok_or("sshd binary not found")?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let d = dir.path();
    let host_key = d.join("host_key");
    keygen(&host_key, cfg.host_key_type.unwrap_or("ed25519"), "")?;
    let user_key = d.join("user_key");
    keygen(&user_key, "ed25519", "")?;
    std::fs::copy(d.join("user_key.pub"), d.join("authorized_keys")).map_err(|e| e.to_string())?;
    let host_pub = std::fs::read_to_string(d.join("host_key.pub")).map_err(|e| e.to_string())?;

    let mut last_err = String::new();
    for _attempt in 0..3 {
        let port = free_port();
        let mut conf = String::new();
        let _ = writeln!(conf, "Port {port}");
        let _ = writeln!(conf, "ListenAddress 127.0.0.1");
        let _ = writeln!(conf, "HostKey {}", host_key.display());
        let _ = writeln!(conf, "PidFile {}", d.join("sshd.pid").display());
        let _ = writeln!(
            conf,
            "AuthorizedKeysFile {}",
            d.join("authorized_keys").display()
        );
        let _ = writeln!(conf, "StrictModes no");
        if !cfg!(windows) {
            let _ = writeln!(conf, "UsePAM no");
        }
        let _ = writeln!(conf, "PasswordAuthentication no");
        let _ = writeln!(conf, "KbdInteractiveAuthentication no");
        let _ = writeln!(conf, "PubkeyAuthentication yes");
        let _ = writeln!(conf, "Subsystem sftp internal-sftp");
        let _ = writeln!(conf, "LogLevel VERBOSE");
        for line in &cfg.extra {
            let _ = writeln!(conf, "{line}");
        }
        let conf_path = d.join("sshd_config");
        std::fs::write(&conf_path, conf).map_err(|e| e.to_string())?;
        let log = std::fs::File::create(d.join("sshd.log")).map_err(|e| e.to_string())?;

        let mut child = Command::new(&sshd)
            .args(["-D", "-e", "-f"])
            .arg(&conf_path)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("spawn sshd: {e}"))?;

        if wait_for_banner(port, Duration::from_secs(10)) {
            return Ok(TestSshd {
                dir,
                port,
                user: current_user(),
                host_pub,
                user_key,
                child,
            });
        }
        let _ = child.kill();
        let _ = child.wait();
        last_err = format!(
            "sshd did not start on port {port}: {}",
            std::fs::read_to_string(d.join("sshd.log")).unwrap_or_default()
        );
    }
    Err(last_err)
}

fn wait_for_banner(port: u16, timeout: Duration) -> bool {
    use std::io::Read;
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 8];
            if s.read_exact(&mut buf).is_ok() && &buf == b"SSH-2.0-" {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

impl TestSshd {
    pub fn target(&self) -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: self.port,
            user: self.user.clone(),
        }
    }

    pub fn host_key(&self) -> russh::keys::PublicKey {
        russh::keys::PublicKey::from_openssh(self.host_pub.trim()).expect("host key")
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Remote path string for a file in the server's scratch dir. The server
    /// runs as the current user on this machine, so local == remote.
    pub fn remote(&self, name: &str) -> String {
        self.path(name).to_string_lossy().replace('\\', "/")
    }

    pub fn log(&self) -> String {
        std::fs::read_to_string(self.path("sshd.log")).unwrap_or_default()
    }

    /// Authorizes an additional user public key file.
    pub fn authorize(&self, pub_path: &Path) {
        let mut keys = std::fs::read_to_string(self.path("authorized_keys")).unwrap();
        keys.push_str(&std::fs::read_to_string(pub_path).unwrap());
        std::fs::write(self.path("authorized_keys"), keys).unwrap();
    }
}

impl Drop for TestSshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
