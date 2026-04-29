use std::collections::{BTreeMap, BTreeSet};

use crate::{
    models::{SceneParticleKind, SceneParticleScheduleMode},
    services::scene_render_planner_service::{SceneRenderColor, SceneRenderParticleItem},
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
struct SceneInputParticleState {
    line_emission_credit: f64,
    petal_emission_credit: f64,
    line_points: Vec<SceneTrailPoint>,
    petals: Vec<SceneParticle>,
}

#[derive(Debug, Default)]
struct SceneAutonomousParticleState {
    emission_credit: f64,
    instantaneous_emitted: bool,
    particles: Vec<SceneParticle>,
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

    fn centered(&mut self) -> f64 {
        self.next_f64() - 0.5
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
            for points in state.line_points.windows(2) {
                let start = points[0];
                let end = points[1];
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let length = (dx * dx + dy * dy).sqrt().max(1.0);
                let center_x = (start.x + end.x) / 2.0;
                let center_y_bottom = (start.y + end.y) / 2.0;
                let age = clamp_f64((now_ms - end.created_at_ms) / item.lifetime_ms, 0.0, 1.0);
                let alpha = 1.0 - age;
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
                    let top = canvas_height - center_y_bottom - thickness / 2.0;
                    segments.push(SceneParticlePrimitive {
                        left: center_x - length / 2.0,
                        top,
                        width: length,
                        height: thickness,
                        rotation: -(dy).atan2(dx),
                        opacity: 1.0,
                        color,
                        transform_origin_x: center_x,
                        transform_origin_y: top + thickness / 2.0,
                    });
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
    for _ in 0..emit_count {
        let angle = random.range(0.0, std::f64::consts::TAU);
        let speed = random.range(item.speed_range[0], item.speed_range[1]);
        let spread = item.size.max(1.0) * 2.0;
        let size = match item.particle_kind {
            SceneParticleKind::LineTrail => item.size.max(1.0) * random.range(1.0, 2.4),
            SceneParticleKind::PetalTrail => 9.0 + item.size.max(0.5) * random.range(8.0, 18.0),
        };
        state.particles.push(SceneParticle {
            x: item.spawn_origin[0] + random.centered() * spread,
            y: item.spawn_origin[1] + random.centered() * spread,
            vx: angle.cos() * speed,
            vy: angle.sin() * speed,
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
    let age = clamp_f64((now_ms - particle.born_at_ms) / particle.life_ms, 0.0, 1.0);
    let opacity = 1.0 - age;
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

#[cfg(test)]
mod tests {
    use crate::models::{SceneParticleKind, SceneParticleScheduleMode};
    use crate::services::scene_render_planner_service::{
        SceneRenderColor, SceneRenderParticleItem,
    };

    use super::{SceneParticleCursor, SceneParticleScheduler};

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
        }
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
}
