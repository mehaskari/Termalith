//! Remote desktop against real servers, set up by the Linux CI workflow:
//! xrdp with its own certificate and a TigerVNC server. Each test is
//! skipped unless its environment variables are set.

#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use remote_desktop::{
    CertFingerprint, Client, DesktopTarget, Os, Protocol, launch, plan_for, read_certificate,
};
use zeroize::Zeroizing;

/// FreeRDP 3 exit status when the TLS connection fails, which is what a
/// refused certificate produces (measured with sdl-freerdp 3.32).
const FREERDP_TLS_FAILED: i32 = 143;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn host_port(s: &str) -> (String, u16) {
    let (h, p) = s.rsplit_once(':').expect("host:port");
    (h.to_owned(), p.parse().expect("port"))
}

fn target(protocol: Protocol, host: &str, port: u16, user: Option<&str>) -> DesktopTarget {
    DesktopTarget {
        protocol,
        host: host.into(),
        port,
        username: user.map(Into::into),
        domain: None,
    }
}

/// Runs the plan and returns the exit code if the client ends within
/// `within`, or `None` if it is still running (then it is stopped).
async fn exit_within(
    plan: &remote_desktop::LaunchPlan,
    password: Option<&str>,
    within: Duration,
) -> Option<i32> {
    let mut l = launch(plan, password.map(|p| Zeroizing::new(p.into())), "client")
        .await
        .expect("client starts");
    let child = l.child.as_mut().expect("tracked client");
    let start = Instant::now();
    while start.elapsed() < within {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status.code().unwrap_or(-1));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    None
}

#[tokio::test]
async fn xrdp_certificate_is_read_and_freerdp_accepts_only_it() {
    let Some(addr) = env("TERMALITH_TEST_XRDP") else {
        eprintln!("TERMALITH_TEST_XRDP not set; skipped");
        return;
    };
    let (host, port) = host_port(&addr);
    let expected = env("TERMALITH_TEST_XRDP_SHA256").expect("expected fingerprint");

    let fp = read_certificate(&host, port, &host, Duration::from_secs(10)).unwrap();
    assert_eq!(fp.hex(), expected.trim().to_ascii_lowercase());

    let Some(program) = env("TERMALITH_FREERDP") else {
        eprintln!("TERMALITH_FREERDP not set; FreeRDP part skipped");
        return;
    };
    let client = Client {
        id: "xfreerdp",
        name: "FreeRDP",
        protocol: Protocol::Rdp,
        program: program.into(),
    };
    // No real account: xrdp shows its login screen, which keeps the
    // connection open; what matters is that TLS was accepted.
    let t = target(Protocol::Rdp, &host, port, Some("nobody"));

    let wrong = plan_for(
        Os::Linux,
        &client,
        &t,
        Path::new("/tmp"),
        Some(CertFingerprint::of(b"not the server's certificate")),
    )
    .unwrap();
    assert_eq!(
        exit_within(&wrong, Some("x"), Duration::from_secs(20)).await,
        Some(FREERDP_TLS_FAILED),
        "a certificate that was not pinned must be refused"
    );

    let right = plan_for(Os::Linux, &client, &t, Path::new("/tmp"), Some(fp)).unwrap();
    let code = exit_within(&right, Some("x"), Duration::from_secs(10)).await;
    assert_ne!(
        code,
        Some(FREERDP_TLS_FAILED),
        "the pinned certificate must be accepted"
    );
    eprintln!("FreeRDP with the pinned certificate: {code:?} (None = still connected)");
}

#[tokio::test]
async fn tigervnc_viewer_connects_with_the_launch_plan() {
    let Some(addr) = env("TERMALITH_TEST_VNC") else {
        eprintln!("TERMALITH_TEST_VNC not set; skipped");
        return;
    };
    let viewer = env("TERMALITH_VNCVIEWER").expect("viewer path");
    let log = env("TERMALITH_TEST_VNC_LOG").expect("server log path");
    let (host, port) = host_port(&addr);
    let client = Client {
        id: "vncviewer",
        name: "TigerVNC Viewer",
        protocol: Protocol::Vnc,
        program: viewer.into(),
    };
    let plan = plan_for(
        Os::Linux,
        &client,
        &target(Protocol::Vnc, &host, port, None),
        Path::new("/tmp"),
        None,
    )
    .unwrap();
    let before = std::fs::read_to_string(&log).unwrap_or_default().len();
    let code = exit_within(&plan, None, Duration::from_secs(8)).await;
    assert_eq!(code, None, "the viewer should stay connected");
    let after = std::fs::read_to_string(&log).unwrap();
    assert!(
        after[before..]
            .to_ascii_lowercase()
            .contains("connections: accepted"),
        "server log after launch:\n{}",
        &after[before..]
    );
}

