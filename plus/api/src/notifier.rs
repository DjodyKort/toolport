//! Operating-system notifications, which only the desktop app process can send.

use std::cell::{Cell, RefCell};

pub trait Notifier {
    /// Shows one notification; an error says why the system did not (no permission, no daemon).
    fn notify(&self, title: &str, body: &str) -> Result<(), String>;

    /// Whether the app window has the focus right now.
    fn window_focused(&self) -> bool;
}

#[derive(Default)]
pub struct FakeNotifier {
    pub sent: RefCell<Vec<(String, String)>>,
    pub focused: Cell<bool>,
    pub refuse: RefCell<Option<String>>,
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
}
