use std::path::Path;

use crate::models::WallpaperRecord;

#[cfg(target_os = "macos")]
use std::collections::BTreeMap;

#[cfg(target_os = "macos")]
use chrono::{TimeZone, Utc};
#[cfg(target_os = "macos")]
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

#[cfg(target_os = "macos")]
use crate::{
    models::{SceneRuntimeDocument, WallpaperRuntime, WallpaperType},
    services::{
        runtime_document_service,
        scene_native_renderer_service::rasterize_scene_text_item_snapshot,
        scene_render_planner_service::{
            build_scene_render_plan_with_resolver, SceneRenderBlendMode, SceneRenderDrawKind,
            SceneRenderIssue, SceneRenderPlan, SceneRenderQuad, SceneRenderSourceKind,
        },
        scene_resource_service::{default_builtin_scene_assets_root, SceneResourceResolver},
        scene_runtime_settings_service,
    },
};

#[cfg(target_os = "macos")]
const SCENE_SNAPSHOT_WIDTH: u32 = 1920;
#[cfg(target_os = "macos")]
const SCENE_SNAPSHOT_HEIGHT: u32 = 1080;

#[cfg(target_os = "macos")]
pub fn capture_scene_static_snapshot(
    record: &WallpaperRecord,
    output_path: &Path,
) -> Result<(), String> {
    if !matches!(record.wallpaper_type, WallpaperType::Scene) {
        return Err(format!(
            "scene snapshot capture received a non-Scene record: {:?}",
            record.wallpaper_type
        ));
    }

    let runtime_record =
        runtime_document_service::runtime_record_with_context(record, scene_capture_time(), None);
    let WallpaperRuntime::Scene { scene } = runtime_record.runtime else {
        return Err("scene snapshot capture could not build a Scene runtime document".to_string());
    };
    let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
        &record.managed_path,
        default_builtin_scene_assets_root(),
        scene_runtime_settings_service::persisted_external_assets_root(),
    );
    capture_scene_runtime_snapshot_with_resolver(&scene, &resolver, output_path)
}

#[cfg(target_os = "macos")]
fn scene_capture_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .expect("fixed Scene snapshot capture time")
}

#[cfg(not(target_os = "macos"))]
pub fn capture_scene_static_snapshot(
    record: &WallpaperRecord,
    output_path: &Path,
) -> Result<(), String> {
    let _ = (record, output_path);
    Err("scene static snapshot generation is only implemented on macOS".to_string())
}

#[cfg(target_os = "macos")]
fn capture_scene_runtime_snapshot_with_resolver(
    scene: &SceneRuntimeDocument,
    resolver: &SceneResourceResolver,
    output_path: &Path,
) -> Result<(), String> {
    capture_scene_runtime_snapshot_with_size(
        scene,
        resolver,
        output_path,
        SCENE_SNAPSHOT_WIDTH,
        SCENE_SNAPSHOT_HEIGHT,
    )
}

#[cfg(target_os = "macos")]
fn capture_scene_runtime_snapshot_with_size(
    scene: &SceneRuntimeDocument,
    resolver: &SceneResourceResolver,
    output_path: &Path,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let report = build_scene_render_plan_with_resolver(scene, Some(resolver));
    if report.is_blocked() {
        return Err(format!(
            "Scene snapshot capture is blocked: {}",
            format_scene_issues(&report.fatal_errors())
        ));
    }
    capture_scene_render_plan_snapshot_with_size(&report.plan, output_path, width, height)
}

