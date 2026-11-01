//! Installing a public key on a server (what `ssh-copy-id` does), over the
//! SFTP connection of an open terminal.

use protocol_ssh::{PublicKey, SftpClient, SshError};

use crate::AppError;
use crate::transfers::remote_join;

/// authorized_keys files larger than this are not edited.
const MAX_AUTHORIZED_KEYS: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyInstall {
    Added { path: String },
    AlreadyPresent { path: String },
}

/// The `type base64` part of an authorized_keys line (options and comment
/// ignored), if the line holds `key`.
fn line_has_key(line: &str, key: &PublicKey) -> bool {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return false;
    }
    // Lines may start with options (`from="…",command="…" ssh-ed25519 …`):
    // try every position where a key could begin.
    let words: Vec<&str> = line.split_whitespace().collect();
    (0..words.len()).any(|i| {
        PublicKey::from_openssh(&words[i..].join(" ")).is_ok_and(|k| k.key_data() == key.key_data())
    })
}

/// Appends `key_line` to `home/.ssh/authorized_keys`, creating the folder
/// (0700) and file (0600) if needed. Existing content and permissions are
/// kept; nothing happens if the key is already there.
pub async fn install_public_key(
    sftp: &SftpClient,
    home: &str,
    key_line: &str,
) -> Result<KeyInstall, AppError> {
    let key = PublicKey::from_openssh(key_line.trim())
        .map_err(|e| AppError::invalid("public key", e.to_string()))?;
    let line = key
        .to_openssh()
        .map_err(|e| AppError::invalid("public key", e.to_string()))?;
    let dir = remote_join(home, ".ssh");
    if !sftp.exists(&dir).await? {
        sftp.create_dir(&dir).await?;
        sftp.set_mode(&dir, 0o700).await?;
    }
    let path = remote_join(&dir, "authorized_keys");
    if !sftp.exists(&path).await? {
        sftp.create_file(&path, format!("{line}\n").as_bytes(), 0o600)
            .await?;
        return Ok(KeyInstall::Added { path });
    }
    let (data, stat) = match sftp.read_file(&path, MAX_AUTHORIZED_KEYS).await {
        Ok(x) => x,
        Err(SshError::TooLarge { .. }) => {
            return Err(AppError::invalid(
                "authorized_keys",
                "is larger than 1 MiB; edit it manually",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    let text = String::from_utf8_lossy(&data);
    if text.lines().any(|l| line_has_key(l, &key)) {
        return Ok(KeyInstall::AlreadyPresent { path });
    }
    let mut new = text.into_owned();
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(&line);
    new.push('\n');
    sftp.replace_file(&path, new.as_bytes(), &stat).await?;
    Ok(KeyInstall::Added { path })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2lES7/sb57Bd3YUeuSm4Hzfn0koMxFvyrAdqlrzQwa me@box";

    #[test]
    fn matches_keys_with_options_and_other_comments() {
        let k = PublicKey::from_openssh(KEY).unwrap();
        assert!(line_has_key(KEY, &k));
        assert!(line_has_key(
            "from=\"10.0.0.0/8\",no-pty ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIM2lES7/sb57Bd3YUeuSm4Hzfn0koMxFvyrAdqlrzQwa other",
            &k
        ));
        assert!(!line_has_key("# ssh-ed25519 AAAA", &k));
        assert!(!line_has_key("garbage", &k));
    }
}
