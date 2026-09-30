//! A software rasteriser over a 32-bit DIB section.
//!
//! This is what stands in for egui's `Painter`. The target is the DIB's own
//! memory rather than a copy of it, so a fill is a write through a pointer and
//! presenting a frame costs nothing beyond the `UpdateLayeredWindow` call.
//!
//! Everything except text is drawn by hand. The overlays are solid fills, a
//! progress bar, an optional bitmap and a lot of small procedural noise, for
//! which GDI's own drawing calls are a poor fit — and more to the point, GDI
//! cannot write a meaningful alpha channel into a 32-bit DIB, which is the whole
//! reason this backend exists. Text is the exception; see `text.rs`.

use std::ffi::c_void;

use super::api::*;

/// A straight (non-premultiplied) 8-bit RGBA colour.
#[derive(Clone, Copy, Debug)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba { r, g, b, a: 255 }
    }

    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba { r, g, b, a }
    }
}

/// A rectangle in pixels, top-left origin, positive extent.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

/// How a source bitmap is mapped onto a destination rectangle.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Fill the destination and crop the overflow. What a fullscreen scare wants.
    Cover,
    /// Draw at one source pixel per destination pixel. What a glyph mask wants.
    Exact,
}

/// The drawing target: a DIB section, its memory DC, and a pointer to its bits.
pub struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pub width: i32,
    pub height: i32,
    /// The DIB's own pixels, BGRA and top-down. Valid for as long as the bitmap
    /// is selected into `dc` and has not been deleted, which it is not until
    /// `release`.
    bits: *mut u8,
}

impl Canvas {
    /// Allocates a canvas of the given size, or `None` if GDI refuses.
    pub fn new(width: i32, height: i32) -> Option<Canvas> {
        let width = width.max(1);
        let height = height.max(1);

        // A null `GetDC` yields the screen DC, which is the one `UpdateLayeredWindow`
        // wants as its destination and the only one available before a window exists.
        let screen = unsafe { GetDC(std::ptr::null_mut()) };
        if screen.is_null() {
            return None;
        }

        let dc = unsafe { CreateCompatibleDC(screen) };
        unsafe { ReleaseDC(std::ptr::null_mut(), screen) };
        if dc.is_null() {
            return None;
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as UINT,
                biWidth: width,
                // Negative height requests a top-down DIB, so row 0 is the top
                // of the image and no vertical flip is needed anywhere.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [0; 3],
        };

        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = unsafe {
            CreateDIBSection(
                dc,
                &info,
                DIB_RGB_COLORS as UINT,
                &mut bits,
                std::ptr::null_mut(),
                0,
            )
        };

        if bitmap.is_null() || bits.is_null() {
            unsafe { DeleteDC(dc) };
            return None;
        }

        let previous = unsafe { SelectObject(dc, bitmap) };

        Some(Canvas {
            dc,
            bitmap,
            previous,
            width,
            height,
            bits: bits as *mut u8,
        })
    }

