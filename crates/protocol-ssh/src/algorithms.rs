//! Algorithm policy on top of russh's defaults (ADR-043).
//!
//! Two lists: the normal one (modern algorithms plus the ECDH NIST curves
//! OpenSSH also offers by default) and, only when the user allowed it for a
//! connection, a legacy one for old switches and routers that know nothing
//! newer (SHA-1 key exchange, CBC ciphers, HMAC-SHA1, `ssh-rsa` and
//! `ssh-dss` host keys). Host keys are verified the same way in both.

use std::borrow::Cow;

use russh::keys::Algorithm;
use russh::{Preferred, cipher, kex, mac};

/// Key exchange algorithms OpenSSH enables by default that russh leaves out.
const EXTRA_KEX: &[kex::Name] = &[
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
];

/// Weak key exchange, strongest first. Only with legacy algorithms allowed.
pub const LEGACY_KEX: &[kex::Name] = &[kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1];

/// Weak ciphers (CBC mode), strongest first.
pub const LEGACY_CIPHERS: &[cipher::Name] = &[
    cipher::AES_256_CBC,
    cipher::AES_192_CBC,
    cipher::AES_128_CBC,
    cipher::TRIPLE_DES_CBC,
];

/// Weak MACs.
pub const LEGACY_MACS: &[mac::Name] = &[mac::HMAC_SHA1_ETM, mac::HMAC_SHA1];

/// Weak host key signatures: RSA with SHA-1 and DSA.
pub fn legacy_host_keys() -> [Algorithm; 2] {
    [Algorithm::Rsa { hash: None }, Algorithm::Dsa]
}

/// Whether `name` (an algorithm the server offered) is in a legacy list.
pub fn is_legacy(name: &str) -> bool {
    LEGACY_KEX.iter().any(|n| n.as_ref() == name)
        || LEGACY_CIPHERS.iter().any(|n| n.as_ref() == name)
        || LEGACY_MACS.iter().any(|n| n.as_ref() == name)
        || legacy_host_keys().iter().any(|a| a.as_str() == name)
}

/// russh's pseudo key exchange names (extension and strict-kex markers)
/// stay last.
fn is_marker(name: &kex::Name) -> bool {
    let n: &str = name.as_ref();
    n.starts_with("ext-info-") || n.starts_with("kex-strict-")
}

/// The algorithms offered to a server; `legacy` adds the weak ones after
/// all modern choices, so a modern server never negotiates them.
pub fn preferred(legacy: bool) -> Preferred {
    let mut kex_list: Vec<kex::Name> = Preferred::DEFAULT
        .kex
        .iter()
        .filter(|n| !is_marker(n))
        .cloned()
        .collect();
    kex_list.extend_from_slice(EXTRA_KEX);
    if legacy {
        kex_list.extend_from_slice(LEGACY_KEX);
    }
    kex_list.extend(
        Preferred::DEFAULT
            .kex
            .iter()
            .filter(|n| is_marker(n))
            .cloned(),
    );

    let mut key: Vec<Algorithm> = Preferred::DEFAULT
        .key
        .iter()
        .filter(|a| !matches!(a, Algorithm::Rsa { hash: None }))
        .cloned()
        .collect();
    let mut cipher_list: Vec<cipher::Name> = Preferred::DEFAULT.cipher.to_vec();
    let mut mac_list: Vec<mac::Name> = Preferred::DEFAULT.mac.to_vec();
    if legacy {
        key.extend(legacy_host_keys());
        cipher_list.extend_from_slice(LEGACY_CIPHERS);
        mac_list.extend_from_slice(LEGACY_MACS);
    }
    Preferred {
        kex: Cow::Owned(kex_list),
        key: Cow::Owned(key),
        cipher: Cow::Owned(cipher_list),
        mac: Cow::Owned(mac_list),
        ..Preferred::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_list_has_no_sha1_or_cbc() {
        let p = preferred(false);
        assert!(!p.key.contains(&Algorithm::Rsa { hash: None }));
        assert!(!p.key.contains(&Algorithm::Dsa));
        assert!(p.key.contains(&Algorithm::Ed25519));
        assert!(
            p.key
                .iter()
                .any(|a| matches!(a, Algorithm::Rsa { hash: Some(_) }))
        );
        assert!(p.kex.iter().all(|n| !is_legacy(n.as_ref())));
        assert!(p.cipher.iter().all(|n| !is_legacy(n.as_ref())));
        assert!(p.mac.iter().all(|n| !is_legacy(n.as_ref())));
        // OpenSSH's default ECDH curves are offered.
        assert!(p.kex.contains(&kex::ECDH_SHA2_NISTP256));
    }

    #[test]
    fn legacy_list_adds_weak_algorithms_after_modern_ones() {
        let p = preferred(true);
        for k in LEGACY_KEX {
            assert!(p.kex.contains(k));
        }
        assert!(p.cipher.contains(&cipher::TRIPLE_DES_CBC));
        assert!(p.mac.contains(&mac::HMAC_SHA1));
        assert!(p.key.contains(&Algorithm::Rsa { hash: None }));
        assert!(p.key.contains(&Algorithm::Dsa));
        let pos = |n: &kex::Name| p.kex.iter().position(|x| x == n).unwrap();
        assert!(
            pos(&kex::CURVE25519) < pos(&kex::DH_G1_SHA1),
            "modern first"
        );
        let last = p.kex.last().unwrap();
        assert!(is_marker(last), "markers stay last, got {last:?}");
        assert_eq!(p.cipher[0], Preferred::DEFAULT.cipher[0]);
    }

    #[test]
    fn recognises_legacy_names() {
        assert!(is_legacy("diffie-hellman-group1-sha1"));
        assert!(is_legacy("3des-cbc"));
        assert!(is_legacy("hmac-sha1"));
        assert!(is_legacy("ssh-rsa"));
        assert!(is_legacy("ssh-dss"));
        assert!(!is_legacy("curve25519-sha256"));
        assert!(!is_legacy("rsa-sha2-256"));
        assert!(!is_legacy("something-else"));
    }
}
