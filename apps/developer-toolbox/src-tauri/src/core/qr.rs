//! Bounded, deterministic QR generation for Developer Toolbox.
//!
//! The `qrcode` crate owns the ISO/IEC 18004 encoding algorithm. This module
//! owns the application boundary: preset validation, byte/size limits, a
//! deterministic SVG renderer, and a small grayscale PNG encoder. Payloads
//! never enter logs or error strings.

use base64::Engine;
use qrcode::bits::Bits;
use qrcode::{Color, EcLevel, QrCode, Version};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

pub const MAX_PAYLOAD_BYTES: usize = 4_096;
pub const MAX_WIFI_SSID_BYTES: usize = 32;
pub const MAX_WIFI_PASSWORD_BYTES: usize = 63;
pub const MIN_OUTPUT_SIZE: u32 = 64;
pub const MAX_OUTPUT_SIZE: u32 = 2_048;
pub const MIN_QUIET_ZONE: u8 = 4;
pub const MAX_QUIET_ZONE: u8 = 16;
const MAX_VERSION: u8 = 40;
const MAX_MODULE_SCALE: u32 = 64;
const MAX_BINARY_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_BINARY_OUTPUT_BASE64_LENGTH: usize = MAX_BINARY_OUTPUT_BYTES;
const MAX_SVG_OUTPUT_BYTES: usize = 4 * 1024 * 1024;

const EMPTY_INPUT_ERROR: &str = "QR 입력은 비어 있을 수 없습니다.";
const INPUT_TOO_LONG_ERROR: &str = "QR 입력이 너무 깁니다.";
const INVALID_INPUT_ERROR: &str = "QR 입력 형식이 올바르지 않습니다.";
const INVALID_WIFI_ERROR: &str = "Wi-Fi 설정이 올바르지 않습니다.";
const INVALID_VERSION_ERROR: &str = "QR 버전이 올바르지 않습니다.";
const INVALID_EC_ERROR: &str = "QR 오류 보정 수준이 올바르지 않습니다.";
const INVALID_SIZE_ERROR: &str = "QR 크기가 올바르지 않습니다.";
const INVALID_QUIET_ZONE_ERROR: &str = "QR 여백이 올바르지 않습니다.";
const SMALL_SIZE_ERROR: &str = "QR 크기가 버전과 여백에 비해 작습니다.";
const CAPACITY_ERROR: &str = "QR 용량을 초과했습니다. 버전 또는 오류 보정 수준을 조정하세요.";
const RENDER_ERROR: &str = "QR 이미지를 생성하지 못했습니다.";

/// UI request shared by native command and deterministic Rust fixtures.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerateQrRequest {
    pub preset: String,
    pub text: Option<String>,
    pub url: Option<String>,
    pub wifi: Option<WifiRequest>,
    /// `None` selects the smallest normal QR version that fits.
    pub version: Option<u8>,
    pub error_correction: String,
    /// Maximum requested edge length in pixels. The actual edge is module-aligned.
    pub size: u32,
    pub quiet_zone: u8,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WifiRequest {
    pub ssid: String,
    pub password: String,
    pub security: String,
    pub hidden: bool,
}

/// Result returned to the frontend. The payload is intentionally absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrResult {
    pub svg: String,
    pub png_base64: String,
    pub width: u32,
    pub version: u8,
    pub modules: u16,
    pub quiet_zone: u8,
    pub payload_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Text,
    Url,
    Wifi,
}

