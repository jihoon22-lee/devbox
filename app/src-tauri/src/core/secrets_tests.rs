use super::*;
use devbox_protocol::message::ClientMessage;
use serde_json::json;
use std::cell::Cell;

struct Fixture {
    fail: bool,
    seals: Cell<usize>,
    opens: Cell<usize>,
}
impl Fixture {
    fn new(fail: bool) -> Self {
        Self {
            fail,
            seals: Cell::new(0),
            opens: Cell::new(0),
        }
    }
}
impl KeyProtector for Fixture {
    fn generate(&self) -> Result<Zeroizing<Vec<u8>>, Locked> {
        Ok(Zeroizing::new(vec![7; 32]))
    }
    fn seal(&self, _key: &[u8]) -> Result<Vec<u8>, Locked> {
        self.seals.set(self.seals.get() + 1);
        if self.fail {
            Err(Locked)
        } else {
            Ok(vec![9; 48])
        }
    }
    fn unseal(&self, _blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, Locked> {
        self.opens.set(self.opens.get() + 1);
        if self.fail {
            Err(Locked)
        } else {
            Ok(Zeroizing::new(vec![7; 32]))
        }
    }
}
fn response(id: u32, result: serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(&json!({"type":"response","id":id,"result":result})).unwrap();
    let mut frame = ((body.len() + 5) as u32).to_le_bytes().to_vec();
    frame.push(0);
    frame.extend_from_slice(&0u32.to_le_bytes());
    frame.extend(body);
    frame
}
fn request(frame: &[u8], expected: &str) -> (u32, Box<serde_json::value::RawValue>) {
    assert_eq!(devbox_protocol::frame::frame_channel(frame), Some(0));
    let ClientMessage::Request {
        id, method, params, ..
    } = serde_json::from_slice(&frame[9..]).unwrap()
    else {
        panic!("request")
    };
    assert_eq!(method, expected);
    (id, params)
}
fn begin(flow: &mut NativeSecrets, provider: &Fixture) -> (u32, Box<serde_json::value::RawValue>) {
    let hello = flow.start("test");
    let ClientMessage::Hello {
        client, instance, ..
    } = serde_json::from_slice(&hello[9..]).unwrap()
    else {
        panic!("hello")
    };
    assert_eq!(client, devbox_protocol::message::ClientKind::App);
    assert_eq!(instance, "test");
    let body = serde_json::to_vec(&json!({"type":"welcome","version":env!("CARGO_PKG_VERSION"),"protocol":devbox_protocol::PROTOCOL_VERSION,"daemonId":"fixture"})).unwrap();
    let mut frame = ((body.len() + 5) as u32).to_le_bytes().to_vec();
    frame.push(0);
    frame.extend_from_slice(&0u32.to_le_bytes());
    frame.extend(body);
    request(
        &flow.receive(&frame, provider).unwrap(),
        "secrets.key_status",
    )
}
#[test]
fn windows_app_unlocks_when_daemon_interop_is_unavailable() {
    let provider = Fixture::new(false);
    let mut flow = NativeSecrets::default();
    let (id, _) = begin(&mut flow, &provider);
    let status = json!({"locked":true,"keyId":fingerprint(&[7;32]),"blob":[9,9,9]});
    let unlock = flow.receive(&response(id, status), &provider).unwrap();
    let (id, params) = request(&unlock, "secrets.unlock");
    let key: devbox_protocol::secrets::Unlock = serde_json::from_str(params.get()).unwrap();
    assert_eq!(key.key, vec![7; 32]);
    assert_eq!(provider.opens.get(), 1);
    assert_eq!(provider.seals.get(), 0);
    assert!(
        flow.receive(&response(id, json!({"rev":"1"})), &provider)
            .is_none()
    );
    assert!(flow.finished());
}
#[test]
fn native_first_run_seals_and_bootstraps_before_unlocking() {
    let provider = Fixture::new(false);
    let mut flow = NativeSecrets::default();
    let (id, _) = begin(&mut flow, &provider);
    let bootstrap = flow
        .receive(
            &response(id, json!({"locked":true,"keyId":null,"blob":null})),
            &provider,
        )
        .unwrap();
    let (id, params) = request(&bootstrap, "secrets.bootstrap");
    let params: serde_json::Value = serde_json::from_str(params.get()).unwrap();
    assert_eq!(
        params,
        json!({"blob":vec![9;48],"keyId":fingerprint(&[7;32])})
    );
    assert!(params.get("key").is_none());
    let unlock = flow
        .receive(&response(id, json!({"rev":"1"})), &provider)
        .unwrap();
    request(&unlock, "secrets.unlock");
    assert_eq!(provider.seals.get(), 1);
    assert_eq!(provider.opens.get(), 0);
}
#[test]
fn native_failure_and_mismatch_never_replace_existing_key() {
    for (fail, key_id) in [(true, fingerprint(&[7; 32])), (false, "wrong-key".into())] {
        let provider = Fixture::new(fail);
        let mut flow = NativeSecrets::default();
        let (id, _) = begin(&mut flow, &provider);
        assert!(
            flow.receive(
                &response(id, json!({"locked":true,"keyId":key_id,"blob":[9]})),
                &provider
            )
            .is_none()
        );
        assert!(flow.finished());
        assert_eq!(provider.seals.get(), 0);
    }
}
#[test]
fn native_bootstrap_failure_never_sends_plaintext_unlock() {
    let provider = Fixture::new(false);
    let mut flow = NativeSecrets::default();
    let (id, _) = begin(&mut flow, &provider);
    let bootstrap = flow
        .receive(
            &response(id, json!({"locked":true,"keyId":null,"blob":null})),
            &provider,
        )
        .unwrap();
    let (id, _) = request(&bootstrap, "secrets.bootstrap");
    let mut failed = response(id, json!(null));
    failed.truncate(9);
    let body = serde_json::to_vec(
        &json!({"type":"response","id":id,"error":{"code":"secrets.key_mismatch"}}),
    )
    .unwrap();
    failed[..4].copy_from_slice(&((body.len() + 5) as u32).to_le_bytes());
    failed.extend(body);
    assert!(flow.receive(&failed, &provider).is_none());
    assert!(flow.finished());
}
#[test]
fn native_channel_and_request_ids_are_isolated_from_renderer_frames() {
    let provider = Fixture::new(false);
    let mut flow = NativeSecrets::default();
    let (id, _) = begin(&mut flow, &provider);
    let status = json!({"locked":true,"keyId":fingerprint(&[7;32]),"blob":[9]});
    let mut renderer = response(id, status.clone());
    renderer[5..9].copy_from_slice(&1u32.to_le_bytes());
    assert!(!is_native(&renderer));
    assert!(flow.receive(&renderer, &provider).is_none());
    assert!(
        flow.receive(&response(id + 1, status.clone()), &provider)
            .is_none()
    );
    assert!(!flow.finished());
    assert_eq!(provider.opens.get(), 0);
    assert!(flow.receive(&response(id, status), &provider).is_some());
}

#[test]
fn native_seal_failure_and_inconsistent_metadata_keep_storage_untouched() {
    for (fail, status) in [
        (true, json!({"locked":true,"keyId":null,"blob":null})),
        (false, json!({"locked":true,"keyId":"existing","blob":null})),
        (false, json!({"locked":true,"keyId":null,"blob":[9]})),
        (
            false,
            json!({"locked":false,"keyId":"existing","blob":null}),
        ),
    ] {
        let provider = Fixture::new(fail);
        let mut flow = NativeSecrets::default();
        let (id, _) = begin(&mut flow, &provider);
        assert!(flow.receive(&response(id, status), &provider).is_none());
        assert!(flow.finished());
        assert_eq!(provider.opens.get(), 0);
        assert_eq!(provider.seals.get(), usize::from(fail));
    }
}

#[test]
fn native_malformed_bootstrap_ack_does_not_release_the_key() {
    let provider = Fixture::new(false);
    let mut flow = NativeSecrets::default();
    let (id, _) = begin(&mut flow, &provider);
    let bootstrap = flow
        .receive(
            &response(id, json!({"locked":true,"keyId":null,"blob":null})),
            &provider,
        )
        .unwrap();
    let (id, _) = request(&bootstrap, "secrets.bootstrap");
    assert!(
        flow.receive(&response(id, json!({"unexpected":true})), &provider)
            .is_none()
    );
    assert!(flow.finished());
}
