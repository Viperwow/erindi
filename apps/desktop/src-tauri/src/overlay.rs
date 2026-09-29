use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

/// The bubble plus room for a tooltip above it.
const HEIGHT: u32 = 260;

/// The bubble's box in CSS pixels, relative to the overlay window, as the page reports it.
#[derive(Clone, Copy, Default, Debug, serde::Deserialize)]
pub struct Rect {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Clone, Default)]
pub struct BubbleRect(pub Arc<Mutex<Rect>>);

#[tauri::command]
pub fn set_bubble_rect(rect: Rect, state: tauri::State<BubbleRect>) {
    *state.0.lock().unwrap() = rect;
}

/// Whether a cursor in physical pixels is over the bubble of a window at `window` with `scale`.
fn inside(cursor: (f64, f64), window: (f64, f64), scale: f64, rect: Rect) -> bool {
    let (x, y) = ((cursor.0 - window.0) / scale, (cursor.1 - window.1) / scale);
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

fn keep_tracking(active: &AtomicU64, op: u64) -> bool {
    active.load(Ordering::SeqCst) == op
}

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

/// Takes the mouse only while the cursor is over the bubble, so tooltips and clicks work there
/// and the transparent rest of the overlay never swallows clicks meant for other apps.
/// Stops once `active` moves off `op`.
pub fn track_bubble_hover(app: &AppHandle, active: Arc<AtomicU64>, op: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(window) = app.get_webview_window("overlay") else {
            return;
        };
        let rect = app.state::<BubbleRect>().inner().clone();
        while keep_tracking(&active, op) {
            let over = (|| {
                let cursor = app.cursor_position().ok()?;
                let pos = window.inner_position().ok()?;
                let scale = window.scale_factor().ok()?;
                let bubble = *rect.0.lock().unwrap();
                Some(inside(
                    (cursor.x, cursor.y),
                    (pos.x as f64, pos.y as f64),
                    scale,
                    bubble,
                ))
            })()
            .unwrap_or(false);
            let _ = window.set_ignore_cursor_events(!over);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_the_bubble_only() {
        let r = Rect {
            left: 100.0,
            top: 50.0,
            right: 460.0,
            bottom: 190.0,
        };
        assert!(inside((200.0, 100.0), (0.0, 0.0), 1.0, r));
        assert!(!inside((50.0, 100.0), (0.0, 0.0), 1.0, r));
        assert!(!inside((200.0, 195.0), (0.0, 0.0), 1.0, r));
    }

    #[test]
    fn inside_uses_the_window_scale() {
        let r = Rect {
            left: 100.0,
            top: 50.0,
            right: 460.0,
            bottom: 190.0,
        };
        assert!(inside(
            (1000.0 + 300.0, 500.0 + 150.0),
            (1000.0, 500.0),
            1.5,
            r
        ));
        assert!(!inside(
            (1000.0 + 700.0, 500.0 + 150.0),
            (1000.0, 500.0),
            1.5,
            r
        ));
    }

    #[test]
    fn hover_stops_when_the_op_changes() {
        let active = Arc::new(AtomicU64::new(7));
        assert!(keep_tracking(&active, 7));
        active.store(8, Ordering::SeqCst);
        assert!(!keep_tracking(&active, 7));
    }
}
