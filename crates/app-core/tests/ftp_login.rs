//! FTP and FTPS sessions through the connector, against the real servers of
//! `scripts/ftp-test-server.py`. Skipped unless the CI step sets
//! TERMALITH_TEST_FTP and TERMALITH_TEST_FTPS.

use std::sync::Arc;
use std::time::Duration;

use app_core::{
    AppError, AuthMethod, Connector, FTPS_CERT_ALGORITHM, HostKeyPrompt, PasswordInput,
    PromptBroker, ProxySetting, ProxyType, SessionInput, SessionProtocol, SessionStore, Settings,
};
use credential_store::MemorySecretStore;
use protocol_ssh::{FileKnownHosts, HostKeyDecision};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

struct Rig {
    connector: Connector,
    prompts: mpsc::UnboundedReceiver<HostKeyPrompt>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (tx, prompts) = mpsc::unbounded_channel();
    let connector = Connector {
        sessions: Arc::new(SessionStore::open_in_memory().unwrap()),
        secrets: Arc::new(MemorySecretStore::default()),
        known_hosts: Arc::new(FileKnownHosts::new(dir.path().join("kh"))),
        broker: Arc::new(PromptBroker::new(
            move |p| {
                let _ = tx.send(p);
            },
            Duration::from_secs(10),
        )),
        timeout: Duration::from_secs(5),
        agent: None,
        kbd: None,
    };
    Rig {
        connector,
        prompts,
        _dir: dir,
    }
}

fn session(addr: &str, protocol: SessionProtocol) -> SessionInput {
    let (host, port) = addr.rsplit_once(':').unwrap();
    SessionInput {
        name: "files".into(),
        host: host.to_owned(),
        port: port.parse().unwrap(),
        username: "termalith".into(),
        auth: AuthMethod::Password,
        folder: None,
        jump_via: None,
        protocol,
        domain: None,
        tags: Vec::new(),
        forward_agent: false,
        x11_forward: false,
        startup_command: None,
        files_only: false,
    }
}

fn typed(password: &str, save: bool) -> PasswordInput {
    PasswordInput::Provided {
        secret: Zeroizing::new(password.to_owned()),
        save,
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// A proxy promises that connections go through it, so FTP must not dial
/// around it. Refused before any network use.
#[tokio::test]
async fn ftp_does_not_bypass_a_configured_proxy() {
    let r = rig();
    let id = r
        .connector
        .sessions
        .create(&session("127.0.0.1:1", SessionProtocol::Ftp))
        .unwrap()
        .id;
    let settings = Settings {
        proxy: Some(ProxySetting {
            kind: ProxyType::Socks5,
            host: "proxy.corp".into(),
            port: 1080,
            username: None,
        }),
        ..Settings::default()
    };
    r.connector.sessions.save_settings(&settings).unwrap();
    assert!(matches!(
        r.connector.connect_ftp(&id, typed("x", false)).await,
        Err(AppError::Invalid { field: "proxy", .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn ftp_password_is_saved_only_after_a_successful_login() {
    let (Some(addr), Some(password)) = (
        env("TERMALITH_TEST_FTP"),
        env("TERMALITH_TEST_FTP_PASSWORD"),
    ) else {
        eprintln!("TERMALITH_TEST_FTP not set; skipped");
        return;
    };
    let r = rig();
    let id = r
        .connector
        .sessions
        .create(&session(&addr, SessionProtocol::Ftp))
        .unwrap()
        .id;
    assert!(matches!(
        r.connector.connect_ftp(&id, PasswordInput::None).await,
        Err(AppError::PasswordRequired)
    ));
    let wrong = format!("{password}-wrong");
    assert!(
        r.connector
            .connect_ftp(&id, typed(&wrong, true))
            .await
            .is_err()
    );
    assert!(!r.connector.sessions.get(&id).unwrap().has_saved_secret);

    let (client, label) = r
        .connector
        .connect_ftp(&id, typed(&password, true))
        .await
        .unwrap();
    assert_eq!(label, "files");
    assert_eq!(client.home().await.unwrap(), "/");
    assert!(r.connector.sessions.get(&id).unwrap().has_saved_secret);
    // From the saved secret, without asking.
    r.connector
        .connect_ftp(&id, PasswordInput::None)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ftps_certificate_is_asked_once_and_can_be_refused() {
    let (Some(addr), Some(password)) = (
        env("TERMALITH_TEST_FTPS"),
        env("TERMALITH_TEST_FTP_PASSWORD"),
    ) else {
        eprintln!("TERMALITH_TEST_FTPS not set; skipped");
        return;
    };
    let mut r = rig();
    let id = r
        .connector
        .sessions
        .create(&session(&addr, SessionProtocol::Ftps))
        .unwrap()
        .id;
    let connector = Arc::new(r.connector);

    // Refused: the login fails and nothing is saved.
    let attempt = {
        let (connector, id, password) = (Arc::clone(&connector), id.clone(), password.clone());
        tokio::spawn(async move { connector.connect_ftp(&id, typed(&password, true)).await })
    };
    let prompt = r.prompts.recv().await.unwrap();
    assert_eq!(prompt.algorithm, FTPS_CERT_ALGORITHM);
    assert!(
        connector
            .broker
            .respond(prompt.request_id, HostKeyDecision::Reject)
    );
    assert!(attempt.await.unwrap().is_err());
    assert!(!connector.sessions.get(&id).unwrap().has_saved_secret);

    // Accepted and saved: no second question.
    let attempt = {
        let (connector, id, password) = (Arc::clone(&connector), id.clone(), password.clone());
        tokio::spawn(async move { connector.connect_ftp(&id, typed(&password, true)).await })
    };
    let prompt = r.prompts.recv().await.unwrap();
    assert!(
        connector
            .broker
            .respond(prompt.request_id, HostKeyDecision::AcceptAndSave)
    );
    let (client, _) = attempt.await.unwrap().unwrap();
    assert!(client.list("/").await.is_ok());
    connector
        .connect_ftp(&id, PasswordInput::None)
        .await
        .unwrap();
    assert!(r.prompts.try_recv().is_err(), "asked twice");
}
