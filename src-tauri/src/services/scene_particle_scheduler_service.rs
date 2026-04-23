use crate::{
    models::SceneParticleKind,
    services::scene_render_planner_service::{SceneRenderColor, SceneRenderParticleItem},
};

const MAX_TRAIL_POINTS: usize = 32;
const MAX_PETALS: usize = 96;
const TRAIL_LIFETIME_MS: f64 = 520.0;

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
    line_emission_credit: f64,
    petal_emission_credit: f64,
    line_points: Vec<SceneTrailPoint>,
    petals: Vec<ScenePetalParticle>,
    random: SceneRandom,
}

#[derive(Debug, Clone, Copy)]
struct SceneTrailPoint {
    x: f64,
    y: f64,
    created_at_ms: f64,
}

#[derive(Debug, Clone, Copy)]
struct ScenePetalParticle {
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

        let line_item = items
            .iter()
            .find(|item| item.particle_kind == SceneParticleKind::LineTrail);
        let petal_item = items
            .iter()
            .find(|item| item.particle_kind == SceneParticleKind::PetalTrail);
        let previous = self.last_cursor;
        self.last_cursor = cursor;

        if let Some(cursor) = cursor {
            if let Some(line_item) = line_item {
                self.emit_line_points(previous, cursor, line_item, now_ms);
            } else {
                self.line_points.clear();
                self.line_emission_credit = 0.0;
            }

            if let Some(petal_item) = petal_item {
                self.emit_petals(previous, cursor, petal_item, now_ms);
            } else {
                self.petals.clear();
                self.petal_emission_credit = 0.0;
            }
        } else {
            self.line_emission_credit = 0.0;
            self.petal_emission_credit = 0.0;
        }

        self.line_points
            .retain(|point| now_ms - point.created_at_ms < TRAIL_LIFETIME_MS);
        self.advance_petals(delta_ms, now_ms);
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
        for points in self.line_points.windows(2) {
            let start = points[0];
            let end = points[1];
            let dx = end.x - start.x;
            let dy = end.y - start.y;
            let length = (dx * dx + dy * dy).sqrt().max(1.0);
            let center_x = (start.x + end.x) / 2.0;
            let center_y_bottom = (start.y + end.y) / 2.0;
            let age = clamp_f64((now_ms - end.created_at_ms) / TRAIL_LIFETIME_MS, 0.0, 1.0);
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
        segments
    }

    pub fn petal_primitives(&self, canvas_height: f64, now_ms: f64) -> Vec<SceneParticlePrimitive> {
        self.petals
            .iter()
            .filter(|particle| now_ms - particle.born_at_ms < particle.life_ms)
            .map(|particle| {
                let age = clamp_f64((now_ms - particle.born_at_ms) / particle.life_ms, 0.0, 1.0);
                let top = canvas_height - particle.y - particle.size * 0.34;
                SceneParticlePrimitive {
                    left: particle.x - particle.size / 2.0,
                    top,
                    width: particle.size,
                    height: particle.size * 0.68,
                    rotation: particle.rotation.to_radians(),
                    opacity: 1.0 - age,
                    color: particle.color,
                    transform_origin_x: particle.x,
                    transform_origin_y: top + particle.size * 0.34,
                }
            })
            .collect()
    }

    fn emit_line_points(
        &mut self,
        previous: Option<SceneParticleCursor>,
        cursor: SceneParticleCursor,
        item: &SceneRenderParticleItem,
        now_ms: f64,
    ) {
        let Some(previous) = previous else {
            self.line_points.push(SceneTrailPoint {
                x: cursor.x,
                y: cursor.y,
                created_at_ms: now_ms,
            });
            return;
        };

        let distance = cursor_distance(previous, cursor);
        let spacing = clamp_f64(item.size * 1.1, 6.0, 24.0);
        self.line_emission_credit += distance / spacing;
        let emit_count = self.line_emission_credit.floor().clamp(0.0, 8.0) as usize;
        if emit_count == 0 {
            return;
        }
        self.line_emission_credit -= emit_count as f64;

        for index in 0..emit_count {
            let t = (index + 1) as f64 / emit_count as f64;
            self.line_points.push(SceneTrailPoint {
                x: previous.x + (cursor.x - previous.x) * t,
                y: previous.y + (cursor.y - previous.y) * t,
                created_at_ms: now_ms,
            });
        }
        if self.line_points.len() > MAX_TRAIL_POINTS {
            self.line_points
                .drain(0..self.line_points.len().saturating_sub(MAX_TRAIL_POINTS));
        }
    }

