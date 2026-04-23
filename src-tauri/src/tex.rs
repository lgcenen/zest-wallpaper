use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context, Result};
use image::{DynamicImage, ImageBuffer, Rgba, RgbaImage};

use crate::models::SceneAssetKind;

const TEXV_MAGIC: &[u8; 9] = b"TEXV0005\0";
const TEXI_MAGIC: &[u8; 9] = b"TEXI0001\0";
const TEXB_V1_MAGIC: &[u8; 9] = b"TEXB0001\0";
const TEXB_V2_MAGIC: &[u8; 9] = b"TEXB0002\0";
const TEXB_V3_MAGIC: &[u8; 9] = b"TEXB0003\0";
const TEXB_V4_MAGIC: &[u8; 9] = b"TEXB0004\0";
const TEXTURE_FLAG_VIDEO: u32 = 32;
const TEXTURE_FLAG_ALPHA_CHANNEL_PRIORITY: u32 = 524_288;
const FREE_IMAGE_UNKNOWN: u32 = u32::MAX;
const FREE_IMAGE_PNG: u32 = 13;
const FREE_IMAGE_JPEG: u32 = 2;
const FREE_IMAGE_GIF: u32 = 25;
const FREE_IMAGE_WEBP: u32 = 35;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextureContainerVersion {
    V1,
    V2,
    V3,
    V4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextureFormat {
    Rgba8888 = 0,
    Rgb888 = 1,
    Dxt5 = 4,
    Dxt3 = 6,
    Dxt1 = 7,
    Rg88 = 8,
    R8 = 9,
    Bc7 = 12,
}

#[derive(Debug, Clone)]
struct TextureHeader {
    format: TextureFormat,
    flags: u32,
    texture_width: u32,
    texture_height: u32,
    width: u32,
    height: u32,
    container_version: TextureContainerVersion,
    free_image_format: Option<u32>,
    is_video: bool,
    image_count: u32,
}

#[derive(Debug, Clone)]
struct TextureMipmap {
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ExtractedTextureAsset {
    pub kind: SceneAssetKind,
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

fn read_u32(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i32(cursor: &mut Cursor<&[u8]>) -> Result<i32> {
    let mut bytes = [0_u8; 4];
    cursor.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_magic(cursor: &mut Cursor<&[u8]>) -> Result<[u8; 9]> {
    let mut bytes = [0_u8; 9];
    cursor.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_null_terminated_string(cursor: &mut Cursor<&[u8]>) -> Result<String> {
    let bytes = cursor.get_ref();
    let start = cursor.position() as usize;
    let Some(end) = bytes[start..].iter().position(|byte| *byte == 0) else {
        bail!("Texture metadata string is missing a null terminator");
    };
    let end_index = start + end;
    let text = String::from_utf8(bytes[start..end_index].to_vec())?;
    cursor.set_position((end_index + 1) as u64);
    Ok(text)
}

fn parse_texture_format(value: u32) -> Result<TextureFormat> {
    match value {
        0 => Ok(TextureFormat::Rgba8888),
        1 => Ok(TextureFormat::Rgb888),
        4 => Ok(TextureFormat::Dxt5),
        6 => Ok(TextureFormat::Dxt3),
        7 => Ok(TextureFormat::Dxt1),
        8 => Ok(TextureFormat::Rg88),
        9 => Ok(TextureFormat::R8),
        12 => Ok(TextureFormat::Bc7),
        _ => Err(anyhow!("Unsupported TEX format value {value}")),
    }
}

fn parse_header(bytes: &[u8]) -> Result<(TextureHeader, Cursor<&[u8]>)> {
    let mut cursor = Cursor::new(bytes);

    let magic = read_magic(&mut cursor)?;
    if &magic != TEXV_MAGIC {
        bail!(
            "Unexpected TEX container magic {:?}",
            String::from_utf8_lossy(&magic)
        );
    }

    let sub_magic = read_magic(&mut cursor)?;
    if &sub_magic != TEXI_MAGIC {
        bail!(
            "Unexpected TEX info magic {:?}",
            String::from_utf8_lossy(&sub_magic)
        );
    }

    let format = parse_texture_format(read_u32(&mut cursor)?)?;
    let flags = read_u32(&mut cursor)?;
    let texture_width = read_u32(&mut cursor)?;
    let texture_height = read_u32(&mut cursor)?;
    let width = read_u32(&mut cursor)?;
    let height = read_u32(&mut cursor)?;
    let _editor_color = read_u32(&mut cursor)?;

    let container_magic = read_magic(&mut cursor)?;
    let image_count = read_u32(&mut cursor)?;

    let (container_version, free_image_format, is_video) = if &container_magic == TEXB_V4_MAGIC {
        let free_image_format = read_u32(&mut cursor)?;
        let is_video = read_u32(&mut cursor)? == 1;
        (
            TextureContainerVersion::V4,
            (free_image_format != FREE_IMAGE_UNKNOWN).then_some(free_image_format),
            is_video,
        )
    } else if &container_magic == TEXB_V3_MAGIC {
        let free_image_format = read_u32(&mut cursor)?;
        (
            TextureContainerVersion::V3,
            (free_image_format != FREE_IMAGE_UNKNOWN).then_some(free_image_format),
            false,
        )
    } else if &container_magic == TEXB_V2_MAGIC {
        (TextureContainerVersion::V2, None, false)
    } else if &container_magic == TEXB_V1_MAGIC {
        (TextureContainerVersion::V1, None, false)
    } else {
        bail!(
            "Unsupported TEX image container {:?}",
            String::from_utf8_lossy(&container_magic)
        );
    };

    Ok((
        TextureHeader {
            format,
            flags,
            texture_width,
            texture_height,
            width,
            height,
            container_version,
            free_image_format,
            is_video,
            image_count,
        },
        cursor,
    ))
}

fn read_mipmap(cursor: &mut Cursor<&[u8]>, header: &TextureHeader) -> Result<TextureMipmap> {
    if matches!(header.container_version, TextureContainerVersion::V4)
        && !v4_uses_standard_layout(cursor)?
    {
        let _param1 = read_u32(cursor)?;
        let _param2 = read_u32(cursor)?;
        let _condition_json = read_null_terminated_string(cursor)?;
        let _param3 = read_u32(cursor)?;
    }

    let width = read_u32(cursor)?;
    let height = read_u32(cursor)?;

    let (compression, uncompressed_size) = if matches!(
        header.container_version,
        TextureContainerVersion::V2 | TextureContainerVersion::V3 | TextureContainerVersion::V4
    ) {
        (read_u32(cursor)?, read_i32(cursor)?)
    } else {
        (0, 0)
    };

    let compressed_size = read_i32(cursor)?;
    if compressed_size < 0 {
        bail!("Encountered negative TEX mipmap size");
    }

    let compressed_size = compressed_size as usize;
    let mut compressed = vec![0_u8; compressed_size];
    cursor.read_exact(&mut compressed)?;

    let bytes = match compression {
        0 => compressed,
        1 => {
            let expected = usize::try_from(uncompressed_size.max(0)).unwrap_or(0);
            lz4_flex::block::decompress(&compressed, expected)
                .context("Failed to decompress LZ4 TEX mipmap payload")?
        }
        other => bail!("Unsupported TEX mipmap compression flag {other}"),
    };

    Ok(TextureMipmap {
        width,
        height,
        bytes,
    })
}

fn v4_uses_standard_layout(cursor: &Cursor<&[u8]>) -> Result<bool> {
    let bytes = cursor.get_ref();
    let start = cursor.position() as usize;
    if start + 20 > bytes.len() {
        return Ok(false);
    }

    let width = u32::from_le_bytes(bytes[start..start + 4].try_into()?);
    let height = u32::from_le_bytes(bytes[start + 4..start + 8].try_into()?);
    let compression = u32::from_le_bytes(bytes[start + 8..start + 12].try_into()?);
    let compressed_size = i32::from_le_bytes(bytes[start + 16..start + 20].try_into()?);
    let remaining = bytes.len().saturating_sub(start + 20);

    Ok(width > 0
        && height > 0
        && width <= 65_536
        && height <= 65_536
        && compression <= 1
        && compressed_size >= 0
        && (compressed_size as usize) <= remaining)
}

fn decode_raw_rgba(bytes: Vec<u8>, width: u32, height: u32) -> Result<RgbaImage> {
    ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bytes)
        .ok_or_else(|| anyhow!("TEX RGBA payload length does not match the declared dimensions"))
}

fn decode_raw_rgb(bytes: &[u8], width: u32, height: u32) -> Result<RgbaImage> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| anyhow!("TEX RGB payload size overflow"))?;
    if bytes.len() != expected {
        bail!("TEX RGB payload length does not match the declared dimensions");
    }
    let rgba = bytes
        .chunks_exact(3)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255_u8])
        .collect::<Vec<_>>();
    decode_raw_rgba(rgba, width, height)
}

fn decode_r8(bytes: &[u8], width: u32, height: u32, flags: u32) -> Result<RgbaImage> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| anyhow!("TEX R8 payload size overflow"))?;
    if bytes.len() != expected {
        bail!("TEX R8 payload length does not match the declared dimensions");
    }
    let alpha_priority = (flags & TEXTURE_FLAG_ALPHA_CHANNEL_PRIORITY) != 0;
    let rgba = bytes
        .iter()
        .flat_map(|value| {
            if alpha_priority {
                [255_u8, 255_u8, 255_u8, *value]
            } else {
                [*value, *value, *value, 255_u8]
            }
        })
        .collect::<Vec<_>>();
    decode_raw_rgba(rgba, width, height)
}

