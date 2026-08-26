use crate::commands::docs::{resolve_root, AppState};
use crate::core::db::{self, LinkResolution};
use serde::Serialize;
use std::sync::Arc;

const MAX_ANALYSIS_BYTES: usize = 10 * 1024 * 1024;
const MAX_CANDIDATE_QUERY_BYTES: usize = 256;
const WIKILINK_ERROR: &str = "위키링크 정보를 불러올 수 없습니다";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WikilinkOccurrence {
    pub target: String,
    pub label: String,
    pub line: usize,
    pub column: usize,
    pub from: usize,
    pub to: usize,
    pub status: &'static str,
    pub resolved_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WikilinkCandidate {
    pub path: String,
    pub title: String,
    pub link_target: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Backlink {
    pub source_path: String,
    pub target: String,
    pub line: usize,
    pub column: usize,
}

#[tauri::command]
pub fn analyze_wikilinks(
    state: tauri::State<'_, Arc<AppState>>,
    content: String,
) -> Result<Vec<WikilinkOccurrence>, String> {
    if content.len() > MAX_ANALYSIS_BYTES {
        return Err(WIKILINK_ERROR.to_string());
    }
    let conn = state.db.lock().map_err(|_| WIKILINK_ERROR.to_string())?;
    db::analyze_wikilinks(&conn, &content)
        .map(|links| links.into_iter().map(occurrence_dto).collect())
        .map_err(|_| WIKILINK_ERROR.to_string())
}

#[tauri::command]
pub fn wikilink_candidates(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<WikilinkCandidate>, String> {
    if query.len() > MAX_CANDIDATE_QUERY_BYTES || query.contains(['\0', '\r', '\n']) {
        return Err(WIKILINK_ERROR.to_string());
    }
    let conn = state.db.lock().map_err(|_| WIKILINK_ERROR.to_string())?;
    db::wikilink_candidates(&conn, &query)
        .map(|candidates| {
            candidates
                .into_iter()
                .map(|candidate| WikilinkCandidate {
                    path: candidate.path,
                    title: candidate.title,
                    link_target: candidate.link_target,
                })
                .collect()
        })
        .map_err(|_| WIKILINK_ERROR.to_string())
}

#[tauri::command]
pub fn backlinks(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
) -> Result<Vec<Backlink>, String> {
    let conn = state.db.lock().map_err(|_| WIKILINK_ERROR.to_string())?;
    let root = resolve_root(&conn).map_err(|_| WIKILINK_ERROR.to_string())?;
    let resolved =
        crate::core::inbound::resolve_note(&root, &rel).map_err(|_| WIKILINK_ERROR.to_string())?;
    db::backlinks(&conn, &resolved.relative_path)
        .map(|links| {
            links
                .into_iter()
                .map(|link| Backlink {
                    source_path: link.source_path,
                    target: link.target,
                    line: link.line,
                    column: link.column,
                })
                .collect()
        })
        .map_err(|_| WIKILINK_ERROR.to_string())
}

fn occurrence_dto(link: db::AnalyzedWikilink) -> WikilinkOccurrence {
    let (status, resolved_path) = match link.resolution {
        LinkResolution::Resolved(path) => ("resolved", Some(path)),
        LinkResolution::Missing => ("missing", None),
        LinkResolution::Ambiguous => ("ambiguous", None),
        LinkResolution::Invalid => ("invalid", None),
    };
    WikilinkOccurrence {
        target: link.occurrence.target,
        label: link.occurrence.label,
        line: link.occurrence.line,
        column: link.occurrence.column,
        from: link.occurrence.from_utf16,
        to: link.occurrence.to_utf16,
        status,
        resolved_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occurrence_dto_never_uses_unresolved_target_as_a_path() {
        let parsed = crate::core::wikilink::parse_wikilinks("[[../secret]]")
            .into_iter()
            .next()
            .unwrap();
        let dto = occurrence_dto(db::AnalyzedWikilink {
            occurrence: parsed,
            resolution: LinkResolution::Invalid,
        });
        assert_eq!(dto.status, "invalid");
        assert_eq!(dto.resolved_path, None);
    }
}
