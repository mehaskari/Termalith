//! Old devices: an in-process russh server that offers only the algorithms
//! of an old switch (diffie-hellman-group1-sha1, aes128-cbc, hmac-sha1,
//! ssh-rsa host key). Without legacy algorithms the client refuses with a
//! typed error the UI can turn into a question; with them it connects
//! (ADR-043). A server with only the ECDH NIST curves needs no legacy mode.

use std::borrow::Cow;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use protocol_ssh::{
    Auth, ConnectOptions, FileKnownHosts, KnownHostsStore, RejectUnknown, SshError, SshSession,
    Target,
};
use russh::keys::{Algorithm, PrivateKey};
use russh::server::{self, Auth as ServerAuth};
use russh::{MethodKind, MethodSet, Preferred, cipher, kex, mac};
use zeroize::Zeroizing;

/// Random per run (nothing hard-coded).
static PASSWORD: LazyLock<String> = LazyLock::new(|| {
    use rand::RngExt;
    let bytes: [u8; 8] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
});

struct Server;

impl server::Handler for Server {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        _user: &str,
        password: &str,
    ) -> Result<ServerAuth, Self::Error> {
        Ok(if password == PASSWORD.as_str() {
            ServerAuth::Accept
        } else {
            ServerAuth::reject()
        })
    }
}

struct TestServer {
    port: u16,
    key: russh::keys::PublicKey,
}

async fn start(key: PrivateKey, preferred: Preferred) -> TestServer {
    let public = key.public_key().clone();
    let config = Arc::new(server::Config {
        keys: vec![key],
        preferred,
        methods: MethodSet::from(&[MethodKind::Password][..]),
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let config = Arc::clone(&config);
            tokio::spawn(async move {
                if let Ok(session) = server::run_stream(config, tcp, Server).await {
                    let _ = session.await;
                }
            });
        }
    });
    TestServer { port, key: public }
}

/// An old switch: SHA-1 key exchange, CBC, HMAC-SHA1, RSA host key with SHA-1.
async fn old_switch() -> TestServer {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Rsa { hash: None }).unwrap();
    start(
        key,
        Preferred {
            kex: Cow::Borrowed(&[kex::DH_G1_SHA1, kex::EXTENSION_SUPPORT_AS_SERVER]),
            key: Cow::Borrowed(&[Algorithm::Rsa { hash: None }]),
            cipher: Cow::Borrowed(&[cipher::AES_128_CBC]),
            mac: Cow::Borrowed(&[mac::HMAC_SHA1]),
            ..Preferred::DEFAULT
        },
    )
    .await
}

async fn connect(s: &TestServer, legacy: bool) -> Result<SshSession, SshError> {
    let dir = tempfile::tempdir().unwrap();
    let kh = FileKnownHosts::new(dir.path().join("known_hosts"));
    kh.add("127.0.0.1", s.port, &s.key).unwrap();
    let mut opts = ConnectOptions::new(
        Arc::new(kh) as Arc<dyn KnownHostsStore>,
        Arc::new(RejectUnknown),
    );
    opts.timeout = Duration::from_secs(20);
    opts.legacy_algorithms = legacy;
    let target = Target {
        host: "127.0.0.1".into(),
        port: s.port,
        user: "admin".into(),
    };
    SshSession::connect(
        &target,
        &Auth::Password(Zeroizing::new(PASSWORD.clone())),
        &opts,
    )
    .await
}

#[tokio::test]
async fn an_old_switch_is_refused_with_a_question_and_accepted_with_legacy_algorithms() {
    let s = old_switch().await;
    match connect(&s, false).await {
        Err(SshError::NoCommonAlgorithm {
            kind,
            theirs,
            legacy_would_help,
        }) => {
            assert_eq!(kind, "kex");
            assert!(
                theirs.iter().any(|t| t == "diffie-hellman-group1-sha1"),
                "{theirs:?}"
            );
            assert!(legacy_would_help);
        }
        Err(other) => panic!("expected NoCommonAlgorithm, got {other}"),
        Ok(_) => panic!("must not connect without legacy algorithms"),
    }
    let session = connect(&s, true)
        .await
        .expect("connects with legacy algorithms");
    drop(session);
}

#[tokio::test]
async fn a_server_with_only_ecdh_nist_curves_needs_no_legacy_mode() {
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let s = start(
        key,
        Preferred {
            kex: Cow::Borrowed(&[kex::ECDH_SHA2_NISTP256, kex::EXTENSION_SUPPORT_AS_SERVER]),
            ..Preferred::DEFAULT
        },
    )
    .await;
    connect(&s, false)
        .await
        .expect("ecdh-sha2-nistp256 is in the normal list");
}

#[tokio::test]
async fn legacy_mode_is_not_offered_when_it_would_not_help() {
    // Only an algorithm Termalith never supports (none of ours, none legacy).
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let s = start(
        key,
        Preferred {
            cipher: Cow::Borrowed(&[cipher::NONE]),
            ..Preferred::DEFAULT
        },
    )
    .await;
    match connect(&s, false).await {
        Err(SshError::NoCommonAlgorithm {
            legacy_would_help, ..
        }) => assert!(!legacy_would_help),
        Err(other) => panic!("expected NoCommonAlgorithm, got {other}"),
        Ok(_) => panic!("must not connect"),
    }
}
