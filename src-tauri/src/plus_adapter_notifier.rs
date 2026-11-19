//! The desktop app's side of notifications: the raw calls of the plugin upstream already
//! registers. What they mean is decided in `toolport_plus::attention::system_notifier`.

use tauri::{Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};
use toolport_plus_api::{NotifyPlatform, Permission, Toast};

pub(crate) const TOAST_EVENT: &str = "plus-attention-toast";

pub(crate) struct AppPlatform {
    handle: tauri::AppHandle,
}

impl AppPlatform {
    pub(crate) fn new(handle: tauri::AppHandle) -> Self {
        Self { handle }
    }
}

impl NotifyPlatform for AppPlatform {
    fn show(&self, title: &str, body: &str) -> Result<(), String> {
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

    /// The plugin answers `Granted` on every desktop without asking the system, and `show` throws
    /// its own errors away, so an answer of `Granted` says nothing here. Only a refusal the
    /// plugin reports is believed.
    fn permission(&self) -> Permission {
        match self.handle.notification().permission_state() {
            Ok(PermissionState::Denied) => Permission::Blocked,
            Ok(PermissionState::Prompt | PermissionState::PromptWithRationale) => {
                Permission::NotAsked
            }
            Ok(PermissionState::Granted) | Err(_) => Permission::Unknown,
        }
    }

    fn emit_toast(&self, toast: &Toast) -> Result<(), String> {
        self.handle
            .emit_to("main", TOAST_EVENT, toast)
            .map_err(|e| e.to_string())
    }

    fn open_settings(&self) -> Result<(), String> {
        open_system_settings()
    }
}

#[cfg(target_os = "macos")]
fn open_system_settings() -> Result<(), String> {
    run_opener(
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.Notifications-Settings.extension"),
    )
}

#[cfg(windows)]
fn open_system_settings() -> Result<(), String> {
    run_opener(std::process::Command::new("explorer.exe").arg("ms-settings:notifications"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn open_system_settings() -> Result<(), String> {
    Err("this system has no notification settings Toolport+ can open".to_string())
}

#[cfg(any(target_os = "macos", windows))]
fn run_opener(command: &mut std::process::Command) -> Result<(), String> {
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open the system settings: {e}"))
}
