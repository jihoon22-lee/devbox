//! 순수 이미지 자산 정책.
//!
//! 파일시스템과 Tauri IPC는 command 레이어가 소유한다. 이 모듈은 untrusted
//! bytes를 bounded static raster 포맷으로 판정하고, content hash 이름과
//! 현재 Markdown 문서에서 보이는 상대 링크를 결정론적으로 만든다.

use sha2::{Digest, Sha256};
use std::fmt;

pub const ASSET_DIR: &str = "assets";
/// 저장 이미지와 Markdown preview가 공유하는 파일 크기 상한.
pub const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;
/// 압축 이미지가 비정상적으로 큰 디코딩 버퍼를 만들지 않도록 하는 상한.
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_NOTE_PATH_BYTES: usize = 4 * 1024;
pub const HASH_HEX_LENGTH: usize = 64;
pub const MAX_HEADER_SCAN_BYTES: usize = 64 * 1024;

const IMAGE_ASSET_ERROR: &str = "이미지 자산을 저장할 수 없습니다";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetError {
    InvalidNotePath,
    EmptyInput,
    TooLarge,
    UnsupportedFormat,
    InvalidDimensions,
    InvalidAssetPath,
    Storage,
}

impl AssetError {
    pub const fn message(self) -> &'static str {
        IMAGE_ASSET_ERROR
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for AssetError {}

/// 입력 bytes를 먼저 크기와 magic/dimension으로 검증한다. MIME hint나 원래
/// filename은 신뢰하지 않으므로 이 함수의 결과가 저장 extension의 유일한
/// 출처다.
pub fn inspect(bytes: &[u8]) -> Result<ImageFormat, AssetError> {
    if bytes.is_empty() {
        return Err(AssetError::EmptyInput);
    }
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(AssetError::TooLarge);
    }

    let format = detect_format(bytes).ok_or(AssetError::UnsupportedFormat)?;
    let (width, height) = dimensions(format, bytes).ok_or(AssetError::InvalidDimensions)?;
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || u64::from(width).saturating_mul(u64::from(height)) > MAX_IMAGE_PIXELS
    {
        return Err(AssetError::InvalidDimensions);
    }
    Ok(format)
}

pub fn content_hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(HASH_HEX_LENGTH);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub fn asset_relative_path(hash: &str, format: ImageFormat) -> Result<String, AssetError> {
    if hash.len() != HASH_HEX_LENGTH || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AssetError::InvalidAssetPath);
    }
    Ok(format!("{ASSET_DIR}/{hash}.{}", format.extension()))
}

/// 현재 note 경로에서 root-relative asset path를 가리키는 POSIX Markdown
/// destination을 만든다. 생성되는 이름은 공백·괄호·제어문자·사용자 입력을
/// 포함하지 않는다.
pub fn markdown_destination(note_rel: &str, asset_rel: &str) -> Result<String, AssetError> {
    validate_note_path(note_rel)?;
    validate_asset_path(asset_rel)?;

    let depth = note_rel.split('/').count().saturating_sub(1);
    let prefix = "../".repeat(depth);
    Ok(format!("{prefix}{asset_rel}"))
}

pub fn markdown_link(note_rel: &str, asset_rel: &str) -> Result<String, AssetError> {
    Ok(format!(
        "![image]({})",
        markdown_destination(note_rel, asset_rel)?
    ))
}

pub fn validate_note_path(note_rel: &str) -> Result<(), AssetError> {
    if note_rel.is_empty()
        || note_rel.len() > MAX_NOTE_PATH_BYTES
        || !note_rel.ends_with(".md")
        || note_rel.contains(['\\', '\0'])
        || note_rel.chars().any(char::is_control)
    {
        return Err(AssetError::InvalidNotePath);
    }
    let mut segments = note_rel.split('/');
    if segments.any(|segment| {
        segment.is_empty() || segment == "." || segment == ".." || segment.contains(':')
    }) {
        return Err(AssetError::InvalidNotePath);
    }
    Ok(())
}

fn validate_asset_path(asset_rel: &str) -> Result<(), AssetError> {
    let Some(name) = asset_rel.strip_prefix("assets/") else {
        return Err(AssetError::InvalidAssetPath);
    };
    let mut parts = name.split('.');
    let Some(hash) = parts.next() else {
        return Err(AssetError::InvalidAssetPath);
    };
    let Some(extension) = parts.next() else {
        return Err(AssetError::InvalidAssetPath);
    };
    if parts.next().is_some()
        || hash.len() != HASH_HEX_LENGTH
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !matches!(extension, "png" | "jpg" | "gif" | "webp")
    {
        return Err(AssetError::InvalidAssetPath);
    }
    Ok(())
}

fn detect_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

