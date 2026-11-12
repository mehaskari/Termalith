//! The bandwidth test against a real iperf3 server. CI starts one
//! (`iperf3 -s -D`) and sets TERMALITH_TEST_IPERF; elsewhere this skips.

use network_tools::{IPERF_PORT, bandwidth_test};
use tokio_util::sync::CancellationToken;

#[tokio::test(flavor = "multi_thread")]
async fn upload_and_download_against_iperf3() {
    let Ok(host) = std::env::var("TERMALITH_TEST_IPERF") else {
        eprintln!("SKIPPED: TERMALITH_TEST_IPERF not set");
        return;
    };
    let cancel = CancellationToken::new();
    for download in [false, true] {
        let mut samples = Vec::new();
        let r = bandwidth_test(&host, IPERF_PORT, 2, download, &cancel, |s| samples.push(s))
            .await
            .unwrap_or_else(|e| panic!("download={download}: {e}"));
        eprintln!(
            "download={download}: {:.0} Mbit/s over {:.2} s",
            r.bits_per_second / 1e6,
            r.seconds
        );
        assert!(r.bytes > 1_000_000, "moved too little: {r:?}");
        assert!(r.seconds >= 1.9, "{r:?}");
        assert!(!samples.is_empty());
        // The server accepts the next test once this one is closed.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
