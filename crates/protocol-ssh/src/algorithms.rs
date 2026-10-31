//! Algorithm policy on top of russh's defaults.

use std::borrow::Cow;

use russh::Preferred;
use russh::keys::Algorithm;

/// russh's safe defaults minus `ssh-rsa` host-key signatures (RSA with
/// SHA-1), which OpenSSH disabled by default in 8.8. RSA host keys are still
/// accepted with `rsa-sha2-256/512`.
pub(crate) fn preferred() -> Preferred {
    let key: Vec<Algorithm> = Preferred::DEFAULT
        .key
        .iter()
        .filter(|a| !matches!(a, Algorithm::Rsa { hash: None }))
        .cloned()
        .collect();
    Preferred {
        key: Cow::Owned(key),
        ..Preferred::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_rsa_host_keys_are_removed() {
        let p = preferred();
        assert!(!p.key.contains(&Algorithm::Rsa { hash: None }));
        assert!(p.key.contains(&Algorithm::Ed25519));
        assert!(
            p.key
                .iter()
                .any(|a| matches!(a, Algorithm::Rsa { hash: Some(_) }))
        );
    }

    #[test]
    fn kex_and_ciphers_are_russh_safe_defaults() {
        let p = preferred();
        assert_eq!(p.kex, Preferred::DEFAULT.kex);
        assert_eq!(p.cipher, Preferred::DEFAULT.cipher);
        assert_eq!(p.mac, Preferred::DEFAULT.mac);
    }
}
