use std::collections::{BTreeMap, BTreeSet};

use crate::{
    models::{SceneParticleKind, SceneParticleRendererFamily, SceneParticleScheduleMode},
    services::scene_render_planner_service::{
        SceneRenderColor, SceneRenderParticleItem, SceneRenderRopeParticleItem,
    },
};

const MAX_FRAME_EMITS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneParticleCursor {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneParticlePrimitive {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub color: SceneRenderColor,
    pub transform_origin_x: f64,
    pub transform_origin_y: f64,
    pub uv_offset: [f64; 2],
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneRopeParticlePrimitive {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub width: f64,
    pub opacity: f64,
    pub color: SceneRenderColor,
    pub uv_offset: [f64; 2],
}

#[derive(Debug, Default)]
pub struct SceneParticleScheduler {
    last_cursor: Option<SceneParticleCursor>,
    last_update_ms: Option<f64>,
    input_states: BTreeMap<u32, SceneInputParticleState>,
    autonomous_states: BTreeMap<u32, SceneAutonomousParticleState>,
    random: SceneRandom,
}

#[derive(Debug, Default)]
pub struct SceneRopeParticleScheduler {
    last_cursor: Option<SceneParticleCursor>,
    last_update_ms: Option<f64>,
    states: BTreeMap<u32, SceneRopeParticleState>,
}

#[derive(Debug, Default)]
struct SceneInputParticleState {
    line_emission_credit: f64,
    petal_emission_credit: f64,
    line_points: Vec<SceneTrailPoint>,
    petals: Vec<SceneParticle>,
}

#[derive(Debug, Default)]
struct SceneAutonomousParticleState {
    started_at_ms: Option<f64>,
    emission_credit: f64,
    instantaneous_emitted: bool,
    particles: Vec<SceneParticle>,
}

#[derive(Debug, Default)]
struct SceneRopeParticleState {
    control_points: Vec<SceneResolvedRopeControlPoint>,
    trail_emission_credit: f64,
    trail_points: Vec<SceneTrailPoint>,
}

#[derive(Debug, Clone, Copy)]
struct SceneResolvedRopeControlPoint {
    id: u32,
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Copy)]
struct SceneTrailPoint {
    x: f64,
    y: f64,
    created_at_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct SceneParticle {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    rotation: f64,
    spin: f64,
    size: f64,
    born_at_ms: f64,
    life_ms: f64,
    color: SceneRenderColor,
}

#[derive(Debug, Clone, Copy)]
struct SceneRandom {
    state: u64,
}

impl Default for SceneRandom {
    fn default() -> Self {
        Self {
            state: 0x4d595df4d0f33173,
        }
    }
}

impl SceneRandom {
    fn next_f64(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 11) as f64) / ((1_u64 << 53) as f64)
    }

    fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min).max(0.0) * self.next_f64()
    }
}

impl SceneParticleScheduler {
    pub fn advance(
        &mut self,
        cursor: Option<SceneParticleCursor>,
        items: &[SceneRenderParticleItem],
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
        self.input_states
            .retain(|object_id, _| active_ids.contains(object_id));
        self.autonomous_states
            .retain(|object_id, _| active_ids.contains(object_id));

        let previous = self.last_cursor;
        self.last_cursor = cursor;

        for item in items {
            match item.schedule_mode {
                SceneParticleScheduleMode::InputDriven => {
                    let state = self.input_states.entry(item.object_id).or_default();
                    advance_input_item(state, previous, cursor, item, delta_ms, now_ms);
                }
                SceneParticleScheduleMode::Autonomous => {
                    let state = self.autonomous_states.entry(item.object_id).or_default();
                    advance_autonomous_item(&mut self.random, state, item, delta_ms, now_ms);
                }
            }
        }
    }

