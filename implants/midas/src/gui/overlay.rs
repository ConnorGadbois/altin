//! The egui application that hosts every overlay.
//!
//! One `eframe` instance, one winit event loop, many viewports. See the module
//! docs in `gui/mod.rs` for why that is the only shape that works.
//!
//! Overlays are drawn rather than borrowed from native widgets because there is
//! no native "fake blue screen" to borrow. The exception is `msgbox`, which uses
//! the real `MessageBoxW` on Windows precisely *because* a genuine system dialog
//! is the joke - see `gui/win.rs`.
//!
//! ## Drawing style
//!
//! Each overlay's callback shows an empty `CentralPanel` purely to obtain a
//! `LayerId` and the viewport's available rect, then paints with the returned
//! `Painter`. That is deliberate: the panels' default frame, margins and
//! `CentralPanel` styling are all unwanted for a full-bleed fake dialog, and
//! painting directly keeps every element positioned in absolute pixels.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use egui::{
    pos2, vec2, Align2, CentralPanel, Color32, Context, FontId, Frame, Painter, Pos2, Rect, Stroke,
    TextureHandle, TextureOptions, Vec2, ViewportBuilder, ViewportId, WindowLevel,
};

use super::{current_stop_epoch, register_active, unregister_active, Active, Overlay, Request};
use crate::rng::Rng;

/// Animated overlays redraw on this cadence. A full-rate repaint would peg a
/// core for the lifetime of a `spook` overlay; 20fps is plenty for static.
const SPOOK_FRAME_MS: u64 = 50;

/// Textures larger than this are downscaled before upload.
///
/// The ceiling is not arbitrary: egui's default `max_texture_side` is 2048, and
/// `load_texture` debug-asserts against it. Going above that would need the
/// context's option changed as well.
const MAX_TEXTURE_DIM: u32 = 2048;

/// The UV rectangle covering a whole texture.
const UV_FULL: Rect = Rect {
    min: Pos2::ZERO,
    max: pos2(1.0, 1.0),
};

/// Decoded, ready-to-draw overlay state.
enum Content {
    Image {
        texture: TextureHandle,
        natural: Vec2,
        mode: String,
        topmost: bool,
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
        x: f32,
        y: f32,
        size: f32,
    },
    Spook {
        mode: String,
    },
}

struct Live {
    content: Content,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    deadline: Option<Instant>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) fn run(rx: Receiver<Request>) {
    let options = eframe::NativeOptions {
        // The root viewport is never shown; every visible surface is a child
        // viewport. Without this there is a stray 1x1 window on the desktop.
        viewport: ViewportBuilder::default()
            .with_app_id("midas-root")
            .with_visible(false)
            .with_decorations(false)
            .with_inner_size([1.0, 1.0]),
        ..Default::default()
    };

    // Both an error and a panic have to be reported rather than swallowed.
    //
    // `run_native` returning is the only signal the check-in thread gets, and it
    // only ever learns "the channel is dead" - which says nothing about the
    // cause. The overwhelmingly common cause is a target with no usable OpenGL
    // implementation, such as a bare VM or an RDP session, where context
    // creation fails before the first frame. That is an operator-facing fact:
    // every overlay command will fail on this host, and the reason should come
    // back in the task result instead of a generic message.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            "midas",
            options,
            Box::new(move |_cc| Ok(Box::new(OverlayApp { rx, live: HashMap::new() }))),
        )
    }));

    let reason = match outcome {
        Ok(Ok(())) => "the overlay event loop exited on its own".to_string(),
        Ok(Err(e)) => format!("could not start the overlay event loop: {e}"),
        Err(panic) => {
            // A panic payload is a `&str` or `String` depending on how it was
            // raised, and anything else is not worth printing.
            let payload = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "(no message)".to_string());

            format!(
                "the overlay event loop panicked: {}",
                crate::log::normalise_panic(&payload)
            )
        }
    };

    super::record_host_failure(reason);
}

