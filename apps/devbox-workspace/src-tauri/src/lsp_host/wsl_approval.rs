//! Windows owns saved configuration and one-use approval. Linux observes code
//! in the selected running distro; no command/probe runs while reviewing it.
use super::{
    approval::{record, Record, FILE},
    settings::Settings,
};
use crate::{
    definitions::{Definitions, ExecutionDefinitions},
    host::Host,
    platform::{definition_write::DefinitionTarget, wsl_project::WslProjectLease},
};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Command {
    language_id: String,
    executable: String,
    args: Vec<String>,
    runtime: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeReview {
    digest: String,
    commands: Vec<Command>,
    environment_keys: Vec<String>,
}
impl NativeReview {
    fn decode(value: Value) -> Result<Self> {
        let review: Self = serde_json::from_value(value).map_err(|_| "wsl_protocol_invalid")?;
        if review.digest.len() != 64
            || !review.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            || review.commands.is_empty()
            || review.commands.len() > 64
            || !matches!(review.environment_keys.as_slice(), [home] if home == "HOME")
                && review.environment_keys != ["HOME", "PATH"]
        {
            return Err("wsl_protocol_invalid");
        }
        let mut languages = std::collections::BTreeSet::new();
        for command in &review.commands {
            if !languages.insert(&command.language_id)
                || ![
                    "rust",
                    "javascript",
                    "typescript",
                    "python",
                    "html",
                    "css",
                    "json",
                ]
                .contains(&command.language_id.as_str())
            {
                return Err("wsl_protocol_invalid");
            }
            for path in std::iter::once(&command.executable).chain(command.runtime.iter()) {
                let parsed = devbox_filesystem::parse_safe_project_path(path)
                    .ok_or("wsl_protocol_invalid")?;
                if parsed.kind() != devbox_filesystem::ProjectPathKind::Posix {
                    return Err("wsl_protocol_invalid");
                }
            }
        }
        Ok(review)
    }
}
pub(super) struct Snapshot {
    host: Arc<Host>,
    settings: Settings,
    lease: WslProjectLease,
    definitions: ExecutionDefinitions,
    native: NativeReview,
    approval_bytes: Option<Vec<u8>>,
    digest: String,
}
impl Snapshot {
    pub(super) fn capture(
        host: Arc<Host>,
        context: &ProjectContext,
        deadline: u64,
        require_existing: bool,
    ) -> Result<Self> {
        crate::files_host::current_deadline(deadline)?;
        let settings = Settings::open(&host, context)?;
        let config = settings.execution_config()?;
        let approval_bytes = settings.private().read(FILE)?;
        let approval = record(approval_bytes.as_deref(), context)?;
        if require_existing
            && approval.as_ref().is_none_or(|record| {
                record.config_revision != settings.revision().unwrap_or_default()
            })
        {
            return Err("lsp_execution_approval_required");
        }
        let lease = host
            .projects()?
            .admit_wsl(host.helper_directory()?, context)?;
        let definitions = Definitions::default().execution_evidence(&host, context, deadline)?;
        let native = NativeReview::decode(lease.file_request_until(
            context,
            "lsp_capture",
            json!({"config":config}),
            deadline,
        )?)?;
        let digest = crate::definitions::digest(
            &serde_json::to_vec(&(settings.revision()?, definitions.digest(), &native.digest))
                .map_err(|_| "lsp_source_path_invalid")?,
        );
        let snapshot = Self {
            host,
            settings,
            lease,
            definitions,
            native,
            approval_bytes,
            digest,
        };
        snapshot.revalidate(deadline)?;
        if require_existing && !snapshot.approved()? {
            return Err("lsp_execution_approval_required");
        }
        Ok(snapshot)
    }
    pub(super) fn context(&self) -> &ProjectContext {
        &self.settings.context
    }
    pub(super) fn approved(&self) -> Result<bool> {
        Ok(record(self.approval_bytes.as_deref(), self.context())?
            .is_some_and(|record| record.digest == self.digest))
    }
    pub(super) fn metadata(&self, deadline: u64) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        self.settings.revalidate(&self.host)?;
        if self.lease.binding() != self.settings.binding() {
            return Err("lsp_context_changed");
        }
        self.definitions.revalidate_until(deadline)?;
        if self.settings.private().read(FILE)? != self.approval_bytes {
            return Err("lsp_execution_approval_required");
        }
        crate::files_host::current_deadline(deadline)
    }
    fn revalidate(&self, deadline: u64) -> Result<()> {
        self.metadata(deadline)?;
        self.lease.file_request_until(
            self.context(),
            "lsp_validate",
            json!({"digest":self.native.digest}),
            deadline,
        )?;
        self.metadata(deadline)
    }
    pub(super) fn host(&self) -> &Host {
        &self.host
    }
    pub(super) fn root(&self) -> &str {
        &self.settings.binding().root
    }
    pub(super) fn shutdown(&self) -> Result<()> {
        self.lease.shutdown()
    }
    pub(super) fn request(
        &self,
        method: &str,
        args: Value,
        document: Option<workspace_wsl::lsp_wire::DocumentProof>,
        deadline: u64,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        authorize: &dyn Fn(&str) -> Result<()>,
    ) -> Result<Value> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis();
        let remaining = u128::from(deadline).saturating_sub(now).min(29000) as u64;
        if remaining == 0 {
            return Err("request_expired");
        }
        self.lease.execute_lsp(
            self.context(),
            json!({"digest":self.native.digest,"method":method,"args":args,"document":document}),
            std::time::Instant::now() + std::time::Duration::from_millis(remaining),
            cancelled,
            authorize,
        )
    }
    pub(super) fn view(&self) -> Result<Value> {
        Ok(
            json!({"approved":self.approved()?,"workspaceRoot":self.settings.binding().root,"configRevision":self.settings.revision()?,"commands":self.native.commands,"environmentKeys":self.native.environment_keys,"definitionsDigest":self.definitions.digest()}),
        )
    }
    pub(super) fn approve(&self, deadline: u64) -> Result<()> {
        self.revalidate(deadline)?;
        let bytes = serde_json::to_vec(&Record {
            schema_version: 1,
            context: self.context().clone(),
            config_revision: self.settings.revision()?,
            digest: self.digest.clone(),
        })
        .map_err(|_| "lsp_approval_invalid")?;
        DefinitionTarget::capture(
            &self.settings.private().path().join(FILE),
            self.approval_bytes.as_deref(),
        )?
        .write_utf8(&bytes, || self.metadata(deadline))
        .map(|_| ())
    }
}

