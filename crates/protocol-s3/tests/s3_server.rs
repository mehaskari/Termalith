//! The client against a real S3 protocol implementation: s3s-fs serves a
//! temporary folder as an S3 endpoint in this process (path-style, SigV4
//! with per-run random keys).

use std::net::SocketAddr;

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use protocol_s3::{S3Client, S3Config, S3Error};
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

fn random(tag: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{tag}{n:x}{:x}", std::process::id())
}

/// Starts s3s-fs on 127.0.0.1 with bucket `test`; returns the address,
/// the credentials and the folder (kept alive by the caller).
async fn server() -> (SocketAddr, String, String, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("test")).unwrap();
    let (ak, sk) = (random("AK"), random("sk"));
    let fs = s3s_fs::FileSystem::new(root.path()).unwrap();
    let mut b = S3ServiceBuilder::new(fs);
    b.set_auth(SimpleAuth::from_single(ak.clone(), sk.clone()));
    let service = b.build();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let http = ConnBuilder::new(TokioExecutor::new());
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let conn = http.serve_connection(TokioIo::new(socket), service.clone());
            let conn = conn.into_owned();
            tokio::spawn(async move {
                let _ = conn.await;
            });
        }
    });
    (addr, ak, sk, root)
}

fn config(addr: SocketAddr, ak: &str, sk: &str) -> S3Config {
    S3Config {
        endpoint_host: "127.0.0.1".into(),
        port: addr.port(),
        https: false,
        region: String::new(),
        bucket: "test".into(),
        path_style: true,
        access_key: ak.into(),
        secret_key: Zeroizing::new(sk.into()),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn files_folders_transfers_and_edits() {
    let (addr, ak, sk, _root) = server().await;
    let c = S3Client::connect(&config(addr, &ak, &sk))
        .await
        .expect("connect");
    let cancel = CancellationToken::new();
    let local = tempfile::tempdir().unwrap();

    assert_eq!(c.home().await.unwrap(), "/");
    assert!(c.list("/").await.unwrap().is_empty());

    // An empty folder (holds a hidden .keep object).
    c.create_dir("/empty").await.unwrap();
    assert!(c.create_dir("/empty").await.is_err());
    let after = c.list("/").await.unwrap();
    assert!(
        after.iter().any(|e| e.name == "empty" && e.dir),
        "{after:?}"
    );
    assert!(c.list("/empty").await.unwrap().is_empty());
    let small = local.path().join("small.txt");
    std::fs::write(&small, b"hello s3\n").unwrap();
    assert_eq!(
        c.upload(&small, "/logs/app.txt", false, &cancel, |_| {})
            .await
            .unwrap(),
        9
    );
    assert!(matches!(
        c.upload(&small, "/logs/app.txt", false, &cancel, |_| {})
            .await,
        Err(S3Error::DestinationExists(_))
    ));
    let root = c.list("/").await.unwrap();
    assert!(root.iter().any(|e| e.name == "logs" && e.dir), "{root:?}");
    assert!(root.iter().any(|e| e.name == "empty" && e.dir), "{root:?}");
    let logs = c.list("/logs").await.unwrap();
    assert_eq!(logs.len(), 1, "{logs:?}");
    assert_eq!((logs[0].name.as_str(), logs[0].size), ("app.txt", 9));

    // A multipart upload (just over one part) and its download.
    let big_data: Vec<u8> = (0..9 * 1024 * 1024 + 123)
        .map(|i| (i % 251) as u8)
        .collect();
    let big = local.path().join("big.bin");
    std::fs::write(&big, &big_data).unwrap();
    let mut last = 0;
    let sent = c
        .upload(&big, "/big.bin", false, &cancel, |n| last = n)
        .await
        .unwrap();
    assert_eq!((sent, last), (big_data.len() as u64, big_data.len() as u64));
    let back = local.path().join("back.bin");
    c.download("/big.bin", &back, false, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), big_data);

    // A resumed download continues after the bytes already there.
    let partial = local.path().join(".resume.part");
    std::fs::write(&partial, &big_data[..1000]).unwrap();
    let resumed = local.path().join("resumed.bin");
    let total = c
        .download_resume("/big.bin", &resumed, &partial, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(total, big_data.len() as u64);
    assert_eq!(std::fs::read(&resumed).unwrap(), big_data);

    // Edit: read, replace, and refuse when it changed meanwhile.
    let (text, stat) = c.read_file("/logs/app.txt", 1024).await.unwrap();
    assert_eq!(text, b"hello s3\n");
    let new = c
        .replace_file("/logs/app.txt", b"changed\n", &stat)
        .await
        .unwrap();
    assert_eq!(new.size, Some(8));
    assert!(matches!(
        c.replace_file("/logs/app.txt", b"again\n", &stat).await,
        Err(S3Error::Modified(_))
    ));
    assert!(matches!(
        c.read_file("/big.bin", 10).await,
        Err(S3Error::TooLarge { .. })
    ));

    // Rename a file; folders cannot be renamed; delete file and folder.
    c.rename("/logs/app.txt", "/logs/renamed.txt")
        .await
        .unwrap();
    assert!(c.rename("/logs", "/other").await.is_err());
    assert!(c.delete("/logs").await.is_err(), "not empty");
    c.delete("/logs/renamed.txt").await.unwrap();
    c.delete("/empty").await.unwrap();
    c.delete("/big.bin").await.unwrap();
    assert!(c.list("/").await.unwrap().is_empty());
    assert!(c.list("/../x").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_secret_is_refused() {
    let (addr, ak, sk, _root) = server().await;
    let r = S3Client::connect(&config(addr, &ak, &format!("{sk}x"))).await;
    assert!(r.is_err(), "wrong secret must fail");
}
