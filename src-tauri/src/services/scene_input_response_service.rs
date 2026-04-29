use crate::services::{
    input_service::SharedInputSnapshot, scene_render_planner_service::SceneRenderCamera,
};

const DEFAULT_MOTION_RESPONSE_RATE: f64 = 9.0;
const DEFAULT_CURSOR_RESPONSE_RATE: f64 = 12.0;
const DEFAULT_RELEASE_RESPONSE_RATE: f64 = 6.0;
const DEFAULT_PARALLAX_CANVAS_FACTOR: f64 = 0.012;
const DEFAULT_SHAKE_X_SCALE: f64 = 3.2;
const DEFAULT_SHAKE_Y_SCALE: f64 = 2.4;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneInputTarget {
    pub active: bool,
    pub motion_x: f64,
    pub motion_y: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneInputResponse {
    pub motion_x: f64,
    pub motion_y: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_active: bool,
}

impl SceneInputResponse {
    pub fn cursor(self) -> Option<(f64, f64)> {
        self.cursor_active.then_some((self.cursor_x, self.cursor_y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneInputCoordinatorConfig {
    pub motion_response_rate: f64,
    pub cursor_response_rate: f64,
    pub release_response_rate: f64,
    pub parallax_canvas_factor: f64,
    pub shake_x_scale: f64,
    pub shake_y_scale: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneInputViewport {
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneInputSceneBounds {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneInputProjection {
    pub target: Option<SceneInputTarget>,
    pub diagnostics: Vec<SceneInputCoordinatorDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneInputCoordinatorFrame {
    pub response: SceneInputResponse,
    pub camera_offset: (f64, f64),
    pub diagnostics: Vec<SceneInputCoordinatorDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneInputCoordinatorUpdate {
    pub target: Option<SceneInputTarget>,
    pub camera: SceneRenderCamera,
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub delta_seconds: f64,
    pub scene_time_seconds: f64,
    pub projection_diagnostics: Vec<SceneInputCoordinatorDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneInputCoordinatorDiagnostic {
    pub severity: SceneInputCoordinatorDiagnosticSeverity,
    pub code: String,
    pub runtime_stage: String,
    pub message: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneInputCoordinatorDiagnosticSeverity {
    Info,
    Warning,
}

#[derive(Debug, Clone, Copy)]
pub struct SceneInputResponseState {
    response: SceneInputResponse,
    config: SceneInputCoordinatorConfig,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SceneInputCoordinator {
    response_state: SceneInputResponseState,
}

impl Default for SceneInputCoordinatorConfig {
    fn default() -> Self {
        Self {
            motion_response_rate: DEFAULT_MOTION_RESPONSE_RATE,
            cursor_response_rate: DEFAULT_CURSOR_RESPONSE_RATE,
            release_response_rate: DEFAULT_RELEASE_RESPONSE_RATE,
            parallax_canvas_factor: DEFAULT_PARALLAX_CANVAS_FACTOR,
            shake_x_scale: DEFAULT_SHAKE_X_SCALE,
            shake_y_scale: DEFAULT_SHAKE_Y_SCALE,
        }
    }
}

impl Default for SceneInputResponseState {
    fn default() -> Self {
        Self {
            response: SceneInputResponse::default(),
            config: SceneInputCoordinatorConfig::default(),
        }
    }
}

impl SceneInputResponseState {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_config(config: SceneInputCoordinatorConfig) -> Self {
        Self {
            response: SceneInputResponse::default(),
            config,
        }
    }

    pub fn update(
        &mut self,
        target: Option<SceneInputTarget>,
        delta_seconds: f64,
    ) -> SceneInputResponse {
        self.update_with_config(target, delta_seconds, self.config)
    }

    fn update_with_config(
        &mut self,
        target: Option<SceneInputTarget>,
        delta_seconds: f64,
        config: SceneInputCoordinatorConfig,
    ) -> SceneInputResponse {
        let delta_seconds = delta_seconds.clamp(1.0 / 240.0, 0.25);
        match target.filter(|target| target.active) {
            Some(target) => {
                let motion_factor = smoothing_factor(delta_seconds, config.motion_response_rate);
                let cursor_factor = smoothing_factor(delta_seconds, config.cursor_response_rate);
                self.response.motion_x +=
                    (target.motion_x.clamp(-1.0, 1.0) - self.response.motion_x) * motion_factor;
                self.response.motion_y +=
                    (target.motion_y.clamp(-1.0, 1.0) - self.response.motion_y) * motion_factor;
                self.response.cursor_x +=
                    (target.cursor_x - self.response.cursor_x) * cursor_factor;
                self.response.cursor_y +=
                    (target.cursor_y - self.response.cursor_y) * cursor_factor;
                self.response.cursor_active = true;
            }
            None => {
                let release_factor = smoothing_factor(delta_seconds, config.release_response_rate);
                self.response.motion_x += (0.0 - self.response.motion_x) * release_factor;
                self.response.motion_y += (0.0 - self.response.motion_y) * release_factor;
                self.response.cursor_active = false;
            }
        }

        self.response
    }

    pub fn reset(&mut self) {
        self.response = SceneInputResponse::default();
    }
}

impl SceneInputCoordinator {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_config(config: SceneInputCoordinatorConfig) -> Self {
        Self {
            response_state: SceneInputResponseState::with_config(config),
        }
    }

    pub fn update(&mut self, update: SceneInputCoordinatorUpdate) -> SceneInputCoordinatorFrame {
        let response = self
            .response_state
            .update(update.target, update.delta_seconds);
        let camera_offset = scene_camera_offset(
            &update.camera,
            response,
            SceneInputSceneBounds {
                width: update.canvas_width,
                height: update.canvas_height,
            },
            update.scene_time_seconds,
            self.response_state.config,
        );

        SceneInputCoordinatorFrame {
            response,
            camera_offset,
            diagnostics: update.projection_diagnostics,
        }
    }

    pub fn reset(&mut self) {
        self.response_state.reset();
    }
}

pub fn project_shared_input_to_scene(
    snapshot: Option<&SharedInputSnapshot>,
    viewport: SceneInputViewport,
    scene_bounds: SceneInputSceneBounds,
) -> SceneInputProjection {
    let mut diagnostics = Vec::new();

    if !viewport.width.is_finite()
        || !viewport.height.is_finite()
        || viewport.width <= 0.0
        || viewport.height <= 0.0
    {
        diagnostics.push(SceneInputCoordinatorDiagnostic::warning(
            "input-viewport-invalid",
            "shared-input-projection",
            "Scene input projection received an invalid viewport.",
            format!(
                "viewport width={} height={}",
                viewport.width, viewport.height
            ),
        ));
        return SceneInputProjection {
            target: None,
            diagnostics,
        };
    }

    if !scene_bounds.width.is_finite()
        || !scene_bounds.height.is_finite()
        || scene_bounds.width <= 0.0
        || scene_bounds.height <= 0.0
    {
        diagnostics.push(SceneInputCoordinatorDiagnostic::warning(
            "input-scene-bounds-invalid",
            "shared-input-projection",
            "Scene input projection received invalid scene bounds.",
            format!(
                "scene width={} height={}",
                scene_bounds.width, scene_bounds.height
            ),
        ));
        return SceneInputProjection {
            target: None,
            diagnostics,
        };
    }

    let Some(snapshot) = snapshot else {
        diagnostics.push(SceneInputCoordinatorDiagnostic::warning(
            "shared-input-snapshot-missing",
            "shared-input-projection",
            "Scene input coordinator has no shared input snapshot.",
            "The shared input service did not return a frame for this render pass.",
        ));
        return SceneInputProjection {
            target: None,
            diagnostics,
        };
    };

    if !snapshot.active {
        diagnostics.push(SceneInputCoordinatorDiagnostic::info(
            "shared-input-inactive",
            "shared-input-projection",
            "Scene input coordinator is releasing cursor state because shared input is inactive.",
            "The shared input frame reported no active cursor inside the desktop bounds.",
        ));
        return SceneInputProjection {
            target: None,
            diagnostics,
        };
    }

    if snapshot.desktop_width <= 0.0 || snapshot.desktop_height <= 0.0 {
        diagnostics.push(SceneInputCoordinatorDiagnostic::warning(
            "shared-input-desktop-bounds-invalid",
            "shared-input-projection",
            "Scene input coordinator received invalid desktop bounds.",
            format!(
                "desktop width={} height={}",
                snapshot.desktop_width, snapshot.desktop_height
            ),
        ));
        return SceneInputProjection {
            target: None,
            diagnostics,
        };
    }

    let local_x = (snapshot.system_x - viewport.origin_x).clamp(0.0, viewport.width);
    let local_y = (snapshot.system_y - viewport.origin_y).clamp(0.0, viewport.height);
    let normalized_x = local_x / viewport.width;
    let normalized_y = local_y / viewport.height;

    SceneInputProjection {
        target: Some(SceneInputTarget {
            active: true,
            motion_x: (normalized_x - 0.5) * 2.0,
            motion_y: ((1.0 - normalized_y) - 0.5) * 2.0,
            cursor_x: normalized_x * scene_bounds.width,
            cursor_y: normalized_y * scene_bounds.height,
        }),
        diagnostics,
    }
}

pub fn scene_camera_offset(
    camera: &SceneRenderCamera,
    input_response: SceneInputResponse,
    scene_bounds: SceneInputSceneBounds,
    scene_time_seconds: f64,
    config: SceneInputCoordinatorConfig,
) -> (f64, f64) {
    let safe_width = scene_bounds.width.max(1.0);
    let safe_height = scene_bounds.height.max(1.0);
    let parallax_x = input_response.motion_x
        * camera.parallax_mouse_influence
        * (safe_width * config.parallax_canvas_factor);
    let parallax_y = input_response.motion_y
        * camera.parallax_mouse_influence
        * (safe_height * config.parallax_canvas_factor);
    let (shake_x, shake_y) = scene_camera_shake_offset(camera, scene_time_seconds, config);

    (parallax_x + shake_x, parallax_y + shake_y)
}

pub fn scene_camera_shake_offset(
    camera: &SceneRenderCamera,
    scene_time_seconds: f64,
    config: SceneInputCoordinatorConfig,
) -> (f64, f64) {
    if !camera.camera_shake
        || camera.camera_shake_amplitude <= 0.0
        || camera.camera_shake_speed <= 0.0
    {
        return (0.0, 0.0);
    }

    let time = scene_time_seconds.max(0.0);
    let speed = camera.camera_shake_speed.max(0.0);
    (
        (time * (speed + 0.35)).sin() * (camera.camera_shake_amplitude * config.shake_x_scale),
        (time * (speed + 0.18)).cos() * (camera.camera_shake_amplitude * config.shake_y_scale),
    )
}

fn smoothing_factor(delta_seconds: f64, response_rate: f64) -> f64 {
    (1.0 - (-response_rate * delta_seconds).exp()).clamp(0.0, 1.0)
}

impl SceneInputCoordinatorDiagnostic {
    fn warning(
        code: impl Into<String>,
        runtime_stage: impl Into<String>,
        message: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            severity: SceneInputCoordinatorDiagnosticSeverity::Warning,
            code: code.into(),
            runtime_stage: runtime_stage.into(),
            message: message.into(),
            reason: reason.into(),
        }
    }

    fn info(
        code: impl Into<String>,
        runtime_stage: impl Into<String>,
        message: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            severity: SceneInputCoordinatorDiagnosticSeverity::Info,
            code: code.into(),
            runtime_stage: runtime_stage.into(),
            message: message.into(),
            reason: reason.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        project_shared_input_to_scene, scene_camera_offset, scene_camera_shake_offset,
        SceneInputCoordinator, SceneInputCoordinatorConfig,
        SceneInputCoordinatorDiagnosticSeverity, SceneInputCoordinatorUpdate, SceneInputResponse,
        SceneInputResponseState, SceneInputSceneBounds, SceneInputTarget, SceneInputViewport,
    };
    use crate::services::{
        input_service::SharedInputSnapshot, scene_render_planner_service::SceneRenderCamera,
    };

    fn camera(parallax_mouse_influence: f64, shake: bool) -> SceneRenderCamera {
        SceneRenderCamera {
            zoom: 1.0,
            center: [0.5, 0.5],
            camera_shake: shake,
            camera_shake_amplitude: 0.6,
            camera_shake_speed: 1.4,
            parallax_mouse_influence,
        }
    }

    fn shared_snapshot(system_x: f64, system_y: f64, active: bool) -> SharedInputSnapshot {
        SharedInputSnapshot {
            system_x,
            system_y,
            desktop_width: 1920.0,
            desktop_height: 1080.0,
            active,
            ..SharedInputSnapshot::default()
        }
    }

    #[test]
    fn input_response_state_smooths_motion_toward_target() {
        let mut state = SceneInputResponseState::default();

        let first = state.update(
            Some(SceneInputTarget {
                active: true,
                motion_x: 1.0,
                motion_y: -1.0,
                cursor_x: 100.0,
                cursor_y: 200.0,
            }),
            1.0 / 60.0,
        );

        assert!(first.motion_x > 0.0 && first.motion_x < 1.0);
        assert!(first.motion_y < 0.0 && first.motion_y > -1.0);
        assert!(first.cursor_active);
        assert!(first.cursor_x > 0.0 && first.cursor_x < 100.0);
    }

    #[test]
    fn input_response_state_releases_motion_when_input_goes_inactive() {
        let mut state = SceneInputResponseState::default();
        let _ = state.update(
            Some(SceneInputTarget {
                active: true,
                motion_x: 0.8,
                motion_y: 0.4,
                cursor_x: 320.0,
                cursor_y: 240.0,
            }),
            1.0 / 60.0,
        );

        let released = state.update(None, 1.0 / 30.0);

        assert!(!released.cursor_active);
        assert!(released.motion_x < 0.8);
        assert!(released.motion_y < 0.4);
        assert!(released.motion_x > 0.0);
    }

    #[test]
    fn reset_clears_smoothed_input_state() {
        let mut state = SceneInputResponseState::default();
        let _ = state.update(
            Some(SceneInputTarget {
                active: true,
                motion_x: 0.6,
                motion_y: 0.2,
                cursor_x: 480.0,
                cursor_y: 120.0,
            }),
            1.0 / 60.0,
        );

        state.reset();

        let reset = state.update(None, 1.0 / 60.0);
        assert_eq!(reset.motion_x, 0.0);
        assert_eq!(reset.motion_y, 0.0);
        assert!(!reset.cursor_active);
    }

    #[test]
    fn shared_input_projection_maps_cursor_into_scene_space() {
        let projection = project_shared_input_to_scene(
            Some(&shared_snapshot(400.0, 300.0, true)),
            SceneInputViewport {
                origin_x: 100.0,
                origin_y: 100.0,
                width: 800.0,
                height: 400.0,
            },
            SceneInputSceneBounds {
                width: 1600.0,
                height: 900.0,
            },
        );
        let target = projection.target.expect("active input target");

        assert!(projection.diagnostics.is_empty());
        assert!((target.cursor_x - 600.0).abs() < 0.0001);
        assert!((target.cursor_y - 450.0).abs() < 0.0001);
        assert!((target.motion_x + 0.25).abs() < 0.0001);
        assert!(target.motion_y.abs() < 0.0001);
    }

    #[test]
    fn input_coordinator_smooths_cursor_and_parallax_from_projected_target() {
        let mut coordinator = SceneInputCoordinator::with_config(SceneInputCoordinatorConfig {
            motion_response_rate: 24.0,
            cursor_response_rate: 24.0,
            ..SceneInputCoordinatorConfig::default()
        });
        let frame = coordinator.update(SceneInputCoordinatorUpdate {
            target: Some(SceneInputTarget {
                active: true,
                motion_x: 1.0,
                motion_y: -0.5,
                cursor_x: 800.0,
                cursor_y: 240.0,
            }),
            camera: camera(0.5, false),
            canvas_width: 1600.0,
            canvas_height: 900.0,
            delta_seconds: 1.0 / 60.0,
            scene_time_seconds: 1.0,
            projection_diagnostics: Vec::new(),
        });

        assert!(frame.response.cursor_active);
        assert!(frame.response.cursor_x > 0.0 && frame.response.cursor_x < 800.0);
        assert!(frame.response.motion_x > 0.0 && frame.response.motion_x < 1.0);
        assert!(frame.camera_offset.0 > 0.0);
        assert!(frame.camera_offset.1 < 0.0);
    }

    #[test]
    fn camera_shake_uses_scene_time_and_stays_deterministic_for_paused_frames() {
        let config = SceneInputCoordinatorConfig::default();
        let first = scene_camera_shake_offset(&camera(0.0, true), 2.0, config);
        let second = scene_camera_shake_offset(&camera(0.0, true), 2.0, config);
        let later = scene_camera_shake_offset(&camera(0.0, true), 2.2, config);

        assert_eq!(first, second);
        assert_ne!(first, (0.0, 0.0));
        assert_ne!(first, later);
    }

    #[test]
    fn camera_offset_combines_parallax_and_shake_without_renderer_state() {
        let offset = scene_camera_offset(
            &camera(0.4, true),
            SceneInputResponse {
                motion_x: 0.5,
                motion_y: -0.25,
                cursor_x: 0.0,
                cursor_y: 0.0,
                cursor_active: true,
            },
            SceneInputSceneBounds {
                width: 1000.0,
                height: 500.0,
            },
            1.75,
            SceneInputCoordinatorConfig::default(),
        );

        assert!(offset.0.abs() > 0.1);
        assert!(offset.1.abs() > 0.1);
    }

    #[test]
    fn projection_diagnostics_distinguish_inactive_and_invalid_input_boundaries() {
        let inactive = project_shared_input_to_scene(
            Some(&shared_snapshot(0.0, 0.0, false)),
            SceneInputViewport {
                width: 100.0,
                height: 100.0,
                ..SceneInputViewport::default()
            },
            SceneInputSceneBounds {
                width: 100.0,
                height: 100.0,
            },
        );

        assert!(inactive.target.is_none());
        assert_eq!(
            inactive.diagnostics[0].severity,
            SceneInputCoordinatorDiagnosticSeverity::Info
        );
        assert_eq!(inactive.diagnostics[0].code, "shared-input-inactive");

        let invalid = project_shared_input_to_scene(
            Some(&shared_snapshot(0.0, 0.0, true)),
            SceneInputViewport {
                width: 0.0,
                height: 100.0,
                ..SceneInputViewport::default()
            },
            SceneInputSceneBounds {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(
            invalid.diagnostics[0].severity,
            SceneInputCoordinatorDiagnosticSeverity::Warning
        );
        assert_eq!(invalid.diagnostics[0].code, "input-viewport-invalid");
    }

    #[test]
    fn coordinator_reset_clears_lifecycle_state_for_switched_scene() {
        let mut coordinator = SceneInputCoordinator::default();
        let _ = coordinator.update(SceneInputCoordinatorUpdate {
            target: Some(SceneInputTarget {
                active: true,
                motion_x: 0.9,
                motion_y: 0.4,
                cursor_x: 640.0,
                cursor_y: 360.0,
            }),
            camera: camera(0.4, false),
            canvas_width: 1280.0,
            canvas_height: 720.0,
            delta_seconds: 1.0 / 30.0,
            scene_time_seconds: 0.5,
            projection_diagnostics: Vec::new(),
        });

        coordinator.reset();
        let released = coordinator.update(SceneInputCoordinatorUpdate {
            target: None,
            camera: camera(0.4, false),
            canvas_width: 1280.0,
            canvas_height: 720.0,
            delta_seconds: 1.0 / 60.0,
            scene_time_seconds: 0.5,
            projection_diagnostics: Vec::new(),
        });

        assert_eq!(released.response, SceneInputResponse::default());
        assert_eq!(released.camera_offset, (0.0, 0.0));
    }
}