fn decode_rg88(bytes: &[u8], width: u32, height: u32) -> Result<RgbaImage> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(2))
        .ok_or_else(|| anyhow!("TEX RG88 payload size overflow"))?;
    if bytes.len() != expected {
        bail!("TEX RG88 payload length does not match the declared dimensions");
    }
    let rgba = bytes
        .chunks_exact(2)
        .flat_map(|channels| [channels[1], channels[1], channels[1], channels[0]])
        .collect::<Vec<_>>();
    decode_raw_rgba(rgba, width, height)
}

fn decode_bcn(
    bytes: &[u8],
    width: u32,
    height: u32,
    encoding: bcndecode::BcnEncoding,
) -> Result<RgbaImage> {
    let rgba = bcndecode::decode(
        bytes,
        width as usize,
        height as usize,
        encoding,
        bcndecode::BcnDecoderFormat::RGBA,
    )
    .map_err(|error| anyhow!("Failed to decode BCN texture: {error:?}"))?;
    decode_raw_rgba(rgba, width, height)
}

fn payload_looks_like_mp4(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return false;
    }
    matches!(
        &bytes[8..12],
        b"mp42" | b"isom" | b"msnv" | b"mp41" | b"avc1" | b"iso2"
    )
}

fn is_video_texture(header: &TextureHeader, mipmap: &TextureMipmap) -> bool {
    header.is_video
        || (header.flags & TEXTURE_FLAG_VIDEO) != 0
        || payload_looks_like_mp4(&mipmap.bytes)
}

