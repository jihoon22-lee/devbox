//! Bounded command metadata. A descriptor or a palette selection grants no authority.
use crate::ProjectContext;
use serde::{Deserialize, Serialize};

pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_COMMANDS: usize = 2048;
pub const MAX_RESULTS: usize = 256;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextRequirement {
    None,
    Project,
    Selection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisabledReason {
    NotInstalled,
    VersionMismatch,
    ProviderUnavailable,
    ContextRequired,
    SelectionRequired,
    Stale,
    PermissionDenied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    Project,
    Repository,
    Worktree,
    Task,
    Service,
    Run,
    File,
    Note,
    SavedQuery,
    TerminalProfile,
    TerminalWindow,
    Session,
    Capture,
    Transform,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Target {
    Route { route: String },
    Entity { entity: EntityKind, id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub owner: String,
    pub component: String,
    pub label: String,
    pub revision: String,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_route: Option<String>,
    pub required_context: ContextRequirement,
    pub context: Option<ProjectContext>,
    pub destructive: bool,
    pub requires_review: bool,
    pub disabled_reason: Option<DisabledReason>,
}

/// Only references and typed context cross the command boundary, never argv/body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub operation_id: String,
    pub command_id: String,
    pub revision: String,
    pub context: Option<ProjectContext>,
    pub selection_id: Option<String>,
}

pub fn opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !applink::contains_sensitive_value(value)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub fn revision(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
fn slug(value: &str) -> bool {
    opaque_id(value) && !value.contains('_') && value.bytes().all(|b| !b.is_ascii_uppercase())
}
impl Descriptor {
    pub fn validate(&self) -> Result<()> {
        if !["workspace", "api-studio", "knowledge", "control-center"]
            .contains(&self.owner.as_str())
            || !self.id.starts_with(&(self.owner.clone() + "."))
            || self.id.len() > 256
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            || self
                .component
                .strip_prefix(&(self.owner.clone() + "."))
                .is_none_or(|part| !slug(part))
            || self.label.trim().is_empty()
            || self.label.len() > 256
            || self.label.chars().any(char::is_control)
            || applink::contains_sensitive_value(&self.label)
            || !revision(&self.revision)
            || self
                .context
                .as_ref()
                .is_some_and(|context| context.validate().is_err())
            || self.review_route.as_ref().is_some_and(|route| !slug(route))
            || (self.destructive && !self.requires_review)
        {
            return Err("command_metadata_invalid");
        }
        match &self.target {
            Target::Route { route } if !slug(route) => Err("command_target_invalid"),
            Target::Entity { id, .. } if !opaque_id(id) => Err("command_target_invalid"),
            _ => Ok(()),
        }
    }
    /// The native provider still resolves the current target and owns its review.
    pub fn validate_request(&self, request: &Request) -> Result<()> {
        self.validate()?;
        if !opaque_id(&request.operation_id)
            || request.command_id != self.id
            || request.revision != self.revision
            || request
                .context
                .as_ref()
                .is_some_and(|context| context.validate().is_err())
            || request
                .selection_id
                .as_ref()
                .is_some_and(|id| !opaque_id(id))
        {
            return Err("command_request_stale");
        }
        if self.disabled_reason.is_some() {
            return Err("command_disabled");
        }
        if self.context.is_some() && self.context != request.context {
            return Err("command_context_changed");
        }
        if self.required_context == ContextRequirement::Project && request.context.is_none() {
            return Err("command_context_required");
        }
        if self.required_context == ContextRequirement::Selection && request.selection_id.is_none()
        {
            return Err("command_selection_required");
        }
        Ok(())
    }
}

pub fn validate_query(value: &str) -> Result<()> {
    if value.len() > MAX_QUERY_BYTES
        || value.chars().any(char::is_control)
        || applink::contains_sensitive_value(value)
    {
        return Err("command_query_invalid");
    }
    Ok(())
}

/// Legacy Launcher ranking preserved: first matching field, exact/prefix/substring.
/// Callers rank all bounded source entries before applying their result cap.
pub fn match_fields<'a>(fields: impl IntoIterator<Item = &'a str>, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(20);
    }
    fields
        .into_iter()
        .take(8)
        .enumerate()
        .find_map(|(index, field)| {
            let value = field.to_lowercase();
            let base = (index * 3) as u8;
            if value == needle {
                Some(base)
            } else if value.starts_with(needle) {
                Some(base + 1)
            } else if value.contains(needle) {
                Some(base + 2)
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> Descriptor {
        Descriptor {
            id: "workspace.open-files".into(),
            owner: "workspace".into(),
            component: "workspace.shell".into(),
            label: "파일".into(),
            revision: "a".repeat(64),
            target: Target::Route {
                route: "files".into(),
            },
            review_route: None,
            required_context: ContextRequirement::None,
            context: None,
            destructive: false,
            requires_review: false,
            disabled_reason: None,
        }
    }
    fn request() -> Request {
        Request {
            operation_id: "synthetic-operation".into(),
            command_id: "workspace.open-files".into(),
            revision: "a".repeat(64),
            context: None,
            selection_id: None,
        }
    }
    #[test]
    fn stale_disabled_and_forged_raw_payloads_do_not_dispatch() {
        let mut command = descriptor();
        assert!(command.validate_request(&request()).is_ok());
        let mut stale = request();
        stale.revision = "b".repeat(64);
        assert_eq!(
            command.validate_request(&stale),
            Err("command_request_stale")
        );
        command.disabled_reason = Some(DisabledReason::NotInstalled);
        assert_eq!(
            command.validate_request(&request()),
            Err("command_disabled")
        );
        let mut value = serde_json::to_value(request()).unwrap();
        value["argv"] = serde_json::json!(["sh"]);
        assert!(serde_json::from_value::<Request>(value).is_err());
        command = descriptor();
        command.target = Target::Entity {
            entity: EntityKind::File,
            id: "C:\\private.txt".into(),
        };
        assert!(command.validate().is_err());
    }
    #[test]
    fn destructive_commands_require_review_and_selection_is_only_a_reference() {
        let mut command = descriptor();
        command.destructive = true;
        assert!(command.validate().is_err());
        command.requires_review = true;
        command.required_context = ContextRequirement::Selection;
        assert_eq!(
            command.validate_request(&request()),
            Err("command_selection_required")
        );
        let mut selected = request();
        selected.selection_id = Some("owned-selection".into());
        assert!(command.validate_request(&selected).is_ok());
    }
    #[test]
    fn ranking_preserves_field_priority_and_unicode() {
        assert_eq!(match_fields(["작업 파일", "task"], "작업"), Some(1));
        assert_eq!(match_fields(["other", "task"], "task"), Some(3));
        assert_eq!(match_fields(["task suffix", "task"], "task"), Some(1));
        assert_eq!(match_fields(["other"], ""), Some(20));
    }
}
