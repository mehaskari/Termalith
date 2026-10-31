//! Integration tests against a real OpenSSH server (see `common`).

use test_sshd as common;

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{SshdConfig, TestSshd};

fn target(s: &TestSshd) -> Target {
    Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: s.user.clone(),
    }
}

fn host_key(s: &TestSshd) -> russh::keys::PublicKey {
    russh::keys::PublicKey::from_openssh(s.host_pub.trim()).expect("host key")
}
use protocol_ssh::{
    Auth, CancellationToken, ConnectOptions, FileKnownHosts, HostKeyDecider, HostKeyDecision,
    HostKeyInfo, KnownHostsStore, RejectUnknown, ShellEvent, SshError, SshSession, Target,
    TransferOptions,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Decider that returns a fixed answer and counts how often it was asked.
struct Fixed {
    answer: HostKeyDecision,
    calls: AtomicUsize,
    last_fingerprint: std::sync::Mutex<Option<String>>,
}

impl Fixed {
    fn new(answer: HostKeyDecision) -> Arc<Self> {
        Arc::new(Self {
            answer,
            calls: AtomicUsize::new(0),
            last_fingerprint: Default::default(),
        })
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl HostKeyDecider for Fixed {
    fn decide<'a>(
        &'a self,
        info: &'a HostKeyInfo,
    ) -> Pin<Box<dyn Future<Output = HostKeyDecision> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.last_fingerprint.lock().unwrap() = Some(info.fingerprint.clone());
        Box::pin(async move { self.answer })
    }
}

fn key_auth(s: &TestSshd) -> Auth {
    Auth::KeyFile {
        path: s.user_key.clone(),
        passphrase: None,
    }
}

fn known_hosts(s: &TestSshd) -> Arc<FileKnownHosts> {
    Arc::new(FileKnownHosts::new(s.path("client_known_hosts")))
}

fn opts(kh: Arc<dyn KnownHostsStore>, decider: Arc<dyn HostKeyDecider>) -> ConnectOptions {
    let mut o = ConnectOptions::new(kh, decider);
    o.timeout = Duration::from_secs(20);
    o
}

fn fp(key: &russh::keys::PublicKey) -> String {
    key.fingerprint(russh::keys::HashAlg::Sha256).to_string()
}

/// Connects trusting the server's real host key (pre-recorded).
async fn connect_trusted(s: &TestSshd) -> SshSession {
    let kh = known_hosts(s);
    kh.add("127.0.0.1", s.port, &host_key(s)).unwrap();
    SshSession::connect(&target(s), &key_auth(s), &opts(kh, Arc::new(RejectUnknown)))
        .await
        .unwrap_or_else(|e| panic!("connect failed: {e}\nsshd log:\n{}", s.log()))
}

macro_rules! sshd_or_skip {
    ($cfg:expr) => {
        match common::start($cfg) {
            Some(s) => s,
            None => return,
        }
    };
}

// ---------------------------------------------------------------- host keys

#[tokio::test(flavor = "multi_thread")]
async fn unknown_host_key_is_rejected_by_default() {
    let s = sshd_or_skip!(SshdConfig::default());
    let kh = known_hosts(&s);
    let err = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh.clone(), Arc::new(RejectUnknown)),
    )
    .await
    .err()
    .expect("must not connect");
    match err {
        SshError::HostKeyRejected { fingerprint, .. } => assert_eq!(fingerprint, fp(&host_key(&s))),
        other => panic!("expected HostKeyRejected, got {other:?}"),
    }
    assert!(!kh.path().exists(), "nothing may be recorded on rejection");
}

