//! The painters, in GDI instead of egui.
//!
//! A deliberate transcription of `gui/overlay.rs` rather than a shared
//! abstraction. egui positions everything in floats against a `Rect` and blends
//! through a `Painter`; this positions in integers against a `Canvas` and
//! composites premultiplied bytes. The two agree on layout, colour and copy, and
//! nothing else, so a shared drawing layer would be an indirection over two
//! backends that do not have a common representation to abstract.

use super::canvas::{Canvas, Fit, Rect, Rgba};
use super::text::{Style, TextRenderer};
use crate::rng::Rng;

/// Animated overlays redraw on this cadence. A full-rate repaint would peg a
/// core for the lifetime of a `spook` overlay.
pub const SPOOK_FRAME_MS: u32 = 50;

/// The largest image dimension kept after decoding.
///
/// Not a texture limit - there is no texture here - but a bound on how much is
/// held in memory and how many destination pixels a fullscreen blit touches. A
/// prank image larger than this is downscaled, and 4K is already absurd for one.
const MAX_IMAGE_DIM: u32 = 4096;

/// Where and how big the overlay's window is.
pub struct Geometry {
    pub width: i32,
    pub height: i32,
    /// Top-left in screen coordinates.
    pub x: i32,
    pub y: i32,
    /// Whether the window stays above everything else.
    pub topmost: bool,
}

