use std::{cell::RefCell, collections::BTreeMap, ffi::c_void};

use image::DynamicImage;
use objc2::{
    rc::Retained,
    runtime::AnyObject,
};
use objc2_app_kit::{
    NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSGraphicsContext,
    NSImageInterpolation, NSLineBreakMode, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSStringDrawingContext, NSStringDrawingOptions,
    NSStringNSExtendedStringDrawing, NSTextAlignment,
};
use objc2_core_foundation::{CFArray, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_core_text::{CTFontDescriptor, CTFontManagerCreateFontDescriptorsFromURL};
use objc2_foundation::{NSDictionary, NSAttributedStringKey, NSString, NSURL};

use crate::{
    models::SceneTextBehavior,
    services::scene_render_planner_service::{
        SceneRenderColor, SceneRenderTextItem, SceneTextHorizontalAlign,
    },
};

#[derive(Clone)]
struct SceneCachedTextFont {
    font: Retained<NSFont>,
    fallback_detail: Option<SceneTextFontFallbackDetail>,
}

thread_local! {
    static SCENE_TEXT_FONT_CACHE: RefCell<BTreeMap<String, SceneCachedTextFont>> = const {
        RefCell::new(BTreeMap::new())
    };
}

pub(crate) struct SceneTextRasterizedTexture<W> {
    pub(crate) image: DynamicImage,
    pub(crate) warnings: Vec<W>,
}

pub(crate) struct SceneResolvedTextFont {
    pub(crate) font: Retained<NSFont>,
    pub(crate) fallback_detail: Option<SceneTextFontFallbackDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneTextFontFallbackDetail {
    pub(crate) reason: String,
    pub(crate) attempted_files: Vec<String>,
    pub(crate) family_candidates: Vec<String>,
}

pub(crate) fn rasterize_text_texture<W, F>(
    item: &SceneRenderTextItem,
    unsupported_effect_warning: F,
    text_font_fallback_warning: impl Fn(&SceneRenderTextItem, SceneTextFontFallbackDetail) -> W,
) -> Result<SceneTextRasterizedTexture<W>, String>
where
    F: Fn(&SceneRenderTextItem, &str) -> W,
{
    let width = item.quad.width.max(1.0).ceil() as usize;
    let height = item.quad.height.max(1.0).ceil() as usize;
    let bytes_per_row = width
        .checked_mul(4)
        .ok_or_else(|| "text texture row size overflowed".to_string())?;
    let total_bytes = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| "text texture buffer size overflowed".to_string())?;
    let mut pixels = vec![0_u8; total_bytes];
    let color_space = CGColorSpace::new_device_rgb()
        .ok_or_else(|| "Core Graphics RGB color space is unavailable".to_string())?;
    let bitmap_info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr().cast::<c_void>(),
            width,
            height,
            8,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
        )
    }
    .ok_or_else(|| "unable to create Core Graphics bitmap context for text".to_string())?;

    let graphics_context = NSGraphicsContext::graphicsContextWithCGContext_flipped(&context, true);
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&graphics_context));
    graphics_context.setImageInterpolation(NSImageInterpolation::High);

    let color = nscolor_from_scene_color(item.color);
    let paragraph_style = paragraph_style_for_text(item);
    let string = NSString::from_str(item.text.as_str());
    let options = text_drawing_options(item);
    let mut warnings = item
        .effect_paths
        .iter()
        .filter(|path| !path.to_ascii_lowercase().contains("blur"))
        .map(|path| unsupported_effect_warning(item, path))
        .collect::<Vec<_>>();
    let resolved_point_size =
        resolve_scene_text_point_size(item, &string, &color, &paragraph_style, options)?;
    let resolved_font = scene_text_font_with_point_size(item, resolved_point_size)?;
    if let Some(detail) = resolved_font.fallback_detail.clone() {
        warnings.push(text_font_fallback_warning(item, detail));
    }
    let attributes = build_text_attributes(&resolved_font.font, &color, &paragraph_style);
    let rect = CGRect::new(
        CGPoint::new(item.content_left, item.content_top),
        CGSize::new(item.content_width.max(1.0), item.content_height.max(1.0)),
    );
    unsafe {
        string.drawWithRect_options_attributes_context(
            rect,
            options,
            Some(&attributes),
            Some(&NSStringDrawingContext::new()),
        );
    }
    graphics_context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    let mut image = image::RgbaImage::from_raw(width as u32, height as u32, pixels)
        .ok_or_else(|| "text pixels could not be rewrapped as RGBA".to_string())?;
    image::imageops::flip_vertical_in_place(&mut image);
    unpremultiply_rgba_pixels(&mut image);
    if item.blur_enabled {
        let blur_source = image.clone();
        let mut blurred = image::imageops::blur(
            &DynamicImage::ImageRgba8(blur_source),
            item.blur_radius as f32,
        );
        for pixel in blurred.pixels_mut() {
            pixel.0[3] = ((pixel.0[3] as f32) * 0.34) as u8;
        }
        image::imageops::overlay(&mut blurred, &image, 0, 0);
        image = blurred;
    }
    Ok(SceneTextRasterizedTexture {
        image: DynamicImage::ImageRgba8(image),
        warnings,
    })
}

