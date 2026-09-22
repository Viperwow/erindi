fn main() {
    // tauri-build embeds icons/icon.ico into the exe but does not rerun when it changes.
    println!("cargo:rerun-if-changed=icons");
    let commands = tauri_build::AppManifest::new().commands(&[
        "open_session",
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
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("failed to run tauri-build");
}
