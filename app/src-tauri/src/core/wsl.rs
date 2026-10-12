#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decode_bom_short_and_partial_utf16() {
        assert_eq!(decode_output(&[255, 254, 65, 0, 66]), "A�");
        let text = "Wsl/Service/WSL_E_DISTRO_NOT_FOUND\r\n";
        let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode_output(&bytes), text);
        assert_eq!(
            error_code(&decode_output(&bytes)).as_deref(),
            Some("WSL_E_DISTRO_NOT_FOUND")
        );
        assert_eq!(decode_output(b"DEVBOX-BRIDGE/1\n"), "DEVBOX-BRIDGE/1\n");
    }
    #[test]
    fn forced_choice_must_be_existing_noninternal_wsl2() {
        let list = vec![
            Distro {
                name: "Ubuntu".into(),
                version: 2,
                default: true,
            },
            Distro {
                name: "docker-desktop".into(),
                version: 2,
                default: false,
            },
            Distro {
                name: "Old".into(),
                version: 1,
                default: false,
            },
        ];
        assert!(validate_choice(&list, "Gone").is_err());
        assert!(validate_choice(&list, "docker-desktop").is_err());
        assert!(validate_choice(&list, "Old").is_err());
        assert_eq!(validate_choice(&list, "Ubuntu").unwrap(), "Ubuntu");
        assert_eq!(pick(&list, Some("Gone")), None);
        assert_eq!(pick(&list[..2], None).as_deref(), Some("Ubuntu"));
    }
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct Distro {
    pub name: String,
    pub version: u32,
    pub default: bool,
}
pub fn is_internal(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["docker-desktop", "rancher-desktop", "podman-machine"]
        .iter()
        .any(|p| n.starts_with(p))
}
pub fn validate_choice(list: &[Distro], name: &str) -> Result<String, &'static str> {
    match list
        .iter()
        .find(|d| d.name == name && !is_internal(&d.name))
    {
        Some(d) if d.version == 2 => Ok(d.name.clone()),
        Some(_) => Err("WSL2만 지원합니다"),
        None => Err("사용 가능한 WSL 배포판이 아닙니다"),
    }
}
pub fn pick(list: &[Distro], saved: Option<&str>) -> Option<String> {
    let user: Vec<_> = list.iter().filter(|d| !is_internal(&d.name)).collect();
    if let Some(name) = saved {
        if user.iter().any(|d| d.name == name) {
            return Some(name.into());
        }
    }
    match user.as_slice() {
        [d] => Some(d.name.clone()),
        _ => None,
    }
}
pub fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[255, 254]);
    let utf16 = bom
        || (bytes.len() >= 4
            && bytes
                .iter()
                .skip(1)
                .step_by(2)
                .take(32)
                .filter(|b| **b == 0)
                .count()
                * 2
                >= bytes.len().min(64) / 2);
    if !utf16 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let b = if bom { &bytes[2..] } else { bytes };
    let units: Vec<_> = b
        .chunks_exact(2)
        .map(|v| u16::from_le_bytes([v[0], v[1]]))
        .collect();
    let mut text = String::from_utf16_lossy(&units);
    if b.len() % 2 != 0 {
        text.push('\u{fffd}');
    }
    text
}
pub fn error_code(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .find(|s| s.starts_with("WSL_E_"))
        .map(str::to_owned)
}
