mod agents;
mod history;
mod overlay;
mod runtime;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use erindi_core::agent::Agent;
use erindi_core::controller::{Key, Msg};
use erindi_core::transcript::{self, Details};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

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
            list_microphones,
            set_hotkeys_paused,
            list_sessions,
            open_history_session,
            continue_session,
            delete_session,
            model_status,
            download_model,
            test_command,
            default_patterns,
            agent_status,
            recheck_agents
        ])
        .setup(|app| {
            overlay::create(app.handle())?;
            let path = app.path().app_config_dir()?.join("settings.json");
            let history_path = app.path().app_data_dir()?.join("sessions.json");
            let settings = Arc::new(RwLock::new(Settings::load(&path)));
            let agents = agents::Agents::default();
            agents.recheck(app.handle());
            app.manage(agents.clone());
            let runtime = Runtime::start(
                app.handle().clone(),
                settings.clone(),
                &history_path,
                agents,
            );
            if let Err(e) = register_hotkeys(app.handle(), &settings.read().unwrap(), &runtime) {
                eprintln!("{e}");
            }
            runtime.set_cleanup(settings.read().unwrap().model_commands);
            app.manage(runtime);
            if !erindi_core::models::SPEECH.installed(&runtime::models_dir()) {
                show_settings(app.handle());
            }
            app.manage(SettingsStore {
                path,
                shared: settings,
            });
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("bundled icon"))
                .tooltip("Erindi")
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
        .expect("failed to build Erindi")
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Sessions {
    entries: Vec<history::Entry>,
    active: Option<uuid::Uuid>,
    /// The model and permission each session has now, from the agent's own log.
    details: HashMap<uuid::Uuid, Details>,
}

#[tauri::command]
fn list_sessions(runtime: tauri::State<Runtime>) -> Sessions {
    let (entries, active) = runtime.sessions();
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default();
    let logs: HashMap<_, _> = Agent::ALL
        .into_iter()
        .map(|a| (a, transcript::find_logs(a, &home)))
        .collect();
    let details = entries
        .iter()
        .filter_map(|e| {
            let path = logs[&e.agent].get(e.native()?)?;
            Some((e.id, transcript::read(e.agent, path)?))
        })
        .collect();
    Sessions {
        entries,
        active,
        details,
    }
}

#[tauri::command]
fn agent_status(agents: tauri::State<agents::Agents>) -> Vec<agents::AgentStatus> {
    agents.status()
}

/// `force` re-checks now; otherwise only when the last check is stale, as on window focus.
#[tauri::command]
fn recheck_agents(app: AppHandle, agents: tauri::State<agents::Agents>, force: bool) {
    if force {
        agents.recheck(&app)
    } else {
        agents.recheck_if_stale(&app)
    }
}

#[tauri::command]
fn open_history_session(runtime: tauri::State<Runtime>, id: uuid::Uuid) -> Result<(), String> {
    runtime.open_history_session(id)
}

#[tauri::command]
fn continue_session(runtime: tauri::State<Runtime>, id: uuid::Uuid) -> Result<(), String> {
    runtime.continue_session(id)
}

#[tauri::command]
fn delete_session(runtime: tauri::State<Runtime>, id: uuid::Uuid) -> Result<(), String> {
    runtime.delete_session(id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelStatus {
    id: &'static str,
    label: &'static str,
    installed: bool,
    downloading: bool,
}

/// Models downloading now, so a remounted row keeps its progress and a second request is refused.
static DOWNLOADING: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());

#[tauri::command]
fn model_status() -> Vec<ModelStatus> {
    let dir = runtime::models_dir();
    [&erindi_core::models::SPEECH, &erindi_core::models::CLEANUP]
        .into_iter()
        .map(|m| ModelStatus {
            id: m.id,
            label: m.label,
            installed: m.installed(&dir),
            downloading: DOWNLOADING.lock().unwrap().contains(&m.id),
        })
        .collect()
}

#[derive(Clone, serde::Serialize)]
struct Progress {
    id: &'static str,
    done: u64,
    total: u64,
}

#[derive(Clone, serde::Serialize)]
struct Done {
    id: &'static str,
    error: Option<String>,
}

#[tauri::command]
fn download_model(
    app: AppHandle,
    runtime: tauri::State<Runtime>,
    id: String,
) -> Result<(), String> {
    let model = erindi_core::models::by_id(&id).ok_or("Unknown model")?;
    {
        let mut active = DOWNLOADING.lock().unwrap();
        if active.contains(&model.id) {
            return Err("This model is already downloading".into());
        }
        active.push(model.id);
    }
    let runtime = runtime.inner().clone();
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        let result = erindi_core::models::download(model, &runtime::models_dir(), |done, total| {
            if last.elapsed() >= std::time::Duration::from_millis(100) || done == total {
                last = std::time::Instant::now();
                let progress = Progress {
                    id: model.id,
                    done,
                    total,
                };
                let _ = app.emit_to("settings", "model-progress", progress);
            }
        });
        DOWNLOADING.lock().unwrap().retain(|id| *id != model.id);
        if result.is_ok() && model.id == "speech" {
            runtime.load_speech();
        }
        let done = Done {
            id: model.id,
            error: result.err(),
        };
        let _ = app.emit_to("settings", "model-done", done);
    });
    Ok(())
}

