//! The Win32 edge: the imports, and the thin safe wrappers the window is built from.
//!
//! This is the only module in the workspace that contains `unsafe`, and this crate is the
//! only one whose lint level for it is not `forbid` — see this crate's `Cargo.toml` and
//! `docs/adr/0013-the-window-is-written-against-win32.md`. Two rules hold inside it:
//! every foreign call sits in an `unsafe` block with a `SAFETY` note, and nothing that
//! crosses the module boundary takes or returns a raw pointer that the caller has to
//! dereference. Outside this file the crate is ordinary safe Rust.
//!
//! The imports are declared by hand rather than pulled from `windows-sys`, and that is a
//! measurement rather than a preference: `windows-sys` builds Windows import libraries with
//! `dlltool`, which this host does not have, so the whole `windows-rs` ecosystem cannot
//! compile here. `rustup` *does* ship `libuser32.a`, `libkernel32.a` and `libgdi32.a` for
//! `x86_64-pc-windows-gnu`, so `#[link(name = "user32")]` resolves without a toolchain
//! install. See `docs/adr/0013` for the probe that established it.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;

/// A window handle.
pub(crate) type Hwnd = *mut c_void;
/// A device context.
pub(crate) type Hdc = *mut c_void;
/// A module instance handle.
pub(crate) type Hinstance = *mut c_void;
/// A GDI object: brush, pen or font.
pub(crate) type Hgdi = *mut c_void;
/// A bitmap handle.
pub(crate) type Hbitmap = *mut c_void;

/// `WS_OVERLAPPEDWINDOW`: a resizable window with a title bar and the usual frame.
const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
/// Let the system choose the position.
const CW_USEDEFAULT: i32 = -0x8000_0000;
/// Show the window at the size it was created.
const SW_SHOW: i32 = 5;
/// Minimise the window.
const SW_MINIMIZE: i32 = 6;
/// Maximise it.
const SW_MAXIMIZE: i32 = 3;
/// Restore it from either state.
const SW_RESTORE: i32 = 9;
/// `CS_HREDRAW | CS_VREDRAW`: repaint the whole client area when the size changes.
const CS_REDRAW: u32 = 0x0003;
/// `IDC_ARROW`, as a resource identifier rather than a pointer to a string.
const IDC_ARROW: usize = 32_512;

/// `WM_CLOSE`: the window is being asked to close.
const WM_CLOSE: u32 = 0x0010;
/// `WM_DESTROY`: the window is gone.
const WM_DESTROY: u32 = 0x0002;
/// `WM_SETFOCUS`: the window gained the keyboard.
const WM_SETFOCUS: u32 = 0x0007;
/// `WM_SIZE`: the client area changed size.
const WM_SIZE: u32 = 0x0005;
/// `WM_PAINT`.
const WM_PAINT: u32 = 0x000F;
/// `WM_ERASEBKGND`. Never painted into: every pixel is drawn in `WM_PAINT`.
const WM_ERASE_BACKGROUND: u32 = 0x0014;
/// `WM_QUIT`, which ends a message loop.
const WM_QUIT: u32 = 0x0012;
/// `WM_KEYDOWN`.
const WM_KEYDOWN: u32 = 0x0100;
/// `WM_KEYUP`.
const WM_KEYUP: u32 = 0x0101;
/// `WM_CHAR`: a character, after the layout has had its say.
const WM_CHAR: u32 = 0x0102;
/// `WM_SYSKEYDOWN`: a key pressed with Alt held, which is how Windows delivers Alt+arrow.
const WM_SYSKEYDOWN: u32 = 0x0104;
/// `WM_SYSKEYUP`.
const WM_SYSKEYUP: u32 = 0x0105;
/// `WM_MOUSEMOVE`: the pointer moved over the client area.
const WM_MOUSEMOVE: u32 = 0x0200;
/// `WM_LBUTTONDOWN` and `WM_LBUTTONUP`.
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
/// `WM_LBUTTONDBLCLK`: a second press inside the double-click interval.
const WM_LBUTTONDBLCLK: u32 = 0x0203;
/// `WM_RBUTTONDOWN` and `WM_RBUTTONUP`.
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
/// `WM_MBUTTONDOWN` and `WM_MBUTTONUP`.
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
/// `WM_MOUSEWHEEL`: the wheel turned, in multiples of 120, with *screen* coordinates.
const WM_MOUSEWHEEL: u32 = 0x020A;
/// `WM_MOUSELEAVE`: the pointer left the client area, asked for through `TrackMouseEvent`.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// `WM_CAPTURECHANGED`: the pointer was taken away from this window.
const WM_CAPTURECHANGED: u32 = 0x0215;
/// `WM_SETCURSOR`: the pointer is over the window, and the cursor is the window's to choose.
const WM_SETCURSOR: u32 = 0x0020;
/// `WM_TIMER`.
const WM_TIMER: u32 = 0x0113;
/// `WM_GETMINMAXINFO`: the window's own smallest size.
const WM_GETMINMAXINFO: u32 = 0x0024;
/// `WM_NCHITTEST`: what a point is over. The presenter is asked, so a frameless window still
/// has a title strip that moves it.
const WM_NCHITTEST: u32 = 0x0084;
/// `HTCLIENT` and `HTCAPTION`, the two hit-test answers the presenter gives.
pub(crate) const HTCLIENT: isize = 1;
/// What a press in the title strip answers, which is what makes a custom frame movable.
pub(crate) const HTCAPTION: isize = 2;

/// Mouse button state, from the high half of a mouse message's `wparam`.
const MK_LBUTTON: usize = 0x0001;
const MK_SHIFT: usize = 0x0004;
const MK_CONTROL: usize = 0x0008;

/// The smallest client the layout is built for. Windows enforces it through
/// `WM_GETMINMAXINFO`, so the panels can never be narrower than what they hold.
const MIN_SIZE: (i32, i32) = (1024, 640);

/// `TME_LEAVE`: ask for a `WM_MOUSELEAVE` when the pointer goes.
const TME_LEAVE: u32 = 0x0002;

/// The system cursors, by resource identifier.
const IDC_IBEAM: usize = 32_513;
const IDC_CROSS: usize = 32_515;
const IDC_SIZEWE: usize = 32_644;
const IDC_SIZENS: usize = 32_645;
const IDC_SIZEALL: usize = 32_646;
const IDC_HAND: usize = 32_649;

/// `CS_DBLCLKS`: deliver `WM_LBUTTONDBLCLK`.
const CS_DBLCLKS: u32 = 0x0008;
/// `WS_POPUP`: the style a frameless window is made from.
const WS_POPUP: u32 = 0x8000_0000;
/// `WS_THICKFRAME`: the resizing border, kept even without a caption.
const WS_THICKFRAME: u32 = 0x0004_0000;
/// `WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU`.
const WS_FRAME_BUTTONS: u32 = 0x0002_0000 | 0x0001_0000 | 0x0008_0000;
/// `WS_EX_APPWINDOW`: a popup still gets a taskbar button.
const WS_EX_APPWINDOW: u32 = 0x0004_0000;
/// `GCLP_HICON` and `GCLP_HICONSM`, the class's two icons.
const GCLP_HICON: i32 = -14;
const GCLP_HICONSM: i32 = -34;

/// `HALFTONE`: the stretch mode the viewer scales frames with.
const HALFTONE: i32 = 4;
/// `NULL_PEN`, for a shape with no outline.
const NULL_PEN: i32 = 8;
/// `LOGPIXELSY`: vertical dots per inch, for the interface's scale.
const LOGPIXELSY: i32 = 90;
/// `GRADIENT_FILL_RECT_V`: a gradient from the first vertex down to the second.
const GRADIENT_FILL_RECT_V: u32 = 1;
/// `AC_SRC_OVER`, the only blend operation there is.
const AC_SRC_OVER: u8 = 0;
/// `OFN_*`: an explorer-style dialog that must find the file and must not change the process's
/// working directory.
const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;

/// `VK_SHIFT`.
pub(crate) const VK_SHIFT: i32 = 0x10;
/// `VK_CONTROL`.
pub(crate) const VK_CONTROL: i32 = 0x11;
/// `VK_MENU`, which is Alt.
pub(crate) const VK_MENU: i32 = 0x12;

/// `DT_LEFT`: the default horizontal alignment.
const DT_LEFT: u32 = 0x0000;
/// `DT_CENTER`.
const DT_CENTER: u32 = 0x0001;
/// `DT_RIGHT`.
const DT_RIGHT: u32 = 0x0002;
/// `DT_VCENTER`: centring vertically requires a single line.
const DT_VCENTER: u32 = 0x0004;
/// `DT_SINGLELINE`.
const DT_SINGLELINE: u32 = 0x0020;
/// `DT_NOPREFIX`: draw `&` as itself rather than as an accelerator marker.
const DT_NOPREFIX: u32 = 0x0800;
/// `DT_END_ELLIPSIS`: clip with an ellipsis rather than mid-glyph.
const DT_END_ELLIPSIS: u32 = 0x8000;
/// `DT_WORDBREAK`: wrap at spaces.
const DT_WORDBREAK: u32 = 0x0010;
/// `DT_TOP`.
const DT_TOP: u32 = 0x0000;

