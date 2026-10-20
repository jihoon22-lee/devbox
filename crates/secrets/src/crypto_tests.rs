use crate::crypto::{Key, open, seal};
#[test]
fn seal_open_round_trip_and_wrong_aad_fails() {
    let key = Key::from_bytes([7; 32]);
    let id = key.id();
    let sealed = seal(&key, "TOKEN", &id, b"owned-token").expect("seal");
    assert_eq!(
        open(&key, "TOKEN", &id, &sealed).expect("open").as_slice(),
        b"owned-token"
    );
    assert!(open(&key, "OTHER", &id, &sealed).is_err());
    assert!(open(&key, "TOKEN", "wrong", &sealed).is_err());
    assert_ne!(
        sealed,
        seal(&key, "TOKEN", &id, b"owned-token").expect("random nonce")
    );
    assert!(sealed.len() >= 24 + 16);
}
