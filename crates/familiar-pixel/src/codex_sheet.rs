//! Header-only validation for a still WebP pack sheet.
//!
//! This checks magic bytes, container and bitstream headers, canvas size, and
//! the file length cap. It does not map frames, decode pixels, crop, or import
//! animation data.

use std::fs::File;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

pub const CODEX_SHEET_WIDTH: u32 = 1536;
pub const CODEX_SHEET_HEIGHT: u32 = 1872;
pub const CODEX_SHEET_MAX_BYTES: u64 = 20 * 1024 * 1024;

const PNG_SIGNATURE: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const JPEG_SOI: &[u8] = &[0xFF, 0xD8, 0xFF];
const GIF87A: &[u8] = b"GIF87a";
const GIF89A: &[u8] = b"GIF89a";
const WEBP_HEADER_LEN: usize = 12;
const VP8X_ANIMATION_FLAG: u8 = 0x02;

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
    #[error("unsupported image format (only WebP is accepted); export the sheet as WebP. The file extension is ignored")]
    UnsupportedFormat,
    #[error("animated WebP is not supported; export a single still WebP image")]
    Animated,
    #[error("image is {width}x{height} pixels; expected {CODEX_SHEET_WIDTH}x{CODEX_SHEET_HEIGHT}; export a canvas of exactly that size")]
    WrongDimensions { width: u32, height: u32 },
    #[error("file is {len} bytes; maximum is {CODEX_SHEET_MAX_BYTES} bytes (20 MiB); compress or re-export the file")]
    TooLarge { len: u64 },
}