#[derive(serde::Serialize)]
struct TestResult {
    commands: Vec<erindi_core::commands::Command>,
    rest: String,
}

/// What the parser makes of `text` with the patterns on screen, saved or not.
#[tauri::command]
fn test_command(
    patterns: erindi_core::commands::Patterns,
    text: String,
) -> Result<TestResult, String> {
    let (commands, rest) = erindi_core::commands::Parser::new(&patterns)?.parse(&text);
    Ok(TestResult { commands, rest })
}

#[tauri::command]
fn default_patterns() -> erindi_core::commands::Patterns {
    erindi_core::commands::Patterns::default()
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
    if settings.model_commands && !erindi_core::models::CLEANUP.installed(&runtime::models_dir()) {
        return Err("Download the command model first".into());
    }
    settings.save(&store.path)?;
    *store.shared.write().unwrap() = settings.clone();
    runtime.send(settings.session_msg());
    runtime.set_cleanup(settings.model_commands);
    app.state::<agents::Agents>().recheck(&app);
    register_hotkeys(&app, &settings, &runtime)
}

/// Lets the settings window record a hotkey without triggering the registered ones.
#[tauri::command]
fn set_hotkeys_paused(
    app: AppHandle,
    store: tauri::State<SettingsStore>,
    runtime: tauri::State<Runtime>,
    paused: bool,
) -> Result<(), String> {
    if paused {
        return app
            .global_shortcut()
            .unregister_all()
            .map_err(|e| e.to_string());
    }
    let settings = store.shared.read().unwrap().clone();
    register_hotkeys(&app, &settings, &runtime)
}

#[tauri::command]
fn list_microphones() -> Vec<String> {
    erindi_audio_asr::capture::input_devices()
}

fn register_hotkeys(app: &AppHandle, settings: &Settings, runtime: &Runtime) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    let mut errors = vec![];
    for (combo, key) in [
        (&settings.talk_hotkey, Key::Talk),
        (&settings.new_session_hotkey, Key::NewSession),
        (&settings.terminal_hotkey, Key::Terminal),
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

pub(crate) fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.eval("location.hash = 'settings'");
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App("index.html#settings".into()),
    )
    .title("Erindi")
    .inner_size(880.0, 680.0)
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

    #[test]
    fn settings_window_can_manage_models() {
        let cap = capability(include_str!("../capabilities/settings.json"));
        let perms = cap["permissions"].as_array().unwrap();
        for p in [
            "allow-model-status",
            "allow-download-model",
            "allow-test-command",
            "allow-default-patterns",
            "allow-agent-status",
            "allow-recheck-agents",
        ] {
            assert!(perms.contains(&json!(p)), "{p}");
        }
    }
}