    fn emit_petals(
        &mut self,
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
        self.petal_emission_credit += (distance / spacing) * density;
        let emit_count = self.petal_emission_credit.floor().clamp(0.0, 6.0) as usize;
        if emit_count == 0 {
            return;
        }
        self.petal_emission_credit -= emit_count as f64;

        let previous = previous.unwrap_or(cursor);
        for index in 0..emit_count {
            let t = (index + 1) as f64 / emit_count as f64;
            let base_x = previous.x + (cursor.x - previous.x) * t;
            let base_y = previous.y + (cursor.y - previous.y) * t;
            let spread =
                (index as f64 - (emit_count.saturating_sub(1) as f64) / 2.0) * item.size * 4.2;
            self.petals.push(ScenePetalParticle {
                x: base_x + spread,
                y: base_y + self.random.centered() * 8.0,
                vx: self.random.centered() * 0.9,
                vy: -0.4 - self.random.next_f64() * 0.7,
                rotation: self.random.next_f64() * 360.0,
                spin: self.random.centered() * 1.8,
                size: 9.0 + item.size * (10.0 + self.random.next_f64() * 8.0),
                born_at_ms: now_ms,
                life_ms: 1100.0 + self.random.next_f64() * 600.0,
                color: item.color,
            });
        }
        if self.petals.len() > MAX_PETALS {
            self.petals
                .drain(0..self.petals.len().saturating_sub(MAX_PETALS));
        }
    }

    fn advance_petals(&mut self, delta_ms: f64, now_ms: f64) {
        let delta_scale = (delta_ms / (1000.0 / 60.0)).clamp(0.25, 4.0);
        self.petals = self
            .petals
            .iter()
            .copied()
            .map(|mut particle| {
                particle.x += particle.vx * delta_scale;
                particle.y += particle.vy * delta_scale;
                particle.rotation += particle.spin * delta_scale;
                particle.vy -= 0.015 * delta_scale;
                particle
            })
            .filter(|particle| now_ms - particle.born_at_ms < particle.life_ms)
            .collect();
    }
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
    use crate::models::SceneParticleKind;
    use crate::services::scene_render_planner_service::{
        SceneRenderColor, SceneRenderParticleItem,
    };

    use super::{SceneParticleCursor, SceneParticleScheduler};

    fn line_item() -> SceneRenderParticleItem {
        SceneRenderParticleItem {
            object_id: 1,
            object_name: "Trail".to_string(),
            particle_kind: SceneParticleKind::LineTrail,
            color: SceneRenderColor {
                red: 255,
                green: 255,
                blue: 255,
                alpha: 255,
            },
            size: 12.0,
            emission_rate: 48.0,
        }
    }

    fn petal_item() -> SceneRenderParticleItem {
        SceneRenderParticleItem {
            object_id: 2,
            object_name: "Petal".to_string(),
            particle_kind: SceneParticleKind::PetalTrail,
            color: SceneRenderColor {
                red: 255,
                green: 192,
                blue: 192,
                alpha: 220,
            },
            size: 1.8,
            emission_rate: 96.0,
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

        let alive = scheduler.petal_primitives(1080.0, 1060.0);
        let expired = scheduler.petal_primitives(1080.0, 3200.0);

        assert!(!alive.is_empty());
        assert!(expired.is_empty());
    }

    #[test]
    fn reset_clears_scheduler_state() {
        let mut scheduler = SceneParticleScheduler::default();
        scheduler.advance(
            Some(SceneParticleCursor { x: 20.0, y: 20.0 }),
            &[line_item(), petal_item()],
            1000.0,
        );

        scheduler.reset();

        assert!(scheduler
            .line_primitives(&line_item(), 1080.0, 1000.0)
            .is_empty());
        assert!(scheduler.petal_primitives(1080.0, 1000.0).is_empty());
    }
}