#[cfg(target_os = "macos")]
fn capture_scene_render_plan_snapshot_with_size(
    plan: &SceneRenderPlan,
    output_path: &Path,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Scene snapshot capture requires a non-zero output size".to_string());
    }

    let mut canvas = RgbaImage::from_pixel(width, height, opaque_clear_pixel(plan));
    let visual_items = plan
        .visuals
        .iter()
        .map(|item| (item.object_id, item))
        .collect::<BTreeMap<_, _>>();
    let text_items = plan
        .texts
        .iter()
        .map(|item| (item.object_id, item))
        .collect::<BTreeMap<_, _>>();
    let mut rendered_count = 0usize;
    let mut unsupported = Vec::new();

    for draw_item in &plan.draw_order {
        match draw_item.kind {
            SceneRenderDrawKind::Visual => {
                let Some(item) = visual_items.get(&draw_item.object_id) else {
                    continue;
                };
                let image = load_scene_snapshot_image(&item.texture_path, item.source_kind)?;
                if draw_projected_image(
                    &mut canvas,
                    &image,
                    item.quad,
                    plan,
                    width,
                    height,
                    item.blend_mode,
                ) {
                    rendered_count += 1;
                }
            }
            SceneRenderDrawKind::Text => {
                let Some(item) = text_items.get(&draw_item.object_id) else {
                    continue;
                };
                let image = rasterize_scene_text_item_snapshot(item).map_err(|error| {
                    format!(
                        "Scene text {} could not be rasterized for snapshot capture: {error}",
                        item.object_name
                    )
                })?;
                if draw_projected_image(
                    &mut canvas,
                    &image.to_rgba8(),
                    item.quad,
                    plan,
                    width,
                    height,
                    SceneRenderBlendMode::Normal,
                ) {
                    rendered_count += 1;
                }
            }
            SceneRenderDrawKind::Audio => unsupported.push(format!(
                "Scene audio item {} is dynamic and is not captured as a still",
                draw_item.object_id
            )),
            SceneRenderDrawKind::Particle => unsupported.push(format!(
                "Scene particle item {} is dynamic and is not captured as a still",
                draw_item.object_id
            )),
            SceneRenderDrawKind::RopeParticle => unsupported.push(format!(
                "Scene rope particle item {} is dynamic and is not captured as a still",
                draw_item.object_id
            )),
            SceneRenderDrawKind::SpriteParticle => unsupported.push(format!(
                "Scene sprite particle item {} is dynamic and is not captured as a still",
                draw_item.object_id
            )),
            SceneRenderDrawKind::Sound => unsupported.push(format!(
                "Scene sound item {} has no visual still output",
                draw_item.object_id
            )),
        }
    }

    if rendered_count == 0 {
        let detail = if unsupported.is_empty() {
            "no supported visual or text items were draw-ready".to_string()
        } else {
            unsupported.join("; ")
        };
        return Err(format!(
            "Scene snapshot capture produced no still output: {detail}"
        ));
    }

    DynamicImage::ImageRgba8(canvas)
        .save_with_format(output_path, ImageFormat::Png)
        .map_err(|error| {
            format!(
                "failed to write Scene static snapshot to {}: {error}",
                output_path.display()
            )
        })
}

#[cfg(target_os = "macos")]
fn load_scene_snapshot_image(
    path: &Path,
    source_kind: SceneRenderSourceKind,
) -> Result<RgbaImage, String> {
    match source_kind {
        SceneRenderSourceKind::Image => crate::services::scene_resource_service::load_scene_texture_image(path)
            .map_err(|error| {
                format!(
                    "Scene snapshot capture could not decode visual texture {}: {error}",
                    path.display()
                )
            })
            .map(|image| image.to_rgba8()),
        SceneRenderSourceKind::Video => render_scene_video_frame_image(path),
    }
}

#[cfg(target_os = "macos")]
fn render_scene_video_frame_image(source_path: &Path) -> Result<RgbaImage, String> {
    use std::ptr;

    use objc2::{runtime::AnyObject, AnyThread};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey};
    use objc2_av_foundation::{AVAsset, AVAssetImageGenerator};
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSDictionary, NSURL};

    let source = source_path.to_str().ok_or_else(|| {
        format!(
            "Scene video source path is not valid UTF-8 for snapshot capture: {}",
            source_path.display()
        )
    })?;
    let url = NSURL::from_file_path(source)
        .ok_or_else(|| format!("AVFoundation rejected Scene video source path: {source}"))?;
    let asset = unsafe { AVAsset::assetWithURL(&url) };
    let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    unsafe {
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(CMTime::new(0, 600));
        generator.setRequestedTimeToleranceAfter(CMTime::new(0, 600));
    }

    #[allow(deprecated)]
    let image = unsafe {
        generator.copyCGImageAtTime_actualTime_error(CMTime::new(0, 600), ptr::null_mut())
    }
    .map_err(|error| {
        format!(
            "Scene snapshot capture could not extract initial video frame from {}: {}",
            source_path.display(),
            error.localizedDescription()
        )
    })?;

    let bitmap = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, AnyObject>::new();
    let data = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }
    .ok_or_else(|| {
        format!(
            "Scene snapshot capture could not encode initial video frame as PNG for {}",
            source_path.display()
        )
    })?;
    image::load_from_memory(&data.to_vec())
        .map_err(|error| {
            format!(
                "Scene snapshot capture could not decode extracted video frame for {}: {error}",
                source_path.display()
            )
        })
        .map(|image| image.to_rgba8())
}