pub fn validate_codex_sheet_path(path: &Path) -> Result<CodexSheetInfo, CodexSheetError> {
    let file = File::open(path)
        .map_err(|err| CodexSheetError::Unreadable(format!("could not read file ({err})")))?;
    let meta = file.metadata().map_err(|err| {
        CodexSheetError::Unreadable(format!("could not read file metadata ({err})"))
    })?;
    if !meta.is_file() {
        return Err(CodexSheetError::Unreadable(
            "not a regular file; pass a path to a WebP file, not a directory".to_string(),
        ));
    }
    let len = meta.len();
    if len > CODEX_SHEET_MAX_BYTES {
        return Err(CodexSheetError::TooLarge { len });
    }
    let mut limited = file.take(CODEX_SHEET_MAX_BYTES + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|err| CodexSheetError::Unreadable(format!("could not read file ({err})")))?;
    if bytes.len() as u64 > CODEX_SHEET_MAX_BYTES {
        return Err(CodexSheetError::TooLarge {
            len: bytes.len() as u64,
        });
    }
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
        Detected::Webp => parse_webp(bytes)?,
        Detected::Unsupported => return Err(CodexSheetError::UnsupportedFormat),
        Detected::Unreadable => {
            return Err(CodexSheetError::Unreadable(
                "truncated or damaged header".to_string(),
            ))
        }
    };

    if width != CODEX_SHEET_WIDTH || height != CODEX_SHEET_HEIGHT {
        return Err(CodexSheetError::WrongDimensions { width, height });
    }

    Ok(CodexSheetInfo {
        format: CodexSheetFormat::Webp,
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
    if bytes.len() < WEBP_HEADER_LEN {
        return Detected::Unreadable;
    }
    Detected::Unsupported
}

fn is_webp_magic(bytes: &[u8]) -> bool {
    bytes.len() >= WEBP_HEADER_LEN && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
}

fn parse_webp(bytes: &[u8]) -> Result<(u32, u32), CodexSheetError> {
    if bytes.len() < WEBP_HEADER_LEN {
        return Err(CodexSheetError::Unreadable(
            "truncated WebP header".to_string(),
        ));
    }
    let riff_size = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64;
    let riff_end = 8u64
        .checked_add(riff_size)
        .ok_or_else(|| CodexSheetError::Unreadable("invalid RIFF size".to_string()))?;
    if riff_end < WEBP_HEADER_LEN as u64 {
        return Err(CodexSheetError::Unreadable(
            "RIFF size is below the WebP minimum".to_string(),
        ));
    }
    if riff_size % 2 == 1 {
        return Err(CodexSheetError::Unreadable("RIFF size is odd".to_string()));
    }
    if riff_end > bytes.len() as u64 {
        return Err(CodexSheetError::Unreadable(
            "RIFF size extends past the end of the file".to_string(),
        ));
    }
    if riff_end < bytes.len() as u64 {
        return Err(CodexSheetError::Unreadable(
            "trailing data after the RIFF payload".to_string(),
        ));
    }
    let riff_end = riff_end as usize;

    let mut offset = WEBP_HEADER_LEN;
    let mut canvas = None;
    let mut image = None;
    while offset < riff_end {
        if riff_end - offset < 8 {
            return Err(CodexSheetError::Unreadable(
                "truncated WebP chunk header".to_string(),
            ));
        }
        let fourcc = &bytes[offset..offset + 4];
        let chunk_size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let data_start = offset + 8;
        let Some(data_end) = data_start.checked_add(chunk_size) else {
            return Err(CodexSheetError::Unreadable(
                "WebP chunk extends past the RIFF payload".to_string(),
            ));
        };
        if data_end > riff_end {
            return Err(CodexSheetError::Unreadable(
                "WebP chunk extends past the RIFF payload".to_string(),
            ));
        }
        let padded = chunk_size + (chunk_size & 1);
        let Some(next) = data_start.checked_add(padded) else {
            return Err(CodexSheetError::Unreadable(
                "WebP chunk padding extends past the RIFF payload".to_string(),
            ));
        };
        if next > riff_end {
            return Err(CodexSheetError::Unreadable(
                "WebP chunk padding extends past the RIFF payload".to_string(),
            ));
        }
        let payload = &bytes[data_start..data_end];
        let first_chunk = offset == WEBP_HEADER_LEN;
        match fourcc {
            b"VP8X" => {
                if !first_chunk {
                    return Err(CodexSheetError::Unreadable(
                        "VP8X chunk must be the first chunk".to_string(),
                    ));
                }
                if payload.len() < 10 {
                    return Err(CodexSheetError::Unreadable(
                        "truncated VP8X chunk".to_string(),
                    ));
                }
                if payload[0] & VP8X_ANIMATION_FLAG != 0 {
                    return Err(CodexSheetError::Animated);
                }
                canvas = Some(vp8x_dimensions(payload)?);
            }
            b"ANIM" | b"ANMF" => return Err(CodexSheetError::Animated),
            b"VP8L" | b"VP8 " => {
                if image.is_some() {
                    return Err(CodexSheetError::Unreadable(
                        "WebP file has more than one VP8 or VP8L image chunk".to_string(),
                    ));
                }
                if canvas.is_none() && !first_chunk {
                    return Err(CodexSheetError::Unreadable(
                        "VP8/VP8L image chunk must be the first chunk".to_string(),
                    ));
                }
                image = Some(if fourcc == b"VP8L" {
                    vp8l_dimensions(payload)?
                } else {
                    vp8_dimensions(payload)?
                });
            }
            _ => {}
        }
        offset = next;
    }

    let image = image.ok_or_else(|| {
        CodexSheetError::Unreadable("WebP file has no VP8 or VP8L image chunk".to_string())
    })?;
    if let Some(canvas) = canvas {
        if canvas != image {
            return Err(CodexSheetError::Unreadable(format!(
                "VP8/VP8L image is {}x{}; it does not match the VP8X canvas {}x{}",
                image.0, image.1, canvas.0, canvas.1
            )));
        }
    }
    Ok(image)
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
    if payload.len() < 5 {
        return Err(CodexSheetError::Unreadable(
            "truncated VP8L chunk".to_string(),
        ));
    }
    if payload[0] != 0x2F {
        return Err(CodexSheetError::Unreadable(
            "VP8L frame is missing its signature".to_string(),
        ));
    }
    let bits = u32::from_le_bytes(payload[1..5].try_into().unwrap());
    let version = (bits >> 29) & 0x7;
    if version != 0 {
        return Err(CodexSheetError::Unreadable(
            "VP8L version must be 0".to_string(),
        ));
    }
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
    if payload[0] & 0x01 != 0 {
        return Err(CodexSheetError::Unreadable(
            "VP8 frame is not a keyframe".to_string(),
        ));
    }
    if payload[3..6] != [0x9D, 0x01, 0x2A] {
        return Err(CodexSheetError::Unreadable(
            "VP8 image data is missing its start code (damaged WebP)".to_string(),
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
    use std::fs;

    fn solid_png(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba([16, 80, 140, 255]));
        let mut out = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut out);
        encoder
            .write_image(img.as_raw(), width, height, image::ColorType::Rgba8)
            .unwrap();
        out
    }

    fn vp8x_payload(width: u32, height: u32, flags: u8) -> Vec<u8> {
        let mut payload = vec![0u8; 10];
        payload[0] = flags;
        let w = (width - 1).to_le_bytes();
        let h = (height - 1).to_le_bytes();
        payload[4] = w[0];
        payload[5] = w[1];
        payload[6] = w[2];
        payload[7] = h[0];
        payload[8] = h[1];
        payload[9] = h[2];
        payload
    }

    fn vp8l_payload(width: u32, height: u32) -> Vec<u8> {
        let packed = (width - 1) | ((height - 1) << 14);
        let mut payload = vec![0x2F];
        payload.extend(packed.to_le_bytes());
        payload
    }

    fn vp8_payload(width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![0u8; 10];
        payload[0] = 0x10;
        payload[3] = 0x9D;
        payload[4] = 0x01;
        payload[5] = 0x2A;
        let w = (width as u16 & 0x3FFF).to_le_bytes();
        let h = (height as u16 & 0x3FFF).to_le_bytes();
        payload[6] = w[0];
        payload[7] = w[1];
        payload[8] = h[0];
        payload[9] = h[1];
        payload
    }

    fn webp_riff(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::from(*b"WEBP");
        for (fourcc, payload) in chunks {
            body.extend_from_slice(*fourcc);
            body.extend((payload.len() as u32).to_le_bytes());
            body.extend(payload);
            if payload.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut out = Vec::from(*b"RIFF");
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    fn webp_vp8x_only(width: u32, height: u32) -> Vec<u8> {
        webp_riff(&[(b"VP8X", vp8x_payload(width, height, 0))])
    }

    fn webp_simple_vp8l(width: u32, height: u32) -> Vec<u8> {
        webp_riff(&[(b"VP8L", vp8l_payload(width, height))])
    }

    fn webp_simple_vp8(width: u32, height: u32) -> Vec<u8> {
        webp_riff(&[(b"VP8 ", vp8_payload(width, height))])
    }

    fn webp_vp8x_then_vp8(canvas: (u32, u32), image: (u32, u32)) -> Vec<u8> {
        webp_riff(&[
            (b"VP8X", vp8x_payload(canvas.0, canvas.1, 0)),
            (b"VP8 ", vp8_payload(image.0, image.1)),
        ])
    }

    fn pad_webp_with_unknown_chunk(mut bytes: Vec<u8>, target_len: usize) -> Vec<u8> {
        assert!(
            target_len >= bytes.len() + 8,
            "target must leave room for an unknown chunk header"
        );
        let extra = target_len - bytes.len();
        assert_eq!(extra % 2, 0, "unknown chunk total must be even");
        let payload_len = extra - 8;
        bytes.extend(b"JUNK");
        bytes.extend((payload_len as u32).to_le_bytes());
        bytes.resize(target_len, 0);
        let riff_size = (target_len - 8) as u32;
        bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
        bytes
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
        let bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(info.format, CodexSheetFormat::Webp);
        assert_eq!(info.width, CODEX_SHEET_WIDTH);
        assert_eq!(info.height, CODEX_SHEET_HEIGHT);
        assert_eq!(info.len, bytes.len() as u64);
    }

    #[test]
    fn valid_vp8_bitstream_1536x1872_is_accepted() {
        let bytes = webp_simple_vp8(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(
            (info.width, info.height),
            (CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)
        );
    }

    #[test]
    fn vp8x_followed_by_matching_vp8l_is_accepted() {
        let bytes = webp_riff(&[
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
            (b"VP8L", vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
        ]);
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(
            (info.width, info.height),
            (CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)
        );
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
        let bytes = webp_simple_vp8l(256, 256);
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
    fn vp8x_only_chunk_is_rejected() {
        let bytes = webp_vp8x_only(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        assert_eq!(bytes.len(), 30);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string().contains("no VP8 or VP8L image chunk"),
            "{err}"
        );
    }

    #[test]
    fn animated_vp8x_flag_is_rejected() {
        let bytes = webp_riff(&[(
            b"VP8X",
            vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, VP8X_ANIMATION_FLAG),
        )]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert_eq!(err, CodexSheetError::Animated);
        assert_eq!(
            err.to_string(),
            "animated WebP is not supported; export a single still WebP image"
        );
    }

    #[test]
    fn anim_and_anmf_chunks_are_rejected() {
        let bytes = webp_riff(&[
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
            (b"ANIM", vec![0; 6]),
            (b"VP8L", vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
        ]);
        assert_eq!(
            validate_codex_sheet(&bytes).unwrap_err(),
            CodexSheetError::Animated
        );

        let bytes = webp_riff(&[
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
            (b"ANMF", vec![0; 16]),
            (b"VP8L", vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
        ]);
        assert_eq!(
            validate_codex_sheet(&bytes).unwrap_err(),
            CodexSheetError::Animated
        );
    }

    #[test]
    fn vp8x_canvas_and_vp8_bitstream_must_match() {
        let bytes = webp_vp8x_then_vp8((CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT), (64, 64));
        let err = validate_codex_sheet(&bytes).unwrap_err();
        let text = err.to_string();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(text.contains("64x64"), "{text}");
        assert!(text.contains("1536x1872"), "{text}");
        assert!(text.contains("does not match the VP8X canvas"), "{text}");
    }

    #[test]
    fn riff_size_zero_is_rejected() {
        let mut bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("RIFF size is below the WebP minimum"),
            "{err}"
        );
    }

    #[test]
    fn riff_size_past_end_of_file_is_rejected() {
        let mut bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        bytes[4..8].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("RIFF size extends past the end of the file"),
            "{err}"
        );
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
    fn exactly_20_mib_valid_webp_is_accepted() {
        let bytes = pad_webp_with_unknown_chunk(
            webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT),
            CODEX_SHEET_MAX_BYTES as usize,
        );
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(info.len, CODEX_SHEET_MAX_BYTES);
        assert_eq!(
            (info.width, info.height),
            (CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)
        );
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
        let webp = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let err = validate_codex_sheet(&webp[..16]).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("unreadable"), "{err}");
    }

    #[test]
    fn truncated_vp8_bitstream_is_unreadable() {
        let mut payload = vp8_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        payload.truncate(4);
        let bytes = webp_riff(&[
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
            (b"VP8 ", payload),
        ]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("truncated VP8"), "{err}");
    }

    #[test]
    fn vp8_interframe_is_rejected() {
        let mut payload = vp8_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        payload[0] |= 0x01;
        let bytes = webp_riff(&[(b"VP8 ", payload)]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("not a keyframe"), "{err}");
    }

    #[test]
    fn vp8_missing_start_code_is_rejected() {
        let mut payload = vp8_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        payload[3] = 0x00;
        let bytes = webp_riff(&[(b"VP8 ", payload)]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("start code"), "{err}");
    }

    #[test]
    fn vp8l_nonzero_version_is_rejected() {
        let mut payload = vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        payload[4] |= 0x20;
        let bytes = webp_riff(&[(b"VP8L", payload)]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("VP8L version must be 0"), "{err}");
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
    fn path_accepts_exactly_20_mib_valid_webp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sheet.webp");
        let bytes = pad_webp_with_unknown_chunk(
            webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT),
            CODEX_SHEET_MAX_BYTES as usize,
        );
        fs::write(&path, &bytes).unwrap();
        let info = validate_codex_sheet_path(&path).unwrap();
        assert_eq!(info.len, CODEX_SHEET_MAX_BYTES);
    }

    #[test]
    fn path_missing_file_is_unreadable() {
        let err = validate_codex_sheet_path(Path::new("/no/such/codex-sheet.webp")).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)));
    }

    #[test]
    fn vp8_then_vp8l_is_rejected() {
        let bytes = webp_riff(&[
            (b"VP8 ", vp8_payload(64, 64)),
            (b"VP8L", vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
        ]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("more than one VP8 or VP8L image chunk"),
            "{err}"
        );
    }

    #[test]
    fn vp8_before_vp8x_is_rejected() {
        let bytes = webp_riff(&[
            (b"VP8 ", vp8_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
        ]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("VP8X chunk must be the first chunk"),
            "{err}"
        );
    }

    #[test]
    fn two_image_bitstreams_are_rejected() {
        let bytes = webp_riff(&[
            (
                b"VP8X",
                vp8x_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT, 0),
            ),
            (b"VP8 ", vp8_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
            (b"VP8L", vp8l_payload(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)),
        ]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("more than one VP8 or VP8L image chunk"),
            "{err}"
        );
    }

    #[test]
    fn odd_riff_size_is_rejected() {
        let mut bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        bytes.push(0);
        let size = (bytes.len() - 8) as u32;
        assert_eq!(size % 2, 1);
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(err.to_string().contains("RIFF size is odd"), "{err}");
    }

    #[test]
    fn trailing_data_after_riff_is_rejected() {
        let mut bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        bytes.extend_from_slice(&[0, 1, 2, 3]);
        let err = validate_codex_sheet(&bytes).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert!(
            err.to_string()
                .contains("trailing data after the RIFF payload"),
            "{err}"
        );
    }

    #[test]
    fn path_rejects_directory() {
        let dir = tempfile::tempdir().unwrap();
        let err = validate_codex_sheet_path(dir.path()).unwrap_err();
        assert!(matches!(err, CodexSheetError::Unreadable(_)), "{err:?}");
        assert_eq!(
            err.to_string(),
            "image file is unreadable: not a regular file; pass a path to a WebP file, not a directory"
        );
    }

    #[test]
    fn vp8l_header_dimensions_are_read() {
        let bytes = webp_simple_vp8l(CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT);
        let info = validate_codex_sheet(&bytes).unwrap();
        assert_eq!(info.format, CodexSheetFormat::Webp);
        assert_eq!(
            (info.width, info.height),
            (CODEX_SHEET_WIDTH, CODEX_SHEET_HEIGHT)
        );
    }
}