impl Preset {
    fn parse(value: &str) -> Result<Self, &'static str> {
        match value {
            "text" => Ok(Self::Text),
            "url" => Ok(Self::Url),
            "wifi" => Ok(Self::Wifi),
            _ => Err(INVALID_INPUT_ERROR),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildError {
    EmptyInput,
    InputTooLong,
    InvalidInput,
    InvalidWifi,
    InvalidVersion,
    InvalidEc,
    InvalidSize,
    InvalidQuietZone,
    SmallSize,
    Capacity,
    Render,
}

impl BuildError {
    const fn message(self) -> &'static str {
        match self {
            Self::EmptyInput => EMPTY_INPUT_ERROR,
            Self::InputTooLong => INPUT_TOO_LONG_ERROR,
            Self::InvalidInput => INVALID_INPUT_ERROR,
            Self::InvalidWifi => INVALID_WIFI_ERROR,
            Self::InvalidVersion => INVALID_VERSION_ERROR,
            Self::InvalidEc => INVALID_EC_ERROR,
            Self::InvalidSize => INVALID_SIZE_ERROR,
            Self::InvalidQuietZone => INVALID_QUIET_ZONE_ERROR,
            Self::SmallSize => SMALL_SIZE_ERROR,
            Self::Capacity => CAPACITY_ERROR,
            Self::Render => RENDER_ERROR,
        }
    }
}

/// Generate both deterministic export formats without persisting or logging the payload.
pub fn generate(request: GenerateQrRequest) -> Result<QrResult, String> {
    generate_inner(request).map_err(|error| error.message().to_string())
}

fn generate_inner(request: GenerateQrRequest) -> Result<QrResult, BuildError> {
    let preset = Preset::parse(&request.preset).map_err(|_| BuildError::InvalidInput)?;
    validate_options(&request)?;
    let payload = build_payload(preset, &request)?;
    if payload.is_empty() {
        return Err(BuildError::EmptyInput);
    }
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(BuildError::InputTooLong);
    }

    let ec_level = parse_ec_level(&request.error_correction)?;
    let code = build_code(&payload, request.version, ec_level)?;
    let modules = u32::try_from(code.width()).map_err(|_| BuildError::Render)?;
    let quiet_zone = u32::from(request.quiet_zone);
    let total_modules = modules
        .checked_add(quiet_zone.checked_mul(2).ok_or(BuildError::Render)?)
        .ok_or(BuildError::Render)?;
    let scale = request.size / total_modules;
    if scale == 0 {
        return Err(BuildError::SmallSize);
    }
    let scale = scale.min(MAX_MODULE_SCALE);
    let width = total_modules.checked_mul(scale).ok_or(BuildError::Render)?;

    let svg = render_svg(&code, scale, quiet_zone, width)?;
    let png = render_png(&code, scale, quiet_zone, width)?;
    let png_base64 = base64::engine::general_purpose::STANDARD.encode(png);
    if png_base64.len() > MAX_BINARY_OUTPUT_BASE64_LENGTH {
        return Err(BuildError::Render);
    }

    Ok(QrResult {
        svg,
        png_base64,
        width,
        version: match code.version() {
            Version::Normal(version) => u8::try_from(version).map_err(|_| BuildError::Render)?,
            Version::Micro(_) => return Err(BuildError::Render),
        },
        modules: u16::try_from(modules).map_err(|_| BuildError::Render)?,
        quiet_zone: request.quiet_zone,
        payload_bytes: payload.len(),
    })
}

fn validate_options(request: &GenerateQrRequest) -> Result<(), BuildError> {
    if let Some(version) = request.version {
        if !(1..=MAX_VERSION).contains(&version) {
            return Err(BuildError::InvalidVersion);
        }
    }
    if !(MIN_OUTPUT_SIZE..=MAX_OUTPUT_SIZE).contains(&request.size) {
        return Err(BuildError::InvalidSize);
    }
    if !(MIN_QUIET_ZONE..=MAX_QUIET_ZONE).contains(&request.quiet_zone) {
        return Err(BuildError::InvalidQuietZone);
    }
    Ok(())
}

fn parse_ec_level(value: &str) -> Result<EcLevel, BuildError> {
    match value {
        "L" => Ok(EcLevel::L),
        "M" => Ok(EcLevel::M),
        "Q" => Ok(EcLevel::Q),
        "H" => Ok(EcLevel::H),
        _ => Err(BuildError::InvalidEc),
    }
}

