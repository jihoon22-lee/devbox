//! Native capture evaluation and bounded, process-only reveal references.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use devbox_secrets::Sealer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use zeroize::{Zeroize, Zeroizing};
const TTL_MS: u64 = 30 * 60 * 1000;
const MAX_ENTRIES: usize = 128;
const MAX_SEALED_BYTES: usize = 12 * 1024 * 1024;
const UNAVAILABLE: &str = "capture_reference_unavailable";

#[derive(Clone, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ResponseCapture {
    pub id: String,
    pub enabled: bool,
    pub variable: String,
    pub source: String,
    pub target: String,
}
#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct SealedCapture {
    pub name: String,
    pub value: String,
    pub reference: String,
}
#[derive(Default, Clone, Debug, Serialize, ts_rs::TS)]
pub struct NativeCaptureOutcome {
    pub values: Vec<SealedCapture>,
    pub missing: Vec<String>,
    pub errors: Vec<String>,
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphabetic()
                || b == b'_'
                || (i > 0 && (b.is_ascii_digit() || matches!(b, b'.' | b'-')))
        })
}
pub(crate) fn validate(definitions: &[ResponseCapture]) -> Result<(), &'static str> {
    if definitions.len() > 20
        || definitions.iter().any(|d| {
            d.id.len() > 128
                || !valid_name(&d.variable)
                || d.target.encode_utf16().count() > 256
                || !matches!(d.source.as_str(), "status" | "header" | "jsonPath")
        })
    {
        Err("capture_input_invalid")
    } else {
        Ok(())
    }
}
// serde_json owns copies of token strings: wipe them before releasing the tree.
struct SecretJson(Value);
impl Drop for SecretJson {
    fn drop(&mut self) {
        fn wipe(value: Value) {
            match value {
                Value::String(mut value) => value.zeroize(),
                Value::Array(values) => {
                    for value in values {
                        wipe(value);
                    }
                }
                Value::Object(values) => {
                    for (mut key, value) in values {
                        key.zeroize();
                        wipe(value);
                    }
                }
                _ => {}
            }
        }
        wipe(std::mem::take(&mut self.0));
    }
}
pub(crate) fn evaluate(
    definitions: &[ResponseCapture],
    status: u16,
    headers: &[(Zeroizing<String>, Zeroizing<String>)],
    text: Option<&str>,
    is_json: bool,
    sealer: &dyn Sealer,
) -> NativeCaptureOutcome {
    let mut output = NativeCaptureOutcome::default();
    let document = text
        .filter(|_| is_json)
        .and_then(|text| serde_json::from_str(text).ok())
        .map(SecretJson);
    for definition in definitions.iter().filter(|d| d.enabled) {
        output.values.retain(|v| v.name != definition.variable);
        output.missing.retain(|name| name != &definition.variable);
        let plain: Result<Option<Zeroizing<String>>, &'static str> =
            match definition.source.as_str() {
                "status" => Ok(Some(Zeroizing::new(status.to_string()))),
                "header" => Ok(headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&definition.target))
                    .map(|(_, value)| value.clone())),
                "jsonPath" => document
                    .as_ref()
                    .ok_or("capture_json_unavailable")
                    .and_then(|document| {
                        crate::core::capture_path::evaluate(&document.0, &definition.target).map(
                            |values| {
                                values.first().map(|value| {
                                    Zeroizing::new(match value {
                                        Value::String(value) => value.clone(),
                                        value => value.to_string(),
                                    })
                                })
                            },
                        )
                    }),
                _ => Err("capture_input_invalid"),
            };
        let sealed = plain.and_then(|value| {
            let value = value.ok_or("capture_value_missing")?;
            if value.len() > 65536 {
                return Err("capture_value_too_large");
            }
            if value.contains("[REDACTED]") {
                return Err("capture_value_redacted");
            }
            devbox_secrets::seal_v1(sealer, &value)
                .map(|value| B64.encode(value))
                .map_err(|_| "capture_seal_failed")
        });
        match sealed {
            Ok(value) => output.values.push(SealedCapture {
                name: definition.variable.clone(),
                value,
                reference: String::new(),
            }),
            Err(code) => {
                output.missing.push(definition.variable.clone());
                if code != "capture_value_missing" {
                    output.errors.push(code.into());
                }
            }
        }
    }
    output
}
fn unseal(value: &str, sealer: &dyn Sealer) -> Result<Zeroizing<String>, &'static str> {
    let blob = B64.decode(value).map_err(|_| UNAVAILABLE)?;
    devbox_secrets::unseal_v1(sealer, &blob).map_err(|_| UNAVAILABLE)
}
struct Stored {
    name: String,
    value: String,
    request_id: String,
    expires: u64,
    discarded: Option<u64>,
}
#[derive(Default)]
pub(crate) struct CaptureStore {
    entries: BTreeMap<String, Stored>,
}
impl CaptureStore {
    fn prune(&mut self, now: u64) {
        self.entries.retain(|_, entry| {
            entry.expires > now && entry.discarded.is_none_or(|deadline| deadline > now)
        });
    }
    pub(crate) fn invalidate(&mut self, names: impl Iterator<Item = String>) {
        for name in names {
            self.entries.retain(|_, entry| entry.name != name);
        }
    }
    pub(crate) fn accept(
        &mut self,
        request_id: &str,
        outcome: &mut NativeCaptureOutcome,
        now: u64,
    ) -> Result<(), &'static str> {
        self.prune(now);
        self.invalidate(
            outcome
                .missing
                .iter()
                .cloned()
                .chain(outcome.values.iter().map(|v| v.name.clone())),
        );
        let incoming = outcome.values.iter().map(|v| v.value.len()).sum::<usize>();
        if incoming > MAX_SEALED_BYTES || outcome.values.len() > 20 {
            return Err("capture_limit");
        }
        while self.entries.len() + outcome.values.len() > MAX_ENTRIES
            || self.entries.values().map(|v| v.value.len()).sum::<usize>() + incoming
                > MAX_SEALED_BYTES
        {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires)
                .map(|(id, _)| id.clone())
                .ok_or("capture_limit")?;
            self.entries.remove(&oldest);
        }
        let references = (0..outcome.values.len())
            .map(|_| super::grpc_selection::random_hex_128().map_err(|_| UNAVAILABLE))
            .collect::<Result<Vec<_>, _>>()?;
        for (value, reference) in outcome.values.iter_mut().zip(references) {
            value.reference = reference;
            self.entries.insert(
                value.reference.clone(),
                Stored {
                    name: value.name.clone(),
                    value: value.value.clone(),
                    request_id: request_id.into(),
                    expires: now.saturating_add(TTL_MS),
                    discarded: None,
                },
            );
        }
        Ok(())
    }
    pub(crate) fn reveal(
        &mut self,
        reference: &str,
        now: u64,
        sealer: &dyn Sealer,
    ) -> Result<String, &'static str> {
        self.prune(now);
        let entry = self
            .entries
            .get(reference)
            .filter(|entry| entry.discarded.is_none())
            .ok_or(UNAVAILABLE)?;
        unseal(&entry.value, sealer).map(|value| value.to_string())
    }
    pub(crate) fn discard(&mut self, references: &[String], now: u64) {
        self.prune(now);
        for reference in references.iter().take(MAX_ENTRIES) {
            if let Some(entry) = self.entries.get_mut(reference) {
                entry.discarded = Some(now.saturating_add(8000));
            }
        }
    }
    pub(crate) fn restore(&mut self, references: &[String], now: u64) -> Result<(), &'static str> {
        self.prune(now);
        if references.len() > MAX_ENTRIES
            || references.iter().any(|reference| {
                !self
                    .entries
                    .get(reference)
                    .is_some_and(|entry| entry.discarded.is_some())
            })
        {
            return Err(UNAVAILABLE);
        }
        for reference in references {
            if let Some(entry) = self.entries.get_mut(reference) {
                entry.discarded = None;
            }
        }
        Ok(())
    }
    pub(crate) fn revoke_request(&mut self, request_id: &str) {
        self.entries
            .retain(|_, entry| entry.request_id != request_id);
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
pub(crate) fn now() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_secrets::{SealError, Sealer};
    use zeroize::Zeroizing;
    struct Mock;
    impl Sealer for Mock {
        fn seal(&self, value: &str) -> Result<Vec<u8>, SealError> {
            Ok(value.bytes().map(|b| b ^ 0xaa).collect())
        }
        fn unseal(&self, value: &[u8]) -> Result<Zeroizing<String>, SealError> {
            String::from_utf8(value.iter().map(|b| b ^ 0xaa).collect())
                .map(Zeroizing::new)
                .map_err(|_| SealError::CryptoFailure)
        }
    }
    fn definition(variable: &str, source: &str, target: &str) -> ResponseCapture {
        ResponseCapture {
            enabled: true,
            id: "c".into(),
            variable: variable.into(),
            source: source.into(),
            target: target.into(),
        }
    }
    #[test]
    fn seals_native_values_without_projecting_plaintext() {
        let defs = vec![
            definition("token", "jsonPath", "$.access_token"),
            definition("head", "header", "authorization"),
            definition("status", "status", ""),
        ];
        let out = evaluate(
            &defs,
            201,
            &[(
                Zeroizing::new("Authorization".into()),
                Zeroizing::new("private-header".into()),
            )],
            Some(r#"{"access_token":"private-token"}"#),
            true,
            &Mock,
        );
        assert_eq!(out.values.len(), 3);
        let json = serde_json::to_string(&out).unwrap();
        assert!(!json.contains("private-token") && !json.contains("private-header"));
        assert_eq!(
            unseal(&out.values[0].value, &Mock).unwrap().as_str(),
            "private-token"
        );
        assert_eq!(
            unseal(&out.values[1].value, &Mock).unwrap().as_str(),
            "private-header"
        );
    }
    #[test]
    fn failed_duplicate_capture_removes_the_previous_value_and_bounds_inputs() {
        let defs = [
            definition("token", "status", ""),
            definition("token", "jsonPath", "$.missing"),
        ];
        let out = evaluate(&defs, 200, &[], Some("{}"), true, &Mock);
        assert!(out.values.is_empty());
        assert_eq!(out.missing, ["token"]);
        assert!(out.errors.is_empty());
        let out = evaluate(
            &[definition("token", "jsonPath", "$.token")],
            200,
            &[],
            Some(r#"{"token":"[REDACTED]"}"#),
            true,
            &Mock,
        );
        assert!(out.values.is_empty());
        assert_eq!(out.missing, ["token"]);
        let huge = serde_json::json!({"token":"x".repeat(65537)}).to_string();
        assert!(evaluate(
            &[definition("token", "jsonPath", "$.token")],
            200,
            &[],
            Some(&huge),
            true,
            &Mock
        )
        .values
        .is_empty());
        assert!(validate(&vec![definition("token", "status", ""); 21]).is_err());
        assert!(validate(&[definition("bad name", "status", "")]).is_err());
    }
    #[test]
    fn replacing_a_variable_revokes_old_references_and_store_is_bounded() {
        let mut store = CaptureStore::default();
        let mut outcome = evaluate(
            &[definition("token", "status", "")],
            200,
            &[],
            None,
            false,
            &Mock,
        );
        store.accept("a", &mut outcome, 1).unwrap();
        let original = outcome.values[0].reference.clone();
        store.accept("b", &mut outcome, 2).unwrap();
        assert!(store.reveal(&original, 2, &Mock).is_err());
        for i in 0..200 {
            outcome.values[0].name = format!("value{i}");
            store.accept("c", &mut outcome, 3 + i).unwrap();
        }
        assert_eq!(store.entries.len(), MAX_ENTRIES);
        store.clear();
        assert!(store.entries.is_empty());
    }

    #[test]
    fn references_are_scoped_revocable_and_expire_without_plaintext_retention() {
        let mut store = CaptureStore::default();
        let mut out = evaluate(
            &[definition("token", "status", "")],
            200,
            &[],
            None,
            false,
            &Mock,
        );
        store.accept("request-a", &mut out, 100).unwrap();
        let reference = out.values[0].reference.clone();
        assert_eq!(store.reveal(&reference, 100, &Mock).unwrap(), "200");
        assert!(store.reveal("forged", 100, &Mock).is_err());
        store.discard(std::slice::from_ref(&reference), 101);
        assert!(store.reveal(&reference, 102, &Mock).is_err());
        assert!(store
            .restore(&[reference.clone(), "missing".into()], 102)
            .is_err());
        assert!(store.reveal(&reference, 102, &Mock).is_err());
        store
            .restore(std::slice::from_ref(&reference), 103)
            .unwrap();
        assert_eq!(store.reveal(&reference, 103, &Mock).unwrap(), "200");
        store.discard(std::slice::from_ref(&reference), 104);
        assert!(store
            .restore(std::slice::from_ref(&reference), 8105)
            .is_err());
        assert!(store.reveal(&reference, 8105, &Mock).is_err());
        store.accept("request-b", &mut out, 9000).unwrap();
        let reference = out.values[0].reference.clone();
        store.revoke_request("request-b");
        assert!(store.reveal(&reference, 9001, &Mock).is_err());
        store.accept("request-c", &mut out, 10000).unwrap();
        assert!(store
            .reveal(&out.values[0].reference, 10000 + TTL_MS, &Mock)
            .is_err());
    }
}