#[cfg(target_os = "macos")]
fn opaque_clear_pixel(plan: &SceneRenderPlan) -> Rgba<u8> {
    let alpha = plan.clear_color.alpha as f32 / 255.0;
    Rgba([
        ((plan.clear_color.red as f32) * alpha).round() as u8,
        ((plan.clear_color.green as f32) * alpha).round() as u8,
        ((plan.clear_color.blue as f32) * alpha).round() as u8,
        255,
    ])
}

#[cfg(target_os = "macos")]
fn draw_projected_image(
    canvas: &mut RgbaImage,
    source: &RgbaImage,
    quad: SceneRenderQuad,
    plan: &SceneRenderPlan,
    view_width: u32,
    view_height: u32,
    blend_mode: SceneRenderBlendMode,
) -> bool {
    if source.width() == 0 || source.height() == 0 || quad.width <= 0.0 || quad.height <= 0.0 {
        return false;
    }

    let projection = ProjectedQuad::new(quad, plan, view_width, view_height);
    let Some(bounds) = projection.pixel_bounds(view_width, view_height) else {
        return false;
    };
    let mut drew_pixel = false;

    for y in bounds.min_y..=bounds.max_y {
        for x in bounds.min_x..=bounds.max_x {
            let Some((u, v)) = projection.texture_coordinates(x as f64 + 0.5, y as f64 + 0.5)
            else {
                continue;
            };
            let source_x = ((source.width() - 1) as f64 * u.clamp(0.0, 1.0)).round() as u32;
            let source_y = ((source.height() - 1) as f64 * v.clamp(0.0, 1.0)).round() as u32;
            let source_pixel = *source.get_pixel(source_x, source_y);
            if source_pixel[3] == 0 {
                continue;
            }
            blend_pixel(
                canvas.get_pixel_mut(x, y),
                source_pixel,
                projection.opacity,
                blend_mode,
            );
            drew_pixel = true;
        }
    }

    drew_pixel
}

#[cfg(target_os = "macos")]
fn blend_pixel(
    destination: &mut Rgba<u8>,
    source: Rgba<u8>,
    opacity: f64,
    blend_mode: SceneRenderBlendMode,
) {
    let source_alpha = (source[3] as f32 / 255.0) * opacity.clamp(0.0, 1.0) as f32;
    if source_alpha <= 0.0 {
        return;
    }

    for index in 0..3 {
        let src = source[index] as f32;
        let dst = destination[index] as f32;
        let value = match blend_mode {
            SceneRenderBlendMode::Normal => src * source_alpha + dst * (1.0 - source_alpha),
            SceneRenderBlendMode::Additive => dst + src * source_alpha,
            SceneRenderBlendMode::Multiply => {
                dst * (1.0 - source_alpha) + (dst * src / 255.0) * source_alpha
            }
        };
        destination[index] = value.round().clamp(0.0, 255.0) as u8;
    }
    destination[3] = 255;
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct ProjectedQuad {
    center_x: f64,
    center_y: f64,
    width: f64,
    height: f64,
    rotation: f64,
    opacity: f64,
    flip_x: bool,
    flip_y: bool,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy)]
struct PixelBounds {
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
}

#[cfg(target_os = "macos")]
impl ProjectedQuad {
    fn new(
        quad: SceneRenderQuad,
        plan: &SceneRenderPlan,
        view_width: u32,
        view_height: u32,
    ) -> Self {
        let view_width = view_width as f64;
        let view_height = view_height as f64;
        let cover_scale = (view_width / plan.canvas_width)
            .max(view_height / plan.canvas_height)
            .max(0.001);
        let camera_scale = cover_scale * plan.camera.zoom.max(0.001);
        let origin_x = (view_width - plan.canvas_width * camera_scale) / 2.0;
        let origin_y = (view_height - plan.canvas_height * camera_scale) / 2.0;

        Self {
            center_x: origin_x + (quad.left + quad.width / 2.0) * camera_scale,
            center_y: origin_y + (quad.top + quad.height / 2.0) * camera_scale,
            width: quad.width * camera_scale,
            height: quad.height * camera_scale,
            rotation: quad.rotation,
            opacity: quad.opacity,
            flip_x: quad.flip_x,
            flip_y: quad.flip_y,
        }
    }