fn decode_texture_image(header: &TextureHeader, mipmap: &TextureMipmap) -> Result<DynamicImage> {
    if let Some(free_image_format) = header.free_image_format {
        if matches!(
            free_image_format,
            FREE_IMAGE_PNG | FREE_IMAGE_JPEG | FREE_IMAGE_GIF | FREE_IMAGE_WEBP
        ) {
            let image = image::load_from_memory(&mipmap.bytes)
                .context("Unable to decode embedded image from TEX payload")?;
            return Ok(image);
        }
        bail!("Unsupported embedded TEX image format {free_image_format}");
    }

    let rgba = match header.format {
        TextureFormat::Rgba8888 => {
            decode_raw_rgba(mipmap.bytes.clone(), mipmap.width, mipmap.height)?
        }
        TextureFormat::Rgb888 => decode_raw_rgb(&mipmap.bytes, mipmap.width, mipmap.height)?,
        TextureFormat::R8 => decode_r8(&mipmap.bytes, mipmap.width, mipmap.height, header.flags)?,
        TextureFormat::Rg88 => decode_rg88(&mipmap.bytes, mipmap.width, mipmap.height)?,
        TextureFormat::Dxt1 => decode_bcn(
            &mipmap.bytes,
            mipmap.width,
            mipmap.height,
            bcndecode::BcnEncoding::Bc1,
        )?,
        TextureFormat::Dxt3 => decode_bcn(
            &mipmap.bytes,
            mipmap.width,
            mipmap.height,
            bcndecode::BcnEncoding::Bc2,
        )?,
        TextureFormat::Dxt5 => decode_bcn(
            &mipmap.bytes,
            mipmap.width,
            mipmap.height,
            bcndecode::BcnEncoding::Bc3,
        )?,
        TextureFormat::Bc7 => bail!("BC7 TEX textures are not supported yet"),
    };

    Ok(DynamicImage::ImageRgba8(rgba))
}