/// `BI_RGB`: an uncompressed bitmap.
const BI_RGB: u32 = 0;
/// `DIB_RGB_COLORS`.
const DIB_RGB_COLORS: u32 = 0;
/// `SRCCOPY`.
const SRCCOPY: u32 = 0x00CC_0020;
/// `TRANSPARENT`: text background mode.
const TRANSPARENT: i32 = 1;
/// `PM_REMOVE`.
const PM_REMOVE: u32 = 1;
/// `CLEARTYPE_QUALITY`.
const CLEARTYPE_QUALITY: u32 = 5;
/// `DEFAULT_CHARSET`.
const DEFAULT_CHARSET: u32 = 1;
/// `FW_NORMAL` and `FW_SEMIBOLD`.
const FW_NORMAL: i32 = 400;
const FW_SEMIBOLD: i32 = 600;

/// A rectangle, in client coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct Rect {
    /// Left edge, inclusive.
    pub(crate) left: i32,
    /// Top edge, inclusive.
    pub(crate) top: i32,
    /// Right edge, exclusive.
    pub(crate) right: i32,
    /// Bottom edge, exclusive.
    pub(crate) bottom: i32,
}

impl Rect {
    /// A rectangle from its corners.
    pub(crate) const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Width in pixels, never negative.
    pub(crate) const fn width(self) -> i32 {
        if self.right > self.left {
            self.right - self.left
        } else {
            0
        }
    }

    /// Height in pixels, never negative.
    pub(crate) const fn height(self) -> i32 {
        if self.bottom > self.top {
            self.bottom - self.top
        } else {
            0
        }
    }

    /// Whether the rectangle has any area at all.
    pub(crate) const fn is_empty(self) -> bool {
        self.width() == 0 || self.height() == 0
    }

    /// A rectangle inset on every side, or an empty rectangle if that would invert it.
    pub(crate) const fn inset(self, amount: i32) -> Self {
        Self::new(
            self.left + amount,
            self.top + amount,
            self.right - amount,
            self.bottom - amount,
        )
    }
}

/// A colour in `0x00BBGGRR`, which is what GDI takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Colour(u32);

impl Colour {
    /// A colour from its parts.
    pub(crate) const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
    }

    /// A colour from a `#RRGGBB` literal, so the palette is written once.
    pub(crate) const fn hex(value: u32) -> Self {
        Self::rgb(
            ((value >> 16) & 0xFF) as u8,
            ((value >> 8) & 0xFF) as u8,
            (value & 0xFF) as u8,
        )
    }

    /// The red, green and blue of it, for a caller that has to mix rather than fill.
    pub(crate) const fn parts(self) -> (u8, u8, u8) {
        (
            (self.0 & 0xFF) as u8,
            ((self.0 >> 8) & 0xFF) as u8,
            ((self.0 >> 16) & 0xFF) as u8,
        )
    }
}

/// A solid brush, released when it goes out of scope.
#[derive(Debug)]
pub(crate) struct Brush(Hgdi);

impl Brush {
    /// A brush of one colour.
    pub(crate) fn solid(colour: Colour) -> Self {
        // SAFETY: CreateSolidBrush takes a colour by value and may return null only when GDI
        // is out of resources; a null brush is passed to SelectObject, which then leaves the
        // previous object selected rather than misbehaving.
        Self(unsafe { CreateSolidBrush(colour.0) })
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from CreateSolidBrush and is released exactly once,
            // here, because Brush is the only owner.
            unsafe { DeleteObject(self.0) };
        }
    }
}

/// A pen, released when it goes out of scope.
#[derive(Debug)]
pub(crate) struct Pen(Hgdi);

impl Pen {
    /// A pen of one colour and width.
    pub(crate) fn solid(colour: Colour, width: i32) -> Self {
        // SAFETY: CreatePen takes three integers by value and may return null only when GDI
        // is out of resources.
        Self(unsafe { CreatePen(0, width.max(1), colour.0) })
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from CreatePen and is released exactly once, here,
            // because Pen is the only owner.
            unsafe { DeleteObject(self.0) };
        }
    }
}

/// A font, released when it goes out of scope.
#[derive(Debug)]
pub(crate) struct Font(Hgdi);

impl Font {
    /// A face from the system's own font directory.
    pub(crate) fn new(face: &str, height: i32, semibold: bool) -> Self {
        let name = wide(face);
        let weight = if semibold { FW_SEMIBOLD } else { FW_NORMAL };
        // SAFETY: every argument is a value or a pointer to a NUL-terminated wide string that
        // outlives the call, and the returned handle is owned by the Font that wraps it.
        Self(unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                0,
                0,
                CLEARTYPE_QUALITY,
                0,
                name.as_ptr(),
            )
        })
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the handle came from CreateFontW and is released exactly once, here,
            // because Font is the only owner.
            unsafe { DeleteObject(self.0) };
        }
    }
}

/// Where a line of text sits horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    /// Flush left.
    Left,
    /// Centred.
    Centre,
    /// Flush right.
    Right,
}

/// One key press or release, with the modifiers that were down when it arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    /// The virtual key code.
    pub(crate) code: u16,
    /// Control was down.
    pub(crate) ctrl: bool,
    /// Shift was down.
    pub(crate) shift: bool,
    /// Alt was down.
    pub(crate) alt: bool,
    /// This is a press rather than a release.
    pub(crate) pressed: bool,
}

/// A point in the client area, with the modifiers that were down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Pointer {
    /// Horizontal position, from the client's left edge.
    pub(crate) x: i32,
    /// Vertical position, from the client's top edge.
    pub(crate) y: i32,
    /// Control was down.
    pub(crate) ctrl: bool,
    /// Shift was down.
    pub(crate) shift: bool,
}

/// Which button an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Button {
    /// The primary button.
    Left,
    /// The secondary button, which opens menus.
    Right,
    /// The middle button, which the viewer uses to pan.
    Middle,
}

/// What the window sends the presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event {
    /// The window needs painting, into this device context.
    Paint(Hdc),
    /// The client area changed size.
    Resize,
    /// A key arrived.
    Key(Key),
    /// A character arrived, after the keyboard layout had its say.
    Text(char),
    /// The window gained the keyboard.
    Focus,
    /// The window is being asked to close.
    Close,
    /// The pointer moved over the client area.
    Move(Pointer),
    /// A button was pressed or released.
    Press {
        /// Where it happened, and with which modifiers.
        at: Pointer,
        /// Which button.
        button: Button,
        /// `true` for a press, `false` for a release.
        down: bool,
        /// A second press inside the double-click interval.
        double: bool,
    },
    /// The wheel turned over a point: 120 units per notch, positive is up.
    Wheel {
        /// Where the pointer was.
        at: Pointer,
        /// The rotation, in the units Windows uses.
        delta: i32,
    },
    /// The pointer left the client area.
    Leave,
    /// The frame's own timer fired.
    Timer,
    /// The pointer was taken away: a drag that was in progress has ended badly.
    CaptureLost,
    /// Where a point falls in the frame. The answer is a hit-test code — `HTCLIENT` for
    /// ordinary content, `HTCAPTION` for the strip that moves the window, and `Default` to let
    /// Windows decide, which is what keeps the resizing borders and the resize cursors working.
    HitTest(Pointer),
    /// The pointer is over the client area and the cursor is the window's to choose.
    ChooseCursor(Pointer),
}

/// What the presenter wants done about an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reaction {
    /// Let Windows handle it.
    Default,
    /// Handled, with this result. For a hit test this is the code: `HTCLIENT`, `HTCAPTION`, or
    /// `1` for a cursor that was set.
    Handled(isize),
    /// The window should close: it is destroyed, and the loop ends.
    Quit,
}

/// The presenter's event handler: the application, as an opaque pointer, and the event.
pub(crate) type Handler = fn(*mut c_void, Event) -> Reaction;

thread_local! {
    /// The handler, its context and the window of the one window this thread owns. A thread
    /// owns one window because the window procedure is a free function and this is how it
    /// finds the state that belongs to it; the presenter is single-windowed by design.
    static CURRENT: RefCell<Option<Live>> = const { RefCell::new(None) };
    /// Modifiers forced on for a synthesised key, used by the self-check described in
    /// `lib.rs`. Reading them costs nothing on the real path, where they are false.
    /// `None` means the real keyboard is asked; `Some` means the synthetic state stands in its
    /// place, which is what makes a scripted check reproducible on a machine whose keys are
    /// being touched.
    static FORCED: Cell<Option<(bool, bool, bool)>> = const { Cell::new(None) };
}

