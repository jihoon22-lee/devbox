use serde::Deserialize;
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Terminal,
    Agent,
    AgentsGrid,
    Note,
    Code,
    Capture,
}
impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Agent => "agent",
            Self::AgentsGrid => "agents-grid",
            Self::Note => "note",
            Self::Code => "code",
            Self::Capture => "capture",
        }
    }
    pub fn target(&self, id: Option<&str>) -> Option<(String, String)> {
        if id.is_some_and(|id| {
            id.is_empty() || matches!(id, "." | "..") || id.len() > 8192 || id.contains('\0')
        }) {
            return None;
        }
        if matches!(self, Self::Capture | Self::AgentsGrid) && id.is_some() {
            return None;
        }
        let suffix = id
            .map(|id| {
                id.as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            })
            .unwrap_or_else(|| "main".into());
        let path = id.map(|id| {
            id.as_bytes()
                .iter()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || b"-_.~".contains(b) {
                        (*b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        });
        Some((
            format!("popout-{}-{suffix}", self.name()),
            format!(
                "/popout/{}{}",
                self.name(),
                path.map(|p| format!("/{p}")).unwrap_or_default()
            ),
        ))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_and_label_encode_untrusted_ids_without_navigation() {
        let (label, path) = Kind::Code.target(Some("a/b?x#한")).unwrap();
        assert_eq!(label, "popout-code-612f623f7823ed959c");
        assert_eq!(path, "/popout/code/a%2Fb%3Fx%23%ED%95%9C");
        assert!(Kind::Capture.target(Some("other")).is_none());
        assert!(Kind::Note.target(Some("\0")).is_none());
    }
}
