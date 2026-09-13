//! Task launch evidence has no command, environment or process ownership fields.
use crate::ObjectStamp;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskLaunch {
    pub schema_version: u32,
    pub root: String,
    pub cwd: String,
    pub root_object: ObjectStamp,
    pub source_digest: String,
}
pub fn validate(input: &TaskLaunch) -> Result<(), &'static str> {
    if input.schema_version != 1
        || [&input.root_object.scope, &input.root_object.object]
            .iter()
            .any(|value| {
                value.len() != 32
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            })
        || input.source_digest.len() != 64
        || !input
            .source_digest
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        || [&input.root, &input.cwd].iter().any(|path| {
            !path.starts_with('/')
                || path.len() > 32768
                || path.chars().any(char::is_control)
                || path
                    .split('/')
                    .skip(1)
                    .any(|part| matches!(part, "" | "." | ".."))
        })
    {
        return Err("task_launch_invalid");
    }
    Ok(())
}
