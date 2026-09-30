//! The Win32 entry points the GDI overlay backend needs.
//!
//! Declared by hand for the same reasons as `gui/win.rs`: a short dependency
//! list, a small binary, and identical linking under the MSVC and GNU targets.
//! Only windowing and GDI are here — none of this touches OpenGL, which is the
//! entire point of the module.
//!
//! The overlay is composited by the window manager rather than by us. A
//! `WS_EX_LAYERED` window is handed a premultiplied 32-bit bitmap through
//! `UpdateLayeredWindow` and the desktop mixes it in, so the same code path
//! serves the opaque overlays (`fakebsod`, `fakeupdate`, fullscreen images) and
//! the translucent ones (`faketoast`, `desktopnote`, a centred image) without a
//! special case. It also means the window never needs a paint message, so a
//! frame is: rasterise into a DIB, call `UpdateLayeredWindow`, done.

// The Win32 spellings are kept verbatim so each declaration can be read against
// the SDK header by name.
#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;

pub type HWND = *mut c_void;
pub type HDC = *mut c_void;
pub type HGDIOBJ = *mut c_void;
pub type HBITMAP = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type UINT = u32;
pub type DWORD = u32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type BOOL = i32;
pub type COLORREF = u32;

pub type WNDPROC = unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT;

// --- Window styles ----------------------------------------------------------
pub const WS_POPUP: DWORD = 0x8000_0000;
pub const WS_EX_LAYERED: DWORD = 0x0008_0000;
pub const WS_EX_TOPMOST: DWORD = 0x0000_0008;
/// Keeps the overlay out of the taskbar and the Alt+Tab list, which a prank
/// window has no business appearing in.
pub const WS_EX_TOOLWINDOW: DWORD = 0x0000_0080;
/// The overlay never takes focus, so the target keeps typing and the keystrokes
/// do not land in a dialog that is not really a dialog.
pub const WS_EX_NOACTIVATE: DWORD = 0x0800_0000;

// --- Window classes ---------------------------------------------------------
pub const CS_HREDRAW: u32 = 0x0002;
pub const CS_VREDRAW: u32 = 0x0001;

// --- Messages ---------------------------------------------------------------
pub const WM_DESTROY: UINT = 0x0002;
pub const WM_PAINT: UINT = 0x000F;
pub const WM_TIMER: UINT = 0x0113;

/// `IDC_ARROW` is a `MAKEINTRESOURCE` id, so it is the bare number.
pub const IDC_ARROW: usize = 32512;

/// `SW_SHOWNOACTIVATE`. The overlays are `WS_EX_NOACTIVATE` and this keeps them
/// that way, so a prank never takes focus off whatever the target is doing.
pub const SW_SHOWNOACTIVATE: i32 = 4;

// --- GDI --------------------------------------------------------------------
/// `DIB_RGB_COLORS`, passed as the `usage` argument.
pub const DIB_RGB_COLORS: u32 = 0;
pub const BI_RGB: u32 = 0;
pub const TRANSPARENT: i32 = 1;
/// GDI clips text to the update region by default; a fake dialog has no reason
/// to look like it is being redrawn in slices.
pub const CLIP_DEFAULT: DWORD = 0x0000_0000;

// --- UpdateLayeredWindow ----------------------------------------------------
pub const ULW_ALPHA: DWORD = 0x0000_0002;
pub const AC_SRC_OVER: u8 = 0x00;
pub const AC_SRC_ALPHA: u8 = 0x01;

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

#[repr(C)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: UINT,
    pub wparam: WPARAM,
    pub lparam: LPARAM,
    pub time: DWORD,
    pub pt: POINT,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: UINT,
    pub style: UINT,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}

#[repr(C)]
pub struct BITMAPINFOHEADER {
    pub biSize: UINT,
    pub biWidth: i32,
    /// Negative for a top-down DIB, which is what is wanted here: the buffer is
    /// then laid out in reading order, so row 0 is the top of the image and no
    /// flipping is needed anywhere.
    pub biHeight: i32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: DWORD,
    pub biSizeImage: DWORD,
    pub biXPelsPerMeter: i32,
    pub biYPelsPerMeter: i32,
    pub biClrUsed: DWORD,
    pub biClrImportant: DWORD,
}

/// `CreateDIBSection` wants a `BITMAPINFO`; with no colour table or masks used
/// the three-DWORD tail is still part of the struct.
#[repr(C)]
pub struct BITMAPINFO {
    pub bmiHeader: BITMAPINFOHEADER,
    pub bmiColors: [COLORREF; 3],
}

