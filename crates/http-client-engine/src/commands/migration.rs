//! Native API owner adapters for copied legacy data. Never returns plaintext
//! secret material; the existing v1 DPAPI domains remain unchanged.
use super::request::EnvironmentVariable;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
const ERROR: &str = "legacy_api_storage_invalid";
const MAX_BYTES: usize = 20 * 1024 * 1024;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentStore {
    version: u32,
    environments: Vec<Environment>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Environment {
    id: String,
    name: String,
    variables: Vec<EnvironmentVariable>,
}
fn parse_environment(raw: &str) -> Result<EnvironmentStore, String> {
    if raw.len() > MAX_BYTES {
        return Err(ERROR.into());
    }
    let store: EnvironmentStore = serde_json::from_str(raw).map_err(|_| ERROR)?;
    if store.version != 1 || store.environments.len() > 10_000 {
        return Err(ERROR.into());
    }
    let mut ids = std::collections::HashSet::new();
    let mut count = 0usize;
    for env in &store.environments {
        if env.id.is_empty() || env.id.len() > 256 || env.name.len() > 4096 || !ids.insert(&env.id)
        {
            return Err(ERROR.into());
        }
        let mut keys = std::collections::HashSet::new();
        for variable in &env.variables {
            count += 1;
            if count > 10_000
                || variable.key.is_empty()
                || variable.key.len() > 1024
                || !keys.insert(&variable.key)
                || variable.value.len() > MAX_BYTES
            {
                return Err(ERROR.into());
            }
        }
    }
    Ok(store)
}
fn prepare_with(raw: &str, sealer: &dyn devbox_secrets::Sealer) -> Result<(String, usize), String> {
    let mut store = parse_environment(raw)?;
    let mut missing = 0;
    for env in &mut store.environments {
        for variable in &mut env.variables {
            if variable.secret {
                let available = B64
                    .decode(&variable.value)
                    .ok()
                    .and_then(|blob| devbox_secrets::unseal_v1(sealer, &blob).ok())
                    .is_some();
                if !available {
                    variable.value.clear();
                    missing += 1;
                }
            }
        }
    }
    Ok((serde_json::to_string(&store).map_err(|_| ERROR)?, missing))
}
pub(crate) fn prepare_environment(raw: &str) -> Result<(String, usize), String> {
    prepare_with(raw, crate::platform::platform_sealer().as_ref())
}
pub(crate) fn sanitize(serialized: String, environment: &str) -> Result<String, String> {
    if serialized.len() > MAX_BYTES {
        return Err(ERROR.into());
    }
    let variables = parse_environment(environment)?
        .environments
        .into_iter()
        .flat_map(|env| env.variables)
        .collect();
    super::request::sanitize_persisted_json(serialized, variables)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Unavailable;
    impl devbox_secrets::Sealer for Unavailable {
        fn seal(&self, _: &str) -> Result<Vec<u8>, devbox_secrets::SealError> {
            Err(devbox_secrets::SealError::CryptoFailure)
        }
        fn unseal(
            &self,
            _: &[u8],
        ) -> Result<zeroize::Zeroizing<String>, devbox_secrets::SealError> {
            Err(devbox_secrets::SealError::CryptoFailure)
        }
    }
    #[test]
    fn unavailable_secret_is_a_reconnect_slot_and_original_ciphertext_is_unchanged() {
        let raw = r#"{"version":1,"environments":[{"id":"e1","name":"fixture","variables":[{"key":"TOKEN","value":"b3BhcXVlLWZpeHR1cmU=","secret":true},{"key":"PORT","value":"9000","secret":false}]}]}"#;
        let (safe, missing) = prepare_with(raw, &Unavailable).unwrap();
        let safe = parse_environment(&safe).unwrap();
        assert_eq!(missing, 1);
        assert_eq!(safe.environments[0].variables[0].value, "");
        assert!(safe.environments[0].variables[0].secret);
        assert_eq!(safe.environments[0].variables[1].value, "9000");
        assert!(raw.contains("b3BhcXVlLWZpeHR1cmU="));
        assert!(prepare_with(r#"{"version":99,"environments":[]}"#, &Unavailable).is_err());
    }
}