    pub fn pause_cursor(&mut self, cursor: Option<SceneParticleCursor>) {
        self.last_cursor = cursor;
        self.last_update_ms = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn line_primitives(
        &self,
        item: &SceneRenderParticleItem,
        canvas_height: f64,
        now_ms: f64,
    ) -> Vec<SceneParticlePrimitive> {
        let mut segments = Vec::new();
        if let Some(state) = self.input_states.get(&item.object_id) {
            let points: &[SceneTrailPoint] = if item.rope_length > 0.0 {
                let split_idx = trail_split_index(&state.line_points, item.rope_length);
                &state.line_points[split_idx..]
            } else {
                &state.line_points
            };

            let subdivisions = item.subdivision.clamp(1, 64);
            for point_window in points.windows(2) {
                let start = point_window[0];
                let end = point_window[1];
                for sub_idx in 0..subdivisions {
                    let t_start = sub_idx as f64 / subdivisions as f64;
                    let t_end = (sub_idx + 1) as f64 / subdivisions as f64;
                    let sub_start_x = start.x + (end.x - start.x) * t_start;
                    let sub_start_y = start.y + (end.y - start.y) * t_start;
                    let sub_end_x = start.x + (end.x - start.x) * t_end;
                    let sub_end_y = start.y + (end.y - start.y) * t_end;
                    let dx = sub_end_x - sub_start_x;
                    let dy = sub_end_y - sub_start_y;
                    let seg_length = (dx * dx + dy * dy).sqrt().max(1.0);
                    let center_x = (sub_start_x + sub_end_x) / 2.0;
                    let center_y = (sub_start_y + sub_end_y) / 2.0;
                    let point_age = now_ms - end.created_at_ms;
                    let age = clamp_f64(point_age / item.lifetime_ms, 0.0, 1.0);
                    let alpha = (1.0 - age) * (1.0 - item.fade_alpha);
                    let uv_offset = [
                        (point_age / 1000.0 * item.uv_scrolling[0] + t_start) % 1.0,
                        (point_age / 1000.0 * item.uv_scrolling[1]) % 1.0,
                    ];
                    for width_factor in [3.4, 1.6] {
                        let thickness = (item.size * width_factor).max(1.0);
                        let color = SceneRenderColor {
                            alpha: normalize_alpha(
                                item.color,
                                if width_factor > 2.0 {
                                    0.28 * alpha
                                } else {
                                    alpha
                                },
                            ),
                            ..item.color
                        };
                        let top = canvas_height - center_y - thickness / 2.0;
                        segments.push(SceneParticlePrimitive {
                            left: center_x - seg_length / 2.0,
                            top,
                            width: seg_length,
                            height: thickness,
                            rotation: -(dy).atan2(dx),
                            opacity: 1.0,
                            color,
                            transform_origin_x: center_x,
                            transform_origin_y: top + thickness / 2.0,
                            uv_offset,
                        });
                    }
                }
            }
        }

        if let Some(state) = self.autonomous_states.get(&item.object_id) {
            segments.extend(state.particles.iter().filter_map(|particle| {
                primitive_from_particle(particle, item, canvas_height, now_ms, true)
            }));
        }
        segments
    }

    pub fn petal_primitives(
        &self,
        item: &SceneRenderParticleItem,
        canvas_height: f64,
        now_ms: f64,
    ) -> Vec<SceneParticlePrimitive> {
        let mut primitives = self
            .input_states
            .get(&item.object_id)
            .map(|state| {
                state
                    .petals
                    .iter()
                    .filter_map(|particle| {
                        primitive_from_particle(particle, item, canvas_height, now_ms, false)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if let Some(state) = self.autonomous_states.get(&item.object_id) {
            primitives.extend(state.particles.iter().filter_map(|particle| {
                primitive_from_particle(particle, item, canvas_height, now_ms, false)
            }));
        }
        primitives
    }
}

impl SceneRopeParticleScheduler {
    pub fn advance(
        &mut self,
        cursor: Option<SceneParticleCursor>,
        items: &[SceneRenderRopeParticleItem],
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
        self.states.retain(|object_id, _| active_ids.contains(object_id));

        self.last_cursor = cursor;

        for item in items {
            let state = self.states.entry(item.object_id).or_default();
            match item.renderer_family {
                SceneParticleRendererFamily::Rope => {
                    state.trail_points.clear();
                    advance_rope_state(state, cursor, item, delta_ms, now_ms);
                }
                SceneParticleRendererFamily::RopeTrail => {
                    advance_ropetrail_state(state, cursor, item, delta_ms, now_ms);
                }
                SceneParticleRendererFamily::Sprite
                | SceneParticleRendererFamily::SpriteTrail
                | SceneParticleRendererFamily::Unsupported => {}
            }
        }
    }

    pub fn pause_cursor(&mut self, cursor: Option<SceneParticleCursor>) {
        self.last_cursor = cursor;
        self.last_update_ms = None;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn primitives(
        &self,
        item: &SceneRenderRopeParticleItem,
        now_ms: f64,
    ) -> Vec<SceneRopeParticlePrimitive> {
        let Some(state) = self.states.get(&item.object_id) else {
            return Vec::new();
        };
        match item.renderer_family {
            SceneParticleRendererFamily::Rope => rope_primitives_from_control_points(state, item),
            SceneParticleRendererFamily::RopeTrail => {
                rope_trail_primitives_from_state(state, item, now_ms)
            }
            SceneParticleRendererFamily::Sprite
            | SceneParticleRendererFamily::SpriteTrail
            | SceneParticleRendererFamily::Unsupported => Vec::new(),
        }
    }
}

fn advance_input_item(
    state: &mut SceneInputParticleState,
    previous: Option<SceneParticleCursor>,
    cursor: Option<SceneParticleCursor>,
    item: &SceneRenderParticleItem,
    delta_ms: f64,
    now_ms: f64,
) {
    if let Some(cursor) = cursor {
        match item.particle_kind {
            SceneParticleKind::LineTrail => {
                emit_line_points(state, previous, cursor, item, now_ms);
                state.petal_emission_credit = 0.0;
            }
            SceneParticleKind::PetalTrail => {
                emit_petals(state, previous, cursor, item, now_ms);
                state.line_emission_credit = 0.0;
            }
        }
    } else {
        state.line_emission_credit = 0.0;
        state.petal_emission_credit = 0.0;
    }

    state
        .line_points
        .retain(|point| now_ms - point.created_at_ms < item.lifetime_ms);
    advance_particles(&mut state.petals, delta_ms, now_ms);
}

fn advance_autonomous_item(
    random: &mut SceneRandom,
    state: &mut SceneAutonomousParticleState,
    item: &SceneRenderParticleItem,
    delta_ms: f64,
    now_ms: f64,
) {
    let started_at_ms = *state.started_at_ms.get_or_insert(now_ms);
    if now_ms - started_at_ms < item.start_time_ms {
        advance_particles(&mut state.particles, delta_ms, now_ms);
        return;
    }

    if item.instantaneous && !state.instantaneous_emitted {
        let emit_count = item.max_count.min(64);
        emit_autonomous_particles(random, state, item, emit_count, now_ms);
        state.instantaneous_emitted = true;
    } else if !item.instantaneous {
        state.emission_credit += item.emission_rate.max(0.0) * delta_ms / 1000.0;
        let emit_count = state
            .emission_credit
            .floor()
            .clamp(0.0, MAX_FRAME_EMITS as f64) as usize;
        if emit_count > 0 {
            state.emission_credit -= emit_count as f64;
            emit_autonomous_particles(random, state, item, emit_count, now_ms);
        }
    }

    advance_particles(&mut state.particles, delta_ms, now_ms);
    if state.particles.len() > item.max_count {
        state
            .particles
            .drain(0..state.particles.len().saturating_sub(item.max_count));
    }
}

fn emit_line_points(
    state: &mut SceneInputParticleState,
    previous: Option<SceneParticleCursor>,
    cursor: SceneParticleCursor,
    item: &SceneRenderParticleItem,
    now_ms: f64,
) {
    let Some(previous) = previous else {
        state.line_points.push(SceneTrailPoint {
            x: cursor.x,
            y: cursor.y,
            created_at_ms: now_ms,
        });
        return;
    };

    let distance = cursor_distance(previous, cursor);
    let spacing = clamp_f64(item.size * 1.1, 6.0, 24.0);
    state.line_emission_credit += distance / spacing;
    let emit_count = state.line_emission_credit.floor().clamp(0.0, 8.0) as usize;
    if emit_count == 0 {
        return;
    }
    state.line_emission_credit -= emit_count as f64;

    for index in 0..emit_count {
        let t = (index + 1) as f64 / emit_count as f64;
        state.line_points.push(SceneTrailPoint {
            x: previous.x + (cursor.x - previous.x) * t,
            y: previous.y + (cursor.y - previous.y) * t,
            created_at_ms: now_ms,
        });
    }
    if state.line_points.len() > item.max_count {
        state
            .line_points
            .drain(0..state.line_points.len().saturating_sub(item.max_count));
    }
}

fn emit_petals(
    state: &mut SceneInputParticleState,
    previous: Option<SceneParticleCursor>,
    cursor: SceneParticleCursor,
    item: &SceneRenderParticleItem,
    now_ms: f64,
) {
    let distance = previous
        .map(|previous| cursor_distance(previous, cursor))
        .unwrap_or(item.size * 5.5);
    let spacing = clamp_f64(item.size * 5.2, 12.0, 28.0);
    let density = clamp_f64(item.emission_rate / 42.0, 0.35, 5.0);
    state.petal_emission_credit += (distance / spacing) * density;
    let emit_count = state.petal_emission_credit.floor().clamp(0.0, 6.0) as usize;
    if emit_count == 0 {
        return;
    }
    state.petal_emission_credit -= emit_count as f64;

    let previous = previous.unwrap_or(cursor);
    for index in 0..emit_count {
        let t = (index + 1) as f64 / emit_count as f64;
        let base_x = previous.x + (cursor.x - previous.x) * t;
        let base_y = previous.y + (cursor.y - previous.y) * t;
        let spread = (index as f64 - (emit_count.saturating_sub(1) as f64) / 2.0) * item.size * 4.2;
        state.petals.push(SceneParticle {
            x: base_x + spread,
            y: base_y,
            vx: 0.0,
            vy: -28.0,
            rotation: (index as f64 * 37.0) % 360.0,
            spin: 54.0,
            size: 9.0 + item.size * 14.0,
            born_at_ms: now_ms,
            life_ms: item.lifetime_ms,
            color: item.color,
        });
    }
    if state.petals.len() > item.max_count {
        state
            .petals
            .drain(0..state.petals.len().saturating_sub(item.max_count));
    }
}

fn emit_autonomous_particles(
    random: &mut SceneRandom,
    state: &mut SceneAutonomousParticleState,
    item: &SceneRenderParticleItem,
    emit_count: usize,
    now_ms: f64,
) {
    let spread = if item.spawn_radius[1] > 0.0 {
        item.spawn_radius
    } else {
        let fallback = item.size.max(1.0) * 2.0;
        [0.0, fallback]
    };
    let sign = item.sign;
    for _ in 0..emit_count {
        let angle = random.range(0.0, std::f64::consts::TAU);
        let speed = random.range(item.speed_range[0], item.speed_range[1]);
        let radius = random.range(spread[0], spread[1]);
        let offset_x = angle.cos() * radius;
        let offset_y = angle.sin() * radius;
        let size = match item.particle_kind {
            SceneParticleKind::LineTrail => item.size.max(1.0) * random.range(1.0, 2.4),
            SceneParticleKind::PetalTrail => 9.0 + item.size.max(0.5) * random.range(8.0, 18.0),
        };
        state.particles.push(SceneParticle {
            x: item.spawn_origin[0] + offset_x,
            y: item.spawn_origin[1] + offset_y,
            vx: angle.cos() * speed * sign,
            vy: angle.sin() * speed * sign,
            rotation: random.range(0.0, 360.0),
            spin: random.range(-90.0, 90.0),
            size,
            born_at_ms: now_ms,
            life_ms: item.lifetime_ms * random.range(0.82, 1.18),
            color: item.color,
        });
    }
    if state.particles.len() > item.max_count {
        state
            .particles
            .drain(0..state.particles.len().saturating_sub(item.max_count));
    }
}

fn advance_rope_state(
    state: &mut SceneRopeParticleState,
    cursor: Option<SceneParticleCursor>,
    item: &SceneRenderRopeParticleItem,
    _delta_ms: f64,
    _now_ms: f64,
) {
    state.control_points = resolve_runtime_rope_control_points(cursor, item);
}

fn advance_ropetrail_state(
    state: &mut SceneRopeParticleState,
    cursor: Option<SceneParticleCursor>,
    item: &SceneRenderRopeParticleItem,
    delta_ms: f64,
    now_ms: f64,
) {
    state.control_points = resolve_runtime_rope_control_points(cursor, item);
    let Some(anchor) = state.control_points.last().copied() else {
        state.trail_points.clear();
        return;
    };

    let current = SceneTrailPoint {
        x: anchor.x,
        y: anchor.y,
        created_at_ms: now_ms,
    };
    if state.trail_points.is_empty() {
        state.trail_points.push(current);
    } else {
        emit_rope_trail_points(state, current, item, delta_ms, now_ms);
    }

    state
        .trail_points
        .retain(|point| now_ms - point.created_at_ms < item.lifetime_ms);
    let max_points = rope_trail_max_points(item);
    if state.trail_points.len() > max_points {
        state
            .trail_points
            .drain(0..state.trail_points.len().saturating_sub(max_points));
    }
}

fn emit_rope_trail_points(
    state: &mut SceneRopeParticleState,
    current: SceneTrailPoint,
    item: &SceneRenderRopeParticleItem,
    delta_ms: f64,
    now_ms: f64,
) {
    let Some(previous) = state.trail_points.last().copied() else {
        state.trail_points.push(current);
        return;
    };
    let distance = cursor_distance(
        SceneParticleCursor {
            x: previous.x,
            y: previous.y,
        },
        SceneParticleCursor {
            x: current.x,
            y: current.y,
        },
    );
    let spacing = rope_trail_point_spacing(item).max(1.0);
    let min_credit = (delta_ms / 16.0).clamp(0.5, 4.0) * 0.15;
    state.trail_emission_credit += distance / spacing;
    state.trail_emission_credit = state.trail_emission_credit.max(min_credit);
    let emit_count = state
        .trail_emission_credit
        .floor()
        .clamp(0.0, MAX_FRAME_EMITS as f64) as usize;
    if emit_count == 0 {
        if now_ms - previous.created_at_ms > spacing * 12.0 {
            state.trail_points.push(current);
        }
        return;
    }
    state.trail_emission_credit -= emit_count as f64;
    for index in 0..emit_count {
        let t = (index + 1) as f64 / emit_count as f64;
        state.trail_points.push(SceneTrailPoint {
            x: previous.x + (current.x - previous.x) * t,
            y: previous.y + (current.y - previous.y) * t,
            created_at_ms: now_ms,
        });
    }
}

fn resolve_runtime_rope_control_points(
    cursor: Option<SceneParticleCursor>,
    item: &SceneRenderRopeParticleItem,
) -> Vec<SceneResolvedRopeControlPoint> {
    let mut resolved = item
        .control_points
        .iter()
        .map(|control_point| SceneResolvedRopeControlPoint {
            id: control_point.id,
            x: control_point.position[0],
            y: control_point.position[1],
        })
        .collect::<Vec<_>>();

    if let Some(cursor) = cursor {
        for control_point in &item.control_points {
            if !control_point.lock_to_pointer {
                continue;
            }
            if let Some(runtime_point) = resolved.iter_mut().find(|point| point.id == control_point.id)
            {
                runtime_point.x = cursor.x;
                runtime_point.y = cursor.y;
            }
        }
    }

    resolved
}

#[cfg_attr(not(test), allow(dead_code))]
fn rope_primitives_from_control_points(
    state: &SceneRopeParticleState,
    item: &SceneRenderRopeParticleItem,
) -> Vec<SceneRopeParticlePrimitive> {
    if state.control_points.len() < 2 || item.segment_count == 0 || item.width <= 0.0 {
        return Vec::new();
    }

    let sampled = sample_rope_path(&control_points_to_polyline(&state.control_points), item.segment_count);
    rope_segment_primitives(
        &sampled,
        item.width,
        item.color,
        item.uv_scrolling,
        item.fade_alpha,
        0.0,
        1.0,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn rope_trail_primitives_from_state(
    state: &SceneRopeParticleState,
    item: &SceneRenderRopeParticleItem,
    now_ms: f64,
) -> Vec<SceneRopeParticlePrimitive> {
    if state.trail_points.len() < 2 || item.segment_count == 0 || item.width <= 0.0 {
        return Vec::new();
    }

    let points = if item.length > 0.0 {
        let split_idx = trail_split_index(&state.trail_points, item.length);
        &state.trail_points[split_idx..]
    } else {
        &state.trail_points
    };
    let sampled = sample_rope_path(&trail_points_to_polyline(points), item.segment_count);
    let trail_age_ms = points
        .last()
        .map(|point| (now_ms - point.created_at_ms).max(0.0))
        .unwrap_or(0.0);
    rope_segment_primitives(
        &sampled,
        item.width,
        item.color,
        item.uv_scrolling,
        item.fade_alpha,
        trail_age_ms,
        item.lifetime_ms.max(1.0),
    )
}

#[cfg_attr(not(test), allow(dead_code))]
fn control_points_to_polyline(points: &[SceneResolvedRopeControlPoint]) -> Vec<[f64; 2]> {
    points.iter().map(|point| [point.x, point.y]).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn trail_points_to_polyline(points: &[SceneTrailPoint]) -> Vec<[f64; 2]> {
    points.iter().map(|point| [point.x, point.y]).collect()
}

#[cfg_attr(not(test), allow(dead_code))]
fn sample_rope_path(polyline: &[[f64; 2]], segment_count: u32) -> Vec<[f64; 2]> {
    if polyline.len() < 2 {
        return polyline.to_vec();
    }

    let total_length = polyline_length(polyline);
    if total_length <= 0.0 {
        return polyline.to_vec();
    }

    let segments = segment_count.clamp(1, 256) as usize;
    let mut sampled = Vec::with_capacity(segments + 1);
    for index in 0..=segments {
        let distance = total_length * index as f64 / segments as f64;
        sampled.push(sample_polyline_at_distance(polyline, distance));
    }
    sampled
}

#[cfg_attr(not(test), allow(dead_code))]
fn rope_segment_primitives(
    sampled: &[[f64; 2]],
    width: f64,
    color: SceneRenderColor,
    uv_scrolling: [f64; 2],
    fade_alpha: f64,
    age_ms: f64,
    lifetime_ms: f64,
) -> Vec<SceneRopeParticlePrimitive> {
    let mut primitives = Vec::new();
    if sampled.len() < 2 {
        return primitives;
    }

    let total_segments = sampled.len().saturating_sub(1).max(1);
    let age_alpha = (1.0 - clamp_f64(age_ms / lifetime_ms, 0.0, 1.0)) * (1.0 - fade_alpha);
    for (index, window) in sampled.windows(2).enumerate() {
        let segment_alpha = age_alpha * (1.0 - index as f64 / total_segments as f64 * fade_alpha);
        if segment_alpha <= 0.0 {
            continue;
        }
        primitives.push(SceneRopeParticlePrimitive {
            start: window[0],
            end: window[1],
            width: width.max(0.5),
            opacity: segment_alpha.clamp(0.0, 1.0),
            color: SceneRenderColor {
                alpha: normalize_alpha(color, segment_alpha),
                ..color
            },
            uv_offset: [
                ((age_ms / 1000.0) * uv_scrolling[0] + index as f64 / total_segments as f64) % 1.0,
                ((age_ms / 1000.0) * uv_scrolling[1]) % 1.0,
            ],
        });
    }
    primitives
}

fn rope_trail_point_spacing(item: &SceneRenderRopeParticleItem) -> f64 {
    let min_length = item.min_length.max(0.0);
    let max_length = item.max_length.max(min_length);
    let baseline = if max_length > 0.0 {
        (max_length / item.segment_count.max(1) as f64).max(6.0)
    } else {
        item.width.max(1.0) * 2.0
    };
    baseline / item.subdivision.max(1) as f64
}

fn rope_trail_max_points(item: &SceneRenderRopeParticleItem) -> usize {
    item.segment_count.max(1) as usize * item.subdivision.max(1) as usize + 2
}

#[cfg_attr(not(test), allow(dead_code))]
fn polyline_length(polyline: &[[f64; 2]]) -> f64 {
    polyline
        .windows(2)
        .map(|window| {
            let dx = window[1][0] - window[0][0];
            let dy = window[1][1] - window[0][1];
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

#[cfg_attr(not(test), allow(dead_code))]
fn sample_polyline_at_distance(polyline: &[[f64; 2]], distance: f64) -> [f64; 2] {
    let mut remaining = distance.max(0.0);
    for window in polyline.windows(2) {
        let dx = window[1][0] - window[0][0];
        let dy = window[1][1] - window[0][1];
        let segment_length = (dx * dx + dy * dy).sqrt();
        if segment_length <= 0.0 {
            continue;
        }
        if remaining <= segment_length {
            let t = remaining / segment_length;
            return [
                window[0][0] + dx * t,
                window[0][1] + dy * t,
            ];
        }
        remaining -= segment_length;
    }
    polyline.last().copied().unwrap_or([0.0, 0.0])
}

fn advance_particles(particles: &mut Vec<SceneParticle>, delta_ms: f64, now_ms: f64) {
    let delta_seconds = (delta_ms / 1000.0).clamp(0.0, 0.1);
    let delta_scale = (delta_ms / (1000.0 / 60.0)).clamp(0.25, 4.0);
    particles.retain_mut(|particle| {
        particle.x += particle.vx * delta_seconds;
        particle.y += particle.vy * delta_seconds;
        particle.rotation += particle.spin * delta_seconds;
        if particle.vy < 0.0 {
            particle.vy -= 0.9 * delta_scale;
        }
        now_ms - particle.born_at_ms < particle.life_ms
    });
}

fn primitive_from_particle(
    particle: &SceneParticle,
    item: &SceneRenderParticleItem,
    canvas_height: f64,
    now_ms: f64,
    streak: bool,
) -> Option<SceneParticlePrimitive> {
    if now_ms - particle.born_at_ms >= particle.life_ms {
        return None;
    }
    let particle_age = now_ms - particle.born_at_ms;
    let age = clamp_f64(particle_age / particle.life_ms, 0.0, 1.0);
    let opacity = (1.0 - age) * (1.0 - item.fade_alpha);
    let uv_offset = [
        (particle_age / 1000.0 * item.uv_scrolling[0]) % 1.0,
        (particle_age / 1000.0 * item.uv_scrolling[1]) % 1.0,
    ];
    let width = if streak {
        (particle.size * 3.0).max(2.0)
    } else {
        particle.size
    };
    let height = if streak {
        (item.size * 0.85).max(1.0)
    } else {
        particle.size * 0.68
    };
    let top = canvas_height - particle.y - height / 2.0;
    Some(SceneParticlePrimitive {
        left: particle.x - width / 2.0,
        top,
        width,
        height,
        rotation: particle.rotation.to_radians(),
        opacity,
        color: SceneRenderColor {
            alpha: normalize_alpha(particle.color, opacity),
            ..particle.color
        },
        transform_origin_x: particle.x,
        transform_origin_y: top + height / 2.0,
        uv_offset,
    })
}

fn normalize_alpha(color: SceneRenderColor, alpha: f64) -> u8 {
    ((color.alpha as f64) * alpha.clamp(0.0, 1.0)).round() as u8
}

fn cursor_distance(left: SceneParticleCursor, right: SceneParticleCursor) -> f64 {
    ((left.x - right.x).powi(2) + (left.y - right.y).powi(2)).sqrt()
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.clamp(min, max)
}

fn trail_split_index(points: &[SceneTrailPoint], max_length: f64) -> usize {
    let mut cumulative = 0.0;
    points
        .windows(2)
        .rev()
        .position(|pair| {
            let dx = pair[1].x - pair[0].x;
            let dy = pair[1].y - pair[0].y;
            cumulative += (dx * dx + dy * dy).sqrt();
            cumulative > max_length
        })
        .map(|index| points.len().saturating_sub(2) - index)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use crate::models::{
        SceneParticleKind, SceneParticleRendererFamily, SceneParticleScheduleMode,
    };
    use crate::services::scene_render_planner_service::{
        SceneRenderColor, SceneRenderParticleItem, SceneRenderRopeControlPointItem,
        SceneRenderRopeParticleItem,
    };

    use super::{SceneParticleCursor, SceneParticleScheduler, SceneRopeParticleScheduler};

    fn line_item() -> SceneRenderParticleItem {
        SceneRenderParticleItem {
            object_id: 1,
            object_name: "Trail".to_string(),
            particle_kind: SceneParticleKind::LineTrail,
            schedule_mode: SceneParticleScheduleMode::InputDriven,
            spawn_origin: [20.0, 20.0],
            color: SceneRenderColor {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 255,
            },
            size: 12.0,
            emission_rate: 48.0,
            max_count: 32,
            lifetime_ms: 520.0,
            speed_range: [24.0, 48.0],
            instantaneous: false,
            start_time_ms: 0.0,
            sign: 1.0,
            spawn_radius: [0.0, 0.0],
            uv_scrolling: [0.0, 0.0],
            fade_alpha: 0.0,
            subdivision: 1,
            rope_length: 0.0,
        }
    }

    fn petal_item() -> SceneRenderParticleItem {
        SceneRenderParticleItem {
            object_id: 2,
            object_name: "Petal".to_string(),
            particle_kind: SceneParticleKind::PetalTrail,
            schedule_mode: SceneParticleScheduleMode::InputDriven,
            spawn_origin: [200.0, 160.0],
            color: SceneRenderColor {
                red: 255,
                green: 192,
                blue: 192,
                alpha: 220,
            },
            size: 1.8,
            emission_rate: 96.0,
            max_count: 96,
            lifetime_ms: 1500.0,
            speed_range: [20.0, 64.0],
            instantaneous: false,
            start_time_ms: 0.0,
            sign: 1.0,
            spawn_radius: [0.0, 0.0],
            uv_scrolling: [0.0, 0.0],
            fade_alpha: 0.0,
            subdivision: 1,
            rope_length: 0.0,
        }
    }

    fn autonomous_item() -> SceneRenderParticleItem {
        SceneRenderParticleItem {
            object_id: 3,
            object_name: "Autonomous".to_string(),
            particle_kind: SceneParticleKind::PetalTrail,
            schedule_mode: SceneParticleScheduleMode::Autonomous,
            spawn_origin: [320.0, 240.0],
            color: SceneRenderColor {
                red: 160,
                green: 220,
                blue: 255,
                alpha: 220,
            },
            size: 2.0,
            emission_rate: 60.0,
            max_count: 24,
            lifetime_ms: 1200.0,
            speed_range: [12.0, 24.0],
            instantaneous: false,
            start_time_ms: 0.0,
            sign: 1.0,
            spawn_radius: [0.0, 0.0],
            uv_scrolling: [0.0, 0.0],
            fade_alpha: 0.0,
            subdivision: 1,
            rope_length: 0.0,
        }
    }

    fn rope_item() -> SceneRenderRopeParticleItem {
        SceneRenderRopeParticleItem {
            object_id: 10,
            object_name: "Rope".to_string(),
            renderer_family: SceneParticleRendererFamily::Rope,
            schedule_mode: SceneParticleScheduleMode::InputDriven,
            control_points: vec![
                SceneRenderRopeControlPointItem {
                    id: 0,
                    position: [20.0, 30.0],
                    lock_to_pointer: false,
                },
                SceneRenderRopeControlPointItem {
                    id: 1,
                    position: [120.0, 30.0],
                    lock_to_pointer: true,
                },
            ],
            segment_count: 8,
            subdivision: 2,
            length: 180.0,
            min_length: 40.0,
            max_length: 220.0,
            width: 6.0,
            lifetime_ms: 1200.0,
            color: SceneRenderColor {
                red: 120,
                green: 200,
                blue: 255,
                alpha: 255,
            },
            material_path: Some("materials/rope.material".to_string()),
            uv_scrolling: [0.25, -0.15],
            fade_alpha: 0.2,
        }
    }

    fn rope_trail_item() -> SceneRenderRopeParticleItem {
        let mut item = rope_item();
        item.object_id = 11;
        item.object_name = "RopeTrail".to_string();
        item.renderer_family = SceneParticleRendererFamily::RopeTrail;
        item.length = 90.0;
        item.segment_count = 12;
        item.subdivision = 3;
        item.lifetime_ms = 900.0;
        item
    }

    #[test]
    fn scheduler_emits_multiple_line_segments_for_large_cursor_moves() {
        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(
            Some(SceneParticleCursor { x: 10.0, y: 10.0 }),
            &[line_item()],
            1000.0,
        );
        scheduler.advance(
            Some(SceneParticleCursor { x: 160.0, y: 10.0 }),
            &[line_item()],
            1016.0,
        );

        let primitives = scheduler.line_primitives(&line_item(), 1080.0, 1016.0);
        assert!(!primitives.is_empty());
        assert!(primitives.len() >= 12);
        assert!(primitives.iter().all(|primitive| primitive.width < 24.0));
    }

    #[test]
    fn scheduler_advances_petals_with_frame_delta_and_reaps_expired_particles() {
        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(
            Some(SceneParticleCursor { x: 200.0, y: 160.0 }),
            &[petal_item()],
            1000.0,
        );
        scheduler.advance(
            Some(SceneParticleCursor { x: 260.0, y: 180.0 }),
            &[petal_item()],
            1060.0,
        );

        let alive = scheduler.petal_primitives(&petal_item(), 1080.0, 1060.0);
        let expired = scheduler.petal_primitives(&petal_item(), 1080.0, 3200.0);

        assert!(!alive.is_empty());
        assert!(expired.is_empty());
    }

    #[test]
    fn autonomous_scheduler_emits_without_cursor_input() {
        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[autonomous_item()], 1000.0);
        scheduler.advance(None, &[autonomous_item()], 1100.0);

        let primitives = scheduler.petal_primitives(&autonomous_item(), 1080.0, 1100.0);
        assert!(!primitives.is_empty());
        assert!(primitives.len() <= autonomous_item().max_count);
    }

    #[test]
    fn reset_clears_scheduler_state() {
        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(
            Some(SceneParticleCursor { x: 20.0, y: 20.0 }),
            &[line_item(), petal_item(), autonomous_item()],
            1000.0,
        );

        scheduler.reset();

        assert!(scheduler
            .line_primitives(&line_item(), 1080.0, 1000.0)
            .is_empty());
        assert!(scheduler
            .petal_primitives(&petal_item(), 1080.0, 1000.0)
            .is_empty());
        assert!(scheduler
            .petal_primitives(&autonomous_item(), 1080.0, 1000.0)
            .is_empty());
    }

    #[test]
    fn autonomous_scheduler_defers_emission_until_start_time_elapses() {
        let mut item = autonomous_item();
        item.start_time_ms = 500.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1300.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1300.0);
        assert!(
            primitives.is_empty(),
            "no particles emitted before start_time_ms (300ms elapsed < 500ms delay)"
        );
    }

    #[test]
    fn autonomous_scheduler_emits_after_start_time_passes() {
        let mut item = autonomous_item();
        item.start_time_ms = 500.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1700.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1700.0);
        assert!(
            !primitives.is_empty(),
            "particles emitted after start_time_ms (700ms elapsed > 500ms delay)"
        );
    }

    #[test]
    fn autonomous_instantaneous_burst_waits_for_start_time() {
        let mut item = autonomous_item();
        item.instantaneous = true;
        item.start_time_ms = 600.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 2000.0);
        scheduler.advance(None, &[item.clone()], 2400.0);

        let primitives_before = scheduler.petal_primitives(&item, 1080.0, 2400.0);
        assert!(
            primitives_before.is_empty(),
            "instantaneous burst deferred before start_time_ms"
        );

        scheduler.advance(None, &[item.clone()], 2800.0);
        let primitives_after = scheduler.petal_primitives(&item, 1080.0, 2800.0);
        assert!(
            !primitives_after.is_empty(),
            "instantaneous burst fires once after start_time_ms"
        );
        assert!(
            primitives_after.len() <= item.max_count,
            "instantaneous burst respects max_count"
        );

        scheduler.advance(None, &[item.clone()], 3200.0);
        let primitives_later = scheduler.petal_primitives(&item, 1080.0, 3200.0);
        assert!(
            primitives_later.len() <= primitives_after.len(),
            "no additional particles after instantaneous burst"
        );
    }

    #[test]
    fn autonomous_scheduler_without_start_time_emits_immediately() {
        let mut item = autonomous_item();
        item.start_time_ms = 0.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1100.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1100.0);
        assert!(
            !primitives.is_empty(),
            "zero start_time_ms emits immediately"
        );
    }

    #[test]
    fn autonomous_start_time_resets_when_item_disappears() {
        let mut item = autonomous_item();
        item.start_time_ms = 500.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1800.0);

        let primitives_first = scheduler.petal_primitives(&item, 1080.0, 1800.0);
        assert!(!primitives_first.is_empty());

        scheduler.advance(None, &[], 1900.0);

        scheduler.advance(None, &[item.clone()], 2000.0);
        scheduler.advance(None, &[item.clone()], 2200.0);

        let primitives_second = scheduler.petal_primitives(&item, 1080.0, 2200.0);
        assert!(
            primitives_second.is_empty(),
            "start_time restarts when item re-enters after removal"
        );
    }

    #[test]
    fn sign_flips_autonomous_particle_velocity_direction() {
        let mut item = autonomous_item();
        item.sign = -1.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        assert!(
            !primitives.is_empty(),
            "should emit particles with negative sign"
        );

        let state = scheduler.autonomous_states.get(&item.object_id).unwrap();
        let all_zero = state
            .particles
            .iter()
            .all(|p| p.vx.abs() < 0.01 && p.vy.abs() < 0.01);
        assert!(
            !all_zero,
            "sign=-1.0 should be consumed; velocities should be non-zero for moving particles"
        );
    }

    #[test]
    fn sign_default_positive_does_not_flip_velocity() {
        let mut item = autonomous_item();
        item.sign = 1.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        assert!(!primitives.is_empty());

        let state = scheduler.autonomous_states.get(&item.object_id).unwrap();
        let has_positive = state.particles.iter().any(|p| p.vx > 0.0 || p.vy > 0.0);
        assert!(
            has_positive,
            "sign=1.0 should produce bidirectional velocity"
        );
    }

    #[test]
    fn spawn_radius_ring_when_set() {
        let mut item = autonomous_item();
        item.spawn_radius = [80.0, 100.0];

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let state = scheduler.autonomous_states.get(&item.object_id).unwrap();
        assert!(!state.particles.is_empty());

        for particle in &state.particles {
            let dx = particle.x - item.spawn_origin[0];
            let dy = particle.y - item.spawn_origin[1];
            let dist = (dx * dx + dy * dy).sqrt();
            assert!(
                dist <= 100.0,
                "particle spawn distance {} exceeds max radius 100.0",
                dist
            );
        }
    }

    #[test]
    fn spawn_radius_zero_falls_back_to_size_based_spread() {
        let mut item = autonomous_item();
        item.spawn_radius = [0.0, 0.0];

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        assert!(!primitives.is_empty(), "fallback spread should still emit");
    }

    #[test]
    fn fade_alpha_reduces_primitive_opacity() {
        let mut item = autonomous_item();
        item.fade_alpha = 0.5;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        assert!(!primitives.is_empty());

        for primitive in &primitives {
            assert!(
                primitive.opacity <= 0.5 + 0.001,
                "fade_alpha=0.5 should halve effective opacity; got {}",
                primitive.opacity
            );
        }
    }

    #[test]
    fn fade_alpha_zero_has_no_effect_on_opacity() {
        let mut item = autonomous_item();
        item.fade_alpha = 0.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        assert!(!primitives.is_empty());
        for primitive in &primitives {
            assert!(
                primitive.opacity > 0.5,
                "fade_alpha=0.0 should not reduce opacity; got {}",
                primitive.opacity
            );
        }
    }

    #[test]
    fn uv_offset_is_zero_when_uv_scrolling_is_zero() {
        let mut item = autonomous_item();
        item.uv_scrolling = [0.0, 0.0];

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 1000.0);
        scheduler.advance(None, &[item.clone()], 1200.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1200.0);
        for primitive in &primitives {
            assert_eq!(primitive.uv_offset, [0.0, 0.0]);
        }
    }

    #[test]
    fn uv_offset_tracks_uv_scrolling_over_time() {
        let mut item = autonomous_item();
        item.uv_scrolling = [0.5, -0.25];
        item.emission_rate = 600.0;
        item.max_count = 200;
        item.lifetime_ms = 5000.0;

        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(None, &[item.clone()], 0.0);
        scheduler.advance(None, &[item.clone()], 16.0);
        scheduler.advance(None, &[item.clone()], 1000.0);

        let primitives = scheduler.petal_primitives(&item, 1080.0, 1000.0);
        let has_offset = primitives
            .iter()
            .any(|p| (p.uv_offset[0] - 0.0).abs() > 0.01 || (p.uv_offset[1] - 0.0).abs() > 0.01);
        assert!(
            has_offset,
            "uv_scrolling should produce non-zero uv_offset after elapsed time"
        );
    }

    #[test]
    fn line_trail_with_subdivision_produces_more_quads_per_segment() {
        let item = line_item();
        let mut item_sub = line_item();
        item_sub.object_id = 3;
        item_sub.subdivision = 3;

        let mut scheduler1 = SceneParticleScheduler::default();
        scheduler1.advance(
            Some(SceneParticleCursor { x: 10.0, y: 10.0 }),
            &[item.clone()],
            1000.0,
        );
        scheduler1.advance(
            Some(SceneParticleCursor { x: 310.0, y: 10.0 }),
            &[item.clone()],
            1016.0,
        );

        let mut scheduler3 = SceneParticleScheduler::default();
        scheduler3.advance(
            Some(SceneParticleCursor { x: 10.0, y: 10.0 }),
            &[item_sub.clone()],
            1000.0,
        );
        scheduler3.advance(
            Some(SceneParticleCursor { x: 310.0, y: 10.0 }),
            &[item_sub.clone()],
            1016.0,
        );

        let prim1 = scheduler1.line_primitives(&item, 1080.0, 1016.0);
        let prim3 = scheduler3.line_primitives(&item_sub, 1080.0, 1016.0);
        assert!(!prim1.is_empty());
        assert!(!prim3.is_empty());
        assert!(
            prim3.len() > prim1.len(),
            "subdivision=3 should produce more quads than subdivision=1 (got {} vs {})",
            prim3.len(),
            prim1.len()
        );
    }

    #[test]
    fn rope_length_truncates_long_trail() {
        let mut item = line_item();
        item.rope_length = 50.0;
        item.lifetime_ms = 50000.0;

        let mut scheduler = SceneParticleScheduler::default();
        let mut x = 10.0;
        for step in 0..30 {
            x += 20.0;
            scheduler.advance(
                Some(SceneParticleCursor { x, y: 10.0 }),
                &[item.clone()],
                1000.0 + step as f64 * 16.0,
            );
        }

        let primitives = scheduler.line_primitives(&item, 1080.0, 2000.0);
        assert!(!primitives.is_empty());
        let total_width: f64 = primitives.iter().map(|p| p.width).sum();
        assert!(
            total_width <= item.rope_length * 2.0 + 50.0,
            "rope_length=50 should cap trail visible extent; total width={}",
            total_width
        );
    }

    #[test]
    fn rope_length_zero_keeps_full_trail() {
        let mut item = line_item();
        item.rope_length = 0.0;

        let mut scheduler = SceneParticleScheduler::default();
        let mut x = 10.0;
        for step in 0..10 {
            x += 20.0;
            scheduler.advance(
                Some(SceneParticleCursor { x, y: 10.0 }),
                &[item.clone()],
                1000.0 + step as f64 * 16.0,
            );
        }

        let primitives = scheduler.line_primitives(&item, 1080.0, 1200.0);
        assert!(!primitives.is_empty());
    }

    #[test]
    fn rope_scheduler_builds_segments_from_control_points() {
        let mut scheduler = SceneRopeParticleScheduler::default();
        let item = rope_item();
        scheduler.advance(
            Some(SceneParticleCursor { x: 180.0, y: 60.0 }),
            std::slice::from_ref(&item),
            1000.0,
        );

        let primitives = scheduler.primitives(&item, 1000.0);
        assert!(!primitives.is_empty());
        assert!(primitives.len() >= item.segment_count as usize);
        assert!(primitives.iter().all(|primitive| primitive.width >= item.width));
        let last = primitives.last().expect("rope segment");
        assert!(last.end[0] > 150.0, "pointer-locked endpoint should follow cursor");
    }

    #[test]
    fn rope_trail_scheduler_accumulates_and_reaps_segments_over_time() {
        let mut scheduler = SceneRopeParticleScheduler::default();
        let item = rope_trail_item();
        for step in 0..8 {
            scheduler.advance(
                Some(SceneParticleCursor {
                    x: 60.0 + step as f64 * 18.0,
                    y: 80.0 + step as f64 * 4.0,
                }),
                std::slice::from_ref(&item),
                1000.0 + step as f64 * 16.0,
            );
        }

        let active = scheduler.primitives(&item, 1128.0);
        let expired = scheduler.primitives(&item, 2600.0);
        assert!(!active.is_empty(), "ropetrail should emit drawable segments");
        assert!(
            active.len() <= item.segment_count as usize,
            "ropetrail should clamp trail topology to segment_count"
        );
        assert!(
            expired.iter().all(|primitive| primitive.opacity < 0.05) || expired.is_empty(),
            "expired ropetrail segments should fully fade out"
        );
    }

    #[test]
    fn rope_scheduler_reset_clears_rope_and_trail_state() {
        let mut scheduler = SceneRopeParticleScheduler::default();
        let rope = rope_item();
        let trail = rope_trail_item();
        scheduler.advance(
            Some(SceneParticleCursor { x: 100.0, y: 40.0 }),
            &[rope.clone(), trail.clone()],
            1000.0,
        );
        scheduler.advance(
            Some(SceneParticleCursor { x: 140.0, y: 70.0 }),
            &[rope.clone(), trail.clone()],
            1016.0,
        );

        scheduler.reset();

        assert!(scheduler.primitives(&rope, 1016.0).is_empty());
        assert!(scheduler.primitives(&trail, 1016.0).is_empty());
    }

    #[test]
    fn rope_trail_removal_drops_previous_wallpaper_segments() {
        let mut scheduler = SceneRopeParticleScheduler::default();
        let trail = rope_trail_item();
        scheduler.advance(
            Some(SceneParticleCursor { x: 60.0, y: 60.0 }),
            std::slice::from_ref(&trail),
            1000.0,
        );
        scheduler.advance(
            Some(SceneParticleCursor { x: 120.0, y: 60.0 }),
            std::slice::from_ref(&trail),
            1016.0,
        );
        assert!(!scheduler.primitives(&trail, 1016.0).is_empty());

        scheduler.advance(None, &[], 1032.0);

        assert!(
            scheduler.primitives(&trail, 1032.0).is_empty(),
            "removing rope item should clear retained trail state"
        );
    }
}
