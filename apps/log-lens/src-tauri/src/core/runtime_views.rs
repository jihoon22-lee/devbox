//! Resolve only Runtime source revisions and preserve the selected filter's
//! identity. An unmatched filter remains unmatched, never becomes all sources.
use super::{model::run_source_parts, FilterSpec, SourceSpec};
pub fn rebase(
    sources: &mut [SourceSpec],
    filter: &mut FilterSpec,
    mut revision: impl FnMut(&str, bool) -> Option<String>,
) -> Result<usize, &'static str> {
    let mut unavailable = 0;
    for source in sources {
        source.validate().map_err(|_| "runtime_settings_invalid")?;
        let old = source.opaque_id();
        let replacement = match source {
            SourceSpec::Run { source_id } => {
                let (run, stream) =
                    run_source_parts(source_id).map_err(|_| "runtime_settings_invalid")?;
                let revision = revision(run, true).unwrap_or_else(|| {
                    unavailable += 1;
                    "0".repeat(64)
                });
                Some(SourceSpec::RuntimeRun {
                    run_id: run.into(),
                    stream: stream.into(),
                    revision,
                })
            }
            SourceSpec::RuntimeRun {
                run_id,
                stream,
                revision: current,
            } => {
                let next = revision(run_id, current == &"0".repeat(64));
                if next.is_none() {
                    unavailable += 1;
                }
                Some(SourceSpec::RuntimeRun {
                    run_id: run_id.clone(),
                    stream: stream.clone(),
                    revision: next.unwrap_or_else(|| current.clone()),
                })
            }
            _ => None,
        };
        if let Some(next) = replacement {
            if filter.source_id.as_deref() == Some(&old) {
                filter.source_id = Some(next.opaque_id());
            }
            *source = next;
        }
    }
    Ok(unavailable)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_mapping_keeps_a_disconnected_source_and_specific_filter() {
        let source = SourceSpec::Run {
            source_id: "run-manager:old-run:stdout".into(),
        };
        let mut filter = FilterSpec {
            source_id: Some(source.opaque_id()),
            ..Default::default()
        };
        let mut sources = vec![source];
        assert_eq!(rebase(&mut sources, &mut filter, |_, _| None).unwrap(), 1);
        assert_eq!(filter.source_id, Some(sources[0].opaque_id()));
        assert!(
            matches!(&sources[0],SourceSpec::RuntimeRun {revision,..} if revision==&"0".repeat(64))
        );
        rebase(&mut sources, &mut filter, |_, imported| {
            assert!(imported);
            Some("a".repeat(64))
        })
        .unwrap();
        assert_eq!(filter.source_id, Some(sources[0].opaque_id()));
    }
    #[test]
    fn stale_unmatched_filter_never_widens() {
        let mut filter = FilterSpec {
            source_id: Some("missing-specific-source".into()),
            ..Default::default()
        };
        let mut sources = vec![SourceSpec::Run {
            source_id: "run-manager:run:stderr".into(),
        }];
        rebase(&mut sources, &mut filter, |_, _| Some("b".repeat(64))).unwrap();
        assert_eq!(filter.source_id.as_deref(), Some("missing-specific-source"));
    }
}
