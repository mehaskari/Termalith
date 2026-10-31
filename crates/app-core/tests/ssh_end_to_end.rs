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
        agent: None,
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
            jump_via: None,
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
            jump_via: None,
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
            jump_via: None,
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

async fn wait_event(
    rx: &mut mpsc::UnboundedReceiver<app_core::TransferEvent>,
) -> app_core::TransferEvent {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("transfer event")
            .expect("channel open");
        if !matches!(ev, app_core::TransferEvent::Progress { .. }) {
            return ev;
        }
    }
}

fn event_sink() -> (
    impl FnMut(app_core::TransferEvent) + Send + 'static,
    mpsc::UnboundedReceiver<app_core::TransferEvent>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        move |e| {
            let _ = tx.send(e);
        },
        rx,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn file_browser_transfers_over_the_terminal_connection() {
    use app_core::{TransferEvent, Transfers};
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let (sink, _c) = collector();
    let id = r
        .hub
        .open_ssh(session, TerminalSize::new(80, 24).unwrap(), sink)
        .await
        .unwrap();
    let remote_dir = r.sshd.remote("files");
    std::fs::create_dir_all(r.sshd.path("files")).unwrap();
    std::fs::write(r.sshd.path("files").join("remote.txt"), b"from the server").unwrap();

    let sftp = r.hub.sftp(id).await.unwrap();
    // Same client is reused for the same terminal.
    assert!(Arc::ptr_eq(&sftp, &r.hub.sftp(id).await.unwrap()));
    let listing = sftp.list(&remote_dir).await.unwrap();
    assert_eq!(
        listing.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["remote.txt"]
    );

    let transfers = Arc::new(Transfers::default());
    let local = tempfile::tempdir().unwrap();

    // Download into a backend-chosen directory; a second download gets a
    // unique name instead of overwriting.
    for expected in ["remote.txt", "remote (1).txt"] {
        let (sink, mut rx) = event_sink();
        transfers
            .start_download(
                sftp.clone(),
                &remote_dir,
                "remote.txt",
                Some(15),
                local.path().into(),
                sink,
            )
            .unwrap();
        match wait_event(&mut rx).await {
            TransferEvent::Done { destination } => {
                assert_eq!(destination, local.path().join(expected).to_string_lossy());
                assert_eq!(std::fs::read(&destination).unwrap(), b"from the server");
            }
            other => panic!("{other:?}"),
        }
    }

    // Untrusted remote names cannot escape the destination directory.
    let (sink, _rx) = event_sink();
    assert!(
        transfers
            .start_download(
                sftp.clone(),
                &remote_dir,
                "../escape.txt",
                None,
                local.path().into(),
                sink
            )
            .is_err()
    );

    // Upload keeps the name and refuses to overwrite.
    let up = local.path().join("upload.bin");
    std::fs::write(&up, vec![7u8; 300_000]).unwrap();
    let (sink, mut rx) = event_sink();
    transfers
        .start_upload(sftp.clone(), up.clone(), &remote_dir, sink)
        .unwrap();
    assert!(matches!(
        wait_event(&mut rx).await,
        TransferEvent::Done { .. }
    ));
    assert_eq!(
        std::fs::read(r.sshd.path("files").join("upload.bin"))
            .unwrap()
            .len(),
        300_000
    );
    let (sink, mut rx) = event_sink();
    transfers
        .start_upload(sftp.clone(), up, &remote_dir, sink)
        .unwrap();
    match wait_event(&mut rx).await {
        TransferEvent::Failed { message } => {
            assert!(message.contains("already exists"), "{message}")
        }
        other => panic!("{other:?}"),
    }

    // Cancelling a large download leaves nothing behind.
    std::fs::write(
        r.sshd.path("files").join("big.bin"),
        vec![1u8; 20 * 1024 * 1024],
    )
    .unwrap();
    let (sink, mut rx) = event_sink();
    let tid = transfers
        .start_download(
            sftp.clone(),
            &remote_dir,
            "big.bin",
            None,
            local.path().into(),
            sink,
        )
        .unwrap();
    assert!(transfers.cancel(tid));
    assert_eq!(wait_event(&mut rx).await, TransferEvent::Cancelled);
    assert!(!local.path().join("big.bin").exists());
    assert!(std::fs::read_dir(local.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains("termalith-part")
    }));
    let start = Instant::now();
    while transfers.active_count() > 0 {
        assert!(start.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // SFTP is not available on local terminals.
    let local_term = r
        .hub
        .open_local(
            &terminal_pty::default_shell().unwrap(),
            TerminalSize::new(80, 24).unwrap(),
            |_| true,
        )
        .unwrap();
    assert!(matches!(
        r.hub.sftp(local_term).await,
        Err(AppError::Invalid { .. })
    ));
    r.hub.close_all();
}

#[tokio::test(flavor = "multi_thread")]
async fn forgetting_a_changed_key_allows_a_fresh_prompt() {
    let Some(mut r) = rig() else { return };
    let impostor = protocol_ssh::PublicKey::from_openssh(
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2lES7/sb57Bd3YUeuSm4Hzfn0koMxFvyrAdqlrzQwa old",
    )
    .unwrap();
    r.known_hosts
        .add("127.0.0.1", r.sshd.port, &impostor)
        .unwrap();
    let rec = key_session(&r);
    let changed = r
        .connector
        .connect(ConnectRequest::Saved {
            id: rec.id.clone(),
            secret: PasswordInput::None,
        })
        .await;
    assert!(matches!(
        changed,
        Err(AppError::Ssh(protocol_ssh::SshError::HostKeyChanged { .. }))
    ));

    let removed = r
        .connector
        .forget_host_key("127.0.0.1", r.sshd.port)
        .unwrap();
    assert_eq!(removed.removed, 1);

    let c = r.connector.clone();
    let connecting = tokio::spawn(async move {
        c.connect(ConnectRequest::Saved {
            id: rec.id,
            secret: PasswordInput::None,
        })
        .await
    });
    let prompt = r.prompts.recv().await.unwrap();
    assert_eq!(prompt.fingerprint, host_fp(&r));
    r.broker
        .respond(prompt.request_id, HostKeyDecision::AcceptAndSave);
    connecting
        .await
        .unwrap()
        .expect("connect after re-trust")
        .session
        .disconnect()
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_session_without_an_agent_explains_why() {
    let Some(r) = rig() else { return };
    let rec = r
        .connector
        .sessions
        .create(&SessionInput {
            name: "via agent".into(),
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::Agent,
            folder: None,
            jump_via: None,
        })
        .unwrap();
    // The rig's connector has `agent: None`.
    match r
        .connector
        .connect(ConnectRequest::Saved {
            id: rec.id,
            secret: PasswordInput::None,
        })
        .await
    {
        Err(AppError::Invalid { field, reason }) => {
            assert_eq!(field, "ssh-agent");
            assert!(reason.contains("SSH_AUTH_SOCK"), "{reason}");
        }
        other => panic!("expected an explanation, got {:?}", other.err()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn saved_session_behind_a_jump_host() {
    let Some(mut r) = rig() else { return };
    let bastion = key_session(&r);
    let mut behind = SessionInput {
        name: "behind bastion".into(),
        host: "127.0.0.1".into(),
        port: r.sshd.port,
        username: r.sshd.user.clone(),
        auth: AuthMethod::KeyFile {
            path: r.sshd.user_key.clone(),
        },
        folder: None,
        jump_via: Some(bastion.id.clone()),
    };
    let target = r.connector.sessions.create(&behind).unwrap();

    // Answer every host-key prompt (the jump host first, then the target).
    let broker = r.broker.clone();
    let mut prompts = std::mem::replace(&mut r.prompts, mpsc::unbounded_channel().1);
    let answered = Arc::new(AtomicUsize::new(0));
    let a = answered.clone();
    let answering = tokio::spawn(async move {
        while let Some(p) = prompts.recv().await {
            a.fetch_add(1, Ordering::SeqCst);
            broker.respond(p.request_id, HostKeyDecision::AcceptAndSave);
        }
    });

    let connected = r
        .connector
        .connect(ConnectRequest::Saved {
            id: target.id.clone(),
            secret: PasswordInput::None,
        })
        .await
        .unwrap_or_else(|e| panic!("connect via jump: {e}\n{}", r.sshd.log()));
    let out = connected
        .session
        .exec("printf hello-through-bastion")
        .await
        .unwrap();
    assert_eq!(out.stdout, b"hello-through-bastion");
    // Same server for both hops: the key is trusted after the first prompt.
    assert_eq!(answered.load(Ordering::SeqCst), 1);
    connected.session.disconnect().await.unwrap();
    answering.abort();

    // A password jump host without a saved password is explained.
    let pw_bastion = r
        .connector
        .sessions
        .create(&SessionInput {
            name: "pw bastion".into(),
            host: "127.0.0.1".into(),
            port: r.sshd.port,
            username: r.sshd.user.clone(),
            auth: AuthMethod::Password,
            folder: None,
            jump_via: None,
        })
        .unwrap();
    behind.jump_via = Some(pw_bastion.id);
    r.connector.sessions.update(&target.id, &behind).unwrap();
    match r
        .connector
        .connect(ConnectRequest::Saved {
            id: target.id,
            secret: PasswordInput::None,
        })
        .await
    {
        Err(AppError::Invalid { field, reason }) => {
            assert_eq!(field, "jump host");
            assert!(reason.contains("pw bastion"), "{reason}");
        }
        other => panic!("expected jump host explanation, got {:?}", other.err()),
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn split_panes_share_one_connection() {
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let probe = Arc::clone(&session);
    let size = TerminalSize::new(80, 24).unwrap();
    let (sink_a, (out_a, _, _)) = collector();
    let a = r.hub.open_ssh(session, size, sink_a).await.unwrap();
    let (sink_b, (out_b, _, exited_b)) = collector();
    let b = r.hub.open_ssh_sibling(a, size, sink_b).await.unwrap();
    assert_eq!(r.hub.connection_users(a).unwrap(), 2);
    // File browser state is per connection, not per pane.
    assert!(Arc::ptr_eq(
        &r.hub.sftp(a).await.unwrap(),
        &r.hub.sftp(b).await.unwrap()
    ));

    // Independent shells.
    r.hub.write(a, b"echo pane-a-$((1+1))\n").await.unwrap();
    r.hub.write(b, b"echo pane-b-$((2+2))\n").await.unwrap();
    wait_for(&out_a, "pane-a-2").await;
    wait_for(&out_b, "pane-b-4").await;
    assert!(!String::from_utf8_lossy(&out_a.lock().unwrap()).contains("pane-b-4"));

    // Closing one pane keeps the connection for the other.
    r.hub.close(a).await.unwrap();
    assert_eq!(r.hub.connection_users(b).unwrap(), 1);
    r.hub.write(b, b"echo still-$((3+3))\n").await.unwrap();
    wait_for(&out_b, "still-6").await;
    assert_eq!(probe.exec("printf alive").await.unwrap().stdout, b"alive");

    // The last pane ending disconnects the connection.
    r.hub.write(b, b"exit\n").await.unwrap();
    exited_b
        .recv_timeout(Duration::from_secs(10))
        .expect("Exited");
    let start = Instant::now();
    while probe.exec("true").await.is_ok() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "connection was not closed"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(r.hub.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn tunnels_on_a_terminal_connection() {
    use app_core::{TunnelKind, TunnelSpec, Tunnels};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let (sink, _c) = collector();
    let term = r
        .hub
        .open_ssh(session, TerminalSize::new(80, 24).unwrap(), sink)
        .await
        .unwrap();

    let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = echo.accept().await {
            tokio::spawn(async move {
                let (mut rd, mut wr) = s.split();
                let _ = tokio::io::copy(&mut rd, &mut wr).await;
            });
        }
    });

    let tunnels = Tunnels::default();
    let info = tunnels
        .start(
            &r.hub,
            term,
            TunnelSpec {
                kind: TunnelKind::Local {
                    remote_host: "127.0.0.1".into(),
                    remote_port: echo_port,
                },
                local_port: 0,
                allow_remote_clients: false,
            },
        )
        .await
        .unwrap();
    assert!(!info.exposed);
    let mut c = tokio::net::TcpStream::connect(&info.local_address)
        .await
        .unwrap();
    c.write_all(b"tunnel ok").await.unwrap();
    let mut buf = [0u8; 9];
    c.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"tunnel ok");
    assert_eq!(tunnels.list(Some(term)).len(), 1);
    assert_eq!(tunnels.list(Some(term + 999)).len(), 0);

    // Same local port twice is reported clearly.
    let port = info
        .local_address
        .rsplit(':')
        .next()
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let dup = tunnels
        .start(
            &r.hub,
            term,
            TunnelSpec {
                kind: TunnelKind::Socks,
                local_port: port,
                allow_remote_clients: false,
            },
        )
        .await;
    assert!(
        matches!(
            dup,
            Err(AppError::Invalid {
                field: "local port",
                ..
            })
        ),
        "got {:?}",
        dup.err()
    );

    // Tunnels need an SSH terminal.
    let local = r
        .hub
        .open_local(
            &terminal_pty::default_shell().unwrap(),
            TerminalSize::new(80, 24).unwrap(),
            |_| true,
        )
        .unwrap();
    assert!(
        tunnels
            .start(
                &r.hub,
                local,
                TunnelSpec {
                    kind: TunnelKind::Socks,
                    local_port: 0,
                    allow_remote_clients: false
                }
            )
            .await
            .is_err()
    );

    assert_eq!(tunnels.stop_for_terminal(term), 1);
    assert!(tunnels.list(None).is_empty());
    r.hub.close_all();
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_tunnel_through_the_manager() {
    use app_core::{TunnelKind, TunnelSpec, Tunnels};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let Some(r) = rig() else { return };
    let session = trusted_connection(&r).await;
    let (sink, _c) = collector();
    let term = r
        .hub
        .open_ssh(session, TerminalSize::new(80, 24).unwrap(), sink)
        .await
        .unwrap();
    let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = echo.accept().await {
            tokio::spawn(async move {
                let (mut rd, mut wr) = s.split();
                let _ = tokio::io::copy(&mut rd, &mut wr).await;
            });
        }
    });
    let tunnels = Tunnels::default();
    let info = tunnels
        .start(
            &r.hub,
            term,
            TunnelSpec {
                kind: TunnelKind::Remote {
                    target_host: "127.0.0.1".into(),
                    target_port: echo_port,
                },
                local_port: 0,
                allow_remote_clients: false,
            },
        )
        .await
        .unwrap();
    assert!(
        info.local_address.starts_with("server:127.0.0.1:"),
        "{}",
        info.local_address
    );
    assert!(!info.exposed);
    let port: u16 = info
        .local_address
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut c = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    c.write_all(b"R ok").await.unwrap();
    let mut buf = [0u8; 4];
    c.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"R ok");
    assert!(tunnels.stop(info.id));
    r.hub.close_all();
}
