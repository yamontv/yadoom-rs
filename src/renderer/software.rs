//! ---------------------------------------------------------------------------
//! Classic software (CPU) column renderer
//!
//! * Fills an `&mut [u32]` frame-buffer in **0xAARRGGBB** format.
//! * Relies on the BSP pipeline to feed *front-to-back* [`WallSpan`]s, so no
//!   Z-buffer is needed.
//! ---------------------------------------------------------------------------

use crate::{
    renderer::{ClipBands, PlaneSpan, Renderer, Rgba, WallSpan},
    world::texture::{NO_TEXTURE, TextureBank},
};

/*───────────────────────────────────────────────────────────────────────*/
/*                              Backend                                 */
/*───────────────────────────────────────────────────────────────────────*/

/// Doom-style column renderer.
pub struct Software {
    scratch: Vec<Rgba>,
    width: usize,
    height: usize,
}

impl Default for Software {
    fn default() -> Self {
        Self {
            scratch: Vec::new(),
            width: 0,
            height: 0,
        }
    }
}

/*──────────────────────── Renderer trait impl ────────────────────────*/
impl Renderer for Software {
    fn begin_frame(&mut self, w: usize, h: usize) {
        // (re)allocate if resolution changed
        if w != self.width || h != self.height {
            debug_assert!(w * h != 0);
            self.width = w;
            self.height = h;
            self.scratch.resize(w * h, 0);
        }

        /* dark-grey clear */
        self.scratch.fill(0xFF_202020);
    }

    fn draw_wall(&mut self, wall_span: &WallSpan, bands: &ClipBands, bank: &TextureBank) {
        debug_assert!(wall_span.x_end < self.width as i32);

        let tex = bank
            .texture(wall_span.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        /* pre-compute per-column linear increments ----------------------------*/
        let step = ColumnStep::from_drawcall(wall_span);

        /* cursor that will walk across the wall strip */
        let mut cur = ColumnCursor::from_drawcall(wall_span);

        /* render every vertical column in the span ---------------------------*/
        for x in wall_span.x_start..=wall_span.x_end {
            if self.column_visible(x, cur.y_top, cur.y_bot, bands) {
                self.draw_column(x, cur, wall_span, tex, bands);
            }
            cur.advance(&step);
        }
    }

    fn draw_plane(&mut self, _span: &PlaneSpan, _bands: &ClipBands, _bank: &TextureBank) {
        // TODO
    }

    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize),
    {
        submit(&self.scratch, self.width, self.height);
    }
}

/*──────────────────────── helper structs ─────────────────────────────*/

/// Per-column attributes that advance linearly across the strip.
#[derive(Clone, Copy)]
struct ColumnStep {
    duoz: f32,
    dinvz: f32,
    dytop: f32,
    dybot: f32,
}
impl ColumnStep {
    fn from_drawcall(wall_span: &WallSpan) -> Self {
        let span_w = (wall_span.x_end - wall_span.x_start).max(1) as f32;
        Self {
            duoz: (wall_span.u1_over_z - wall_span.u0_over_z) / span_w,
            dinvz: (wall_span.inv_z1 - wall_span.inv_z0) / span_w,
            dytop: (wall_span.y_top1 - wall_span.y_top0) / span_w,
            dybot: (wall_span.y_bot1 - wall_span.y_bot0) / span_w,
        }
    }
}

/// Current per-column parameters that march from left to right.
#[derive(Clone, Copy)]
struct ColumnCursor {
    uoz: f32,
    inv_z: f32,
    y_top: f32,
    y_bot: f32,
}
impl ColumnCursor {
    fn from_drawcall(wall_span: &WallSpan) -> Self {
        Self {
            uoz: wall_span.u0_over_z,
            inv_z: wall_span.inv_z0,
            y_top: wall_span.y_top0,
            y_bot: wall_span.y_bot0,
        }
    }

    #[inline(always)]
    fn advance(&mut self, s: &ColumnStep) {
        self.uoz += s.duoz;
        self.inv_z += s.dinvz;
        self.y_top += s.dytop;
        self.y_bot += s.dybot;
    }
}

/*──────────────────────── column rendering ───────────────────────────*/

impl Software {
    /// True if any part of this column is within the current clip bands.
    fn column_visible(&self, x: i32, y_top: f32, y_bot: f32, bands: &ClipBands) -> bool {
        let top_band = bands.ceil[x as usize] as f32;
        let bot_band = bands.floor[x as usize] as f32;
        y_top < bot_band && y_bot > top_band
    }

    /// Draw a single vertical slice (one screen column).
    fn draw_column(
        &mut self,
        x: i32,
        cur: ColumnCursor,
        wall_span: &WallSpan,
        tex: &crate::world::texture::Texture,
        bands: &ClipBands,
    ) {
        /* clip to integer pixel rows */
        let col = x as usize;
        let y0 = cur.y_top.max(bands.ceil[col] as f32) as i32;
        let y1 = cur.y_bot.min(bands.floor[col] as f32) as i32;
        if y0 > y1 {
            return;
        }

        /* fixed Doom tiling ---------------------------------------------------*/
        let col_h_px = (cur.y_bot - cur.y_top).max(1.0);
        let step_v = wall_span.wall_h / col_h_px; // map-units / px
        let center_y = self.height as f32 * 0.5;
        let mut v_mu = wall_span.texturemid_mu + (y0 as f32 - center_y) * step_v;

        let u_tex = ((cur.uoz / cur.inv_z) as i32).rem_euclid(tex.w as i32) as usize;

        for y in y0..=y1 {
            let v_tex = (v_mu as i32).rem_euclid(tex.h as i32) as usize;
            self.scratch[y as usize * self.width + col] = tex.pixels[v_tex * tex.w + u_tex];
            v_mu += step_v;
        }
    }
}
