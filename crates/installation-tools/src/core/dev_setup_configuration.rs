//! Strict WinGet Configuration v3 package-only import and canonical export.
//!
//! An imported YAML document is data, never an execution plan.  This module
//! accepts only the native DSC v3 `Microsoft.WinGet/Package` resource against
//! the fixed `winget` source name, then renders a new app-owned document from the
//! small validated model.  No imported resource name, description, dependency,
//! command, path, module, registry setting, or arbitrary property crosses that
//! normalization boundary.

use serde::Deserialize;
use std::collections::HashSet;

pub const CONFIGURATION_SCHEMA: &str =
    "https://raw.githubusercontent.com/PowerShell/DSC/main/schemas/2023/08/config/document.json";
pub const CONFIGURATION_SCHEMA_VERSION: &str = "0.3";
pub const PACKAGE_RESOURCE_TYPE: &str = "Microsoft.WinGet/Package";
pub const MAX_CONFIGURATION_BYTES: usize = 256 * 1024;
pub const MAX_CONFIGURATION_LINES: usize = 4_096;
pub const MAX_CONFIGURATION_LINE_BYTES: usize = 8 * 1024;
pub const MAX_CONFIGURATION_INDENT: usize = 32;
pub const MAX_PACKAGE_RESOURCES: usize = 16;
pub const MAX_PACKAGE_ID_BYTES: usize = 128;
pub const MAX_PACKAGE_VERSION_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigurationFailure {
    TooLarge,
    UnsafeYaml,
    InvalidDocument,
    UnsupportedResource,
    InvalidPackage,
    TooManyResources,
    DuplicateResource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageConfiguration {
    pub packages: Vec<PackageRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRequirement {
    pub package_id: String,
    pub desired: PackageDesiredState,
    pub requested_agreement_acceptance: bool,
    pub declared_elevation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageDesiredState {
    Present,
    Latest,
    Version(String),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedConfiguration {
    #[serde(rename = "$schema")]
    schema: String,
    #[serde(default)]
    metadata: Option<ImportedRootMetadata>,
    resources: Vec<ImportedResource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedRootMetadata {
    winget: ImportedWingetMetadata,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedWingetMetadata {
    processor: ImportedProcessor,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ImportedProcessor {
    Identifier(String),
    Detail(ImportedProcessorDetail),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedProcessorDetail {
    identifier: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedResource {
    #[serde(rename = "type")]
    resource_type: String,
    name: String,
    properties: ImportedPackageProperties,
    #[serde(default)]
    metadata: Option<ImportedResourceMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedPackageProperties {
    id: String,
    source: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "useLatest")]
    use_latest: Option<bool>,
    #[serde(default, rename = "matchOption")]
    match_option: Option<String>,
    #[serde(default, rename = "installMode")]
    install_mode: Option<String>,
    #[serde(default, rename = "acceptAgreements")]
    accept_agreements: Option<bool>,
    #[serde(default, rename = "_exist")]
    exists: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedResourceMetadata {
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    winget: Option<ImportedResourceWingetMetadata>,
    #[serde(default, rename = "securityContext")]
    security_context: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedResourceWingetMetadata {
    #[serde(rename = "securityContext")]
    security_context: String,
}

/// Parse an external YAML document into the package-only model.
///
/// The lexical preflight rejects YAML aliases, anchors, tags, merge keys,
/// directives, and multiple documents before deserialization.  Together with
/// byte/line/indent bounds this prevents compact YAML features from expanding
/// into an unbounded in-memory graph before the typed allowlist is applied.
pub fn parse_configuration(input: &str) -> Result<PackageConfiguration, ConfigurationFailure> {
    preflight_yaml(input)?;
    let imported = serde_yaml_ng::from_str::<ImportedConfiguration>(input)
        .map_err(|_| ConfigurationFailure::InvalidDocument)?;
    if imported.schema != CONFIGURATION_SCHEMA {
        return Err(ConfigurationFailure::InvalidDocument);
    }
    if let Some(metadata) = imported.metadata {
        let processor = match metadata.winget.processor {
            ImportedProcessor::Identifier(identifier) => identifier,
            ImportedProcessor::Detail(detail) => detail.identifier,
        };
        if processor != "dscv3" {
            return Err(ConfigurationFailure::InvalidDocument);
        }
    }
    if imported.resources.is_empty() {
        return Err(ConfigurationFailure::InvalidDocument);
    }
    if imported.resources.len() > MAX_PACKAGE_RESOURCES {
        return Err(ConfigurationFailure::TooManyResources);
    }

    let mut resource_names = HashSet::with_capacity(imported.resources.len());
    let mut package_ids = HashSet::with_capacity(imported.resources.len());
    let mut packages = Vec::with_capacity(imported.resources.len());
    for resource in imported.resources {
        if resource.resource_type != PACKAGE_RESOURCE_TYPE {
            return Err(ConfigurationFailure::UnsupportedResource);
        }
        if !valid_resource_name(&resource.name)
            || !resource_names.insert(resource.name.to_lowercase())
        {
            return Err(ConfigurationFailure::DuplicateResource);
        }
        let properties = resource.properties;
        if !valid_package_id(&properties.id)
            || properties.source != "winget"
            || properties.exists == Some(false)
            || !matches!(properties.match_option.as_deref(), None | Some("equals"))
            || !matches!(
                properties.install_mode.as_deref(),
                None | Some("default") | Some("silent")
            )
            || !package_ids.insert(properties.id.to_ascii_lowercase())
        {
            return Err(ConfigurationFailure::InvalidPackage);
        }
        let desired = match (properties.version, properties.use_latest.unwrap_or(false)) {
            (Some(_), true) => return Err(ConfigurationFailure::InvalidPackage),
            (Some(version), false) if valid_version(&version) => {
                PackageDesiredState::Version(version)
            }
            (Some(_), false) => return Err(ConfigurationFailure::InvalidPackage),
            (None, true) => PackageDesiredState::Latest,
            (None, false) => PackageDesiredState::Present,
        };
        let declared_elevation = resource
            .metadata
            .as_ref()
            .map(validate_metadata)
            .transpose()?
            .unwrap_or(false);
        packages.push(PackageRequirement {
            package_id: properties.id,
            desired,
            requested_agreement_acceptance: properties.accept_agreements.unwrap_or(false),
            declared_elevation,
        });
    }

    Ok(PackageConfiguration { packages })
}

/// Render only validated package requirements as a fresh DSC v3 document.
/// Agreement acceptance is never copied silently from an imported file; the
/// caller enables it only after a separate explicit user confirmation.
pub fn render_configuration(
    packages: &[PackageRequirement],
    accept_agreements: bool,
) -> Result<String, ConfigurationFailure> {
    if packages.is_empty() || packages.len() > MAX_PACKAGE_RESOURCES {
        return Err(ConfigurationFailure::InvalidDocument);
    }
    let mut seen = HashSet::with_capacity(packages.len());
    let mut output = format!(
        "# yaml-language-server: $schema=https://aka.ms/configuration-dsc-schema/{CONFIGURATION_SCHEMA_VERSION}\n$schema: {CONFIGURATION_SCHEMA}\nmetadata:\n  winget:\n    processor:\n      identifier: dscv3\nresources:\n"
    );
    for (index, package) in packages.iter().enumerate() {
        if !valid_package_id(&package.package_id)
            || !seen.insert(package.package_id.to_ascii_lowercase())
        {
            return Err(ConfigurationFailure::InvalidPackage);
        }
        let id = yaml_string(&package.package_id);
        output.push_str(&format!(
            "  - type: {PACKAGE_RESOURCE_TYPE}\n    name: DevboxPackage{:02}\n    properties:\n      id: {id}\n      source: winget\n      matchOption: equals\n",
            index + 1
        ));
        match &package.desired {
            PackageDesiredState::Present => {}
            PackageDesiredState::Latest => output.push_str("      useLatest: true\n"),
            PackageDesiredState::Version(version) if valid_version(version) => {
                output.push_str(&format!("      version: {}\n", yaml_string(version)));
            }
            PackageDesiredState::Version(_) => {
                return Err(ConfigurationFailure::InvalidPackage);
            }
        }
        output.push_str("      installMode: silent\n");
        if accept_agreements {
            output.push_str("      acceptAgreements: true\n");
        }
        output.push_str(
            "    metadata:\n      description: Devbox reviewed package-only configuration\n",
        );
    }
    if output.len() > MAX_CONFIGURATION_BYTES {
        return Err(ConfigurationFailure::TooLarge);
    }
    Ok(output)
}

fn validate_metadata(metadata: &ImportedResourceMetadata) -> Result<bool, ConfigurationFailure> {
    if metadata
        .description
        .as_deref()
        .is_some_and(|description| !valid_display_text(description, 512))
    {
        return Err(ConfigurationFailure::InvalidPackage);
    }
    let direct = metadata.security_context.as_deref();
    let nested = metadata
        .winget
        .as_ref()
        .map(|winget| winget.security_context.as_str());
    if direct.is_some() && nested.is_some() {
        return Err(ConfigurationFailure::InvalidPackage);
    }
    let context = direct.or(nested);
    if !matches!(context, None | Some("current") | Some("elevated")) {
        return Err(ConfigurationFailure::InvalidPackage);
    }
    Ok(context == Some("elevated"))
}

fn preflight_yaml(input: &str) -> Result<(), ConfigurationFailure> {
    if input.is_empty()
        || input.len() > MAX_CONFIGURATION_BYTES
        || input.contains('\0')
        || input
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r'))
    {
        return Err(ConfigurationFailure::TooLarge);
    }
    let mut lines = 0_usize;
    for line in input.lines() {
        lines += 1;
        if lines > MAX_CONFIGURATION_LINES || line.len() > MAX_CONFIGURATION_LINE_BYTES {
            return Err(ConfigurationFailure::TooLarge);
        }
        if line.contains('\t')
            || line
                .chars()
                .take_while(|character| *character == ' ')
                .count()
                > MAX_CONFIGURATION_INDENT
        {
            return Err(ConfigurationFailure::UnsafeYaml);
        }
        let code = yaml_code_without_comment(line);
        let trimmed = code.trim();
        if is_yaml_document_marker(trimmed)
            || trimmed.starts_with('%')
            || contains_yaml_graph_token(code)
        {
            return Err(ConfigurationFailure::UnsafeYaml);
        }
    }
    Ok(())
}

fn is_yaml_document_marker(value: &str) -> bool {
    ["---", "..."].iter().any(|marker| {
        value == *marker
            || value
                .strip_prefix(marker)
                .and_then(|suffix| suffix.chars().next())
                .is_some_and(char::is_whitespace)
    })
}

fn yaml_code_without_comment(line: &str) -> &str {
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut previous = None;
    for (index, character) in line.char_indices() {
        if double_quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                double_quoted = false;
            }
        } else if single_quoted {
            if character == '\'' {
                single_quoted = false;
            }
        } else if character == '"' {
            double_quoted = true;
        } else if character == '\'' {
            single_quoted = true;
        } else if character == '#' && previous.is_none_or(char::is_whitespace) {
            return &line[..index];
        }
        previous = Some(character);
    }
    line
}

fn contains_yaml_graph_token(line: &str) -> bool {
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let characters = line.char_indices().collect::<Vec<_>>();
    for (position, (_, character)) in characters.iter().copied().enumerate() {
        if double_quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                double_quoted = false;
            }
            continue;
        }
        if single_quoted {
            if character == '\'' {
                single_quoted = false;
            }
            continue;
        }
        if character == '"' {
            double_quoted = true;
            continue;
        }
        if character == '\'' {
            single_quoted = true;
            continue;
        }
        let previous = position
            .checked_sub(1)
            .and_then(|index| characters.get(index))
            .map(|(_, value)| *value);
        let next = characters.get(position + 1).map(|(_, value)| *value);
        if character == '<' && next == Some('<') {
            return true;
        }
        if matches!(character, '&' | '*' | '!')
            && previous.is_none_or(|value| {
                value.is_whitespace() || matches!(value, ':' | ',' | '[' | '{' | '-')
            })
            && next.is_some_and(|value| !value.is_whitespace())
        {
            return true;
        }
    }
    false
}

fn valid_resource_name(value: &str) -> bool {
    valid_display_text(value, 128)
}

fn valid_display_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '\u{2028}' | '\u{2029}'))
}

pub fn valid_package_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_PACKAGE_ID_BYTES {
        return false;
    }
    let segments = value.split('.').collect::<Vec<_>>();
    (2..=8).contains(&segments.len())
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment.len() <= 32
                && segment
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && segment
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
}

fn valid_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PACKAGE_VERSION_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}

fn yaml_string(value: &str) -> String {
    serde_json::to_string(value).expect("validated UTF-8 strings always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v3(resources: &str) -> String {
        format!(
            "$schema: {CONFIGURATION_SCHEMA}\nmetadata:\n  winget:\n    processor:\n      identifier: dscv3\nresources:\n{resources}"
        )
    }

    #[test]
    fn imports_only_exact_native_package_resources() {
        let parsed = parse_configuration(&v3(
            "  - type: Microsoft.WinGet/Package\n    name: Git\n    properties:\n      id: Git.Git\n      source: winget\n      useLatest: true\n      installMode: silent\n      acceptAgreements: true\n    metadata:\n      description: Install Git\n      winget:\n        securityContext: elevated\n",
        ))
        .unwrap();
        assert_eq!(
            parsed.packages,
            vec![PackageRequirement {
                package_id: "Git.Git".into(),
                desired: PackageDesiredState::Latest,
                requested_agreement_acceptance: true,
                declared_elevation: true,
            }]
        );
    }

    #[test]
    fn accepts_the_legacy_scalar_dscv3_processor_marker() {
        let input = format!(
            "$schema: {CONFIGURATION_SCHEMA}\nmetadata:\n  winget:\n    processor: dscv3\nresources:\n  - type: Microsoft.WinGet/Package\n    name: VSCode\n    properties:\n      id: Microsoft.VisualStudioCode\n      source: winget\n"
        );
        assert!(parse_configuration(&input).is_ok());
    }

    #[test]
    fn canonical_export_round_trips_without_imported_metadata() {
        let packages = vec![
            PackageRequirement {
                package_id: "Git.Git".into(),
                desired: PackageDesiredState::Latest,
                requested_agreement_acceptance: true,
                declared_elevation: true,
            },
            PackageRequirement {
                package_id: "Microsoft.PowerShell".into(),
                desired: PackageDesiredState::Version("7.6.1".into()),
                requested_agreement_acceptance: false,
                declared_elevation: false,
            },
        ];
        let exported = render_configuration(&packages, false).unwrap();
        assert!(!exported.contains("acceptAgreements"));
        assert!(!exported.contains("securityContext"));
        let round_trip = parse_configuration(&exported).unwrap();
        assert_eq!(round_trip.packages.len(), 2);
        assert!(round_trip
            .packages
            .iter()
            .all(|package| !package.requested_agreement_acceptance && !package.declared_elevation));

        let apply = render_configuration(&packages[..1], true).unwrap();
        assert!(apply.contains("acceptAgreements: true"));
        assert!(!apply.contains("7.6.1"));
    }

    #[test]
    fn rejects_executable_registry_custom_source_and_removal_resources() {
        for resource_type in [
            "Microsoft.DSC.Transitional/RunCommandOnSet",
            "Microsoft.Windows/Registry",
            "Microsoft.WinGet.DSC/WinGetPackage",
        ] {
            let input = v3(&format!(
                "  - type: {resource_type}\n    name: Unsafe\n    properties:\n      id: Git.Git\n      source: winget\n"
            ));
            assert_eq!(
                parse_configuration(&input),
                Err(ConfigurationFailure::UnsupportedResource)
            );
        }
        for extra in [
            "      source: msstore\n",
            "      source: winget\n      _exist: false\n",
            "      source: winget\n      matchOption: containsCaseInsensitive\n",
            "      source: winget\n      installMode: interactive\n",
            "      source: winget\n      version: 1.0\n      useLatest: true\n",
        ] {
            let input = v3(&format!(
                "  - type: Microsoft.WinGet/Package\n    name: Unsafe\n    properties:\n      id: Git.Git\n{extra}"
            ));
            assert_eq!(
                parse_configuration(&input),
                Err(ConfigurationFailure::InvalidPackage)
            );
        }
    }

    #[test]
    fn rejects_unknown_fields_duplicates_and_unbounded_yaml_features() {
        let unknown = v3(
            "  - type: Microsoft.WinGet/Package\n    name: Git\n    dependsOn: [Other]\n    properties:\n      id: Git.Git\n      source: winget\n",
        );
        assert_eq!(
            parse_configuration(&unknown),
            Err(ConfigurationFailure::InvalidDocument)
        );
        let duplicate = v3(
            "  - type: Microsoft.WinGet/Package\n    name: Git\n    properties:\n      id: Git.Git\n      source: winget\n  - type: Microsoft.WinGet/Package\n    name: Git2\n    properties:\n      id: Git.Git\n      source: winget\n",
        );
        assert_eq!(
            parse_configuration(&duplicate),
            Err(ConfigurationFailure::InvalidPackage)
        );
        let duplicate_name = v3(
            "  - type: Microsoft.WinGet/Package\n    name: Git\n    properties:\n      id: Git.Git\n      source: winget\n  - type: Microsoft.WinGet/Package\n    name: git\n    properties:\n      id: Microsoft.PowerShell\n      source: winget\n",
        );
        assert_eq!(
            parse_configuration(&duplicate_name),
            Err(ConfigurationFailure::DuplicateResource)
        );
        for marker in ["&anchor", "*anchor", "!unsafe", "<<: *base", "---"] {
            let input = format!("{}\n{marker}\n", v3(""));
            assert_eq!(
                parse_configuration(&input),
                Err(ConfigurationFailure::UnsafeYaml)
            );
        }
        let inline_document =
            format!("--- {{\"$schema\": \"{CONFIGURATION_SCHEMA}\", \"resources\": []}}\n");
        assert_eq!(
            parse_configuration(&inline_document),
            Err(ConfigurationFailure::UnsafeYaml)
        );
    }

    #[test]
    fn package_identifiers_and_versions_are_narrow_and_bounded() {
        for valid in [
            "Git.Git",
            "Microsoft.VisualStudioCode",
            "DBBrowserForSQLite.DBBrowserForSQLite",
            "Vendor_Name.Package-1",
        ] {
            assert!(valid_package_id(valid));
        }
        for invalid in [
            "Git",
            ".Git",
            "Git.",
            "Git Git.Package",
            "Git/Package.Name",
            "Git.Package;shutdown",
            "--help.Package",
            "Vendor.-Package",
            "Vendor.Package-",
        ] {
            assert!(!valid_package_id(invalid));
        }
        assert!(valid_version("1.2.3-preview+1"));
        assert!(!valid_version("1.2.3 --force"));
    }
}
