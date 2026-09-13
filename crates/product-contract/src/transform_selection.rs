//! Native source-owned selection. Only masked text crosses the product pipe.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub source: String,
    pub text: String,
    pub redacted: bool,
}
impl Selection {
    pub fn prepare(source: &str, text: &str) -> Result<Self, &'static str> {
        if !matches!(source, "code-pad" | "log-lens") || text.len() > 128 * 1024 {
            return Err("selection_invalid");
        }
        let (payload, redacted) = applink::ToolboxTextPayload::from_selected_text(source, text)?;
        Ok(Self {
            source: source.into(),
            text: payload.text,
            redacted,
        })
    }
    pub fn revision(&self) -> Result<String, &'static str> {
        if !matches!(self.source.as_str(), "code-pad" | "log-lens") || self.text.len() > 128 * 1024
        {
            return Err("selection_invalid");
        }
        applink::ToolboxTextPayload {
            text: self.text.clone(),
        }
        .validate()?;
        Ok(
            Sha256::digest(serde_json::to_vec(self).map_err(|_| "selection_invalid")?)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        )
    }
}
/// CodeMirror offsets count UTF-16 units. Reject split surrogate pairs.
pub fn selected_utf16(text: &str, from: usize, to: usize) -> Result<&str, &'static str> {
    if from >= to {
        return Err("selection_invalid");
    }
    let mut units = 0;
    let mut begin = None;
    for (byte, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if units == from {
            begin = Some(byte);
        }
        if units == to {
            return begin
                .and_then(|start| text.get(start..byte))
                .ok_or("selection_invalid");
        }
        if units > to {
            break;
        }
        units += character.len_utf16();
    }
    Err("selection_invalid")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selections_preserve_unicode_and_reject_non_exportable_producers() {
        assert_eq!(selected_utf16("가😀나", 1, 3).unwrap(), "😀");
        assert!(selected_utf16("가😀나", 2, 3).is_err());
        assert!(Selection::prepare("hmac", "synthetic").is_err());
        assert!(Selection::prepare("webhook-lab", "synthetic").is_err());
        let value =
            Selection::prepare("code-pad", "safe\nAuthorization: Bearer synthetic-secret").unwrap();
        assert!(value.redacted);
        assert!(!value.text.contains("synthetic-secret"));
    }
}
