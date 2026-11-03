//! Embedded RDP against the Windows Remote Desktop service of the CI
//! runner (NLA/CredSSP, a real Windows certificate, Display Control).
//! Skipped unless the workflow sets TERMALITH_TEST_WINRDP.

#![cfg(all(windows, feature = "embedded"))]

use std::time::{Duration, Instant};

use remote_desktop::embedded::{LoginParams, ViewerEvent, capture, start};
use remote_desktop::read_certificate;
use zeroize::Zeroizing;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[test]
fn embedded_rdp_against_windows() {
    let Some(addr) = env("TERMALITH_TEST_WINRDP") else {
        eprintln!("TERMALITH_TEST_WINRDP not set; skipped");
        return;
    };
    let (host, port) = addr.rsplit_once(':').expect("host:port");
    let port: u16 = port.parse().expect("port");
    let user = env("TERMALITH_TEST_WINRDP_USER").expect("user");
    let password = env("TERMALITH_TEST_WINRDP_PASSWORD").expect("password");
    let domain = env("TERMALITH_TEST_WINRDP_DOMAIN");

    let pin = read_certificate(host, port, host, Duration::from_secs(10)).unwrap();
    eprintln!("windows rdp certificate: {}", pin.display());
    let params = || LoginParams {
        host,
        port,
        server_name: host,
        username: &user,
        password: Zeroizing::new(password.clone()),
        domain: domain.as_deref(),
        pin,
        width: 1280,
        height: 720,
    };

    let stats = match capture(&params(), Duration::from_secs(15)) {
        Ok(s) => s,
        Err(e) => {
            // Diagnose: the same login without a domain name.
            let mut p = params();
            p.domain = None;
            let second = capture(&p, Duration::from_secs(15));
            panic!("capture failed: {e}; without a domain: {second:?}");
        }
    };
    eprintln!("windows embedded RDP: {stats:?}");
    assert!(stats.graphics_updates > 0, "no graphics updates");

    let (tx, rx) = std::sync::mpsc::channel();
    let viewer = start(&params(), move |ev| tx.send(ev).is_ok()).unwrap();
    let mut frames = 0;
    let mut sizes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut asked_resize = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ViewerEvent::Connected { width, height }) => sizes.push((width, height)),
            Ok(ViewerEvent::Frame { .. }) => {
                frames += 1;
                viewer.ack();
                if frames == 20 && !asked_resize {
                    asked_resize = true;
                    viewer.clipboard("from termalith".into());
                    viewer.resize(1024, 768);
                }
            }
            Ok(ViewerEvent::Clipboard(text)) => eprintln!("remote clipboard: {} chars", text.len()),
            Ok(ViewerEvent::Closed { reason }) => panic!("closed: {reason:?}"),
            Err(_) => {}
        }
    }
    eprintln!("windows viewer: {frames} frames, sizes {sizes:?}");
    assert!(frames > 0, "no frames");
    viewer.close();
}