struct OverlayApp {
    rx: Receiver<Request>,
    live: HashMap<u64, Live>,
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);
        self.expire();

        let screen = screen_rect(ctx);
        let time = ctx.input(|i| i.time);

        let ids: Vec<u64> = self.live.keys().copied().collect();
        let mut animated: Vec<ViewportId> = Vec::new();

        for id in ids {
            let Some(live) = self.live.get_mut(&id) else {
                continue;
            };

            let viewport_id = ViewportId::from_hash_of(id);
            let builder = live.content.builder(ctx, id, screen);

            if live.content.animated() {
                animated.push(viewport_id);
            }

            // A viewport stays alive for as long as `show_viewport_immediate` is
            // called with the same id, so dropping the entry in `expire` is what
            // closes the window.
            ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
                let panel = CentralPanel::default()
                    .frame(Frame::none())
                    .show(ctx, |_ui| {});

                let rect = panel.response.rect;
                let mut painter = panel.response.ctx.layer_painter(panel.response.layer_id);
                painter.set_clip_rect(rect);

                live.content.draw(&mut painter, rect, time, id);
            });
        }

        // Repaints are requested per-viewport so the other overlays do not have
        // to be redrawn 20 times a second just because one is animating.
        for viewport_id in animated {
            ctx.request_repaint_after_for(Duration::from_millis(SPOOK_FRAME_MS), viewport_id);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        for id in self.live.keys() {
            unregister_active(*id);
        }
        self.live.clear();
    }
}

impl OverlayApp {
    fn drain(&mut self, ctx: &Context) {
        while let Ok(request) = self.rx.try_recv() {
            // A stop that landed between submission and this drain wins. See the
            // STOP_EPOCH note in gui/mod.rs.
            if request.epoch < current_stop_epoch() {
                continue;
            }

            match prepare(&request, ctx) {
                Ok(content) => {
                    let deadline = request.duration.map(|d| Instant::now() + d);

                    register_active(Active {
                        id: request.id,
                        label: request.kind.label(),
                        born: SystemTime::now(),
                        deadline,
                        cancel: request.cancel.clone(),
                    });

                    self.live.insert(
                        request.id,
                        Live {
                            content,
                            cancel: request.cancel,
                            deadline,
                        },
                    );
                }
                Err(e) => {
                    // Paths are checked before the ack is sent, so reaching here
                    // means the file became unreadable, or stopped being a
                    // decodable image, in between.
                    crate::log::log(&format!("overlay {} failed: {e}", request.id));
                }
            }
        }
    }

    fn expire(&mut self) {
        let now = Instant::now();

        let expired: Vec<u64> = self
            .live
            .iter()
            .filter(|(_, live)| {
                live.cancel.load(Ordering::SeqCst)
                    || live.deadline.is_some_and(|d| now >= d)
            })
            .map(|(id, _)| *id)
            .collect();

        for id in expired {
            self.live.remove(&id);
            unregister_active(id);
        }
    }
}

/// The whole screen, falling back to a plausible desktop if the backend has not
/// reported one yet.
fn screen_rect(ctx: &Context) -> Rect {
    let rect = ctx.input(|i| i.screen_rect);
    if rect.width() > 0.0 && rect.height() > 0.0 {
        rect
    } else {
        // Reported as `Rect::NOTHING` until the first frame completes, which
        // would otherwise produce zero-sized viewports on the first pass.
        Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))
    }
}

// ---------------------------------------------------------------------------
// Preparation
// ---------------------------------------------------------------------------

