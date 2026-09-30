//! Text rendering, and how it recovers an alpha channel from GDI.
//!
//! The overlays want the real system font, and GDI is the only way to get it.
//! egui's bundled font is neither available in this backend nor desirable: a
//! convincing fake system dialog has to look like it was drawn by the system.
//!
//! The problem is that GDI does not write the alpha channel of a 32-bit DIB. It
//! blends the glyph against whatever is already in the device context, so text
//! drawn straight onto the overlay would arrive as colour with a meaningless
//! coverage value, and a `WS_EX_LAYERED` window would composite it wrongly.
//!
//! So text goes into a scratch DIB whose background is a sentinel colour no
//! overlay uses, and the result is converted back into a glyph mask. GDI
//! antialiases by mixing the text colour toward the background, so a pixel's
//! distance from the sentinel *is* its coverage - the antialiasing is recovered
//! exactly rather than thresholded to a hard edge.
//!
//! Grayscale antialiasing is requested rather than ClearType on purpose.
//! ClearType renders each subpixel with a different colour, which would both
//! destroy the coverage arithmetic above and leave coloured fringes once the
//! glyph is composited somewhere other than an LCD.

use std::os::windows::ffi::OsStrExt;

use super::api::*;
use super::canvas::{Canvas, Fit, Rect, Rgba};

/// Slack on every side of a run, so a negative side bearing or an antialiased
/// overhang is not clipped out of the scratch surface.
const PAD: i32 = 4;

/// A background colour no painter uses.
///
/// Magenta specifically: the overlays are white, grey and the accent blues, so
/// the channel with the largest distance to the text colour is a strong one and
/// the coverage division below is well conditioned.
const SENTINEL: Rgba = Rgba::rgb(255, 0, 255);

/// The proportional face, used for anything meant to look like a system dialog.
const UI_FACE: &str = "Segoe UI";
/// The fixed-pitch face, used for the matrix rain and the blue screen's frown,
/// where a uniform advance is the whole point.
const MONO_FACE: &str = "Consolas";

/// How a run of text is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// Character cell height in pixels.
    pub height: i32,
    pub bold: bool,
    pub monospace: bool,
}

impl Style {
    pub fn new(height: i32) -> Style {
        Style {
            height,
            bold: false,
            monospace: false,
        }
    }

    pub fn bold(mut self) -> Style {
        self.bold = true;
        self
    }

    pub fn monospace(mut self) -> Style {
        self.monospace = true;
        self
    }
}

/// A TrueType screen font at a cell height, weight and family.
struct Font {
    handle: HGDIOBJ,
    style: Style,
}

