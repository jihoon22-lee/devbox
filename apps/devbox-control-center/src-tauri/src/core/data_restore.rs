//! Directory ownership decisions for restartable, non-destructive restoration.
//! A receipt records physical identities, never paths supplied by a renderer.
use serde::{Deserialize, Serialize};
pub type Identity = (u64, u64);
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Namespace {
    pub original: Option<Identity>,
    pub prepared: Option<Identity>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Preserve,
    Publish,
    Complete,
}
impl Namespace {
    pub fn apply(
        &self,
        live: Option<Identity>,
        displaced: Option<Identity>,
        staged: Option<Identity>,
    ) -> Result<Step> {
        if self.original.is_some() && self.original == self.prepared {
            return Err("restore_identity_conflict");
        }
        if self.original.is_some()
            && live == self.original
            && displaced.is_none()
            && staged == self.prepared
        {
            return Ok(Step::Preserve);
        }
        if displaced == self.original
            && live.is_none()
            && staged == self.prepared
            && self.prepared.is_some()
        {
            return Ok(Step::Publish);
        }
        if live == self.prepared && displaced == self.original && staged.is_none() {
            return Ok(Step::Complete);
        }
        Err("restore_namespace_changed")
    }
    /// Reverse only owned renames. Keep any changes made during health in the
    /// operation's retained directory instead of overwriting or deleting them.
    pub fn rollback(
        &self,
        live: Option<Identity>,
        displaced: Option<Identity>,
        staged: Option<Identity>,
        retained: Option<Identity>,
    ) -> Result<Step> {
        if self.original.is_some() && self.original == self.prepared {
            return Err("restore_identity_conflict");
        }
        let prepared_accounted = match self.prepared {
            Some(id) => {
                (staged == Some(id) && retained.is_none())
                    || (retained == Some(id) && staged.is_none())
            }
            None => staged.is_none() && retained.is_none(),
        };
        if self.prepared.is_some()
            && live == self.prepared
            && displaced == self.original
            && staged.is_none()
            && retained.is_none()
        {
            return Ok(Step::Preserve);
        }
        if live.is_none()
            && displaced == self.original
            && self.original.is_some()
            && prepared_accounted
        {
            return Ok(Step::Publish);
        }
        if live == self.original && displaced.is_none() && prepared_accounted {
            return Ok(Step::Complete);
        }
        Err("restore_namespace_changed")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_rename_boundary_can_resume_or_preserve_and_undo() {
        let old = Some((1, 11));
        let new = Some((1, 12));
        let namespace = Namespace {
            original: old,
            prepared: new,
        };
        assert_eq!(namespace.apply(old, None, new), Ok(Step::Preserve));
        assert_eq!(namespace.apply(None, old, new), Ok(Step::Publish));
        assert_eq!(namespace.apply(new, old, None), Ok(Step::Complete));
        assert_eq!(namespace.rollback(old, None, new, None), Ok(Step::Complete));
        assert_eq!(namespace.rollback(None, old, new, None), Ok(Step::Publish));
        assert_eq!(namespace.rollback(new, old, None, None), Ok(Step::Preserve));
        assert_eq!(namespace.rollback(None, old, None, new), Ok(Step::Publish));
        assert_eq!(namespace.rollback(old, None, None, new), Ok(Step::Complete));
    }
    #[test]
    fn absent_namespaces_stay_absent_and_foreign_ones_are_never_adopted() {
        for original in [None, Some((1, 11))] {
            for prepared in [None, Some((1, 12))] {
                let namespace = Namespace { original, prepared };
                assert_eq!(
                    namespace.apply(prepared, original, None),
                    Ok(Step::Complete)
                );
                assert_eq!(
                    namespace.rollback(original, None, prepared, None),
                    Ok(Step::Complete)
                );
                assert!(namespace.apply(Some((1, 99)), original, prepared).is_err());
                assert!(namespace
                    .rollback(Some((1, 99)), original, prepared, None)
                    .is_err());
                if let Some(prepared) = prepared {
                    assert!(namespace
                        .apply(Some(prepared), original, Some(prepared))
                        .is_err());
                }
            }
        }
    }
}
