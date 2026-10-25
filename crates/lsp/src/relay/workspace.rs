//! Workspace provenance validation and one-shot renderer acknowledgements.
use super::*;

impl Relay {
    pub(super) async fn workspace_control(
        &mut self,
        client: u32,
        method: &str,
        value: Value,
        access: &DocumentAccess,
    ) -> Result<RelayOutput, LspError> {
        let mut out = RelayOutput::default();
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        if method == "devbox/finishWorkspaceEdit" {
            if let Ok(id) = serde_json::from_value::<crate::WorkspaceIdentity>(params) {
                self.proofs.finish(client, &self.root, self.generation, &id);
            }
            return Ok(out);
        }
        let id = request_id(&value)?;
        let valid = if method == "devbox/validateWorkspaceEdit" {
            if let Ok(request) = serde_json::from_value::<crate::WorkspaceValidation>(params) {
                self.validate_workspace(client, request, access)
                    .await
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };
        out.clients.push((
            client,
            json!({"jsonrpc":"2.0","id":id,"result":valid}).to_string(),
        ));
        Ok(out)
    }
    async fn validate_workspace(
        &mut self,
        client: u32,
        request: crate::WorkspaceValidation,
        access: &DocumentAccess,
    ) -> Result<(), LspError> {
        let captured = self
            .proofs
            .validation(client, &self.root, self.generation, &request)?;
        let open = self.documents.snapshots(client);
        for expected in captured.targets.as_ref().ok_or(LspError::InvalidInput)? {
            if expected.lease_id.is_some() && open.get(&expected.uri) != Some(expected) {
                return Err(LspError::LeaseLost);
            }
        }
        // Dirty buffers can be much smaller than their raw files. Bound the final
        // authority pass independently instead of assuming buffer bytes bound disk IO.
        let raw_bytes = request
            .targets
            .iter()
            .try_fold(0_u64, |sum, target| {
                sum.checked_add(target.disk_rev.as_ref().map_or(0, |rev| rev.size))
            })
            .ok_or(LspError::LimitExceeded)?;
        if raw_bytes > 32 * 1024 * 1024 {
            return Err(LspError::LimitExceeded);
        }
        let started = std::time::Instant::now();
        for current in &request.targets {
            if !self
                .proofs
                .matches(client, &self.root, self.generation, &request.identity)
            {
                return Err(LspError::InvalidInput);
            }
            if started.elapsed() >= std::time::Duration::from_secs(2) {
                return Err(LspError::LimitExceeded);
            }
            let disk = access(current.uri.clone(), current.lease_id.clone()).await?;
            if disk.uri != current.uri
                || disk.disk_rev != current.disk_rev
                || started.elapsed() >= std::time::Duration::from_secs(2)
            {
                return Err(LspError::InvalidInput);
            }
        }
        // Authority reads are awaited, but the actor serializes state while checking.
        if !self
            .proofs
            .matches(client, &self.root, self.generation, &request.identity)
        {
            return Err(LspError::InvalidInput);
        }
        self.proofs.validated(&request.identity.token);
        Ok(())
    }
    pub(super) fn workspace_response(&mut self, pending: &Pending, value: &mut Value) {
        if pending.workspace_edit {
            if let Some(edit) = value.get("result").cloned() {
                match self.proofs.response(
                    pending.client,
                    &pending.id,
                    &self.root,
                    self.generation,
                    &edit,
                ) {
                    Ok(proof) => value["result"] = json!({"edit": edit, "proof": proof}),
                    Err(_) => {
                        if let Ok(hints) = crate::workspace::targets(&edit) {
                            self.capture_hints.insert(pending.client, hints);
                        }
                        self.proofs.cancel(pending.client, &pending.id);
                        *value =
                            serde_json::from_str(&crate::workspace::refused(pending.id.clone()))
                                .expect("refusal");
                    }
                }
            } else {
                self.proofs.cancel(pending.client, &pending.id);
            }
        }
    }
}
