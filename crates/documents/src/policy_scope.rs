use crate::{DocKey, DocKind, DocumentsService};
impl DocumentsService {
    /// Project code must stay in the selected project, including through aliases.
    /// Agent roots are deliberately separate even if a broadly registered project
    /// contains them. Notes retain their existing vault/root resolution policy.
    pub(crate) async fn policy_scope(&self, key: &DocKey, kind: DocKind) -> Self {
        if kind != DocKind::Code || !key.root.starts_with("project:") {
            return self.clone();
        }
        let private = (self.s.roots)()
            .await
            .into_iter()
            .filter(|root| root.id.starts_with("agents:"))
            .map(|root| root.path)
            .collect();
        self.scoped_to(&key.root).excluding(private)
    }
}