fn dimensions(format: ImageFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    match format {
        ImageFormat::Png => png_dimensions(bytes),
        ImageFormat::Jpeg => jpeg_dimensions(bytes),
        ImageFormat::Gif => gif_dimensions(bytes),
        ImageFormat::Webp => webp_dimensions(bytes),
    }
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(bytes[16..20].try_into().ok()?),
        u32::from_be_bytes(bytes[20..24].try_into().ok()?),
    ))
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }
    Some((
        u32::from(u16::from_le_bytes(bytes[6..8].try_into().ok()?)),
        u32::from(u16::from_le_bytes(bytes[8..10].try_into().ok()?)),
    ))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let limit = bytes.len().min(MAX_HEADER_SCAN_BYTES);
    let mut cursor = 2;
    while cursor + 1 < limit {
        if bytes[cursor] != 0xff {
            cursor += 1;
            continue;
        }
        while cursor < limit && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= limit {
            break;
        }
        let marker = bytes[cursor];
        cursor += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x00 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if cursor + 2 > limit {
            break;
        }
        let segment_len = usize::from(u16::from_be_bytes(
            bytes[cursor..cursor + 2].try_into().ok()?,
        ));
        if segment_len < 2 || cursor.checked_add(segment_len)? > limit {
            break;
        }
        if is_jpeg_sof(marker) {
            if segment_len < 7 {
                return None;
            }
            let height = u32::from(u16::from_be_bytes(
                bytes[cursor + 3..cursor + 5].try_into().ok()?,
            ));
            let width = u32::from(u16::from_be_bytes(
                bytes[cursor + 5..cursor + 7].try_into().ok()?,
            ));
            return Some((width, height));
        }
        cursor += segment_len;
    }
    None
}

fn is_jpeg_sof(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 20 {
        return None;
    }
    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            let width_minus_one =
                u32::from(bytes[24]) | (u32::from(bytes[25]) << 8) | (u32::from(bytes[26]) << 16);
            let height_minus_one =
                u32::from(bytes[27]) | (u32::from(bytes[28]) << 8) | (u32::from(bytes[29]) << 16);
            let width = width_minus_one.saturating_add(1);
            let height = height_minus_one.saturating_add(1);
            Some((width, height))
        }
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => Some((
            u32::from(u16::from_le_bytes(bytes[26..28].try_into().ok()?)) & 0x3fff,
            u32::from(u16::from_le_bytes(bytes[28..30].try_into().ok()?)) & 0x3fff,
        )),
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            let bits = u32::from_le_bytes(bytes[21..25].try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8];
        bytes.extend(height.to_be_bytes());
        bytes.extend(width.to_be_bytes());
        bytes.extend([1, 1, 0x11, 0, 2, 0x11, 0]);
        bytes
    }

    fn gif(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend(width.to_le_bytes());
        bytes.extend(height.to_le_bytes());
        bytes
    }

    fn webp_vp8x(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        let width_minus_one = width - 1;
        let height_minus_one = height - 1;
        bytes[24..27].copy_from_slice(&[
            width_minus_one as u8,
            (width_minus_one >> 8) as u8,
            (width_minus_one >> 16) as u8,
        ]);
        bytes[27..30].copy_from_slice(&[
            height_minus_one as u8,
            (height_minus_one >> 8) as u8,
            (height_minus_one >> 16) as u8,
        ]);
        bytes
    }

    #[test]
    fn recognizes_supported_formats_and_dimensions() {
        assert_eq!(inspect(&png(640, 480)), Ok(ImageFormat::Png));
        assert_eq!(inspect(&jpeg(640, 480)), Ok(ImageFormat::Jpeg));
        assert_eq!(inspect(&gif(640, 480)), Ok(ImageFormat::Gif));
        assert_eq!(inspect(&webp_vp8x(640, 480)), Ok(ImageFormat::Webp));
    }

    #[test]
    fn rejects_empty_oversized_unsupported_and_malformed_inputs() {
        assert_eq!(inspect(&[]), Err(AssetError::EmptyInput));
        assert_eq!(
            inspect(&vec![0; MAX_ASSET_BYTES + 1]),
            Err(AssetError::TooLarge)
        );
        assert_eq!(inspect(b"<svg></svg>"), Err(AssetError::UnsupportedFormat));
        assert_eq!(
            inspect(&png(MAX_IMAGE_DIMENSION + 1, 1)),
            Err(AssetError::InvalidDimensions)
        );
        assert_eq!(
            inspect(&png(1, 1)[..20]),
            Err(AssetError::InvalidDimensions)
        );
    }

    #[test]
    fn rejects_jpeg_without_header_dimensions_and_webp_bad_dimensions() {
        assert_eq!(
            inspect(&[0xff, 0xd8, 0xff, 0xda]),
            Err(AssetError::InvalidDimensions)
        );
        assert_eq!(
            inspect(&webp_vp8x(MAX_IMAGE_DIMENSION + 1, 1)),
            Err(AssetError::InvalidDimensions)
        );
    }

    #[test]
    fn creates_deterministic_hash_name_and_note_relative_links() {
        let bytes = png(1, 1);
        let hash = content_hash_hex(&bytes);
        assert_eq!(hash.len(), HASH_HEX_LENGTH);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let asset = asset_relative_path(&hash, ImageFormat::Png).unwrap();
        assert_eq!(
            markdown_link("note.md", &asset).unwrap(),
            format!("![image]({asset})")
        );
        assert_eq!(
            markdown_link("Notes/deep/note.md", &asset).unwrap(),
            format!("![image](../../{asset})")
        );
    }

    #[test]
    fn rejects_unsafe_note_and_asset_paths_without_echoing_them() {
        let secret = "../private/token.md";
        let error = markdown_link(secret, "assets/not-a-hash.png").unwrap_err();
        assert_eq!(error.to_string(), IMAGE_ASSET_ERROR);
        assert!(!error.to_string().contains(secret));
        assert!(validate_note_path("Notes\\note.md").is_err());
        assert!(validate_note_path("C:/outside.md").is_err());
        assert!(markdown_destination("note.md", "../secret.png").is_err());
        assert!(markdown_destination("note.md", "assets/abc.png").is_err());
    }
}
