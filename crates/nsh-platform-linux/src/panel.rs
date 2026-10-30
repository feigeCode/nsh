//! Panel windows: a second window of the app (no GTK backend yet).
//!
//! The API is the one the macOS backend implements, so an app's panel code
//! compiles unchanged on every desktop; GTK opens one window per app
//! (`run_app`'s) today, and [`Panel::open`] fails with
//! [`PanelError::Unsupported`] before creating anything.

use std::{error::Error, fmt};

use nsh_core::desktop::WindowFrame;

use crate::{AppOptions, AppSource};

/// How a panel floats: its level in the window server's stacking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelLevel {
    /// With the other windows of the app.
    Normal,
    /// Above normal windows.
    #[default]
    Floating,
    /// Just above the Dock or the taskbar: over it and other floating
    /// windows.
    AboveDock,
}

/// Options of a [`Panel`]: the same shape on every desktop.
#[derive(Debug, Clone)]
pub struct PanelOptions {
    /// The window title.
    pub title: String,
    /// The window's content size in points. `None` opens it at 70% of the
    /// main screen's visible size, centered.
    pub size: Option<(i32, i32)>,
    /// How it floats (default [`PanelLevel::Floating`]).
    pub level: PanelLevel,
    /// No title bar and no frame (the default): the app draws every part of
    /// the window.
    pub borderless: bool,
    /// The window's background is transparent and the app draws what it
    /// wants (default `false`).
    pub transparent: bool,
    /// Hides the window when the app is not the active one (default
    /// `false`).
    pub hides_on_deactivate: bool,
    /// The window moves when its background is dragged (default `false`).
    pub movable_by_background: bool,
    /// The window takes the keyboard without activating the app (default
    /// `false`).
    pub nonactivating: bool,
    /// The app of the panel.
    pub app: AppOptions,
}

impl Default for PanelOptions {
    fn default() -> Self {
        Self {
            title: String::new(),
            size: None,
            level: PanelLevel::default(),
            borderless: true,
            transparent: false,
            hides_on_deactivate: false,
            movable_by_background: false,
            nonactivating: false,
            app: AppOptions::default(),
        }
    }
}

/// Why a [`Panel`] could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelError {
    /// The platform does not provide panel windows.
    Unsupported(&'static str),
    /// The options are not usable.
    Invalid(&'static str),
}

impl fmt::Display for PanelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => {
                write!(f, "panel windows are not supported here: {message}")
            }
            Self::Invalid(message) => write!(f, "invalid panel options: {message}"),
        }
    }
}

impl Error for PanelError {}

/// A panel window: none on Linux yet.
pub struct Panel;

impl Panel {
    /// Fails with [`PanelError::Unsupported`]: GTK has no panel windows
    /// yet.
    ///
    /// # Errors
    ///
    /// [`PanelError::Unsupported`], always.
    pub fn open(_options: PanelOptions, _source: AppSource) -> Result<Self, PanelError> {
        Err(PanelError::Unsupported(
            "the GTK backend opens one window per app",
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
