fn main() {
    let commands = tauri_build::AppManifest::new().commands(&[
        "open_session",
        "get_settings",
        "save_settings",
        "list_microphones",
        "set_hotkeys_paused",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(commands))
        .expect("failed to run tauri-build");
}
