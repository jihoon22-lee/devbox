//! Supported, bounded lock schemas. Unknown formats fail explicitly.
use crate::model::*;
use std::collections::BTreeMap;
mod cargo;
mod go;
mod node;
mod pnpm;
mod python;
mod yarn;
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ITEMS: usize = 10_000;
pub type Manifests = BTreeMap<String, String>;
#[derive(Default)]
pub(super) struct Inventory {
    items: BTreeMap<(Ecosystem, String), BTreeMap<(String, DependencySource), bool>>,
    count: usize,
}
impl Inventory {
    pub fn add(
        &mut self,
        ecosystem: Ecosystem,
        name: &str,
        version: &str,
        source: DependencySource,
        direct: bool,
    ) -> Result<(), DepsError> {
        if name.is_empty()
            || version.is_empty()
            || name.len() > 512
            || version.len() > 512
            || name.chars().any(char::is_control)
            || version.chars().any(char::is_control)
        {
            return Err(DepsError::LimitExceeded);
        }
        let versions = self.items.entry((ecosystem, name.into())).or_default();
        let entry = versions.entry((version.into(), source));
        if matches!(entry, std::collections::btree_map::Entry::Vacant(_)) {
            self.count += 1;
        }
        *entry.or_default() |= direct;
        if self.count > MAX_ITEMS {
            return Err(DepsError::LimitExceeded);
        }
        Ok(())
    }
    fn finish(self) -> Vec<Dependency> {
        self.items
            .into_iter()
            .map(|((ecosystem, name), vs)| Dependency {
                ecosystem,
                name,
                direct: vs.values().any(|x| *x),
                versions: vs
                    .into_iter()
                    .map(|((version, source), direct)| DependencyVersion {
                        version,
                        source,
                        direct,
                    })
                    .collect(),
            })
            .collect()
    }
}
pub fn parse(
    kind: LockKind,
    text: &str,
    manifests: &Manifests,
) -> Result<Vec<Dependency>, DepsError> {
    if text.len() > MAX_BYTES || manifests.values().any(|s| s.len() > MAX_BYTES) {
        return Err(DepsError::LimitExceeded);
    }
    let mut out = Inventory::default();
    match kind {
        LockKind::Pnpm => pnpm::parse(text, &mut out),
        LockKind::Npm => node::parse(text, &mut out),
        LockKind::Yarn => yarn::parse(text, manifests, &mut out),
        LockKind::Cargo => cargo::parse(text, manifests, &mut out),
        LockKind::Go => go::parse(text, manifests, &mut out),
        LockKind::Poetry | LockKind::Uv => python::parse(kind, text, manifests, &mut out),
    }?;
    Ok(out.finish())
}
pub(super) fn malformed(lock: LockKind) -> DepsError {
    DepsError::MalformedLock { lock }
}
pub(super) fn manifest<'a>(
    ms: &'a Manifests,
    path: &str,
    lock: LockKind,
) -> Result<&'a str, DepsError> {
    ms.get(path)
        .map(String::as_str)
        .ok_or(DepsError::MissingManifest { lock })
}
pub(super) fn yaml<T: serde::de::DeserializeOwned>(
    text: &str,
    kind: LockKind,
) -> Result<Vec<T>, DepsError> {
    let options = serde_saphyr::options! {
        with_snippet:false, reject_unsupported_tags:true,
        budget:serde_saphyr::budget! { max_events:200_000,max_nodes:100_000,max_depth:32,max_documents:4,max_aliases:0,max_anchors:0,max_total_scalar_bytes:MAX_BYTES, },
    };
    serde_saphyr::from_multiple_with_options(text, options).map_err(|_| malformed(kind))
}
pub(crate) fn cargo_manifests(text: &str) -> Result<Vec<String>, DepsError> {
    cargo::manifests(text)
}