/// The one window this thread owns, and how to reach the state that belongs to it.
#[derive(Debug, Clone, Copy)]
struct Live {
    hwnd: Hwnd,
    handler: Handler,
    context: *mut c_void,
}

/// Runs a closure with the application behind the pointer the presenter handed over.
///
/// This is the only place the pointer is turned back into a reference, and it is safe to call
/// because the pointer comes from a `&mut` that the caller keeps alive across the whole
/// message loop: [`Window`] does not outlive it.
pub(crate) fn with_app<A, R>(context: *mut c_void, body: impl FnOnce(&mut A) -> R) -> R {
    // SAFETY: the context is the address of the caller's own `A`, which is alive for as long
    // as the window that carries this pointer is, and one window belongs to one thread, so no
    // second reference to the same `A` can exist here.
    let app = unsafe { &mut *context.cast::<A>() };
    body(app)
}

/// A rectangle in screen coordinates, as `AdjustWindowRect` expects.
#[repr(C)]
struct WindowRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

/// A point.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

/// A message.
///
/// The layout is the contract with the message queue; the fields the presenter does not read
/// are still filled in by Windows, which is why the whole struct is exempt from the dead-code
/// lint rather than the fields being deleted.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) struct Message {
    /// The window the message is for.
    pub(crate) hwnd: Hwnd,
    /// The message identifier.
    pub(crate) message: u32,
    /// First parameter.
    pub(crate) wparam: usize,
    /// Second parameter.
    pub(crate) lparam: isize,
    /// The time it was posted.
    pub(crate) time: u32,
    /// Where the pointer was.
    point: Point,
}

/// What `BeginPaint` fills in.
///
/// Exempt from the dead-code lint for the same reason as [`Message`]: the layout is the ABI.
#[repr(C)]
#[allow(dead_code)]
struct PaintStruct {
    hdc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

/// A bitmap header, for the blits and the read-back.
///
/// Exempt from the dead-code lint for the same reason as [`Message`]: the layout is the ABI.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pixels_per_meter: i32,
    y_pixels_per_meter: i32,
    colours_used: u32,
    colours_important: u32,
}

/// A header plus room for the colour table that a 32-bit DIB does not use.
#[repr(C)]
#[derive(Clone, Copy)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colours: [u32; 1],
}

/// One end of a `GradientFill`.
///
/// The dead-code lint is exempted for the same reason as [`Message`]: the layout is the ABI,
/// and the fields the module does not read are still written by the call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct TriVertex {
    x: i32,
    y: i32,
    red: u16,
    green: u16,
    blue: u16,
    alpha: u16,
}

/// Which two vertices a gradient runs between.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct GradientRect {
    upper_left: u32,
    lower_right: u32,
}

/// How `AlphaBlend` combines the two surfaces.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct BlendFunction {
    blend_op: u8,
    blend_flags: u8,
    source_alpha: u8,
    alpha_format: u8,
}

/// The two bitmaps an icon is made of.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct IconInfo {
    is_icon: i32,
    x_hotspot: u32,
    y_hotspot: u32,
    mask: Hbitmap,
    colour: Hbitmap,
}

/// What `TrackMouseEvent` is asked for.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
struct TrackMouse {
    size: u32,
    flags: u32,
    hwnd: Hwnd,
    hover_ms: u32,
}

/// What `WM_GETMINMAXINFO` carries, of which this module sets the smallest size.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
#[allow(dead_code)]
struct MinMaxInfo {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track: Point,
    max_track: Point,
}

/// The longest path a file dialog may return.
const DIALOG_PATH: usize = 1024;

/// The file dialog, declared in full because the size in its first field *is* the version.
#[repr(C)]
#[allow(dead_code)]
struct OpenFileNameW {
    size: u32,
    owner: Hwnd,
    instance: Hinstance,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_extension: *const u16,
    custom_data: isize,
    hook: *mut c_void,
    template: *const u16,
    reserved: *mut c_void,
    reserved_value: u32,
    flags_ex: u32,
}

impl BitmapInfo {
    /// A top-down 32-bit `BI_RGB` description of a buffer of the given size.
    fn top_down(width: i32, height: i32) -> Self {
        Self {
            header: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width,
                // A negative height asks for rows top to bottom, which is the order both the
                // renderer and this module use.
                height: -height,
                planes: 1,
                bit_count: 32,
                compression: BI_RGB,
                size_image: 0,
                x_pixels_per_meter: 0,
                y_pixels_per_meter: 0,
                colours_used: 0,
                colours_important: 0,
            },
            colours: [0],
        }
    }
}

