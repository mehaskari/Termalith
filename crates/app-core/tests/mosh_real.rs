//! A Mosh session built by Termalith against the test sshd, with the real
//! `mosh` client and `mosh-server` (installed by the CI step). Skipped
//! unless TERMALITH_TEST_MOSH=1; it adds a Host entry to the CI user's
//! ~/.ssh/config, because ssh reads that file from the account's home.

#![cfg(unix)]

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::{AuthMethod, SessionInput, SessionProtocol, TerminalHub, find_mosh, mosh_command};
use terminal_pty::{TerminalEvent, TerminalSize};
use test_sshd::SshdConfig;

#[tokio::test(flavor = "multi_thread")]
async fn a_command_runs_through_mosh() {
    if std::env::var("TERMALITH_TEST_MOSH").as_deref() != Ok("1") {
        eprintln!("TERMALITH_TEST_MOSH not set; skipped");
        return;
    }
    let mosh = find_mosh().expect("mosh installed");
    let sshd = test_sshd::start(SshdConfig::default()).expect("sshd");

    // ssh finds the key and trusts the host key through ~/.ssh/config.
    let known = sshd.path("mosh_known_hosts");
    std::fs::write(
        &known,
        format!("[127.0.0.1]:{} {}\n", sshd.port, sshd.host_pub.trim()),
    )
    .unwrap();
    let home = std::env::var("HOME").unwrap();
    std::fs::create_dir_all(format!("{home}/.ssh")).unwrap();
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(format!("{home}/.ssh/config"))
        .unwrap();
    writeln!(
        config,
        "\nHost 127.0.0.1\n  IdentityFile {}\n  UserKnownHostsFile {}\n  IdentitiesOnly yes\n",
        sshd.user_key.display(),
        known.display()
    )
    .unwrap();

    let session = SessionInput {
        name: "mosh".into(),
        host: "127.0.0.1".into(),
        port: sshd.port,
        username: sshd.user.clone(),
        auth: AuthMethod::Agent,
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
    };
    let spec = mosh_command(&session, &mosh)
        .unwrap()
        .env("LANG", "C.UTF-8");
    let out = Arc::new(Mutex::new(Vec::new()));
    let o = out.clone();
    let hub = TerminalHub::with_default_flow();
    let id = hub
        .open_local(&spec, TerminalSize::new(100, 30).unwrap(), move |ev| {
            if let TerminalEvent::Output(b) = ev {
                o.lock().unwrap().extend_from_slice(&b);
            }
            true
        })
        .unwrap();

    let text = || String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    // Keep typing until the remote shell answers (Mosh draws only the screen).
    let deadline = Instant::now() + Duration::from_secs(40);
    while Instant::now() < deadline && !text().contains("MOSH_42") {
        let _ = hub.write(id, b"echo MOSH_$((40+2))\r").await;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let seen = text();
    let _ = hub.close(id).await;
    assert!(
        seen.contains("MOSH_42"),
        "no answer through mosh:\n{seen}\n{}",
        sshd.log()
    );
}