fn prepare(request: &Request, ctx: &Context) -> Result<Content, String> {
    match &request.kind {
        Overlay::Image {
            path,
            mode,
            topmost,
        } => {
            let (texture, natural) = load_texture(ctx, path)?;
            Ok(Content::Image {
                texture,
                natural,
                mode: mode.clone(),
                topmost: *topmost,
            })
        }

        // A scare is just a fullscreen image; the sound half is played by the
        // command layer so this side stays purely visual.
        Overlay::Scare { path } => {
            let (texture, natural) = load_texture(ctx, path)?;
            Ok(Content::Image {
                texture,
                natural,
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

fn load_texture(ctx: &Context, path: &str) -> Result<(TextureHandle, Vec2), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| format!("{path}: {e}"))?
        .to_rgba8();

    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 {
        return Err(format!("{path}: image has no dimensions"));
    }

    let decoded = if width > MAX_TEXTURE_DIM || height > MAX_TEXTURE_DIM {
        let scale = MAX_TEXTURE_DIM as f32 / width.max(height) as f32;
        image::imageops::resize(
            &decoded,
            (width as f32 * scale).round() as u32,
            (height as f32 * scale).round() as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };

    let (width, height) = decoded.dimensions();
    let natural = Vec2::new(width as f32, height as f32);
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [width as usize, height as usize],
        decoded.as_raw(),
    );

    let texture = ctx.load_texture(
        format!("midas-{path}"),
        color,
        TextureOptions::LINEAR,
    );

    Ok((texture, natural))
}

// ---------------------------------------------------------------------------
// Viewport geometry
// ---------------------------------------------------------------------------

impl Content {
    fn animated(&self) -> bool {
        matches!(self, Content::Spook { .. })
    }

    fn builder(&self, ctx: &Context, id: u64, screen: Rect) -> ViewportBuilder {
        let app_id = format!("midas-{id}");
        let title = format!("midas {id}");
        let screen = screen.size();

        let base = ViewportBuilder::default()
            .with_app_id(app_id)
            .with_title(title)
            .with_decorations(false)
            .with_resizable(false);

        match self {
            Content::Image {
                natural, mode, topmost, ..
            } => {
                let size = match mode.as_str() {
                    "fullscreen" => screen,
                    "center" => *natural,
                    // `fit` is the only remaining mode, so it is the default arm.
                    _ => *natural * 0.7,
                };

                let size = Vec2::new(
                    size.x.min(screen.x).max(1.0),
                    size.y.min(screen.y).max(1.0),
                );

                base.with_inner_size(size)
                    .with_position(pos2(
                        ((screen.x - size.x) / 2.0).round(),
                        ((screen.y - size.y) / 2.0).round(),
                    ))
                    .with_transparent(mode != "fullscreen")
                    .with_window_level(if *topmost {
                        WindowLevel::AlwaysOnTop
                    } else {
                        // `Normal` rather than "unset": a viewport level must
                        // be chosen explicitly to override whatever the root
                        // window happens to use.
                        WindowLevel::Normal
                    })
            }

            Content::Bsod { .. } | Content::Update { .. } | Content::Spook { .. } => base
                .with_fullscreen(true)
                .with_window_level(WindowLevel::AlwaysOnTop),

            Content::Toast { title, body } => {
                let _ = title;

                // Measure the body so a long notification is not clipped by a
                // fixed-height window.
                let galley = ctx.fonts(|f| {
                    f.layout(
                        body.clone(),
                        FontId::proportional(13.0),
                        Color32::PLACEHOLDER,
                        TOAST_TEXT_WIDTH,
                    )
                });

                let size = Vec2::new(
                    TOAST_TEXT_WIDTH + TOAST_PADDING * 2.0,
                    (TOAST_HEADER + galley.size().y + TOAST_PADDING).min(320.0),
                );
                let margin = 16.0;

                base.with_inner_size(size)
                    .with_position(pos2(
                        (screen.x - size.x - margin).max(0.0).round(),
                        (screen.y - size.y - margin).max(0.0).round(),
                    ))
                    .with_transparent(true)
                    .with_window_level(WindowLevel::AlwaysOnTop)
            }

            Content::Note {
                text, x, y, size: font_size, ..
            } => {
                let font = FontId::proportional(*font_size);
                let galley = ctx.fonts(|f| {
                    f.layout(
                        text.clone(),
                        font,
                        Color32::PLACEHOLDER,
                        screen.x * 0.7,
                    )
                });

                let size = Vec2::new(
                    (galley.size().x + 32.0).clamp(64.0, screen.x * 0.95),
                    (galley.size().y + 24.0).max(font_size * 2.0),
                );

                base.with_inner_size(size)
                    .with_position(pos2(
                        x.clamp(0.0, (screen.x - size.x).max(0.0)),
                        y.clamp(0.0, (screen.y - size.y).max(0.0)),
                    ))
                    .with_transparent(true)
                    .with_window_level(WindowLevel::AlwaysOnTop)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    fn draw(&self, painter: &mut Painter, rect: Rect, time: f64, seed: u64) {
        match self {
            Content::Image {
                texture,
                natural,
                mode,
                ..
            } => draw_image(painter, rect, texture, *natural, mode),

            Content::Bsod { stopcode, percent } => {
                draw_bsod(painter, rect, stopcode, *percent)
            }

            Content::Update { title, percent } => {
                draw_update(painter, rect, title, *percent)
            }

            Content::Toast { title, body } => draw_toast(painter, rect, title, body),

            Content::Note { text, size, .. } => {
                draw_note(painter, rect, text, *size)
            }

            Content::Spook { mode } => draw_spook(painter, rect, mode, time, seed),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-overlay painters
// ---------------------------------------------------------------------------

/// Toast metrics, shared between sizing the window and painting into it.
const TOAST_PADDING: f32 = 14.0;
const TOAST_TEXT_WIDTH: f32 = 320.0;
const TOAST_HEADER: f32 = 40.0;
const TOAST_LINE_HEIGHT: f32 = 17.0;

fn draw_image(
    painter: &Painter,
    rect: Rect,
    texture: &TextureHandle,
    natural: Vec2,
    mode: &str,
) {
    let (scale, offset) = if mode == "fullscreen" {
        // `cover`: fill the viewport and let the overflow crop. More dramatic
        // than letterboxing, which is the point for a fullscreen scare.
        let scale = (rect.width() / natural.x).max(rect.height() / natural.y);
        (
            scale,
            Vec2::new(
                (rect.width() - natural.x * scale) / 2.0,
                (rect.height() - natural.y * scale) / 2.0,
            ),
        )
    } else {
        // `contain`: never upscale past 1:1, never crop.
        let scale = (rect.width() / natural.x).min(rect.height() / natural.y).min(1.0);
        (scale, (rect.size() - natural * scale) / 2.0)
    };

    painter.image(
        texture.id(),
        Rect::from_min_size(rect.min + offset, natural * scale),
        UV_FULL,
        Color32::WHITE,
    );
}

fn draw_bsod(painter: &Painter, rect: Rect, stopcode: &str, percent: i64) {
    let white = Color32::WHITE;
    painter.rect_filled(rect, 0.0, Color32::from_rgb(0, 0, 170));

    let pad = rect.width() * 0.06;

    // The frown, at roughly the height a real BSOD puts it.
    let frown = FontId::monospace((rect.height() * 0.15).min(rect.width() * 0.15));
    painter.text(
        pos2(rect.center().x, rect.min.y + rect.height() * 0.26),
        Align2::CENTER_CENTER,
        ":(",
        frown,
        white,
    );

    let bar_size = Vec2::new(rect.width() * 0.26, 18.0);
    let bar = Rect::from_center_size(
        pos2(rect.center().x, rect.min.y + rect.height() * 0.52),
        bar_size,
    );
    painter.rect_filled(bar, 2.0, Color32::from_white_alpha(45));
    painter.rect_filled(
        Rect::from_min_size(
            bar.min,
            Vec2::new(
                bar_size.x * percent.clamp(0, 100) as f32 / 100.0,
                bar_size.y,
            ),
        ),
        2.0,
        white,
    );

    painter.text(
        pos2(rect.center().x, bar.max.y + 22.0),
        Align2::CENTER_TOP,
        format!("{percent}% complete"),
        FontId::proportional(17.0),
        white,
    );

    let body = FontId::proportional((rect.width() * 0.014).clamp(12.0, 22.0));
    let left = pos2(rect.min.x + pad, rect.max.y - pad);
    let right = pos2(rect.max.x - pad, rect.max.y - pad);

    let lines: [(&str, Align2, Pos2); 4] = [
        (
            "Your PC ran into a problem and needs to restart. We're just collecting some error info, and then we'll restart for you.",
            Align2::LEFT_BOTTOM,
            left,
        ),
        (
            "For more information about this error and possible fixes, visit",
            Align2::LEFT_BOTTOM,
            pos2(left.x, left.y - 46.0),
        ),
        (
            "https://www.windows.com/werr",
            Align2::LEFT_BOTTOM,
            pos2(left.x, left.y - 70.0),
        ),
        ("100% complete", Align2::RIGHT_BOTTOM, right),
    ];

    for (line, anchor, at) in lines {
        painter.text(at, anchor, line, body.clone(), white);
    }

    painter.text(
        pos2(right.x, right.y - 46.0),
        Align2::RIGHT_BOTTOM,
        format!("Stop code: {stopcode}"),
        body,
        white,
    );
}

fn draw_update(painter: &Painter, rect: Rect, title: &str, percent: i64) {
    let white = Color32::WHITE;
    let accent = Color32::from_rgb(0, 120, 215);

    painter.rect_filled(rect, 0.0, Color32::from_rgb(24, 32, 48));

    // A lighter band across the upper portion, approximating the Windows update
    // screen well enough to read as "this is a system dialog".
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), rect.height() * 0.4)),
        0.0,
        Color32::from_rgb(32, 48, 74),
    );

    painter.text(
        pos2(rect.center().x, rect.min.y + rect.height() * 0.30),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional((rect.width() * 0.026).clamp(20.0, 48.0)),
        white,
    );

    painter.text(
        pos2(rect.center().x, rect.min.y + rect.height() * 0.40),
        Align2::CENTER_CENTER,
        "Please wait while your computer installs updates",
        FontId::proportional((rect.width() * 0.013).clamp(12.0, 22.0)),
        Color32::from_white_alpha(200),
    );

    let bar_size = Vec2::new(rect.width() * 0.3, 8.0);
    let bar = Rect::from_center_size(
        pos2(rect.center().x, rect.min.y + rect.height() * 0.52),
        bar_size,
    );
    painter.rect_filled(bar, 4.0, Color32::from_white_alpha(40));
    painter.rect_filled(
        Rect::from_min_size(
            bar.min,
            Vec2::new(
                bar_size.x * percent.clamp(0, 100) as f32 / 100.0,
                bar_size.y,
            ),
        ),
        4.0,
        accent,
    );

    painter.text(
        pos2(rect.center().x, bar.max.y + 20.0),
        Align2::CENTER_CENTER,
        format!("{percent}% complete"),
        FontId::proportional((rect.width() * 0.012).clamp(12.0, 20.0)),
        Color32::from_white_alpha(180),
    );

    painter.text(
        pos2(rect.center().x, rect.max.y - rect.height() * 0.12),
        Align2::CENTER_CENTER,
        "Do not turn off your computer",
        FontId::proportional((rect.width() * 0.012).clamp(12.0, 20.0)),
        Color32::from_white_alpha(150),
    );
}

fn draw_toast(painter: &Painter, rect: Rect, title: &str, body: &str) {
    painter.rect_filled(rect, 8.0, Color32::from_rgb(43, 43, 43));
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, Color32::from_white_alpha(40)));

    painter.text(
        pos2(rect.min.x + TOAST_PADDING, rect.min.y + 12.0),
        Align2::LEFT_TOP,
        title,
        FontId::proportional(14.0),
        Color32::from_rgb(120, 180, 255),
    );

    // The painter's `text` does not wrap, so lines are laid out by hand. The
    // window was sized to fit the wrapped body in `builder`, so this only
    // truncates if the two disagree about the estimate.
    let font = FontId::proportional(13.0);
    let mut y = rect.min.y + TOAST_HEADER;

    let max_lines = (((rect.max.y - y - 8.0) / TOAST_LINE_HEIGHT).floor() as usize).max(1);

    for line in wrap_text(body, TOAST_TEXT_WIDTH, 13.0).into_iter().take(max_lines) {
        painter.text(
            pos2(rect.min.x + TOAST_PADDING, y),
            Align2::LEFT_TOP,
            line,
            font.clone(),
            Color32::from_gray(235),
        );
        y += TOAST_LINE_HEIGHT;
    }
}

fn draw_note(painter: &Painter, rect: Rect, text: &str, size: f32) {
    let font = FontId::proportional(size);

    // Transparent window, so a drop shadow is what keeps the text legible over
    // an arbitrary wallpaper.
    let lines = wrap_text(text, rect.width() - 24.0, size);

    for (offset, color) in [
        (2.0, Color32::from_black_alpha(190)),
        (0.0, Color32::from_rgb(255, 236, 120)),
    ] {
        let mut y = rect.min.y + 12.0 + offset;
        for line in &lines {
            painter.text(
                pos2(rect.min.x + 12.0 + offset, y),
                Align2::LEFT_TOP,
                line,
                font.clone(),
                color,
            );
            y += size * 1.25;
        }
    }
}

fn draw_spook(painter: &Painter, rect: Rect, mode: &str, time: f64, seed: u64) {
    match mode {
        "matrix" => draw_matrix(painter, rect, time, seed),
        "scanline" => draw_scanline(painter, rect, time),
        "glitch" => draw_glitch(painter, rect, time, seed),
        // `static` is the default arm, and the only mode with no time input.
        _ => draw_static(painter, rect, time, seed),
    }
}

fn draw_static(painter: &Painter, rect: Rect, time: f64, seed: u64) {
    painter.rect_filled(rect, 0.0, Color32::from_gray(8));

    let frame = (time * 30.0) as u64;
    let mut rng = Rng::new(seed ^ frame.wrapping_mul(0x9E37_79B9_7F4A_7C15));

    // A couple of thousand rects reads as noise and stays cheap; one rect per
    // pixel would not.
    for _ in 0..2200 {
        let w = rng.range(2.0, 12.0);
        let h = rng.range(1.0, 4.0);
        let grey = rng.range(20.0, 255.0) as u8;

        painter.rect_filled(
            Rect::from_min_size(
                pos2(rng.range(0.0, rect.width()), rng.range(0.0, rect.height())),
                Vec2::new(w, h),
            ),
            0.0,
            Color32::from_gray(grey),
        );
    }
}

fn draw_matrix(painter: &Painter, rect: Rect, time: f64, seed: u64) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(0, 8, 0));

    // ASCII only, deliberately: egui's bundled default font has no CJK
    // coverage, so katakana would render as blank boxes rather than glyphs.
    const GLYPHS: &[u8] = b"0123456789ABCDEF$<>#@%&*+=|\\";

    let cell: f32 = 18.0;
    let columns = (rect.width() / cell).ceil() as u32;
    let rows = (rect.height() / cell).ceil() as u32;
    let tail = 14i64;

    let font = FontId::monospace(cell * 0.9);

    for column in 0..columns {
        // Per-column speed and phase derived from the column index, so the
        // animation is a pure function of time and needs no stored state.
        let column_seed = seed ^ (column as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let speed = 6.0 + (column_seed % 9) as f64;
        let phase = ((column_seed >> 8) % 1000) as f64 / 1000.0;

        let span = rows as i64 + tail;
        let head = (time.mul_add(speed, phase * 60.0) % span as f64) as i64;

        for offset in 0..tail {
            let row = head - offset;
            if row < 0 || row >= rows as i64 {
                continue;
            }

            let index = (column_seed.rotate_left(row as u32 * 5) as usize) % GLYPHS.len();
            let glyph = GLYPHS[index] as char;

            // Brightest at the head, fading down the tail.
            let alpha = 255 - (offset as f32 / tail as f32 * 235.0) as u8;
            let color = if offset == 0 {
                Color32::from_rgb(200, 255, 200)
            } else {
                Color32::from_rgba_unmultiplied(0, 210, 60, alpha)
            };

            painter.text(
                pos2(column as f32 * cell, row as f32 * cell),
                Align2::LEFT_TOP,
                glyph,
                font.clone(),
                color,
            );
        }
    }
}

fn draw_scanline(painter: &Painter, rect: Rect, time: f64) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(6, 10, 6));

    let mut y = 0.0f32;
    while y < rect.height() {
        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.min.x, rect.min.y + y),
                Vec2::new(rect.width(), 1.0),
            ),
            0.0,
            Color32::from_black_alpha(140),
        );
        y += 3.0;
    }

    // A brighter band sweeping down, which is what sells it as a display fault.
    let span = rect.height() + 200.0;
    let band_y = ((time * 260.0) % span as f64) as f32 - 100.0;

    for offset in 0..3 {
        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.min.x, rect.min.y + band_y + offset as f32 * 2.0),
                Vec2::new(rect.width(), 1.0),
            ),
            0.0,
            Color32::from_rgb(120, 255, 160),
        );
    }
}