#[repr(C)]
pub struct BLENDFUNCTION {
    pub BlendOp: u8,
    pub BlendFlags: u8,
    pub SourceConstantAlpha: u8,
    pub AlphaFormat: u8,
}

#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    pub fn UnregisterClassW(class: *const u16, instance: HINSTANCE) -> BOOL;
    pub fn CreateWindowExW(
        ex_style: DWORD,
        class: *const u16,
        title: *const u16,
        style: DWORD,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: HGDIOBJ,
        instance: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub fn DestroyWindow(hwnd: HWND) -> BOOL;
    pub fn DefWindowProcW(hwnd: HWND, message: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    pub fn GetMessageW(message: *mut MSG, hwnd: HWND, min: u32, max: u32) -> BOOL;
    pub fn TranslateMessage(message: *const MSG) -> BOOL;
    pub fn DispatchMessageW(message: *const MSG) -> LRESULT;
    pub fn PostQuitMessage(exit_code: i32);
    pub fn SetTimer(hwnd: HWND, id: usize, interval: u32, callback: *mut c_void) -> usize;
    pub fn KillTimer(hwnd: HWND, id: usize) -> BOOL;
    pub fn BeginPaint(hwnd: HWND, paint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hwnd: HWND, paint: *mut PAINTSTRUCT) -> BOOL;
    pub fn GetDC(hwnd: HWND) -> HDC;
    pub fn ReleaseDC(hwnd: HWND, dc: HDC) -> i32;
    pub fn LoadCursorW(instance: HINSTANCE, name: *const u16) -> HCURSOR;
    pub fn ShowWindow(hwnd: HWND, command: i32) -> BOOL;
    pub fn UpdateLayeredWindow(
        hwnd: HWND,
        screen_dc: HDC,
        destination: *const POINT,
        size: *const SIZE,
        source_dc: HDC,
        source: *const POINT,
        key: COLORREF,
        blend: *const BLENDFUNCTION,
        flags: DWORD,
    ) -> BOOL;
    pub fn SetProcessDPIAware() -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateCompatibleDC(dc: HDC) -> HDC;
    pub fn DeleteDC(dc: HDC) -> BOOL;
    pub fn SelectObject(dc: HDC, object: HGDIOBJ) -> HGDIOBJ;
    pub fn DeleteObject(object: HGDIOBJ) -> BOOL;
    pub fn CreateDIBSection(
        dc: HDC,
        info: *const BITMAPINFO,
        usage: UINT,
        bits: *mut *mut c_void,
        section: HANDLE_SECTION,
        offset: DWORD,
    ) -> HBITMAP;
    pub fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike_out: u32,
        charset: u32,
        output: u32,
        clip: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> HGDIOBJ;
    pub fn GetTextExtentPoint32W(dc: HDC, text: *const u16, count: i32, size: *mut SIZE) -> BOOL;
    pub fn SetTextColor(dc: HDC, colour: COLORREF) -> COLORREF;
    pub fn SetBkMode(dc: HDC, mode: i32) -> i32;
    pub fn TextOutW(dc: HDC, x: i32, y: i32, text: *const u16, count: i32) -> BOOL;
    pub fn GetStockObject(index: i32) -> HGDIOBJ;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(module: *const u16) -> HINSTANCE;
}

/// A file-mapping handle. Only ever passed as null to `CreateDIBSection`, but
/// it has its own type in the SDK rather than reusing `HANDLE`.
pub type HANDLE_SECTION = *mut c_void;

/// Arguments for `CreateFontW`.
///
/// A TrueType face is requested so the fake system dialogs get the same smoothed
/// Segoe UI the real ones use, rather than a bitmap-struck face.
///
/// `ANTIALIASED_QUALITY` rather than `CLEARTYPE_QUALITY`, deliberately.
pub const DEFAULT_CHARSET: u32 = 1;
pub const OUT_TT_PRECIS: u32 = 4;
pub const FF_DONTCARE: u32 = 0;
pub const ANTIALIASED_QUALITY: u32 = 4;
pub const FW_NORMAL: i32 = 400;
pub const FW_BOLD: i32 = 700;
pub const DEFAULT_PITCH: u32 = 0;

/// `DEFAULT_GUI_FONT` stock object, restored into a memory DC before a face that
/// is still selected into it is deleted.
pub const DEFAULT_GUI_FONT: i32 = 17;
