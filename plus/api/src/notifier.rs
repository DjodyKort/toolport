//! Operating-system notifications, which only the desktop app process can send.

use std::cell::{Cell, RefCell};

use serde::Serialize;

/// What the system lets the app do. `Unknown` is the honest answer on a platform that never
/// reports it: a notification handed over is then neither confirmed nor refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    Allowed,
    NotAsked,
    Blocked,
    #[default]
    Unknown,
}

impl Permission {
    pub fn word(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::NotAsked => "not-asked",
            Self::Blocked => "blocked",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        [Self::Allowed, Self::NotAsked, Self::Blocked, Self::Unknown]
            .into_iter()
            .find(|p| p.word() == word)
    }
}

/// The screen a row belongs to, so the window can stay quiet when it already shows the row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToastRow {
    pub id: String,
    pub screen: String,
}

/// What the window shows as an in-app toast when it has the focus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub rows: Vec<ToastRow>,
}

/// The raw calls of one platform. Everything that decides what to say lives above this layer, so
/// it can be tested with [`FakePlatform`].
pub trait NotifyPlatform {
    /// Hands one notification to the system. Success means handed over, not shown.
    fn show(&self, title: &str, body: &str) -> Result<(), String>;

    fn window_focused(&self) -> bool;

    fn permission(&self) -> Permission;

    /// Sends the toast to the window.
    fn emit_toast(&self, toast: &Toast) -> Result<(), String>;

    /// Opens the system's own notification settings for this app.
    fn open_settings(&self) -> Result<(), String>;
}

pub trait Notifier {
    /// Shows one notification; an error says why the system did not (blocked, no daemon).
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;

    /// Whether the app window has the focus right now.
    fn window_focused(&self) -> bool;

    fn permission(&self) -> Permission;

    /// Shows the toast inside the window.
    fn toast(&self, toast: &Toast) -> Result<(), String>;

    fn open_settings(&self) -> Result<(), String>;
}

#[derive(Default)]
pub struct FakeNotifier {
    pub sent: RefCell<Vec<(String, String)>>,
    pub toasts: RefCell<Vec<Toast>>,
    pub focused: Cell<bool>,
    pub refuse: RefCell<Option<String>>,
    pub permission: Cell<Permission>,
    pub settings_opened: Cell<u32>,
}

impl Notifier for FakeNotifier {
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        if let Some(reason) = self.refuse.borrow().clone() {
            return Err(reason);
        }
        self.sent.borrow_mut().push((title.into(), body.into()));
        Ok(())
    }

    fn window_focused(&self) -> bool {
        self.focused.get()
    }

    fn permission(&self) -> Permission {
        self.permission.get()
    }

    fn toast(&self, toast: &Toast) -> Result<(), String> {
        self.toasts.borrow_mut().push(toast.clone());
        Ok(())
    }

    fn open_settings(&self) -> Result<(), String> {
        self.settings_opened.set(self.settings_opened.get() + 1);
        Ok(())
    }
}

#[derive(Default)]
pub struct FakePlatform {
    pub shown: RefCell<Vec<(String, String)>>,
    pub toasts: RefCell<Vec<Toast>>,
    pub focused: Cell<bool>,
    pub permission: Cell<Permission>,
    /// What the system says when asked to show a notification, to model a daemon that is gone.
    pub show_error: RefCell<Option<String>>,
    pub emit_error: RefCell<Option<String>>,
    pub settings_opened: Cell<u32>,
}

impl NotifyPlatform for FakePlatform {
    fn show(&self, title: &str, body: &str) -> Result<(), String> {
        if let Some(error) = self.show_error.borrow().clone() {
            return Err(error);
        }
        self.shown.borrow_mut().push((title.into(), body.into()));
        Ok(())
    }

    fn window_focused(&self) -> bool {
        self.focused.get()
    }

    fn permission(&self) -> Permission {
        self.permission.get()
    }

    fn emit_toast(&self, toast: &Toast) -> Result<(), String> {
        if let Some(error) = self.emit_error.borrow().clone() {
            return Err(error);
        }
        self.toasts.borrow_mut().push(toast.clone());
        Ok(())
    }

    fn open_settings(&self) -> Result<(), String> {
        self.settings_opened.set(self.settings_opened.get() + 1);
        Ok(())
    }
}
