use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{
    models::SceneParticleChildKind,
    services::scene_render_planner_service::{
        SceneRenderBlendMode, SceneRenderColor, SceneRenderSpriteControlPointItem,
        SceneRenderSpriteParticleItem, SceneSpriteParticleAttractorConfig, SceneSpriteParticleConfig,
        SceneSpriteParticleFrame, SceneSpriteParticleOscillationConfig,
        SceneSpriteParticleVortexConfig,
    },
};

const MAX_FRAME_EMITS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpriteParticlePrimitive {
    pub texture_path: PathBuf,
    pub blend_mode: SceneRenderBlendMode,
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub uv_rect: [f32; 4],
    pub rotation: f64,
    pub opacity: f64,
    pub color: SceneRenderColor,
    pub transform_origin_x: f64,
    pub transform_origin_y: f64,
}

#[derive(Debug, Default)]
pub struct SceneSpriteParticleScheduler {
    states: BTreeMap<u32, SceneSpriteSystemState>,
    last_update_ms: Option<f64>,
    random: SceneSpriteRandom,
}

#[derive(Debug, Default)]
struct SceneSpriteSystemState {
    root: SceneSpriteEmitterState,
    children: Vec<SceneSpriteEmitterState>,
}

#[derive(Debug, Default)]
struct SceneSpriteEmitterState {
    started_at_ms: Option<f64>,
    emission_credit: f64,
    instantaneous_emitted: bool,
    sequence_cursor: usize,
    particles: Vec<SceneSpriteParticle>,
}

#[derive(Debug, Clone)]
struct SceneSpriteParticle {
    texture_path: PathBuf,
    texture_frames: Vec<SceneSpriteParticleFrame>,
    uv_rect: [f32; 4],
    aspect_ratio: f64,
    blend_mode: SceneRenderBlendMode,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    rotation_degrees: f64,
    angular_velocity: f64,
    turbulence: f64,
    size: f64,
    size_change: Option<[f64; 2]>,
    position_oscillation: Option<SceneSpriteParticleOscillation>,
    alpha_oscillation: Option<SceneSpriteParticleOscillation>,
    size_oscillation: Option<SceneSpriteParticleOscillation>,
    gravity: [f64; 2],
    drag: f64,
    fade_in_ms: f64,
    fade_out_ms: f64,
    born_at_ms: f64,
    life_ms: f64,
    color: SceneRenderColor,
    sequence_multiplier: f64,
}

#[derive(Debug, Clone, Copy)]
struct SceneSpriteParticleOscillation {
    amplitude: f64,
    frequency_hz: f64,
    phase_radians: f64,
    axis_scale: [f64; 2],
}

#[derive(Debug, Clone, Copy)]
struct SceneResolvedSpriteControlPoint {
    id: u32,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct SceneSpriteRandom {
    state: u64,
}

impl Default for SceneSpriteRandom {
    fn default() -> Self {
        Self {
            state: 0x9d2c_5680_5f61_2bf5,
        }
    }
}

impl SceneSpriteRandom {
    fn next_f64(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 11) as f64) / ((1_u64 << 53) as f64)
    }

    fn centered(&mut self) -> f64 {
        self.next_f64() - 0.5
    }

    fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min).max(0.0) * self.next_f64()
    }

    fn chance(&mut self, probability: f64) -> bool {
        self.next_f64() <= probability.clamp(0.0, 1.0)
    }
}

impl SceneSpriteParticleScheduler {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn advance(&mut self, items: &[SceneRenderSpriteParticleItem], now_ms: f64) {
        self.advance_with_cursor(items, None, now_ms);
    }

    pub fn advance_with_cursor(
        &mut self,
        items: &[SceneRenderSpriteParticleItem],
        cursor: Option<(f64, f64)>,
        now_ms: f64,
    ) {
        let delta_ms = self
            .last_update_ms
            .map(|previous| (now_ms - previous).clamp(0.0, 100.0))
            .unwrap_or(16.0);
        self.last_update_ms = Some(now_ms);

        let active_ids = items
            .iter()
            .map(|item| item.object_id)
            .collect::<BTreeSet<_>>();
        self.states
            .retain(|object_id, _| active_ids.contains(object_id));

        for item in items {
            let state = self.states.entry(item.object_id).or_default();
            while state.children.len() < item.children.len() {
                state.children.push(SceneSpriteEmitterState::default());
            }
            state.children.truncate(item.children.len());
            let control_points =
                resolve_runtime_sprite_control_points(cursor, &item.config.control_points);
            let can_emit = item.schedule_mode != crate::models::SceneParticleScheduleMode::InputDriven
                || cursor.is_some();

            let mut root_deaths = advance_emitter_particles(
                &mut self.random,
                &mut state.root,
                &item.config,
                &control_points,
                delta_ms,
                now_ms,
            );
            emit_for_config(
                &mut self.random,
                &mut state.root,
                &item.config,
                &control_points,
                can_emit,
                delta_ms,
                now_ms,
                None,
            );

            for (child_index, child) in item.children.iter().enumerate() {
                let child_state = &mut state.children[child_index];
                let child_control_points =
                    resolve_runtime_sprite_control_points(cursor, &child.config.control_points);
                advance_emitter_particles(
                    &mut self.random,
                    child_state,
                    &child.config,
                    &child_control_points,
                    delta_ms,
                    now_ms,
                );

                match child.child_type {
                    SceneParticleChildKind::Static => {
                        emit_for_config(
                            &mut self.random,
                            child_state,
                            &child.config,
                            &child_control_points,
                            can_emit,
                            delta_ms,
                            now_ms,
                            Some(child.probability),
                        );
                    }
                    SceneParticleChildKind::EventFollow => {
                        for parent in &state.root.particles {
                            if remaining_capacity(child_state, child.config.max_count) == 0 {
                                break;
                            }
                            if self.random.chance(child.probability / 60.0) {
                                let mut spawned = spawn_particle(
                                    &mut self.random,
                                    &mut child_state.sequence_cursor,
                                    &child.config,
                                    &child_control_points,
                                    now_ms,
                                    [parent.x, parent.y],
                                );
                                spawned.vx += parent.vx;
                                spawned.vy += parent.vy;
                                child_state.particles.push(spawned);
                            }
                        }
                    }
                    SceneParticleChildKind::EventDeath => {
                        for death in &root_deaths {
                            if remaining_capacity(child_state, child.config.max_count) == 0 {
                                break;
                            }
                            if self.random.chance(child.probability) {
                                let spawned = spawn_particle(
                                    &mut self.random,
                                    &mut child_state.sequence_cursor,
                                    &child.config,
                                    &child_control_points,
                                    now_ms,
                                    [death[0], death[1]],
                                );
                                child_state.particles.push(spawned);
                            }
                        }
                    }
                    SceneParticleChildKind::EventSpawn | SceneParticleChildKind::Unsupported => {}
                }
            }
            root_deaths.clear();
        }
    }