/// The window class description.
#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Hinstance,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu: *const u16,
    class_name: *const u16,
    icon_small: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    #[allow(clippy::too_many_arguments)]
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, title: *const u16) -> i32;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn AdjustWindowRect(rect: *mut WindowRect, style: u32, menu: i32) -> i32;
    fn GetMessageW(message: *mut Message, hwnd: Hwnd, first: u32, last: u32) -> i32;
    fn PeekMessageW(message: *mut Message, hwnd: Hwnd, first: u32, last: u32, remove: u32) -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn PostMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
    fn PostQuitMessage(code: i32);
    fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn LoadCursorW(instance: Hinstance, name: *const u16) -> *mut c_void;
    fn FillRect(hdc: Hdc, rect: *const Rect, brush: Hgdi) -> i32;
    fn FrameRect(hdc: Hdc, rect: *const Rect, brush: Hgdi) -> i32;
    fn DrawTextW(hdc: Hdc, text: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
    fn GetDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, hdc: Hdc) -> i32;
    fn UpdateWindow(hwnd: Hwnd) -> i32;
    fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
    fn IsZoomed(hwnd: Hwnd) -> i32;
    fn SetCapture(hwnd: Hwnd) -> Hwnd;
    fn ReleaseCapture() -> i32;
    fn SetCursor(cursor: *mut c_void) -> *mut c_void;
    fn SetTimer(hwnd: Hwnd, id: usize, milliseconds: u32, callback: *mut c_void) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn TrackMouseEvent(event: *mut TrackMouse) -> i32;
    fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
    fn GetCursorPos(point: *mut Point) -> i32;
    fn SetClassLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn CreateIconIndirect(info: *const IconInfo) -> *mut c_void;
    fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn SetProcessDPIAware() -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(colour: u32) -> Hgdi;
    fn CreatePen(style: i32, width: i32, colour: u32) -> Hgdi;
    #[allow(clippy::too_many_arguments)]
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike_out: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> Hgdi;
    fn SelectObject(hdc: Hdc, object: Hgdi) -> Hgdi;
    fn DeleteObject(object: Hgdi) -> i32;
    fn SetTextColor(hdc: Hdc, colour: u32) -> u32;
    fn SetBkMode(hdc: Hdc, mode: i32) -> i32;
    fn MoveToEx(hdc: Hdc, x: i32, y: i32, previous: *mut Point) -> i32;
    fn LineTo(hdc: Hdc, x: i32, y: i32) -> i32;
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn CreateCompatibleBitmap(hdc: Hdc, width: i32, height: i32) -> Hbitmap;
    fn CreateBitmap(
        width: i32,
        height: i32,
        planes: u32,
        bits_per_pixel: u32,
        bits: *const u8,
    ) -> Hbitmap;
    fn CreateDIBSection(
        hdc: Hdc,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> Hbitmap;
    fn DeleteDC(hdc: Hdc) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn StretchDIBits(
        hdc: Hdc,
        dst_x: i32,
        dst_y: i32,
        dst_width: i32,
        dst_height: i32,
        src_x: i32,
        src_y: i32,
        src_width: i32,
        src_height: i32,
        bits: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        rop: u32,
    ) -> i32;
    fn GetDIBits(
        hdc: Hdc,
        bitmap: Hbitmap,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapInfo,
        usage: u32,
    ) -> i32;
    fn BitBlt(
        destination: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Hdc,
        source_x: i32,
        source_y: i32,
        rop: u32,
    ) -> i32;
    fn Polygon(hdc: Hdc, points: *const Point, count: i32) -> i32;
    fn Polyline(hdc: Hdc, points: *const Point, count: i32) -> i32;
    fn Ellipse(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    fn RoundRect(
        hdc: Hdc,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> i32;
    fn SetStretchBltMode(hdc: Hdc, mode: i32) -> i32;
    fn SetBrushOrgEx(hdc: Hdc, x: i32, y: i32, previous: *mut Point) -> i32;
    fn GetDeviceCaps(hdc: Hdc, index: i32) -> i32;
    fn IntersectClipRect(hdc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    fn SaveDC(hdc: Hdc) -> i32;
    fn RestoreDC(hdc: Hdc, saved: i32) -> i32;
    fn GetStockObject(index: i32) -> Hgdi;
    fn GetTextExtentPoint32W(hdc: Hdc, value: *const u16, count: i32, size: *mut Size) -> i32;
}

/// A width and a height, as `GetTextExtentPoint32W` reports them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct Size {
    width: i32,
    height: i32,
}

#[link(name = "msimg32")]
unsafe extern "system" {
    #[allow(clippy::too_many_arguments)]
    fn AlphaBlend(
        destination: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Hdc,
        source_x: i32,
        source_y: i32,
        source_width: i32,
        source_height: i32,
        blend: BlendFunction,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn GradientFill(
        hdc: Hdc,
        vertices: *const TriVertex,
        count: u32,
        rects: *const GradientRect,
        rect_count: u32,
        mode: u32,
    ) -> i32;
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(dialog: *mut OpenFileNameW) -> i32;
    fn GetSaveFileNameW(dialog: *mut OpenFileNameW) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryPerformanceCounter(value: *mut i64) -> i32;
    fn QueryPerformanceFrequency(value: *mut i64) -> i32;
    fn GetLastError() -> u32;
}

/// The client area of the window this thread owns, in pixels.
///
/// The presenter asks for this while painting, which is the one moment it needs to know how
/// much room it has.
pub(crate) fn client_rect() -> Rect {
    let mut rect = Rect::new(0, 0, 0, 0);
    CURRENT.with(|current| {
        if let Some(live) = *current.borrow() {
            // SAFETY: the window is live for as long as the presenter's loop runs, and `rect`
            // is a live local for GetClientRect to fill in.
            unsafe { GetClientRect(live.hwnd, &mut rect) };
        }
    });
    rect
}

/// A string as Windows wants it: UTF-16, NUL-terminated.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Draws the process's module handle.
fn instance() -> Hinstance {
    // SAFETY: a null name asks for the executable's own module, which always exists.
    unsafe { GetModuleHandleW(std::ptr::null()) }
}

/// The window procedure: decodes what the presenter needs and forwards the rest.
unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let Some(live) = CURRENT.with(|current| *current.borrow()) else {
        // SAFETY: no handler means no state to touch, so Windows' own behaviour is correct.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    let handler = |event: Event| (live.handler)(live.context, event);
    let reaction = match message {
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN
        | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
            let at = pointer_of(wparam, lparam);
            if message == WM_MOUSEMOVE {
                // Asking on every move is what Windows documents: the request is one-shot, and
                // it is what turns the pointer's departure into a `WM_MOUSELEAVE` the
                // presenter can clear its hover state with.
                let mut track = TrackMouse {
                    size: std::mem::size_of::<TrackMouse>() as u32,
                    flags: TME_LEAVE,
                    hwnd,
                    hover_ms: 0,
                };
                // SAFETY: the structure is a live local laid out as the call expects.
                unsafe { TrackMouseEvent(&mut track) };
                handler(Event::Move(at))
            } else {
                handler(Event::Press {
                    at,
                    button: if message == WM_RBUTTONDOWN || message == WM_RBUTTONUP {
                        Button::Right
                    } else if message == WM_MBUTTONDOWN || message == WM_MBUTTONUP {
                        Button::Middle
                    } else {
                        Button::Left
                    },
                    down: message == WM_LBUTTONDOWN
                        || message == WM_RBUTTONDOWN
                        || message == WM_MBUTTONDOWN,
                    double: message == WM_LBUTTONDBLCLK,
                })
            }
        }
        WM_MOUSEWHEEL => {
            // The wheel is the one mouse message that carries screen coordinates.
            let mut where_it_was = screen_point(lparam);
            // SAFETY: the window is live and the point is a live local.
            unsafe { ScreenToClient(live.hwnd, &mut where_it_was) };
            handler(Event::Wheel {
                at: Pointer {
                    x: where_it_was.x,
                    y: where_it_was.y,
                    ctrl: wparam & MK_CONTROL != 0,
                    shift: wparam & MK_SHIFT != 0,
                },
                delta: i32::from(((wparam >> 16) as u32 & 0xFFFF) as u16 as i16),
            })
        }
        WM_MOUSELEAVE => handler(Event::Leave),
        WM_TIMER => handler(Event::Timer),
        WM_CAPTURECHANGED => handler(Event::CaptureLost),
        WM_NCHITTEST => {
            let mut point = screen_point(lparam);
            // SAFETY: the window is live and the point is a live local.
            unsafe { ScreenToClient(live.hwnd, &mut point) };
            handler(Event::HitTest(Pointer {
                x: point.x,
                y: point.y,
                ctrl: false,
                shift: false,
            }))
        }
        WM_SETCURSOR if (lparam as usize & 0xFFFF) == HTCLIENT as usize => {
            let mut point = Point::default();
            // SAFETY: both calls take live locals, and the window is live.
            unsafe {
                GetCursorPos(&mut point);
                ScreenToClient(live.hwnd, &mut point);
            }
            handler(Event::ChooseCursor(Pointer {
                x: point.x,
                y: point.y,
                ctrl: false,
                shift: false,
            }))
        }
        WM_GETMINMAXINFO => {
            let info = lparam as *mut MinMaxInfo;
            if !info.is_null() {
                // SAFETY: the message carries a pointer to a struct Windows owns for the
                // duration of the call, and only the smallest tracked size is written.
                unsafe {
                    (*info).min_track = Point {
                        x: MIN_SIZE.0,
                        y: MIN_SIZE.1,
                    };
                }
            }
            Reaction::Handled(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let key = Key {
                code: (wparam & 0xFFFF) as u16,
                ctrl: modifier(VK_CONTROL),
                shift: modifier(VK_SHIFT),
                alt: modifier(VK_MENU),
                pressed: message == WM_KEYDOWN || message == WM_SYSKEYDOWN,
            };
            handler(Event::Key(key))
        }
        WM_CHAR => {
            // A synthesised key pressed while `ctrl` or `alt` is *forced* still comes back as a
            // character: `TranslateMessage` consults the machine's real keyboard rather than the
            // state a check set, and the machine's keyboard has nothing held. A real `ctrl+k`
            // produces no character at all, so the letter is dropped rather than typed into
            // whichever field happens to hold the keyboard.
            let shortcut = modifier(VK_CONTROL) || modifier(VK_MENU);
            match char::from_u32((wparam & 0xFFFF) as u32) {
                Some(character) if !shortcut => handler(Event::Text(character)),
                _ => Reaction::Handled(0),
            }
        }
        WM_SETFOCUS => handler(Event::Focus),
        WM_SIZE => handler(Event::Resize),
        WM_PAINT => {
            // SAFETY: an all-zero PaintStruct is the documented initial state, and BeginPaint
            // fills in every field it uses.
            let mut paint: PaintStruct = unsafe { std::mem::zeroed() };
            // SAFETY: BeginPaint is paired with EndPaint below, and `paint` is a live local
            // that BeginPaint fills in.
            let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
            handler(Event::Paint(hdc));
            // SAFETY: the same `paint` that BeginPaint filled in, and the same window.
            unsafe { EndPaint(hwnd, &paint) };
            Reaction::Handled(0)
        }
        WM_ERASE_BACKGROUND => Reaction::Handled(1),
        // A close request is the presenter's to answer: it is the last chance to write the
        // preferences and to say goodbye to the session, so it is asked rather than assumed.
        WM_CLOSE => handler(Event::Close),
        WM_DESTROY => {
            // SAFETY: the window is gone; this asks the loop to end after it drains.
            unsafe { PostQuitMessage(0) };
            Reaction::Handled(0)
        }
        _ => Reaction::Default,
    };
    match reaction {
        Reaction::Default => {
            // SAFETY: the message was not one this module handles, so Windows' own behaviour
            // for it is what is wanted.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        Reaction::Handled(value) => value,
        Reaction::Quit => {
            // SAFETY: destroying the window is what a close request means; WM_DESTROY will
            // follow and end the loop.
            unsafe { DestroyWindow(hwnd) };
            0
        }
    }
}

/// The client point and modifiers of a mouse message.
fn pointer_of(wparam: usize, lparam: isize) -> Pointer {
    let (x, y) = {
        let point = screen_point(lparam);
        (point.x, point.y)
    };
    Pointer {
        x,
        y,
        ctrl: wparam & MK_CONTROL != 0,
        shift: wparam & MK_SHIFT != 0,
    }
}

/// A point packed into an `lparam`.
fn screen_point(lparam: isize) -> Point {
    Point {
        x: i32::from((lparam as u32 & 0xFFFF) as u16 as i16),
        y: i32::from((((lparam as usize) >> 16) as u32 & 0xFFFF) as u16 as i16),
    }
}

/// The two coordinates of a point, packed the way a mouse message wants them.
fn pack(x: i32, y: i32) -> isize {
    (((y as u32 & 0xFFFF) << 16) | (x as u32 & 0xFFFF)) as isize
}

/// Whether a modifier is down, or forced on by the self-check.
fn modifier(key: i32) -> bool {
    if let Some(forced) = FORCED.with(Cell::get) {
        return match key {
            VK_SHIFT => forced.1,
            VK_CONTROL => forced.0,
            VK_MENU => forced.2,
            _ => false,
        };
    }
    key_down(key)
}

/// Whether a virtual key is down right now.
fn key_down(key: i32) -> bool {
    // SAFETY: GetKeyState takes a virtual key code and reports the state; it touches nothing
    // that belongs to this process.
    (unsafe { GetKeyState(key) } as u16 & 0x8000) != 0
}

/// Forces the modifier state for synthesised keys, so the self-check can exercise them
/// without injecting input into the machine's real keyboard queue.
///
/// The forced state **replaces** the real one rather than being added to it: a check that ran
/// while a person's finger rested on `shift` would otherwise exercise strokes it never meant
/// to, and the failure would look like a bug in the window.
pub(crate) fn force_modifiers(ctrl: bool, shift: bool, alt: bool) {
    FORCED.with(|forced| forced.set(Some((ctrl, shift, alt))));
}

/// A window, and the loop that drives it.
#[derive(Debug)]
pub(crate) struct Window {
    hwnd: Hwnd,
}

impl Window {
    /// Creates a window with a title and a client area of the given size.
    ///
    /// The window is not shown: the caller shows it once it has painted something. With
    /// `frameless` the caption is gone and the interface draws its own: the resizing border
    /// stays (`WS_THICKFRAME`), because no drawing of ours should have to reimplement resizing.
    pub(crate) fn create(
        handler: Handler,
        context: *mut c_void,
        title: &str,
        width: i32,
        height: i32,
        frameless: bool,
    ) -> Result<Self, String> {
        let class_name = wide("MajestatWindow");
        let title = wide(title);
        let style = if frameless {
            WS_POPUP | WS_THICKFRAME | WS_FRAME_BUTTONS
        } else {
            WS_OVERLAPPEDWINDOW
        };
        let mut rect = WindowRect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        // SAFETY: through here the class is registered once per process, the window is created
        // from it, and `rect` is a live local for AdjustWindowRect to adjust in place.
        unsafe {
            let cursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16);
            let class = WndClassExW {
                size: std::mem::size_of::<WndClassExW>() as u32,
                // Double clicks are wanted: a double click on a media tile places it.
                style: CS_REDRAW | CS_DBLCLKS,
                proc: Some(window_proc),
                class_extra: 0,
                window_extra: 0,
                instance: instance(),
                icon: std::ptr::null_mut(),
                cursor,
                // No background brush: every pixel is drawn in WM_PAINT, and letting Windows
                // erase first is what makes a redraw flicker.
                background: std::ptr::null_mut(),
                menu: std::ptr::null(),
                class_name: class_name.as_ptr(),
                icon_small: std::ptr::null_mut(),
            };
            if RegisterClassExW(&class) == 0 {
                // `ERROR_CLASS_ALREADY_EXISTS`: a second window in one process is fine, and
                // the class it finds is the one this module registered.
                if GetLastError() != 1410 {
                    return Err("the window class could not be registered".to_owned());
                }
            }
            AdjustWindowRect(&mut rect, style, 0);
            let hwnd = CreateWindowExW(
                if frameless { WS_EX_APPWINDOW } else { 0 },
                class_name.as_ptr(),
                title.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rect.right - rect.left,
                rect.bottom - rect.top,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance(),
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                return Err("the window could not be created".to_owned());
            }
            CURRENT.with(|current| {
                *current.borrow_mut() = Some(Live {
                    hwnd,
                    handler,
                    context,
                });
            });
            Ok(Self { hwnd })
        }
    }

    /// Makes the window visible.
    pub(crate) fn show(&self) {
        // SAFETY: the handle is live for as long as this Window exists.
        unsafe { ShowWindow(self.hwnd, SW_SHOW) };
    }

    /// Minimises the window, as the frame's own button does.
    pub(crate) fn minimize(&self) {
        // SAFETY: the handle is live for as long as this Window exists.
        unsafe { ShowWindow(self.hwnd, SW_MINIMIZE) };
    }

    /// Maximises the window, or restores it when it is already maximised.
    pub(crate) fn toggle_maximise(&self) {
        // SAFETY: the handle is live, and IsZoomed only reads its placement.
        let command = unsafe {
            if IsZoomed(self.hwnd) != 0 {
                SW_RESTORE
            } else {
                SW_MAXIMIZE
            }
        };
        // SAFETY: the handle is live for as long as this Window exists.
        unsafe { ShowWindow(self.hwnd, command) };
    }

    /// Asks the window to close, the same way the system menu does: the close message is
    /// posted, so the ordinary shutdown path runs rather than the window being torn down here.
    pub(crate) fn request_close(&self) {
        // SAFETY: the handle is live and the message is one it handles.
        unsafe { PostMessageW(self.hwnd, WM_CLOSE, 0, 0) };
    }

    /// Repaints one rectangle now, rather than waiting for the next message.
    pub(crate) fn invalidate_rect(&self, rect: Rect) {
        // SAFETY: the handle is live and `rect` is a live local for the duration of the call.
        unsafe { InvalidateRect(self.hwnd, &rect, 0) };
    }

    /// Forces a synchronous paint of whatever is damaged, which is what the snapshot needs.
    pub(crate) fn paint_now(&self) {
        // SAFETY: the handle is live for as long as this Window exists.
        unsafe {
            InvalidateRect(self.hwnd, std::ptr::null(), 0);
            UpdateWindow(self.hwnd);
        }
    }

    /// Puts the class's two icons on, which is what the taskbar and the title bar show.
    pub(crate) fn set_icon(&self, large: *mut c_void, small: *mut c_void) {
        // SAFETY: the window is live, and the icons are owned by the caller for the life of
        // the process; SetClassLongPtrW returns the previous value, which is forgotten.
        unsafe {
            SetClassLongPtrW(self.hwnd, GCLP_HICON, large as isize);
            SetClassLongPtrW(self.hwnd, GCLP_HICONSM, small as isize);
        }
    }

    /// Whether the window is maximised.
    pub(crate) fn is_maximised(&self) -> bool {
        // SAFETY: the window is live.
        unsafe { IsZoomed(self.hwnd) != 0 }
    }

    /// Starts the window's own timer, which is how a playing transport gets its ticks.
    pub(crate) fn start_timer(&self, milliseconds: u32) {
        // SAFETY: the window is live, and a null callback means the timer posts `WM_TIMER`
        // rather than calling into a function pointer.
        unsafe {
            SetTimer(
                self.hwnd,
                TIMER_ID,
                milliseconds.max(1),
                std::ptr::null_mut(),
            );
        }
    }

    /// Stops it. A timer that keeps ticking while nothing moves is a repaint nobody asked for.
    pub(crate) fn stop_timer(&self) {
        // SAFETY: the window is live and the identifier is the one the timer was made with.
        unsafe {
            KillTimer(self.hwnd, TIMER_ID);
        }
    }

    /// Takes the pointer, so a drag keeps reporting after it leaves the window.
    pub(crate) fn capture(&self) {
        // SAFETY: the window is live.
        unsafe {
            SetCapture(self.hwnd);
        }
    }

    /// Gives it back.
    pub(crate) fn release_capture(&self) {
        // SAFETY: releasing a capture this window holds is a no-op if it holds none.
        unsafe {
            ReleaseCapture();
        }
    }

    /// Posts a pointer move, as the mouse would.
    pub(crate) fn post_move(&self, x: i32, y: i32, held: bool) {
        let state = if held { MK_LBUTTON } else { 0 };
        // SAFETY: the window is live and the message is one it handles.
        unsafe {
            PostMessageW(self.hwnd, WM_MOUSEMOVE, state, pack(x, y));
        }
    }

    /// Posts a button press or release at a point.
    pub(crate) fn post_press(&self, x: i32, y: i32, button: Button, down: bool) {
        let message = match (button, down) {
            (Button::Left, true) => WM_LBUTTONDOWN,
            (Button::Left, false) => WM_LBUTTONUP,
            (Button::Right, true) => WM_RBUTTONDOWN,
            (Button::Right, false) => WM_RBUTTONUP,
            (Button::Middle, true) => WM_MBUTTONDOWN,
            (Button::Middle, false) => WM_MBUTTONUP,
        };
        let state = if down && button == Button::Left {
            MK_LBUTTON
        } else {
            0
        };
        // SAFETY: the window is live and the message is one it handles.
        unsafe {
            PostMessageW(self.hwnd, message, state, pack(x, y));
        }
    }

    /// Posts a wheel notch over a point, as the mouse would.
    pub(crate) fn post_wheel(&self, x: i32, y: i32, delta: i32, ctrl: bool) {
        let mut point = Point { x, y };
        // SAFETY: the wheel carries the point in screen coordinates, so the probe converts
        // through the same call the real message would go through.
        unsafe { ClientToScreen(self.hwnd, &mut point) };
        let state = if ctrl { MK_CONTROL } else { 0 } | ((delta as usize & 0xFFFF) << 16);
        // SAFETY: the window is live and the message is one it handles.
        unsafe {
            PostMessageW(self.hwnd, WM_MOUSEWHEEL, state, pack(point.x, point.y));
        }
    }

    /// Posts the frame's timer message.
    pub(crate) fn post_timer(&self) {
        // SAFETY: the window is live and the message is one it handles.
        unsafe {
            PostMessageW(self.hwnd, WM_TIMER, TIMER_ID, 0);
        }
    }

    /// What the window's own hit test answers for a point in the client area, through the real
    /// message path: the window procedure converts the point and asks the presenter.
    pub(crate) fn hit_test(&self, x: i32, y: i32) -> isize {
        let mut point = Point { x, y };
        // SAFETY: the window is live and the point is a live local.
        unsafe { ClientToScreen(self.hwnd, &mut point) };
        // SAFETY: the window is live and the message is one it handles.
        unsafe { SendMessageW(self.hwnd, WM_NCHITTEST, 0, pack(point.x, point.y)) }
    }

    /// Asks for a file to read.
    pub(crate) fn open_dialog(
        &self,
        title: &str,
        description: &str,
        pattern: &str,
        extension: &str,
    ) -> Option<std::path::PathBuf> {
        self.file_dialog(title, description, pattern, extension, false)
    }

    /// Asks where to write a file.
    pub(crate) fn save_dialog(
        &self,
        title: &str,
        description: &str,
        pattern: &str,
        extension: &str,
    ) -> Option<std::path::PathBuf> {
        self.file_dialog(title, description, pattern, extension, true)
    }

    /// The dialog itself: one `OPENFILENAMEW` for both directions.
    fn file_dialog(
        &self,
        title: &str,
        description: &str,
        pattern: &str,
        extension: &str,
        saving: bool,
    ) -> Option<std::path::PathBuf> {
        let mut filter: Vec<u16> = Vec::new();
        for part in [description, pattern] {
            filter.extend(part.encode_utf16());
            filter.push(0);
        }
        filter.push(0);
        let title = wide(title);
        let extension = wide(extension);
        let mut file: Vec<u16> = vec![0; DIALOG_PATH];
        let mut dialog = OpenFileNameW {
            size: std::mem::size_of::<OpenFileNameW>() as u32,
            owner: self.hwnd,
            instance: instance(),
            filter: filter.as_ptr(),
            custom_filter: std::ptr::null_mut(),
            max_custom_filter: 0,
            filter_index: 1,
            file: file.as_mut_ptr(),
            max_file: DIALOG_PATH as u32,
            file_title: std::ptr::null_mut(),
            max_file_title: 0,
            initial_dir: std::ptr::null(),
            title: title.as_ptr(),
            flags: OFN_EXPLORER
                | OFN_NOCHANGEDIR
                | OFN_PATHMUSTEXIST
                | OFN_FILEMUSTEXIST
                | if saving { OFN_OVERWRITEPROMPT } else { 0 },
            file_offset: 0,
            file_extension: 0,
            default_extension: extension.as_ptr(),
            custom_data: 0,
            hook: std::ptr::null_mut(),
            template: std::ptr::null(),
            reserved: std::ptr::null_mut(),
            reserved_value: 0,
            flags_ex: 0,
        };
        // SAFETY: the dialog struct describes live buffers that outlive the call, and the
        // owner window is live. The call is modal and returns when the person is done.
        let chosen = unsafe {
            if saving {
                GetSaveFileNameW(&mut dialog)
            } else {
                GetOpenFileNameW(&mut dialog)
            }
        };
        if chosen == 0 {
            return None;
        }
        let end = file.iter().position(|unit| *unit == 0)?;
        let path = String::from_utf16_lossy(&file[..end]);
        if path.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(path))
        }
    }

    /// Asks Windows to repaint the window.
    pub(crate) fn invalidate(&self) {
        // SAFETY: a null rectangle means the whole client area, and the window is live.
        unsafe { InvalidateRect(self.hwnd, std::ptr::null(), 0) };
    }

    /// Sets the title bar.
    pub(crate) fn set_title(&self, title: &str) {
        let title = wide(title);
        // SAFETY: the string is NUL-terminated and lives for the call.
        unsafe { SetWindowTextW(self.hwnd, title.as_ptr()) };
    }

    /// Runs one blocking turn of the message loop, dispatching to the handler.
    ///
    /// Returns `false` once the window has been destroyed.
    pub(crate) fn wait_for_event(&self) -> bool {
        let mut message: Message = unsafe { std::mem::zeroed() };
        // SAFETY: GetMessageW fills in a live local and returns 0 only for the quit message.
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result <= 0 {
            return false;
        }
        // SAFETY: the message came from the queue and belongs to this thread.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        true
    }

    /// Handles every message already queued, without blocking.
    ///
    /// Returns the number of messages dispatched.
    pub(crate) fn drain(&self) -> usize {
        let mut count = 0;
        loop {
            let mut message: Message = unsafe { std::mem::zeroed() };
            // SAFETY: PeekMessageW fills in a live local and only reports whether a message
            // was there; nothing is dispatched unless it says so.
            let found =
                unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) };
            if found == 0 {
                return count;
            }
            if message.message == WM_QUIT {
                return count;
            }
            // SAFETY: the message came from this thread's queue.
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            count += 1;
        }
    }

    /// Posts a key press as though the keyboard had produced it.
    pub(crate) fn post_key(&self, code: u16) {
        // SAFETY: the window is live and the message is one it handles.
        unsafe { PostMessageW(self.hwnd, WM_KEYDOWN, code as usize, 0) };
    }

    /// Posts a character as though the keyboard had produced it.
    pub(crate) fn post_text(&self, character: u16) {
        // SAFETY: the window is live and the message is one it handles.
        unsafe { PostMessageW(self.hwnd, WM_CHAR, character as usize, 0) };
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        CURRENT.with(|current| *current.borrow_mut() = None);
        if !self.hwnd.is_null() {
            // SAFETY: the window is ours and this runs once, when the Window is dropped.
            unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

/// A memory device context with a bitmap selected into it, plus the bit it was made from.
#[derive(Debug)]
pub(crate) struct Scratch {
    hdc: Hdc,
    bitmap: Hbitmap,
    source: Hdc,
    width: i32,
    height: i32,
}

impl Scratch {
    /// A scratch surface that belongs to no window: a 1x1 field of colour is all an alpha fill
    /// needs, and a surface tied to a window would have to be rebuilt when the window is.
    pub(crate) fn detached(width: i32, height: i32) -> Self {
        // SAFETY: the screen context is asked for and released around the creation, and the
        // bitmap and context are released in Drop.
        unsafe {
            let source = GetDC(std::ptr::null_mut());
            let hdc = CreateCompatibleDC(source);
            let bitmap = CreateCompatibleBitmap(source, width.max(1), height.max(1));
            SelectObject(hdc, bitmap);
            Self {
                hdc,
                bitmap,
                source,
                width: width.max(1),
                height: height.max(1),
            }
        }
    }

    /// The device context to draw into.
    pub(crate) fn hdc(&self) -> Hdc {
        self.hdc
    }

    /// The width of the surface, in pixels.
    pub(crate) const fn width(&self) -> i32 {
        self.width
    }

    /// The height of the surface, in pixels.
    pub(crate) const fn height(&self) -> i32 {
        self.height
    }

    /// Reads the pixels back as rows of blue, green, red and ignored, top to bottom.
    pub(crate) fn pixels(&self) -> Vec<u8> {
        let mut info = BitmapInfo::top_down(self.width, self.height);
        let mut buffer = vec![0u8; (self.width * self.height * 4) as usize];
        // SAFETY: `info` describes the buffer exactly, which is what GetDIBits requires, and
        // the bitmap is live in this context.
        unsafe {
            GetDIBits(
                self.hdc,
                self.bitmap,
                0,
                self.height as u32,
                buffer.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );
        }
        buffer
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // SAFETY: every handle here was created by this Scratch and is released once.
        unsafe {
            DeleteDC(self.hdc);
            DeleteObject(self.bitmap);
            ReleaseDC(std::ptr::null_mut(), self.source);
        }
    }
}

/// The identifier of the window's own timer, which is also the `wparam` of its message.
const TIMER_ID: usize = 1;

/// The clock, in seconds, from a counter that does not go backwards when the wall clock does.
pub(crate) fn now() -> f64 {
    let mut frequency = 0i64;
    let mut counter = 0i64;
    // SAFETY: both calls take live locals and touch nothing else.
    unsafe {
        QueryPerformanceFrequency(&mut frequency);
        QueryPerformanceCounter(&mut counter);
    }
    if frequency <= 0 {
        0.0
    } else {
        counter as f64 / frequency as f64
    }
}

/// Declares the process aware of its dots per inch, so the interface is drawn rather than
/// stretched by the system.
pub(crate) fn become_dpi_aware() {
    // SAFETY: the call takes no arguments and affects only this process's presentation.
    unsafe {
        SetProcessDPIAware();
    }
}

/// How much bigger a pixel is than at 96 dots per inch.
pub(crate) fn dpi_scale() -> f64 {
    // SAFETY: a screen context is always available, and it is released in the same block.
    unsafe {
        let screen = GetDC(std::ptr::null_mut());
        if screen.is_null() {
            return 1.0;
        }
        let dpi = GetDeviceCaps(screen, LOGPIXELSY);
        ReleaseDC(std::ptr::null_mut(), screen);
        if dpi <= 0 { 1.0 } else { f64::from(dpi) / 96.0 }
    }
}

/// A brush for a colour, made once and reused.
///
/// Glyph drawing asks for the same handful of colours thousands of times a second, and a brush
/// per call would be a GDI handle per call. The cache is bounded by the palette the interface
/// actually uses; its handles live until the process ends, which the process's own teardown
/// reclaims.
fn cached_brush(colour: Colour) -> Hgdi {
    thread_local! {
        static CACHE: RefCell<Vec<(u32, Hgdi)>> = const { RefCell::new(Vec::new()) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, handle)) = cache.iter().find(|(key, _)| *key == colour.0) {
            return *handle;
        }
        // SAFETY: CreateSolidBrush takes a colour by value; the handle is kept for the life of
        // the thread and never freed twice.
        let handle = unsafe { CreateSolidBrush(colour.0) };
        if !handle.is_null() {
            cache.push((colour.0, handle));
        }
        handle
    })
}

