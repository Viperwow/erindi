use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

/// Aurora strip plus room for the transcript bubble above it.
const HEIGHT: u32 = 200;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("overlay.html".into()))
        .title("Erindi Overlay")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .build()?;
    window.set_ignore_cursor_events(true)?;
    Ok(())
}

/// Shows the overlay along the bottom of the monitor under the mouse cursor.
pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window("overlay") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        return;
    }
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let area = monitor.work_area();
        let _ = window.set_size(PhysicalSize::new(area.size.width, HEIGHT));
        let _ = window.set_position(PhysicalPosition::new(
            area.position.x,
            area.position.y + area.size.height as i32 - HEIGHT as i32,
        ));
    }
    let _ = window.show();
}

/// Region of the transcript bubble, measured from the bottom center of the overlay.
const BUBBLE_HALF_WIDTH: f64 = 400.0;
const BUBBLE_BOTTOM: f64 = 48.0;
const BUBBLE_TOP: f64 = 190.0;

/// Accepts clicks only while the cursor is over the bubble, so the transparent rest of the
/// overlay never swallows clicks meant for other apps. Stops once `active` moves off `op`.
pub fn track_bubble_hover(app: &AppHandle, active: Arc<AtomicU64>, op: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window("overlay") else {
            return;
        };
        while active.load(Ordering::SeqCst) == op {
            let inside = (|| {
                let cursor = app.cursor_position().ok()?;
                let pos = window.outer_position().ok()?;
                let size = window.outer_size().ok()?;
                let scale = window.scale_factor().ok()?;
                let center = pos.x as f64 + size.width as f64 / 2.0;
                let bottom = pos.y as f64 + size.height as f64;
                Some(
                    (cursor.x - center).abs() <= BUBBLE_HALF_WIDTH * scale
                        && cursor.y <= bottom - BUBBLE_BOTTOM * scale
                        && cursor.y >= bottom - BUBBLE_TOP * scale,
                )
            })()
            .unwrap_or(false);
            let _ = window.set_ignore_cursor_events(!inside);
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = window.set_ignore_cursor_events(true);
    });
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("overlay") {
        let _ = window.hide();
    }
}