    pub fn pause(&mut self) {
        self.last_update_ms = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn primitives(
        &self,
        item: &SceneRenderSpriteParticleItem,
        canvas_height: f64,
        now_ms: f64,
    ) -> Vec<SceneSpriteParticlePrimitive> {
        let mut primitives = Vec::new();
        let Some(state) = self.states.get(&item.object_id) else {
            return primitives;
        };
        primitives.extend(
            state
                .root
                .particles
                .iter()
                .filter_map(|particle| primitive_from_particle(particle, canvas_height, now_ms)),
        );
        for child in &state.children {
            primitives.extend(
                child.particles.iter().filter_map(|particle| {
                    primitive_from_particle(particle, canvas_height, now_ms)
                }),
            );
        }
        primitives
    }
}

fn advance_emitter_particles(
    random: &mut SceneSpriteRandom,
    state: &mut SceneSpriteEmitterState,
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
    delta_ms: f64,
    now_ms: f64,
) -> Vec<[f64; 2]> {
    let delta_seconds = (delta_ms / 1000.0).clamp(0.0, 0.1);
    let mut deaths = Vec::new();
    state.particles.retain_mut(|particle| {
        apply_sprite_forces(particle, config, control_points, delta_seconds);
        particle.x += particle.vx * delta_seconds;
        particle.y += particle.vy * delta_seconds;
        if particle.turbulence > 0.0 {
            particle.vx += random.centered() * particle.turbulence * delta_seconds;
            particle.vy += random.centered() * particle.turbulence * delta_seconds;
        }
        particle.rotation_degrees += particle.angular_velocity * delta_seconds;
        let alive = now_ms - particle.born_at_ms < particle.life_ms;
        if !alive {
            deaths.push([particle.x, particle.y]);
        }
        alive
    });
    deaths
}

fn emit_for_config(
    random: &mut SceneSpriteRandom,
    state: &mut SceneSpriteEmitterState,
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
    can_emit: bool,
    delta_ms: f64,
    now_ms: f64,
    probability: Option<f64>,
) {
    if !can_emit {
        return;
    }
    let started_at_ms = *state.started_at_ms.get_or_insert(now_ms);
    if now_ms - started_at_ms < config.start_time_ms {
        return;
    }
    let origin = sprite_spawn_origin(config, control_points);
    if config.instantaneous && !state.instantaneous_emitted {
        let emit_count = remaining_capacity(state, config.max_count).min(MAX_FRAME_EMITS);
        for _ in 0..emit_count {
            if probability
                .map(|value| random.chance(value))
                .unwrap_or(true)
            {
                let spawned = spawn_particle(
                    random,
                    &mut state.sequence_cursor,
                    config,
                    control_points,
                    now_ms,
                    origin,
                );
                state.particles.push(spawned);
            }
        }
        state.instantaneous_emitted = true;
    } else if !config.instantaneous {
        state.emission_credit += config.emission_rate.max(0.0) * delta_ms / 1000.0;
        let requested_count = state
            .emission_credit
            .floor()
            .clamp(0.0, MAX_FRAME_EMITS as f64) as usize;
        if requested_count > 0 {
            state.emission_credit -= requested_count as f64;
            let emit_count = requested_count.min(remaining_capacity(state, config.max_count));
            for _ in 0..emit_count {
                if probability
                    .map(|value| random.chance(value))
                    .unwrap_or(true)
                {
                    let spawned = spawn_particle(
                        random,
                        &mut state.sequence_cursor,
                        config,
                        control_points,
                        now_ms,
                        origin,
                    );
                    state.particles.push(spawned);
                }
            }
        }
    }
}

fn spawn_particle(
    random: &mut SceneSpriteRandom,
    sequence_cursor: &mut usize,
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
    now_ms: f64,
    origin: [f64; 2],
) -> SceneSpriteParticle {
    let (x, y, _angle, velocity) =
        spawn_position_and_velocity(random, sequence_cursor, config, control_points, origin);
    let color = interpolate_color(
        config.color_min,
        config.color_max,
        random.next_f64(),
        random.range(config.alpha_range[0], config.alpha_range[1]),
    );
    let frame = choose_texture_frame(random, config);

    SceneSpriteParticle {
        texture_path: config.texture_path.clone(),
        texture_frames: config.texture_frames.clone(),
        uv_rect: frame.uv_rect,
        aspect_ratio: frame.aspect_ratio,
        blend_mode: config.blend_mode,
        x,
        y,
        vx: velocity[0],
        vy: velocity[1],
        rotation_degrees: random.range(config.rotation_range[0], config.rotation_range[1]),
        angular_velocity: random.range(
            config.angular_velocity_range[0],
            config.angular_velocity_range[1],
        ),
        turbulence: config.turbulence,
        size: random
            .range(config.size_range[0], config.size_range[1])
            .max(0.5),
        size_change: config.size_change,
        position_oscillation: spawn_position_oscillation(random, config.position_oscillation),
        alpha_oscillation: spawn_position_oscillation(random, config.alpha_oscillation),
        size_oscillation: spawn_position_oscillation(random, config.size_oscillation),
        gravity: config.gravity,
        drag: config.drag,
        fade_in_ms: config.fade_in_ms.max(0.0),
        fade_out_ms: config.fade_out_ms.max(0.0),
        born_at_ms: now_ms,
        life_ms: random
            .range(config.lifetime_ms_range[0], config.lifetime_ms_range[1])
            .max(16.0),
        color,
        sequence_multiplier: config.sequence_multiplier,
    }
}

fn spawn_position_oscillation(
    random: &mut SceneSpriteRandom,
    config: Option<SceneSpriteParticleOscillationConfig>,
) -> Option<SceneSpriteParticleOscillation> {
    config.map(|config| SceneSpriteParticleOscillation {
        amplitude: random
            .range(config.amplitude_range[0], config.amplitude_range[1])
            .abs(),
        frequency_hz: random
            .range(config.frequency_range[0], config.frequency_range[1])
            .max(0.0),
        phase_radians: random.range(config.phase_range[0], config.phase_range[1])
            * std::f64::consts::TAU,
        axis_scale: config.axis_scale,
    })
}

fn resolve_runtime_sprite_control_points(
    cursor: Option<(f64, f64)>,
    control_points: &[SceneRenderSpriteControlPointItem],
) -> Vec<SceneResolvedSpriteControlPoint> {
    let mut resolved = control_points
        .iter()
        .map(|control_point| SceneResolvedSpriteControlPoint {
            id: control_point.id,
            x: control_point.position[0],
            y: control_point.position[1],
        })
        .collect::<Vec<_>>();
    if let Some((cursor_x, cursor_y)) = cursor {
        for control_point in control_points {
            if !control_point.lock_to_pointer {
                continue;
            }
            if let Some(runtime_point) = resolved.iter_mut().find(|point| point.id == control_point.id)
            {
                runtime_point.x = cursor_x;
                runtime_point.y = cursor_y;
            }
        }
    }
    resolved
}

fn sprite_spawn_origin(
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
) -> [f64; 2] {
    sprite_primary_control_point(config, control_points)
        .map(|point| [point.x, point.y])
        .unwrap_or(config.spawn_origin)
}

fn sprite_primary_control_point<'a>(
    config: &SceneSpriteParticleConfig,
    control_points: &'a [SceneResolvedSpriteControlPoint],
) -> Option<&'a SceneResolvedSpriteControlPoint> {
    config
        .control_points
        .iter()
        .find(|control_point| control_point.lock_to_pointer)
        .and_then(|control_point| control_points.iter().find(|point| point.id == control_point.id))
        .or_else(|| control_points.first())
}

