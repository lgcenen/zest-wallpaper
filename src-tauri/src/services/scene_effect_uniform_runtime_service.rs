use super::*;
use super::scene_effect_input_runtime_service::{
    phase10_optional_texel_size, phase10_optional_texture_resolution, phase10_texel_size,
    phase10_texture_resolution, Phase10PassTextures,
};
use super::scene_effect_runtime_service::{
    phase10_effect_family_from_program, phase10_effect_uniform_values,
    phase10_perspective_corner_uniforms, phase10_skew_controls, phase10_spin_controls,
    phase10_transform_controls, phase10_uniform_color, phase10_uniform_float,
    phase10_uniform_vec2, phase10_uniform_vec4, rotate2d,
    Phase10ResolvedPass,
};

#[cfg(target_os = "macos")]
pub(super) fn build_phase10_effect_uniforms_for_pass(
    resolved_pass: &Phase10ResolvedPass<'_>,
    pass_textures: &Phase10PassTextures,
    width: usize,
    height: usize,
    elapsed_seconds: f64,
) -> Phase10EffectUniforms {
    let effect_kind = phase10_effect_family_from_program(&resolved_pass.pass.program);
    let mut uniforms = Phase10EffectUniforms {
        color: [1.0, 1.0, 1.0, 1.0],
        user0: [0.0, 0.0, 0.0, 0.0],
        user1: [0.0, 0.0, 0.0, 0.0],
        primary_resolution: phase10_texture_resolution(
            pass_textures.slots.first().and_then(|slot| slot.as_ref()),
        ),
        slot1_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
        ),
        slot2_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
        ),
        slot3_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
        ),
        slot4_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(4).and_then(|slot| slot.as_ref()),
        ),
        slot5_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(5).and_then(|slot| slot.as_ref()),
        ),
        slot6_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(6).and_then(|slot| slot.as_ref()),
        ),
        slot7_resolution: phase10_optional_texture_resolution(
            pass_textures.slots.get(7).and_then(|slot| slot.as_ref()),
        ),
        texel_size: phase10_texel_size(pass_textures.slots.first().and_then(|slot| slot.as_ref())),
        aux_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(1).and_then(|slot| slot.as_ref()),
        ),
        aux2_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(2).and_then(|slot| slot.as_ref()),
        ),
        aux3_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(3).and_then(|slot| slot.as_ref()),
        ),
        aux4_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(4).and_then(|slot| slot.as_ref()),
        ),
        aux5_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(5).and_then(|slot| slot.as_ref()),
        ),
        aux6_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(6).and_then(|slot| slot.as_ref()),
        ),
        aux7_texel_size: phase10_optional_texel_size(
            pass_textures.slots.get(7).and_then(|slot| slot.as_ref()),
        ),
        screen_size: [width.max(1) as f32, height.max(1) as f32],
        time: elapsed_seconds as f32,
        intensity: 1.0,
        speed: 1.0,
        radius: 1.0,
        angle: 0.0,
    };
    let uniform_values = phase10_effect_uniform_values(resolved_pass);

    match effect_kind {
        Some(SceneCompatEffectKind::Pulse) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["amount", "pulseamount"], 1.0);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed", "pulsespeed"], 3.0);
            uniforms.user0[0] =
                phase10_uniform_float(&uniform_values, &["phase", "pulsephase"], 0.0);
            uniforms.user0[1] =
                phase10_uniform_float(&uniform_values, &["power"], 1.0).max(0.001);
            let bounds =
                phase10_uniform_vec2(&uniform_values, &["bounds", "pulsethresholds"], [0.0, 1.0]);
            uniforms.user0[2] = bounds[0];
            uniforms.user0[3] = bounds[1];
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["noisespeed"], 0.5).max(0.0);
            uniforms.angle = phase10_uniform_float(&uniform_values, &["noiseamount"], 0.0);
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["tintlow", "tintcolor1"],
                [1.0, 1.0, 1.0, 1.0],
            );
            let tint_high = phase10_uniform_color(
                &uniform_values,
                &["tinthigh", "tintcolor2"],
                [1.0, 1.0, 1.0, 1.0],
            );
            uniforms.user1 = [tint_high[0], tint_high[1], tint_high[2], 1.0];
        }
        Some(SceneCompatEffectKind::Shake) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["strength", "amp"], 0.1);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
            let bounds =
                phase10_uniform_vec2(&uniform_values, &["bounds", "gbounds"], [0.0, 1.0]);
            let friction =
                phase10_uniform_vec2(&uniform_values, &["friction", "gfriction"], [1.0, 1.0]);
            uniforms.user0[0] = bounds[0];
            uniforms.user0[1] = bounds[1];
            uniforms.user1[0] = friction[0];
            uniforms.user1[1] = friction[1];
        }
        Some(SceneCompatEffectKind::WaterRipple) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["ripplestrength", "strength"], 0.1);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["animationspeed"], 0.15);
            uniforms.radius = phase10_uniform_float(&uniform_values, &["scale"], 1.0);
            uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["scrollspeed"], 0.0);
            uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["ratio"], 1.0);
            uniforms.angle =
                phase10_uniform_float(&uniform_values, &["scrolldirection", "direction"], 0.0);
        }
        Some(SceneCompatEffectKind::WaterWaves) => {
            uniforms.intensity = phase10_uniform_float(&uniform_values, &["strength"], 0.1);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 5.0);
            uniforms.angle = phase10_uniform_float(&uniform_values, &["direction"], 0.0);
            let direction = rotate2d([0.0, 1.0], uniforms.angle);
            uniforms.user0[0] = direction[0];
            uniforms.user0[1] = direction[1];
            uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["scale"], 200.0);
            uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["exponent"], 1.0);
        }
        Some(SceneCompatEffectKind::Tint) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "blendalpha"], 1.0);
            uniforms.color =
                phase10_uniform_color(&uniform_values, &["color", "tintcolor"], [1.0, 0.0, 0.0, 1.0]);
        }
        Some(SceneCompatEffectKind::Scroll) => {
            uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["speedx"], 0.2);
            uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["speedy"], 0.2);
            let repeat = phase10_uniform_vec2(&uniform_values, &["repeat", "scale"], [1.0, 1.0]);
            uniforms.user0[2] = repeat[0];
            uniforms.user0[3] = repeat[1];
        }
        Some(SceneCompatEffectKind::LightShafts) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["colorwintensity", "intensity"], 1.0);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["rayspeed", "speed"], 0.2);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["rayradius", "radius"], 0.5);
            uniforms.user0 =
                phase10_uniform_vec4(&uniform_values, &["rayscale", "scale"], [0.5, 0.1, 0.0, 0.0]);
            let feather =
                phase10_uniform_vec2(&uniform_values, &["rayfeather", "feather"], [0.05, 0.2]);
            uniforms.user0[2] = feather[0];
            uniforms.user0[3] = feather[1];
            uniforms.user1[0] =
                phase10_uniform_float(&uniform_values, &["raysmoothness", "smoothness"], 0.75);
            uniforms.user1[1] =
                phase10_uniform_float(&uniform_values, &["noiseamount", "noise"], 0.33);
            uniforms.user1[2] = phase10_uniform_float(&uniform_values, &["noisescale"], 1.0);
            uniforms.user1[3] =
                phase10_uniform_float(&uniform_values, &["colorwexponent", "exponent"], 0.5);
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["colorastart", "colorstart"],
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        Some(SceneCompatEffectKind::FoliageSway) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["strength"], 33.34) / 100.0;
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 3.0);
            uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["phase"], 0.0);
            uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["power"], 1.0);
            uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["mode"], 0.0);
        }
        Some(SceneCompatEffectKind::Circle) => {}
        Some(SceneCompatEffectKind::Opacity) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "useralpha"], 1.0);
        }
        Some(SceneCompatEffectKind::Transform) => {
            (uniforms.user0, uniforms.user1, uniforms.angle) =
                phase10_transform_controls(&uniform_values);
        }
        Some(SceneCompatEffectKind::Skew) => {
            uniforms.user0 = phase10_skew_controls(&uniform_values);
        }
        Some(SceneCompatEffectKind::Perspective) => {
            (uniforms.user0, uniforms.user1) =
                phase10_perspective_corner_uniforms(&uniform_values);
        }
        Some(SceneCompatEffectKind::Spin) => {
            (uniforms.angle, uniforms.speed, uniforms.user0) =
                phase10_spin_controls(&uniform_values);
        }
        Some(SceneCompatEffectKind::Swing) => {
            uniforms.intensity = phase10_uniform_float(&uniform_values, &["amount"], 0.2);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
            uniforms.radius = phase10_uniform_float(&uniform_values, &["size"], 0.4);
            uniforms.angle = phase10_uniform_float(&uniform_values, &["center"], 0.5);
            uniforms.user0 =
                phase10_uniform_vec4(&uniform_values, &["point0"], [0.25, 0.5, 0.75, 0.5]);
            let point1 = phase10_uniform_vec2(&uniform_values, &["point1"], [0.75, 0.5]);
            uniforms.user0[2] = point1[0];
            uniforms.user0[3] = point1[1];
            uniforms.user1[0] = phase10_uniform_float(&uniform_values, &["feather"], 0.01);
            uniforms.user1[1] = phase10_uniform_float(&uniform_values, &["noisespeed"], 0.15);
            uniforms.user1[2] = phase10_uniform_float(&uniform_values, &["noiseamount"], 0.2);
        }
        Some(SceneCompatEffectKind::Twirl) => {
            uniforms.intensity = phase10_uniform_float(&uniform_values, &["amount"], 0.2);
            uniforms.speed = phase10_uniform_float(&uniform_values, &["speed"], 1.0);
            uniforms.radius = phase10_uniform_float(&uniform_values, &["size"], 0.5);
            uniforms.angle = phase10_uniform_float(&uniform_values, &["angle"], 0.0);
            uniforms.user0 =
                phase10_uniform_vec4(&uniform_values, &["center"], [0.5, 0.5, 1.0, 0.002]);
            uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["ratio"], 1.0);
            uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["feather"], 0.002);
            uniforms.user1[0] = phase10_uniform_float(&uniform_values, &["noisespeed"], 0.15);
            uniforms.user1[1] = phase10_uniform_float(&uniform_values, &["noiseamount"], 0.5);
        }
        Some(SceneCompatEffectKind::ChromaticAberration) => {
            uniforms.user0 = phase10_uniform_vec4(
                &uniform_values,
                &[
                    "center",
                    "uieditorpropertiescenter",
                    "ui_editor_properties_center",
                ],
                [0.5, 0.5, 0.5, 9.0],
            );
            uniforms.user0[2] = phase10_uniform_float(
                &uniform_values,
                &[
                    "centerfalloff",
                    "uieditorpropertiescenterfalloff",
                    "ui_editor_properties_center_falloff",
                ],
                uniforms.user0[2],
            );
            uniforms.user0[3] = phase10_uniform_float(
                &uniform_values,
                &[
                    "strength",
                    "uieditorpropertiesstrength",
                    "ui_editor_properties_strength",
                ],
                uniforms.user0[3],
            );
            uniforms.angle = phase10_uniform_float(
                &uniform_values,
                &[
                    "direction",
                    "uieditorpropertiesdirection",
                    "ui_editor_properties_direction",
                ],
                1.5707964,
            );
        }
        Some(SceneCompatEffectKind::ColorKey) => {
            uniforms.intensity = phase10_uniform_float(&uniform_values, &["alpha"], 0.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["fuzziness", "keyfuzz"], 0.0);
            uniforms.angle =
                phase10_uniform_float(&uniform_values, &["tolerance", "keytolerance"], 0.1);
            uniforms.color =
                phase10_uniform_color(&uniform_values, &["color", "keycolor"], [1.0, 1.0, 1.0, 1.0]);
        }
        Some(SceneCompatEffectKind::FishEye) => {
            uniforms.user0 = phase10_uniform_vec4(&uniform_values, &["center"], [0.5, 0.5, 1.0, 1.0]);
            uniforms.user0[2] =
                phase10_uniform_float(&uniform_values, &["size"], uniforms.user0[2]);
            uniforms.user0[3] = phase10_uniform_float(
                &uniform_values,
                &["distortion", "scale"],
                uniforms.user0[3],
            );
        }
        Some(SceneCompatEffectKind::EdgeDetection) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "blendalpha"], 1.0);
            uniforms.speed =
                phase10_uniform_float(&uniform_values, &["brightness", "blendbrightness"], 1.0);
            uniforms.radius = phase10_uniform_float(
                &uniform_values,
                &["detectthreshold", "detectionthreshold"],
                0.5,
            );
            uniforms.angle = phase10_uniform_float(
                &uniform_values,
                &["detectmultiply", "detectionmultiply"],
                1.0,
            );
            let color1 =
                phase10_uniform_color(&uniform_values, &["outlinecolor"], [0.0, 0.0, 0.0, 1.0]);
            let color2 = phase10_uniform_color(
                &uniform_values,
                &["outlinebackground"],
                [1.0, 1.0, 1.0, 1.0],
            );
            uniforms.color = color1;
            uniforms.user0 = [color2[0], color2[1], color2[2], 1.0];
        }
        Some(SceneCompatEffectKind::Iris) => {
            uniforms.user0 = phase10_uniform_vec4(&uniform_values, &["scale"], [20.0, 20.0, 0.0, 0.0]);
            uniforms.color =
                phase10_uniform_color(&uniform_values, &["color", "eyecolor"], [1.0, 1.0, 1.0, 1.0]);
        }
        Some(SceneCompatEffectKind::CloudMotion) => {
            uniforms.intensity = phase10_uniform_float(
                &uniform_values,
                &[
                    "amount",
                    "uieditorpropertiesamount",
                    "ui_editor_properties_amount",
                ],
                0.1,
            );
            uniforms.angle = phase10_uniform_float(
                &uniform_values,
                &[
                    "direction",
                    "uieditorpropertiesdirection",
                    "ui_editor_properties_direction",
                ],
                1.5707964,
            );
        }
        Some(SceneCompatEffectKind::Clouds) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "cloudsalpha"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["threshold", "cloudthreshold"], 0.0);
            uniforms.user0[0] =
                phase10_uniform_float(&uniform_values, &["feather", "cloudfeather"], 0.5);
            uniforms.user0[1] =
                phase10_uniform_float(&uniform_values, &["smoothness", "cloudlod"], 0.0);
            let speed = phase10_uniform_vec4(
                &uniform_values,
                &["speed", "cloudspeeds"],
                [0.01, 0.01, -0.02, -0.02],
            );
            let scale = phase10_uniform_vec4(
                &uniform_values,
                &["scale", "cloudscales"],
                [1.3, 1.3, 0.5, 0.5],
            );
            uniforms.user1 = [speed[0], speed[1], scale[0], scale[2]];
            uniforms.color =
                phase10_uniform_color(&uniform_values, &["colorstart", "color1"], [1.0, 1.0, 1.0, 1.0]);
            let color2 =
                phase10_uniform_color(&uniform_values, &["colorend", "color2"], [1.0, 1.0, 1.0, 1.0]);
            uniforms.user0[2] = color2[0];
            uniforms.user0[3] = color2[1];
        }
        Some(SceneCompatEffectKind::WaterFlow) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["strength", "flowamp"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["phasescale", "flowphasescale"], 2.0);
        }
        Some(SceneCompatEffectKind::Nitro) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["multiply", "nitroalpha"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["smoothness", "nitrolod"], 1.0);
            let bounds =
                phase10_uniform_vec2(&uniform_values, &["bounds", "nitroranges"], [0.3, 0.25]);
            uniforms.user0[0] = bounds[0];
            uniforms.user0[1] = bounds[1];
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["colorstart", "nitrocolor0"],
                [0.0, 0.5, 1.0, 1.0],
            );
            let color1 = phase10_uniform_color(
                &uniform_values,
                &["colorend", "nitrocolor1"],
                [1.0, 1.0, 1.0, 1.0],
            );
            uniforms.user0[2] = color1[0];
            uniforms.user0[3] = color1[1];
            uniforms.user1[0] = color1[2];
        }
        Some(SceneCompatEffectKind::Blend) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["multiply"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["alpha", "alphamultiply"], 1.0);
        }
        Some(SceneCompatEffectKind::DepthParallax) => {
            uniforms.user0 = phase10_uniform_vec4(&uniform_values, &["scale"], [1.0, 1.0, 0.3, 0.0]);
            uniforms.user0[2] =
                phase10_uniform_float(&uniform_values, &["center"], uniforms.user0[2]);
            uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["sens"], 1.0);
        }
        Some(SceneCompatEffectKind::Reflection) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "reflectionalpha"], 1.0);
        }
        Some(SceneCompatEffectKind::Shimmer) => {
            uniforms.intensity = phase10_uniform_float(
                &uniform_values,
                &[
                    "uieditorpropertiesamount",
                    "uieditorpropertiesbrightness",
                    "ui_editor_properties_brightness",
                ],
                1.0,
            );
            uniforms.speed = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertiesspeed", "ui_editor_properties_speed"],
                1.0,
            );
            uniforms.radius = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertiesdelay", "ui_editor_properties_delay"],
                2.0,
            );
            uniforms.angle = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertiesdirection", "ui_editor_properties_direction"],
                1.5707964,
            );
            uniforms.user0 = phase10_uniform_vec4(
                &uniform_values,
                &[
                    "uieditorpropertiesgranularity",
                    "ui_editor_properties_granularity",
                ],
                [1.0, 1.0, 0.0, 0.05],
            );
            uniforms.user0[1] = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertieswidth", "ui_editor_properties_width"],
                uniforms.user0[1],
            );
            uniforms.user0[2] = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertiesoffset", "ui_editor_properties_offset"],
                uniforms.user0[2],
            );
            uniforms.user0[3] = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertiestimescale", "ui_editor_properties_timescale"],
                uniforms.user0[3],
            );
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["uieditorpropertiescolor", "ui_editor_properties_color"],
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        Some(SceneCompatEffectKind::FilmGrain) => {
            uniforms.intensity = phase10_uniform_float(
                &uniform_values,
                &["strength", "uieditorpropertiesstrength", "noisealpha"],
                1.0,
            );
            uniforms.radius = phase10_uniform_float(
                &uniform_values,
                &["exponent", "uieditorpropertiespower", "noisepower"],
                0.5,
            );
        }
        Some(SceneCompatEffectKind::Vhs) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["strength", "noisealpha"], 1.0);
            uniforms.speed =
                phase10_uniform_float(&uniform_values, &["distortionspeed"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["distortionstrength"], 1.0);
            uniforms.angle =
                phase10_uniform_float(&uniform_values, &["distortionwidth"], 1.0);
            uniforms.user0[0] = phase10_uniform_float(&uniform_values, &["scale"], 0.3);
            uniforms.user0[1] = phase10_uniform_float(&uniform_values, &["artifacts"], 1.0);
            uniforms.user0[2] = phase10_uniform_float(&uniform_values, &["chromatic"], 0.3);
            uniforms.user0[3] = phase10_uniform_float(&uniform_values, &["tracking"], 0.5);
        }
        Some(SceneCompatEffectKind::BlendGradient) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["multiply"], 1.0);
            uniforms.speed =
                phase10_uniform_float(&uniform_values, &["gradientscale"], 0.05);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["alpha", "alphamultiply"], 1.0);
            uniforms.angle =
                phase10_uniform_float(&uniform_values, &["edgebrightness"], 1.0);
            uniforms.color =
                phase10_uniform_color(&uniform_values, &["edgecolor"], [1.0, 0.75, 0.0, 1.0]);
        }
        Some(SceneCompatEffectKind::WaterCaustics) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesbrightness"], 1.0);
            uniforms.speed =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesspeed"], 1.0);
            uniforms.radius =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesgranularity"], 2.0);
            uniforms.angle =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesglow"], 0.5);
            uniforms.user0[0] =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesdistortion"], 1.0);
            uniforms.user0[1] = phase10_uniform_float(
                &uniform_values,
                &["uieditorpropertieschromaticaberration"],
                1.0,
            );
            uniforms.user0[2] =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiesblur"], 0.0);
            uniforms.user0[3] =
                phase10_uniform_float(&uniform_values, &["uieditorpropertiestimeoffset"], 0.0);
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["uieditorpropertiescolorstart"],
                [0.7, 0.9, 1.0, 1.0],
            );
            let color2 = phase10_uniform_color(
                &uniform_values,
                &["uieditorpropertiescolorend"],
                [0.4, 0.6, 1.0, 1.0],
            );
            uniforms.user1[0] = color2[0];
            uniforms.user1[1] = color2[1];
            uniforms.user1[2] = color2[2];
        }
        Some(SceneCompatEffectKind::Fire) => {
            uniforms.intensity =
                phase10_uniform_float(&uniform_values, &["alpha", "uieditorpropertiesalpha"], 2.0);
            uniforms.speed =
                phase10_uniform_float(&uniform_values, &["speed", "uieditorpropertiesspeed"], 1.0);
            uniforms.radius = phase10_uniform_float(&uniform_values, &["phasescale"], 1.0);
            uniforms.angle = phase10_uniform_float(&uniform_values, &["distortion"], 1.0);
            uniforms.user0 = phase10_uniform_vec4(
                &uniform_values,
                &["scale", "uieditorpropertiesscale"],
                [2.0, 0.0, 0.0, 0.0],
            );
            uniforms.user0[1] = phase10_uniform_float(
                &uniform_values,
                &["threshold", "uieditorpropertiesthreshold"],
                0.0,
            );
            uniforms.user0[2] =
                phase10_uniform_float(&uniform_values, &["feather", "uieditorpropertiesfeather"], 0.5);
            uniforms.user0[3] = phase10_uniform_float(
                &uniform_values,
                &["smoothness", "uieditorpropertiessmoothness"],
                0.0,
            );
            uniforms.color = phase10_uniform_color(
                &uniform_values,
                &["colorstart", "uieditorpropertiescolorstart"],
                [1.0, 0.25, 0.0, 1.0],
            );
            let color2 = phase10_uniform_color(
                &uniform_values,
                &["colorend", "uieditorpropertiescolorend"],
                [1.0, 0.8, 0.0, 1.0],
            );
            uniforms.user1[0] = color2[0];
            uniforms.user1[1] = color2[1];
            uniforms.user1[2] = color2[2];
        }
        Some(SceneCompatEffectKind::XRay) => {
            uniforms.intensity = phase10_uniform_float(
                &uniform_values,
                &["multiply", "uieditorpropertiesmultiply"],
                1.0,
            );
            uniforms.radius = phase10_uniform_float(
                &uniform_values,
                &[
                    "uieditorparticleelementexponent",
                    "ui_editor_particle_element_exponent",
                ],
                1.0,
            );
        }
        _ => {}
    }

    uniforms
}
