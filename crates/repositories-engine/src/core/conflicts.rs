//! Pure porcelain conflict records and operation markers.
use serde::Serialize;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
}
impl ConflictKind {
    pub fn choices(self) -> &'static [&'static str] {
        match self {
            Self::BothModified | Self::BothAdded => &["ours", "theirs", "content"],
            Self::AddedByUs | Self::DeletedByThem => &["ours", "delete"],
            Self::AddedByThem | Self::DeletedByUs => &["theirs", "delete"],
            Self::BothDeleted => &["delete"],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    pub kind: ConflictKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ConflictOperation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}
pub fn parse_unmerged(status: &str) -> Result<Vec<ConflictFile>, String> {
    let error = "conflict_operation_failed";
    if !status.is_empty() && !status.ends_with('\0') {
        return Err(error.into());
    }
    let mut records = status.split_terminator('\0');
    let mut result = Vec::new();
    while let Some(record) = records.next() {
        if record.starts_with("2 ") {
            records.next().ok_or(error)?;
            continue;
        }
        if !record.starts_with("u ") {
            continue;
        }
        let fields = record.splitn(11, ' ').collect::<Vec<_>>();
        if fields.len() != 11 || fields.iter().any(|value| value.is_empty()) {
            return Err(error.into());
        }
        let kind = match fields[1] {
            "UU" => ConflictKind::BothModified,
            "AA" => ConflictKind::BothAdded,
            "DD" => ConflictKind::BothDeleted,
            "AU" => ConflictKind::AddedByUs,
            "UA" => ConflictKind::AddedByThem,
            "DU" => ConflictKind::DeletedByUs,
            "UD" => ConflictKind::DeletedByThem,
            _ => return Err(error.into()),
        };
        result.push(ConflictFile {
            path: fields[10].into(),
            kind,
        });
    }
    Ok(result)
}
pub fn operation_from_markers(markers: &[&str]) -> Option<ConflictOperation> {
    if markers
        .iter()
        .any(|name| matches!(*name, "rebase-merge" | "rebase-apply"))
    {
        Some(ConflictOperation::Rebase)
    } else if markers.contains(&"MERGE_HEAD") {
        Some(ConflictOperation::Merge)
    } else if markers.contains(&"CHERRY_PICK_HEAD") {
        Some(ConflictOperation::CherryPick)
    } else if markers.contains(&"REVERT_HEAD") {
        Some(ConflictOperation::Revert)
    } else {
        None
    }
}
pub fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line.starts_with("<<<<<<< ") || line == "=======" || line.starts_with(">>>>>>> ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmerged_records_map_to_kinds() {
        let status = concat!(
            "u UU N... 100644 100644 100644 100644 a1 b1 c1 src/app.rs\0",
            "u DU N... 100644 000000 100644 100644 a2 b2 c2 gone.txt\0",
            "1 .M N... 100644 100644 100644 d e clean.txt\0",
            "u AA N... 000000 100644 100644 100644 a3 b3 c3 both new.txt\0",
        );
        let files = parse_unmerged(status).unwrap();
        assert_eq!(
            files,
            vec![
                ConflictFile {
                    path: "src/app.rs".into(),
                    kind: ConflictKind::BothModified
                },
                ConflictFile {
                    path: "gone.txt".into(),
                    kind: ConflictKind::DeletedByUs
                },
                ConflictFile {
                    path: "both new.txt".into(),
                    kind: ConflictKind::BothAdded
                },
            ]
        );
        assert!(parse_unmerged("u ZZ N... 1 2 3 4 a b c x\0").is_err());
    }

    #[test]
    fn operations_come_from_git_dir_markers() {
        assert_eq!(
            operation_from_markers(&["MERGE_HEAD", "ORIG_HEAD"]),
            Some(ConflictOperation::Merge)
        );
        assert_eq!(
            operation_from_markers(&["rebase-merge"]),
            Some(ConflictOperation::Rebase)
        );
        assert_eq!(
            operation_from_markers(&["rebase-apply"]),
            Some(ConflictOperation::Rebase)
        );
        assert_eq!(
            operation_from_markers(&["CHERRY_PICK_HEAD"]),
            Some(ConflictOperation::CherryPick)
        );
        assert_eq!(
            operation_from_markers(&["REVERT_HEAD"]),
            Some(ConflictOperation::Revert)
        );
        assert_eq!(operation_from_markers(&["ORIG_HEAD"]), None);
    }

    #[test]
    fn markers_must_start_a_line() {
        assert!(has_conflict_markers(
            "a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> other\n"
        ));
        assert!(!has_conflict_markers(
            "let arrow = \"<<<<<<< not a marker\";\n"
        ));
        assert!(!has_conflict_markers("plain text\n"));
    }

    #[test]
    fn deletion_conflicts_offer_delete_and_the_surviving_side() {
        assert_eq!(ConflictKind::DeletedByUs.choices(), &["theirs", "delete"]);
        assert_eq!(ConflictKind::DeletedByThem.choices(), &["ours", "delete"]);
        assert_eq!(
            ConflictKind::BothModified.choices(),
            &["ours", "theirs", "content"]
        );
    }
}

#[cfg(test)]
mod record_boundaries {
    use super::*;
    #[test]
    fn rename_source_names_are_not_misread_as_conflict_records() {
        assert!(
            parse_unmerged("2 R. N... 1 2 3 a b R100 new\0u UU fake name\0")
                .unwrap()
                .is_empty()
        );
        assert!(parse_unmerged("2 R. N... 1 2 3 a b R100 new\0").is_err());
        assert!(parse_unmerged("u UU N... 1 2 3 4 a b c file").is_err());
    }
}