fn spawn_position_and_velocity(
    random: &mut SceneSpriteRandom,
    sequence_cursor: &mut usize,
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
    origin: [f64; 2],
) -> (f64, f64, f64, [f64; 2]) {
    if let (Some(sequence), Some(anchor)) = (
        config.sequence_control_point,
        sprite_primary_control_point(config, control_points),
    ) {
        let sequence_index = *sequence_cursor % sequence.count.max(1);
        *sequence_cursor = (*sequence_cursor).wrapping_add(1);
        let angle = std::f64::consts::TAU * sequence_index as f64 / sequence.count.max(1) as f64;
        let radius = config.spawn_radius[1]
            .max(config.size_range[0] * 0.25)
            .max(4.0);
        let ring_offset = [angle.cos() * radius, angle.sin() * radius];
        let base_velocity = [
            random.range(sequence.speed_range[0][0], sequence.speed_range[1][0]),
            random.range(sequence.speed_range[0][1], sequence.speed_range[1][1]),
        ];
        let velocity = if base_velocity[0].abs() > 0.001 || base_velocity[1].abs() > 0.001 {
            rotate_2d(base_velocity, angle + config.orientation)
        } else {
            choose_velocity(random, config, angle)
        };
        return (
            anchor.x + ring_offset[0],
            anchor.y + ring_offset[1],
            angle,
            velocity,
        );
    }

    let radius = random.range(config.spawn_radius[0], config.spawn_radius[1]);
    let angle = random.range(0.0, std::f64::consts::TAU);
    let x = origin[0] + angle.cos() * radius;
    let y = origin[1] + angle.sin() * radius;
    let velocity = choose_velocity(random, config, angle);
    (x, y, angle, velocity)
}