#[cfg(test)]
pub(crate) fn check_owned_fixture(
    host: Arc<Host>,
    context: &ProjectContext,
    program: &str,
    disk: &std::path::Path,
) {
    let deadline = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 29000
    };
    let settings = Settings::open(&host, context).unwrap();
    let mut config = settings.view().unwrap()["config"].clone();
    config["enabled"] = json!(true);
    let marker = format!("{}/must-not-execute-lsp", settings.binding().root);
    config["server_by_language"] =
        json!({"rust":{"kind":"custom","executable":program,"args":[marker]}});
    let revision = settings.revision().unwrap();
    settings
        .save(
            &host,
            json!({"config":config,"nativeRevision":revision,"recoverInvalid":false}),
            deadline(),
        )
        .unwrap();
    assert!(matches!(
        Snapshot::capture(host.clone(), context, deadline(), true),
        Err("lsp_execution_approval_required")
    ));
    let mut approvals = super::approval::Approvals::default();
    let review = approvals
        .preview(Snapshot::capture(host.clone(), context, deadline(), false).unwrap())
        .unwrap();
    assert_eq!(review["commands"][0]["executable"], program);
    assert_eq!(review["approved"], false);
    let token = review["previewId"].as_str().unwrap();
    approvals.approve(context, token, deadline()).unwrap();
    assert_eq!(
        approvals.approve(context, token, deadline()).unwrap_err(),
        "lsp_review_expired"
    );
    let approved = Snapshot::capture(host.clone(), context, deadline(), true).unwrap();
    assert!(approved.approved().unwrap());
    approved.revalidate(deadline()).unwrap();
    let pending = approvals
        .preview(Snapshot::capture(host.clone(), context, deadline(), false).unwrap())
        .unwrap();
    let original = std::fs::read(disk).unwrap();
    std::fs::write(disk, b"changed native code").unwrap();
    assert_eq!(
        approvals
            .approve(context, pending["previewId"].as_str().unwrap(), deadline())
            .unwrap_err(),
        "lsp_sources_changed"
    );
    assert_eq!(
        approved.revalidate(deadline()).unwrap_err(),
        "lsp_sources_changed"
    );
    std::fs::write(disk, original).unwrap();
    approved.revalidate(deadline()).unwrap();
    approvals.revoke(&host, context, deadline()).unwrap();
    assert_eq!(
        approved.revalidate(deadline()).unwrap_err(),
        "lsp_execution_approval_required"
    );
    assert!(!disk.parent().unwrap().join("must-not-execute-lsp").exists());
    println!("WSL LSP review: native command evidence, one-use approval, code mutation and revocation passed without execution");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_review_rejects_foreign_transports_and_unknown_environment() {
        let view = json!({"digest":"a".repeat(64), "commands":[{"languageId":"rust","executable":"/usr/bin/server","args":[],"runtime":null}], "environmentKeys":["HOME","PATH"]});
        NativeReview::decode(view.clone()).unwrap();
        for path in [
            r"C:\server.exe",
            r"\\wsl.localhost\Foreign\server",
            "relative",
            "/a/../server",
        ] {
            let mut invalid = view.clone();
            invalid["commands"][0]["executable"] = json!(path);
            assert!(NativeReview::decode(invalid).is_err());
        }
        let mut invalid = view;
        invalid["environmentKeys"] = json!(["HOME", "TOKEN"]);
        assert!(NativeReview::decode(invalid).is_err());
    }
}
