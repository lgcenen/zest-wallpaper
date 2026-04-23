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

#[derive(Debug, Clone, Copy, Default)]
pub struct SceneInputResponseState {
    response: SceneInputResponse,
}

impl SceneInputResponseState {
    pub fn update(
        &mut self,
        target: Option<SceneInputTarget>,
        delta_seconds: f64,
    ) -> SceneInputResponse {
        let delta_seconds = delta_seconds.clamp(1.0 / 240.0, 0.25);
        match target.filter(|target| target.active) {
            Some(target) => {
                let motion_factor = smoothing_factor(delta_seconds, 9.0);
                let cursor_factor = smoothing_factor(delta_seconds, 12.0);
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
                let release_factor = smoothing_factor(delta_seconds, 6.0);
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

fn smoothing_factor(delta_seconds: f64, response_rate: f64) -> f64 {
    (1.0 - (-response_rate * delta_seconds).exp()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{SceneInputResponseState, SceneInputTarget};

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
}
