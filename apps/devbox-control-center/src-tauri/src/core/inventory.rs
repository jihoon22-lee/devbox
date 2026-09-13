//! Public inventory metadata. Absence from a verified portable declaration is
//! distinct from an unreadable installation; neither guesses an ARP uninstaller.
use devbox_catalog::products::{ProductCatalog, SOURCE};
use product_contract::installation::{Manifest, PRODUCTS};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub installation_key: Option<String>,
    pub generation: Option<String>,
    pub suite_version: Option<String>,
    pub declaration: &'static str,
    pub installer_registration: &'static str,
    pub products: Vec<Product>,
    pub components: Vec<Component>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    pub id: String,
    pub name: String,
    pub bundle_identifier: String,
    pub binary: &'static str,
    pub version: Option<String>,
    pub runtime: &'static str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: String,
    pub owner: String,
    pub binary: &'static str,
    pub runtime: &'static str,
    pub version: Option<String>,
}
pub fn observe(
    captured: Option<(&Manifest, &str, &BTreeMap<String, &'static str>)>,
) -> Result<Inventory, &'static str> {
    let catalog = ProductCatalog::parse(SOURCE)?;
    let state = |product: &str| match captured {
        Some((manifest, _, issues)) if manifest.members.iter().any(|m| m.product == product) => {
            if issues.contains_key(product) {
                "unknown"
            } else {
                "verified"
            }
        }
        Some(_) => "notIncluded",
        None => "unknown",
    };
    let version = |product: &str| {
        captured
            .filter(|_| state(product) == "verified")
            .map(|(manifest, _, _)| manifest.suite_version.clone())
    };
    Ok(Inventory {
        installation_key: captured.map(|(_, key, _)| key.into()),
        generation: captured.map(|(manifest, _, _)| manifest.generation.clone()),
        suite_version: captured.map(|(manifest, _, _)| manifest.suite_version.clone()),
        declaration: if captured.is_some() {
            "verified"
        } else {
            "unknown"
        },
        installer_registration: "unknown",
        products: PRODUCTS
            .iter()
            .map(|id| Product {
                id: (*id).into(),
                name: catalog
                    .products
                    .iter()
                    .find(|p| p.id == *id)
                    .map_or_else(|| (*id).into(), |p| p.label.clone()),
                bundle_identifier: catalog
                    .products
                    .iter()
                    .find(|p| p.id == *id)
                    .map(|p| p.identifier.clone())
                    .unwrap_or_default(),
                binary: state(id),
                version: version(id),
                runtime: "unknown",
            })
            .collect(),
        components: catalog
            .components
            .iter()
            .map(|component| Component {
                id: component.id.clone(),
                owner: component.owner.clone(),
                binary: state(&component.owner),
                runtime: "unknown",
                version: version(&component.owner),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_scope_is_unknown_and_never_an_empty_installed_list() {
        let result = observe(None).unwrap();
        assert_eq!(result.products.len(), 4);
        assert!(result.products.iter().all(|p| p.binary == "unknown"));
        assert_eq!(result.installer_registration, "unknown");
    }
}