/// A pen for a colour and width, made once and reused, for the same reason as [`cached_brush`].
fn cached_pen(colour: Colour, width: i32) -> Hgdi {
    thread_local! {
        static CACHE: RefCell<Vec<((u32, i32), Hgdi)>> = const { RefCell::new(Vec::new()) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (colour.0, width);
        if let Some((_, handle)) = cache.iter().find(|(candidate, _)| *candidate == key) {
            return *handle;
        }
        // SAFETY: CreatePen takes three values; the handle is kept for the life of the thread.
        let handle = unsafe { CreatePen(0, width.max(1), colour.0) };
        if !handle.is_null() {
            cache.push((key, handle));
        }
        handle
    })
}

/// Fills a rectangle with a colour, without asking the caller to own a brush.
pub(crate) fn fill_colour(hdc: Hdc, rect: Rect, colour: Colour) {
    if rect.is_empty() {
        return;
    }
    // SAFETY: the context is live for the duration of a paint, and the brush is cached.
    unsafe { FillRect(hdc, &rect, cached_brush(colour)) };
}

/// A gradient from one colour at the top to another at the bottom.
///
/// GDI's own `GradientFill`, which is a driver-side blend rather than a loop here: a hundred
/// scan lines drawn one at a time would be a hundred calls per panel per frame.
pub(crate) fn gradient(hdc: Hdc, rect: Rect, top: Colour, bottom: Colour) {
    if rect.is_empty() {
        return;
    }
    // A `TRIVERTEX` carries each channel as 0..25500 rather than 0..255.
    let channels = |colour: Colour| {
        (
            ((colour.0 & 0xFF) * 100) as u16,
            (((colour.0 >> 8) & 0xFF) * 100) as u16,
            (((colour.0 >> 16) & 0xFF) * 100) as u16,
        )
    };
    let (red, green, blue) = channels(top);
    let (red2, green2, blue2) = channels(bottom);
    let vertices = [
        TriVertex {
            x: rect.left,
            y: rect.top,
            red,
            green,
            blue,
            alpha: 0,
        },
        TriVertex {
            x: rect.right,
            y: rect.bottom,
            red: red2,
            green: green2,
            blue: blue2,
            alpha: 0,
        },
    ];
    let rectangles = [GradientRect {
        upper_left: 0,
        lower_right: 1,
    }];
    // SAFETY: both arrays are live locals whose counts are passed with them.
    unsafe {
        GradientFill(
            hdc,
            vertices.as_ptr(),
            vertices.len() as u32,
            rectangles.as_ptr(),
            rectangles.len() as u32,
            GRADIENT_FILL_RECT_V,
        );
    }
}

