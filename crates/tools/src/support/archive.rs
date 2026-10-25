use super::{
    BundleInput, SupportBundle, files,
    redact::{Redactor, Values},
};
use crate::{R, ToolsError};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    version: String,
    instance: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    logs_omitted: Option<&'static str>,
    omitted_files: usize,
    files: Vec<String>,
    daemon_log_days: u8,
}
pub(super) fn build(mut input: BundleInput, values: Option<Values>) -> R<SupportBundle> {
    let logs_omitted = values.is_none().then_some("secrets_locked");
    let redact = Redactor::new(
        &input.paths.home,
        &input.notes_vault,
        values.unwrap_or_default(),
    )?;
    let mut entries = BTreeMap::<String, String>::new();
    for check in &mut input.doctor.checks {
        check.id = redact.text(&check.id)?;
        check.message = redact.text(&check.message)?;
        check.fix = check
            .fix
            .as_deref()
            .map(|text| redact.text(text))
            .transpose()?;
    }
    entries.insert(
        "doctor.json".into(),
        serde_json::to_string_pretty(&input.doctor).map_err(super::failed)?,
    );
    let metadata = Metadata {
        version: redact.text(&input.version)?,
        instance: redact.text(input.paths.instance.as_str())?,
    };
    entries.insert(
        "metadata.json".into(),
        serde_json::to_string_pretty(&metadata).map_err(super::failed)?,
    );
    entries.insert(
        "settings-summary.json".into(),
        serde_json::to_string_pretty(&super::summary::configuration(&input.paths.config_dir))
            .map_err(super::failed)?,
    );
    let mut omitted_files = 0;
    let mut collected_bytes: usize = entries.values().map(String::len).sum();
    if logs_omitted.is_none() {
        let (candidates, omitted) = files::candidates(&input.paths.log_dir)?;
        omitted_files += omitted;
        for (index, (path, crash)) in candidates.into_iter().enumerate() {
            match files::read(&path, files::MAX_FILE) {
                Ok(text) => {
                    let sanitized = redact.text(&text)?;
                    if collected_bytes.saturating_add(sanitized.len()) > files::MAX_TOTAL {
                        omitted_files += 1;
                        continue;
                    }
                    collected_bytes += sanitized.len();
                    entries.insert(
                        format!("{}/{index}.log", if crash { "crashes" } else { "logs" }),
                        sanitized,
                    );
                }
                Err(_) => omitted_files += 1,
            }
        }
    }
    // All archive entry names are fixed/generated: source filenames and paths
    // cannot disclose credentials or introduce traversal entries.
    let manifest = Manifest {
        logs_omitted,
        omitted_files,
        files: entries.keys().cloned().collect(),
        daemon_log_days: 7,
    };
    entries.insert(
        "manifest.json".into(),
        serde_json::to_string_pretty(&manifest).map_err(super::failed)?,
    );
    let mut total = 0usize;
    for text in entries.values_mut() {
        *text = redact.finish(text)?;
        total = total.saturating_add(text.len());
        if total > files::MAX_TOTAL {
            return Err(ToolsError::SupportTooLarge.into());
        }
    }
    let directory = files::directory(&input.paths.data_dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory).map_err(super::failed)?;
    files::private(temporary.as_file())?;
    {
        let mut zip = ZipWriter::new(temporary.as_file_mut());
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o600);
        for (name, text) in entries {
            zip.start_file(name, options).map_err(super::failed)?;
            zip.write_all(text.as_bytes()).map_err(super::failed)?;
        }
        zip.finish().map_err(super::failed)?;
    }
    temporary.as_file().sync_all().map_err(super::failed)?;
    let size_bytes = temporary.as_file().metadata().map_err(super::failed)?.len();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(super::failed)?
        .as_millis();
    let output = directory.join(format!(
        "devbox-support-{timestamp}-{}.zip",
        uuid::Uuid::new_v4().simple()
    ));
    files::no_symlinks(&directory)?;
    temporary
        .persist_noclobber(&output)
        .map_err(super::failed)?;
    Ok(SupportBundle {
        path: output.to_string_lossy().into_owned(),
        size_bytes,
        logs_omitted: logs_omitted.map(Into::into),
    })
}
