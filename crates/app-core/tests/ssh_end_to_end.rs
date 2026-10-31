//! Saved session → host-key prompt → SSH terminal in the hub, against a real
//! OpenSSH server (see the `test-sshd` crate).
// The shell-interaction tests are Unix-only (POSIX commands); their helpers
// are unused on Windows.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::{
    AppError, AuthMethod, ConnectRequest, Connector, FLOW_HIGH_WATERMARK, HostKeyPrompt,
    PasswordInput, PromptBroker, SessionInput, SessionStore, TerminalHub,
};
use credential_store::MemorySecretStore;
use protocol_ssh::{FileKnownHosts, HostKeyDecision, KnownHostsStore};
use terminal_pty::{TerminalEvent, TerminalSize};
use test_sshd::{SshdConfig, TestSshd};
use tokio::sync::mpsc;

struct Rig {
    sshd: TestSshd,
    connector: Arc<Connector>,
    broker: Arc<PromptBroker>,
    prompts: mpsc::UnboundedReceiver<HostKeyPrompt>,
    known_hosts: Arc<FileKnownHosts>,
    hub: TerminalHub,
}

fn rig() -> Option<Rig> {
    let sshd = test_sshd::start(SshdConfig::default())?;
    let (tx, prompts) = mpsc::unbounded_channel();
    let broker = Arc::new(PromptBroker::new(
        move |p| {
            let _ = tx.send(p);
        },
        Duration::from_secs(10),
    ));
    let known_hosts = Arc::new(FileKnownHosts::new(sshd.path("app_known_hosts")));
    let connector = Arc::new(Connector {
        sessions: Arc::new(SessionStore::open_in_memory().unwrap()),
        secrets: Arc::new(MemorySecretStore::default()),
        known_hosts: known_hosts.clone(),
        broker: broker.clone(),
        timeout: Duration::from_secs(20),
    });
    Some(Rig {
        sshd,
        connector,
        broker,
        prompts,
        known_hosts,
        hub: TerminalHub::with_default_flow(),
    })
}

fn key_session(r: &Rig) -> app_core::SessionRecord {
    r.connector
        .sessions
        .create(&SessionInput {
            name: "test server".into(),
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::KeyFile {
                path: r.sshd.user_key.clone(),
            },
            folder: None,
        })
        .unwrap()
}