/// Fills a rectangle with a colour at an opacity, over whatever is already there.
///
/// The source is a one-pixel surface: it is stretched, so the blend is a flat wash rather than
/// a copy of anything.
pub(crate) fn blend(hdc: Hdc, source: &Scratch, rect: Rect, colour: Colour, alpha: u8) {
    if rect.is_empty() {
        return;
    }
    let field = Rect::new(0, 0, 1, 1);
    // SAFETY: the scratch context is live and one pixel of it is being filled.
    unsafe { FillRect(source.hdc(), &field, cached_brush(colour)) };
    let blend = BlendFunction {
        blend_op: AC_SRC_OVER,
        blend_flags: 0,
        source_alpha: alpha,
        alpha_format: 0,
    };
    // SAFETY: the destination is live for the duration of a paint and the source is borrowed.
    unsafe {
        AlphaBlend(
            hdc,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            source.hdc(),
            0,
            0,
            1,
            1,
            blend,
        );
    }
}

/// A filled rounded rectangle, with an optional outline.
pub(crate) fn rounded(hdc: Hdc, rect: Rect, radius: i32, colour: Colour, outline: Option<Colour>) {
    if rect.is_empty() {
        return;
    }
    // SAFETY: the context is live for the duration of a paint; every handle is cached or a
    // stock object, and the two it replaces are put back before returning.
    unsafe {
        let brush = SelectObject(hdc, cached_brush(colour));
        let pen = match outline {
            Some(line) => SelectObject(hdc, cached_pen(line, 1)),
            None => SelectObject(hdc, GetStockObject(NULL_PEN)),
        };
        RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            radius.clamp(0, rect.height() / 2) * 2,
            radius.clamp(0, rect.height() / 2) * 2,
        );
        SelectObject(hdc, brush);
        SelectObject(hdc, pen);
    }
}

