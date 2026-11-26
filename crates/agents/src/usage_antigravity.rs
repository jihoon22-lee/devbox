//! agy's token usage, read from the one transcript file its hooks named for the task's
//! conversation. Each `PLANNER_RESPONSE` step is one model call with its own counts. The total is
//! their sum over the conversation, the way Claude's is the sum of its messages: input tokens
//! that were not read from the cache, cache-read tokens and output tokens. agy's own status line
//! shows a smaller "total input", which is not this sum. Evidence:
//! `tests/fixtures/antigravity-1.5.17/transcript_full.jsonl` (real transcript of agy 1.5.17).
use crate::{
    antigravity::transcript_path,
    model::{AgentTask, AgentUsage, Tokens},
    tracker::Binding,
    usage::{empty, safe_file},
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
};

/// A larger transcript is not read.
const MAX_BYTES: u64 = 128 * 1024 * 1024;
/// A longer line is skipped, never parsed or held.
const MAX_LINE: usize = 4 * 1024 * 1024;

/// What usage reads of a step; what was said and done in it is skipped without being built.
#[derive(Deserialize)]
struct Step {
    #[serde(rename = "type")]
    kind: Option<String>,
    step_index: Option<u64>,
    input_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

/// The usage of the task's conversation. The transcript's path came from a hook payload, which
/// the tool writes, so it is checked again here: below agy's data folder in `home`, a regular
/// file, no symlink on the way, and no larger than [`MAX_BYTES`].
pub(crate) fn usage(home: &Path, task: &AgentTask, binding: Option<&Binding>) -> AgentUsage {
    let Some(named) = binding.and_then(|binding| binding.transcript.as_deref()) else {
        return empty(false);
    };
    if task.session_id.is_none() {
        return empty(false);
    }
    let Some(path) = named
        .to_str()
        .and_then(|named| transcript_path(home, named))
    else {
        return empty(true);
    };
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return empty(false),
        Err(_) => return empty(true),
        Ok(meta) if !meta.is_file() || meta.len() > MAX_BYTES => return empty(true),
        Ok(_) => {}
    }
    if !safe_file(&path) {
        return empty(true);
    }
    match File::open(&path) {
        Ok(file) => read(BufReader::new(file.take(MAX_BYTES))),
        Err(_) => empty(true),
    }
}

/// Sums the model calls of a transcript. A step written twice counts once, with its larger
/// counts. A line that is too long, is not a step or is still being written is left out and
/// reported as truncated.
fn read(mut reader: impl BufRead) -> AgentUsage {
    let mut steps: HashMap<u64, Tokens> = HashMap::new();
    let mut truncated = false;
    let mut line = Vec::new();
    loop {
        line.clear();
        let bytes = match reader
            .by_ref()
            .take((MAX_LINE + 1) as u64)
            .read_until(b'\n', &mut line)
        {
            Ok(bytes) => bytes,
            Err(_) => return finish(steps, true),
        };
        if bytes == 0 {
            break;
        }
        let ended = line.last() == Some(&b'\n');
        if bytes > MAX_LINE {
            truncated = true;
            if !ended && reader.skip_until(b'\n').is_err() {
                break;
            }
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Some(step) = ended
            .then(|| serde_json::from_slice::<Step>(&line).ok())
            .flatten()
        else {
            truncated = true;
            continue;
        };
        if step.kind.as_deref() != Some("PLANNER_RESPONSE")
            || (step.input_tokens.is_none()
                && step.cache_read_tokens.is_none()
                && step.output_tokens.is_none())
        {
            continue;
        }
        let Some(index) = step.step_index else {
            truncated = true;
            continue;
        };
        let counted = steps.entry(index).or_default();
        counted.input = counted.input.max(step.input_tokens.unwrap_or(0));
        counted.cache_read = counted.cache_read.max(step.cache_read_tokens.unwrap_or(0));
        counted.output = counted.output.max(step.output_tokens.unwrap_or(0));
    }
    finish(steps, truncated)
}

fn finish(steps: HashMap<u64, Tokens>, mut truncated: bool) -> AgentUsage {
    let tokens = (!steps.is_empty()).then(|| {
        steps.values().fold(Tokens::default(), |mut sum, step| {
            for (total, count) in [
                (&mut sum.input, step.input),
                (&mut sum.output, step.output),
                (&mut sum.cache_read, step.cache_read),
            ] {
                *total = total.checked_add(count).unwrap_or_else(|| {
                    truncated = true;
                    u64::MAX
                });
            }
            sum
        })
    });
    AgentUsage { tokens, truncated }
}

#[cfg(test)]
#[path = "usage_antigravity_tests.rs"]
mod tests;
