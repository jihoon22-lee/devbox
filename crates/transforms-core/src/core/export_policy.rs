//! Fixed tool commands and output policy. Renderer flags never grant export.
use super::workflows::{self, PipelineStep, SavedPipelineMetadata, WorkflowMetadata};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OutputPolicy {
    Exportable,
    Sensitive,
    NonPersistable,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCommand {
    pub id: String,
    pub command_id: String,
    pub group: String,
    pub name: String,
    pub route: String,
    pub policy: OutputPolicy,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u8,
    pub product: String,
    pub component: String,
    pub tools: Vec<ToolCommand>,
}
pub fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../apps/api-studio-tools.json"))
            .expect("checked-in transform command manifest")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum OutputSource {
    Tool {
        #[serde(rename = "toolId")]
        tool_id: String,
    },
    Pipeline {
        #[serde(rename = "inputType")]
        input_type: String,
        steps: Vec<PipelineStep>,
    },
}
impl OutputSource {
    /// Validates identity/typed transitions only; every accepted output must
    /// still pass native redaction, bounds and recipient validation.
    pub fn require_exportable(&self) -> Result<(), &'static str> {
        match self {
            Self::Tool { tool_id } => {
                let tool = manifest().tools.iter().find(|tool| &tool.id == tool_id);
                if tool.is_none_or(|tool| tool.policy == OutputPolicy::NonPersistable) {
                    return Err("transform_export_denied");
                }
            }
            Self::Pipeline { input_type, steps } => {
                workflows::validate(&WorkflowMetadata {
                    pipelines: vec![SavedPipelineMetadata {
                        id: "export-preview".into(),
                        input_type: input_type.clone(),
                        steps: steps.clone(),
                        updated_at: 0,
                    }],
                    ..WorkflowMetadata::default()
                })
                .map_err(|_| "transform_export_denied")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_is_unique_and_unknown_or_hmac_exports_are_denied() {
        let manifest = manifest();
        assert_eq!(
            (
                manifest.schema_version,
                manifest.product.as_str(),
                manifest.component.as_str()
            ),
            (1, "api-studio", "api-studio.transforms")
        );
        assert_eq!(manifest.tools.len(), 21);
        let mut ids = std::collections::HashSet::new();
        for tool in &manifest.tools {
            assert!(ids.insert(&tool.id));
            assert_eq!(
                tool.command_id,
                format!("api-studio.transforms.{}", tool.id)
            );
            assert_eq!(tool.route, "transforms");
            assert!(!tool.group.is_empty() && !tool.name.is_empty());
            assert_eq!(
                OutputSource::Tool {
                    tool_id: tool.id.clone()
                }
                .require_exportable()
                .is_ok(),
                tool.id != "hmac"
            );
        }
        assert!(OutputSource::Tool {
            tool_id: "unknown".into()
        }
        .require_exportable()
        .is_err());
        assert!(serde_json::from_value::<OutputSource>(serde_json::json!({
            "kind": "tool", "toolId": "hmac", "exportable": true
        }))
        .is_err());
    }
    #[test]
    fn pipeline_checks_the_whole_chain_and_never_accepts_hmac() {
        let pipeline = |ids: &[&str]| OutputSource::Pipeline {
            input_type: "text".into(),
            steps: ids
                .iter()
                .map(|id| PipelineStep {
                    transformer_id: (*id).into(),
                })
                .collect(),
        };
        assert!(pipeline(&["json-parse", "json-format"])
            .require_exportable()
            .is_ok());
        for ids in [
            &[][..],
            &["json-format"][..],
            &["hmac"][..],
            &["json-parse", "hmac", "json-format"][..],
        ] {
            assert!(pipeline(ids).require_exportable().is_err());
        }
    }
}