/// A filled polygon whose points are given in a square from (0,0) to (1,1).
pub(crate) fn polygon(hdc: Hdc, rect: Rect, points: &[(f32, f32)], colour: Colour) {
    let Some((mapped, count)) = map_points(rect, points) else {
        return;
    };
    // SAFETY: the context is live and the mapped points are a live local whose length is
    // passed with it.
    unsafe {
        let brush = SelectObject(hdc, cached_brush(colour));
        let pen = SelectObject(hdc, cached_pen(colour, 1));
        Polygon(hdc, mapped.as_ptr(), count);
        SelectObject(hdc, brush);
        SelectObject(hdc, pen);
    }
}

/// A polyline through points given in a square from (0,0) to (1,1).
pub(crate) fn polyline(hdc: Hdc, rect: Rect, points: &[(f32, f32)], colour: Colour, width: i32) {
    let Some((mapped, count)) = map_points(rect, points) else {
        return;
    };
    // SAFETY: as in `polygon`, with a pen of the requested width.
    unsafe {
        let pen = SelectObject(hdc, cached_pen(colour, width));
        Polyline(hdc, mapped.as_ptr(), count);
        SelectObject(hdc, pen);
    }
}

/// An ellipse inside a box given in a square from (0,0) to (1,1).
pub(crate) fn ellipse(hdc: Hdc, rect: Rect, box_: (f32, f32, f32, f32), colour: Colour) {
    let left = rect.left + (box_.0 * rect.width() as f32) as i32;
    let top = rect.top + (box_.1 * rect.height() as f32) as i32;
    let right = rect.left + (box_.2 * rect.width() as f32) as i32;
    let bottom = rect.top + (box_.3 * rect.height() as f32) as i32;
    // SAFETY: the context is live and both handles are cached.
    unsafe {
        let brush = SelectObject(hdc, cached_brush(colour));
        let pen = SelectObject(hdc, cached_pen(colour, 1));
        Ellipse(hdc, left, top, right, bottom);
        SelectObject(hdc, brush);
        SelectObject(hdc, pen);
    }
}

