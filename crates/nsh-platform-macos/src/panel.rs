//! Panel windows: a second window of the app, beside [`run_app`]'s.
//!
//! A panel is a window of its own, as a floating palette or a popover is:
//! it shows an [`AppSource`] of its own (a Rust root, a bundle or a dev
//! server), so it can run a different app than the main window, with an
//! engine of its own ([`AppView`]). It has no title bar by default, can
//! float above other apps' windows ([`PanelLevel`]) and can take the
//! keyboard without activating the app ([`PanelOptions::nonactivating`]).
//! A panel lives exactly as long as its [`Panel`] does: dropping it closes
//! the window and stops its app.
//!
//! [`Panel::open`] runs on the main thread, as [`run_app`] does. Rust
//! components run on the engine thread, so a panel is opened by the host's
//! main-thread code (as an app's runner does), not from a component's event
//! handler.

use std::{
    cell::{Cell, RefCell},
    error::Error,
    fmt,
    rc::Rc,
};

use nsh_core::desktop::WindowFrame;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained};
use objc2_app_kit::{
    NSApplicationActivationOptions, NSBackingStoreType, NSColor, NSFloatingWindowLevel,
    NSNormalWindowLevel, NSPanel, NSResponder, NSRunningApplication, NSScreen, NSWindow,
    NSWindowLevel, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::{App, AppOptions, AppSource, AppView};

/// How a panel floats: its level in the window server's stacking order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelLevel {
    /// With the other windows of the app (below the Dock).
    Normal,
    /// Above normal windows, below the Dock, menus and system panels
    /// (Spotlight).
    #[default]
    Floating,
    /// Just above the Dock: over the Dock and other floating windows, below
    /// menus.
    AboveDock,
}

