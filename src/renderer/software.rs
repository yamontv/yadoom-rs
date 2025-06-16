//! ---------------------------------------------------------------------------
//! Classic software (CPU) column renderer
//!
//! * Fills an `&mut [u32]` frame-buffer in **0xAARRGGBB** format.
//! * Relies on the BSP pipeline to feed *front-to-back* [`DrawCall`]s, so no
//!   Z-buffer is needed.
//!
//! Safety: the only `unsafe` block reconstructs an `&mut [u32]` slice from the
//! raw pointer cached in `begin_frame` – the pointer’s lifetime is bounded by
//! `begin_frame`/`end_frame`.
//! ---------------------------------------------------------------------------

use crate::{
    renderer::{ClipKind, DrawCall, Renderer, Rgba},
    world::texture::{NO_TEXTURE, TextureBank},
};

/*───────────────────────────────────────────────────────────────────────*/
/*                              Backend                                 */
/*───────────────────────────────────────────────────────────────────────*/

/// Doom-style column renderer.
pub struct Software {
    scratch: Vec<Rgba>,
    /* clip bands survive across columns */
    ceil_clip: Vec<i16>,
    floor_clip: Vec<i16>,
    width: usize,
    height: usize,
}

impl Default for Software {
    fn default() -> Self {
        Self {
            scratch: Vec::new(),
            ceil_clip: Vec::new(),
            floor_clip: Vec::new(),
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
            self.width = w;
            self.height = h;
            self.scratch.resize(w * h, 0);
            self.ceil_clip.resize(w, 0);
            self.floor_clip.resize(w, h as i16 - 1);
        }

        /* dark-grey clear */
        self.scratch.fill(0xFF_202020);

        /* reset per-column clip ranges */
        self.ceil_clip.fill(0);
        self.floor_clip.fill(self.height as i16 - 1);
    }

    fn draw_wall(&mut self, dc: &DrawCall, bank: &TextureBank) {
        let tex = bank
            .texture(dc.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        /* pre-compute per-column linear increments ----------------------------*/
        let span_w = (dc.x_end - dc.x_start).max(1) as f32;
        let step = ColumnStep::from_drawcall(dc, span_w);

        /* cursor that will walk across the wall strip */
        let mut cur = ColumnCursor::from_drawcall(dc);

        /* render every vertical column in the span ---------------------------*/
        for x in dc.x_start..=dc.x_end {
            if self.column_visible(x, cur.y_top, cur.y_bot) {
                self.draw_column(x, cur, dc, tex);
            }
            cur.advance(&step);
        }
    }

    fn end_frame(&mut self, target: Option<&mut [Rgba]>) {
        if let Some(dst) = target {
            debug_assert_eq!(dst.len(), self.scratch.len());
            dst.copy_from_slice(&self.scratch); // one fast memcpy
        }
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
    fn from_drawcall(dc: &DrawCall, span_w: f32) -> Self {
        Self {
            duoz: (dc.u1_over_z - dc.u0_over_z) / span_w,
            dinvz: (dc.inv_z1 - dc.inv_z0) / span_w,
            dytop: (dc.y_top1 - dc.y_top0) / span_w,
            dybot: (dc.y_bot1 - dc.y_bot0) / span_w,
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
    fn from_drawcall(dc: &DrawCall) -> Self {
        Self {
            uoz: dc.u0_over_z,
            inv_z: dc.inv_z0,
            y_top: dc.y_top0,
            y_bot: dc.y_bot0,
        }
    }
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
    fn column_visible(&self, x: i32, y_top: f32, y_bot: f32) -> bool {
        let top_band = self.ceil_clip[x as usize] as f32;
        let bot_band = self.floor_clip[x as usize] as f32;
        y_top < bot_band && y_bot > top_band
    }

    /// Draw a single vertical slice (one screen column).
    fn draw_column(
        &mut self,
        x: i32,
        cur: ColumnCursor,
        dc: &DrawCall,
        tex: &crate::world::texture::Texture,
    ) {
        /* clip to integer pixel rows */
        let col = x as usize;
        let y0 = cur.y_top.max(self.ceil_clip[col] as f32) as i32;
        let y1 = cur.y_bot.min(self.floor_clip[col] as f32) as i32;
        if y0 > y1 {
            return;
        }

        /* fixed Doom tiling ---------------------------------------------------*/
        let col_h_px = (cur.y_bot - cur.y_top).max(1.0);
        let step_v = dc.wall_h / col_h_px; // map-units / px
        let center_y = self.height as f32 * 0.5;
        let mut v_mu = dc.texturemid_mu + (y0 as f32 - center_y) * step_v;

        let u_tex = ((cur.uoz / cur.inv_z) as i32).rem_euclid(tex.w as i32) as usize;

        for y in y0..=y1 {
            let v_tex = (v_mu as i32).rem_euclid(tex.h as i32) as usize;
            self.scratch[y as usize * self.width + col] = tex.pixels[v_tex * tex.w + u_tex];
            v_mu += step_v;
        }

        /* update clip bands so farther geometry is culled */
        match dc.kind {
            ClipKind::Solid => {
                self.ceil_clip[col] = (y1 as i16).saturating_add(1);
                self.floor_clip[col] = (y0 as i16).saturating_sub(1);
            }
            ClipKind::Upper => self.ceil_clip[col] = (y1 as i16).saturating_add(1),
            ClipKind::Lower => self.floor_clip[col] = (y0 as i16).saturating_sub(1),
        }
    }
}

/*──────────────────────────────── Tests ───────────────────────────────*/
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        renderer::RendererExt,
        world::texture::{Texture, TextureBank},
    };

    /* tiny helpers ---------------------------------------------------*/
    fn tiny_bank() -> TextureBank {
        let mut bank = TextureBank::default_with_checker();
        bank.insert(
            "BLUE",
            Texture {
                w: 4,
                h: 4,
                pixels: vec![0xFF_0000FF; 16],
            },
        )
        .unwrap();
        bank
    }
    fn blue_span() -> DrawCall {
        DrawCall {
            tex_id: 1,
            u0_over_z: 0.0,
            u1_over_z: 1.0,
            inv_z0: 1.0,
            inv_z1: 1.0,
            x_start: 1,
            x_end: 2,
            y_top0: 1.0,
            y_top1: 1.0,
            y_bot0: 4.0,
            y_bot1: 4.0,
            kind: ClipKind::Lower,
            texturemid_mu: 0.0,
            wall_h: 10.0,
        }
    }

    #[test]
    fn software_renders_span() {
        let mut fb = vec![0; 8 * 8];
        let bank = tiny_bank();
        let mut sw = Software::default();

        sw.draw_frame(&mut fb, 8, 8, &[blue_span()], &bank);

        assert!(
            fb.iter().any(|&px| px == 0xFF_0000FF),
            "renderer failed to write any blue pixels"
        );
    }
}
