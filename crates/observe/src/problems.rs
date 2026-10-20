use crate::{Problem, ProblemSeverity};
use regex::Regex;
use std::{
    path::{Component, Path},
    sync::OnceLock,
};

const FIELD_LIMIT: usize = 4096;
const CONTEXT_LINES: u8 = 32;

#[derive(Clone)]
struct Header {
    severity: ProblemSeverity,
    message: String,
}

struct Location {
    file: String,
    line: u32,
    col: Option<u32>,
}

/// One small context per run. Only the most recent traceback frame is retained.
#[derive(Default)]
pub struct ProblemParser {
    header: Option<Header>,
    frame: Option<Location>,
    python: bool,
    age: u8,
}

struct LocationContext<'a> {
    root: &'a Path,
    cwd: Option<&'a Path>,
    windows: Option<&'a str>,
    original: bool,
}
impl ProblemParser {
    #[cfg(test)]
    pub fn push(&mut self, line: &str, root: &Path) -> Option<Problem> {
        self.push_context(
            line,
            &LocationContext {
                root,
                cwd: Some(root),
                windows: None,
                original: false,
            },
        )
    }
    pub fn push_source(&mut self, line: &str, run: &crate::ProblemRun) -> Option<Problem> {
        self.push_context(
            line,
            &LocationContext {
                root: &run.root,
                cwd: run.source_cwd.as_deref(),
                windows: run.windows_directory.as_deref(),
                original: true,
            },
        )
    }
    fn push_context(&mut self, line: &str, project_root: &LocationContext<'_>) -> Option<Problem> {
        self.age = self.age.saturating_add(1);
        if self.age > CONTEXT_LINES {
            *self = Self::default();
        }
        let Some(clean) = clean_line(line) else {
            *self = Self::default();
            return None;
        };
        let text = clean.trim();
        if text == "Traceback (most recent call last):" {
            *self = Self {
                python: true,
                ..Self::default()
            };
            return None;
        }
        if text.starts_with("File \"") {
            self.header = None;
            self.python = true;
            self.age = 0;
            // An invalid/outside newest frame must also discard the earlier frame.
            self.frame = python_location(text, project_root);
            return None;
        }
        if self.python && exception_header(text) {
            let frame = self.frame.take();
            *self = Self::default();
            return make_problem(
                frame?,
                Header {
                    severity: ProblemSeverity::Error,
                    message: text.into(),
                },
            );
        }
        if let Some(header) = diagnostic_header(text) {
            self.frame = None;
            self.python = false;
            self.age = 0;
            self.header = (header.message.len() <= FIELD_LIMIT).then_some(header);
            return None;
        }
        if text.starts_with("-->") || text.starts_with(":::") {
            let header = self.header.clone()?;
            return make_problem(rust_location(text, project_root)?, header);
        }
        if let Some(location) = stack_location(text, project_root) {
            let header = self.header.clone().unwrap_or(Header {
                severity: ProblemSeverity::Error,
                message: "Test failure".into(),
            });
            return make_problem(location, header);
        }
        single_line(text, project_root)
    }
}

pub fn extract(line: &str, project_root: &Path) -> Option<Problem> {
    let context = LocationContext {
        root: project_root,
        cwd: Some(project_root),
        windows: None,
        original: false,
    };
    let project_root = &context;
    let clean = clean_line(line)?;
    let text = clean.trim();
    if let Some(location) = rust_location(text, project_root) {
        return make_problem(
            location,
            Header {
                severity: ProblemSeverity::Error,
                message: "Compiler diagnostic".into(),
            },
        );
    }
    if let Some(location) = python_location(text, project_root) {
        return make_problem(
            location,
            Header {
                severity: ProblemSeverity::Error,
                message: "Python traceback".into(),
            },
        );
    }
    if let Some(location) = stack_location(text, project_root) {
        return make_problem(
            location,
            Header {
                severity: ProblemSeverity::Error,
                message: "Test failure".into(),
            },
        );
    }
    single_line(text, project_root)
}