/// Decoded, ready-to-draw overlay state.
pub enum Content {
    Image {
        pixels: Vec<u8>,
        width: i32,
        height: i32,
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

impl Content {
    pub fn animated(&self) -> bool {
        matches!(self, Content::Spook { .. })
    }

    /// Decides the window size and position.
    ///
    /// `screen` is the full desktop in pixels, and `text` is needed because the
    /// notification and the desktop note are sized to their own content - the
    /// same measurement has to happen before the window exists and again while
    /// drawing into it, so it lives in one place.
    pub fn geometry(&self, screen: (i32, i32), text: &mut TextRenderer) -> Geometry {
        let (screen_w, screen_h) = (screen.0.max(1), screen.1.max(1));

        match self {
            Content::Image {
                width,
                height,
                mode,
                topmost,
                ..
            } => {
                let size = match mode.as_str() {
                    "fullscreen" => (screen_w, screen_h),
                    "center" => (*width, *height),
                    // `fit` is the only remaining mode, so it is the default arm.
                    _ => ((*width as f32 * 0.7) as i32, (*height as f32 * 0.7) as i32),
                };

                let width = size.0.clamp(1, screen_w);
                let height = size.1.clamp(1, screen_h);

                Geometry {
                    width,
                    height,
                    x: (screen_w - width) / 2,
                    y: (screen_h - height) / 2,
                    topmost: *topmost || mode == "fullscreen",
                }
            }

            Content::Bsod { .. } | Content::Update { .. } | Content::Spook { .. } => Geometry {
                width: screen_w,
                height: screen_h,
                x: 0,
                y: 0,
                topmost: true,
            },

            Content::Toast { title, body } => {
                let lines = text.wrap(body, TOAST_BODY, TOAST_TEXT_WIDTH);
                let title_w = text.measure(title, TOAST_TITLE);
                let body_w = lines
                    .iter()
                    .map(|line| text.measure(line, TOAST_BODY))
                    .max()
                    .unwrap_or(0);

                let width = (title_w.max(body_w) + TOAST_PADDING * 2).clamp(64, screen_w);
                let height =
                    (TOAST_HEADER + lines.len() as i32 * TOAST_LINE_HEIGHT + TOAST_PADDING)
                        .clamp(64, 320)
                        .min(screen_h);

                Geometry {
                    width,
                    height,
                    // Bottom-right, inset by a margin, which is where a real
                    // notification appears.
                    x: (screen_w - width - TOAST_MARGIN).max(0),
                    y: (screen_h - height - TOAST_MARGIN).max(0),
                    topmost: true,
                }
            }

            Content::Note {
                text: body, x, y, size, ..
            } => {
                let style = Style::new(*size as i32);
                let wrap_at = (screen_w as f32 * 0.7) as i32;
                let lines = text.wrap(body, style, wrap_at);
                let widest = lines
                    .iter()
                    .map(|line| text.measure(line, style))
                    .max()
                    .unwrap_or(0);

                let width = (widest + NOTE_PADDING * 2)
                    .clamp(64, (screen_w as f32 * 0.95) as i32)
                    .max(1);
                let height = ((lines.len() as f32 * size * 1.25) as i32 + NOTE_PADDING * 2)
                    .max(*size as i32 * 2)
                    .clamp(1, screen_h);
                let height = height.max(1);

                Geometry {
                    width,
                    height,
                    x: (*x as i32).clamp(0, (screen_w - width).max(0)),
                    y: (*y as i32).clamp(0, (screen_h - height).max(0)),
                    topmost: true,
                }
            }
        }
    }

    /// Paints one frame.
    pub fn draw(
        &self,
        canvas: &mut Canvas,
        text: &mut TextRenderer,
        time: f64,
        seed: u64,
    ) {
        match self {
            Content::Image {
                pixels,
                width,
                height,
                mode,
                ..
            } => draw_image(canvas, pixels, *width, *height, mode),

            Content::Bsod { stopcode, percent } => draw_bsod(canvas, text, stopcode, *percent),

            Content::Update { title, percent } => draw_update(canvas, text, title, *percent),

            Content::Toast { title, body } => draw_toast(canvas, text, title, body),

            Content::Note { text: body, size, .. } => {
                draw_note(canvas, text, body, *size as i32)
            }

            Content::Spook { mode } => draw_spook(canvas, text, mode, time, seed),
        }
    }
}

/// Decodes an image file into the raw RGBA the rasteriser draws.
pub fn load_image(path: &str) -> Result<(Vec<u8>, i32, i32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| format!("{path}: {e}"))?
        .to_rgba8();

    let (width, height) = decoded.dimensions();
    if width == 0 || height == 0 {
        return Err(format!("{path}: image has no dimensions"));
    }

    let decoded = if width > MAX_IMAGE_DIM || height > MAX_IMAGE_DIM {
        let scale = MAX_IMAGE_DIM as f32 / width.max(height) as f32;
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
    Ok((decoded.into_raw(), width as i32, height as i32))
}

// ---------------------------------------------------------------------------
// Per-overlay painters
// ---------------------------------------------------------------------------

/// Toast metrics, shared between sizing the window and painting into it.
const TOAST_PADDING: i32 = 14;
const TOAST_TEXT_WIDTH: i32 = 320;
const TOAST_HEADER: i32 = 40;
const TOAST_LINE_HEIGHT: i32 = 18;
const TOAST_MARGIN: i32 = 16;
const TOAST_TITLE: Style = Style {
    height: 14,
    bold: true,
    monospace: false,
};
const TOAST_BODY: Style = Style {
    height: 13,
    bold: false,
    monospace: false,
};

/// Desktop note metrics.
const NOTE_PADDING: i32 = 12;

const WHITE: Rgba = Rgba::rgb(255, 255, 255);

fn draw_image(canvas: &mut Canvas, pixels: &[u8], width: i32, height: i32, mode: &str) {
    let (screen_w, screen_h) = (canvas.width, canvas.height);
    let target = Rect::new(0, 0, screen_w, screen_h);

    match mode {
        "fullscreen" => {
            // `cover`: fill the viewport and let the overflow crop. More
            // dramatic than letterboxing, which is the point for a scare.
            canvas.blit(pixels, width, height, target, Fit::Cover, 1.0);
        }
        "center" => canvas.blit(pixels, width, height, target, Fit::Exact, 1.0),
        // `fit`: contain, and never upscale past 1:1, so a small image is not
        // blown up into a blurry mess.
        _ => {
            let scale = (screen_w as f32 / width as f32)
                .min(screen_h as f32 / height as f32)
                .min(1.0);
            let draw_w = (width as f32 * scale) as i32;
            let draw_h = (height as f32 * scale) as i32;

            canvas.blit(
                pixels,
                width,
                height,
                Rect::new(
                    (screen_w - draw_w) / 2,
                    (screen_h - draw_h) / 2,
                    draw_w,
                    draw_h,
                ),
                Fit::Exact,
                1.0,
            );
        }
    }
}

fn draw_bsod(canvas: &mut Canvas, text: &mut TextRenderer, stopcode: &str, percent: i64) {
    let (w, h) = (canvas.width, canvas.height);
    let centre_x = w / 2;
    canvas.fill(Rgba::rgb(0, 0, 170));

    // The frown, at roughly the height a real BSOD puts it.
    let frown = Style::new(((h as f32 * 0.15) as i32).min((w as f32 * 0.15) as i32))
        .monospace()
        .bold();
    text.draw_centred(canvas, ":(", centre_x, (h as f32 * 0.26) as i32 - h / 12, frown, WHITE);

    let bar_w = (w as f32 * 0.26) as i32;
    let bar_h = 18;
    let bar_x = centre_x - bar_w / 2;
    let bar_y = (h as f32 * 0.52) as i32;

    canvas.fill_rect(
        Rect::new(bar_x, bar_y, bar_w, bar_h),
        Rgba::with_alpha(255, 255, 255, 45),
    );
    canvas.fill_rect(
        Rect::new(bar_x, bar_y, bar_w * percent.clamp(0, 100) as i32 / 100, bar_h),
        WHITE,
    );

    text.draw_centred(
        canvas,
        &format!("{percent}% complete"),
        centre_x,
        bar_y + bar_h + 22,
        Style::new(17),
        WHITE,
    );

    let body = Style::new(((w as f32 * 0.014) as i32).clamp(12, 22));
    let pad = (w as f32 * 0.06) as i32;
    let left = pad;
    let right = w - pad;
    let bottom = h - pad;
    let line = ((body.height as f32 * 2.0) as i32).max(20);

    // Stacked upwards from the bottom edge, matching the real layout.
    let lines: [&str; 3] = [
        "Your PC ran into a problem and needs to restart. We're just collecting some error info, and then we'll restart for you.",
        "For more information about this error and possible fixes, visit",
        "https://www.windows.com/werr",
    ];

    for (index, content) in lines.iter().enumerate() {
        text.draw(
            canvas,
            content,
            (left, bottom - (lines.len() as i32 - 1 - index as i32) * line - body.height),
            body,
            WHITE,
        );
    }

    text.draw_right(canvas, "100% complete", right, bottom - body.height, body, WHITE);
    text.draw_right(
        canvas,
        &format!("Stop code: {stopcode}"),
        right,
        bottom - 2 * line - body.height,
        body,
        WHITE,
    );
}

fn draw_update(canvas: &mut Canvas, text: &mut TextRenderer, title: &str, percent: i64) {
    let (w, h) = (canvas.width, canvas.height);
    let centre_x = w / 2;

    canvas.fill(Rgba::rgb(24, 32, 48));
    // A lighter band across the upper portion, which is enough to read as "this
    // is a system dialog" rather than a plain coloured screen.
    canvas.fill_rect(
        Rect::new(0, 0, w, (h as f32 * 0.4) as i32),
        Rgba::rgb(32, 48, 74),
    );

    let title_style = Style::new(((w as f32 * 0.026) as i32).clamp(20, 48));
    text.draw_centred(
        canvas,
        title,
        centre_x,
        (h as f32 * 0.30) as i32 - title_style.height / 2,
        title_style,
        WHITE,
    );

    let sub = Style::new(((w as f32 * 0.013) as i32).clamp(12, 22));
    text.draw_centred(
        canvas,
        "Please wait while your computer installs updates",
        centre_x,
        (h as f32 * 0.40) as i32 - sub.height / 2,
        sub,
        Rgba::with_alpha(255, 255, 255, 200),
    );

    let bar_w = (w as f32 * 0.3) as i32;
    let bar_h = 8;
    let bar_x = centre_x - bar_w / 2;
    let bar_y = (h as f32 * 0.52) as i32;

    canvas.fill_rect(
        Rect::new(bar_x, bar_y, bar_w, bar_h),
        Rgba::with_alpha(255, 255, 255, 40),
    );
    canvas.fill_rect(
        Rect::new(
            bar_x,
            bar_y,
            bar_w * percent.clamp(0, 100) as i32 / 100,
            bar_h,
        ),
        Rgba::rgb(0, 120, 215),
    );

    let small = Style::new(((w as f32 * 0.012) as i32).clamp(12, 20));
    text.draw_centred(
        canvas,
        &format!("{percent}% complete"),
        centre_x,
        bar_y + bar_h + 20,
        small,
        Rgba::with_alpha(255, 255, 255, 180),
    );
    text.draw_centred(
        canvas,
        "Do not turn off your computer",
        centre_x,
        h - (h as f32 * 0.12) as i32 - small.height / 2,
        small,
        Rgba::with_alpha(255, 255, 255, 150),
    );
}

fn draw_toast(canvas: &mut Canvas, text: &mut TextRenderer, title: &str, body: &str) {
    let (w, h) = (canvas.width, canvas.height);
    let surface = Rect::new(0, 0, w, h);

    canvas.fill_rect(surface, Rgba::rgb(43, 43, 43));

    // A one-pixel border. Drawn as four thin rectangles because the rasteriser
    // has no stroked primitive, and rounding is not worth the approximation.
    let border = Rgba::with_alpha(255, 255, 255, 40);
    canvas.fill_rect(Rect::new(0, 0, w, 1), border);
    canvas.fill_rect(Rect::new(0, h - 1, w, 1), border);
    canvas.fill_rect(Rect::new(0, 0, 1, h), border);
    canvas.fill_rect(Rect::new(w - 1, 0, 1, h), border);

    text.draw(
        canvas,
        title,
        (TOAST_PADDING, 12),
        TOAST_TITLE,
        Rgba::rgb(120, 180, 255),
    );

    // The window was sized to the wrapped body in `geometry`, so this only
    // truncates if the two disagree, which they should not.
    let mut y = TOAST_HEADER;
    let max_lines = ((h - y - 8) / TOAST_LINE_HEIGHT).max(1) as usize;

    for line in text.wrap(body, TOAST_BODY, TOAST_TEXT_WIDTH).into_iter().take(max_lines) {
        text.draw(
            canvas,
            &line,
            (TOAST_PADDING, y),
            TOAST_BODY,
            Rgba::rgb(235, 235, 235),
        );
        y += TOAST_LINE_HEIGHT;
    }
}

fn draw_note(canvas: &mut Canvas, text_renderer: &mut TextRenderer, body: &str, size: i32) {
    let style = Style::new(size.max(1));
    let lines = text_renderer.wrap(body, style, canvas.width - NOTE_PADDING * 2);
    let step = ((size as f32 * 1.25) as i32).max(1);

    // A transparent window over an arbitrary wallpaper, so a drop shadow is what
    // keeps the text legible.
    for (offset, colour) in [(2, Rgba::with_alpha(0, 0, 0, 190)), (0, Rgba::rgb(255, 236, 120))] {
        let mut y = NOTE_PADDING + offset;
        for line in &lines {
            text_renderer.draw(
                canvas,
                line,
                (NOTE_PADDING + offset, y),
                style,
                colour,
            );
            y += step;
        }
    }
}

fn draw_spook(
    canvas: &mut Canvas,
    text: &mut TextRenderer,
    mode: &str,
    time: f64,
    seed: u64,
) {
    match mode {
        "matrix" => draw_matrix(canvas, text, time, seed),
        "scanline" => draw_scanline(canvas, time),
        "glitch" => draw_glitch(canvas, time, seed),
        // `static` is the default arm, and the only mode with no time input.
        _ => draw_static(canvas, time, seed),
    }
}

fn draw_static(canvas: &mut Canvas, time: f64, seed: u64) {
    let (w, h) = (canvas.width, canvas.height);
    canvas.fill(Rgba::rgb(8, 8, 8));

    let frame = (time * 30.0) as u64;
    let mut rng = Rng::new(seed ^ frame.wrapping_mul(0x9E37_79B9_7F4A_7C15));

    // A couple of thousand rects reads as noise and stays cheap; one rect per
    // pixel would not.
    for _ in 0..2200 {
        let width = rng.range_i32(2, 12);
        let height = rng.range_i32(1, 4);
        let grey = rng.range_i32(20, 256) as u8;

        canvas.fill_rect(
            Rect::new(
                rng.range_i32(0, w),
                rng.range_i32(0, h),
                width,
                height,
            ),
            Rgba::rgb(grey, grey, grey),
        );
    }
}

fn draw_matrix(canvas: &mut Canvas, text: &mut TextRenderer, time: f64, seed: u64) {
    let (w, h) = (canvas.width, canvas.height);
    canvas.fill(Rgba::rgb(0, 8, 0));

    // ASCII only, deliberately: the matrix rain reads as a terminal, and a
    // fixed-pitch face has no CJK coverage to draw with anyway.
    const GLYPHS: &[u8] = b"0123456789ABCDEF$<>#@%&*+=|\\";

    let cell = 20;
    let columns = w.div_euclid(cell);
    let rows = h.div_euclid(cell);
    let tail = 14i64;
    let style = Style::new(17).monospace();

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
            let colour = if offset == 0 {
                Rgba::rgb(200, 255, 200)
            } else {
                Rgba::with_alpha(0, 210, 60, 255 - (offset * 235 / tail) as u8)
            };

            text.draw(
                canvas,
                &glyph.to_string(),
                (column * cell, row as i32 * cell),
                style,
                colour,
            );
        }
    }
}

