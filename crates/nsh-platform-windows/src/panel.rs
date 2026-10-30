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
//! [`Panel::open`] runs on the thread that runs the host's message loop, as
//! [`run_app`] does: top-level windows of one thread share its loop, so the
//! host opens a panel and keeps running. Rust components run on the engine
//! thread, so a panel is opened by the host's main-thread code (as an app's
//! runner does), not from a component's event handler.
//!
//! Win32 has no panel concept, so the window is made to look like one:
//! a borderless window is a `WS_POPUP` window without a caption, `show`
//! shows it without activating the app (and `hide` hands the focus back to
//! the window that was in front), and [`PanelLevel::Floating`] and
//! [`PanelLevel::AboveDock`] are both `HWND_TOPMOST` (Windows has no
//! desktop-icon level to float above instead). [`PanelOptions::position`]
//! places the window's outer frame in screen points at its own scale, and
//! [`PanelOptions::hides_on_resign_key`] hides it from the window procedure,
//! as it loses the keyboard (`WM_KILLFOCUS`): a nonactivating panel never
//! gets an activation message.
//!
//! [`PanelOptions::transparent`], [`PanelOptions::hides_on_deactivate`] and
//! [`PanelOptions::movable_by_background`] have no Win32 equivalent here
//! and do nothing: transparency would need a layered window the Direct2D
//! swap chain does not draw through, hiding on deactivation would need the
//! app's activation, which a panel does not change, and moving by
//! background would need its hit tests.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    rc::Rc,
};

pub use nsh_core::desktop::{PanelError, PanelLevel, PanelOptions};
use nsh_core::{
    desktop::WindowFrame,
    geometry::{Points, Size},
};
use nsh_engine::App;
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        UI::{
            HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem},
            WindowsAndMessaging::{
                DestroyWindow, GWL_EXSTYLE, GWL_STYLE, GetForegroundWindow, GetWindowLongPtrW,
                GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, IsWindow, IsWindowVisible,
                SPI_GETWORKAREA, SW_HIDE, SW_SHOWNA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetForegroundWindow,
                SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW,
                WINDOW_EX_STYLE, WINDOW_STYLE, WS_CAPTION, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP, WS_SYSMENU, WS_THICKFRAME,
            },
        },
    },
    core::HSTRING,
};

use crate::{AppOptions, AppSource, AppView};

/// A panel window (see the [module](self)'s documentation): its own window
/// and app, closed and stopped when it is dropped. Drop it on the thread
/// that runs the message loop, where it was opened.
///
/// The `app`'s [`AppOptions`] are the app's own, not the window's: its
/// `data_dir`, `limits` and `components` come from the host, as they do for
/// the [main window](run_app)'s app, and every app keeps its data in
/// `<data_dir>/<app id>` ([`AppOptions::data_dir`]), so apps of different
/// ids stay apart.
///
/// The window's title is [`PanelOptions::title`]: the `app`'s
/// [`AppOptions::title`] names the [main window](run_app), which a panel
/// does not have.
///
/// The `app`'s calls and posts reach the host on the engine thread
/// ([`AppOptions::host`]): a host that shows, hides or closes a panel from
/// one of them hands it back to the main thread first, as windows belong to
/// the thread that runs the message loop.
pub struct Panel {
    app: Rc<AppView>,
    /// The level the window has, to re-assert it when it shows again.
    level: Cell<PanelLevel>,
    /// The window in front before the panel showed, to give it the focus
    /// back.
    previous: RefCell<Option<HWND>>,
}

impl Panel {
    /// Opens a panel window showing `source` with `options`, without
    /// showing it: call [`show`](Self::show) to show it.
    ///
    /// # Errors
    ///
    /// [`PanelError::Invalid`] when the panel's size is not positive, or
    /// when Win32 does not create the window (it is logged).
    pub fn open(
        options: PanelOptions,
        app: AppOptions,
        source: AppSource,
    ) -> Result<Self, PanelError> {
        let size = panel_size(options.size)?;
        let title = options.title.clone();
        let view = match AppView::new(None, AppOptions { size, title, ..app }) {
            Ok(view) => view,
            Err(error) => {
                log::error!("the panel's window could not be created: {error}");
                return Err(PanelError::Invalid(
                    "the panel's window could not be created",
                ));
            }
        };
        apply(&view, &options, size);
        if let Some((x, y)) = options.position {
            place(&view, x, y);
        }
        match source {
            AppSource::Rust(root) => view.load(App::Rust(root)),
            AppSource::Bundle(bundle) => view.load(App::Js(bundle)),
            #[cfg(feature = "dev")]
            AppSource::DevServer(url) => crate::dev::start(&view, url),
        }
        Ok(Self {
            app: view,
            level: Cell::new(options.level),
            previous: RefCell::new(None),
        })
    }

