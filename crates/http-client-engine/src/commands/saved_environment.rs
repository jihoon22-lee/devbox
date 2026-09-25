//! Current saved request sanitization using bounded environment metadata.
use super::request::EnvironmentVariable;
use serde::{Deserialize, Serialize};
const ERROR: &str = "saved_environment_invalid";
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
    #[test]
    fn saved_environment_rejects_future_shape_and_duplicate_keys() {
        for raw in [
            r#"{"version":2,"environments":[]}"#,
            r#"{"version":1,"environments":[],"future":true}"#,
            r#"{"version":1,"environments":[{"id":"e","name":"fixture","variables":[{"key":"A","value":"1","secret":false},{"key":"A","value":"2","secret":false}]}]}"#,
        ] {
            assert!(sanitize("{}".into(), raw).is_err());
        }
    }
    #[test]
    fn current_environment_sanitization_preserves_safe_saved_values() {
        let environment = r#"{"version":1,"environments":[{"id":"e","name":"fixture","variables":[{"key":"PORT","value":"9000","secret":false}]}]}"#;
        let safe = sanitize(
            r#"{"name":"fixture","url":"http://localhost:${PORT}"}"#.into(),
            environment,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&safe).unwrap();
        assert_eq!(value["name"], "fixture");
        assert_eq!(value["url"], "http://localhost:${PORT}");
    }
}
