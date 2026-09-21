mod overlay;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_settings(app)
        }))
        .setup(|app| {
            overlay::create(app.handle())?;
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
    fn overlay_can_only_listen_to_events() {
        let cap = capability(include_str!("../capabilities/overlay.json"));
        assert_eq!(cap["windows"], json!(["overlay"]));
        assert_eq!(
            cap["permissions"],
            json!(["core:event:allow-listen", "core:event:allow-unlisten"])
        );
    }

    #[test]
    fn settings_capability_is_scoped_to_settings_window() {
        let cap = capability(include_str!("../capabilities/settings.json"));
        assert_eq!(cap["windows"], json!(["settings"]));
    }
}
