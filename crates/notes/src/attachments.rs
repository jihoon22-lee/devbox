//! Image attachment policy, ported from v0.9.0 `knowledge-vault-engine/core/assets.rs`.
//! Untrusted bytes are accepted only as bounded static raster formats. v1 names are
//! `attachments/<sha256 first 12 hex>-<cleaned name>.<ext>`: the same bytes and
//! name always give the same path. No filesystem access here.

use sha2::{Digest, Sha256};
use std::fmt;

pub const ASSET_DIR: &str = "attachments";
pub const HASH_PREFIX: usize = 12;
pub const MAX_NAME_CHARS: usize = 64;
/// Stored attachments and rendered images share this size limit.
pub const MAX_ASSET_BYTES: usize = 2 * 1024 * 1024;
/// Keeps compressed images from implying huge decode buffers.
pub const MAX_IMAGE_DIMENSION: u32 = 16_384;
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;
pub const MAX_NOTE_PATH_BYTES: usize = 4 * 1024;
pub const HASH_HEX_LENGTH: usize = 64;
pub const MAX_HEADER_SCAN_BYTES: usize = 64 * 1024;

const IMAGE_ASSET_ERROR: &str = "image attachment refused";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl ImageFormat {
    pub const fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }
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

/// Checks size, magic and dimensions. The original name and any MIME hint are
/// not trusted: this result is the only source of the stored extension.
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

/// Keeps letters, digits, `-` and `_` of the last path component without its
/// extension; everything else becomes `-`. Never empty.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    let mut out = String::new();
    for c in stem.chars().take(MAX_NAME_CHARS) {
        let c = if c.is_alphanumeric() || c == '_' {
            c
        } else {
            '-'
        };
        if !(c == '-' && (out.is_empty() || out.ends_with('-'))) {
            out.push(c);
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "image".into()
    } else {
        out.into()
    }
}

pub fn asset_relative_path(
    hash: &str,
    name: &str,
    format: ImageFormat,
) -> Result<String, AssetError> {
    if hash.len() != HASH_HEX_LENGTH || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AssetError::InvalidAssetPath);
    }
    Ok(format!(
        "{ASSET_DIR}/{}-{}.{}",
        &hash[..HASH_PREFIX],
        clean_name(name),
        format.extension()
    ))
}

/// POSIX Markdown destination from the note to a vault-relative attachment.
/// Generated names contain no spaces, parentheses or control characters.
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
    let Some(name) = asset_rel.strip_prefix("attachments/") else {
        return Err(AssetError::InvalidAssetPath);
    };
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return Err(AssetError::InvalidAssetPath);
    };
    let Some((hash, rest)) = stem.split_once('-') else {
        return Err(AssetError::InvalidAssetPath);
    };
    if hash.len() != HASH_PREFIX
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || rest.is_empty()
        || clean_name(rest) != rest
        || !matches!(extension, "png" | "jpg" | "gif" | "webp")
    {
        return Err(AssetError::InvalidAssetPath);
    }
    Ok(())
}

pub fn detect_format(bytes: &[u8]) -> Option<ImageFormat> {
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
        let asset = asset_relative_path(&hash, "../../My photo!.PNG", ImageFormat::Png)
            .expect("fixture result");
        assert_eq!(asset, format!("attachments/{}-My-photo.png", &hash[..12]));
        assert_eq!(
            asset_relative_path(&hash, "../../My photo!.PNG", ImageFormat::Png)
                .expect("fixture result"),
            asset
        );
        assert_eq!(
            markdown_link("note.md", &asset).expect("fixture result"),
            format!("![image]({asset})")
        );
        assert_eq!(
            markdown_link("Notes/deep/note.md", &asset).expect("fixture result"),
            format!("![image](../../{asset})")
        );
    }

    #[test]
    fn rejects_unsafe_note_and_asset_paths_without_echoing_them() {
        let secret = "../private/token.md";
        let error =
            markdown_link(secret, "attachments/not-a-hash.png").expect_err("must be rejected");
        assert_eq!(error.to_string(), IMAGE_ASSET_ERROR);
        assert!(!error.to_string().contains(secret));
        assert!(validate_note_path("Notes\\note.md").is_err());
        assert!(validate_note_path("C:/outside.md").is_err());
        assert!(markdown_destination("note.md", "../secret.png").is_err());
        assert!(markdown_destination("note.md", "attachments/abc-x.png").is_err());
        assert!(markdown_destination("note.md", "attachments/0123456789ab-../x.png").is_err());
    }
}
