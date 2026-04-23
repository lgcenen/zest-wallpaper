use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use image::{DynamicImage, Rgba, RgbaImage};

#[derive(Debug, Clone)]
pub struct ResolvedSystemTexture {
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub fn resolve_system_texture(key: &str, cache_root: &Path) -> Result<ResolvedSystemTexture> {
    fs::create_dir_all(cache_root)?;
    let file_name = sanitize_key(key);
    let output_path = cache_root.join(format!("{file_name}.png"));
    let (width, height) = match key {
        "$mediaThumbnail" => (512, 512),
        _ => (256, 256),
    };
    if !output_path.exists() {
        let image = match key {
            "$mediaThumbnail" => media_thumbnail_placeholder(),
            _ => generic_system_placeholder(key),
        };
        image
            .save(&output_path)
            .with_context(|| format!("Unable to write {}", output_path.display()))?;
    }

    Ok(ResolvedSystemTexture {
        output_path,
        width,
        height,
    })
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
}

fn media_thumbnail_placeholder() -> DynamicImage {
    let mut image = RgbaImage::new(512, 512);
    for y in 0..512 {
        let fy = y as f32 / 511.0;
        for x in 0..512 {
            let fx = x as f32 / 511.0;
            let base = [
                mix(28, 14, fy),
                mix(33, 22, fx),
                mix(38, 26, (fx + fy) * 0.5),
                255,
            ];
            image.put_pixel(x, y, Rgba(base));
        }
    }

    paint_rect(&mut image, 76, 76, 360, 360, [235, 240, 245, 24]);
    paint_rect(&mut image, 112, 112, 288, 288, [245, 246, 248, 30]);
    paint_rect(&mut image, 176, 160, 160, 192, [250, 236, 208, 48]);
    paint_rect(&mut image, 202, 186, 108, 140, [253, 248, 236, 228]);
    paint_rect(&mut image, 154, 370, 204, 16, [244, 162, 97, 210]);
    paint_rect(&mut image, 154, 396, 144, 10, [210, 214, 220, 124]);
    DynamicImage::ImageRgba8(image)
}

fn generic_system_placeholder(key: &str) -> DynamicImage {
    let mut image = RgbaImage::new(256, 256);
    let seed = key.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as u32)
    });
    for y in 0..256 {
        for x in 0..256 {
            let shade = (((x + y) as u32 + seed) % 48) as u8;
            image.put_pixel(
                x,
                y,
                Rgba([26 + shade, 34 + shade / 2, 42 + shade / 3, 255]),
            );
        }
    }
    paint_rect(&mut image, 48, 48, 160, 160, [255, 255, 255, 18]);
    paint_rect(&mut image, 72, 72, 112, 112, [255, 255, 255, 26]);
    DynamicImage::ImageRgba8(image)
}

fn mix(start: u8, end: u8, amount: f32) -> u8 {
    ((start as f32 * (1.0 - amount)) + (end as f32 * amount)).round() as u8
}

fn paint_rect(image: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    let end_x = (x + width).min(image.width());
    let end_y = (y + height).min(image.height());
    for py in y..end_y {
        for px in x..end_x {
            let base = image.get_pixel(px, py).0;
            let alpha = color[3] as f32 / 255.0;
            image.put_pixel(
                px,
                py,
                Rgba([
                    (base[0] as f32 * (1.0 - alpha) + color[0] as f32 * alpha).round() as u8,
                    (base[1] as f32 * (1.0 - alpha) + color[1] as f32 * alpha).round() as u8,
                    (base[2] as f32 * (1.0 - alpha) + color[2] as f32 * alpha).round() as u8,
                    255,
                ]),
            );
        }
    }
}
