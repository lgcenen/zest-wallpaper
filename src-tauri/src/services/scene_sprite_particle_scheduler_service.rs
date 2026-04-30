use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use crate::{
    models::SceneParticleChildKind,
    services::scene_render_planner_service::{
        SceneRenderBlendMode, SceneRenderColor, SceneRenderSpriteParticleItem,
        SceneSpriteParticleConfig,
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
    particles: Vec<SceneSpriteParticle>,
}

#[derive(Debug, Clone)]
struct SceneSpriteParticle {
    texture_path: PathBuf,
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
    born_at_ms: f64,
    life_ms: f64,
    color: SceneRenderColor,
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
    pub fn advance(&mut self, items: &[SceneRenderSpriteParticleItem], now_ms: f64) {
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

            let mut root_deaths = advance_emitter_particles(
                &mut self.random,
                &mut state.root,
                &item.config,
                delta_ms,
                now_ms,
            );
            emit_for_config(
                &mut self.random,
                &mut state.root,
                &item.config,
                delta_ms,
                now_ms,
                item.config.spawn_origin,
                None,
            );

            for (child_index, child) in item.children.iter().enumerate() {
                let child_state = &mut state.children[child_index];
                advance_emitter_particles(
                    &mut self.random,
                    child_state,
                    &child.config,
                    delta_ms,
                    now_ms,
                );

                match child.child_type {
                    SceneParticleChildKind::Static => {
                        emit_for_config(
                            &mut self.random,
                            child_state,
                            &child.config,
                            delta_ms,
                            now_ms,
                            child.config.spawn_origin,
                            Some(child.probability),
                        );
                    }
                    SceneParticleChildKind::EventFollow => {
                        for parent in &state.root.particles {
                            if self.random.chance(child.probability / 60.0) {
                                let mut spawned = spawn_particle(
                                    &mut self.random,
                                    &child.config,
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
                            if self.random.chance(child.probability) {
                                child_state.particles.push(spawn_particle(
                                    &mut self.random,
                                    &child.config,
                                    now_ms,
                                    [death[0], death[1]],
                                ));
                            }
                        }
                    }
                    SceneParticleChildKind::EventSpawn | SceneParticleChildKind::Unsupported => {}
                }
                clamp_particle_count(child_state, child.config.max_count);
            }
            root_deaths.clear();
            clamp_particle_count(&mut state.root, item.config.max_count);
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
    delta_ms: f64,
    now_ms: f64,
) -> Vec<[f64; 2]> {
    let delta_seconds = (delta_ms / 1000.0).clamp(0.0, 0.1);
    let mut deaths = Vec::new();
    state.particles.retain_mut(|particle| {
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
    clamp_particle_count(state, config.max_count);
    deaths
}

fn emit_for_config(
    random: &mut SceneSpriteRandom,
    state: &mut SceneSpriteEmitterState,
    config: &SceneSpriteParticleConfig,
    delta_ms: f64,
    now_ms: f64,
    origin: [f64; 2],
    probability: Option<f64>,
) {
    let started_at_ms = *state.started_at_ms.get_or_insert(now_ms);
    if now_ms - started_at_ms < config.start_time_ms {
        return;
    }
    if config.instantaneous && !state.instantaneous_emitted {
        let emit_count = config.max_count.min(MAX_FRAME_EMITS);
        for _ in 0..emit_count {
            if probability
                .map(|value| random.chance(value))
                .unwrap_or(true)
            {
                state
                    .particles
                    .push(spawn_particle(random, config, now_ms, origin));
            }
        }
        state.instantaneous_emitted = true;
    } else if !config.instantaneous {
        state.emission_credit += config.emission_rate.max(0.0) * delta_ms / 1000.0;
        let emit_count = state
            .emission_credit
            .floor()
            .clamp(0.0, MAX_FRAME_EMITS as f64) as usize;
        if emit_count > 0 {
            state.emission_credit -= emit_count as f64;
            for _ in 0..emit_count {
                if probability
                    .map(|value| random.chance(value))
                    .unwrap_or(true)
                {
                    state
                        .particles
                        .push(spawn_particle(random, config, now_ms, origin));
                }
            }
        }
    }
    clamp_particle_count(state, config.max_count);
}

fn spawn_particle(
    random: &mut SceneSpriteRandom,
    config: &SceneSpriteParticleConfig,
    now_ms: f64,
    origin: [f64; 2],
) -> SceneSpriteParticle {
    let radius = random.range(config.spawn_radius[0], config.spawn_radius[1]);
    let angle = random.range(0.0, std::f64::consts::TAU);
    let x = origin[0] + angle.cos() * radius;
    let y = origin[1] + angle.sin() * radius;
    let direction = choose_direction(random, config, angle);
    let speed = random.range(config.speed_range[0], config.speed_range[1]);
    let color = interpolate_color(
        config.color_min,
        config.color_max,
        random.next_f64(),
        random.range(config.alpha_range[0], config.alpha_range[1]),
    );

    SceneSpriteParticle {
        texture_path: config.texture_path.clone(),
        blend_mode: config.blend_mode,
        x,
        y,
        vx: direction[0] * speed,
        vy: direction[1] * speed,
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
        born_at_ms: now_ms,
        life_ms: random
            .range(config.lifetime_ms_range[0], config.lifetime_ms_range[1])
            .max(16.0),
        color,
    }
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
    [direction[0] / length * sign, direction[1] / length * sign]
}

fn primitive_from_particle(
    particle: &SceneSpriteParticle,
    canvas_height: f64,
    now_ms: f64,
) -> Option<SceneSpriteParticlePrimitive> {
    if now_ms - particle.born_at_ms >= particle.life_ms {
        return None;
    }
    let age = ((now_ms - particle.born_at_ms) / particle.life_ms).clamp(0.0, 1.0);
    let opacity = 1.0 - age;
    let size_factor = particle
        .size_change
        .map(|range| range[0] + (range[1] - range[0]) * age)
        .unwrap_or(1.0)
        .max(0.0);
    let size = (particle.size * size_factor).max(0.5);
    let top = canvas_height - particle.y - size / 2.0;
    Some(SceneSpriteParticlePrimitive {
        texture_path: particle.texture_path.clone(),
        blend_mode: particle.blend_mode,
        left: particle.x - size / 2.0,
        top,
        width: size,
        height: size,
        rotation: particle.rotation_degrees.to_radians(),
        opacity,
        color: SceneRenderColor {
            alpha: ((particle.color.alpha as f64) * opacity).round() as u8,
            ..particle.color
        },
        transform_origin_x: particle.x,
        transform_origin_y: top + size / 2.0,
    })
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

fn clamp_particle_count(state: &mut SceneSpriteEmitterState, max_count: usize) {
    if state.particles.len() > max_count {
        state
            .particles
            .drain(0..state.particles.len().saturating_sub(max_count));
    }
}

#[cfg(test)]
mod tests {
    use super::{SceneSpriteParticleConfig, SceneSpriteParticleScheduler};
    use crate::models::{SceneParticleChildKind, SceneParticleScheduleMode};
    use crate::services::scene_render_planner_service::{
        SceneRenderBlendMode, SceneRenderColor, SceneRenderSpriteParticleItem,
        SceneSpriteParticleChildItem,
    };
    use std::path::PathBuf;

    fn config() -> SceneSpriteParticleConfig {
        SceneSpriteParticleConfig {
            texture_path: PathBuf::from("/tmp/sprite.png"),
            blend_mode: SceneRenderBlendMode::Normal,
            spawn_origin: [100.0, 120.0],
            spawn_radius: [0.0, 4.0],
            directions: vec![[0.0, 1.0]],
            sign: 1.0,
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
            emission_rate: 120.0,
            max_count: 24,
            start_time_ms: 0.0,
            instantaneous: false,
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
}
