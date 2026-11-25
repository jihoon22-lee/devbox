//! `promptArg`: how a `hooks = "none"` profile gets the task's prompt as an argument. The other
//! families pass the prompt their own fixed way (`start::render_start_script`).
//!
//! | `promptArg` | appended to the command |
//! |---|---|
//! | `"--flag"` or `"-f"` | ` --flag "<prompt>"` |
//! | `"--flag="` | ` --flag="<prompt>"` (one argument) |
//! | `"--"` | ` -- "<prompt>"` |
//! | `""` | ` "<prompt>"` (the prompt alone) |
use crate::plan::sh_quote;
use crate::profile::{Hooks, ProfileError, ToolProfile};

/// The longest flag accepted.
const MAX_FLAG_BYTES: usize = 64;

/// Whether `flag` is the empty string, `--`, or one option name: `-` or `--`, a letter or digit,
/// then letters, digits, `-` and `_`, with an optional `=` at the end. The flag goes into the
/// start script as written, so nothing a shell reads specially is accepted.
fn valid(flag: &str) -> bool {
    if flag.is_empty() || flag == "--" {
        return true;
    }
    let name = flag.strip_suffix('=').unwrap_or(flag);
    let Some(name) = name.strip_prefix("--").or_else(|| name.strip_prefix('-')) else {
        return false;
    };
    flag.len() <= MAX_FLAG_BYTES
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A profile's `promptArg` is one of the accepted forms, and only a `hooks = "none"` profile has
/// one: the tool families pass the prompt their own way.
pub(crate) fn check(profile: &ToolProfile) -> Result<(), ProfileError> {
    let Some(flag) = &profile.prompt_arg else {
        return Ok(());
    };
    let reason = if profile.hooks != Hooks::None {
        "only a profile with hooks = \"none\" takes it"
    } else if !valid(flag) {
        "must be \"\", \"--\" or one option such as \"--prompt\", \"-p\" or \"--prompt=\""
    } else {
        return Ok(());
    };
    Err(ProfileError::PromptArg {
        profile: profile.id.clone(),
        reason,
    })
}

/// The `promptArg` a launch of `profile` uses for `prompt`: none for an empty prompt, so nothing is
/// appended, and none for a tool family.
pub(crate) fn for_launch(profile: &ToolProfile, prompt: &str) -> Option<String> {
    (profile.hooks == Hooks::None && !prompt.trim().is_empty())
        .then(|| profile.prompt_arg.clone())
        .flatten()
}

/// What the start script appends for `flag`: the prompt read from `prompt_file`, in double quotes
/// as one argument, the way the tool families pass it.
pub(crate) fn rendered(flag: &str, prompt_file: &str) -> String {
    let prompt = format!("\"$(cat {})\"", sh_quote(prompt_file));
    match flag {
        "" => format!(" {prompt}"),
        joined if joined.ends_with('=') => format!(" {joined}{prompt}"),
        flag => format!(" {flag} {prompt}"),
    }
}

#[cfg(test)]
#[path = "prompt_arg_tests.rs"]
mod tests;
