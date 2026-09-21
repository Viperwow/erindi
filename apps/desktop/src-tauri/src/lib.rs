mod overlay;
mod runtime;
mod settings;

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use whispio_core::controller::{Key, Msg};

use crate::runtime::{Runtime, SharedSettings};
use crate::settings::Settings;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_settings(app)
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            open_session,
            get_settings,
            save_settings,
            list_microphones
        ])
        .setup(|app| {
            overlay::create(app.handle())?;
            let path = app.path().app_config_dir()?.join("settings.json");
            let settings = Arc::new(RwLock::new(Settings::load(&path)));
            let runtime = Runtime::start(app.handle().clone(), settings.clone());
            if let Err(e) = register_hotkeys(app.handle(), &settings.read().unwrap(), &runtime) {
                eprintln!("{e}");
            }
            app.manage(runtime);
            app.manage(SettingsStore {
                path,
                shared: settings,
            });
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip("Whispio")
                .menu(&Menu::with_items(app, &[&settings, &quit])?)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => show_settings(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Whispio")
        .run(|_, event| {
            // Closing the settings window must not quit the tray app.
            if let RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}

#[tauri::command]
fn open_session(runtime: tauri::State<Runtime>) -> Result<(), String> {
    runtime.open_session()
}

struct SettingsStore {
    path: PathBuf,
    shared: SharedSettings,
}

#[tauri::command]
fn get_settings(store: tauri::State<SettingsStore>) -> Settings {
    store.shared.read().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    store: tauri::State<SettingsStore>,
    runtime: tauri::State<Runtime>,
    settings: Settings,
) -> Result<(), String> {
    settings.validate()?;
    settings.save(&store.path)?;
    *store.shared.write().unwrap() = settings.clone();
    register_hotkeys(&app, &settings, &runtime)
}

#[tauri::command]
fn list_microphones() -> Vec<String> {
    whispio_audio_asr::capture::input_devices()
}

fn register_hotkeys(app: &AppHandle, settings: &Settings, runtime: &Runtime) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut errors = vec![];
    for (combo, key) in [
        (&settings.hold_hotkey, Key::Hold),
        (&settings.toggle_hotkey, Key::Toggle),
    ] {
        let runtime = runtime.clone();
        let registered = shortcuts.on_shortcut(combo.as_str(), move |_, _, event| {
            runtime.send(match event.state() {
                ShortcutState::Pressed => Msg::KeyDown(key),
                ShortcutState::Released => Msg::KeyUp(key),
            })
        });
        if let Err(e) = registered {
            errors.push(format!("Hotkey {combo} is unavailable: {e}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .title("Whispio")
        .inner_size(520.0, 620.0)
        .build();
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    fn capability(json: &str) -> Value {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn overlay_can_only_listen_and_open_the_session() {
        let cap = capability(include_str!("../capabilities/overlay.json"));
        assert_eq!(cap["windows"], json!(["overlay"]));
        assert_eq!(
            cap["permissions"],
            json!([
                "core:event:allow-listen",
                "core:event:allow-unlisten",
                "allow-open-session"
            ])
        );
    }

    #[test]
    fn settings_capability_is_scoped_to_settings_window() {
        let cap = capability(include_str!("../capabilities/settings.json"));
        assert_eq!(cap["windows"], json!(["settings"]));
    }
}
