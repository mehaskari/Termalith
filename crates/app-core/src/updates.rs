//! "Check for updates": compares the running version with the newest
//! published GitHub release and reports where to download it. Nothing is
//! downloaded or installed; drafts are ignored. Only the public releases
//! list is requested, with no identifying data beyond a User-Agent.

use std::sync::Arc;
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::AppError;

const RELEASES_URL: &str = "https://api.github.com/repos/mehaskari/Termalith/releases?per_page=20";
/// Largest response read (the list is a few tens of KiB).
const MAX_BODY: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current: String,
    /// Newest published release, if any could be read.
    pub latest: Option<String>,
    pub newer: bool,
    /// Release page of `latest` (always on github.com).
    pub url: Option<String>,
    pub prerelease: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().trim_start_matches('v')).ok()
}

/// Picks the highest published version from a GitHub releases list and
/// compares it with `current`.
pub fn newest_release(current: &str, releases_json: &str) -> Result<UpdateInfo, AppError> {
    let cur = parse_tag(current)
        .ok_or_else(|| AppError::invalid("version", format!("{current:?} is not a version")))?;
    let releases: Vec<Release> = serde_json::from_str(releases_json)
        .map_err(|e| AppError::Network(format!("unexpected answer from GitHub: {e}")))?;
    let best = releases
        .into_iter()
        .filter(|r| !r.draft && r.html_url.starts_with("https://github.com/"))
        .filter_map(|r| parse_tag(&r.tag_name).map(|v| (v, r)))
        .max_by(|a, b| a.0.cmp(&b.0));
    Ok(match best {
        Some((v, r)) => UpdateInfo {
            current: cur.to_string(),
            latest: Some(v.to_string()),
            newer: v > cur,
            url: Some(r.html_url),
            prerelease: r.prerelease || !v.pre.is_empty(),
        },
        None => UpdateInfo {
            current: cur.to_string(),
            latest: None,
            newer: false,
            url: None,
            prerelease: false,
        },
    })
}

/// Asks GitHub for the releases list (blocking; run off the UI thread).
pub fn check_for_updates(current: &str) -> Result<UpdateInfo, AppError> {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::WebPki)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    let agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(15)))
        .user_agent(format!("Termalith/{current}"))
        .build()
        .new_agent();
    let net = |e: ureq::Error| AppError::Network(format!("could not reach GitHub: {e}"));
    let mut resp = agent
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(net)?;
    let body = resp
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_string()
        .map_err(net)?;
    newest_release(current, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
        {"tag_name": "v0.3.0", "html_url": "https://github.com/x/y/releases/tag/v0.3.0", "draft": true, "prerelease": false},
        {"tag_name": "v0.2.0-rc.1", "html_url": "https://github.com/x/y/releases/tag/v0.2.0-rc.1", "draft": false, "prerelease": true},
        {"tag_name": "v0.1.1", "html_url": "https://github.com/x/y/releases/tag/v0.1.1", "draft": false, "prerelease": false},
        {"tag_name": "nightly", "html_url": "https://github.com/x/y/releases/tag/nightly", "draft": false, "prerelease": true},
        {"tag_name": "v9.0.0", "html_url": "https://evil.example/v9", "draft": false, "prerelease": false}
    ]"#;

    #[test]
    fn newest_published_release_wins_and_drafts_are_ignored() {
        let u = newest_release("0.1.0", LIST).unwrap();
        assert_eq!(u.latest.as_deref(), Some("0.2.0-rc.1"));
        assert!(u.newer);
        assert!(u.prerelease);
        assert!(u.url.unwrap().starts_with("https://github.com/"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        let u = newest_release("0.2.0", LIST).unwrap();
        assert!(!u.newer, "0.2.0 is newer than its own release candidate");
        let u = newest_release("v0.2.0-rc.1", LIST).unwrap();
        assert!(!u.newer);
    }

    #[test]
    fn empty_list_and_bad_input() {
        let u = newest_release("0.1.0", "[]").unwrap();
        assert_eq!((u.latest, u.newer), (None, false));
        assert!(newest_release("0.1.0", "{\"message\": \"rate limited\"}").is_err());
        assert!(newest_release("dev", "[]").is_err());
    }

    /// Talks to the real GitHub API: `cargo test -p app-core live_update_check -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn live_update_check() {
        let u = check_for_updates("0.0.1").unwrap();
        eprintln!("{u:?}");
        assert_eq!(u.current, "0.0.1");
    }
}
