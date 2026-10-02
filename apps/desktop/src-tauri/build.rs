fn main() {
    // tauri-build embeds icons/icon.ico into the exe but does not rerun when it changes.
    println!("cargo:rerun-if-changed=icons");
    // The macOS bundle resource `llama/` is filled by scripts/fetch-llama-macos.sh; without it
    // tauri-build fails, so checks and tests build with an empty one.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        std::fs::create_dir_all("llama").expect("create llama/");
    }
    let commands = tauri_build::AppManifest::new().commands(&[
        "open_session",
        "set_bubble_rect",
        "get_settings",
        "save_settings",
        "list_microphones",
        "set_hotkeys_paused",
        "list_sessions",
        "open_history_session",
        "continue_session",
        "delete_session",
        "model_status",
        "download_model",
        "test_command",
        "default_patterns",
        "agent_status",
        "recheck_agents",
        "codex_limited",
        "folder_untrusted",
        "trust_folder",
        "has_api_key",
        "set_api_key",
        "clear_api_key",
        "api_models",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("failed to run tauri-build");
}