    /// Shows the window above the other windows of its level, without
    /// taking the focus or activating the app: a click raises it and gives
    /// it the keyboard, as a panel's click does.
    pub fn show(&self) {
        let hwnd = self.app.hwnd();
        if hwnd.is_invalid() {
            return;
        }
        // Remember what was in front, so `hide` gives it the focus back.
        // SAFETY: a plain query.
        let foreground = unsafe { GetForegroundWindow() };
        if !foreground.is_invalid() && foreground != hwnd {
            *self.previous.borrow_mut() = Some(foreground);
        }
        // SAFETY: plain calls on the panel's live window; `SW_SHOWNA`
        // shows it without activating it, and the level is re-asserted
        // without moving or resizing it.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
            let _ = SetWindowPos(
                hwnd,
                Some(order_of(self.level.get())),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
    }

    /// Hides the window and gives the focus back to the window that was in
    /// front before it showed.
    pub fn hide(&self) {
        self.dismiss();
    }

    /// Whether the window is on screen.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        let hwnd = self.app.hwnd();
        if hwnd.is_invalid() {
            return false;
        }
        // SAFETY: a plain query.
        unsafe { IsWindowVisible(hwnd) }.as_bool()
    }

    /// Moves and sizes the window: a field left out keeps its value. The
    /// points are pixels at the window's scale; Win32 works on the outer
    /// frame (non-client area included).
    pub fn set_frame(&self, frame: WindowFrame) {
        let hwnd = self.app.hwnd();
        if hwnd.is_invalid() {
            return;
        }
        let mut rect = RECT::default();
        // SAFETY: a plain query with a local out value.
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            return;
        }
        let scale = self.app.surface().scale();
        let x = frame.x.map_or(rect.left, |v| (v.0 * scale).round() as i32);
        let y = frame.y.map_or(rect.top, |v| (v.0 * scale).round() as i32);
        let width = frame
            .width
            .map_or(rect.right - rect.left, |v| (v.0 * scale).round() as i32);
        let height = frame
            .height
            .map_or(rect.bottom - rect.top, |v| (v.0 * scale).round() as i32);
        // SAFETY: a plain call on a live window; `SWP_NOZORDER` keeps the
        // level the panel has.
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    /// Sets the level the window floats at: `HWND_TOPMOST` for
    /// [`PanelLevel::Floating`] and [`PanelLevel::AboveDock`] (Windows has
    /// no desktop-icon level, so both are topmost), `HWND_NOTOPMOST` for
    /// [`PanelLevel::Normal`].
    pub fn set_level(&self, level: PanelLevel) {
        self.level.set(level);
        let hwnd = self.app.hwnd();
        if hwnd.is_invalid() {
            return;
        }
        // SAFETY: plain call on a live window; no other field changes, so
        // its position and size stay.
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                Some(order_of(level)),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        };
    }

    /// Sets the window's title.
    pub fn set_title(&self, title: &str) {
        let hwnd = self.app.hwnd();
        if hwnd.is_invalid() {
            return;
        }
        // SAFETY: a plain call on a live window with a temporary string.
        let _ = unsafe { SetWindowTextW(hwnd, &HSTRING::from(title)) };
    }

    /// Closes the window, stops its app and gives the focus back: [`Drop`]
    /// then has nothing left to do.
    pub fn close(self) {
        self.dismiss();
        self.app.stop();
        let hwnd = self.app.hwnd();
        if !hwnd.is_invalid() {
            // SAFETY: the panel's own window, destroyed once (`Drop` skips
            // it once it is gone).
            let _ = unsafe { DestroyWindow(hwnd) };
        }
    }

    /// Hides the window and gives the focus back to the window that was in
    /// front before it showed.
    fn dismiss(&self) {
        let hwnd = self.app.hwnd();
        // SAFETY: a plain query on a window that may have closed.
        if !hwnd.is_invalid() && unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            // SAFETY: a plain call on a live window.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
        if let Some(previous) = self.previous.borrow_mut().take()
            && !previous.is_invalid()
            // SAFETY: a plain query on a window that may have closed.
            && unsafe { IsWindow(Some(previous)) }.as_bool()
        {
            // SAFETY: a plain call on the window that was in front.
            let _ = unsafe { SetForegroundWindow(previous) };
        }
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.dismiss();
        self.app.stop();
        let hwnd = self.app.hwnd();
        // SAFETY: only a window that still exists is destroyed; a closed
        // panel's window is gone.
        if !hwnd.is_invalid() && unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
    }
}

