//! The app's [`Notifier`]: the decisions about what the system's answers mean, over the raw calls
//! of a [`NotifyPlatform`]. A fake platform stands in for the desktop in tests.

use toolport_plus_api::{Notifier, NotifyPlatform, Permission, Toast};

pub const BLOCKED: &str = "the system blocks notifications from Toolport+; allow them in System Settings > Notifications > Toolport+";

pub struct SystemNotifier<P: NotifyPlatform>(pub P);

impl<P: NotifyPlatform> Notifier for SystemNotifier<P> {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        if self.0.permission() == Permission::Blocked {
            return Err(BLOCKED.to_string());
        }
        self.0.show(title, body)
    }

    fn window_focused(&self) -> bool {
        self.0.window_focused()
    }

    fn permission(&self) -> Permission {
        self.0.permission()
    }

    fn toast(&self, toast: &Toast) -> Result<(), String> {
        self.0.emit_toast(toast)
    }

    fn open_settings(&self) -> Result<(), String> {
        self.0.open_settings()
    }
}
