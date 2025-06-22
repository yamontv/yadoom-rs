//! ----------------------------------------------------------------------------
//! Classic Doom‑style **software column renderer** (CPU).
//!
//! * Fills an `&mut [u32]` frame‑buffer in **0xAARRGGBB** order.
//! * Relies on the BSP pipeline to feed *already‑clipped* [`WallSpan`]s in
//!   **front‑to‑back** order – therefore no Z‑buffer is needed.
//! * Owns **no visibility state**; it merely respects the supplied [`ClipBands`].
//!
//! ----------------------------------------------------------------------------
use crate::{
    renderer::{ClipBands, PlaneSpan, Renderer, Rgba, WallSpan},
    world::texture::{NO_TEXTURE, Texture, TextureBank},
};

const DIST_FADE_FULL: f32 = 2000.0;

/*───────────────────────────────────────────────────────────────────────────*/
/*                               Backend                                    */
/*───────────────────────────────────────────────────────────────────────────*/

/// Minimal, stateless software renderer that mimics the original DOOM column
/// rasteriser.
#[derive(Debug, Default)]
pub struct Software {
    /// Temporary RGBA back‑buffer.
    scratch: Vec<Rgba>,
    width: usize,
    height: usize,
}

/*─────────────────────────── Renderer trait ───────────────────────────────*/
impl Renderer for Software {
    /// (Re)allocate the scratch buffer and clear it to a dark grey.
    fn begin_frame(&mut self, w: usize, h: usize) {
        if w != self.width || h != self.height {
            self.width = w;
            self.height = h;
            self.scratch.resize(w * h, 0);
        }
        // dark‑grey clear
        self.scratch.fill(0xFF_20_20_20);
    }

    /// Rasterise **one** pre‑clipped wall span.
    fn draw_wall(&mut self, span: &WallSpan, bands: &ClipBands, bank: &TextureBank) {
        debug_assert!(span.x_end < self.width as i32);

        // Resolve texture; fall back to the hard‑coded "missing" patch.
        let tex = bank
            .texture(span.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        // Pre‑compute per‑column linear increments.
        let step = Step::from_span(span);
        let mut cur = Cursor::from_span(span);

        // Walk every column in the span left → right.
        for x in span.x_start..=span.x_end {
            // Skip columns that are fully occluded by earlier geometry.
            if column_visible(x, cur.y_top, cur.y_bot, bands) {
                draw_column(
                    &mut self.scratch,
                    x as usize,
                    self.width,
                    self.height,
                    cur,
                    span,
                    bank,
                    tex,
                    bands,
                );
            }
            cur.advance(&step);
        }
    }

    fn draw_plane(&mut self, span: &PlaneSpan, bank: &TextureBank) {
        let tex = bank
            .texture(span.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        // Linear interpolation across the horizontal run
        let w = (span.x_end - span.x_start).max(1) as f32;
        let duoz = (span.u1_over_z - span.u0_over_z) / w;
        let dvoz = (span.v1_over_z - span.v0_over_z) / w;
        let dinvz = (span.inv_z1 - span.inv_z0) / w;

        let mut uoz = span.u0_over_z;
        let mut voz = span.v0_over_z;
        let mut invz = span.inv_z0;

        let fb_y = span.y as usize;
        let row = &mut self.scratch[fb_y * self.width..][..self.width];

        let z = 1.0 / invz;
        let dist_idx = (z / DIST_FADE_FULL * 31.0).min(31.0) as usize;
        let base_idx = ((255 - span.light) >> 3) as usize; // 0=bright…31=dark
        let shade_idx = (base_idx + dist_idx).min(31) as u8;

        for x in span.x_start..=span.x_end {
            let col = x as usize;

            let u = ((uoz / invz) as i32).rem_euclid(tex.w as i32) as usize;
            let v = ((voz / invz) as i32).rem_euclid(tex.h as i32) as usize;
            row[col] = bank.get_color(shade_idx, tex.pixels[v * tex.w + u]);

            uoz += duoz;
            voz += dvoz;
            invz += dinvz;
        }
    }

    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32) {
        let mut x0 = x0;
        let mut y0 = y0;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if (0..self.width as i32).contains(&x0) && (0..self.height as i32).contains(&y0) {
                self.scratch[y0 as usize * self.width + x0 as usize] = col;
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    /// Hand the finished frame to the caller.
    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize),
    {
        submit(&self.scratch, self.width, self.height);
    }
}

/*──────────────────────── Internal helpers ───────────────────────────────*/

/// Per‑column attributes advance linearly across the span.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    duoz: f32,
    dinvz: f32,
    dytop: f32,
    dybot: f32,
}
impl Step {
    #[inline]
    fn from_span(s: &WallSpan) -> Self {
        let w = (s.x_end - s.x_start).max(1) as f32;
        Self {
            duoz: (s.u1_over_z - s.u0_over_z) / w,
            dinvz: (s.inv_z1 - s.inv_z0) / w,
            dytop: (s.y_top1 - s.y_top0) / w,
            dybot: (s.y_bot1 - s.y_bot0) / w,
        }
    }
}

/// Per‑column cursor that marches from left → right.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cursor {
    uoz: f32,
    inv_z: f32,
    y_top: f32,
    y_bot: f32,
}
impl Cursor {
    #[inline]
    fn from_span(s: &WallSpan) -> Self {
        Self {
            uoz: s.u0_over_z,
            inv_z: s.inv_z0,
            y_top: s.y_top0,
            y_bot: s.y_bot0,
        }
    }