/// ADR-030: IronRDP logs in to the real xrdp with the pinned certificate,
/// decodes the desktop, and a live viewer session streams frames, takes
/// input and closes cleanly.
#[cfg(feature = "embedded")]
#[test]
fn embedded_rdp_against_xrdp() {
    use remote_desktop::embedded::{LoginParams, ViewerEvent, ViewerInput, capture, start};
    let Some(addr) = env("TERMALITH_TEST_XRDP") else {
        eprintln!("TERMALITH_TEST_XRDP not set; skipped");
        return;
    };
    let (host, port) = host_port(&addr);
    let user = env("TERMALITH_TEST_SSH_USER").unwrap_or_else(|| "nobody".into());
    let password = env("TERMALITH_TEST_SSH_PASSWORD").unwrap_or_default();
    let pin = read_certificate(&host, port, &host, Duration::from_secs(10)).unwrap();
    let params = |pin| LoginParams {
        host: &host,
        port,
        server_name: &host,
        username: &user,
        password: Zeroizing::new(password.clone()),
        domain: None,
        pin,
        width: 1024,
        height: 768,
    };

    let wrong = capture(
        &params(CertFingerprint::of(b"other")),
        Duration::from_secs(2),
    );
    assert!(
        matches!(wrong, Err(remote_desktop::DesktopError::Certificate(_))),
        "{wrong:?}"
    );

    let stats = capture(&params(pin), Duration::from_secs(8)).unwrap();
    eprintln!("embedded RDP: {stats:?}");
    assert!(stats.graphics_updates > 0, "no graphics updates");
    assert!(stats.drawn_pixels > 0, "nothing was drawn");

    // Live session: frames arrive, input is accepted, close ends it.
    let (tx, rx) = std::sync::mpsc::channel();
    let viewer = start(&params(pin), move |ev| tx.send(ev).is_ok()).unwrap();
    let first = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        matches!(
            first,
            ViewerEvent::Connected {
                width: 1024,
                height: 768
            }
        ),
        "{first:?}"
    );
    let mut frames = 0;
    let mut bytes = 0;
    let deadline = Instant::now() + Duration::from_secs(6);
    viewer.input(vec![
        ViewerInput::MouseMove { x: 500, y: 400 },
        ViewerInput::Key {
            scancode: 0x1E,
            down: true,
        },
        ViewerInput::Key {
            scancode: 0x1E,
            down: false,
        },
        ViewerInput::ReleaseAll,
    ]);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ViewerEvent::Frame { w, h, rgba, .. }) => {
                assert_eq!(rgba.len(), usize::from(w) * usize::from(h) * 4);
                frames += 1;
                bytes += rgba.len();
                viewer.ack();
            }
            Ok(ViewerEvent::Closed { reason }) => panic!("closed early: {reason:?}"),
            Ok(other) => panic!("unexpected {other:?}"),
            Err(_) => {}
        }
    }
    eprintln!("viewer: {frames} frames, {bytes} bytes in 6 s");
    assert!(frames > 0, "no frames");

    // Resize through Display Control; servers without it keep the size.
    viewer.resize(800, 600);
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut resized = None;
    while Instant::now() < deadline && resized.is_none() {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ViewerEvent::Connected { width, height }) => resized = Some((width, height)),
            Ok(ViewerEvent::Frame { .. }) => {
                viewer.ack();
            }
            Ok(ViewerEvent::Closed { reason }) => panic!("closed during resize: {reason:?}"),
            Err(_) => {}
        }
    }
    eprintln!("viewer resize to 800x600: {resized:?} (None = server kept its size)");
    viewer.close();
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(ViewerEvent::Closed { .. }) => break,
            Ok(_) => continue,
            Err(e) => panic!("no Closed event: {e}"),
        }
    }
}
