//! Panel windows: a second window of the app (no Win32 backend yet).
//!
//! The API is the one the macOS backend implements, so an app's panel code
//! compiles unchanged on every desktop; Windows opens one window per app
//! (`run_app`'s) today, and [`Panel::open`] fails with
//! [`PanelError::Unsupported`] before creating anything.

use nsh_core::desktop::WindowFrame;
pub use nsh_core::desktop::{PanelError, PanelLevel, PanelOptions};

use crate::{AppOptions, AppSource};

/// A panel window: none on Windows yet.
pub struct Panel;

impl Panel {
    /// Fails with [`PanelError::Unsupported`]: Windows has no panel windows
    /// yet.
    ///
    /// # Errors
    ///
    /// [`PanelError::Unsupported`], always.
    pub fn open(
        _options: PanelOptions,
        _app: AppOptions,
        _source: AppSource,
    ) -> Result<Self, PanelError> {
        Err(PanelError::Unsupported(
            "the Windows backend opens one window per app",
        ))
    }

    /// Does nothing: there is no window to show.
    pub fn show(&self) {}

    /// Does nothing: there is no window to hide.
    pub fn hide(&self) {}

    /// Whether the window is on screen: `false`, always.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        false
    }

    /// Does nothing: there is no window to move.
    pub fn set_frame(&self, _frame: WindowFrame) {}

    /// Does nothing: there is no window to raise.
    pub fn set_level(&self, _level: PanelLevel) {}

    /// Does nothing: there is no window to title.
    pub fn set_title(&self, _title: &str) {}

    /// Does nothing: there is no window to close.
    pub fn close(self) {}
}
