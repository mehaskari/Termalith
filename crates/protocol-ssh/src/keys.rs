//! SSH key files: generate Ed25519 key pairs and list existing keys.
//!
//! Private keys are written in OpenSSH format with mode 0600, optionally
//! encrypted (AES-256-CTR, bcrypt-pbkdf, as `ssh-keygen` does). Existing
//! files are never overwritten.

use std::path::{Path, PathBuf};

use russh::keys::ssh_key::LineEnding;
use russh::keys::{Algorithm, HashAlg, PrivateKey, PublicKey};

use crate::SshError;

const MAX_KEY_FILE: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// File name of the public key without `.pub` (e.g. `id_ed25519`).
    pub name: String,
    pub private_path: Option<PathBuf>,
    pub public_path: PathBuf,
    pub algorithm: String,
    pub fingerprint: String,
    pub comment: String,
    /// The private key needs a passphrase (unknown if it is missing).
    pub encrypted: Option<bool>,
}

/// A name usable as a file name in `~/.ssh` (no paths, no hidden files).
fn valid_name(name: &str) -> Result<(), SshError> {
    let ok = !name.is_empty()
        && name.len() <= 100
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'));
    if ok {
        Ok(())
    } else {
        Err(SshError::Key(
            "use letters, digits, '-', '_', '.' or '@' for the key name".into(),
        ))
    }
}

fn write_new(path: &Path, data: &[u8], mode: u32) -> Result<(), SshError> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut f = opts.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => {
            SshError::DestinationExists(path.display().to_string())
        }
        _ => SshError::Io(e),
    })?;
    f.write_all(data)?;
    f.sync_all()?;
    Ok(())
}

/// Generates an Ed25519 key pair as `dir/name` and `dir/name.pub`.
pub fn generate_ed25519(
    dir: &Path,
    name: &str,
    comment: &str,
    passphrase: Option<&str>,
) -> Result<KeyInfo, SshError> {
    valid_name(name)?;
    if comment.chars().any(char::is_control) || comment.len() > 200 {
        return Err(SshError::Key(
            "the comment is too long or has control characters".into(),
        ));
    }
    let private_path = dir.join(name);
    let public_path = dir.join(format!("{name}.pub"));
    if private_path.exists() || public_path.exists() {
        return Err(SshError::DestinationExists(
            private_path.display().to_string(),
        ));
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // A new ~/.ssh must not be readable by others (sshd checks).
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let mut rng = rand::rng();
    let mut key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .map_err(|e| SshError::Key(e.to_string()))?;
    key.set_comment(comment);
    let stored = match passphrase.filter(|p| !p.is_empty()) {
        Some(p) => key
            .encrypt(&mut rng, p.as_bytes())
            .map_err(|e| SshError::Key(e.to_string()))?,
        None => key.clone(),
    };
    let private = stored
        .to_openssh(LineEnding::LF)
        .map_err(|e| SshError::Key(e.to_string()))?;
    let public = key
        .public_key()
        .to_openssh()
        .map_err(|e| SshError::Key(e.to_string()))?;
    write_new(&private_path, private.as_bytes(), 0o600)?;
    if let Err(e) = write_new(&public_path, format!("{public}\n").as_bytes(), 0o644) {
        let _ = std::fs::remove_file(&private_path);
        return Err(e);
    }
    Ok(KeyInfo {
        name: name.to_owned(),
        private_path: Some(private_path),
        public_path,
        algorithm: key.algorithm().to_string(),
        fingerprint: key.public_key().fingerprint(HashAlg::Sha256).to_string(),
        comment: comment.to_owned(),
        encrypted: Some(stored.is_encrypted()),
    })
}

/// Reads and parses a public key file (one OpenSSH key line).
pub fn read_public_key(path: &Path) -> Result<(PublicKey, String), SshError> {
    if std::fs::metadata(path)?.len() > MAX_KEY_FILE {
        return Err(SshError::Key("public key file is too large".into()));
    }
    let text = std::fs::read_to_string(path)?;
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .ok_or_else(|| SshError::Key("empty public key file".into()))?;
    let key = PublicKey::from_openssh(line).map_err(|e| SshError::Key(e.to_string()))?;
    Ok((key, line.to_owned()))
}

/// Key pairs in `dir`, found through their `.pub` files, sorted by name.
pub fn list_keys(dir: &Path) -> Vec<KeyInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<KeyInfo> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".pub")?.to_owned();
            if !path.is_file() {
                return None;
            }
            let (key, _) = read_public_key(&path).ok()?;
            let private = dir.join(&name);
            let encrypted = std::fs::metadata(&private)
                .ok()
                .filter(|m| m.len() <= MAX_KEY_FILE)
                .and_then(|_| std::fs::read_to_string(&private).ok())
                .and_then(|t| PrivateKey::from_openssh(t).ok())
                .map(|k| k.is_encrypted());
            Some(KeyInfo {
                name,
                private_path: private.is_file().then_some(private),
                public_path: path,
                algorithm: key.algorithm().to_string(),
                fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
                comment: key.comment().to_string(),
                encrypted,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_lists_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        let plain = generate_ed25519(&ssh, "termalith_test", "me@laptop", None).unwrap();
        assert_eq!(plain.encrypted, Some(false));
        assert_eq!(plain.algorithm, "ssh-ed25519");
        let enc = generate_ed25519(&ssh, "locked", "", Some("pw")).unwrap();
        assert_eq!(enc.encrypted, Some(true));
        // The encrypted key opens with the passphrase only.
        let text = std::fs::read_to_string(enc.private_path.as_ref().unwrap()).unwrap();
        assert!(russh::keys::decode_secret_key(&text, None).is_err());
        assert!(russh::keys::decode_secret_key(&text, Some("pw")).is_ok());

        assert!(matches!(
            generate_ed25519(&ssh, "termalith_test", "", None),
            Err(SshError::DestinationExists(_))
        ));
        for bad in ["../x", ".hidden", "a b", ""] {
            assert!(generate_ed25519(&ssh, bad, "", None).is_err(), "{bad}");
        }

        std::fs::write(ssh.join("junk.pub"), "not a key").unwrap();
        let listed = list_keys(&ssh);
        let names: Vec<_> = listed.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["locked", "termalith_test"]);
        assert_eq!(listed[1].fingerprint, plain.fingerprint);
        assert_eq!(listed[1].comment, "me@laptop");
        assert_eq!(listed[0].encrypted, Some(true));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(plain.private_path.as_ref().unwrap()), 0o600);
            assert_eq!(mode(&ssh), 0o700);
        }
    }
}