fn draw_glitch(painter: &Painter, rect: Rect, time: f64, seed: u64) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(10, 0, 14));

    let mut rng = Rng::new(seed ^ (time * 20.0) as u64);

    // Horizontal slices displaced sideways, the classic datamosh look.
    let slices = 40;
    let band = rect.height() / slices as f32;

    for index in 0..slices {
        let shift = if rng.f32() > 0.82 {
            rng.range(-90.0, 90.0)
        } else {
            0.0
        };

        let hue = rng.f32();
        let color = Color32::from_rgba_unmultiplied(
            (hue * 255.0) as u8,
            ((1.0 - hue) * 180.0) as u8,
            (rng.f32() * 200.0) as u8,
            90,
        );

        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.min.x + shift, rect.min.y + index as f32 * band),
                Vec2::new(rect.width(), band),
            ),
            0.0,
            color,
        );
    }

    // A few solid bright bands on top.
    for _ in 0..6 {
        let height = rng.range(2.0, 14.0);
        let alpha = rng.range(40.0, 150.0) as u8;

        painter.rect_filled(
            Rect::from_min_size(
                pos2(
                    rect.min.x + rng.range(-60.0, 60.0),
                    rect.min.y + rng.range(0.0, rect.height()),
                ),
                Vec2::new(rect.width(), height),
            ),
            0.0,
            Color32::from_white_alpha(alpha),
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Greedy word wrap. The painter's `text` call does not wrap, and these strings
/// are short enough that a per-character width estimate is good enough - the
/// only consequence of being out is a line that is a little short or a little
/// wide, and the window was sized with the same estimate.
fn wrap_text(text: &str, max_width: f32, font_size: f32) -> Vec<String> {
    let char_width = (font_size * 0.55).max(1.0);
    let per_line = (max_width / char_width).floor().max(1.0) as usize;

    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };

        if current.is_empty() || candidate.chars().count() <= per_line {
            current = candidate;
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