    fn pixel_bounds(self, view_width: u32, view_height: u32) -> Option<PixelBounds> {
        let half_width = self.width / 2.0;
        let half_height = self.height / 2.0;
        let (sin, cos) = self.rotation.sin_cos();
        let corners = [
            (-half_width, -half_height),
            (half_width, -half_height),
            (-half_width, half_height),
            (half_width, half_height),
        ]
        .map(|(x, y)| {
            (
                self.center_x + x * cos - y * sin,
                self.center_y + x * sin + y * cos,
            )
        });
        let min_x = corners
            .iter()
            .map(|point| point.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let min_y = corners
            .iter()
            .map(|point| point.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.0) as u32;
        let max_x = corners
            .iter()
            .map(|point| point.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(view_width.saturating_sub(1) as f64) as u32;
        let max_y = corners
            .iter()
            .map(|point| point.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .min(view_height.saturating_sub(1) as f64) as u32;

        (min_x <= max_x && min_y <= max_y).then_some(PixelBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    fn texture_coordinates(self, x: f64, y: f64) -> Option<(f64, f64)> {
        let dx = x - self.center_x;
        let dy = y - self.center_y;
        let (sin, cos) = self.rotation.sin_cos();
        let local_x = dx * cos + dy * sin;
        let local_y = -dx * sin + dy * cos;
        let mut u = local_x / self.width + 0.5;
        let mut v = local_y / self.height + 0.5;
        if self.flip_x {
            u = 1.0 - u;
        }
        if self.flip_y {
            v = 1.0 - v;
        }
        (u >= 0.0 && u <= 1.0 && v >= 0.0 && v <= 1.0).then_some((u, v))
    }
}

#[cfg(target_os = "macos")]
fn format_scene_issues(issues: &[SceneRenderIssue]) -> String {
    if issues.is_empty() {
        return "no detailed Scene render issue was reported".to_string();
    }
    issues
        .iter()
        .take(4)
        .map(|issue| issue.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    use chrono::Utc;
    use image::{Rgba, RgbaImage};
    use tempfile::tempdir;

    use crate::{
        models::{
            EvaluatedSceneCamera, EvaluatedSceneObject, EvaluatedSceneObjectBase,
            EvaluatedSceneTransform, EvaluatedTextLayout, EvaluatedTextState, EvaluatedTextStyle,
            SceneAssetKind, SceneEvaluatedDocument, SceneManifest, SceneRuntimeDocument,
            SceneTextBehavior,
        },
        services::{
            scene_render_planner_service::SceneRenderQuad,
            scene_resource_service::{default_builtin_scene_assets_root, SceneResourceResolver},
        },
    };

    use super::{capture_scene_runtime_snapshot_with_size, ProjectedQuad};

    fn write_solid_png(path: &Path, color: [u8; 4]) {
        let mut image = RgbaImage::new(8, 8);
        for pixel in image.pixels_mut() {
            *pixel = Rgba(color);
        }
        image.save(path).expect("write png");
    }

    fn object_base(id: u32, name: &str, bounds: [f64; 4]) -> EvaluatedSceneObjectBase {
        EvaluatedSceneObjectBase {
            id,
            name: name.to_string(),
            parent_id: None,
            dependencies: Vec::new(),
            visible: true,
            alignment: None,
            opacity: 1.0,
            transform: EvaluatedSceneTransform {
                position: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                rotation: 0.0,
                render_bounds: Some(bounds),
            },
        }
    }

    fn image_object(id: u32, name: &str, bounds: [f64; 4], path: &Path) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Visual {
            base: object_base(id, name, bounds),
            asset_kind: SceneAssetKind::Image,
            asset_path: Some(path.display().to_string()),
            system_texture_key: None,
            texture_names: Vec::new(),
            blend_mode: None,
            color: None,
            brightness: None,
            color_blend_mode: None,
            parallax_depth: None,
            angles: None,
            fullscreen: false,
            autosize: false,
            solid_layer: false,
            passthrough: false,
            no_padding: false,
            puppet_path: None,
            animation_layers: Vec::new(),
            primary: false,
            background_candidate: false,
        }
    }

    fn text_object(id: u32, bounds: [f64; 4]) -> EvaluatedSceneObject {
        EvaluatedSceneObject::Text {
            base: object_base(id, "Label", bounds),
            behavior: SceneTextBehavior::Static,
            text: EvaluatedTextState {
                value: "████".to_string(),
                style: EvaluatedTextStyle {
                    color: Some("1 1 1".to_string()),
                    alpha: 1.0,
                    point_size: 34.0,
                    font_path: None,
                    effect_paths: Vec::new(),
                    horizontal_align: Some("left".to_string()),
                    vertical_align: Some("top".to_string()),
                    padding: None,
                    max_rows: Some(1),
                    max_width: None,
                    limit_width: Some(false),
                    limit_use_ellipsis: Some(false),
                    block_align: None,
                },
                layout: EvaluatedTextLayout {
                    size: Some([70.0, 40.0]),
                    render_bounds: Some(bounds),
                    content_bounds: Some(bounds),
                    scaled_point_size: 34.0,
                    scaled_padding: 0.0,
                    world_scale: [1.0, 1.0, 1.0],
                },
                dynamic_input_generation: None,
            },
        }
    }

    fn runtime_scene(
        objects: Vec<(u32, EvaluatedSceneObject)>,
        render_list: Vec<u32>,
    ) -> SceneRuntimeDocument {
        SceneRuntimeDocument {
            runtime_owner_key: None,
            source: SceneManifest::default(),
            evaluated: SceneEvaluatedDocument {
                canvas_width: 200.0,
                canvas_height: 120.0,
                clear_color: Some("0 0 0 1".to_string()),
                camera: EvaluatedSceneCamera {
                    zoom: 1.0,
                    center: [0.0, 0.0],
                    camera_shake: false,
                    camera_shake_amplitude: 0.0,
                    camera_shake_speed: 0.0,
                    parallax_mouse_influence: 0.0,
                },
                parallax: Default::default(),
                objects: objects.into_iter().collect::<BTreeMap<_, _>>(),
                render_list,
                evaluated_at: Utc::now(),
                diagnostics: Vec::new(),
            },
            now_playing: Default::default(),
        }
    }

    #[test]
    fn captures_scene_visual_text_positions_and_draw_order() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let red = managed.join("red.png");
        let blue = managed.join("blue.png");
        write_solid_png(&red, [255, 0, 0, 255]);
        write_solid_png(&blue, [0, 0, 255, 255]);
        let output = managed.join(".snapshot.png.tmp");
        let scene = runtime_scene(
            vec![
                (1, image_object(1, "Red", [10.0, 10.0, 80.0, 80.0], &red)),
                (2, image_object(2, "Blue", [40.0, 40.0, 80.0, 80.0], &blue)),
                (3, text_object(3, [110.0, 20.0, 70.0, 40.0])),
            ],
            vec![1, 2, 3],
        );
        let resolver = SceneResourceResolver::for_managed_root_with_builtin_root(
            &managed,
            default_builtin_scene_assets_root(),
        );

        capture_scene_runtime_snapshot_with_size(&scene, &resolver, &output, 200, 120)
            .expect("capture");

        let snapshot = image::load_from_memory(&fs::read(&output).expect("snapshot bytes"))
            .expect("snapshot")
            .to_rgba8();
        assert_eq!(snapshot.get_pixel(15, 15).0, [255, 0, 0, 255]);
        assert_eq!(snapshot.get_pixel(45, 45).0, [0, 0, 255, 255]);
        assert!((110..180).any(|x| {
            (20..60).any(|y| {
                let pixel = snapshot.get_pixel(x, y);
                pixel[0] > 180 && pixel[1] > 180 && pixel[2] > 180
            })
        }));
    }

    #[test]
    fn snapshot_capture_decodes_tex_visuals_via_shared_scene_texture_loader() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let tex = managed.join("visual.tex");
        fs::write(
            &tex,
            {
                let mut bytes = Vec::new();
                bytes.extend_from_slice(b"TEXV0005\0");
                bytes.extend_from_slice(b"TEXI0001\0");
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(b"TEXB0004\0");
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&u32::MAX.to_le_bytes());
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&1_u32.to_le_bytes());
                bytes.extend_from_slice(&0_u32.to_le_bytes());
                bytes.extend_from_slice(&0_i32.to_le_bytes());
                bytes.extend_from_slice(&4_i32.to_le_bytes());
                bytes.extend_from_slice(&[255, 0, 0, 255]);
                bytes
            },
        )
        .expect("write tex");
        let output = managed.join(".snapshot-tex.png.tmp");
        let scene = runtime_scene(
            vec![(1, image_object(1, "RedTex", [0.0, 0.0, 200.0, 120.0], &tex))],
            vec![1],
        );
        let resolver = SceneResourceResolver::for_managed_root_with_builtin_root(
            &managed,
            default_builtin_scene_assets_root(),
        );

        capture_scene_runtime_snapshot_with_size(&scene, &resolver, &output, 200, 120)
            .expect("capture");

        let snapshot = image::load_from_memory(&fs::read(&output).expect("snapshot bytes"))
            .expect("snapshot")
            .to_rgba8();
        assert_eq!(snapshot.get_pixel(100, 60).0, [255, 0, 0, 255]);
    }

    #[test]
    fn captures_scene_with_cover_projection_for_mismatched_canvas_aspect_ratio() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let red = managed.join("red.png");
        write_solid_png(&red, [255, 0, 0, 255]);
        let output = managed.join(".snapshot-cover.png.tmp");
        let scene = SceneRuntimeDocument {
            runtime_owner_key: None,
            source: SceneManifest::default(),
            evaluated: crate::models::SceneEvaluatedDocument {
                canvas_width: 4000.0,
                canvas_height: 2336.0,
                clear_color: Some("0 0 0 1".to_string()),
                camera: EvaluatedSceneCamera {
                    zoom: 1.0,
                    center: [0.0, 0.0],
                    camera_shake: false,
                    camera_shake_amplitude: 0.0,
                    camera_shake_speed: 0.0,
                    parallax_mouse_influence: 0.0,
                },
                parallax: Default::default(),
                objects: BTreeMap::from([(
                    1,
                    image_object(1, "Cover", [0.0, 0.0, 4000.0, 2336.0], &red),
                )]),
                render_list: vec![1],
                evaluated_at: Utc::now(),
                diagnostics: Vec::new(),
            },
            now_playing: Default::default(),
        };
        let resolver = SceneResourceResolver::for_managed_root_with_builtin_root(
            &managed,
            default_builtin_scene_assets_root(),
        );

        capture_scene_runtime_snapshot_with_size(&scene, &resolver, &output, 200, 120)
            .expect("capture");

        let snapshot = image::load_from_memory(&fs::read(&output).expect("snapshot bytes"))
            .expect("snapshot")
            .to_rgba8();
        assert_eq!(snapshot.get_pixel(0, 60).0, [255, 0, 0, 255]);
        assert_eq!(snapshot.get_pixel(199, 60).0, [255, 0, 0, 255]);
    }