fn host_fp(r: &Rig) -> String {
    protocol_ssh::PublicKey::from_openssh(r.sshd.host_pub.trim())
        .unwrap()
        .fingerprint(Default::default())
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_session_prompts_once_then_is_trusted() {
    let Some(mut r) = rig() else { return };
    let rec = key_session(&r);

    let c = r.connector.clone();
    let id = rec.id.clone();
    let connecting = tokio::spawn(async move {
        c.connect(ConnectRequest::Saved {
            id,
            secret: PasswordInput::None,
        })
        .await
    });
    let prompt = tokio::time::timeout(Duration::from_secs(10), r.prompts.recv())
        .await
        .expect("host-key prompt")
        .unwrap();
    assert_eq!(prompt.port, r.sshd.port);
    assert_eq!(prompt.fingerprint, host_fp(&r));
    assert!(
        r.broker
            .respond(prompt.request_id, HostKeyDecision::AcceptAndSave)
    );
    let connected = connecting.await.unwrap().expect("connected");
    assert_eq!(connected.label, "test server");
    connected.session.disconnect().await.unwrap();
    assert_eq!(
        r.known_hosts
            .keys_for("127.0.0.1", r.sshd.port)
            .unwrap()
            .len(),
        1
    );

    // Second time: trusted, no prompt.
    let again = r
        .connector
        .connect(ConnectRequest::Saved {
            id: rec.id.clone(),
            secret: PasswordInput::None,
        })
        .await
        .expect("trusted reconnect");
    assert!(r.prompts.try_recv().is_err(), "no prompt for a trusted key");
    again.session.disconnect().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_prompt_fails_and_records_nothing() {
    let Some(mut r) = rig() else { return };
    let rec = key_session(&r);
    let c = r.connector.clone();
    let connecting = tokio::spawn(async move {
        c.connect(ConnectRequest::Saved {
            id: rec.id,
            secret: PasswordInput::None,
        })
        .await
    });
    let prompt = r.prompts.recv().await.unwrap();
    r.broker.respond(prompt.request_id, HostKeyDecision::Reject);
    let res = connecting.await.unwrap();
    assert!(
        matches!(
            res,
            Err(AppError::Ssh(
                protocol_ssh::SshError::HostKeyRejected { .. }
            ))
        ),
        "got {:?}",
        res.err()
    );
    assert!(
        r.known_hosts
            .keys_for("127.0.0.1", r.sshd.port)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn password_sessions_without_secret_ask_for_one() {
    let Some(r) = rig() else { return };
    let rec = r
        .connector
        .sessions
        .create(&SessionInput {
            name: "pw".into(),
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::Password,
            folder: None,
        })
        .unwrap();
    let res = r
        .connector
        .connect(ConnectRequest::Saved {
            id: rec.id,
            secret: PasswordInput::None,
        })
        .await;
    assert!(
        matches!(res, Err(AppError::PasswordRequired)),
        "got {:?}",
        res.err()
    );

    let quick = r
        .connector
        .connect(ConnectRequest::Quick {
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::Password,
            secret: PasswordInput::None,
        })
        .await;
    assert!(matches!(quick, Err(AppError::PasswordRequired)));
}

async fn trusted_connection(r: &Rig) -> Arc<protocol_ssh::SshSession> {
    let key = protocol_ssh::PublicKey::from_openssh(r.sshd.host_pub.trim()).unwrap();
    r.known_hosts.add("127.0.0.1", r.sshd.port, &key).unwrap();
    let rec = key_session(r);
    r.connector
        .connect(ConnectRequest::Saved {
            id: rec.id,
            secret: PasswordInput::None,
        })
        .await
        .expect("connect")
        .session
}

type Collected = (
    Arc<Mutex<Vec<u8>>>,
    Arc<AtomicUsize>,
    std::sync::mpsc::Receiver<()>,
);

fn collector() -> (
    impl FnMut(TerminalEvent) -> bool + Send + 'static,
    Collected,
) {
    let out = Arc::new(Mutex::new(Vec::new()));
    let exits = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = std::sync::mpsc::channel();
    let (o, e) = (out.clone(), exits.clone());
    let sink = move |ev| {
        match ev {
            TerminalEvent::Output(b) => o.lock().unwrap().extend_from_slice(&b),
            TerminalEvent::Exited(_) => {
                e.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            }
        }
        true
    };
    (sink, (out, exits, rx))
}

async fn wait_for(out: &Mutex<Vec<u8>>, needle: &str) {
    let start = Instant::now();
    while !String::from_utf8_lossy(&out.lock().unwrap()).contains(needle) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {needle:?}: {:?}",
            String::from_utf8_lossy(&out.lock().unwrap())
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ssh_terminal_through_hub() {
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let (sink, (out, exits, exited)) = collector();
    let id = r
        .hub
        .open_ssh(session, TerminalSize::new(80, 24).unwrap(), sink)
        .await
        .unwrap();

    r.hub.write(id, b"echo e2e-$((6 * 7))\n").await.unwrap();
    wait_for(&out, "e2e-42").await;
    r.hub
        .resize(id, TerminalSize::new(120, 40).unwrap())
        .await
        .unwrap();
    r.hub.write(id, b"stty size\n").await.unwrap();
    wait_for(&out, "40 120").await;

    // Remote exit ends the terminal and removes it from the hub.
    r.hub.write(id, b"exit\n").await.unwrap();
    exited
        .recv_timeout(Duration::from_secs(10))
        .expect("Exited");
    let start = Instant::now();
    while !r.hub.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(exits.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn ssh_output_is_flow_controlled() {
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let (sink, (out, _exits, _exited)) = collector();
    let id = r
        .hub
        .open_ssh(session, TerminalSize::new(80, 24).unwrap(), sink)
        .await
        .unwrap();
    r.hub.write(id, b"yes flow-control-test\n").await.unwrap();

    let start = Instant::now();
    while out.lock().unwrap().len() < FLOW_HIGH_WATERMARK {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "output never reached the window"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let first = out.lock().unwrap().len();
    // One SSH data message may overshoot the window.
    assert!(
        first <= FLOW_HIGH_WATERMARK + 256 * 1024,
        "{first} bytes without acks"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        out.lock().unwrap().len(),
        first,
        "kept reading without acks"
    );

    r.hub.ack(id, first).unwrap();
    let start = Instant::now();
    while out.lock().unwrap().len() == first {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "ack did not resume"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    r.hub.close(id).await.unwrap();
    assert!(r.hub.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn secret_is_saved_only_after_a_successful_connection() {
    let Some(r) = rig() else { return };
    let key = protocol_ssh::PublicKey::from_openssh(r.sshd.host_pub.trim()).unwrap();
    r.known_hosts.add("127.0.0.1", r.sshd.port, &key).unwrap();
    let enc = r.sshd.path("enc_key");
    test_sshd::keygen(&enc, "ed25519", "open sesame").unwrap();
    r.sshd.authorize(&r.sshd.path("enc_key.pub"));
    let rec = r
        .connector
        .sessions
        .create(&SessionInput {
            name: "encrypted key".into(),
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::KeyFile { path: enc },
            folder: None,
        })
        .unwrap();
    let provided = |s: &str| PasswordInput::Provided {
        secret: zeroize::Zeroizing::new(s.to_owned()),
        save: true,
    };

    // Wrong passphrase: fails and nothing is saved.
    let bad = r
        .connector
        .connect(ConnectRequest::Saved {
            id: rec.id.clone(),
            secret: provided("wrong"),
        })
        .await;
    assert!(bad.is_err());
    assert!(!r.connector.sessions.get(&rec.id).unwrap().has_saved_secret);

    // Right passphrase with "save": succeeds and is stored.
    r.connector
        .connect(ConnectRequest::Saved {
            id: rec.id.clone(),
            secret: provided("open sesame"),
        })
        .await
        .expect("connect")
        .session
        .disconnect()
        .await
        .unwrap();
    let saved = r.connector.sessions.get(&rec.id).unwrap();
    assert!(saved.has_saved_secret);
    let stored = r
        .connector
        .secrets
        .get(saved.credential.as_ref().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(stored.as_slice(), b"open sesame");

    // Next time no secret is typed: the saved one is used.
    r.connector
        .connect(ConnectRequest::Saved {
            id: rec.id.clone(),
            secret: PasswordInput::None,
        })
        .await
        .expect("connect with saved passphrase")
        .session
        .disconnect()
        .await
        .unwrap();

    // Deleting the session removes the secret too.
    let cred = saved.credential.unwrap();
    r.connector.delete_session(&rec.id).await.unwrap();
    assert!(r.connector.secrets.get(&cred).unwrap().is_none());
}