fn single_line(text: &str, root: &LocationContext<'_>) -> Option<Problem> {
    static TSC: OnceLock<Regex> = OnceLock::new();
    static COLON: OnceLock<Regex> = OnceLock::new();
    let tsc =
        TSC.get_or_init(|| Regex::new(r"^(.+)\(([0-9]+),([0-9]+)\):\s*(.+)$").expect("tsc regex"));
    let colon = COLON.get_or_init(|| {
        Regex::new(r"^(.+?):([0-9]+)(?::([0-9]+))?(?::\s*|\s+)(.+)$").expect("diagnostic regex")
    });
    let captures = tsc.captures(text).or_else(|| colon.captures(text))?;
    let location = location(
        &captures[1],
        &captures[2],
        captures.get(3).map(|c| c.as_str()),
        root,
    )?;
    let message = captures.get(4)?.as_str().trim();
    let (severity, message) = severity_message(message);
    make_problem(
        location,
        Header {
            severity,
            message: message.into(),
        },
    )
}

fn diagnostic_header(text: &str) -> Option<Header> {
    if let Some(rest) = text.strip_prefix("error[") {
        let (code, message) = rest.split_once("]:")?;
        return Some(Header {
            severity: ProblemSeverity::Error,
            message: format!("{code}: {}", message.trim()),
        });
    }
    for (prefix, severity) in [
        ("error:", ProblemSeverity::Error),
        ("warning:", ProblemSeverity::Warning),
        ("note:", ProblemSeverity::Info),
    ] {
        if let Some(message) = text.strip_prefix(prefix) {
            return Some(Header {
                severity,
                message: message.trim().into(),
            });
        }
    }
    exception_header(text).then(|| Header {
        severity: ProblemSeverity::Error,
        message: text.into(),
    })
}

fn exception_header(text: &str) -> bool {
    let name = text.split_once(':').map_or(text, |(name, _)| name);
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
        && (name.ends_with("Error")
            || name.ends_with("Exception")
            || matches!(name, "KeyboardInterrupt" | "SystemExit"))
}

fn severity_message(message: &str) -> (ProblemSeverity, &str) {
    for (prefix, severity) in [
        ("error", ProblemSeverity::Error),
        ("warning", ProblemSeverity::Warning),
        ("info", ProblemSeverity::Info),
        ("note", ProblemSeverity::Info),
    ] {
        if let Some(rest) = message.strip_prefix(prefix)
            && (rest.is_empty() || rest.starts_with(':') || rest.starts_with(char::is_whitespace))
        {
            return (severity, rest.trim_start_matches(':').trim());
        }
    }
    (ProblemSeverity::Error, message)
}

fn rust_location(text: &str, root: &LocationContext<'_>) -> Option<Location> {
    let text = text
        .strip_prefix("-->")
        .or_else(|| text.strip_prefix(":::"))?
        .trim();
    colon_location(text, root)
}

fn stack_location(text: &str, root: &LocationContext<'_>) -> Option<Location> {
    let text = if let Some(rest) = text.strip_prefix('❯') {
        rest.trim()
    } else {
        let rest = text.strip_prefix("at ")?;
        if let Some(inner) = rest.strip_suffix(')') {
            inner.rsplit_once('(')?.1
        } else {
            rest
        }
    };
    colon_location(text, root)
}

fn colon_location(text: &str, root: &LocationContext<'_>) -> Option<Location> {
    let (head, col) = text.rsplit_once(':')?;
    let (file, line) = head.rsplit_once(':')?;
    location(file, line, Some(col), root)
}

fn python_location(text: &str, root: &LocationContext<'_>) -> Option<Location> {
    let rest = text.strip_prefix("File \"")?;
    let (file, rest) = rest.split_once("\", line ")?;
    let line = rest.split(',').next()?.trim();
    location(file, line, None, root)
}

fn location(
    file: &str,
    line: &str,
    col: Option<&str>,
    root: &LocationContext<'_>,
) -> Option<Location> {
    let line: u32 = line.parse().ok()?;
    let col = col.map(str::parse::<u32>).transpose().ok()?;
    if line == 0 || col == Some(0) {
        return None;
    }
    Some(Location {
        file: resolve_location(root, file)?,
        line,
        col,
    })
}