fn draw_scanline(canvas: &mut Canvas, time: f64) {
    let (w, h) = (canvas.width, canvas.height);
    canvas.fill(Rgba::rgb(6, 10, 6));

    let mut y = 0;
    while y < h {
        canvas.fill_rect(
            Rect::new(0, y, w, 1),
            Rgba::with_alpha(0, 0, 0, 140),
        );
        y += 3;
    }

    // A brighter band sweeping down, which is what sells it as a display fault.
    let span = h as f64 + 200.0;
    let band_y = (time * 260.0 % span) as i32 - 100;

    for offset in 0..3 {
        canvas.fill_rect(
            Rect::new(0, band_y + offset * 2, w, 1),
            Rgba::rgb(120, 255, 160),
        );
    }
}

fn draw_glitch(canvas: &mut Canvas, time: f64, seed: u64) {
    let (w, h) = (canvas.width, canvas.height);
    canvas.fill(Rgba::rgb(10, 0, 14));

    let mut rng = Rng::new(seed ^ (time * 20.0) as u64);

    // Horizontal slices displaced sideways, the classic datamosh look.
    let slices = 40;
    let band = h / slices;

    for index in 0..slices {
        let shift = if rng.f32() > 0.82 {
            rng.range_i32(-90, 90)
        } else {
            0
        };

        let hue = rng.f32();
        canvas.fill_rect(
            Rect::new(shift, index * band, w, band),
            Rgba::with_alpha(
                (hue * 255.0) as u8,
                ((1.0 - hue) * 180.0) as u8,
                (rng.f32() * 200.0) as u8,
                90,
            ),
        );
    }

    // A few solid bright bands on top.
    for _ in 0..6 {
        canvas.fill_rect(
            Rect::new(
                rng.range_i32(-60, 60),
                rng.range_i32(0, h),
                w,
                rng.range_i32(2, 14),
            ),
            Rgba::with_alpha(255, 255, 255, rng.range_i32(40, 151) as u8),
        );
    }
}