/// Maps normalized points into a rectangle, or gives up when there are too many for the
/// fixed buffer the shapes are drawn from.
fn map_points(rect: Rect, points: &[(f32, f32)]) -> Option<([Point; 64], i32)> {
    if points.len() < 2 || points.len() > 64 {
        return None;
    }
    let mut buffer = [Point::default(); 64];
    for (slot, (x, y)) in buffer.iter_mut().zip(points.iter()) {
        *slot = Point {
            x: rect.left + (x * rect.width() as f32) as i32,
            y: rect.top + (y * rect.height() as f32) as i32,
        };
    }
    Some((buffer, points.len() as i32))
}

/// Clips the rest of a paint to a rectangle, returning the state to restore it with.
pub(crate) fn clip(hdc: Hdc, rect: Rect) -> i32 {
    // SAFETY: the context is live for the duration of a paint.
    unsafe {
        let saved = SaveDC(hdc);
        IntersectClipRect(hdc, rect.left, rect.top, rect.right, rect.bottom);
        saved
    }
}

/// Puts the clipping back the way it was.
pub(crate) fn unclip(hdc: Hdc, saved: i32) {
    // SAFETY: `saved` came from `clip` on this same context during this same paint.
    unsafe {
        RestoreDC(hdc, saved);
    }
}

/// Sets the cursor the window is showing.
pub(crate) fn apply_cursor(kind: CursorKind) {
    let identifier = match kind {
        CursorKind::Arrow => IDC_ARROW,
        CursorKind::Text => IDC_IBEAM,
        CursorKind::Cross => IDC_CROSS,
        CursorKind::Horizontal => IDC_SIZEWE,
        CursorKind::Vertical => IDC_SIZENS,
        CursorKind::Move => IDC_SIZEALL,
        CursorKind::Point => IDC_HAND,
    };
    // SAFETY: the resource identifier is a constant and the loaded cursor belongs to the
    // system, so it is not freed here.
    unsafe {
        let cursor = LoadCursorW(std::ptr::null_mut(), identifier as *const u16);
        SetCursor(cursor);
    }
}

/// Which cursor a region asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorKind {
    /// The ordinary pointer.
    Arrow,
    /// Text is editable here.
    Text,
    /// A precise point is being chosen, which is what scrubbing is.
    Cross,
    /// The edge between two panels, or the head of a clip.
    Horizontal,
    /// The edge between two rows.
    Vertical,
    /// Something is being moved.
    Move,
    /// Something is clickable.
    Point,
}

/// Builds an icon from 32-bit pixels, top-down and in BGRA order, of one square size.
///
/// The brand is a raster rather than a drawing — the owner's logo, decoded at build time — so the
/// colour bitmap is a DIB section filled once from a slice, and the mask is empty. An empty mask
/// says every pixel of the colour bitmap is opaque, which is what a supplied logo with a plate
/// under it wants; a drawn mark would need the mask's other half.
///
/// The caller owns the icon and must not free it while the window is using it.
pub(crate) fn icon_from_bgra(size: i32, pixels: &[u8]) -> Option<*mut c_void> {
    let needed = (size as usize).checked_mul(size as usize)?.checked_mul(4)?;
    if size <= 0 || pixels.len() < needed {
        return None;
    }
    // SAFETY: every handle created here is released before the function returns, except the
    // icon itself, which the caller owns. The DIB section is filled once, from a slice this
    // function checked the length of, so the copy cannot run past either buffer.
    unsafe {
        let info = BitmapInfo::top_down(size, size);
        let mut bits: *mut c_void = std::ptr::null_mut();
        let colour = CreateDIBSection(
            std::ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            std::ptr::null_mut(),
            0,
        );
        if colour.is_null() || bits.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u8>(), needed);
        // A monochrome bitmap's rows are padded to the nearest two bytes, and a clear bit means
        // "show the colour bitmap here" — which, with this mask, is everywhere.
        let stride = (size + 15) / 16 * 2;
        let empty = vec![0_u8; (stride * size).max(1) as usize];
        let mask = CreateBitmap(size, size, 1, 1, empty.as_ptr().cast());
        let info = IconInfo {
            is_icon: 1,
            x_hotspot: 0,
            y_hotspot: 0,
            mask,
            colour,
        };
        let icon = CreateIconIndirect(&info);
        DeleteObject(colour);
        DeleteObject(mask);
        if icon.is_null() { None } else { Some(icon) }
    }
}

/// Draws a scaled frame with smoothing, which is what makes a downscaled preview look like a
/// picture rather than like noise.
pub(crate) fn blit_smooth(hdc: Hdc, rect: Rect, width: u32, height: u32, pixels: &[u8]) {
    // SAFETY: the context is live for the duration of a paint. HALFTONE leaves the DC's brush
    // origin undefined, which is why it is reset immediately after.
    unsafe {
        let previous = SetStretchBltMode(hdc, HALFTONE);
        SetBrushOrgEx(hdc, 0, 0, std::ptr::null_mut());
        StretchDIBits(
            hdc,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            0,
            0,
            width as i32,
            height as i32,
            pixels.as_ptr().cast(),
            &BitmapInfo::top_down(width as i32, height as i32),
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        SetStretchBltMode(hdc, previous);
        SetBrushOrgEx(hdc, 0, 0, std::ptr::null_mut());
    }
}

/// Copies one surface onto another, for the back buffer's journey to the screen.
pub(crate) fn copy(hdc: Hdc, source: Hdc, rect: Rect) {
    if rect.is_empty() {
        return;
    }
    // SAFETY: both contexts are live for the duration of the paint.
    unsafe {
        BitBlt(
            hdc,
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            source,
            rect.left,
            rect.top,
            SRCCOPY,
        );
    }
}

/// Draws a one-pixel border around a rectangle.
pub(crate) fn frame(hdc: Hdc, rect: Rect, brush: &Brush) {
    if rect.is_empty() {
        return;
    }
    // SAFETY: the context is live for the duration of a paint, and the brush is borrowed.
    unsafe { FrameRect(hdc, &rect, brush.0) };
}

/// Draws a straight line.
pub(crate) fn line(hdc: Hdc, from: (i32, i32), to: (i32, i32), pen: &Pen) {
    // SAFETY: the context is live for the duration of a paint, and the pen is borrowed.
    unsafe {
        SelectObject(hdc, pen.0);
        MoveToEx(hdc, from.0, from.1, std::ptr::null_mut());
        LineTo(hdc, to.0, to.1);
    }
}

/// Draws one line of text inside a rectangle.
pub(crate) fn text(hdc: Hdc, rect: Rect, value: &str, colour: Colour, font: &Font, align: Align) {
    if rect.is_empty() || value.is_empty() {
        return;
    }
    let wide = wide(value);
    let mut rect = rect;
    let format = DT_SINGLELINE
        | DT_VCENTER
        | DT_NOPREFIX
        | DT_END_ELLIPSIS
        | match align {
            Align::Left => DT_LEFT,
            Align::Centre => DT_CENTER,
            Align::Right => DT_RIGHT,
        };
    // SAFETY: the context, font and string all outlive the call, and `rect` is a live local
    // that DrawTextW is allowed to modify in place.
    unsafe {
        SelectObject(hdc, font.0);
        SetTextColor(hdc, colour.0);
        SetBkMode(hdc, TRANSPARENT);
        DrawTextW(hdc, wide.as_ptr(), -1, &mut rect, format);
    }
}

/// How wide a string is in a font, which is what lets a menu or a tab be as wide as its own
/// label rather than as wide as a guess.
pub(crate) fn text_width(hdc: Hdc, value: &str, font: &Font) -> i32 {
    if value.is_empty() {
        return 0;
    }
    let wide = wide(value);
    let mut size = Size::default();
    // SAFETY: the context and font are live, and `size` is a live local for the call to fill in.
    unsafe {
        SelectObject(hdc, font.0);
        GetTextExtentPoint32W(hdc, wide.as_ptr(), (wide.len() - 1) as i32, &mut size);
    }
    size.width
}

/// Draws a paragraph of text inside a rectangle, wrapping at spaces.
pub(crate) fn text_block(hdc: Hdc, rect: Rect, value: &str, colour: Colour, font: &Font) {
    if rect.is_empty() || value.is_empty() {
        return;
    }
    let wide = wide(value);
    let mut rect = rect;
    // SAFETY: the context, font and string all outlive the call, and `rect` is a live local
    // that DrawTextW may modify in place.
    unsafe {
        SelectObject(hdc, font.0);
        SetTextColor(hdc, colour.0);
        SetBkMode(hdc, TRANSPARENT);
        DrawTextW(
            hdc,
            wide.as_ptr(),
            -1,
            &mut rect,
            DT_WORDBREAK | DT_NOPREFIX | DT_TOP | DT_LEFT,
        );
    }
}