fn build_payload(preset: Preset, request: &GenerateQrRequest) -> Result<Vec<u8>, BuildError> {
    match preset {
        Preset::Text => bounded_text(request.text.as_deref().ok_or(BuildError::EmptyInput)?),
        Preset::Url => bounded_url(request.url.as_deref().ok_or(BuildError::EmptyInput)?),
        Preset::Wifi => build_wifi(request.wifi.as_ref().ok_or(BuildError::InvalidWifi)?),
    }
}

fn bounded_text(value: &str) -> Result<Vec<u8>, BuildError> {
    if value.is_empty() {
        return Err(BuildError::EmptyInput);
    }
    if value.len() > MAX_PAYLOAD_BYTES {
        return Err(BuildError::InputTooLong);
    }
    Ok(value.as_bytes().to_vec())
}

fn bounded_url(value: &str) -> Result<Vec<u8>, BuildError> {
    if value.is_empty() {
        return Err(BuildError::EmptyInput);
    }
    if value.len() > MAX_PAYLOAD_BYTES {
        return Err(BuildError::InputTooLong);
    }
    let Some((scheme, rest)) = value.split_once("://") else {
        return Err(BuildError::InvalidInput);
    };
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || value.chars().any(|character| {
            character.is_whitespace() || character.is_control() || character == '\u{feff}'
        })
    {
        return Err(BuildError::InvalidInput);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return Err(BuildError::InvalidInput);
    }
    Ok(value.as_bytes().to_vec())
}

fn build_wifi(request: &WifiRequest) -> Result<Vec<u8>, BuildError> {
    if request.ssid.is_empty() || request.ssid.len() > MAX_WIFI_SSID_BYTES {
        return Err(BuildError::InvalidWifi);
    }
    if request.password.len() > MAX_WIFI_PASSWORD_BYTES {
        return Err(BuildError::InvalidWifi);
    }
    let security = match request.security.as_str() {
        "WPA" | "WEP" | "nopass" => request.security.as_str(),
        _ => return Err(BuildError::InvalidWifi),
    };
    if security == "nopass" && !request.password.is_empty()
        || security != "nopass" && request.password.is_empty()
    {
        return Err(BuildError::InvalidWifi);
    }

    let mut payload =
        String::with_capacity(12 + request.ssid.len() * 2 + request.password.len() * 2);
    payload.push_str("WIFI:T:");
    payload.push_str(security);
    payload.push_str(";S:");
    push_wifi_escaped(&mut payload, &request.ssid);
    payload.push_str(";P:");
    push_wifi_escaped(&mut payload, &request.password);
    if request.hidden {
        payload.push_str(";H:true");
    }
    payload.push_str(";;");
    bounded_text(&payload)
}

fn push_wifi_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\\' | ';' | ',' | ':' => {
                output.push('\\');
                output.push(character);
            }
            _ => output.push(character),
        }
    }
}

