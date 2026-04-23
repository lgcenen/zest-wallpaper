use std::collections::BTreeSet;

use tauri::AppHandle;

use crate::services::window_service;

const FINDER_BUNDLE_ID: &str = "com.apple.finder";
const CONTENT_WINDOW_LAYER: i32 = 0;
const FULLSCREEN_COVERAGE_THRESHOLD: f64 = 0.995;
const MIN_VISIBLE_ALPHA: f64 = 0.01;

#[derive(Debug, Clone, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn from_xywh(x: f64, y: f64, width: f64, height: f64) -> Option<Self> {
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        Some(Self {
            x,
            y,
            width,
            height,
        })
    }

    fn area(&self) -> f64 {
        self.width * self.height
    }

    fn max_x(&self) -> f64 {
        self.x + self.width
    }

    fn max_y(&self) -> f64 {
        self.y + self.height
    }

    fn intersection(&self, other: &Self) -> Option<Self> {
        let min_x = self.x.max(other.x);
        let min_y = self.y.max(other.y);
        let max_x = self.max_x().min(other.max_x());
        let max_y = self.max_y().min(other.max_y());

        Self::from_xywh(min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FrontmostAppSample {
    pid: i32,
    bundle_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ScreenSample {
    label: String,
    bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
struct WindowSample {
    owner_pid: i32,
    owner_bundle_id: Option<String>,
    layer: i32,
    alpha: f64,
    bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
struct NamedScreenBounds {
    name: String,
    bounds: Rect,
}

pub fn sample_auto_pause_screen_labels(
    app: &AppHandle,
    app_bundle_id: &str,
) -> Result<BTreeSet<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let (frontmost, screens) = sample_frontmost_and_screens(app)?;
        if screens.is_empty() {
            return Ok(BTreeSet::new());
        }

        let windows = sample_window_list()?;
        return Ok(resolve_auto_pause_screen_labels(
            frontmost.as_ref(),
            &screens,
            &windows,
            app_bundle_id,
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        let _ = app_bundle_id;
        Ok(BTreeSet::new())
    }
}

fn resolve_auto_pause_screen_labels(
    _frontmost: Option<&FrontmostAppSample>,
    screens: &[ScreenSample],
    windows: &[WindowSample],
    app_bundle_id: &str,
) -> BTreeSet<String> {
    let candidate_windows = windows
        .iter()
        .filter(|window| is_candidate_window(window, app_bundle_id))
        .collect::<Vec<_>>();
    if candidate_windows.is_empty() {
        return BTreeSet::new();
    }

    screens
        .iter()
        .filter(|screen| {
            candidate_windows
                .iter()
                .any(|window| window_covers_screen(screen, window))
        })
        .map(|screen| screen.label.clone())
        .collect()
}

fn is_candidate_window(window: &WindowSample, app_bundle_id: &str) -> bool {
    window.owner_pid > 0
        && window.layer == CONTENT_WINDOW_LAYER
        && window.alpha > MIN_VISIBLE_ALPHA
        && window.bounds.area() > 0.0
        && !matches!(window.owner_bundle_id.as_deref(), Some(FINDER_BUNDLE_ID))
        && window.owner_bundle_id.as_deref() != Some(app_bundle_id)
}

fn window_covers_screen(screen: &ScreenSample, window: &WindowSample) -> bool {
    let screen_area = screen.bounds.area();
    if screen_area <= 0.0 {
        return false;
    }

    let overlap = screen.bounds.intersection(&window.bounds);
    let Some(overlap) = overlap else {
        return false;
    };

    overlap.area() / screen_area >= FULLSCREEN_COVERAGE_THRESHOLD
}

#[cfg(target_os = "macos")]
fn sample_frontmost_and_screens(
    app: &AppHandle,
) -> Result<(Option<FrontmostAppSample>, Vec<ScreenSample>), String> {
    use dispatch2::run_on_main;
    use objc2_app_kit::{NSScreen, NSWorkspace};

    let expected_labels =
        window_service::expected_player_window_labels(app).map_err(|error| error.to_string())?;
    if expected_labels.is_empty() {
        return Ok((None, Vec::new()));
    }

    run_on_main(move |mtm| {
        let frontmost = NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|application| FrontmostAppSample {
                pid: application.processIdentifier(),
                bundle_id: application
                    .bundleIdentifier()
                    .map(|value| value.to_string()),
            });

        let screens = NSScreen::screens(mtm);
        if screens.is_empty() {
            return Ok((frontmost, Vec::new()));
        }

        let max_y = screens
            .iter()
            .map(|screen| {
                let frame = screen.frame();
                frame.origin.y + frame.size.height
            })
            .fold(f64::NEG_INFINITY, f64::max);

        let mut screen_bounds = screens
            .iter()
            .filter_map(|screen| {
                let frame = screen.frame();
                Some(NamedScreenBounds {
                    name: screen.localizedName().to_string(),
                    bounds: Rect::from_xywh(
                        frame.origin.x,
                        max_y - (frame.origin.y + frame.size.height),
                        frame.size.width,
                        frame.size.height,
                    )?,
                })
            })
            .collect::<Vec<_>>();

        screen_bounds.sort_by(|left, right| {
            left.bounds
                .y
                .total_cmp(&right.bounds.y)
                .then_with(|| left.bounds.x.total_cmp(&right.bounds.x))
                .then_with(|| left.name.cmp(&right.name))
        });

        let screens = expected_labels
            .into_iter()
            .zip(screen_bounds)
            .map(|(label, screen)| ScreenSample {
                label,
                bounds: screen.bounds,
            })
            .collect();

        Ok((frontmost, screens))
    })
}

#[cfg(target_os = "macos")]
fn sample_window_list() -> Result<Vec<WindowSample>, String> {
    use std::collections::BTreeMap;

    use objc2_app_kit::NSRunningApplication;
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFString, CFType, CGRect};
    use objc2_core_graphics::{
        kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer,
        kCGWindowOwnerPID, CGRectMakeWithDictionaryRepresentation, CGRectZero,
        CGWindowListCopyWindowInfo, CGWindowListOption,
    };

    fn dictionary_number_i32(
        dictionary: &CFDictionary<CFString, CFType>,
        key: &CFString,
    ) -> Option<i32> {
        dictionary
            .get(key)?
            .downcast::<CFNumber>()
            .ok()?
            .as_i64()
            .map(|value| value as i32)
    }

    fn dictionary_number_f64(
        dictionary: &CFDictionary<CFString, CFType>,
        key: &CFString,
    ) -> Option<f64> {
        dictionary.get(key)?.downcast::<CFNumber>().ok()?.as_f64()
    }

    fn dictionary_bool(
        dictionary: &CFDictionary<CFString, CFType>,
        key: &CFString,
    ) -> Option<bool> {
        dictionary
            .get(key)?
            .downcast::<CFBoolean>()
            .ok()
            .map(|value| value.as_bool())
    }

    fn dictionary_rect(
        dictionary: &CFDictionary<CFString, CFType>,
        key: &CFString,
    ) -> Option<CGRect> {
        let bounds_dictionary = dictionary.get(key)?.downcast::<CFDictionary>().ok()?;
        let mut rect = unsafe { CGRectZero };
        if unsafe {
            CGRectMakeWithDictionaryRepresentation(Some(bounds_dictionary.as_ref()), &mut rect)
        } {
            Some(rect)
        } else {
            None
        }
    }

    fn rect_from_cgrect(rect: CGRect) -> Option<Rect> {
        Rect::from_xywh(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )
    }

    let Some(window_array) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Ok(Vec::new());
    };

    let owner_pid_key = unsafe { kCGWindowOwnerPID };
    let layer_key = unsafe { kCGWindowLayer };
    let alpha_key = unsafe { kCGWindowAlpha };
    let onscreen_key = unsafe { kCGWindowIsOnscreen };
    let bounds_key = unsafe { kCGWindowBounds };
    let windows = unsafe { window_array.cast_unchecked::<CFDictionary<CFString, CFType>>() };
    let mut owner_bundle_ids = BTreeMap::<i32, Option<String>>::new();
    let samples = windows
        .iter()
        .filter_map(|window| {
            let owner_pid = dictionary_number_i32(&window, owner_pid_key)?;
            let layer = dictionary_number_i32(&window, layer_key)?;
            let alpha = dictionary_number_f64(&window, alpha_key).unwrap_or(1.0);
            let onscreen = dictionary_bool(&window, onscreen_key).unwrap_or(true);
            let bounds = dictionary_rect(&window, bounds_key)?;
            if !onscreen {
                return None;
            }

            let owner_bundle_id = owner_bundle_ids
                .entry(owner_pid)
                .or_insert_with(|| {
                    NSRunningApplication::runningApplicationWithProcessIdentifier(owner_pid)
                        .and_then(|application| {
                            application
                                .bundleIdentifier()
                                .map(|value| value.to_string())
                        })
                })
                .clone();

            Some(WindowSample {
                owner_pid,
                owner_bundle_id,
                layer,
                alpha,
                bounds: rect_from_cgrect(bounds)?,
            })
        })
        .collect();

    Ok(samples)
}

#[cfg(not(target_os = "macos"))]
fn sample_frontmost_and_screens(
    app: &AppHandle,
) -> Result<(Option<FrontmostAppSample>, Vec<ScreenSample>), String> {
    let _ = app;
    Ok((None, Vec::new()))
}

#[cfg(not(target_os = "macos"))]
fn sample_window_list() -> Result<Vec<WindowSample>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        resolve_auto_pause_screen_labels, FrontmostAppSample, Rect, ScreenSample, WindowSample,
    };

    fn frontmost(pid: i32, bundle_id: &str) -> FrontmostAppSample {
        FrontmostAppSample {
            pid,
            bundle_id: Some(bundle_id.to_string()),
        }
    }

    fn screen(label: &str, x: f64, y: f64, width: f64, height: f64) -> ScreenSample {
        ScreenSample {
            label: label.to_string(),
            bounds: Rect::from_xywh(x, y, width, height).expect("screen bounds"),
        }
    }

    fn window(
        owner_pid: i32,
        owner_bundle_id: Option<&str>,
        layer: i32,
        alpha: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> WindowSample {
        WindowSample {
            owner_pid,
            owner_bundle_id: owner_bundle_id.map(|value| value.to_string()),
            layer,
            alpha,
            bounds: Rect::from_xywh(x, y, width, height).expect("window bounds"),
        }
    }

    #[test]
    fn ordinary_frontmost_window_does_not_trigger_auto_pause() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(7, "com.apple.Safari")),
            &[screen("player", 0.0, 0.0, 1920.0, 1080.0)],
            &[window(
                7,
                Some("com.apple.Safari"),
                0,
                1.0,
                120.0,
                80.0,
                1600.0,
                900.0,
            )],
            "com.lin.wallpaperworkbench",
        );

        assert!(paused.is_empty());
    }

    #[test]
    fn near_fullscreen_window_triggers_auto_pause_for_covered_screen() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(7, "com.apple.Safari")),
            &[screen("player", 0.0, 0.0, 1920.0, 1080.0)],
            &[window(
                7,
                Some("com.apple.Safari"),
                0,
                1.0,
                1.0,
                1.0,
                1918.0,
                1078.0,
            )],
            "com.lin.wallpaperworkbench",
        );

        assert_eq!(paused, BTreeSet::from([String::from("player")]));
    }

    #[test]
    fn non_frontmost_covering_window_still_triggers_auto_pause() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(9, "com.apple.Terminal")),
            &[screen("player", 0.0, 0.0, 1920.0, 1080.0)],
            &[
                window(
                    9,
                    Some("com.apple.Terminal"),
                    0,
                    1.0,
                    300.0,
                    200.0,
                    1200.0,
                    700.0,
                ),
                window(
                    7,
                    Some("com.apple.Safari"),
                    0,
                    1.0,
                    0.0,
                    0.0,
                    1920.0,
                    1080.0,
                ),
            ],
            "com.lin.wallpaperworkbench",
        );

        assert_eq!(paused, BTreeSet::from([String::from("player")]));
    }

    #[test]
    fn frontmost_non_covering_window_does_not_clear_a_screen_covered_by_another_app() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(9, "com.apple.Terminal")),
            &[
                screen("player", 0.0, 0.0, 1920.0, 1080.0),
                screen("player-screen-1", 1920.0, 0.0, 1920.0, 1080.0),
            ],
            &[
                window(
                    9,
                    Some("com.apple.Terminal"),
                    0,
                    1.0,
                    220.0,
                    160.0,
                    1000.0,
                    640.0,
                ),
                window(
                    7,
                    Some("com.apple.Safari"),
                    0,
                    1.0,
                    1920.0,
                    0.0,
                    1920.0,
                    1080.0,
                ),
            ],
            "com.lin.wallpaperworkbench",
        );

        assert_eq!(paused, BTreeSet::from([String::from("player-screen-1")]));
    }

    #[test]
    fn finder_and_our_own_windows_do_not_trigger_auto_pause() {
        let screen = screen("player", 0.0, 0.0, 1920.0, 1080.0);
        let finder = window(
            7,
            Some("com.apple.finder"),
            0,
            1.0,
            0.0,
            0.0,
            1920.0,
            1080.0,
        );
        let own_app = window(
            8,
            Some("com.lin.wallpaperworkbench"),
            0,
            1.0,
            0.0,
            0.0,
            1920.0,
            1080.0,
        );

        let finder_paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(11, "com.apple.Safari")),
            std::slice::from_ref(&screen),
            std::slice::from_ref(&finder),
            "com.lin.wallpaperworkbench",
        );
        let own_app_paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(11, "com.apple.Safari")),
            &[screen],
            &[own_app],
            "com.lin.wallpaperworkbench",
        );

        assert!(finder_paused.is_empty());
        assert!(own_app_paused.is_empty());
    }

    #[test]
    fn non_content_layer_window_is_ignored() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(7, "com.apple.Safari")),
            &[screen("player", 0.0, 0.0, 1920.0, 1080.0)],
            &[window(
                7,
                Some("com.apple.Safari"),
                25,
                1.0,
                0.0,
                0.0,
                1920.0,
                1080.0,
            )],
            "com.lin.wallpaperworkbench",
        );

        assert!(paused.is_empty());
    }

    #[test]
    fn only_the_covered_screen_is_marked_auto_paused_in_multi_screen_layout() {
        let paused = resolve_auto_pause_screen_labels(
            Some(&frontmost(42, "com.apple.Safari")),
            &[
                screen("player", 0.0, 0.0, 1920.0, 1080.0),
                screen("player-screen-1", 1920.0, 0.0, 1920.0, 1080.0),
            ],
            &[
                window(
                    42,
                    Some("com.apple.Safari"),
                    0,
                    1.0,
                    0.0,
                    0.0,
                    1918.0,
                    1078.0,
                ),
                window(
                    42,
                    Some("com.apple.Safari"),
                    0,
                    1.0,
                    2300.0,
                    100.0,
                    800.0,
                    600.0,
                ),
            ],
            "com.lin.wallpaperworkbench",
        );

        assert_eq!(paused, BTreeSet::from([String::from("player")]));
    }
}