    #[test]
    fn unsupported_scene_content_fails_without_silent_success() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        fs::create_dir_all(&managed).expect("managed dir");
        let video = managed.join("clip.mp4");
        fs::write(&video, b"video").expect("video");
        let output = managed.join("snapshot.png");
        let scene = runtime_scene(
            vec![(
                7,
                EvaluatedSceneObject::Visual {
                    base: object_base(7, "Video", [0.0, 0.0, 200.0, 120.0]),
                    asset_kind: SceneAssetKind::Video,
                    asset_path: Some(video.display().to_string()),
                    system_texture_key: None,
                    texture_names: Vec::new(),
                    blend_mode: None,
                    color: None,
                    brightness: None,
                    color_blend_mode: None,
                    parallax_depth: None,
                    angles: None,
                    fullscreen: false,
                    autosize: false,
                    solid_layer: false,
                    passthrough: false,
                    no_padding: false,
                    puppet_path: None,
                    animation_layers: Vec::new(),
                    primary: false,
                    background_candidate: false,
                },
            )],
            vec![7],
        );
        let resolver = SceneResourceResolver::for_managed_root_with_builtin_root(
            &managed,
            default_builtin_scene_assets_root(),
        );

        let error = capture_scene_runtime_snapshot_with_size(&scene, &resolver, &output, 200, 120)
            .expect_err("invalid Scene video snapshot should fail");

        assert!(error.contains("Scene snapshot capture could not extract initial video frame"));
        assert!(!output.exists());
    }

    #[test]
    fn projected_quad_maps_rotated_pixels_inside_bounds() {
        let mut quad = SceneRenderQuad {
            left: 50.0,
            top: 20.0,
            width: 60.0,
            height: 30.0,
            rotation: 0.35,
            opacity: 1.0,
            flip_x: false,
            flip_y: false,
        };
        let scene = runtime_scene(Vec::new(), Vec::new());
        let report = crate::services::scene_render_planner_service::build_scene_render_plan(&scene);
        let projected = ProjectedQuad::new(quad, &report.plan, 200, 120);

        assert!(projected
            .texture_coordinates(projected.center_x, projected.center_y)
            .is_some());
        quad.rotation = 0.0;
        let unrotated = ProjectedQuad::new(quad, &report.plan, 200, 120);
        assert!(unrotated.texture_coordinates(50.0, 20.0).is_some());
    }
}