/// Makes the panel's window out of the top-level window
/// [`AppView::new`] created: its style, its level and the tool-window bit
/// that keeps it out of the taskbar's window list.
fn apply(view: &AppView, options: &PanelOptions, size: (i32, i32)) {
    let hwnd = view.hwnd();
    // SAFETY: plain calls on the window just made, before it shows.
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        if options.borderless {
            // A panel has no caption, no resize frame and no system menu,
            // as a borderless window has none.
            style &= !(WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_SYSMENU.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0);
            style |= WS_POPUP.0;
            SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
        }
        let mut ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        // The panel is a tool window, not an app window.
        ex_style |= WS_EX_TOOLWINDOW.0;
        if options.nonactivating {
            // Clicks do not activate the app; the panel takes the keyboard
            // only after the user aims at it.
            ex_style |= WS_EX_NOACTIVATE.0;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style as isize);
        // The window procedure hides the panel when it loses the keyboard,
        // when the options ask for it (no activation message reaches a
        // panel's window otherwise).
        crate::window::set_hides_on_resign_key(hwnd, options.hides_on_resign_key);
        // A window's size is its outer frame, a panel's is its content: a
        // borderless popup has the whole window as its client area, so it
        // is sized to the points the panel asked for, once the frame is
        // gone. A decorated panel was sized by `AppView::new` already.
        let (width, height) = if options.borderless {
            let dpi = crate::window::dpi(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: (size.0 as f32 * scale).round() as i32,
                bottom: (size.1 as f32 * scale).round() as i32,
            };
            let _ = AdjustWindowRectExForDpi(
                &mut rect,
                WINDOW_STYLE(style),
                false,
                WINDOW_EX_STYLE(ex_style),
                dpi,
            );
            (rect.right - rect.left, rect.bottom - rect.top)
        } else {
            (0, 0)
        };
        // The frame changed: ask Win32 to remake it, without moving or
        // activating the window.
        let mut flags = SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE;
        if !options.borderless {
            flags |= SWP_NOSIZE;
        }
        let _ = SetWindowPos(
            hwnd,
            Some(order_of(options.level)),
            0,
            0,
            width,
            height,
            flags,
        );
    }
}

/// Moves the window: `position` is the screen coordinates, in points at the
/// window's scale, of its top-left corner (Win32's outer frame).
fn place(view: &AppView, x: Points, y: Points) {
    let hwnd = view.hwnd();
    if hwnd.is_invalid() {
        return;
    }
    let scale = view.surface().scale();
    let (x, y) = ((x.0 * scale).round() as i32, (y.0 * scale).round() as i32);
    // SAFETY: a plain call on a live window; its size and level stay.
    let _ = unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            0,
            0,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
        )
    };
}

/// The z-order a level is: the closest Win32 has to a panel's level.
fn order_of(level: PanelLevel) -> HWND {
    match level {
        PanelLevel::Normal => HWND_NOTOPMOST,
        PanelLevel::Floating | PanelLevel::AboveDock => HWND_TOPMOST,
    }
}

/// The window's size in points: the options', or 70% of the work area (the
/// screen without the taskbar), as the macOS backend's default.
fn panel_size(size: Option<Size<Points>>) -> Result<(i32, i32), PanelError> {
    if let Some(size) = size {
        let (width, height) = (size.width.0, size.height.0);
        if width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite() {
            return Ok((width.round() as i32, height.round() as i32));
        }
        return Err(PanelError::Invalid("a panel's size must be positive"));
    }
    let mut rect = RECT::default();
    // SAFETY: a plain query with a local out value.
    let area = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&raw mut rect as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if area.is_err() || rect.right <= rect.left || rect.bottom <= rect.top {
        return Ok((800, 600));
    }
    // SAFETY: a plain query.
    let dpi = unsafe { GetDpiForSystem() };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let width = (rect.right - rect.left) as f32 / scale;
    let height = (rect.bottom - rect.top) as f32 / scale;
    Ok(((width * 0.7).round() as i32, (height * 0.7).round() as i32))
}
