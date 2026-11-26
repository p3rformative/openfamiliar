//! Header-only validation for a Codex-sized pack sheet.
//!
//! This checks that a file is WebP, has the required canvas size, and is within
//! the size cap. It does not map frames, crop, or import animation data.

use std::fs;
use std::path::Path;
use thiserror::Error;

pub const CODEX_SHEET_WIDTH: u32 = 1536;
pub const CODEX_SHEET_HEIGHT: u32 = 1872;
pub const CODEX_SHEET_MAX_BYTES: u64 = 20 * 1024 * 1024;

const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const JPEG_SOI: &[u8] = &[0xFF, 0xD8, 0xFF];
const GIF87A: &[u8] = b"GIF87a";
const GIF89A: &[u8] = b"GIF89a";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSheetFormat {
    Webp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexSheetInfo {
    pub format: CodexSheetFormat,
    pub width: u32,
    pub height: u32,
    pub len: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CodexSheetError {
    #[error("image file is unreadable: {0}")]
    Unreadable(String),
    #[error("unsupported image format (only WebP is accepted)")]
    UnsupportedFormat,
    #[error("image is {width}x{height} pixels; expected {CODEX_SHEET_WIDTH}x{CODEX_SHEET_HEIGHT}")]
    WrongDimensions { width: u32, height: u32 },
    #[error("file is {len} bytes; maximum is {CODEX_SHEET_MAX_BYTES} bytes (20 MiB)")]
    TooLarge { len: u64 },
}

pub fn validate_codex_sheet_path(path: &Path) -> Result<CodexSheetInfo, CodexSheetError> {
    let meta = fs::metadata(path).map_err(|err| {
        CodexSheetError::Unreadable(format!("could not read file metadata ({err})"))
    })?;
    if !meta.is_file() {
        return Err(CodexSheetError::Unreadable(
            "not a regular file".to_string(),
        ));
    }
    let len = meta.len();
    if len > CODEX_SHEET_MAX_BYTES {
        return Err(CodexSheetError::TooLarge { len });
    }
    let bytes = fs::read(path)
        .map_err(|err| CodexSheetError::Unreadable(format!("could not read file ({err})")))?;
    validate_codex_sheet(&bytes)
}

pub fn validate_codex_sheet(bytes: &[u8]) -> Result<CodexSheetInfo, CodexSheetError> {
    let len = bytes.len() as u64;
    if len > CODEX_SHEET_MAX_BYTES {
        return Err(CodexSheetError::TooLarge { len });
    }
    if bytes.is_empty() {
        return Err(CodexSheetError::Unreadable("file is empty".to_string()));
    }

    let (width, height) = match detect_format(bytes) {
        Detected::Webp => webp_dimensions(bytes)?,
        Detected::Unsupported => return Err(CodexSheetError::UnsupportedFormat),
        Detected::Unreadable => {
            return Err(CodexSheetError::Unreadable(
                "truncated or damaged header".to_string(),
            ))
        }
    };
    let format = CodexSheetFormat::Webp;

    if width != CODEX_SHEET_WIDTH || height != CODEX_SHEET_HEIGHT {
        return Err(CodexSheetError::WrongDimensions { width, height });
    }

    Ok(CodexSheetInfo {
        format,
        width,
        height,
        len,
    })
}

enum Detected {
    Webp,
    Unsupported,
    Unreadable,
}

fn detect_format(bytes: &[u8]) -> Detected {
    if is_webp_magic(bytes) {
        return Detected::Webp;
    }
    if bytes.starts_with(PNG_SIGNATURE)
        || bytes.starts_with(JPEG_SOI)
        || bytes.starts_with(GIF87A)
        || bytes.starts_with(GIF89A)
    {
        return Detected::Unsupported;
    }
    // A prefix too short to hold a complete WebP magic is treated as
    // unreadable rather than an unknown format.
    if bytes.len() < 12 {
        return Detected::Unreadable;
    }
    Detected::Unsupported
}

fn is_webp_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
}

fn webp_dimensions(bytes: &[u8]) -> Result<(u32, u32), CodexSheetError> {
    if bytes.len() < 12 {
        return Err(CodexSheetError::Unreadable(
            "truncated WebP header".to_string(),
        ));
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let fourcc = &bytes[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_start = offset + 8;
        let Some(data_end) = data_start.checked_add(chunk_size) else {
            return Err(CodexSheetError::Unreadable(
                "truncated WebP chunk".to_string(),
            ));
        };
        if data_end > bytes.len() {
            return Err(CodexSheetError::Unreadable(
                "truncated WebP chunk".to_string(),
            ));
        }
        let payload = &bytes[data_start..data_end];
        let dims = match fourcc {
            b"VP8X" => Some(vp8x_dimensions(payload)?),
            b"VP8L" => Some(vp8l_dimensions(payload)?),
            b"VP8 " => Some(vp8_dimensions(payload)?),
            _ => None,
        };
        if let Some((width, height)) = dims {
            return Ok((width, height));
        }
        // Chunks are padded to even length.
        let padded = chunk_size + (chunk_size & 1);
        offset = match data_start.checked_add(padded) {
            Some(next) => next,
            None => {
                return Err(CodexSheetError::Unreadable(
                    "truncated WebP chunk".to_string(),
                ))
            }
        };
    }
    Err(CodexSheetError::Unreadable(
        "WebP header has no canvas size".to_string(),
    ))
}

fn vp8x_dimensions(payload: &[u8]) -> Result<(u32, u32), CodexSheetError> {
    if payload.len() < 10 {
        return Err(CodexSheetError::Unreadable(
            "truncated VP8X chunk".to_string(),
        ));
    }
    let width = u32::from_le_bytes([payload[4], payload[5], payload[6], 0]) + 1;
    let height = u32::from_le_bytes([payload[7], payload[8], payload[9], 0]) + 1;
    Ok((width, height))
}

fn vp8l_dimensions(payload: &[u8]) -> Result<(u32, u32), CodexSheetError> {
    if payload.len() < 5 || payload[0] != 0x2F {
        return Err(CodexSheetError::Unreadable(
            "truncated VP8L chunk".to_string(),
        ));
    }
    let bits = u32::from_le_bytes(payload[1..5].try_into().unwrap());
    let width = (bits & 0x3FFF) + 1;
    let height = ((bits >> 14) & 0x3FFF) + 1;
    Ok((width, height))
}

fn vp8_dimensions(payload: &[u8]) -> Result<(u32, u32), CodexSheetError> {
    // 3-byte frame tag, 3-byte start code, 2-byte width, 2-byte height.
    if payload.len() < 10 {
        return Err(CodexSheetError::Unreadable(
            "truncated VP8 chunk".to_string(),
        ));
    }
    if payload[3..6] != [0x9D, 0x01, 0x2A] {
        return Err(CodexSheetError::Unreadable(
            "VP8 frame is missing its start code".to_string(),
        ));
    }
    let width = u16::from_le_bytes(payload[6..8].try_into().unwrap()) as u32 & 0x3FFF;
    let height = u16::from_le_bytes(payload[8..10].try_into().unwrap()) as u32 & 0x3FFF;
    if width == 0 || height == 0 {
        return Err(CodexSheetError::Unreadable(
            "VP8 header has a zero dimension".to_string(),
        ));
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn solid_png(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba([16, 80, 140, 255]));
        let mut out = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut out);
        encoder
            .write_image(img.as_raw(), width, height, image::ColorType::Rgba8)
            .unwrap();
        out
    }

    fn webp_vp8x(width: u32, height: u32) -> Vec<u8> {
        let mut payload = [0u8; 10];
        let w = (width - 1).to_le_bytes();
        let h = (height - 1).to_le_bytes();
        payload[4] = w[0];
        payload[5] = w[1];
        payload[6] = w[2];
        payload[7] = h[0];
        payload[8] = h[1];
        payload[9] = h[2];
        let mut out = Vec::from(*b"RIFF\0\0\0\0WEBPVP8X");
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(payload);
        let riff_size = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&riff_size.to_le_bytes());
        out
    }

    fn jpeg_bytes() -> Vec<u8> {
        let img = RgbaImage::from_pixel(8, 8, Rgba([200, 10, 10, 255]));
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new(&mut out);
        img.write_with_encoder(encoder).unwrap();
        out
    }

    #[test]
    fn valid_webp_1536x1872_is_accepted() {
        let bytes = webp_vp8x(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(info.format, CodexSheetFormat::Webp);
        assert_eq!(info.width, CODEX_SHEET_WIDTH);
        assert_eq!(info.height, CODEX_SHEET_HEIGHT);
        assert_eq!(info.len, bytes.len() as u64);
    }

    #[test]
    fn png_is_unsupported_even_at_the_required_size() {
        let bytes = solid_png(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert_eq!(err, CodexSheetError::UnsupportedFormat);
        let text = err.to_string();
        assert!(text.contains("only WebP is accepted"), "{text}");
    }

    #[test]
    fn wrong_dimensions_report_actual_and_expected() {
        let bytes = webp_vp8x(256, 256);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert_eq!(
            err,
            CodexSheetError::WrongDimensions {
                width: 256,
                height: 256
            }
        );
        let text = err.to_string();
        assert!(text.contains("256x256"), "{text}");
        assert!(text.contains("1536x1872"), "{text}");
    }

    #[test]
    fn oversized_payload_is_rejected_before_format_checks() {
        let len = CODEX_SHEET_MAX_BYTES as usize + 1;
        let bytes = vec![0u8; len];
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert_eq!(err, CodexSheetError::TooLarge { len: len as u64 });
        let text = err.to_string();
        assert!(text.contains(&len.to_string()), "{text}");
        assert!(text.contains("20 MiB"), "{text}");
    }

    #[test]
    fn jpeg_and_gif_are_unsupported() {
        let jpeg = jpeg_bytes();
        assert!(jpeg.starts_with(JPEG_SOI), "fixture must be a JPEG");
        let err = validate_codex_sheet(&jpeg).unwrap_err();
        assert_eq!(err, CodexSheetError::UnsupportedFormat);
        assert!(err.to_string().contains("only WebP is accepted"), "{err}");

        let mut gif = GIF89A.to_vec();
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x3B]);
        assert_eq!(
            validate_codex_sheet(&gif).unwrap_err(),
            CodexSheetError::UnsupportedFormat
        );
    }

    #[test]
    fn truncated_webp_is_unreadable() {
        let webp = webp_vp8x(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let err = validate_codex_sheet(&webp[..16]).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("unreadable"), "{err}");
    }

    #[test]
    fn empty_file_is_unreadable() {
        let err = validate_codex_sheet(&[]).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)));
    }

    #[test]
    fn path_rejects_oversized_file_without_reading_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.bin");
        let len = CODEX_SHEET_MAX_BYTES + 1;
        let file = fs::File::create(&path).unwrap();
        file.set_len(len).unwrap();
        drop(file);
        let err = validate_codex_sheet_path(&path).unwrap_err();
        assert_eq!(err, CodexSheetError::TooLarge { len });
    }

    #[test]
    fn path_missing_file_is_unreadable() {
        let err = validate_codex_sheet_path(Path::new("/no/such/codex-sheet.webp")).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)));
    }

    #[test]
    fn vp8l_header_dimensions_are_read() {
        // Signature 0x2F, then 14-bit (width-1) and 14-bit (height-1), LSB first.
        let width = CODEX_SHEET_WIDTH;
        let height = CODEX_SHEET_HEIGHT;
        let packed = (width - 1) | ((height - 1) << 14);
        let mut payload = vec![0x2F];
        payload.extend(packed.to_le_bytes());
        let mut out = Vec::from(*b"RIFF\0\0\0\0WEBPVP8L");
        out.extend((payload.len() as u32).to_le_bytes());
        out.extend(&payload);
        if payload.len() % 2 == 1 {
            out.push(0);
        }
        let riff_size = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&riff_size.to_le_bytes());
        let info = validate_codex_sheet(&out).unwrap();
        assert_eq!(info.format, CodexSheetFormat::Webp);
        assert_eq!((info.width, info.height), (width, height));
    }
}
