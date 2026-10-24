use crate::model::*;
use devbox_protocol::{Exporter, method, topic};
method!(pub DepsList = "deps.list", query, ProjectQuery => DependencyInventory, DepsError);
method!(pub DepsAudit = "deps.audit", mutation, ProjectQuery => AuditJob, DepsError);
method!(pub DepsAuditStatus = "deps.audit_status", query, JobQuery => AuditJob, DepsError);
topic!(pub DepsAuditChanged = "deps.audit_changed", AuditProgress);
pub fn export<X: Exporter>(x: &mut X) {
    x.method::<DepsList>();
    x.method::<DepsAudit>();
    x.method::<DepsAuditStatus>();
    x.topic::<DepsAuditChanged>();
}