fn build_code(
    payload: &[u8],
    version: Option<u8>,
    ec_level: EcLevel,
) -> Result<QrCode, BuildError> {
    if let Some(version) = version {
        return encode_at_version(payload, version, ec_level);
    }

    for version in 1..=MAX_VERSION {
        match encode_at_version(payload, version, ec_level) {
            Ok(code) => return Ok(code),
            Err(BuildError::Capacity) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(BuildError::Capacity)
}

fn encode_at_version(payload: &[u8], version: u8, ec_level: EcLevel) -> Result<QrCode, BuildError> {
    let mut bits = Bits::new(Version::Normal(i16::from(version)));
    bits.push_byte_data(payload).map_err(map_qr_error)?;
    bits.push_terminator(ec_level).map_err(map_qr_error)?;
    QrCode::with_bits(bits, ec_level).map_err(map_qr_error)
}

fn map_qr_error(error: qrcode::types::QrError) -> BuildError {
    match error {
        qrcode::types::QrError::DataTooLong => BuildError::Capacity,
        qrcode::types::QrError::InvalidVersion
        | qrcode::types::QrError::UnsupportedCharacterSet
        | qrcode::types::QrError::InvalidEciDesignator
        | qrcode::types::QrError::InvalidCharacter => BuildError::Render,
    }
}

fn render_svg(
    code: &QrCode,
    scale: u32,
    quiet_zone: u32,
    width: u32,
) -> Result<String, BuildError> {
    let modules = code.width();
    let mut svg = String::with_capacity((width as usize).saturating_mul(8));
    svg.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="{width}" height="{width}" viewBox="0 0 {width} {width}" shape-rendering="crispEdges"><rect width="{width}" height="{width}" fill="#fff"/><path fill="#000" d=""##
    );

    for y in 0..modules {
        let mut x = 0;
        while x < modules {
            if code[(x, y)] != Color::Dark {
                x += 1;
                continue;
            }
            let start = x;
            while x < modules && code[(x, y)] == Color::Dark {
                x += 1;
            }
            let left = (u32::try_from(start).map_err(|_| BuildError::Render)? + quiet_zone)
                .checked_mul(scale)
                .ok_or(BuildError::Render)?;
            let top = (u32::try_from(y).map_err(|_| BuildError::Render)? + quiet_zone)
                .checked_mul(scale)
                .ok_or(BuildError::Render)?;
            let run = u32::try_from(x - start)
                .map_err(|_| BuildError::Render)?
                .checked_mul(scale)
                .ok_or(BuildError::Render)?;
            let _ = write!(svg, "M{left} {top}h{run}v{scale}H{left}V{top}");
        }
    }
    svg.push_str(r#""/></svg>"#);
    if svg.len() > MAX_SVG_OUTPUT_BYTES {
        return Err(BuildError::Render);
    }
    Ok(svg)
}

fn render_png(
    code: &QrCode,
    scale: u32,
    quiet_zone: u32,
    width: u32,
) -> Result<Vec<u8>, BuildError> {
    let width_usize = usize::try_from(width).map_err(|_| BuildError::Render)?;
    let pixels = width_usize
        .checked_mul(width_usize)
        .ok_or(BuildError::Render)?;
    if pixels > MAX_BINARY_OUTPUT_BYTES {
        return Err(BuildError::Render);
    }
    let modules = code.width();
    let mut rows = Vec::with_capacity(pixels);
    for y in 0..width {
        let module_y = y / scale;
        for x in 0..width {
            let module_x = x / scale;
            let dark = module_x >= quiet_zone
                && module_x
                    < quiet_zone + u32::try_from(modules).map_err(|_| BuildError::Render)?
                && module_y >= quiet_zone
                && module_y
                    < quiet_zone + u32::try_from(modules).map_err(|_| BuildError::Render)?
                && code[(
                    usize::try_from(module_x - quiet_zone).map_err(|_| BuildError::Render)?,
                    usize::try_from(module_y - quiet_zone).map_err(|_| BuildError::Render)?,
                )] == Color::Dark;
            rows.push(if dark { 0 } else { 255 });
        }
    }

    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, width);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_filter(png::Filter::NoFilter);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|_| BuildError::Render)?;
        writer
            .write_image_data(&rows)
            .map_err(|_| BuildError::Render)?;
        writer.finish().map_err(|_| BuildError::Render)?;
    }
    if output.len() > MAX_BINARY_OUTPUT_BYTES {
        return Err(BuildError::Render);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(preset: &str) -> GenerateQrRequest {
        GenerateQrRequest {
            preset: preset.to_string(),
            text: Some("https://example.com/devbox".to_string()),
            url: None,
            wifi: None,
            version: Some(3),
            error_correction: "M".to_string(),
            size: 256,
            quiet_zone: 4,
        }
    }

    #[test]
    fn serde_contract_rejects_unknown_request_and_wifi_fields() {
        let unknown_request = json!({
            "preset": "text",
            "text": "safe",
            "url": null,
            "wifi": null,
            "version": null,
            "errorCorrection": "M",
            "size": 256,
            "quietZone": 4,
            "unexpected": "ignored-by-a-lenient-boundary"
        });
        assert!(serde_json::from_value::<GenerateQrRequest>(unknown_request).is_err());

        let unknown_wifi = json!({
            "ssid": "devbox",
            "password": "secret",
            "security": "WPA",
            "hidden": false,
            "unexpected": true
        });
        assert!(serde_json::from_value::<WifiRequest>(unknown_wifi).is_err());
    }

    #[test]
    fn text_generation_is_deterministic_and_payload_is_not_returned() {
        let first = generate(request("text")).unwrap();
        let second = generate(request("text")).unwrap();
        assert_eq!(first.svg, second.svg);
        assert_eq!(first.png_base64, second.png_base64);
        assert_eq!(first.width, 222);
        assert_eq!(first.version, 3);
        assert!(!first.svg.contains("example.com"));
        let png = base64::engine::general_purpose::STANDARD
            .decode(first.png_base64)
            .unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert!(png.len() <= MAX_BINARY_OUTPUT_BYTES);
    }

    #[test]
    fn url_requires_http_or_https_without_whitespace() {
        let mut invalid = request("url");
        invalid.text = None;
        invalid.url = Some("file:///tmp/secret".to_string());
        assert_eq!(generate(invalid), Err(INVALID_INPUT_ERROR.to_string()));

        let mut valid = request("url");
        valid.text = None;
        valid.url = Some("https://example.com/a%20b".to_string());
        assert!(generate(valid).is_ok());

        let mut uppercase = request("url");
        uppercase.text = None;
        uppercase.url = Some("HTTPS://example.com/path".to_string());
        assert!(generate(uppercase).is_ok());

        let mut unicode_whitespace = request("url");
        unicode_whitespace.text = None;
        unicode_whitespace.url = Some("https://example.com/\u{00a0}path".to_string());
        assert_eq!(
            generate(unicode_whitespace),
            Err(INVALID_INPUT_ERROR.to_string())
        );
    }

    #[test]
    fn wifi_preset_escapes_reserved_delimiters() {
        let mut value = request("wifi");
        value.text = None;
        value.wifi = Some(WifiRequest {
            ssid: "dev;box".to_string(),
            password: r"p\;,:".to_string(),
            security: "WPA".to_string(),
            hidden: true,
        });
        let payload = build_wifi(value.wifi.as_ref().unwrap()).unwrap();
        assert_eq!(
            String::from_utf8(payload).unwrap(),
            r"WIFI:T:WPA;S:dev\;box;P:p\\\;\,\:;H:true;;"
        );
        assert!(generate(value).is_ok());
    }

    #[test]
    fn capacity_and_bounds_fail_closed_without_raw_input() {
        let mut oversized = request("text");
        oversized.text = Some("x".repeat(MAX_PAYLOAD_BYTES + 1));
        let error = generate(oversized).unwrap_err();
        assert_eq!(error, INPUT_TOO_LONG_ERROR);
        assert!(!error.contains('x'));

        let mut invalid_version = request("text");
        invalid_version.version = Some(41);
        assert_eq!(
            generate(invalid_version),
            Err(INVALID_VERSION_ERROR.to_string())
        );

        let mut too_small = request("text");
        too_small.size = 64;
        too_small.version = Some(40);
        assert_eq!(generate(too_small), Err(SMALL_SIZE_ERROR.to_string()));

        let mut invalid_quiet_zone = request("text");
        invalid_quiet_zone.quiet_zone = MAX_QUIET_ZONE + 1;
        assert_eq!(
            generate(invalid_quiet_zone),
            Err(INVALID_QUIET_ZONE_ERROR.to_string())
        );
    }

    #[test]
    fn explicit_high_error_correction_rejects_version_capacity() {
        let mut value = request("text");
        value.version = Some(1);
        value.error_correction = "H".to_string();
        value.text = Some("x".repeat(300));
        assert_eq!(generate(value), Err(CAPACITY_ERROR.to_string()));
    }

    #[test]
    fn all_error_correction_levels_are_allowlisted() {
        for level in ["L", "M", "Q", "H"] {
            let mut value = request("text");
            value.version = None;
            value.error_correction = level.to_string();
            assert!(generate(value).is_ok(), "{level}");
        }
        let mut invalid = request("text");
        invalid.error_correction = "X".to_string();
        assert_eq!(generate(invalid), Err(INVALID_EC_ERROR.to_string()));
    }
}