fn load_primary_mipmap(source_path: &Path) -> Result<(TextureHeader, TextureMipmap)> {
    let bytes = fs::read(source_path)
        .with_context(|| format!("Unable to read {}", source_path.display()))?;
    let (header, mut cursor) = parse_header(&bytes)?;

    if header.image_count == 0 {
        bail!("TEX entry does not contain any images");
    }

    let mut first_mipmap = None;

    for image_index in 0..header.image_count {
        let mipmap_count = read_u32(&mut cursor)?;
        for mipmap_index in 0..mipmap_count {
            let mipmap = read_mipmap(&mut cursor, &header)?;
            if image_index == 0 && mipmap_index == 0 {
                first_mipmap = Some(mipmap);
            }
        }
    }

    let mipmap = first_mipmap.ok_or_else(|| anyhow!("TEX entry does not contain any mipmaps"))?;
    Ok((header, mipmap))
}

fn crop_texture_image(header: &TextureHeader, mut image: DynamicImage) -> DynamicImage {
    let crop_width = header.width.min(image.width()).max(1);
    let crop_height = header.height.min(image.height()).max(1);
    if crop_width != image.width() || crop_height != image.height() {
        image = image.crop_imm(0, 0, crop_width, crop_height);
    }
    image
}

pub fn load_tex_image(source_path: &Path) -> Result<DynamicImage> {
    let (header, mipmap) = load_primary_mipmap(source_path)?;
    if is_video_texture(&header, &mipmap) {
        bail!("TEX entry stores video payloads and cannot be loaded as a still image");
    }
    let image = decode_texture_image(&header, &mipmap)?;
    Ok(crop_texture_image(&header, image))
}

pub fn extract_tex_asset(
    source_path: &Path,
    destination_stem: &Path,
) -> Result<ExtractedTextureAsset> {
    let (header, mipmap) = load_primary_mipmap(source_path)?;

    if let Some(parent) = destination_stem.parent() {
        fs::create_dir_all(parent)?;
    }

    if is_video_texture(&header, &mipmap) {
        let output_path = destination_stem.with_extension("mp4");
        fs::write(&output_path, &mipmap.bytes)
            .with_context(|| format!("Unable to write {}", output_path.display()))?;
        return Ok(ExtractedTextureAsset {
            kind: SceneAssetKind::Video,
            output_path,
            width: header.width.max(header.texture_width).max(1),
            height: header.height.max(header.texture_height).max(1),
        });
    }

    let image = crop_texture_image(&header, decode_texture_image(&header, &mipmap)?);
    let crop_width = image.width();
    let crop_height = image.height();

    let output_path = destination_stem.with_extension("png");
    image
        .save(&output_path)
        .with_context(|| format!("Unable to write {}", output_path.display()))?;

    Ok(ExtractedTextureAsset {
        kind: SceneAssetKind::Image,
        output_path,
        width: crop_width.max(header.texture_width.min(crop_width)),
        height: crop_height.max(header.texture_height.min(crop_height)),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::GenericImageView;
    use tempfile::tempdir;

    use super::{load_tex_image, payload_looks_like_mp4};

    fn rgba_tex_bytes(pixel: [u8; 4], width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(super::TEXV_MAGIC);
        bytes.extend_from_slice(super::TEXI_MAGIC);
        bytes.extend_from_slice(&(super::TextureFormat::Rgba8888 as u32).to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(super::TEXB_V4_MAGIC);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&super::FREE_IMAGE_UNKNOWN.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&4_i32.to_le_bytes());
        bytes.extend_from_slice(&pixel);
        bytes
    }

    #[test]
    fn detects_mp4_magic_in_mipmap_payload() {
        let payload = b"\0\0\0\x18ftypmp42\0\0\0\0mp42isom";
        assert!(payload_looks_like_mp4(payload));
    }

    #[test]
    fn load_tex_image_decodes_static_rgba_payloads() {
        let temp = tempdir().expect("temp dir");
        let tex_path = temp.path().join("pixel.tex");
        fs::write(&tex_path, rgba_tex_bytes([12, 34, 56, 78], 1, 1)).expect("write tex");

        let image = load_tex_image(&tex_path).expect("decode tex image");

        assert_eq!(image.dimensions(), (1, 1));
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [12, 34, 56, 78]);
    }
}