pub(crate) fn rasterize_scene_text_item_snapshot(
    item: &SceneRenderTextItem,
) -> Result<DynamicImage, String> {
    rasterize_text_texture(item, |_item, _path| (), |_item, _detail| ())
        .map(|rasterized| rasterized.image)
}

pub(crate) fn text_texture_cache_key(item: &SceneRenderTextItem) -> String {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    item.object_id.hash(&mut hasher);
    item.behavior.hash(&mut hasher);
    item.text.hash(&mut hasher);
    item.font.cache_key.hash(&mut hasher);
    item.font.file_candidates.hash(&mut hasher);
    item.font.family_candidates.hash(&mut hasher);
    item.point_size.to_bits().hash(&mut hasher);
    item.color.red.hash(&mut hasher);
    item.color.green.hash(&mut hasher);
    item.color.blue.hash(&mut hasher);
    item.color.alpha.hash(&mut hasher);
    item.horizontal_align.hash(&mut hasher);
    item.vertical_align.hash(&mut hasher);
    item.blur_enabled.hash(&mut hasher);
    item.blur_radius.to_bits().hash(&mut hasher);
    item.max_rows.hash(&mut hasher);
    item.limit_width.hash(&mut hasher);
    item.limit_use_ellipsis.hash(&mut hasher);
    item.dynamic_input_generation.hash(&mut hasher);
    item.quad.width.to_bits().hash(&mut hasher);
    item.quad.height.to_bits().hash(&mut hasher);
    item.content_left.to_bits().hash(&mut hasher);
    item.content_top.to_bits().hash(&mut hasher);
    item.content_width.to_bits().hash(&mut hasher);
    item.content_height.to_bits().hash(&mut hasher);
    item.effect_paths.hash(&mut hasher);
    format!("text:{}:{:x}", item.object_id, hasher.finish())
}

pub(crate) fn scene_text_font_with_point_size(
    item: &SceneRenderTextItem,
    point_size: f64,
) -> Result<SceneResolvedTextFont, String> {
    let cache_key = scene_text_font_cache_key(item, point_size);
    if let Some(cached_font) =
        SCENE_TEXT_FONT_CACHE.with(|cache| cache.borrow().get(&cache_key).cloned())
    {
        return Ok(SceneResolvedTextFont {
            font: cached_font.font,
            fallback_detail: cached_font.fallback_detail,
        });
    }

    let font = scene_text_font_uncached(item, point_size)?;
    SCENE_TEXT_FONT_CACHE.with(|cache| {
        cache.borrow_mut().insert(
            cache_key,
            SceneCachedTextFont {
                font: font.font.clone(),
                fallback_detail: font.fallback_detail.clone(),
            },
        );
    });
    Ok(font)
}

