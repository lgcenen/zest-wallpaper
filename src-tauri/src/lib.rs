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
            clear_scene_cache, fetch_external_image, get_scene_runtime_settings, open_external_url,
            set_cache_storage_path, set_scene_external_assets_path,
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
            open_external_url,
            fetch_external_image,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build tauri application");

    app.run(|app_handle, event| {
        services::lifecycle_service::handle_run_event(app_handle, &event);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn phase_01a_contract_scopes_static_snapshot_sync_to_active_wallpapers() {
        let wallpaper_commands = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/wallpaper_commands.rs"
        ));
        let workbench_copy = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/workbench-copy.ts"
        ));
        let wallpaper_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/wallpaper-api.ts"
        ));

        assert!(!wallpaper_commands.contains("apply_static_wallpaper"));
        assert!(!wallpaper_gateway.contains("applyStaticWallpaper"));
        assert!(workbench_copy.contains("静态快照同步"));
        assert!(workbench_copy.contains("static snapshot sync"));
    }

    #[test]
    fn phase_03_contract_routes_player_mode_video_to_native_runtime() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let native_video_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_video_service.rs"
        ));

        assert!(player_runtime.contains("NativeVideoStageSurface"));
        assert!(!player_runtime.contains("VideoSurface"));
        assert!(native_video_service.contains("AVPlayerLooper"));
    }

    #[test]
    fn phase_04_contract_routes_player_mode_web_to_native_runtime() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let native_web_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_web_service.rs"
        ));

        assert!(player_runtime.contains("NativeWebStageSurface"));
        assert!(!player_runtime.contains("<iframe"));
        assert!(native_web_service.contains("WKWebView"));
        assert!(native_web_service.contains("evaluateJavaScript_completionHandler"));
    }

    #[test]
    fn phase_04_contract_keeps_web_player_fully_native() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let html_bridge = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/html_wallpaper_bridge.js"
        ));
        let commands_mod =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands/mod.rs"));
        let lib_rs = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
        let web_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/web-api.ts"
        ));
        let app_entry = lib_rs.split("#[cfg(test)]").next().unwrap_or(lib_rs);

        assert!(!player_runtime.contains("getWebRuntimeUrl"));
        assert!(!player_runtime.contains("postHtmlRuntimeMessage"));
        assert!(!player_runtime.contains("toWebPropertyPayload"));
        assert!(!player_runtime.contains("WebWallpaperSurface"));
        assert!(!player_runtime.contains("<iframe"));
        assert!(html_bridge.contains("__wallpaperApplyRuntimeMessage"));
        assert!(!html_bridge.contains("addEventListener(\"message\""));
        assert!(!html_bridge.contains("__wallpaperWorkbench"));
        assert!(!commands_mod.contains("web_commands"));
        assert!(!web_gateway.contains("getWebRuntimeUrl"));
        assert!(!web_gateway.contains("readWebWallpaperHtml"));
        assert!(!app_entry.contains("get_web_runtime_url"));
        assert!(!app_entry.contains("read_web_wallpaper_html"));
    }

    #[test]
    fn phase_04a_contract_uses_shared_input_service_for_scene_and_web() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let player_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/player-api.ts"
        ));
        let lifecycle_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/lifecycle_service.rs"
        ));
        let input_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/input_service.rs"
        ));
        let native_web_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_web_service.rs"
        ));
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));

        assert!(!player_gateway.contains("get_player_input_snapshot"));
        assert!(!player_gateway.contains("\"player:input\""));
        assert!(!player_runtime.contains("getPlayerInputSnapshot"));
        assert!(!player_runtime.contains("onPlayerInput"));
        assert!(!player_runtime.contains("getCursorPosition("));
        assert!(
            !player_runtime.contains("setInterval(() => {\n      void pollCursor();\n    }, 34);")
        );
        assert!(lifecycle_service.contains("input_service::start_input_worker"));
        assert!(!lifecycle_service.contains("start_cursor_worker"));
        assert!(input_service.contains("SharedInputSnapshot"));
        assert!(scene_native_renderer_service.contains("input_service::current_input_snapshot"));
        assert!(native_web_service.contains("dispatch_shared_input"));
        assert!(!native_web_service.contains("mouseLocation()"));
    }

    #[test]
    fn phase_02a_contract_drives_auto_pause_from_window_coverage() {
        let lifecycle_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/lifecycle_service.rs"
        ));
        let auto_pause_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/auto_pause_service.rs"
        ));

        assert!(lifecycle_service.contains("sample_auto_pause_screen_labels"));
        assert!(!lifecycle_service.contains("should_auto_pause(frontmost_bundle_id"));
        assert!(auto_pause_service.contains("CGWindowListCopyWindowInfo"));
        assert!(auto_pause_service.contains("FULLSCREEN_COVERAGE_THRESHOLD"));
    }

    #[test]
    fn phase_04b_contract_models_native_web_bridge_readiness() {
        let lifecycle_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/lifecycle_service.rs"
        ));
        let native_web_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_web_service.rs"
        ));
        let html_bridge = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/html_wallpaper_bridge.js"
        ));

        assert!(lifecycle_service.contains("start_bridge_retry_worker"));
        assert!(native_web_service.contains("WKScriptMessageHandler"));
        assert!(native_web_service.contains("bridge_ready"));
        assert!(native_web_service.contains("bootstrap_pending"));
        assert!(native_web_service.contains("last_bootstrap_hash"));
        assert!(native_web_service.contains("next_bootstrap_retry_at"));
        assert!(!native_web_service.contains("BOOTSTRAP_RETRY_WINDOW"));
        assert!(!native_web_service.contains("STATE_RETRY_WINDOW"));
        assert!(!native_web_service.contains("include_bootstrap"));
        assert!(html_bridge.contains("window.webkit.messageHandlers"));
        assert!(html_bridge.contains("wallpaper:bridge-ready"));
    }

    #[test]
    fn phase_04c_contract_uses_shared_audio_service_for_scene_and_web() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let player_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/player-api.ts"
        ));
        let lifecycle_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/lifecycle_service.rs"
        ));
        let audio_input_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/audio_input_service.rs"
        ));
        let native_web_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_web_service.rs"
        ));
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let html_bridge = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/html_wallpaper_bridge.js"
        ));

        assert!(!player_gateway.contains("get_player_audio_snapshot"));
        assert!(!player_gateway.contains("set_scene_audio_interest"));
        assert!(!player_gateway.contains("\"player:audio\""));
        assert!(!player_runtime.contains("getPlayerAudioSnapshot"));
        assert!(!player_runtime.contains("onPlayerAudio"));
        assert!(!player_runtime.contains("setSceneAudioInterest"));
        assert!(!player_runtime.contains("Math.sin((now / 420 + index * 0.92) * 1.2)"));
        assert!(lifecycle_service.contains("audio_input_service::start_audio_worker"));
        assert!(audio_input_service.contains("AudioSnapshot"));
        assert!(audio_input_service.contains("FftPlanner"));
        assert!(audio_input_service.contains("SCStream"));
        assert!(audio_input_service.contains("add_output_handler_with_queue"));
        assert!(audio_input_service.contains("remove_output_handler"));
        assert!(audio_input_service.contains("AUDIO_CAPTURE_QUEUE_LABEL"));
        assert!(
            scene_native_renderer_service.contains("audio_input_service::current_audio_snapshot")
        );
        assert!(
            scene_native_renderer_service.contains("audio_input_service::set_scene_audio_interest")
        );
        assert!(native_web_service.contains("dispatch_shared_audio"));
        assert!(native_web_service.contains("audio_consumers_active"));
        assert!(native_web_service.contains("\"wallpaper:audio\""));
        assert!(html_bridge.contains("wallpaper:audio-listener"));
    }

    #[test]
    fn phase_05_contract_adds_runtime_diagnostics_and_release_docs() {
        let player_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/player-api.ts"
        ));
        let player_commands = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/player_commands.rs"
        ));
        let services_mod =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/services/mod.rs"));
        let lifecycle_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/lifecycle_service.rs"
        ));
        let diagnostic_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/diagnostic_service.rs"
        ));
        let player_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/player_service.rs"
        ));
        let native_video_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_video_service.rs"
        ));
        let native_web_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/native_web_service.rs"
        ));
        let audio_input_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/audio_input_service.rs"
        ));
        let importer = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/importer.rs"));
        let scene = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/scene.rs"));
        let tex = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/tex.rs"));
        let web_runtime_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/web_runtime_service.rs"
        ));

        assert!(services_mod.contains("pub mod diagnostic_service"));
        assert!(lifecycle_service.contains("DiagnosticServiceState::default"));
        assert!(player_commands.contains("get_player_diagnostics"));
        assert!(player_gateway.contains("getPlayerDiagnostics"));
        assert!(player_gateway.contains("onPlayerDiagnostics"));
        assert!(diagnostic_service.contains("DIAGNOSTIC_EVENT_NAME"));
        assert!(diagnostic_service.contains("RuntimeDiagnosticSeverity"));
        assert!(player_service.contains("NativeHostSyncDisposition"));
        assert!(player_service.contains("apply_runtime_record_transaction"));
        assert!(player_service.contains("rollback_failed_apply"));
        assert!(native_video_service.contains("diagnostic_service::"));
        assert!(native_video_service.contains("resolve_active_video_source_path"));
        assert!(native_web_service.contains("diagnostic_service::"));
        assert!(native_web_service.contains("resolve_active_web_entry_path"));
        assert!(audio_input_service.contains("capture-unavailable"));
        assert!(!importer.contains("/Users/lin/"));
        assert!(!importer.contains("/Volumes/"));
        assert!(!scene.contains("/Users/lin/"));
        assert!(!scene.contains("/Volumes/"));
        assert!(!tex.contains("/Users/lin/"));
        assert!(!tex.contains("/Volumes/"));
        assert!(!web_runtime_service.contains("/Users/lin/"));
        assert!(!web_runtime_service.contains("/Volumes/"));
    }

    #[test]
    fn phase_06_contract_uses_native_overlay_workbench_titlebar() {
        let tauri_config = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"));
        let cargo_toml = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let workbench = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/WorkbenchApp.tsx"
        ));
        let window_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/window_service.rs"
        ));

        assert!(tauri_config.contains("\"label\": \"main\""));
        assert!(tauri_config.contains("\"title\": \"Zest Wallpaper\""));
        assert!(tauri_config.contains("\"macOSPrivateApi\": true"));
        assert!(tauri_config.contains("\"titleBarStyle\": \"Overlay\""));
        assert!(tauri_config.contains("\"hiddenTitle\": true"));
        assert!(tauri_config.contains("\"transparent\": true"));
        assert!(cargo_toml.contains("macos-private-api"));
        assert!(!workbench.contains("window-drag-strip"));
        assert!(!workbench.contains("data-tauri-drag-region"));
        assert!(!workbench.contains("@tauri-apps/api/window"));
        assert!(window_service.contains("ns_window.setOpaque(false)"));
        assert!(window_service.contains("ns_window.setHasShadow(false)"));
        assert!(window_service.contains("ns_window.setBackgroundColor(Some(&clear))"));
        assert!(window_service.contains("drawsBackground"));
        assert!(window_service.contains("setUnderPageBackgroundColor(Some(&clear))"));
        assert!(window_service.contains("setTitlebarAppearsTransparent(true)"));
        assert!(window_service.contains("setTitleVisibility(NSWindowTitleVisibility::Hidden)"));
        assert!(window_service.contains("setMovableByWindowBackground(true)"));
    }

    #[test]
    fn phase_07_contract_routes_scene_player_to_native_host() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let scene_support_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_support_service.rs"
        ));

        assert!(player_runtime.contains("NativeSceneStageSurface"));
        assert!(!player_runtime.contains("<SceneStageSurface"));
        assert!(scene_native_renderer_service.contains("MTKView"));
        assert!(scene_support_service.contains("SceneSupportReport"));
        assert!(scene_support_service.contains("SceneSupportError"));
    }

    #[test]
    fn phase_08_contract_establishes_native_scene_feature_equivalence() {
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let scene_support_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_support_service.rs"
        ));
        let scene_render_planner_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_render_planner_service.rs"
        ));
        let player_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/player_service.rs"
        ));
        let runtime_document_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/runtime_document_service.rs"
        ));
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));

        assert!(scene_native_renderer_service.contains("MTLRenderPipelineDescriptor"));
        assert!(scene_native_renderer_service.contains("drawPrimitives_vertexStart_vertexCount"));
        assert!(scene_native_renderer_service.contains("texture_cache"));
        assert!(scene_native_renderer_service.contains("video_sources"));
        assert!(scene_native_renderer_service.contains("AVPlayerItemVideoOutput"));
        assert!(scene_native_renderer_service.contains("CVMetalTextureCache"));
        assert!(!scene_native_renderer_service.contains("AVAssetImageGenerator"));
        assert!(!scene_native_renderer_service.contains("poster frame"));
        assert!(!scene_native_renderer_service.contains("native_video_service"));
        assert!(!scene_native_renderer_service.contains("AVPlayerView"));
        assert!(scene_native_renderer_service.contains("initWithFrame_device"));
        assert!(scene_native_renderer_service.contains("setDevice(Some(device.as_ref()))"));
        assert!(scene_native_renderer_service.contains("isDescendantOf(container)"));
        assert!(scene_native_renderer_service.contains("AVAudioPlayer"));
        assert!(scene_native_renderer_service.contains("rasterize_text_texture"));
        assert!(scene_native_renderer_service.contains("build_text_attributes"));
        assert!(scene_native_renderer_service.contains("SCENE_TEXT_FONT_CACHE"));
        assert!(!scene_native_renderer_service.contains("apply_standard_text_shadow("));
        assert!(scene_support_service.contains("SceneSupportSeverity"));
        assert!(scene_support_service.contains("NoRenderableVisuals"));
        assert!(scene_render_planner_service.contains("build_scene_render_plan"));
        assert!(scene_render_planner_service.contains("renderBounds"));
        assert!(scene_render_planner_service.contains("SceneRenderTextItem"));
        assert!(scene_render_planner_service.contains("SceneRenderAudioItem"));
        assert!(scene_render_planner_service.contains("SceneRenderParticleItem"));
        assert!(scene_render_planner_service.contains("SceneRenderSoundItem"));
        assert!(scene_render_planner_service.contains("SceneRenderSourceKind::Video"));
        assert!(!scene_render_planner_service.contains("VideoPosterFrame"));
        assert!(!scene_support_service.contains("VideoPosterFrame"));
        assert!(player_service.contains("scene_native_renderer_service::sync_native_scene_runtime"));
        assert!(player_service.contains("scene_requires_periodic_updates"));
        assert!(player_service.contains("match find_record(&store, &active_id)"));
        assert!(runtime_document_service.contains("cached_scene_manifest"));
        assert!(player_runtime.contains("NativeSceneStageSurface"));
    }

    #[test]
    fn phase_09_contract_adds_native_scene_audio_particle_input_and_text_enhancements() {
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let scene_audio_coordinator_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_audio_coordinator_service.rs"
        ));
        let scene_input_response_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_input_response_service.rs"
        ));
        let scene_particle_scheduler_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_particle_scheduler_service.rs"
        ));
        let scene_render_planner_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_render_planner_service.rs"
        ));
        let scene_support_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_support_service.rs"
        ));
        let player_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/player_service.rs"
        ));

        assert!(scene_native_renderer_service.contains("SceneAudioCoordinator"));
        assert!(scene_native_renderer_service.contains("SceneInputCoordinator"));
        assert!(scene_native_renderer_service.contains("SceneParticleScheduler"));
        assert!(scene_native_renderer_service.contains("text-font-fallback"));
        assert!(scene_native_renderer_service.contains("text-effect-unsupported"));
        assert!(scene_native_renderer_service.contains("native scene plan"));
        assert!(scene_audio_coordinator_service.contains("levels_for_count"));
        assert!(scene_audio_coordinator_service.contains("merge_scene_audio_levels"));
        assert!(scene_input_response_service.contains("SceneInputCoordinatorFrame"));
        assert!(scene_input_response_service.contains("SceneInputTarget"));
        assert!(scene_particle_scheduler_service.contains("emit_petals"));
        assert!(scene_particle_scheduler_service.contains("line_primitives"));
        assert!(scene_render_planner_service.contains("effect_paths"));
        assert!(scene_support_service.contains("native Scene runtime"));
        assert!(player_service.contains("scene_update_cadence"));
        assert!(player_service.contains("scene_update_sleep_duration"));
    }

    #[test]
    fn phase_10_contract_adds_native_scene_mdl_shader_material_and_render_graph() {
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let scene_mdl_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_mdl_service.rs"
        ));
        let scene_shader_material_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_shader_material_service.rs"
        ));
        let scene_render_graph_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_render_graph_service.rs"
        ));
        let scene_resource_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_resource_service.rs"
        ));
        let scene_runtime_settings_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_runtime_settings_service.rs"
        ));
        let scene_support_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_support_service.rs"
        ));
        let settings_commands = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/settings_commands.rs"
        ));
        let settings_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/settings-api.ts"
        ));
        let workbench = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/WorkbenchApp.tsx"
        ));

        assert!(scene_mdl_service.contains("parse_scene_mdl"));
        assert!(scene_mdl_service.contains("evaluate_scene_mdl_mesh"));
        assert!(scene_shader_material_service.contains("load_scene_material_plan"));
        assert!(scene_shader_material_service.contains("resolve_shader_program"));
        assert!(scene_render_graph_service.contains("build_scene_phase10_graph"));
        assert!(scene_native_renderer_service.contains("phase10_graph"));
        assert!(scene_native_renderer_service.contains("prepare_phase10_graph"));
        assert!(scene_native_renderer_service.contains("parse_scene_mdl_file"));
        assert!(scene_native_renderer_service.contains("load_shader_program_source"));
        assert!(scene_native_renderer_service.contains("build_scene_phase10_graph"));
        assert!(scene_support_service.contains("material-reference-unresolved"));
        assert!(scene_support_service.contains("MissingTextureBinding"));
        assert!(scene_resource_service.contains("SceneResourceRootKind::ExternalAssets"));
        assert!(scene_resource_service.contains("SceneResourceRootKind::BuiltinAssets"));
        assert!(scene_runtime_settings_service.contains("external_assets_path"));
        assert!(settings_commands.contains("get_scene_runtime_settings"));
        assert!(settings_commands.contains("set_scene_external_assets_path"));
        assert!(settings_gateway.contains("getSceneRuntimeSettings"));
        assert!(settings_gateway.contains("setSceneExternalAssetsPath"));
        assert!(workbench.contains("chooseSceneAssetsDirectory"));
        assert!(workbench.contains("sceneRuntimeSettings"));
        assert!(scene_shader_material_service.contains("assets/shaders/compat/scene-model.metal"));
    }

    #[test]
    fn phase_11_contract_removes_frontend_scene_player_fallback() {
        let player_runtime = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/app-shell/player-runtime.tsx"
        ));
        let player_gateway = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/gateway/player-api.ts"
        ));
        let player_commands = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/commands/player_commands.rs"
        ));
        let lib_rs = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"));
        let app_entry_runtime = lib_rs.split("#[cfg(test)]").next().unwrap_or(lib_rs);
        let scene_native_renderer_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_native_renderer_service.rs"
        ));
        let scene_support_service = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/services/scene_support_service.rs"
        ));

        assert!(player_runtime.contains("NativeSceneStageSurface"));
        assert!(!player_runtime.contains("function SceneStageSurface"));
        assert!(!player_runtime.contains("<SceneStageSurface"));
        assert!(!player_runtime.contains("SceneVisualNode"));
        assert!(!player_runtime.contains("SceneTextNode"));
        assert!(!player_runtime.contains("SceneAudioNode"));
        assert!(!player_runtime.contains("SceneSoundscape"));
        assert!(!player_runtime.contains("SceneParticleOverlay"));
        assert!(!player_runtime.contains("<video"));
        assert!(!player_runtime.contains("<audio"));
        assert!(!player_runtime.contains("FontFace"));
        assert!(!player_gateway.contains("get_player_input_snapshot"));
        assert!(!player_gateway.contains("get_player_audio_snapshot"));
        assert!(!player_gateway.contains("set_scene_audio_interest"));
        assert!(!player_commands.contains("get_player_input_snapshot"));
        assert!(!player_commands.contains("get_player_audio_snapshot"));
        assert!(!player_commands.contains("set_scene_audio_interest"));
        assert!(!app_entry_runtime.contains("get_player_input_snapshot"));
        assert!(!app_entry_runtime.contains("get_player_audio_snapshot"));
        assert!(!app_entry_runtime.contains("set_scene_audio_interest"));
        assert!(scene_native_renderer_service.contains("MTKView"));
        assert!(scene_support_service.contains("ensure_scene_supported_for_apply"));
        assert!(scene_support_service
            .contains("Scene native apply was blocked by support diagnostics."));
    }
}
