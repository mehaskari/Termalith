//! The servers against a real client: curl speaks TFTP and HTTP. Skipped
//! when curl is not installed (CI runners have it).

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use local_servers::{Action, Activity, ServerConfig, ServerKind, start};

fn curl() -> bool {
    Command::new("curl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Runs curl with `args` off the async runtime; returns (exit ok, stdout).
async fn run_curl(args: Vec<String>) -> (bool, Vec<u8>) {
    tokio::task::spawn_blocking(move || {
        let o = Command::new("curl")
            .args(["-s", "--max-time", "20"])
            .args(&args)
            .output()
            .unwrap();
        (o.status.success(), o.stdout)
    })
    .await
    .unwrap()
}

fn data(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

fn folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("fw")).unwrap();
    std::fs::write(dir.path().join("fw").join("image.bin"), data(300_000)).unwrap();
    std::fs::write(dir.path().join("small.txt"), b"hello\n").unwrap();
    dir
}

type Log = Arc<Mutex<Vec<Activity>>>;

async fn server(kind: ServerKind, root: &Path, uploads: bool) -> (local_servers::Running, Log) {
    server_with_login(kind, root, uploads, None).await
}

async fn server_with_login(
    kind: ServerKind,
    root: &Path,
    uploads: bool,
    ftp_login: Option<(String, String)>,
) -> (local_servers::Running, Log) {
    let log: Log = Arc::default();
    let l = Arc::clone(&log);
    let running = start(
        kind,
        ServerConfig {
            root: root.to_path_buf(),
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 0,
            allow_uploads: uploads,
            ftp_login,
        },
        Arc::new(move |a| l.lock().unwrap().push(a)),
    )
    .await
    .unwrap();
    (running, log)
}

#[tokio::test(flavor = "multi_thread")]
async fn tftp_serves_files_to_curl() {
    if !curl() {
        eprintln!("curl not installed; skipped");
        return;
    }
    let dir = folder();
    let (srv, log) = server(ServerKind::Tftp, dir.path(), false).await;
    let base = format!("tftp://{}", srv.local_addr);

    // Default block size and a negotiated one (curl sends blksize/tsize).
    for extra in [
        vec![],
        vec!["--tftp-blksize".to_string(), "1428".to_string()],
    ] {
        let mut args = extra;
        args.push(format!("{base}/fw/image.bin"));
        let (ok, out) = run_curl(args).await;
        assert!(ok, "download failed");
        assert_eq!(out, data(300_000));
    }
    // A file whose size is a multiple of the block size ends with an empty block.
    std::fs::write(dir.path().join("exact.bin"), data(1024)).unwrap();
    let (ok, out) = run_curl(vec![format!("{base}/exact.bin")]).await;
    assert!(ok && out == data(1024));

    let (ok, _) = run_curl(vec![format!("{base}/missing.bin")]).await;
    assert!(!ok, "missing file must fail");
    // Uploads are off.
    let up = dir.path().join("up.bin");
    std::fs::write(&up, b"x").unwrap();
    let (ok, _) = run_curl(vec![
        "-T".into(),
        up.to_string_lossy().into(),
        format!("{base}/new.bin"),
    ])
    .await;
    assert!(!ok, "upload must be refused");
    assert!(!dir.path().join("new.bin").exists());

    let log = log.lock().unwrap().clone();
    assert!(log.iter().any(|a| a.action == Action::Download
        && a.path == "/fw/image.bin"
        && a.bytes == 300_000
        && a.error.is_none()));
    assert!(
        log.iter()
            .any(|a| a.path == "/missing.bin" && a.error.is_some())
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tftp_accepts_uploads_when_allowed() {
    if !curl() {
        return;
    }
    let dir = folder();
    let (srv, _log) = server(ServerKind::Tftp, dir.path(), true).await;
    let base = format!("tftp://{}", srv.local_addr);
    let src = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(src.path(), data(70_000)).unwrap();
    let path = src.path().to_string_lossy().into_owned();
    let (ok, _) = run_curl(vec![
        "-T".into(),
        path.clone(),
        format!("{base}/fw/backup.cfg"),
    ])
    .await;
    assert!(ok, "upload failed");
    assert_eq!(
        std::fs::read(dir.path().join("fw").join("backup.cfg")).unwrap(),
        data(70_000)
    );
    // Never replaces a file.
    let (ok, _) = run_curl(vec!["-T".into(), path, format!("{base}/small.txt")]).await;
    assert!(!ok);
    assert_eq!(
        std::fs::read(dir.path().join("small.txt")).unwrap(),
        b"hello\n"
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_serves_files_ranges_and_listings() {
    if !curl() {
        return;
    }
    let dir = folder();
    std::fs::write(dir.path().join("a b.txt"), b"spaced").unwrap();
    let (srv, log) = server(ServerKind::Http, dir.path(), false).await;
    let base = format!("http://{}", srv.local_addr);

    let (ok, out) = run_curl(vec!["-f".into(), format!("{base}/fw/image.bin")]).await;
    assert!(ok && out == data(300_000));
    let (ok, out) = run_curl(vec![
        "-f".into(),
        "-r".into(),
        "100-199".into(),
        format!("{base}/fw/image.bin"),
    ])
    .await;
    assert!(ok && out == data(300_000)[100..200]);
    let (ok, out) = run_curl(vec!["-f".into(), format!("{base}/a%20b.txt")]).await;
    assert!(ok && out == b"spaced");
    // Listing, and a folder without its slash redirects.
    let (ok, out) = run_curl(vec!["-f".into(), "-L".into(), format!("{base}/fw")]).await;
    let html = String::from_utf8(out).unwrap();
    assert!(ok && html.contains("href=\"image.bin\""), "{html}");
    let (ok, out) = run_curl(vec!["-f".into(), format!("{base}/")]).await;
    let html = String::from_utf8(out).unwrap();
    assert!(
        ok && html.contains("a%20b.txt") && html.contains("fw/"),
        "{html}"
    );

    // Outside the folder, missing files and uploads are refused.
    for (args, why) in [
        (
            vec![
                "--path-as-is".to_string(),
                format!("{base}/../../etc/passwd"),
            ],
            "traversal",
        ),
        (
            vec![format!("{base}/%2e%2e/%2e%2e/etc/passwd")],
            "encoded traversal",
        ),
        (vec![format!("{base}/missing")], "missing"),
    ] {
        let mut a = vec!["-f".to_string()];
        a.extend(args);
        let (ok, _) = run_curl(a).await;
        assert!(!ok, "{why} must fail");
    }
    let src = tempfile::NamedTempFile::new().unwrap();
    let (ok, _) = run_curl(vec![
        "-f".into(),
        "-T".into(),
        src.path().to_string_lossy().into(),
        format!("{base}/x.bin"),
    ])
    .await;
    assert!(!ok, "uploads are off");

    let log = log.lock().unwrap().clone();
    assert!(
        log.iter()
            .any(|a| a.action == Action::Download && a.path == "/fw/image.bin" && a.bytes == 100)
    );
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn http_accepts_put_uploads_when_allowed() {
    if !curl() {
        return;
    }
    let dir = folder();
    let (srv, _log) = server(ServerKind::Http, dir.path(), true).await;
    let base = format!("http://{}", srv.local_addr);
    let src = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(src.path(), data(123_456)).unwrap();
    let path = src.path().to_string_lossy().into_owned();
    let (ok, _) = run_curl(vec![
        "-f".into(),
        "-T".into(),
        path.clone(),
        format!("{base}/fw/up.bin"),
    ])
    .await;
    assert!(ok);
    assert_eq!(
        std::fs::read(dir.path().join("fw").join("up.bin")).unwrap(),
        data(123_456)
    );
    let (ok, _) = run_curl(vec![
        "-f".into(),
        "-T".into(),
        path,
        format!("{base}/small.txt"),
    ])
    .await;
    assert!(!ok, "existing file is not replaced");
    srv.stop().await;
    // Stopped: nothing listens any more.
    let (ok, _) = run_curl(vec!["-f".into(), format!("{base}/small.txt")]).await;
    assert!(!ok);
}

#[tokio::test(flavor = "multi_thread")]
async fn ftp_accepts_a_chosen_login_only() {
    if !curl() {
        return;
    }
    let dir = folder();
    // A per-run password, so no literal secret sits in the test.
    let password = format!("p{:x}", std::process::id() as u64 * 7919 + 104_729);
    let (srv, _log) = server_with_login(
        ServerKind::Ftp,
        dir.path(),
        false,
        Some(("switch-admin".into(), password.clone())),
    )
    .await;
    assert_eq!(
        srv.credentials,
        Some(("switch-admin".to_string(), password.clone()))
    );
    let base = format!("ftp://{}", srv.local_addr);
    let (ok, out) = run_curl(vec![
        "-u".into(),
        format!("switch-admin:{password}"),
        format!("{base}/small.txt"),
    ])
    .await;
    assert!(ok && out == b"hello\n", "chosen login works");
    // The default user name is not accepted with the chosen password.
    let (ok, _) = run_curl(vec![
        "-u".into(),
        format!("termalith:{password}"),
        format!("{base}/small.txt"),
    ])
    .await;
    assert!(!ok, "only the chosen user");
    srv.stop().await;
}

#[tokio::test]
async fn ftp_rejects_unusable_logins() {
    let dir = folder();
    // Per-run random values, so no literal secret sits in the test.
    let good = local_servers::ftp::random_password();
    let short = good[..5].to_string();
    let spaced = format!(" {good} ");
    let long = good.repeat(6);
    let cases = [
        ("", good.as_str()),
        ("a b", good.as_str()),
        ("root;", good.as_str()),
        ("user", short.as_str()),
        ("user", spaced.as_str()),
        ("user", long.as_str()),
    ];
    for (i, (user, password)) in cases.into_iter().enumerate() {
        let r = start(
            ServerKind::Ftp,
            ServerConfig {
                root: dir.path().to_path_buf(),
                bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 0,
                allow_uploads: false,
                ftp_login: Some((user.into(), password.into())),
            },
            Arc::new(|_| {}),
        )
        .await;
        assert!(r.is_err(), "case {i} must be refused");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn ftp_needs_the_password_and_serves_files() {
    if !curl() {
        return;
    }
    let dir = folder();
    let (srv, log) = server(ServerKind::Ftp, dir.path(), false).await;
    let (user, password) = srv.credentials.clone().expect("FTP has credentials");
    let base = format!("ftp://{}", srv.local_addr);
    let auth = format!("{user}:{password}");

    // Wrong password and anonymous are refused.
    for who in [format!("{user}:wrong"), "anonymous:x".to_string()] {
        let (ok, _) = run_curl(vec!["-u".into(), who, format!("{base}/small.txt")]).await;
        assert!(!ok, "login must fail");
    }
    let (ok, out) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        format!("{base}/fw/image.bin"),
    ])
    .await;
    assert!(ok && out == data(300_000), "download");
    // Resumed download (REST).
    let (ok, out) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        "-C".into(),
        "299000".into(),
        format!("{base}/fw/image.bin"),
    ])
    .await;
    assert!(ok && out == data(300_000)[299_000..], "resume");
    let (ok, out) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        "-l".into(),
        format!("{base}/"),
    ])
    .await;
    let names = String::from_utf8(out).unwrap();
    assert!(
        ok && names.contains("small.txt") && names.contains("fw"),
        "{names}"
    );
    // Read-only: no upload, no delete, no rename.
    let src = tempfile::NamedTempFile::new().unwrap();
    let (ok, _) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        "-T".into(),
        src.path().to_string_lossy().into(),
        format!("{base}/new.bin"),
    ])
    .await;
    assert!(
        !ok && !dir.path().join("new.bin").exists(),
        "upload refused"
    );
    let (ok, _) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        "-Q".into(),
        "DELE small.txt".into(),
        format!("{base}/"),
    ])
    .await;
    assert!(
        !ok && dir.path().join("small.txt").exists(),
        "delete refused"
    );
    // Outside the folder.
    let (ok, _) = run_curl(vec![
        "-u".into(),
        auth,
        "--path-as-is".into(),
        format!("{base}/../../etc/passwd"),
    ])
    .await;
    assert!(!ok, "traversal refused");

    // Transfer notifications arrive just after the client is done.
    let wanted = |log: &[Activity]| {
        log.iter().any(|a| {
            a.action == Action::Download && a.path == "/fw/image.bin" && a.bytes == 300_000
        })
    };
    for _ in 0..50 {
        if wanted(&log.lock().unwrap()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let log = log.lock().unwrap().clone();
    assert!(
        log.iter()
            .any(|a| a.action == Action::Login && a.error.is_some() && a.peer == "127.0.0.1")
    );
    assert!(
        log.iter()
            .any(|a| a.action == Action::Login && a.error.is_none())
    );
    assert!(wanted(&log), "{log:?}");
    srv.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn ftp_accepts_new_files_when_allowed() {
    if !curl() {
        return;
    }
    let dir = folder();
    let (srv, _log) = server(ServerKind::Ftp, dir.path(), true).await;
    let (user, password) = srv.credentials.clone().unwrap();
    let base = format!("ftp://{}", srv.local_addr);
    let auth = format!("{user}:{password}");
    let src = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(src.path(), data(90_000)).unwrap();
    let path = src.path().to_string_lossy().into_owned();
    let (ok, _) = run_curl(vec![
        "-u".into(),
        auth.clone(),
        "--ftp-create-dirs".into(),
        "-T".into(),
        path.clone(),
        format!("{base}/backups/sw1.cfg"),
    ])
    .await;
    assert!(ok, "upload into a new folder");
    assert_eq!(
        std::fs::read(dir.path().join("backups").join("sw1.cfg")).unwrap(),
        data(90_000)
    );
    let (ok, _) = run_curl(vec![
        "-u".into(),
        auth,
        "-T".into(),
        path,
        format!("{base}/small.txt"),
    ])
    .await;
    assert!(!ok, "existing file is not replaced");
    assert_eq!(
        std::fs::read(dir.path().join("small.txt")).unwrap(),
        b"hello\n"
    );
    srv.stop().await;
}