pub(crate) fn scene_text_fit_scale(
    item: &SceneRenderTextItem,
    measured_width: f64,
    measured_height: f64,
) -> f64 {
    let container_width = item.content_width.max(1.0);
    let container_height = item.content_height.max(1.0);
    let width_scale = container_width / measured_width.max(1.0);
    let height_scale = container_height / measured_height.max(1.0);
    let prefers_width = item.behavior == SceneTextBehavior::Static;
    let mut scale = if prefers_width {
        width_scale
    } else {
        height_scale
    };
    if measured_width * scale > container_width || item.limit_width || item.limit_use_ellipsis {
        scale = scale.min(width_scale);
    }
    if measured_height * scale > container_height {
        scale = scale.min(height_scale);
    }
    scale.clamp(0.05, 16.0)
}

pub(crate) fn unpremultiply_rgba_pixels(image: &mut image::RgbaImage) {
    for pixel in image.pixels_mut() {
        let alpha = pixel.0[3];
        if alpha == 0 || alpha == 255 {
            continue;
        }
        let alpha_scale = 255.0 / alpha as f32;
        pixel.0[0] = ((pixel.0[0] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
        pixel.0[1] = ((pixel.0[1] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
        pixel.0[2] = ((pixel.0[2] as f32 * alpha_scale).round()).clamp(0.0, 255.0) as u8;
    }
}

fn resolve_scene_text_point_size(
    item: &SceneRenderTextItem,
    text: &NSString,
    color: &NSColor,
    paragraph_style: &NSMutableParagraphStyle,
    options: NSStringDrawingOptions,
) -> Result<f64, String> {
    let authored_fit_behavior =
        matches!(item.behavior, SceneTextBehavior::Static | SceneTextBehavior::Clock);
    let needs_native_metric_fit = authored_fit_behavior
        || item.text.contains('\n')
        || item.limit_width
        || item.limit_use_ellipsis
        || item.max_rows.unwrap_or_default() > 1;
    if !needs_native_metric_fit {
        return Ok(item.point_size.max(1.0));
    }

    let mut point_size = item.point_size.max(1.0);
    for _ in 0..3 {
        let font = scene_text_font_with_point_size(item, point_size)?.font;
        let attributes = build_text_attributes(&font, color, paragraph_style);
        let measured = measure_scene_text_bounds(text, &attributes, item, options);
        let scale = if authored_fit_behavior {
            scene_text_fit_scale(item, measured.size.width, measured.size.height)
        } else {
            scene_text_height_fit_scale(item, measured.size.height).min(1.0)
        };
        let next_point_size = (point_size * scale).clamp(1.0, 2048.0);
        if (next_point_size - point_size).abs() < 0.5 {
            point_size = next_point_size;
            break;
        }
        point_size = next_point_size;
    }
    Ok(point_size)
}

fn measure_scene_text_bounds(
    text: &NSString,
    attributes: &NSDictionary<NSAttributedStringKey, AnyObject>,
    item: &SceneRenderTextItem,
    options: NSStringDrawingOptions,
) -> CGRect {
    let constrain_width =
        item.limit_width || item.max_rows.unwrap_or_default() > 1 || item.text.contains('\n');
    let measure_width = if constrain_width {
        item.content_width.max(1.0)
    } else {
        100_000.0
    };
    let measure_height = 100_000.0;
    unsafe {
        text.boundingRectWithSize_options_attributes_context(
            CGSize::new(measure_width, measure_height),
            options,
            Some(attributes),
            Some(&NSStringDrawingContext::new()),
        )
    }
}

fn scene_text_height_fit_scale(item: &SceneRenderTextItem, measured_height: f64) -> f64 {
    let container_height = item.content_height.max(1.0);
    (container_height / measured_height.max(1.0)).clamp(0.05, 16.0)
}

fn scene_text_font_uncached(
    item: &SceneRenderTextItem,
    point_size: f64,
) -> Result<SceneResolvedTextFont, String> {
    for font_path in &item.font.file_candidates {
        let Some(path_string) = font_path.to_str() else {
            return Err(format!(
                "font path {} is not valid UTF-8",
                font_path.display()
            ));
        };
        let path_string = NSString::from_str(path_string);
        let url = NSURL::fileURLWithPath(&path_string);
        if let Some(descriptors) =
            unsafe { CTFontManagerCreateFontDescriptorsFromURL(url.as_ref()) }
        {
            let typed_descriptors: &CFArray<CTFontDescriptor> =
                unsafe { &*((&*descriptors) as *const _ as *const CFArray<CTFontDescriptor>) };
            if let Some(descriptor) = typed_descriptors.get(0) {
                if let Some(font) = NSFont::fontWithDescriptor_size(descriptor.as_ref(), point_size)
                {
                    return Ok(SceneResolvedTextFont {
                        font,
                        fallback_detail: None,
                    });
                }
            }
        }
    }

    for family_name in &item.font.family_candidates {
        let family_name = NSString::from_str(family_name);
        if let Some(font) = NSFont::fontWithName_size(&family_name, point_size) {
            return Ok(SceneResolvedTextFont {
                font,
                fallback_detail: None,
            });
        }
    }

    let fallback_detail = item
        .font
        .authored_reference
        .as_ref()
        .map(|authored_reference| SceneTextFontFallbackDetail {
            reason: format!(
                "Font reference {authored_reference:?} did not resolve through Scene content, external assets, builtin assets, or authored family candidates; the renderer used the system font as the final fallback."
            ),
            attempted_files: item
                .font
                .file_candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            family_candidates: item.font.family_candidates.clone(),
        });

    Ok(SceneResolvedTextFont {
        font: NSFont::systemFontOfSize(point_size),
        fallback_detail,
    })
}

fn scene_text_font_cache_key(item: &SceneRenderTextItem, point_size: f64) -> String {
    format!("{}:{}", item.font.cache_key, point_size.to_bits())
}

fn build_text_attributes(
    font: &NSFont,
    color: &NSColor,
    paragraph_style: &NSMutableParagraphStyle,
) -> Retained<NSDictionary<NSAttributedStringKey, AnyObject>> {
    unsafe {
        NSDictionary::from_slices(
            &[
                NSFontAttributeName,
                NSForegroundColorAttributeName,
                NSParagraphStyleAttributeName,
            ],
            &[font, color, paragraph_style],
        )
    }
}

fn nscolor_from_scene_color(color: SceneRenderColor) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        color.red as f64 / 255.0,
        color.green as f64 / 255.0,
        color.blue as f64 / 255.0,
        color.alpha as f64 / 255.0,
    )
}

fn paragraph_style_for_text(item: &SceneRenderTextItem) -> Retained<NSMutableParagraphStyle> {
    let style = NSMutableParagraphStyle::new();
    style.setAlignment(match item.horizontal_align {
        SceneTextHorizontalAlign::Left => NSTextAlignment::Left,
        SceneTextHorizontalAlign::Right => NSTextAlignment::Right,
        SceneTextHorizontalAlign::Center => NSTextAlignment::Center,
    });
    style.setLineBreakMode(if item.limit_use_ellipsis {
        NSLineBreakMode::ByTruncatingTail
    } else if item.limit_width {
        NSLineBreakMode::ByWordWrapping
    } else {
        NSLineBreakMode::ByClipping
    });
    style
}

fn text_drawing_options(item: &SceneRenderTextItem) -> NSStringDrawingOptions {
    let mut options =
        NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading;
    if item.limit_use_ellipsis || item.max_rows.unwrap_or_default() > 1 {
        options |= NSStringDrawingOptions::TruncatesLastVisibleLine;
    }
    options
}

#[cfg(test)]
mod tests {
    use super::{
        rasterize_text_texture, scene_text_fit_scale, scene_text_font_with_point_size,
        text_texture_cache_key, unpremultiply_rgba_pixels,
    };
    use crate::{
        models::SceneTextBehavior,
        services::{
            scene_render_planner_service::{
                SceneRenderColor, SceneRenderQuad, SceneRenderTextFontBinding,
                SceneRenderTextItem, SceneTextHorizontalAlign, SceneTextVerticalAlign,
            },
            scene_resource_service::SceneTextFontReferenceKind,
        },
    };

    #[cfg(target_os = "macos")]
    use image::RgbaImage;

    fn sample_text_item() -> SceneRenderTextItem {
        SceneRenderTextItem {
            object_id: 22,
            object_name: "Clock".to_string(),
            behavior: SceneTextBehavior::Clock,
            quad: SceneRenderQuad {
                left: 420.0,
                top: 320.0,
                width: 320.0,
                height: 120.0,
                rotation: 0.0,
                opacity: 1.0,
                flip_x: false,
                flip_y: false,
            },
            content_left: 0.0,
            content_top: 0.0,
            content_width: 320.0,
            content_height: 120.0,
            text: "22:34:53".to_string(),
            font: SceneRenderTextFontBinding {
                authored_reference: Some("fonts/test-clock.otf".to_string()),
                reference_kind: None,
                file_candidates: Vec::new(),
                family_candidates: vec!["Helvetica".to_string()],
                cache_key: "font:test-clock".to_string(),
            },
            point_size: 72.0,
            color: SceneRenderColor {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 220,
            },
            horizontal_align: SceneTextHorizontalAlign::Center,
            vertical_align: SceneTextVerticalAlign::Center,
            blur_enabled: false,
            blur_radius: 0.0,
            effect_paths: Vec::new(),
            max_rows: None,
            limit_width: false,
            limit_use_ellipsis: false,
            dynamic_input_generation: None,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unpremultiply_rgba_restores_straight_alpha_channels() {
        let mut image =
            RgbaImage::from_raw(1, 1, vec![90, 40, 20, 128]).expect("premultiplied RGBA pixel");

        unpremultiply_rgba_pixels(&mut image);

        assert_eq!(image.into_raw(), vec![179, 80, 40, 128]);
    }

    #[test]
    fn text_texture_cache_key_ignores_quad_position_only_changes() {
        let mut item = sample_text_item();
        let key = text_texture_cache_key(&item);

        item.quad.left += 240.0;
        item.quad.top += 80.0;

        assert_eq!(key, text_texture_cache_key(&item));
    }

    #[test]
    fn text_texture_cache_key_tracks_style_and_effect_path_changes() {
        let mut item = sample_text_item();
        let key = text_texture_cache_key(&item);

        item.color.alpha = 255;
        assert_ne!(key, text_texture_cache_key(&item));

        let mut item = sample_text_item();
        item.effect_paths.push("effects/glow.json".to_string());
        assert_ne!(key, text_texture_cache_key(&item));
    }

    #[test]
    fn text_texture_cache_key_tracks_dynamic_input_generation() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::MediaTitle;
        item.dynamic_input_generation = Some(1);
        let key = text_texture_cache_key(&item);

        item.dynamic_input_generation = Some(2);

        assert_ne!(key, text_texture_cache_key(&item));
    }

    #[test]
    fn text_font_cache_key_tracks_font_binding_changes() {
        let mut item = sample_text_item();
        let key = text_texture_cache_key(&item);

        item.font.cache_key = "font:updated".to_string();
        assert_ne!(key, text_texture_cache_key(&item));

        let mut item = sample_text_item();
        item.font
            .family_candidates
            .push("DIN Alternate".to_string());
        assert_ne!(text_texture_cache_key(&sample_text_item()), text_texture_cache_key(&item));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn text_font_cache_preserves_fallback_diagnostics_for_family_only_fonts() {
        let mut item = sample_text_item();
        item.font.authored_reference = Some("__WallpaperPhase09A_MissingFamily__".to_string());
        item.font.reference_kind = Some(SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["__WallpaperPhase09A_MissingFamily__".to_string()];
        item.font.cache_key = "font:phase-09a-missing-family".to_string();

        let first =
            scene_text_font_with_point_size(&item, 17.0).expect("first font resolution");
        let second =
            scene_text_font_with_point_size(&item, 17.0).expect("cached font resolution");

        assert!(first.fallback_detail.is_some());
        assert_eq!(first.fallback_detail, second.fallback_detail);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn plain_text_rasterization_does_not_add_unauthored_shadow_pixels() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Static;
        item.object_name = "Plain Caption".to_string();
        item.text = "SHADOW".to_string();
        item.point_size = 72.0;
        item.content_width = 420.0;
        item.content_height = 180.0;
        item.quad.width = 420.0;
        item.quad.height = 180.0;
        item.color.alpha = 255;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind = Some(SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-plain-shadow-regression".to_string();

        let rasterized = rasterize_text_texture(&item, |_item, _path| (), |_item, _detail| ())
            .expect("plain text raster");
        let dark_shadow_pixels = rasterized
            .image
            .to_rgba8()
            .pixels()
            .filter(|pixel| pixel.0[3] > 8 && pixel.0[0] < 80 && pixel.0[1] < 80 && pixel.0[2] < 80)
            .count();

        assert_eq!(dark_shadow_pixels, 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vertical_calendar_text_uses_native_font_metrics_before_rasterizing() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Date;
        item.text = "2\n9\n\nA\nP\nR\n\n2\n0\n2\n6".to_string();
        item.point_size = 140.0;
        item.content_width = 120.0;
        item.content_height = 120.0;
        item.quad.width = 140.0;
        item.quad.height = 140.0;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind = Some(SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-vertical-calendar".to_string();

        let rasterized = rasterize_text_texture(&item, |_item, _path| (), |_item, _detail| ())
            .expect("vertical text raster");

        assert!(rasterized.image.height() <= item.quad.height.ceil() as u32);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vertical_calendar_text_does_not_shrink_against_single_line_width() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Date;
        item.text = "2\n9\n\nA\nP\nR\n\n2\n0\n2\n6".to_string();
        item.point_size = 140.0;
        item.content_width = 72.0;
        item.content_height = 3000.0;
        item.quad.width = 48.0;
        item.quad.height = 3020.0;
        item.font.authored_reference = Some("Helvetica".to_string());
        item.font.reference_kind = Some(SceneTextFontReferenceKind::FamilyLike);
        item.font.file_candidates.clear();
        item.font.family_candidates = vec!["Helvetica".to_string()];
        item.font.cache_key = "font:phase-09a-vertical-calendar-narrow".to_string();

        let rasterized = rasterize_text_texture(&item, |_item, _path| (), |_item, _detail| ())
            .expect("vertical text raster");

        assert!(rasterized.image.height() > 0);
    }

    #[test]
    fn dynamic_text_fit_scale_clamps_to_width_when_real_glyph_bounds_overflow() {
        let item = sample_text_item();
        let scale = scene_text_fit_scale(&item, 620.0, 180.0);

        assert!(scale < 1.0);
        assert!((scale - (item.content_width / 620.0)).abs() < 0.0001);
    }

    #[test]
    fn static_text_fit_scale_tracks_box_width() {
        let mut item = sample_text_item();
        item.behavior = SceneTextBehavior::Static;
        item.content_width = 300.0;
        item.content_height = 120.0;

        let scale = scene_text_fit_scale(&item, 600.0, 80.0);

        assert!((scale - 0.5).abs() < 0.0001);
    }
}
