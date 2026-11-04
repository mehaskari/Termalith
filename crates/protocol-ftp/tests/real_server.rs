//! Against a real FTP server (`scripts/ftp-test-server.py` in CI). Skipped
//! unless TERMALITH_TEST_FTP is set to `host:port`, with the login in
//! TERMALITH_TEST_FTP_PASSWORD (generated per run).

use protocol_ftp::{EntryKind, FtpClient, FtpConfig, FtpError, read_certificate};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

/// `suffix` spoils the password for the rejected-login check.
fn config(suffix: &str) -> Option<FtpConfig> {
    let addr = std::env::var("TERMALITH_TEST_FTP").ok()?;
    let password = std::env::var("TERMALITH_TEST_FTP_PASSWORD").ok()?;
    let (host, port) = addr.rsplit_once(':')?;
    Some(FtpConfig {
        host: host.to_owned(),
        port: port.parse().ok()?,
        username: "termalith".into(),
        password: Zeroizing::new(format!("{password}{suffix}")),
        tls_pin: None,
    })
}

#[tokio::test]
async fn files_on_a_real_server() {
    let Some(good) = config("") else {
        eprintln!("TERMALITH_TEST_FTP not set; skipped");
        return;
    };
    let wrong = FtpClient::connect(config("-wrong").unwrap()).await;
    assert!(matches!(wrong, Err(FtpError::AuthFailed)));

    let ftp = FtpClient::connect(good).await.unwrap();
    let cancel = CancellationToken::new();
    let home = ftp.home().await.unwrap();
    assert_eq!(home, "/");

    ftp.create_dir("/a dir").await.unwrap();
    assert!(matches!(
        ftp.create_dir("/a dir").await,
        Err(FtpError::DestinationExists(_))
    ));

    // Binary data survives (every byte value, CR and LF included).
    let local = tempfile::tempdir().unwrap();
    let up = local.path().join("up.bin");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i % 256) as u8).collect();
    std::fs::write(&up, &data).unwrap();
    let mut seen = 0;
    let n = ftp
        .upload(&up, "/a dir/up.bin", false, &cancel, |p| seen = p)
        .await
        .unwrap();
    assert_eq!((n, seen), (300_000, 300_000));
    assert!(matches!(
        ftp.upload(&up, "/a dir/up.bin", false, &cancel, |_| {})
            .await,
        Err(FtpError::DestinationExists(_))
    ));
    ftp.upload(&up, "/a dir/up.bin", true, &cancel, |_| {})
        .await
        .unwrap();

    let down = local.path().join("down.bin");
    ftp.download("/a dir/up.bin", &down, false, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&down).unwrap(), data);

    let listed = ftp.list("/a dir").await.unwrap();
    assert_eq!(listed.len(), 1, "no temporary files left: {listed:?}");
    assert_eq!(listed[0].name, "up.bin");
    assert_eq!((listed[0].kind, listed[0].size), (EntryKind::File, 300_000));
    assert!(listed[0].modified.is_some());

    // A cancelled transfer leaves nothing behind, on either side.
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        ftp.upload(&up, "/a dir/gone.bin", false, &cancelled, |_| {})
            .await,
        Err(FtpError::Cancelled)
    ));
    assert_eq!(ftp.list("/a dir").await.unwrap().len(), 1);
    let gone = local.path().join("gone.bin");
    assert!(matches!(
        ftp.download("/a dir/up.bin", &gone, false, &cancelled, |_| {})
            .await,
        Err(FtpError::Cancelled)
    ));
    assert_eq!(std::fs::read_dir(local.path()).unwrap().count(), 2);

    // The editor: read, save, and refuse if someone else changed the file.
    std::fs::write(&up, b"one").unwrap();
    ftp.upload(&up, "/a dir/text.txt", false, &cancel, |_| {})
        .await
        .unwrap();
    let (text, version) = ftp.read_file("/a dir/text.txt", 100).await.unwrap();
    assert_eq!(text, b"one");
    assert!(matches!(
        ftp.read_file("/a dir/text.txt", 2).await,
        Err(FtpError::TooLarge { limit: 2 })
    ));
    let saved = ftp
        .replace_file("/a dir/text.txt", b"two!", &version)
        .await
        .unwrap();
    assert_eq!(saved.size, Some(4));
    assert!(matches!(
        ftp.replace_file("/a dir/text.txt", b"three", &version)
            .await,
        Err(FtpError::Modified(_))
    ));
    assert_eq!(
        ftp.read_file("/a dir/text.txt", 100).await.unwrap().0,
        b"two!"
    );

    ftp.rename("/a dir/text.txt", "/a dir/renamed.txt")
        .await
        .unwrap();
    assert!(matches!(
        ftp.rename("/a dir/renamed.txt", "/a dir/up.bin").await,
        Err(FtpError::DestinationExists(_))
    ));
    ftp.delete("/a dir/renamed.txt").await.unwrap();
    ftp.delete("/a dir/up.bin").await.unwrap();
    ftp.delete("/a dir").await.unwrap();
    assert!(ftp.list("/").await.unwrap().is_empty());
    assert!(matches!(
        ftp.delete("/x\r\nDELE y").await,
        Err(FtpError::InvalidPath)
    ));
}

/// TERMALITH_TEST_FTPS is an FTPS server that requires TLS, and
/// TERMALITH_TEST_FTPS_SHA256 its certificate's fingerprint in hex.
#[tokio::test]
async fn ftps_accepts_only_the_pinned_certificate() {
    let (Ok(addr), Ok(expected), Some(base)) = (
        std::env::var("TERMALITH_TEST_FTPS"),
        std::env::var("TERMALITH_TEST_FTPS_SHA256"),
        config(""),
    ) else {
        eprintln!("TERMALITH_TEST_FTPS not set; skipped");
        return;
    };
    let (host, port) = addr.rsplit_once(':').unwrap();
    let port: u16 = port.parse().unwrap();
    let pin = read_certificate(host, port).await.unwrap();
    let hex: String = pin.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, expected.to_lowercase());

    let secure = |tls_pin| FtpConfig {
        host: host.to_owned(),
        port,
        username: base.username.clone(),
        password: base.password.clone(),
        tls_pin,
    };
    // Wrong certificate: refused before the password is sent, no fallback.
    let wrong = FtpClient::connect(secure(Some([0; 32]))).await;
    assert!(matches!(wrong, Err(FtpError::Tls(_))), "{:?}", wrong.err());
    // The server requires TLS, so plain FTP does not get in either.
    assert!(FtpClient::connect(secure(None)).await.is_err());

    let ftp = FtpClient::connect(secure(Some(pin))).await.unwrap();
    let cancel = CancellationToken::new();
    let local = tempfile::tempdir().unwrap();
    let up = local.path().join("up.bin");
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&up, &data).unwrap();
    // Listing, upload and download use encrypted data connections.
    ftp.upload(&up, "/tls.bin", false, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(ftp.list("/").await.unwrap().len(), 1);
    let down = local.path().join("down.bin");
    ftp.download("/tls.bin", &down, false, &cancel, |_| {})
        .await
        .unwrap();
    assert_eq!(std::fs::read(&down).unwrap(), data);
    ftp.delete("/tls.bin").await.unwrap();
}

/// A server without TLS must fail, not silently continue unencrypted.
#[tokio::test]
async fn a_plain_server_is_not_accepted_as_ftps() {
    let Some(plain) = config("") else {
        eprintln!("TERMALITH_TEST_FTP not set; skipped");
        return;
    };
    assert!(read_certificate(&plain.host, plain.port).await.is_err());
}