fn make_problem(location: Location, header: Header) -> Option<Problem> {
    if header.message.len() > FIELD_LIMIT {
        return None;
    }
    Some(Problem {
        file: location.file,
        line: location.line,
        col: location.col,
        severity: header.severity,
        message: header.message,
        source: String::new(),
    })
}

fn resolve_location(context: &LocationContext<'_>, file: &str) -> Option<String> {
    if !context.original {
        return resolve_within_project(context.root, file);
    }
    let normalized = file.replace('\\', "/");
    let relative = if normalized.as_bytes().get(1) == Some(&b':') {
        let prefix = format!(
            "{}/",
            context.windows?.replace('\\', "/").trim_end_matches('/')
        );
        if !normalized
            .get(..prefix.len())?
            .eq_ignore_ascii_case(&prefix)
        {
            return None;
        }
        &normalized[prefix.len()..]
    } else {
        &normalized
    };
    let path = Path::new(relative);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        context.cwd?.join(path)
    };
    // Preserve the registered project boundary, with the recorded owned worktree
    // as the only additional root. Do not guess cwd for old relative diagnostics.
    for root in [Some(context.root), context.cwd].into_iter().flatten() {
        if let Some(relative) = resolve_within_project(root, absolute.to_str()?) {
            let absolute = root.canonicalize().ok()?.join(relative);
            return Some(match context.cwd.and_then(|cwd| cwd.canonicalize().ok()) {
                Some(cwd) => absolute
                    .strip_prefix(cwd)
                    .unwrap_or(&absolute)
                    .to_str()?
                    .replace('\\', "/"),
                None => absolute.to_str()?.to_owned(),
            });
        }
    }
    None
}

fn resolve_within_project(root: &Path, file: &str) -> Option<String> {
    if file.is_empty()
        || file.len() > FIELD_LIMIT
        || file.contains("://")
        || file.chars().any(char::is_control)
        || (!cfg!(windows) && (file.starts_with('\\') || file.as_bytes().get(1) == Some(&b':')))
    {
        return None;
    }
    let canonical_root = root.canonicalize().ok()?;
    if !canonical_root.is_dir() {
        return None;
    }
    let path = Path::new(file);
    let relative = if path.is_absolute() {
        path.strip_prefix(&canonical_root)
            .or_else(|_| path.strip_prefix(root))
            .ok()?
    } else {
        path
    };
    let mut resolved = canonical_root.clone();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved == canonical_root {
                    return None;
                }
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                match resolved.canonicalize() {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // A dangling symlink cannot be treated as an ordinary missing file.
                        if resolved
                            .symlink_metadata()
                            .is_ok_and(|meta| meta.file_type().is_symlink())
                        {
                            return None;
                        }
                    }
                    Err(_) => return None,
                }
                if !resolved.starts_with(&canonical_root) {
                    return None;
                }
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    let relative = resolved.strip_prefix(canonical_root).ok()?;
    let file = relative.to_str()?.replace(std::path::MAIN_SEPARATOR, "/");
    (!file.is_empty() && file.len() <= FIELD_LIMIT).then_some(file)
}

fn clean_line(line: &str) -> Option<String> {
    // Bound work even for an output line consisting almost entirely of terminal escapes.
    if line.len() > 64 * 1024 {
        return None;
    }
    let mut chars = line.chars().peekable();
    let mut clean = String::with_capacity(line.len().min(FIELD_LIMIT * 2));
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for part in chars.by_ref() {
                        if ('@'..='~').contains(&part) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(part) = chars.next() {
                        if part == '\u{7}' {
                            break;
                        }
                        if part == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if ch == '\r' || ch == '\n' {
            if chars.peek().is_some() {
                return None;
            }
        } else if ch.is_control() && ch != '\t' {
            return None;
        } else {
            clean.push(ch);
        }
    }
    Some(clean)
}