    /// Releases the GDI objects.
    ///
    /// Each overlay window owns a canvas for its lifetime and a `HBITMAP` is a
    /// process-lifetime resource, so dropping the handle without this would leak
    /// one per overlay.
    pub fn release(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                SelectObject(self.dc, self.previous);
                self.previous = std::ptr::null_mut();
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
                self.bitmap = std::ptr::null_mut();
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
                self.dc = std::ptr::null_mut();
            }
        }
    }

    pub fn dc(&self) -> HDC {
        self.dc
    }

    /// The raw BGRA bytes, for the text path to read back out of GDI.
    pub fn pixels(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.bits, self.byte_len()) }
    }

    fn byte_len(&self) -> usize {
        (self.width.max(0) * self.height.max(0) * 4) as usize
    }

    /// Zeroes the surface, making the whole thing transparent.
    ///
    /// Written rather than blended: a `spook` overlay repaints twenty times a
    /// second and this skips the per-pixel blend entirely.
    pub fn wipe(&mut self) {
        if !self.bits.is_null() {
            unsafe { std::ptr::write_bytes(self.bits, 0, self.byte_len()) };
        }
    }

    /// Alpha-blends `colour` over a rectangle, clipped to the canvas.
    pub fn fill_rect(&mut self, rect: Rect, colour: Rgba) {
        if colour.a == 0 || self.bits.is_null() {
            return;
        }

        let x0 = rect.x.max(0);
        let y0 = rect.y.max(0);
        let x1 = rect.right().min(self.width);
        let y1 = rect.bottom().min(self.height);

        if x1 <= x0 || y1 <= y0 {
            return;
        }

        let (sr, sg, sb, sa) = premultiplied(colour);
        // What the destination keeps, since the source claims `sa` of the mix.
        let inv = 255 - sa;

        for y in y0..y1 {
            let row = (y * self.width) as usize * 4;
            for x in x0..x1 {
                let at = row + (x as usize) * 4;
                unsafe {
                    let dst = self.bits.add(at);
                    let dr = *dst as u32;
                    let dg = *dst.add(1) as u32;
                    let db = *dst.add(2) as u32;
                    let da = *dst.add(3) as u32;

                    *dst = (sr + dr * inv / 255).min(255) as u8;
                    *dst.add(1) = (sg + dg * inv / 255).min(255) as u8;
                    *dst.add(2) = (sb + db * inv / 255).min(255) as u8;
                    *dst.add(3) = (sa + da * inv / 255).min(255) as u8;
                }
            }
        }
    }

    pub fn fill(&mut self, colour: Rgba) {
        self.fill_rect(Rect::new(0, 0, self.width, self.height), colour);
    }

    /// Draws a straight-RGBA bitmap into `dst`, alpha-blended and scaled to fit.
    ///
    /// Nearest-neighbour rather than bilinear. These are prank assets, a jump
    /// scare should look harsh, and a bilinear fetch per destination pixel buys
    /// nothing visible at these sizes.
    pub fn blit(
        &mut self,
        source: &[u8],
        source_w: i32,
        source_h: i32,
        dst: Rect,
        fit: Fit,
        opacity: f32,
    ) {
        if source_w <= 0 || source_h <= 0 || dst.w <= 0 || dst.h <= 0 || self.bits.is_null() {
            return;
        }

        let opacity = opacity.clamp(0.0, 1.0);
        if opacity <= 0.0 {
            return;
        }

        let (scale, offset_x, offset_y) = match fit {
            Fit::Exact => (1.0, dst.x as f32, dst.y as f32),
            Fit::Cover => {
                let scale = (dst.w as f32 / source_w as f32).max(dst.h as f32 / source_h as f32);
                (
                    scale,
                    dst.x as f32 + (dst.w as f32 - source_w as f32 * scale) / 2.0,
                    dst.y as f32 + (dst.h as f32 - source_h as f32 * scale) / 2.0,
                )
            }
        };

        let x0 = dst.x.max(0);
        let y0 = dst.y.max(0);
        let x1 = dst.right().min(self.width);
        let y1 = dst.bottom().min(self.height);

        if x1 <= x0 || y1 <= y0 {
            return;
        }

        for y in y0..y1 {
            let sy = (((y as f32 - offset_y) / scale) as i32).clamp(0, source_h - 1);
            let row = (sy as usize) * (source_w as usize) * 4;

            for x in x0..x1 {
                let sx = (((x as f32 - offset_x) / scale) as i32).clamp(0, source_w - 1);
                let index = row + (sx as usize) * 4;

                // Straight alpha, so the source is premultiplied here and then
                // scaled by the overlay opacity in the same step.
                let alpha = ((source[index + 3] as f32 * opacity).round()).clamp(0.0, 255.0) as u32;
                if alpha == 0 {
                    continue;
                }

                let premultiply = |straight: u8| -> u32 { straight as u32 * alpha / 255 };
                let (sr, sg, sb) = (
                    premultiply(source[index]),
                    premultiply(source[index + 1]),
                    premultiply(source[index + 2]),
                );
                let inv = 255 - alpha;

                let at = (y as usize) * (self.width as usize) * 4 + (x as usize) * 4;
                unsafe {
                    let dst_ptr = self.bits.add(at);
                    let dr = *dst_ptr as u32;
                    let dg = *dst_ptr.add(1) as u32;
                    let db = *dst_ptr.add(2) as u32;
                    let da = *dst_ptr.add(3) as u32;

                    *dst_ptr = (sr + dr * inv / 255).min(255) as u8;
                    *dst_ptr.add(1) = (sg + dg * inv / 255).min(255) as u8;
                    *dst_ptr.add(2) = (sb + db * inv / 255).min(255) as u8;
                    *dst_ptr.add(3) = (alpha + da * inv / 255).min(255) as u8;
                }
            }
        }
    }

    /// Hands the frame to the window manager for compositing.
    pub fn present(&self, hwnd: HWND, origin: (i32, i32)) {
        if self.dc.is_null() {
            return;
        }

        let screen = unsafe { GetDC(std::ptr::null_mut()) };
        if screen.is_null() {
            return;
        }

        let destination = POINT {
            x: origin.0,
            y: origin.1,
        };
        let size = SIZE {
            cx: self.width,
            cy: self.height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA,
        };

        unsafe {
            UpdateLayeredWindow(
                hwnd,
                screen,
                &destination,
                &size,
                self.dc,
                &source,
                0,
                &blend,
                ULW_ALPHA,
            );
        }

        unsafe { ReleaseDC(std::ptr::null_mut(), screen) };
    }
}

/// Splits a straight colour into premultiplied components.
fn premultiplied(colour: Rgba) -> (u32, u32, u32, u32) {
    let a = colour.a as u32;
    (
        colour.r as u32 * a / 255,
        colour.g as u32 * a / 255,
        colour.b as u32 * a / 255,
        a,
    )
}
