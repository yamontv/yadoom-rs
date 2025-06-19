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
                    tex,
                    bands,
                );
            }
            cur.advance(&step);
        }
    }

    fn draw_plane(&mut self, _span: &PlaneSpan, _bands: &ClipBands, _bank: &TextureBank) {
        // TODO: floor / ceiling rendering
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
    tex: &Texture,
    bands: &ClipBands,
) {
    // Clip vertically (inclusive).
    let y_min = cur.y_top.max(bands.ceil[col] as f32).ceil() as i32;
    let y_max = cur.y_bot.min(bands.floor[col] as f32).floor() as i32;
    if y_min > y_max {
        return;
    }

    // Fixed DOOM vertical scaling.
    let col_px_h = (cur.y_bot - cur.y_top).max(1.0);
    let dv_mu = span.wall_h / col_px_h; // map‑units per pixel
    let center_y = fb_h as f32 * 0.5;
    let mut v_mu = span.texturemid_mu + (y_min as f32 - center_y) * dv_mu;

    // Horizontal texture coordinate stays constant in a column.
    let u_tex = ((cur.uoz / cur.inv_z) as i32).rem_euclid(tex.w as i32) as usize;

    for y in y_min..=y_max {
        let v_tex = (v_mu as i32).rem_euclid(tex.h as i32) as usize;
        fb[y as usize * fb_w + col] = tex.pixels[v_tex * tex.w + u_tex];
        v_mu += dv_mu;
    }
}

/*────────────────────────────── Tests ───────────────────────────────────*/
#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::{ClipBands, Rgba};
    use crate::world::texture::{Texture, TextureId};

    /// Create a 1×1 white texture and wrap it in a dummy bank.
    fn single_white_bank() -> TextureBank {
        let tex = Texture {
            w: 1,
            h: 1,
            pixels: vec![0xFFFF_FFFF],
        };
        TextureBank::new(tex)
    }

    /// Column (x = 1) is fully hidden by the clip bands → nothing gets drawn.
    #[test]
    fn column_is_clipped_away() {
        const W: usize = 3;
        const H: usize = 3;

        // empty frame-buffer
        let mut sw = Software::default();
        sw.begin_frame(W, H);

        // clip column 1 completely
        let mut ceil = vec![0, H as i32, 0]; // ceil[1] == H   (below screen)
        let mut floor = vec![H as i32 - 1, -1, H as i32 - 1]; // floor[1] == -1 (above screen)
        let bands = ClipBands {
            ceil: &mut ceil,
            floor: &mut floor,
        };

        let span = WallSpan {
            x_start: 1,
            x_end: 1,
            y_top0: 0.0,
            y_top1: 0.0,
            y_bot0: 2.0,
            y_bot1: 2.0,
            wall_h: 64.0,
            tex_id: NO_TEXTURE,
            ..Default::default()
        };

        let bank = single_white_bank();
        sw.draw_wall(&span, &bands, &bank);

        // frame remains the dark-grey clear colour
        assert!(sw.scratch.iter().all(|&px| px == 0xFF_20_20_20));
    }

    #[test]
    fn step_computation_is_correct() {
        let span = WallSpan {
            x_start: 10,
            x_end: 20, // 10-column span (inclusive)
            u0_over_z: 0.0,
            u1_over_z: 11.0,
            inv_z0: 1.0,
            inv_z1: 2.0,
            y_top0: 0.0,
            y_top1: 11.0,
            y_bot0: 20.0,
            y_bot1: 31.0,
            wall_h: 64.0,
            texturemid_mu: 0.0,
            ..Default::default()
        };

        let step = Step::from_span(&span);
        let eps = 1e-5;

        assert!((step.duoz - 1.1).abs() < eps);
        assert!((step.dinvz - 0.1).abs() < eps);
        assert!((step.dytop - 1.1).abs() < eps);
        assert!((step.dybot - 1.1).abs() < eps);
    }

    #[test]
    fn software_draws_visible_pixel() {
        let mut sw = Software::default();
        const W: usize = 4;
        const H: usize = 4;
        sw.begin_frame(W, H);

        // ── full-open clip bands ───────────────────────────────────────────────
        let mut ceil = vec![0; W];
        let mut floor = vec![H as i32 - 1; W];
        let bands = ClipBands {
            ceil: &mut ceil,
            floor: &mut floor,
        };

        // ── 1×1 white texture bank ────────────────────────────────────────────
        let bank = single_white_bank();

        // Wall that covers exactly column 1, rows 1..=2.
        let span = WallSpan {
            x_start: 1,
            x_end: 1,
            u0_over_z: 0.0,
            u1_over_z: 0.0,
            inv_z0: 1.0,
            inv_z1: 1.0,
            y_top0: 1.0,
            y_top1: 1.0,
            y_bot0: 2.0,
            y_bot1: 2.0,
            wall_h: 64.0,
            texturemid_mu: 0.0,
            ..Default::default()
        };

        sw.draw_wall(&span, &bands, &bank);

        // ── verify ────────────────────────────────────────────────────────────
        let idx = |x, y| y * W + x;
        assert_eq!(sw.scratch[idx(1, 1)], 0xFFFF_FFFF);
        assert_eq!(sw.scratch[idx(1, 2)], 0xFFFF_FFFF);

        // All other pixels should remain the clear colour.
        for (i, &px) in sw.scratch.iter().enumerate() {
            if ![idx(1, 1), idx(1, 2)].contains(&i) {
                assert_eq!(px, 0xFF_20_20_20);
            }
        }
    }
}
