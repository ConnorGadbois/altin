//! Opt-in logging, and capture of anything that panicked.
//!
//! Off unless `MIDAS_LOG` is set to something other than `0` or empty, so a
//! release build is silent on the operator's desktop by default. Diagnostics
//! from background threads go through here rather than `eprintln!` for that
//! reason - a prank that prints to a visible console is a prank that gives
//! itself away.
//!
//! Output goes to stderr, which on both target platforms is either invisible
//! (a GUI-subsystem build) or closed, so nothing lands in a visible window
//! unless the operator launched the implant from a terminal. That is the whole
//! reason panics are *also* kept in memory below: stderr is not a channel the
//! operator has access to on a target they are not sitting at, and a panic whose
//! only symptom is a command that quietly does nothing is not a diagnosis.

use std::sync::{Mutex, OnceLock};

/// How many panics are kept for `sysinfo` to report.
const RECENT_LIMIT: usize = 8;

static LOGGING: OnceLock<bool> = OnceLock::new();

pub fn enabled() -> bool {
    *LOGGING.get_or_init(|| {
        std::env::var("MIDAS_LOG")
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

pub fn log(message: &str) {
    if enabled() {
        eprintln!("[midas] {message}");
    }
}

/// `log` for a message that is only assembled when logging is on.
///
/// Used on paths that would otherwise format a string nobody reads.
pub fn log_lazy(build: impl FnOnce() -> String) {
    if enabled() {
        eprintln!("[midas] {}", build());
    }
}

// ---------------------------------------------------------------------------
// Panics
// ---------------------------------------------------------------------------

/// Recent panic messages, kept so `sysinfo` can report them.
static RECENT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn recent() -> &'static Mutex<Vec<String>> {
    RECENT.get_or_init(|| Mutex::new(Vec::new()))
}

/// Reduces a panic message to the part worth acting on.
///
/// A default payload reads
/// `thread 'midas-gui' panicked at <file>:<line>:<col>:\n<message>`. The thread
/// name is Midas' own and identifies nothing, the message sits on a second line
/// where it is easy to lose, and the location is the part actually worth acting
/// on - so the boilerplate is dropped and the rest is flattened onto one line.
pub fn normalise_panic(message: &str) -> String {
    let flattened = message.replace(['\n', '\r'], " ");
    let collapsed = flattened.split_whitespace().collect::<Vec<_>>().join(" ");

    match collapsed.split_once("panicked at ") {
        Some((_, rest)) => rest.to_string(),
        None => collapsed,
    }
}

/// Replaces the default panic hook.
///
/// The default one is deliberately *not* chained to. It prints
/// `thread '<name>' panicked at ...`, and because the name is always one of ours
/// that line comes out identical for every distinct failure - it named nothing,
/// so it read as a repeat of a problem already reported rather than as new
/// information. It goes to stderr as well, which on a target the operator has no
/// terminal on is nowhere at all.
///
/// The normalised form replaces it, and the same text is kept so `sysinfo` can
/// report it back over the same channel the commands arrived on.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        // `Display` on the hook info is the fully formatted message, location
        // included, and is the supported way to read it.
        let normalised = normalise_panic(&format!("{info}"));

        log(&format!("panic: {normalised}"));

        if let Ok(mut recent) = recent().lock() {
            if recent.len() == RECENT_LIMIT {
                recent.remove(0);
            }
            recent.push(normalised);
        }
    }));
}

/// The panics seen so far, newest last, for `sysinfo`.
pub fn recent_panics() -> Vec<String> {
    recent()
        .lock()
        .map(|recent| recent.clone())
        .unwrap_or_default()
}
