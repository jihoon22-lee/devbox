//! Closed v0.8 public asset topology. Declarations grant no download, launch or
//! update authority; the native adapter must bind the official release response.
use product_contract::installation::PRODUCTS;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, &'static str>;
pub const MAX_RELEASE_BYTES: usize = 256 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProductPackage {
    pub id: String,
    pub version: String,
    pub portable: Asset,
    pub files: Vec<Asset>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub schema_version: u32,
    pub release_tag: String,
    pub source_sha: String,
    pub suite_version: String,
    pub protocol_version: u32,
    pub setup: Asset,
    pub products: Vec<ProductPackage>,
    pub notices: Asset,
}
fn hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn version(s: &str) -> bool {
    s.len() <= 32
        && s.split('.').count() == 3
        && s.split('.').all(|p| {
            !p.is_empty()
                && p.len() <= 8
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p == "0" || !p.starts_with('0'))
        })
}
fn asset(a: &Asset) -> Result<()> {
    if !hash(&a.sha256) || a.size == 0 || a.size > MAX_PACKAGE_BYTES {
        return Err("suite_asset_invalid");
    }
    Ok(())
}
pub fn product_file(product: &str, name: &str) -> bool {
    name == format!("devbox-{product}.exe")
        || matches!(name, "THIRD_PARTY_NOTICES.md" | "devbox-installation.json")
        || (product == "workspace"
            && matches!(
                name,
                "resources/wsl/manifest.json" | "resources/wsl/devbox-workspace-wsl"
            ))
        || (product == "control-center" && name == "resources/suite/devbox-suite-bootstrap.exe")
}
impl Release {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_RELEASE_BYTES {
            return Err("suite_release_limit");
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "suite_release_invalid")?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 2
            || self.protocol_version != 1
            || !version(&self.suite_version)
            || self.release_tag != format!("v{}", self.suite_version)
            || self.source_sha.len() != 40
            || !self
                .source_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.products.len() != 4
        {
            return Err("suite_release_invalid");
        }
        asset(&self.setup)?;
        asset(&self.notices)?;
        if self.setup.name != format!("Devbox_{}_x64-setup.exe", self.suite_version)
            || self.notices.name != "THIRD_PARTY_NOTICES.md"
        {
            return Err("suite_asset_name_invalid");
        }
        let mut ids = BTreeSet::new();
        for product in &self.products {
            if !PRODUCTS.contains(&product.id.as_str())
                || !ids.insert(&product.id)
                || product.version != self.suite_version
                || product.portable.name
                    != format!("devbox-{}_{}_x64.zip", product.id, self.suite_version)
            {
                return Err("suite_product_invalid");
            }
            asset(&product.portable)?;
            let mut names = BTreeSet::new();
            let mut total = 0u64;
            for file in &product.files {
                asset(file)?;
                if !product_file(&product.id, &file.name) || !names.insert(file.name.as_str()) {
                    return Err("suite_package_file_invalid");
                }
                total = total
                    .checked_add(file.size)
                    .filter(|v| *v <= MAX_PACKAGE_BYTES)
                    .ok_or("suite_package_limit")?;
            }
            let exe = format!("devbox-{}.exe", product.id);
            let mut expected = vec![
                exe.as_str(),
                "THIRD_PARTY_NOTICES.md",
                "devbox-installation.json",
            ];
            if product.id == "workspace" {
                expected.extend([
                    "resources/wsl/manifest.json",
                    "resources/wsl/devbox-workspace-wsl",
                ]);
            }
            if product.id == "control-center" {
                expected.push("resources/suite/devbox-suite-bootstrap.exe");
            }
            if names != expected.into_iter().collect() {
                return Err("suite_package_incomplete");
            }
            if product
                .files
                .iter()
                .find(|f| f.name == "THIRD_PARTY_NOTICES.md")
                .is_none_or(|f| f.sha256 != self.notices.sha256 || f.size != self.notices.size)
            {
                return Err("suite_notices_mismatch");
            }
        }
        Ok(())
    }
    pub fn asset_url(&self, asset: &Asset) -> Result<String> {
        self.validate()?;
        if asset.name != self.setup.name
            && asset.name != self.notices.name
            && !self.products.iter().any(|p| p.portable.name == asset.name)
        {
            return Err("suite_asset_not_public");
        }
        let url = format!(
            "https://github.com/jihoon22-lee/devbox/releases/download/{}/{}",
            self.release_tag, asset.name
        );
        if !devbox_manager_lib::core::url_policy::is_allowed(&url) {
            return Err("suite_asset_url_denied");
        }
        Ok(url)
    }
}
