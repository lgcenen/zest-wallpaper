mod commands;
mod importer;
mod models;
mod pkg;
mod scene;
mod services;
mod store;
mod system_texture;
mod tex;

use store::AppState;

use commands::{
    player_commands::{
        apply_dynamic_wallpaper, get_player_diagnostics, get_player_state, pause_resume_dynamic,
    },
    settings_commands::{
        clear_scene_cache, fetch_external_image, get_scene_cache_size, get_scene_runtime_settings,
        open_external_url, set_cache_storage_path, set_scene_external_assets_path,
    },
    wallpaper_commands::{
        get_wallpaper_details, import_wallpaper, list_wallpapers, remove_wallpaper,
        set_wallpaper_properties,
    },
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::load().expect("failed to load application state");

    let app = tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_dialog::init())
        .on_menu_event(services::lifecycle_service::handle_menu_event)
        .on_window_event(services::lifecycle_service::handle_window_event)
        .setup(|app| services::lifecycle_service::configure_app_on_setup(app).map_err(Into::into))
        .invoke_handler(tauri::generate_handler![
            list_wallpapers,
            import_wallpaper,
            get_wallpaper_details,
            apply_dynamic_wallpaper,
            set_wallpaper_properties,
            pause_resume_dynamic,
            remove_wallpaper,
            get_scene_runtime_settings,
            get_player_state,
            get_player_diagnostics,
            set_scene_external_assets_path,
            set_cache_storage_path,
            clear_scene_cache,
            get_scene_cache_size,
            open_external_url,
            fetch_external_image,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build tauri application");

    app.run(|app_handle, event| {
        services::lifecycle_service::handle_run_event(app_handle, &event);
    });
}
