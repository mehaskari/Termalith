//! The proxy password lives in the secret store, is attached to the proxy
//! only when a user name is set, and can be replaced or removed.

use std::sync::Arc;
use std::time::Duration;

use app_core::{Connector, PromptBroker, ProxySetting, ProxyType, SessionStore, Settings};
use credential_store::MemorySecretStore;
use protocol_ssh::FileKnownHosts;
use zeroize::Zeroizing;

fn connector(dir: &std::path::Path) -> Connector {
    Connector {
        sessions: Arc::new(SessionStore::open_in_memory().unwrap()),
        secrets: Arc::new(MemorySecretStore::default()),
        known_hosts: Arc::new(FileKnownHosts::new(dir.join("kh"))),
        broker: Arc::new(PromptBroker::new(|_| {}, Duration::from_secs(1))),
        timeout: Duration::from_secs(5),
        agent: None,
        kbd: None,
    }
}

#[tokio::test]
async fn proxy_password_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let c = connector(dir.path());
    assert!(c.proxy().await.unwrap().is_none(), "no proxy configured");

    let mut s = Settings {
        proxy: Some(ProxySetting {
            kind: ProxyType::Socks5,
            host: "proxy.corp".into(),
            port: 1080,
            username: Some(" alice ".into()),
        }),
        ..Settings::default()
    };
    c.sessions.save_settings(&s).unwrap();

    // User name without a saved password: empty password.
    let p = c.proxy().await.unwrap().unwrap();
    let auth = p.auth.unwrap();
    assert_eq!(
        (auth.username.as_str(), auth.password.as_str()),
        ("alice", "")
    );
    assert!(!c.has_proxy_password().unwrap());

    c.set_proxy_password(Some(Zeroizing::new("s3cret".into())))
        .await
        .unwrap();
    assert!(c.has_proxy_password().unwrap());
    let settings_json = serde_json::to_string(&c.sessions.settings().unwrap()).unwrap();
    assert!(!settings_json.contains("s3cret"), "never in the settings");
    assert_eq!(
        c.proxy()
            .await
            .unwrap()
            .unwrap()
            .auth
            .unwrap()
            .password
            .as_str(),
        "s3cret"
    );

    c.set_proxy_password(Some(Zeroizing::new("new".into())))
        .await
        .unwrap();
    assert_eq!(
        c.proxy()
            .await
            .unwrap()
            .unwrap()
            .auth
            .unwrap()
            .password
            .as_str(),
        "new"
    );

    // Without a user name the password is not sent.
    s.proxy.as_mut().unwrap().username = None;
    c.sessions.save_settings(&s).unwrap();
    assert!(c.proxy().await.unwrap().unwrap().auth.is_none());

    c.set_proxy_password(None).await.unwrap();
    assert!(!c.has_proxy_password().unwrap());
}
