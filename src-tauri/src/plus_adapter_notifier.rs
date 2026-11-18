//! The desktop app's notifications, through the plugin upstream already registers.

use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use toolport_plus_api::Notifier;

pub(crate) struct AppNotifier {
    handle: tauri::AppHandle,
}

impl AppNotifier {
    pub(crate) fn new(handle: tauri::AppHandle) -> Self {
        Self { handle }
    }
}

impl Notifier for AppNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        self.handle
            .notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|e| e.to_string())
    }

    fn window_focused(&self) -> bool {
        self.handle
            .get_webview_window("main")
            .is_some_and(|window| window.is_focused().unwrap_or(false))
    }
}