    #[inline(always)]
    fn advance(&mut self, s: &Step) {
        self.uoz += s.duoz;
        self.inv_z += s.dinvz;
        self.y_top += s.dytop;
        self.y_bot += s.dybot;
    }
}

/// Returns `true` when any pixel in column `x` is still inside the free band.
#[inline]
fn column_visible(x: i32, y_top: f32, y_bot: f32, b: &ClipBands) -> bool {
    let col = x as usize;
    y_top < b.floor[col] as f32 && y_bot > b.ceil[col] as f32
}

/// Draw one **visible** vertical column.
#[allow(clippy::too_many_arguments)]
fn draw_column(
    fb: &mut [Rgba],
    col: usize,
    fb_w: usize,
    fb_h: usize,
    cur: Cursor,
    span: &WallSpan,
    bank: &TextureBank,
    tex: &Texture,
    bands: &ClipBands,
) {
    if bands.ceil[col] > bands.floor[col] {
        return;
    }

    // Clip vertically (inclusive).
    let y_min = (cur.y_top.max((bands.ceil[col] + 1) as f32).ceil() as i32).max(0);
    let y_max = (cur.y_bot.min((bands.floor[col] - 1) as f32).floor() as i32).min(fb_h as i32 - 1);
    if y_min >= y_max {
        return;
    }

    // Fixed DOOM vertical scaling.
    let col_px_h = (cur.y_bot - cur.y_top).max(1.0);
    let dv_mu = span.wall_h / col_px_h; // map‑units per pixel
    let center_y = fb_h as f32 * 0.5;
    let mut v_mu = span.texturemid_mu + (y_min as f32 - center_y) * dv_mu;

    // Horizontal texture coordinate stays constant in a column.
    let u_tex = ((cur.uoz / cur.inv_z) as i32).rem_euclid(tex.w as i32) as usize;

    let z = 1.0 / cur.inv_z;
    let dist_idx = (z / DIST_FADE_FULL * 31.0).min(31.0) as usize;
    let base_idx = ((255 - span.light) >> 3) as usize; // 0=bright…31=dark
    let shade_idx = (base_idx + dist_idx).min(31) as u8;

    for y in y_min..=y_max {
        let v_tex = (v_mu as i32).rem_euclid(tex.h as i32) as usize;
        fb[y as usize * fb_w + col] = bank.get_color(shade_idx, tex.pixels[v_tex * tex.w + u_tex]);
        v_mu += dv_mu;
    }
}
