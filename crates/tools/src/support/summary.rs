use serde::Serialize;
use std::{collections::BTreeMap, path::Path};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileSummary {
    source: &'static str,
    readable: bool,
    sections: BTreeMap<&'static str, usize>,
    other_sections: usize,
}
pub(super) fn configuration(config: &Path) -> Vec<FileSummary> {
    ["config.toml", "config.local.toml"]
        .into_iter()
        .map(|source| {
            let table = super::files::read(&config.join(source), 1024 * 1024)
                .ok()
                .and_then(|text| text.parse::<toml::Table>().ok());
            let mut summary = FileSummary {
                source,
                readable: table.is_some(),
                sections: BTreeMap::new(),
                other_sections: 0,
            };
            if let Some(table) = table {
                let known = [
                    "projects", "pc", "windows", "terminal", "agents", "mcp", "secrets",
                ];
                for section in known {
                    if let Some(value) = table.get(section) {
                        summary
                            .sections
                            .insert(section, value.as_table().map_or(0, |value| value.len()));
                    }
                }
                summary.other_sections = table
                    .keys()
                    .filter(|key| !known.contains(&key.as_str()))
                    .count();
            }
            summary
        })
        .collect()
}
