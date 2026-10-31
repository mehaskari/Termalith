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
    let out = sess
        .exec("printf hello; printf oops >&2; exit 3")
        .await
        .unwrap();
    assert_eq!(out.stdout, b"hello");
    assert_eq!(out.stderr, b"oops");
    assert_eq!(out.exit_status, Some(3));
    assert!(!out.truncated);
    sess.disconnect().await.unwrap();
}

async fn read_until(shell: &mut protocol_ssh::ShellChannel, buf: &mut String, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !buf.contains(needle) {
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
    shell.write(b"echo marker-$((6 * 7))\n").await.unwrap();
    read_until(&mut shell, &mut out, "marker-42").await;
    shell.write(b"stty size\n").await.unwrap();
    read_until(&mut shell, &mut out, "24 80").await;
    shell.resize(132, 43).await.unwrap();
    shell.write(b"stty size\n").await.unwrap();
    read_until(&mut shell, &mut out, "43 132").await;
    shell.write(b"exit 5\n").await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match tokio::time::timeout(left, shell.next_event())
            .await
            .expect("exit status")
        {
            ShellEvent::Exited(code) => {
                assert_eq!(code, 5);
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
