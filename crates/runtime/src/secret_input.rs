//! Resolves secret references without adding plaintext to persisted Run definitions.
use crate::{R, RunDef, RuntimeError, RuntimeService};
use devbox_protocol::Fail;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use zeroize::Zeroizing;
fn parts(def: &RunDef) -> (&[String], &BTreeMap<String, String>) {
    match def {
        RunDef::Task(d) => (&d.secrets, &d.env),
        RunDef::Service(d) => (&d.secrets, &d.env),
        _ => panic!("executable definition"),
    }
}
pub fn injected_env(def: &RunDef, key: &str, value: &str) -> bool {
    let (names, _) = parts(def);
    names.iter().any(|name| name == key) || value.contains("{{secret:")
}

pub fn requires(def: &RunDef) -> bool {
    let (names, env) = parts(def);
    !names.is_empty() || env.values().any(|s| s.contains("{{secret:"))
}
fn references(value: &str) -> R<Vec<&str>> {
    let mut text = value;
    let mut names = Vec::new();
    while let Some((_, tail)) = text.split_once("{{secret:") {
        let (name, rest) = tail
            .split_once("}}")
            .ok_or_else(|| invalid("unterminated secret reference"))?;
        if !devbox_secrets::valid_name(name) {
            return Err(invalid("invalid secret reference"));
        }
        names.push(name);
        text = rest;
    }
    Ok(names)
}
fn invalid(reason: &str) -> Fail<RuntimeError> {
    RuntimeError::DefInvalid {
        id: "secrets".into(),
        reason: reason.into(),
    }
    .into()
}
fn mapped(error: devbox_protocol::Fail<devbox_secrets::SecretsError>) -> Fail<RuntimeError> {
    match error {
        Fail::Domain(devbox_secrets::SecretsError::Missing { names }) => {
            RuntimeError::SecretMissing { names }.into()
        }
        Fail::Domain(devbox_secrets::SecretsError::KeyMismatch) => {
            RuntimeError::SecretKeyMismatch.into()
        }
        Fail::Domain(_) => RuntimeError::SecretsLocked.into(),
        Fail::Internal(_) => RuntimeError::SecretsLocked.into(),
    }
}
impl RuntimeService {
    pub fn with_secrets(mut self, secrets: Arc<devbox_secrets::SecretsService>) -> Self {
        self.secrets = Some(secrets);
        self
    }
    pub(crate) async fn secret_payload(&self, def: &RunDef) -> R<Zeroizing<Vec<u8>>> {
        let (explicit, env) = parts(def);
        let mut names: BTreeSet<String> = explicit.iter().cloned().collect();
        for value in env.values() {
            names.extend(references(value)?.into_iter().map(str::to_owned));
        }
        if names.is_empty() {
            return Ok(Zeroizing::new(Vec::new()));
        }
        let service = self.secrets.as_ref().ok_or(RuntimeError::SecretsLocked)?;
        let values = service
            .resolve(&names.into_iter().collect::<Vec<_>>())
            .await
            .map_err(mapped)?;
        let mut result = Zeroizing::new(Vec::new());
        // Redaction records carry original values independently of composed environment values.
        for value in values.values() {
            result.extend_from_slice(b"@=");
            result.extend_from_slice(value);
            result.push(0);
        }
        let mut resolved: BTreeMap<String, Zeroizing<Vec<u8>>> = explicit
            .iter()
            .map(|name| (name.clone(), Zeroizing::new(values[name].to_vec())))
            .collect();
        for (name, value) in env.iter().filter(|(_, v)| v.contains("{{secret:")) {
            let mut text = value.as_str();
            let mut output = Zeroizing::new(Vec::new());
            while let Some((head, tail)) = text.split_once("{{secret:") {
                output.extend_from_slice(head.as_bytes());
                let (reference, rest) = tail.split_once("}}").expect("validated reference");
                output.extend_from_slice(&values[reference]);
                text = rest;
            }
            output.extend_from_slice(text.as_bytes());
            resolved.insert(name.clone(), output);
        }
        for (name, value) in resolved {
            result.extend_from_slice(name.as_bytes());
            result.push(b'=');
            result.extend_from_slice(&value);
            result.push(0);
        }
        if result.len() > 1024 * 1024 {
            return Err(invalid("secret payload exceeds execution input limit"));
        }
        Ok(result)
    }
    pub async fn resolve_run_secrets(&self, id: &str) -> R<Zeroizing<Vec<u8>>> {
        let value = self.stored(id).await?;
        if !value.run.state.is_live() {
            return Err(invalid("run is not live"));
        }
        let def = value
            .secret_definition
            .as_ref()
            .ok_or_else(|| invalid("run has no secret references"))?;
        self.secret_payload(def).await
    }
}