#[tokio::test(flavor = "multi_thread")]
async fn accept_and_save_persists_and_is_trusted_next_time() {
    let s = sshd_or_skip!(SshdConfig::default());
    let kh = known_hosts(&s);
    let decider = Fixed::new(HostKeyDecision::AcceptAndSave);
    let sess = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh.clone(), decider.clone()),
    )
    .await
    .expect("connect");
    assert_eq!(decider.calls(), 1);
    assert_eq!(
        decider.last_fingerprint.lock().unwrap().as_deref(),
        Some(fp(&host_key(&s)).as_str()),
        "user must be shown the server's real fingerprint"
    );
    sess.disconnect().await.unwrap();

    // Second connection: trusted from the file, decider not consulted.
    let strict = Fixed::new(HostKeyDecision::Reject);
    let sess = SshSession::connect(&target(&s), &key_auth(&s), &opts(kh, strict.clone()))
        .await
        .expect("trusted reconnect");
    assert_eq!(strict.calls(), 0);
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn accept_once_does_not_persist() {
    let s = sshd_or_skip!(SshdConfig::default());
    let kh = known_hosts(&s);
    let sess = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh.clone(), Fixed::new(HostKeyDecision::AcceptOnce)),
    )
    .await
    .expect("connect");
    sess.disconnect().await.unwrap();
    assert!(!kh.path().exists());
    let again = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh, Arc::new(RejectUnknown)),
    )
    .await;
    assert!(matches!(again, Err(SshError::HostKeyRejected { .. })));
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_host_key_is_refused_without_asking() {
    let s = sshd_or_skip!(SshdConfig::default());
    let kh = known_hosts(&s);
    let impostor =
        russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone();
    kh.add("127.0.0.1", s.port, &impostor).unwrap();
    let decider = Fixed::new(HostKeyDecision::AcceptAndSave);
    let err = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh.clone(), decider.clone()),
    )
    .await
    .err()
    .expect("must not connect");
    match err {
        SshError::HostKeyChanged {
            presented, known, ..
        } => {
            assert_eq!(presented, fp(&host_key(&s)));
            assert_eq!(known, vec![fp(&impostor)]);
        }
        other => panic!("expected HostKeyChanged, got {other:?}"),
    }
    assert_eq!(
        decider.calls(),
        0,
        "a changed key must never reach the accept prompt"
    );
    assert_eq!(kh.keys_for("127.0.0.1", s.port).unwrap(), vec![impostor]);
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_host_key_of_other_algorithm_is_also_refused() {
    let s = sshd_or_skip!(SshdConfig::default()); // ed25519 host key
    let kh = known_hosts(&s);
    let ecdsa = russh::keys::PrivateKey::random(
        &mut rand::rng(),
        russh::keys::Algorithm::Ecdsa {
            curve: russh::keys::EcdsaCurve::NistP256,
        },
    )
    .unwrap()
    .public_key()
    .clone();
    kh.add("127.0.0.1", s.port, &ecdsa).unwrap();
    let decider = Fixed::new(HostKeyDecision::AcceptAndSave);
    let r = SshSession::connect(&target(&s), &key_auth(&s), &opts(kh, decider.clone())).await;
    assert!(
        matches!(r, Err(SshError::HostKeyChanged { .. })),
        "got {:?}",
        r.err()
    );
    assert_eq!(decider.calls(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn rsa_host_key_negotiates_sha2() {
    let s = sshd_or_skip!(SshdConfig {
        host_key_type: Some("rsa"),
        extra: vec!["HostKeyAlgorithms rsa-sha2-512,rsa-sha2-256".into()],
    });
    let sess = connect_trusted(&s).await;
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sha1_rsa_host_key_algorithm_is_not_accepted() {
    let s = sshd_or_skip!(SshdConfig {
        host_key_type: Some("rsa"),
        extra: vec!["HostKeyAlgorithms ssh-rsa".into()],
    });
    let kh = known_hosts(&s);
    kh.add("127.0.0.1", s.port, &host_key(&s)).unwrap();
    let r = SshSession::connect(
        &target(&s),
        &key_auth(&s),
        &opts(kh, Arc::new(RejectUnknown)),
    )
    .await;
    // Must fail in algorithm negotiation, not later (host-key trust or auth).
    match r {
        Err(SshError::Connect(msg)) => eprintln!("negotiation refused as expected: {msg}"),
        Err(other) => panic!("expected a negotiation failure, got {other:?}"),
        Ok(_) => panic!("client must not negotiate ssh-rsa (SHA-1) host keys"),
    }
    assert!(
        !s.log().contains("Accepted publickey"),
        "authentication must not have been reached"
    );
}

// ---------------------------------------------------------------- auth

#[tokio::test(flavor = "multi_thread")]
async fn unauthorized_key_fails_authentication() {
    let s = sshd_or_skip!(SshdConfig::default());
    let other = s.path("other_key");
    common::keygen(&other, "ed25519", "").unwrap();
    let kh = known_hosts(&s);
    kh.add("127.0.0.1", s.port, &host_key(&s)).unwrap();
    let r = SshSession::connect(
        &target(&s),
        &Auth::KeyFile {
            path: other,
            passphrase: None,
        },
        &opts(kh, Arc::new(RejectUnknown)),
    )
    .await;
    assert!(matches!(r, Err(SshError::AuthFailed)), "got {:?}", r.err());
}

#[tokio::test(flavor = "multi_thread")]
async fn passphrase_protected_key() {
    let s = sshd_or_skip!(SshdConfig::default());
    let key = s.path("enc_key");
    common::keygen(&key, "ed25519", "correct horse").unwrap();
    s.authorize(&s.path("enc_key.pub"));
    let kh = known_hosts(&s);
    kh.add("127.0.0.1", s.port, &host_key(&s)).unwrap();
    let o = opts(kh, Arc::new(RejectUnknown));

    let wrong = Auth::KeyFile {
        path: key.clone(),
        passphrase: Some(Zeroizing::new("wrong".into())),
    };
    assert!(matches!(
        SshSession::connect(&target(&s), &wrong, &o).await,
        Err(SshError::Key(_))
    ));

    let right = Auth::KeyFile {
        path: key,
        passphrase: Some(Zeroizing::new("correct horse".into())),
    };
    SshSession::connect(&target(&s), &right, &o)
        .await
        .expect("connect with passphrase")
        .disconnect()
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn rsa_user_key_signs_with_sha2() {
    // Server refuses SHA-1 RSA signatures, so success proves rsa-sha2-*.
    let s = sshd_or_skip!(SshdConfig {
        host_key_type: None,
        extra: vec!["PubkeyAcceptedAlgorithms rsa-sha2-512,rsa-sha2-256,ssh-ed25519".into()],
    });
    let key = s.path("rsa_user");
    common::keygen(&key, "rsa", "").unwrap();
    s.authorize(&s.path("rsa_user.pub"));
    let kh = known_hosts(&s);
    kh.add("127.0.0.1", s.port, &host_key(&s)).unwrap();
    SshSession::connect(
        &target(&s),
        &Auth::KeyFile {
            path: key,
            passphrase: None,
        },
        &opts(kh, Arc::new(RejectUnknown)),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}\n{}", s.log()))
    .disconnect()
    .await
    .unwrap();
}

#[test]
fn auth_debug_never_prints_secrets() {
    let a = Auth::Password(Zeroizing::new("hunter2".into()));
    assert!(!format!("{a:?}").contains("hunter2"));
    let k = Auth::KeyFile {
        path: "/k".into(),
        passphrase: Some(Zeroizing::new("s3cret".into())),
    };
    assert!(!format!("{k:?}").contains("s3cret"));
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_times_out_on_silent_server() {
    // Accepts TCP but never sends an SSH banner.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let _hold = tokio::spawn(async move {
        let _conn = listener.accept().await;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let dir = tempfile::tempdir().unwrap();
    let mut o = ConnectOptions::new(
        Arc::new(FileKnownHosts::new(dir.path().join("kh"))),
        Arc::new(RejectUnknown),
    );
    o.timeout = Duration::from_millis(500);
    let t = Target {
        host: "127.0.0.1".into(),
        port,
        user: "nobody".into(),
    };
    let start = Instant::now();
    let r = SshSession::connect(&t, &Auth::Password(Zeroizing::new("x".into())), &o).await;
    assert!(
        matches!(r, Err(SshError::Timeout { .. })),
        "got {:?}",
        r.err()
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}

/// Real password authentication needs a root-run sshd and a real account, so
/// it runs only where CI provides one via environment variables.
#[tokio::test(flavor = "multi_thread")]
async fn password_auth_against_system_sshd() {
    let (Ok(host), Ok(port), Ok(user), Ok(pw)) = (
        std::env::var("TERMALITH_TEST_SSH_HOST"),
        std::env::var("TERMALITH_TEST_SSH_PORT"),
        std::env::var("TERMALITH_TEST_SSH_USER"),
        std::env::var("TERMALITH_TEST_SSH_PASSWORD"),
    ) else {
        eprintln!("SKIPPED: TERMALITH_TEST_SSH_* not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let kh: Arc<dyn KnownHostsStore> = Arc::new(FileKnownHosts::new(dir.path().join("kh")));
    let o = opts(kh, Fixed::new(HostKeyDecision::AcceptOnce));
    let t = Target {
        host,
        port: port.parse().unwrap(),
        user,
    };

    let bad = SshSession::connect(
        &t,
        &Auth::Password(Zeroizing::new(format!("{pw}-wrong"))),
        &o,
    )
    .await;
    assert!(
        matches!(bad, Err(SshError::AuthFailed)),
        "got {:?}",
        bad.err()
    );

    let sess = SshSession::connect(&t, &Auth::Password(Zeroizing::new(pw)), &o)
        .await
        .expect("password auth");
    let out = sess.exec("id -un").await.unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), t.user);
    sess.disconnect().await.unwrap();
}

// ---------------------------------------------------------------- channels

#[tokio::test(flavor = "multi_thread")]
async fn exec_returns_output_and_exit_status() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let out = sess.exec(SH.exec).await.unwrap();
    // cmd.exe appends CRLF (and a space before redirection); compare trimmed.
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim(), "oops");
    assert_eq!(out.exit_status, Some(3));
    assert!(!out.truncated);
    sess.disconnect().await.unwrap();
}

/// Commands for the test server's default shell: POSIX sh on macOS/Linux,
/// cmd.exe under Windows OpenSSH (which also renders through ConPTY).
struct ShellDialect {
    exec: &'static str,
    marker_cmd: &'static [u8],
    marker: &'static str,
    size_cmd: &'static [u8],
    size_80x24: &'static str,
    size_132x43: &'static str,
    exit_cmd: &'static [u8],
}

#[cfg(not(windows))]
const SH: ShellDialect = ShellDialect {
    exec: "printf hello; printf oops >&2; exit 3",
    marker_cmd: b"echo marker-$((6 * 7))\n",
    marker: "marker-42",
    size_cmd: b"stty size\n",
    size_80x24: "24 80",
    size_132x43: "43 132",
    exit_cmd: b"exit 5\n",
};

#[cfg(windows)]
const SH: ShellDialect = ShellDialect {
    exec: "echo hello& echo oops 1>&2& exit /b 3",
    marker_cmd: b"set /a 6*7+1000\r",
    marker: "1042",
    size_cmd: b"mode con\r",
    // Matched after removing escapes and whitespace (ConPTY repaints).
    size_80x24: "Columns:80",
    size_132x43: "Columns:132",
    exit_cmd: b"exit 5\r",
};

/// ConPTY output is compared without escape sequences and whitespace.
fn shell_matches(buf: &str, needle: &str) -> bool {
    if cfg!(windows) {
        let mut plain = String::new();
        let mut chars = buf.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => match chars.next() {
                    Some('[') => {
                        for c in chars.by_ref() {
                            if ('\x40'..='\x7e').contains(&c) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        for c in chars.by_ref() {
                            if c == '\x07' {
                                break;
                            }
                        }
                    }
                    _ => {}
                },
                c if c.is_whitespace() => {}
                c => plain.push(c),
            }
        }
        plain.contains(needle)
    } else {
        buf.contains(needle)
    }
}

async fn read_until(shell: &mut protocol_ssh::ShellChannel, buf: &mut String, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !shell_matches(buf, needle) {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, shell.next_event()).await {
            Ok(ShellEvent::Output(b)) => buf.push_str(&String::from_utf8_lossy(&b)),
            Ok(other) => panic!("unexpected {other:?} while waiting for {needle:?}; got {buf:?}"),
            Err(_) => panic!("timed out waiting for {needle:?}; got {buf:?}"),
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn interactive_shell_on_remote_pty() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let mut shell = sess.open_shell(80, 24).await.unwrap();
    let mut out = String::new();
    shell.write(SH.marker_cmd).await.unwrap();
    read_until(&mut shell, &mut out, SH.marker).await;
    shell.write(SH.size_cmd).await.unwrap();
    read_until(&mut shell, &mut out, SH.size_80x24).await;
    shell.resize(132, 43).await.unwrap();
    out.clear();
    shell.write(SH.size_cmd).await.unwrap();
    read_until(&mut shell, &mut out, SH.size_132x43).await;
    shell.write(SH.exit_cmd).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, shell.next_event())
            .await
            .expect("exit status")
        {
            ShellEvent::Exited(code) => {
                // Windows OpenSSH with a PTY reports its ConPTY wrapper's
                // exit code, not the shell's; only Unix servers pass it on.
                if cfg!(not(windows)) {
                    assert_eq!(code, 5);
                }
                break;
            }
            ShellEvent::Output(_) => {}
            ShellEvent::Closed => panic!("closed without exit status"),
        }
    }
    sess.disconnect().await.unwrap();
}

// ---------------------------------------------------------------- SFTP

fn write_test_file(path: &Path, len: usize) -> String {
    // Deterministic, non-repeating content (xorshift), so corruption shows.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut data = Vec::with_capacity(len);
    while data.len() < len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        data.extend_from_slice(&x.to_le_bytes());
    }
    data.truncate(len);
    std::fs::write(path, &data).unwrap();
    hex(&Sha256::digest(&data))
}

fn sha256_file(path: &Path) -> String {
    hex(&Sha256::digest(std::fs::read(path).unwrap()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn no_partials(dir: &Path) {
    let leftovers: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("termalith-part"))
        .collect();
    assert!(leftovers.is_empty(), "partial files left: {leftovers:?}");
}

const SIZE: usize = 5 * 1024 * 1024 + 123;

#[tokio::test(flavor = "multi_thread")]
async fn sftp_upload_download_round_trip() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let sftp = sess.sftp().await.unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let src = local_dir.path().join("src.bin");
    let digest = write_test_file(&src, SIZE);
    let remote = s.remote("uploaded.bin");
    let never = CancellationToken::new();

    let mut last = 0;
    let n = sftp
        .upload(&src, &remote, TransferOptions::default(), &never, |b| {
            last = b
        })
        .await
        .unwrap();
    assert_eq!(n as usize, SIZE);
    assert_eq!(last as usize, SIZE);
    assert_eq!(sha256_file(&s.path("uploaded.bin")), digest);

    let back = local_dir.path().join("back.bin");
    sftp.download(&remote, &back, TransferOptions::default(), &never, |_| {})
        .await
        .unwrap();
    assert_eq!(sha256_file(&back), digest);
    no_partials(s.dir.path());
    no_partials(local_dir.path());
    sftp.close().await.unwrap();
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_refuses_to_overwrite_unless_asked() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let sftp = sess.sftp().await.unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let src = local_dir.path().join("a.txt");
    std::fs::write(&src, b"new").unwrap();
    std::fs::write(s.path("existing.txt"), b"old").unwrap();
    let never = CancellationToken::new();

    let r = sftp
        .upload(
            &src,
            &s.remote("existing.txt"),
            TransferOptions::default(),
            &never,
            |_| {},
        )
        .await;
    assert!(matches!(r, Err(SshError::DestinationExists(_))));
    assert_eq!(std::fs::read(s.path("existing.txt")).unwrap(), b"old");

    let overwrite = TransferOptions {
        overwrite: true,
        ..Default::default()
    };
    sftp.upload(&src, &s.remote("existing.txt"), overwrite, &never, |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(s.path("existing.txt")).unwrap(), b"new");

    let local_existing = local_dir.path().join("local.txt");
    std::fs::write(&local_existing, b"keep").unwrap();
    let r = sftp
        .download(
            &s.remote("existing.txt"),
            &local_existing,
            TransferOptions::default(),
            &never,
            |_| {},
        )
        .await;
    assert!(matches!(r, Err(SshError::DestinationExists(_))));
    assert_eq!(std::fs::read(&local_existing).unwrap(), b"keep");
    no_partials(s.dir.path());
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_cancelled_upload_leaves_nothing() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let sftp = sess.sftp().await.unwrap();
    let local_dir = tempfile::tempdir().unwrap();
    let src = local_dir.path().join("big.bin");
    write_test_file(&src, SIZE);
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    let r = sftp
        .upload(
            &src,
            &s.remote("cancelled.bin"),
            TransferOptions::default(),
            &cancel,
            |b| {
                if b >= 512 * 1024 {
                    c.cancel();
                }
            },
        )
        .await;
    assert!(matches!(r, Err(SshError::Cancelled)), "got {:?}", r.err());
    assert!(!s.path("cancelled.bin").exists());
    no_partials(s.dir.path());
    // The session is still usable afterwards.
    assert!(!sftp.exists(&s.remote("cancelled.bin")).await.unwrap());
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_cancelled_download_leaves_nothing() {
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = connect_trusted(&s).await;
    let sftp = sess.sftp().await.unwrap();
    write_test_file(&s.path("remote.bin"), SIZE);
    let local_dir = tempfile::tempdir().unwrap();
    let dst = local_dir.path().join("dl.bin");
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    let r = sftp
        .download(
            &s.remote("remote.bin"),
            &dst,
            TransferOptions::default(),
            &cancel,
            |b| {
                if b >= 512 * 1024 {
                    c.cancel();
                }
            },
        )
        .await;
    assert!(matches!(r, Err(SshError::Cancelled)), "got {:?}", r.err());
    assert!(!dst.exists());
    no_partials(local_dir.path());
    sess.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn sftp_lists_directories_first_and_resolves_home() {
    let s = sshd_or_skip!(SshdConfig::default());
    let root = s.path("listing");
    std::fs::create_dir_all(root.join("zeta-dir")).unwrap();
    std::fs::create_dir_all(root.join("Alpha-dir")).unwrap();
    std::fs::write(root.join("b.txt"), b"12345").unwrap();
    std::fs::write(root.join("A.log"), b"").unwrap();
    let sess = connect_trusted(&s).await;
    let sftp = sess.sftp().await.unwrap();

    let entries = sftp.list(&s.remote("listing")).await.unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Alpha-dir", "zeta-dir", "A.log", "b.txt"]);
    assert_eq!(entries[0].kind, protocol_ssh::RemoteKind::Dir);
    let b = entries.iter().find(|e| e.name == "b.txt").unwrap();
    assert_eq!((b.kind, b.size), (protocol_ssh::RemoteKind::File, Some(5)));
    assert!(b.modified.is_some());

    let home = sftp.canonicalize(".").await.unwrap();
    assert!(home.starts_with('/'), "home should be absolute: {home}");
    sess.disconnect().await.unwrap();
}

/// A private ssh-agent on its own socket, killed on drop.
#[cfg(unix)]
struct TestAgent {
    socket: std::path::PathBuf,
    child: std::process::Child,
}

#[cfg(unix)]
impl TestAgent {
    fn start(dir: &Path) -> Self {
        let socket = dir.join("agent.sock");
        let child = std::process::Command::new("ssh-agent")
            .args(["-D", "-a"])
            .arg(&socket)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("ssh-agent");
        let start = Instant::now();
        while !socket.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "agent socket never appeared"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        Self { socket, child }
    }

    fn add(&self, key: &Path) {
        let ok = std::process::Command::new("ssh-add")
            .arg(key)
            .env("SSH_AUTH_SOCK", &self.socket)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("ssh-add")
            .status
            .success();
        assert!(ok, "ssh-add failed");
    }
}

#[cfg(unix)]
impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn agent_authentication() {
    let s = sshd_or_skip!(SshdConfig::default());
    let kh = known_hosts(&s);
    kh.add("127.0.0.1", s.port, &host_key(&s)).unwrap();
    let o = opts(kh, Arc::new(RejectUnknown));
    let agent = TestAgent::start(s.dir.path());
    let auth = Auth::Agent {
        socket: agent.socket.clone(),
    };

    // Empty agent: a clear agent error, not a generic failure.
    match SshSession::connect(&target(&s), &auth, &o).await {
        Err(SshError::Agent(msg)) => assert!(msg.contains("no keys"), "{msg}"),
        other => panic!("expected agent error, got {:?}", other.err()),
    }

    // An unrelated key first, then the authorized one: the client tries
    // each identity until the server accepts one.
    let other = s.path("unrelated");
    common::keygen(&other, "ed25519", "").unwrap();
    agent.add(&other);
    agent.add(&s.user_key);
    let sess = SshSession::connect(&target(&s), &auth, &o)
        .await
        .unwrap_or_else(|e| panic!("agent auth failed: {e}\n{}", s.log()));
    assert_eq!(sess.exec("printf ok").await.unwrap().stdout, b"ok");
    sess.disconnect().await.unwrap();

    // No agent at that path.
    let missing = Auth::Agent {
        socket: s.path("no-such.sock"),
    };
    assert!(matches!(
        SshSession::connect(&target(&s), &missing, &o).await,
        Err(SshError::Agent(_))
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_jump_through_a_bastion() {
    // One server plays both roles: the client tunnels through it to itself.
    let s = sshd_or_skip!(SshdConfig::default());
    let jump = Arc::new(connect_trusted(&s).await);

    // The target's host key is checked separately: unknown -> asked.
    let kh = Arc::new(FileKnownHosts::new(s.path("target_known_hosts")));
    let decider = Fixed::new(HostKeyDecision::AcceptOnce);
    let target = Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: s.user.clone(),
    };
    let via = SshSession::connect_via(
        jump.clone(),
        &target,
        &key_auth(&s),
        &opts(kh, decider.clone()),
    )
    .await
    .unwrap_or_else(|e| panic!("jump failed: {e}\n{}", s.log()));
    assert_eq!(
        decider.calls(),
        1,
        "target host key must be verified on its own"
    );
    assert_eq!(
        via.exec("printf via-jump").await.unwrap().stdout,
        b"via-jump"
    );
    via.disconnect().await.unwrap();

    // The jump connection stays usable on its own.
    assert_eq!(
        jump.exec("printf still-up").await.unwrap().stdout,
        b"still-up"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn proxy_jump_refused_by_server_policy() {
    let s = sshd_or_skip!(SshdConfig {
        host_key_type: None,
        extra: vec!["AllowTcpForwarding no".into()],
    });
    let jump = Arc::new(connect_trusted(&s).await);
    let kh = known_hosts(&s);
    let target = Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: s.user.clone(),
    };
    match SshSession::connect_via(
        jump,
        &target,
        &key_auth(&s),
        &opts(kh, Arc::new(RejectUnknown)),
    )
    .await
    {
        Err(SshError::Connect(msg)) => assert!(msg.contains("tunnel"), "{msg}"),
        other => panic!("expected a tunnel error, got {:?}", other.err()),
    }
}

/// TCP echo server on loopback (IPv4 and, where available, IPv6 on the
/// same port, since "localhost" may resolve to ::1 first, e.g. on
/// Windows); returns its port.
async fn echo_server() -> u16 {
    let v4 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = v4.local_addr().unwrap().port();
    let v6 = tokio::net::TcpListener::bind(("::1", port)).await.ok();
    for l in std::iter::once(v4).chain(v6) {
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
    }
    port
}

async fn round_trip(stream: &mut tokio::net::TcpStream, msg: &[u8]) -> Vec<u8> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream.write_all(msg).await.unwrap();
    let mut buf = vec![0u8; msg.len()];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut buf))
        .await
        .expect("echo timed out")
        .unwrap();
    buf
}

#[tokio::test(flavor = "multi_thread")]
async fn local_forward_tunnels_to_a_service_behind_the_server() {
    use protocol_ssh::{ForwardKind, start_forward};
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = Arc::new(connect_trusted(&s).await);
    let echo = echo_server().await;
    let fwd = start_forward(
        sess.clone(),
        "127.0.0.1:0".parse().unwrap(),
        ForwardKind::Local {
            host: "127.0.0.1".into(),
            port: echo,
        },
    )
    .await
    .unwrap();
    assert!(fwd.local_addr().ip().is_loopback());

    // Two concurrent clients through the same forward.
    let mut a = tokio::net::TcpStream::connect(fwd.local_addr())
        .await
        .unwrap();
    let mut b = tokio::net::TcpStream::connect(fwd.local_addr())
        .await
        .unwrap();
    assert_eq!(
        round_trip(&mut a, b"hello through -L").await,
        b"hello through -L"
    );
    assert_eq!(
        round_trip(&mut b, &[7u8; 100_000]).await,
        vec![7u8; 100_000]
    );
    assert_eq!(fwd.total_connections(), 2);

    // Stopping closes the listener.
    let addr = fwd.local_addr();
    fwd.stop();
    drop(fwd);
    let start = Instant::now();
    while tokio::net::TcpStream::connect(addr).await.is_ok() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "listener still open"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn socks5_dynamic_forward() {
    use protocol_ssh::{ForwardKind, start_forward};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let s = sshd_or_skip!(SshdConfig::default());
    let sess = Arc::new(connect_trusted(&s).await);
    let echo = echo_server().await;
    let fwd = start_forward(sess, "127.0.0.1:0".parse().unwrap(), ForwardKind::Socks)
        .await
        .unwrap();

    async fn socks_connect(
        proxy: std::net::SocketAddr,
        addr: &[u8],
        port: u16,
    ) -> (tokio::net::TcpStream, u8) {
        let mut c = tokio::net::TcpStream::connect(proxy).await.unwrap();
        c.write_all(&[5, 1, 0]).await.unwrap();
        let mut m = [0u8; 2];
        c.read_exact(&mut m).await.unwrap();
        assert_eq!(m, [5, 0]);
        let mut req = vec![5, 1, 0];
        req.extend_from_slice(addr);
        req.extend_from_slice(&port.to_be_bytes());
        c.write_all(&req).await.unwrap();
        let mut rep = [0u8; 10];
        c.read_exact(&mut rep).await.unwrap();
        (c, rep[1])
    }

    // IPv4 target.
    let (mut c, code) = socks_connect(fwd.local_addr(), &[1, 127, 0, 0, 1], echo).await;
    assert_eq!(code, 0);
    assert_eq!(
        round_trip(&mut c, b"via socks ipv4").await,
        b"via socks ipv4"
    );

    // Domain-name target, resolved on the server.
    let mut domain = vec![3, 9];
    domain.extend_from_slice(b"localhost");
    let (mut c, code) = socks_connect(fwd.local_addr(), &domain, echo).await;
    assert_eq!(code, 0);
    assert_eq!(
        round_trip(&mut c, b"via socks name").await,
        b"via socks name"
    );

    // Nothing listening there: "connection refused" reply.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = closed.local_addr().unwrap().port();
    drop(closed);
    let (_c, code) = socks_connect(fwd.local_addr(), &[1, 127, 0, 0, 1], dead_port).await;
    assert_eq!(code, 0x05);

    // A client that insists on authentication is turned away.
    let mut c = tokio::net::TcpStream::connect(fwd.local_addr())
        .await
        .unwrap();
    c.write_all(&[5, 1, 2]).await.unwrap();
    let mut m = [0u8; 2];
    c.read_exact(&mut m).await.unwrap();
    assert_eq!(m, [5, 0xFF]);
}