/// Options of a [`Panel`].
#[derive(Debug, Clone)]
pub struct PanelOptions {
    /// The window title (in the window's title bar when it has one, and the
    /// window server's name for it).
    pub title: String,
    /// The window's content size in points. `None` (the default) opens it at
    /// 70% of the main screen's visible size, centered.
    pub size: Option<(i32, i32)>,
    /// How it floats (default [`PanelLevel::Floating`]).
    pub level: PanelLevel,
    /// No title bar and no frame (the default), as palettes and popovers
    /// have it: the app draws every part of the window. `false` gives a
    /// titled window with the system's close and minimize buttons.
    pub borderless: bool,
    /// The window's background is transparent and the app draws what it
    /// wants (default `false`).
    pub transparent: bool,
    /// Hides the window when the app is not the active one (default
    /// `false`).
    pub hides_on_deactivate: bool,
    /// The window moves when its background is dragged (default `false`): a
    /// borderless panel is dragged by whatever it draws where the app has no
    /// control of its own.
    pub movable_by_background: bool,
    /// The window takes the keyboard without activating the app (default
    /// `false`): the app in front keeps its active look while the panel is
    /// typed into.
    pub nonactivating: bool,
    /// The app of the panel: its title and size are not used (the panel's
    /// are), and a bundle or dev server carries the rest.
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
    /// The platform does not provide panel windows (`Panel::open` was called
    /// off the main thread on macOS; Windows and Linux have none).
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

/// A panel window (see the [module](self)'s documentation): its own window
/// and app, closed and stopped when it is dropped.
///
/// Drop it on the main thread, where it was opened.
pub struct Panel {
    window: Retained<PanelWindow>,
    app: Rc<AppView>,
    /// The app in front before the panel showed, to give it the focus back.
    previous: RefCell<Option<Retained<NSRunningApplication>>>,
    closed: Cell<bool>,
}

impl Panel {
    /// Opens a panel window showing `source` with `options`, without
    /// ordering it in front: call [`show`](Self::show) to show it.
    ///
    /// # Errors
    ///
    /// [`PanelError::Unsupported`] off the main thread, and
    /// [`PanelError::Invalid`] for a size that is not positive.
    pub fn open(options: PanelOptions, source: AppSource) -> Result<Self, PanelError> {
        let mtm = MainThreadMarker::new().ok_or(PanelError::Unsupported(
            "a panel window must be opened on the main thread",
        ))?;
        let (width, height) = panel_size(mtm, options.size)?;
        let mut style = if options.borderless {
            NSWindowStyleMask::Borderless
        } else {
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
        };
        if options.nonactivating {
            // Only an NSPanel takes this mask: the window does not activate
            // the app when it is clicked.
            style |= NSWindowStyleMask::NonactivatingPanel;
        }
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        // SAFETY: NSPanel's designated initializer; the alloc is the panel's
        // class and the style is a panel's.
        let window: Retained<PanelWindow> = unsafe {
            msg_send![
                PanelWindow::alloc(mtm),
                initWithContentRect: frame,
                styleMask: style,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        // SAFETY: the panel keeps the window in `self.window` until it closes
        // it (or is dropped), so AppKit must not release it on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(!options.transparent);
        let background = if options.transparent {
            NSColor::clearColor()
        } else {
            NSColor::windowBackgroundColor()
        };
        window.setBackgroundColor(Some(&background));
        window.setHasShadow(true);
        window.setMovableByWindowBackground(options.movable_by_background);
        window.setHidesOnDeactivate(options.hides_on_deactivate);
        window.setLevel(level_of(options.level));
        window.setTitle(&NSString::from_str(&options.title));
        crate::system::open_window(&window, options.app.appearance);
        let view = AppView::new(mtm, options.app);
        window.setContentView(Some(&view.view()));
        window.center();
        match source {
            AppSource::Rust(root) => view.load(App::Rust(root)),
            AppSource::Bundle(bundle) => view.load(App::Js(bundle)),
            #[cfg(feature = "dev")]
            AppSource::DevServer(url) => crate::dev::start(&view, url),
        }
        window.makeFirstResponder(Some(view.surface().root.ns()));
        Ok(Self {
            window,
            app: view,
            previous: RefCell::new(None),
            closed: Cell::new(false),
        })
    }

    /// Shows the window in front of the windows at its level, and makes it
    /// the key window (typed into) without activating the app.
    pub fn show(&self) {
        // Remember the app in front, so `hide` gives the focus back to it.
        *self.previous.borrow_mut() = NSWorkspace::sharedWorkspace().frontmostApplication();
        self.window.makeKeyAndOrderFront(None);
    }

    /// Hides the window, giving the focus back to the app that had it before
    /// the panel showed (when this app still has it).
    pub fn hide(&self) {
        self.window.orderOut(None);
        if let Some(previous) = self.previous.borrow_mut().take() {
            // The app that had the focus before the panel showed takes it
            // back; when the app itself had it, this keeps it there.
            let _ = previous.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }

    /// Whether the window is on screen.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    /// Moves and sizes the window, as [`WindowFrame`] does for the main
    /// window: a field left out keeps its value.
    pub fn set_frame(&self, frame: WindowFrame) {
        crate::chrome::set_frame(&self.window, &frame);
    }

    /// Changes how the window floats (see [`PanelLevel`]).
    pub fn set_level(&self, level: PanelLevel) {
        self.window.setLevel(level_of(level));
    }

    /// Sets the window title (shown when the panel has a title bar).
    pub fn set_title(&self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }

    /// Closes the window and stops its app.
    pub fn close(self) {
        self.closed.set(true);
        self.window.orderOut(None);
        self.window.close();
        self.app.stop();
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        if !self.closed.get() {
            self.window.close();
            self.app.stop();
        }
    }
}

/// The panel's content size: 70% of the main screen when not given.
fn panel_size(mtm: MainThreadMarker, size: Option<(i32, i32)>) -> Result<(f64, f64), PanelError> {
    match size {
        Some((width, height)) if width > 0 && height > 0 => {
            Ok((f64::from(width), f64::from(height)))
        }
        Some(_) => Err(PanelError::Invalid("a panel's size must be positive")),
        None => {
            let visible = NSScreen::mainScreen(mtm)
                .map(|screen| screen.visibleFrame())
                .unwrap_or_else(|| NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 800.0)));
            Ok((visible.size.width * 0.7, visible.size.height * 0.7))
        }
    }
}

/// The window level of a [`PanelLevel`].
fn level_of(level: PanelLevel) -> NSWindowLevel {
    match level {
        PanelLevel::Normal => NSNormalWindowLevel,
        PanelLevel::Floating => NSFloatingWindowLevel,
        // Just above the Dock (the Dock's level is not a window level of
        // AppKit's: the window server's key gives it, and one more is above
        // it).
        PanelLevel::AboveDock => {
            NSWindowLevel::try_from(CGWindowLevelForKey(CGWindowLevelKey::DockWindowLevelKey) + 1)
                .unwrap_or(NSFloatingWindowLevel)
        }
    }
}

define_class!(
    /// A panel window whose borderless (and nonactivating) style still takes
    /// the keyboard: AppKit's `NSPanel` takes it only with a title bar.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NshPanelWindow"]
    struct PanelWindow;

    unsafe impl NSObjectProtocol for PanelWindow {}

    impl PanelWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        // A panel is never the app's main window: the main window keeps its
        // title bar and its menu bar.
        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            false
        }
    }
);

#[cfg(test)]
mod tests {
    use super::{PanelLevel, level_of};

    #[test]
    fn levels_stack_normal_floating_above_dock() {
        let normal = level_of(PanelLevel::Normal);
        let floating = level_of(PanelLevel::Floating);
        let above_dock = level_of(PanelLevel::AboveDock);
        assert!(normal < floating);
        assert!(
            floating < above_dock,
            "AboveDock is over the other floating windows, below menus"
        );
    }
}
