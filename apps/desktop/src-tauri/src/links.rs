//! Opening web links found in terminal output. The text is untrusted, so
//! only plain http(s) URLs are accepted and they are handed to the OS's
//! default-browser opener as a single argument (no shell).

use crate::error::IpcError;

const MAX_URL: usize = 2048;

/// An http(s) URL made of printable ASCII without characters that are
/// never valid unescaped in a URL.
pub fn valid_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = match lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    {
        Some(r) => r,
        None => return false,
    };
    url.len() <= MAX_URL
        && !rest.is_empty()
        && !rest.starts_with('/')
        && url.chars().all(|c| {
            c.is_ascii_graphic()
                && !matches!(c, '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '}' | '|')
        })
}

/// Opens `url` in the default browser.
#[tauri::command]
pub fn open_web_link(url: String) -> Result<(), IpcError> {
    if !valid_web_url(&url) {
        return Err(IpcError::coded(
            "invalid",
            "only http and https links can be opened",
        ));
    }
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("/usr/bin/open", vec![url.as_str()])
    } else if cfg!(windows) {
        ("explorer.exe", vec![url.as_str()])
    } else {
        ("xdg-open", vec![url.as_str()])
    };
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| IpcError::coded("open_failed", format!("could not open the link: {e}")))?;
    Ok(())
}

/// macOS: opens System Settings at Privacy & Security → Local Network.
/// Fixed URL, no input.
#[tauri::command]
pub fn open_local_network_settings() -> Result<(), IpcError> {
    if !cfg!(target_os = "macos") {
        return Err(IpcError::coded(
            "unsupported",
            "only macOS has a Local Network setting",
        ));
    }
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_LocalNetwork")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            IpcError::coded(
                "open_failed",
                format!("could not open System Settings: {e}"),
            )
        })?;
    Ok(())
}

/// Restarts Termalith (a changed Local Network permission applies to a
/// new process). Open sessions are closed as on quit.
#[tauri::command]
pub fn app_restart(app: tauri::AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::valid_web_url;

    #[test]
    fn only_plain_web_urls() {
        for ok in [
            "https://example.com",
            "http://10.0.0.1:8080/a?b=c#d",
            "HTTPS://Example.com/%20x",
        ] {
            assert!(valid_web_url(ok), "{ok}");
        }
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ftp://x",
            "https://",
            "https:///path",
            "https://a b",
            "https://a\"b",
            "https://a\nb",
            "https://exämple.com",
            "-https://x",
            "smb://server/share",
        ] {
            assert!(!valid_web_url(bad), "{bad:?}");
        }
        assert!(!valid_web_url(&format!("https://{}", "a".repeat(3000))));
    }
}
