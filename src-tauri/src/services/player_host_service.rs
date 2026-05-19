use std::collections::BTreeSet;

use tauri::{AppHandle, Runtime, WebviewWindow};

use super::window_service;

pub fn show_player_hosts<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<usize> {
    window_service::show_player_windows(app)
}

pub fn close_player_hosts<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    window_service::close_player_windows(app)
}

pub fn live_player_host_labels<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    window_service::player_window_labels(app)
}

pub fn live_player_host_label_set<R: Runtime>(app: &AppHandle<R>) -> BTreeSet<String> {
    window_service::player_window_label_set(app)
}

pub fn expected_player_host_labels<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Vec<String>> {
    window_service::expected_player_window_labels(app)
}

pub fn expected_player_host_label_set<R: Runtime>(
    app: &AppHandle<R>,
) -> tauri::Result<BTreeSet<String>> {
    window_service::expected_player_window_label_set(app)
}

pub fn player_host_plan_signature<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<String> {
    window_service::player_window_plan_signature(app)
}

pub fn player_host_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
) -> Result<WebviewWindow<R>, String> {
    window_service::player_window(app, label)
}

pub fn set_player_host_snapshot_background_color<R: Runtime>(
    app: &AppHandle<R>,
    red: f64,
    green: f64,
    blue: f64,
) -> Result<usize, String> {
    window_service::set_player_windows_snapshot_background_color(app, red, green, blue)
}
