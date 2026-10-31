//! Round trip through the real OS credential store. Skipped when the store
//! is unavailable unless `TERMALITH_REQUIRE_NATIVE_SECRETS=1` (CI).

use credential_store::{CredentialRef, NativeSecretStore, SecretError, SecretStore};

const SERVICE: &str = "dev.termalith.tests";

fn store_or_skip() -> Option<NativeSecretStore> {
    let required = std::env::var("TERMALITH_REQUIRE_NATIVE_SECRETS").as_deref() == Ok("1");
    match NativeSecretStore::new(SERVICE) {
        Ok(s) => Some(s),
        Err(e) if required => panic!("native store required but unavailable: {e}"),
        Err(e) => {
            eprintln!("SKIPPED: {e}");
            None
        }
    }
}

#[test]
fn round_trip_update_and_delete() {
    let Some(store) = store_or_skip() else { return };
    let r = CredentialRef::generate().unwrap();

    match store.get(&r) {
        Ok(None) => {}
        Err(SecretError::Unavailable(e))
            if std::env::var("TERMALITH_REQUIRE_NATIVE_SECRETS").as_deref() != Ok("1") =>
        {
            eprintln!("SKIPPED: {e}");
            return;
        }
        other => panic!("fresh ref should be empty, got {other:?}"),
    }

    // Non-UTF-8 and multi-byte content must survive unchanged.
    let secret = "pässwörd-🔑-\u{0}-end".as_bytes();
    store.put(&r, secret).expect("put");
    assert_eq!(store.get(&r).unwrap().unwrap().as_slice(), secret);

    store.put(&r, b"rotated").expect("update");
    assert_eq!(store.get(&r).unwrap().unwrap().as_slice(), b"rotated");

    store.delete(&r).expect("delete");
    assert!(store.get(&r).unwrap().is_none());
    store.delete(&r).expect("delete is idempotent");
}

#[test]
fn entries_are_isolated_by_reference() {
    let Some(store) = store_or_skip() else { return };
    let a = CredentialRef::generate().unwrap();
    let b = CredentialRef::generate().unwrap();
    if store.put(&a, b"alpha").is_err()
        && std::env::var("TERMALITH_REQUIRE_NATIVE_SECRETS").as_deref() != Ok("1")
    {
        eprintln!("SKIPPED: store not writable");
        return;
    }
    store.put(&b, b"beta").unwrap();
    assert_eq!(store.get(&a).unwrap().unwrap().as_slice(), b"alpha");
    assert_eq!(store.get(&b).unwrap().unwrap().as_slice(), b"beta");
    store.delete(&a).unwrap();
    assert!(store.get(&a).unwrap().is_none());
    assert_eq!(store.get(&b).unwrap().unwrap().as_slice(), b"beta");
    store.delete(&b).unwrap();
}