fn apply_sprite_forces(
    particle: &mut SceneSpriteParticle,
    config: &SceneSpriteParticleConfig,
    control_points: &[SceneResolvedSpriteControlPoint],
    delta_seconds: f64,
) {
    if particle.drag > 0.0 {
        let damping = (1.0 - particle.drag * delta_seconds).clamp(0.0, 1.0);
        particle.vx *= damping;
        particle.vy *= damping;
    }
    particle.vx += particle.gravity[0] * delta_seconds;
    particle.vy += particle.gravity[1] * delta_seconds;

    let anchor = sprite_primary_control_point(config, control_points);
    for attractor in &config.attractors {
        let Some(anchor) = anchor else {
            break;
        };
        apply_attractor(particle, attractor, anchor, delta_seconds);
    }
    for vortex in &config.vortexes {
        let Some(anchor) = anchor else {
            break;
        };
        apply_vortex(particle, vortex, anchor, delta_seconds);
    }
}

fn apply_attractor(
    particle: &mut SceneSpriteParticle,
    attractor: &SceneSpriteParticleAttractorConfig,
    anchor: &SceneResolvedSpriteControlPoint,
    delta_seconds: f64,
) {
    let target_x = anchor.x + attractor.origin_offset[0];
    let target_y = anchor.y + attractor.origin_offset[1];
    let dx = target_x - particle.x;
    let dy = target_y - particle.y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= 0.001 {
        return;
    }
    if attractor.threshold > 0.0 && distance > attractor.threshold {
        return;
    }
    let falloff = if attractor.threshold > 0.0 {
        1.0 - (distance / attractor.threshold).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let force = attractor.scale * falloff * delta_seconds;
    particle.vx += dx / distance * force;
    particle.vy += dy / distance * force;
}

fn apply_vortex(
    particle: &mut SceneSpriteParticle,
    vortex: &SceneSpriteParticleVortexConfig,
    anchor: &SceneResolvedSpriteControlPoint,
    delta_seconds: f64,
) {
    let center_x = anchor.x + vortex.origin_offset[0];
    let center_y = anchor.y + vortex.origin_offset[1];
    let dx = particle.x - center_x;
    let dy = particle.y - center_y;
    let distance = (dx * dx + dy * dy).sqrt();
    if distance <= 0.001 || distance < vortex.distance_inner {
        return;
    }
    if vortex.distance_outer > 0.0 && distance > vortex.distance_outer {
        return;
    }
    let outer = vortex.distance_outer.max(vortex.distance_inner + 0.001);
    let t = ((distance - vortex.distance_inner) / (outer - vortex.distance_inner)).clamp(0.0, 1.0);
    let speed = vortex.speed_inner + (vortex.speed_outer - vortex.speed_inner) * t;
    let tangent = [-dy / distance, dx / distance];
    particle.vx += tangent[0] * speed * delta_seconds;
    particle.vy += tangent[1] * speed * delta_seconds;
}

fn choose_velocity(
    random: &mut SceneSpriteRandom,
    config: &SceneSpriteParticleConfig,
    fallback_angle: f64,
) -> [f64; 2] {
    if let Some(range) = config.velocity_range {
        return rotate_2d(
            [
                random.range(range[0][0], range[1][0]),
                random.range(range[0][1], range[1][1]),
            ],
            config.orientation,
        );
    }

    let direction = choose_direction(random, config, fallback_angle);
    let speed = random.range(config.speed_range[0], config.speed_range[1]);
    [direction[0] * speed, direction[1] * speed]
}

fn choose_direction(
    random: &mut SceneSpriteRandom,
    config: &SceneSpriteParticleConfig,
    fallback_angle: f64,
) -> [f64; 2] {
    let direction = if config.directions.is_empty() {
        [fallback_angle.cos(), fallback_angle.sin()]
    } else {
        let index = (random.next_f64() * config.directions.len() as f64)
            .floor()
            .clamp(0.0, config.directions.len().saturating_sub(1) as f64)
            as usize;
        config.directions[index]
    };
    let sign = if config.sign < 0.0 { -1.0 } else { 1.0 };
    let length = (direction[0] * direction[0] + direction[1] * direction[1])
        .sqrt()
        .max(0.0001);
    rotate_2d(
        [direction[0] / length * sign, direction[1] / length * sign],
        config.orientation,
    )
}

fn choose_texture_frame(
    random: &mut SceneSpriteRandom,
    config: &SceneSpriteParticleConfig,
) -> SceneSpriteParticleFrame {
    if config.texture_frames.is_empty() {
        return SceneSpriteParticleFrame {
            uv_rect: [0.0, 0.0, 1.0, 1.0],
            aspect_ratio: 1.0,
        };
    }
    let index = (random.next_f64() * config.texture_frames.len() as f64)
        .floor()
        .clamp(0.0, config.texture_frames.len().saturating_sub(1) as f64) as usize;
    config.texture_frames[index].clone()
}

fn primitive_from_particle(
    particle: &SceneSpriteParticle,
    canvas_height: f64,
    now_ms: f64,
) -> Option<SceneSpriteParticlePrimitive> {
    if now_ms - particle.born_at_ms >= particle.life_ms {
        return None;
    }
    let age_ms = now_ms - particle.born_at_ms;
    let age = (age_ms / particle.life_ms).clamp(0.0, 1.0);
    let opacity = particle_alpha(particle, age_ms);
    let size_factor = particle
        .size_change
        .map(|range| range[0] + (range[1] - range[0]) * age)
        .unwrap_or(1.0)
        .max(0.0);
    let (uv_rect, aspect_ratio) =
        if particle.sequence_multiplier > 0.0 && particle.texture_frames.len() > 1 {
            let elapsed_s = age_ms / 1000.0;
            let frame_idx =
                (elapsed_s * particle.sequence_multiplier * particle.texture_frames.len() as f64)
                    .floor() as usize
                    % particle.texture_frames.len();
            let frame = &particle.texture_frames[frame_idx];
            (frame.uv_rect, frame.aspect_ratio)
        } else {
            (particle.uv_rect, particle.aspect_ratio)
        };
    let height = (particle.size * size_factor).max(0.5);
    let height = if let Some(ref osc) = particle.size_oscillation {
        let wave = oscillation_wave(osc, age_ms / 1000.0);
        (height * (1.0 + wave)).max(0.5)
    } else {
        height
    };
    let width = (height * aspect_ratio).max(0.5);
    let render_position = oscillated_position(particle, age_ms / 1000.0);
    let opacity = if let Some(ref osc) = particle.alpha_oscillation {
        let wave = oscillation_wave(osc, age_ms / 1000.0);
        (opacity * (1.0 + wave)).clamp(0.0, 1.0)
    } else {
        opacity
    };
    let top = canvas_height - render_position[1] - height / 2.0;
    Some(SceneSpriteParticlePrimitive {
        texture_path: particle.texture_path.clone(),
        blend_mode: particle.blend_mode,
        left: render_position[0] - width / 2.0,
        top,
        width,
        height,
        uv_rect,
        rotation: particle.rotation_degrees.to_radians(),
        opacity,
        color: SceneRenderColor {
            alpha: ((particle.color.alpha as f64) * opacity).round() as u8,
            ..particle.color
        },
        transform_origin_x: render_position[0],
        transform_origin_y: top + height / 2.0,
    })
}

fn oscillated_position(particle: &SceneSpriteParticle, age_seconds: f64) -> [f64; 2] {
    let Some(oscillation) = particle.position_oscillation else {
        return [particle.x, particle.y];
    };
    let wave = oscillation_wave(&oscillation, age_seconds);
    [
        particle.x + wave * oscillation.axis_scale[0],
        particle.y + wave * oscillation.axis_scale[1],
    ]
}

fn oscillation_wave(oscillation: &SceneSpriteParticleOscillation, age_seconds: f64) -> f64 {
    (oscillation.phase_radians
        + age_seconds.max(0.0) * oscillation.frequency_hz * std::f64::consts::TAU)
        .sin()
        * oscillation.amplitude
}

fn particle_alpha(particle: &SceneSpriteParticle, age_ms: f64) -> f64 {
    let mut alpha: f64 = 1.0;
    if particle.fade_in_ms > 0.0 {
        alpha = alpha.min((age_ms / particle.fade_in_ms).clamp(0.0, 1.0));
    }
    if particle.fade_out_ms > 0.0 {
        let remaining_ms = particle.life_ms - age_ms;
        alpha = alpha.min((remaining_ms / particle.fade_out_ms).clamp(0.0, 1.0));
    }
    alpha.clamp(0.0, 1.0)
}

fn interpolate_color(
    min: SceneRenderColor,
    max: SceneRenderColor,
    t: f64,
    alpha: f64,
) -> SceneRenderColor {
    SceneRenderColor {
        red: lerp_channel(min.red, max.red, t),
        green: lerp_channel(min.green, max.green, t),
        blue: lerp_channel(min.blue, max.blue, t),
        alpha: ((lerp_channel(min.alpha, max.alpha, t) as f64) * alpha.clamp(0.0, 1.0)).round()
            as u8,
    }
}

fn lerp_channel(min: u8, max: u8, t: f64) -> u8 {
    (min as f64 + (max as f64 - min as f64) * t.clamp(0.0, 1.0)).round() as u8
}

fn remaining_capacity(state: &SceneSpriteEmitterState, max_count: usize) -> usize {
    max_count.saturating_sub(state.particles.len())
}

fn rotate_2d(vector: [f64; 2], radians: f64) -> [f64; 2] {
    if radians.abs() <= f64::EPSILON {
        return vector;
    }
    let cos = radians.cos();
    let sin = radians.sin();
    [
        vector[0] * cos - vector[1] * sin,
        vector[0] * sin + vector[1] * cos,
    ]
}

#[cfg(test)]
mod tests {
    use super::{SceneSpriteParticleConfig, SceneSpriteParticleScheduler};
    use crate::models::{SceneParticleChildKind, SceneParticleScheduleMode};
    use crate::services::scene_render_planner_service::{
        SceneRenderBlendMode, SceneRenderColor, SceneRenderSpriteControlPointItem,
        SceneRenderSpriteParticleItem, SceneSpriteParticleAttractorConfig,
        SceneSpriteParticleChildItem, SceneSpriteParticleFrame,
        SceneSpriteParticleOscillationConfig, SceneSpriteParticleSequenceControlPointConfig,
        SceneSpriteParticleVortexConfig,
    };
    use std::path::PathBuf;

    fn config() -> SceneSpriteParticleConfig {
        SceneSpriteParticleConfig {
            texture_path: PathBuf::from("/tmp/sprite.png"),
            texture_frames: vec![SceneSpriteParticleFrame {
                uv_rect: [0.0, 0.0, 1.0, 1.0],
                aspect_ratio: 1.0,
            }],
            blend_mode: SceneRenderBlendMode::Normal,
            spawn_origin: [100.0, 120.0],
            spawn_radius: [0.0, 4.0],
            directions: vec![[0.0, 1.0]],
            sign: 1.0,
            orientation: 0.0,
            velocity_range: None,
            color_min: SceneRenderColor {
                red: 200,
                green: 120,
                blue: 80,
                alpha: 220,
            },
            color_max: SceneRenderColor {
                red: 255,
                green: 220,
                blue: 180,
                alpha: 255,
            },
            alpha_range: [0.8, 1.0],
            size_range: [16.0, 20.0],
            lifetime_ms_range: [80.0, 100.0],
            speed_range: [12.0, 16.0],
            rotation_range: [0.0, 30.0],
            angular_velocity_range: [90.0, 120.0],
            turbulence: 8.0,
            size_change: Some([1.0, 0.5]),
            position_oscillation: None,
            alpha_oscillation: None,
            size_oscillation: None,
            gravity: [0.0, 0.0],
            drag: 0.0,
            fade_in_ms: 0.0,
            fade_out_ms: 0.0,
            emission_rate: 120.0,
            max_count: 24,
            start_time_ms: 0.0,
            instantaneous: false,
            sequence_multiplier: 0.0,
            control_points: vec![],
            sequence_control_point: None,
            attractors: vec![],
            vortexes: vec![],
        }
    }

    #[test]
    fn sprite_scheduler_emits_autonomous_billboard_quads_without_cursor() {
        let item = SceneRenderSpriteParticleItem {
            object_id: 7,
            object_name: "Sprite".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config: config(),
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 100.0);
        scheduler.advance(&[item.clone()], 220.0);

        let primitives = scheduler.primitives(&item, 400.0, 220.0);
        assert!(!primitives.is_empty());
        assert!(primitives[0].width > 0.0);
        assert_eq!(primitives[0].texture_path, PathBuf::from("/tmp/sprite.png"));
    }

    #[test]
    fn sprite_scheduler_preserves_atlas_uv_and_frame_aspect_ratio() {
        let mut config = config();
        config.texture_frames = vec![SceneSpriteParticleFrame {
            uv_rect: [0.25, 0.0, 0.5, 0.75],
            aspect_ratio: 0.5,
        }];
        config.instantaneous = true;
        config.max_count = 1;
        let item = SceneRenderSpriteParticleItem {
            object_id: 8,
            object_name: "SpriteAtlas".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config,
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 100.0);

        let primitives = scheduler.primitives(&item, 400.0, 100.0);
        assert_eq!(primitives.len(), 1);
        assert_eq!(primitives[0].uv_rect, [0.25, 0.0, 0.5, 0.75]);
        assert!((primitives[0].width / primitives[0].height - 0.5).abs() < 0.001);
    }

    #[test]
    fn sprite_scheduler_applies_velocityrandom_vector_components() {
        let mut config = config();
        config.emission_rate = 20.0;
        config.max_count = 256;
        config.velocity_range = Some([[-50.0, -40.0], [-50.0, -40.0]]);
        config.spawn_radius = [0.0, 0.0];
        config.lifetime_ms_range = [1000.0, 1000.0];
        let item = SceneRenderSpriteParticleItem {
            object_id: 9,
            object_name: "SpriteVelocity".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config,
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);
        scheduler.advance(&[item.clone()], 100.0);
        let first = scheduler.primitives(&item, 400.0, 100.0);
        scheduler.advance(&[item.clone()], 200.0);
        let second = scheduler.primitives(&item, 400.0, 200.0);

        assert!(!first.is_empty());
        assert!(!second.is_empty());
        assert!(second[0].left < first[0].left);
        assert!(second[0].top > first[0].top);
    }

    #[test]
    fn sprite_scheduler_emits_input_driven_particles_from_pointer_locked_control_point() {
        let mut cfg = config();
        cfg.spawn_origin = [0.0, 0.0];
        cfg.control_points = vec![SceneRenderSpriteControlPointItem {
            id: 0,
            position: [10.0, 20.0],
            lock_to_pointer: true,
        }];
        cfg.sequence_control_point = Some(SceneSpriteParticleSequenceControlPointConfig {
            count: 5,
            speed_range: [[0.0, 80.0], [0.0, 80.0]],
        });
        cfg.spawn_radius = [0.0, 0.0];
        cfg.emission_rate = 20.0;
        cfg.max_count = 8;
        let item = SceneRenderSpriteParticleItem {
            object_id: 13,
            object_name: "CursorSprite".to_string(),
            schedule_mode: crate::models::SceneParticleScheduleMode::InputDriven,
            config: cfg,
            children: vec![],
        };

        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance_with_cursor(&[item.clone()], Some((200.0, 160.0)), 0.0);
        scheduler.advance_with_cursor(&[item.clone()], Some((240.0, 180.0)), 100.0);

        let primitives = scheduler.primitives(&item, 400.0, 100.0);
        assert!(!primitives.is_empty());
        let centroid_x =
            primitives.iter().map(|primitive| primitive.left + primitive.width / 2.0).sum::<f64>()
                / primitives.len() as f64;
        let centroid_y = primitives
            .iter()
            .map(|primitive| 400.0 - primitive.top - primitive.height / 2.0)
            .sum::<f64>()
            / primitives.len() as f64;
        assert!(centroid_x > 180.0);
        assert!(centroid_y > 140.0);
    }

    #[test]
    fn sprite_scheduler_applies_attractor_and_vortex_forces() {
        let mut cfg = config();
        cfg.instantaneous = true;
        cfg.max_count = 1;
        cfg.spawn_radius = [12.0, 12.0];
        cfg.velocity_range = Some([[0.0, 0.0], [0.0, 0.0]]);
        cfg.lifetime_ms_range = [1000.0, 1000.0];
        cfg.gravity = [0.0, 0.0];
        cfg.drag = 0.0;
        cfg.control_points = vec![SceneRenderSpriteControlPointItem {
            id: 0,
            position: [100.0, 100.0],
            lock_to_pointer: true,
        }];
        cfg.sequence_control_point = Some(SceneSpriteParticleSequenceControlPointConfig {
            count: 1,
            speed_range: [[0.0, 0.0], [0.0, 0.0]],
        });
        cfg.attractors = vec![SceneSpriteParticleAttractorConfig {
            origin_offset: [0.0, 0.0],
            scale: 400.0,
            threshold: 200.0,
        }];
        cfg.vortexes = vec![SceneSpriteParticleVortexConfig {
            origin_offset: [0.0, 0.0],
            distance_inner: 0.0,
            distance_outer: 200.0,
            speed_inner: 240.0,
            speed_outer: 120.0,
        }];
        let item = SceneRenderSpriteParticleItem {
            object_id: 14,
            object_name: "Forces".to_string(),
            schedule_mode: crate::models::SceneParticleScheduleMode::InputDriven,
            config: cfg,
            children: vec![],
        };

        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance_with_cursor(&[item.clone()], Some((100.0, 100.0)), 0.0);
        scheduler.advance_with_cursor(&[item.clone()], Some((100.0, 100.0)), 16.0);
        let start = scheduler.primitives(&item, 400.0, 16.0);
        scheduler.advance_with_cursor(&[item.clone()], Some((100.0, 100.0)), 100.0);
        let next = scheduler.primitives(&item, 400.0, 100.0);

        assert_eq!(start.len(), 1);
        assert_eq!(next.len(), 1);
        let start_center = [start[0].left + start[0].width / 2.0, 400.0 - start[0].top - start[0].height / 2.0];
        let next_center = [next[0].left + next[0].width / 2.0, 400.0 - next[0].top - next[0].height / 2.0];
        assert!(
            (next_center[0] - start_center[0]).abs() > 1.0
                || (next_center[1] - start_center[1]).abs() > 1.0
        );
    }

    #[test]
    fn sprite_scheduler_applies_position_oscillation_to_rendered_position() {
        let mut config = config();
        config.instantaneous = true;
        config.max_count = 1;
        config.spawn_radius = [0.0, 0.0];
        config.velocity_range = Some([[0.0, 0.0], [0.0, 0.0]]);
        config.size_range = [10.0, 10.0];
        config.size_change = None;
        config.lifetime_ms_range = [1000.0, 1000.0];
        config.position_oscillation = Some(SceneSpriteParticleOscillationConfig {
            amplitude_range: [20.0, 20.0],
            frequency_range: [1.0, 1.0],
            phase_range: [0.0, 0.0],
            axis_scale: [1.0, 0.0],
        });
        let item = SceneRenderSpriteParticleItem {
            object_id: 12,
            object_name: "SpriteOscillate".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config,
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);

        let start = scheduler.primitives(&item, 400.0, 0.0);
        let quarter = scheduler.primitives(&item, 400.0, 250.0);

        assert_eq!(start.len(), 1);
        assert_eq!(quarter.len(), 1);
        assert!(quarter[0].left > start[0].left + 15.0);
        assert_eq!(quarter[0].top, start[0].top);
    }

    #[test]
    fn sprite_scheduler_uses_alphafade_only_at_lifetime_edges() {
        let mut config = config();
        config.instantaneous = true;
        config.max_count = 1;
        config.lifetime_ms_range = [1000.0, 1000.0];
        config.fade_in_ms = 100.0;
        config.fade_out_ms = 200.0;
        let item = SceneRenderSpriteParticleItem {
            object_id: 10,
            object_name: "SpriteFade".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config,
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);

        let fade_in = scheduler.primitives(&item, 400.0, 50.0);
        let steady = scheduler.primitives(&item, 400.0, 500.0);
        let fade_out = scheduler.primitives(&item, 400.0, 900.0);

        assert!(fade_in[0].opacity < steady[0].opacity);
        assert_eq!(steady[0].opacity, 1.0);
        assert!(fade_out[0].opacity < steady[0].opacity);
    }

    #[test]
    fn sprite_scheduler_does_not_reap_live_particles_to_make_room_for_new_emissions() {
        let mut config = config();
        config.emission_rate = 1000.0;
        config.max_count = 2;
        config.spawn_radius = [0.0, 0.0];
        config.velocity_range = Some([[100.0, 0.0], [100.0, 0.0]]);
        config.size_range = [2.0, 2.0];
        config.lifetime_ms_range = [10_000.0, 10_000.0];
        let item = SceneRenderSpriteParticleItem {
            object_id: 11,
            object_name: "SpritePool".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config,
            children: vec![],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);
        let initial = scheduler.primitives(&item, 400.0, 0.0);
        scheduler.advance(&[item.clone()], 16.0);
        let after_full = scheduler.primitives(&item, 400.0, 16.0);

        assert_eq!(initial.len(), 2);
        assert_eq!(after_full.len(), 2);
        assert!(after_full[0].left > initial[0].left + 1.0);
    }

    #[test]
    fn eventdeath_child_emits_when_parent_particle_expires() {
        let mut parent = config();
        parent.lifetime_ms_range = [32.0, 32.0];
        parent.emission_rate = 0.0;
        parent.instantaneous = true;
        parent.max_count = 1;
        let mut child_config = config();
        child_config.texture_path = PathBuf::from("/tmp/child.png");
        child_config.emission_rate = 0.0;
        child_config.instantaneous = false;
        let item = SceneRenderSpriteParticleItem {
            object_id: 9,
            object_name: "SpriteChild".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config: parent,
            children: vec![SceneSpriteParticleChildItem {
                child_type: SceneParticleChildKind::EventDeath,
                config: child_config,
                probability: 1.0,
                control_point_start_index: None,
            }],
        };
        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);
        scheduler.advance(&[item.clone()], 48.0);

        let primitives = scheduler.primitives(&item, 400.0, 48.0);
        assert!(primitives
            .iter()
            .any(|primitive| primitive.texture_path == PathBuf::from("/tmp/child.png")));
    }

    #[test]
    fn sprite_sequence_multiplier_zero_does_not_cycle_frames() {
        let c = SceneSpriteParticleConfig {
            texture_frames: vec![
                SceneSpriteParticleFrame {
                    uv_rect: [0.0, 0.0, 0.5, 0.5],
                    aspect_ratio: 1.0,
                },
                SceneSpriteParticleFrame {
                    uv_rect: [0.5, 0.0, 1.0, 0.5],
                    aspect_ratio: 2.0,
                },
            ],
            sequence_multiplier: 0.0,
            ..config()
        };
        let item = SceneRenderSpriteParticleItem {
            object_id: 9,
            object_name: "Multi".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config: c.clone(),
            children: vec![],
        };

        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);
        scheduler.advance(&[item.clone()], 50.0);
        let p1 = scheduler.primitives(&item, 400.0, 50.0);

        scheduler.advance(&[item.clone()], 400.0);
        let p2 = scheduler.primitives(&item, 400.0, 400.0);

        assert!(!p1.is_empty());
        assert!(!p2.is_empty());
        let no_cycle = p1.iter().all(|primitive| {
            let matching = p2.iter().find(|p2| primitive.uv_rect == p2.uv_rect);
            matching.is_some()
        });
        assert!(
            no_cycle,
            "sequence_multiplier=0 should not cause frame UV to cycle beyond spawn-time random choice"
        );
    }

    #[test]
    fn sprite_sequence_multiplier_cycles_frames_over_time() {
        let c = SceneSpriteParticleConfig {
            texture_frames: vec![
                SceneSpriteParticleFrame {
                    uv_rect: [0.0, 0.0, 0.5, 0.5],
                    aspect_ratio: 1.0,
                },
                SceneSpriteParticleFrame {
                    uv_rect: [0.5, 0.0, 1.0, 0.5],
                    aspect_ratio: 2.0,
                },
            ],
            sequence_multiplier: 3.0,
            lifetime_ms_range: [2000.0, 2000.0],
            emission_rate: 600.0,
            max_count: 60,
            ..config()
        };
        let item = SceneRenderSpriteParticleItem {
            object_id: 10,
            object_name: "Cycle".to_string(),
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            config: c.clone(),
            children: vec![],
        };

        let mut scheduler = SceneSpriteParticleScheduler::default();
        scheduler.advance(&[item.clone()], 0.0);
        scheduler.advance(&[item.clone()], 16.0);
        let p1 = scheduler.primitives(&item, 400.0, 16.0);

        scheduler.advance(&[item.clone()], 500.0);
        let p2 = scheduler.primitives(&item, 400.0, 500.0);

        assert!(!p1.is_empty());
        assert!(!p2.is_empty());
        let has_uv_change = p1
            .iter()
            .zip(p2.iter())
            .any(|(a, b)| a.uv_rect != b.uv_rect);
        assert!(
            has_uv_change,
            "sequence_multiplier should cycle frame UV over time"
        );
    }
}