impl Font {
    fn new(style: Style) -> Option<Font> {
        let face: Vec<u16> = std::ffi::OsStr::new(if style.monospace {
            MONO_FACE
        } else {
            UI_FACE
        })
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

        let handle = unsafe {
            CreateFontW(
                // A negative height requests a character cell rather than an em
                // size, which is what a pixel-accurate layout wants.
                -style.height.max(1),
                0,
                0,
                0,
                if style.bold { FW_BOLD } else { FW_NORMAL },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_TT_PRECIS,
                CLIP_DEFAULT,
                ANTIALIASED_QUALITY,
                DEFAULT_PITCH | FF_DONTCARE,
                face.as_ptr(),
            )
        };

        if handle.is_null() {
            None
        } else {
            Some(Font { handle, style })
        }
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { DeleteObject(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

/// Renders runs of text into a glyph mask, and composites that mask.
///
/// One instance per overlay window. The scratch surface and the faces are cached
/// and reused, because a `spook` overlay re-renders the same handful of strings
/// twenty times a second and reallocating GDI objects per frame would be both
/// slow and a leak risk.
pub struct TextRenderer {
    scratch: Canvas,
    /// Faces keyed by `[monospace][bold]`, each cached at one height. Two
    /// families and two weights covers every painter here, and alternating
    /// between them at a fixed size is common enough that a single-slot cache
    /// would rebuild the face on most lines.
    fonts: [[Option<Font>; 2]; 2],
    /// Straight-RGBA glyph coverage at one pixel per source pixel.
    mask: Vec<u8>,
    mask_w: i32,
    mask_h: i32,
}

impl TextRenderer {
    /// Allocates the scratch surface, or `None` if GDI refuses a bitmap at all.
    ///
    /// There is no usable fallback for this: without a DIB there is no surface to
    /// render into, so the caller reports it as an overlay that could not open.
    pub fn new() -> Option<TextRenderer> {
        Some(TextRenderer {
            scratch: Canvas::new(64, 32)?,
            fonts: [[None, None], [None, None]],
            mask: Vec::new(),
            mask_w: 0,
            mask_h: 0,
        })
    }

    /// Selects the face for this style, rebuilding it if the cached one is a
    /// different size.
    fn font_for(&mut self, style: Style) -> Option<HGDIOBJ> {
        let column = usize::from(style.monospace);
        let row = usize::from(style.bold);
        let slot = &mut self.fonts[column][row];

        if let Some(font) = slot.as_ref() {
            if font.style.height == style.height {
                return Some(font.handle);
            }
        }

        // The outgoing face may still be selected in the scratch DC, and deleting
        // a selected GDI object is not allowed, so put the stock font back first.
        unsafe { SelectObject(self.scratch.dc(), GetStockObject(DEFAULT_GUI_FONT)) };

        let replacement = Font::new(style)?;
        let handle = replacement.handle;
        *slot = Some(replacement);

        Some(handle)
    }

    /// Advance width of `text` in pixels.
    pub fn measure(&mut self, text: &str, style: Style) -> i32 {
        if text.is_empty() {
            return 0;
        }

        let Some(handle) = self.font_for(style) else {
            return 0;
        };

        let wide: Vec<u16> = text.encode_utf16().collect();
        let dc = self.scratch.dc();

        unsafe {
            SelectObject(dc, handle);
            let mut size = SIZE { cx: 0, cy: 0 };
            if GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut size) == 0 {
                return 0;
            }
            size.cx
        }
    }

    /// Draws `text` with its top-left at `at`, returning true if anything was
    /// composited.
    ///
    /// `at` is the top-left of the run's box, not of the first inked pixel: the
    /// face's leading and left side bearing sit inside it. That is the stable
    /// reference for a layout that positions text against a grid, and it is
    /// close enough to the ink origin that centring by measured width lands
    /// where it should.
    pub fn draw(
        &mut self,
        target: &mut Canvas,
        text: &str,
        at: (i32, i32),
        style: Style,
        colour: Rgba,
    ) -> bool {
        if text.is_empty() {
            return false;
        }

        let Some(handle) = self.font_for(style) else {
            return false;
        };

        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return false;
        }

        // Size the scratch to the run.
        let dc = self.scratch.dc();
        let (extent_w, extent_h) = unsafe {
            SelectObject(dc, handle);
            let mut size = SIZE { cx: 0, cy: 0 };
            let ok = GetTextExtentPoint32W(dc, wide.as_ptr(), wide.len() as i32, &mut size);
            (if ok == 0 { 0 } else { size.cx }, size.cy)
        };

        if extent_w <= 0 || extent_h <= 0 {
            return false;
        }

        let needed_w = extent_w + PAD * 2;
        let needed_h = extent_h + PAD * 2;

        if self.scratch.width < needed_w || self.scratch.height < needed_h {
            self.scratch.release();
            match Canvas::new(needed_w, needed_h) {
                Some(canvas) => self.scratch = canvas,
                None => return false,
            }
        }

        self.render_to_scratch(handle, &wide, colour);
        self.extract_mask(colour);

        // The glyph was drawn `PAD` in from the scratch origin, so the mask is
        // offset back out by the same amount. Negative coordinates are fine:
        // `blit` clips to the canvas.
        target.blit(
            &self.mask,
            self.mask_w,
            self.mask_h,
            Rect::new(at.0 - PAD, at.1 - PAD, self.mask_w, self.mask_h),
            Fit::Exact,
            1.0,
        );

        true
    }

    /// Draws `text` horizontally centred on `centre_x`, with its top at `top`.
    pub fn draw_centred(
        &mut self,
        target: &mut Canvas,
        text: &str,
        centre_x: i32,
        top: i32,
        style: Style,
        colour: Rgba,
    ) {
        let width = self.measure(text, style);
        self.draw(target, text, (centre_x - width / 2, top), style, colour);
    }

    /// Draws `text` with its right edge at `right_x` and its top at `top`.
    pub fn draw_right(
        &mut self,
        target: &mut Canvas,
        text: &str,
        right_x: i32,
        top: i32,
        style: Style,
        colour: Rgba,
    ) {
        let width = self.measure(text, style);
        self.draw(target, text, (right_x - width, top), style, colour);
    }

    /// Draws the run over a cleared sentinel background and lets GDI do the
    /// antialiasing.
    fn render_to_scratch(&mut self, handle: HGDIOBJ, wide: &[u16], colour: Rgba) {
        self.scratch.wipe();
        self.scratch.fill(SENTINEL);

        let dc = self.scratch.dc();
        unsafe {
            SelectObject(dc, handle);
            // An opaque background mode would repaint the whole run's bounding box
            // in the background colour; transparent mode leaves the sentinel fill
            // alone and only blends the glyph into it.
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, to_colorref(colour));
            TextOutW(dc, PAD, PAD, wide.as_ptr(), wide.len() as i32);
        }
    }

    /// Turns the sentinel background into a straight-alpha glyph mask.
    ///
    /// GDI mixed each pixel as `SENTINEL * (1 - c) + colour * c`, so solving for
    /// the coverage on the channel where the two differ most recovers `c`
    /// exactly, antialiasing included.
    fn extract_mask(&mut self, colour: Rgba) {
        let (w, h) = (self.scratch.width, self.scratch.height);
        self.mask_w = w;
        self.mask_h = h;

        let needed = (w * h * 4) as usize;
        if self.mask.len() < needed {
            self.mask.resize(needed, 0);
        }

        // Index of the channel with the longest distance to travel. The DIB is
        // BGRA, so the byte order is the reverse of `Rgba`.
        let sentinel = [SENTINEL.b, SENTINEL.g, SENTINEL.r];
        let target = [colour.b, colour.g, colour.r];

        let mut channel = 0usize;
        let mut span = 0i32;
        for index in 0..3 {
            let distance = (target[index] as i32 - sentinel[index] as i32).abs();
            if distance > span {
                span = distance;
                channel = index;
            }
        }

        // Only reachable if a painter asked for magenta text, which would be
        // invisible against the sentinel anyway. Nothing to recover.
        if span == 0 {
            self.mask[..needed].fill(0);
            return;
        }

        let pixels = self.scratch.pixels();
        let from = sentinel[channel] as i32;

        for index in 0..(w * h) as usize {
            let at = index * 4;
            let value = pixels[at + channel] as i32;

            // Clamped rather than wrapped, so a stray colour outside the expected
            // ramp cannot alias into full coverage somewhere else on the glyph.
            let coverage = ((value - from) * 255 / span).clamp(0, 255) as u8;

            self.mask[at] = colour.b;
            self.mask[at + 1] = colour.g;
            self.mask[at + 2] = colour.r;
            self.mask[at + 3] = coverage;
        }
    }

    /// Greedy word wrap against real measured widths.
    ///
    /// The strings involved are a notification title, a notification body and a
    /// desktop note, so the quadratic cost of re-measuring a candidate line is
    /// irrelevant next to getting the window sized to match what gets drawn.
    pub fn wrap(&mut self, text: &str, style: Style, max_width: i32) -> Vec<String> {
        let max_width = max_width.max(1);
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };

            if current.is_empty() || self.measure(&candidate, style) <= max_width {
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
}

/// GDI stores colours as `0x00BBGGRR`, the reverse of how they are written here.
fn to_colorref(colour: Rgba) -> COLORREF {
    (colour.r as u32) | ((colour.g as u32) << 8) | ((colour.b as u32) << 16)
}
