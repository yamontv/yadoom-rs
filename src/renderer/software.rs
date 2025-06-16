//! ---------------------------------------------------------------------------
//! Software (CPU) backend implementing [`Renderer`].
//!
//! * Fills an `&mut [u32]` frame-buffer in 0xAARRGGBB format.
//! * Relies on the caller (pipeline) to supply `DrawCall`s **front-to-back**,
//!   so no Z-buffer is required.
//!
//! Safety: the only `unsafe` is re-building the slice from the raw pointer
//! whose lifetime is bounded by `begin_frame`/`end_frame`.
//! ---------------------------------------------------------------------------
use crate::{
    renderer::{ClipKind, DrawCall, Renderer, Rgba},
    world::texture::{NO_TEXTURE, TextureBank},
};

/// Classic column renderer (Doom-style).
///
/// Typical use:
/// ```ignore
/// sw.begin_frame(Some(&mut frame), W, H);
/// for dc in &drawcalls { sw.draw_wall(dc, &tex_bank); }
/// sw.end_frame();
/// ```
pub struct Software {
    fb_ptr: *mut Rgba, // raw so we can store between calls
    fb_len: usize,
    width: usize,
    height: usize,

    // per-column clip bands, re-used each frame
    ceil_clip: Vec<i16>,
    floor_clip: Vec<i16>,
}

impl Default for Software {
    fn default() -> Self {
        Self {
            fb_ptr: core::ptr::null_mut(),
            fb_len: 0,
            width: 0,
            height: 0,
            ceil_clip: Vec::new(),
            floor_clip: Vec::new(),
        }
    }
}

/*=====================================================================*/
/*                         Renderer impl                               */
/*=====================================================================*/
impl Renderer for Software {
    fn begin_frame(&mut self, target: Option<&mut [Rgba]>, w: usize, h: usize) {
        let buf = target.expect("Software backend needs a CPU frame-buffer");
        self.fb_ptr = buf.as_mut_ptr();
        self.fb_len = buf.len();
        self.width = w;
        self.height = h;
        buf.fill(0xFF_202020); // dark-grey clear

        // reset clip bands each frame
        self.ceil_clip.clear();
        self.ceil_clip.resize(w, 0);
        self.floor_clip.clear();
        self.floor_clip.resize(w, h as i16 - 1);
    }

    fn draw_wall(&mut self, dc: &DrawCall, bank: &TextureBank) {
        // Safety: ptr/len originate from slice given to begin_frame
        let fb: &mut [u32] = unsafe { core::slice::from_raw_parts_mut(self.fb_ptr, self.fb_len) };

        let w = self.width;

        // one texture lookup per call, with checker fallback
        let tex = bank
            .texture(dc.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        /*--------- per-column increments (pre-divide) ----------------*/
        let span_w = (dc.x_end - dc.x_start).max(1) as f32;
        let duoz = (dc.u1_over_z - dc.u0_over_z) / span_w;
        let dinvz = (dc.inv_z1 - dc.inv_z0) / span_w;
        let dytop = (dc.y_top1 - dc.y_top0) / span_w;
        let dybot = (dc.y_bot1 - dc.y_bot0) / span_w;

        let mut uoz = dc.u0_over_z;
        let mut inv_z = dc.inv_z0;
        let mut y_top = dc.y_top0;
        let mut y_bot = dc.y_bot0;

        for x in dc.x_start..=dc.x_end {
            if let Some(&col_clip_top) = self.ceil_clip.get(x as usize) {
                if let Some(&col_clip_bot) = self.floor_clip.get(x as usize) {
                    // compute this column's unclipped Y
                    let yt = y_top;
                    let yb = y_bot;

                    // test against current clip bands
                    if yt < col_clip_bot as f32 && yb > col_clip_top as f32 {
                        // clip into integer pixel rows
                        let y0 = yt.max(col_clip_top as f32) as i32;
                        let y1 = yb.min(col_clip_bot as f32) as i32;

                        // draw vertical slice
                        let col = x as usize;

                        /*------------------------------------------------------
                         * True Doom tiling:
                         * v_world      – distance in map units from wall top
                         * step_v       – map-units advanced per screen pixel
                         * tex_v        – (v_world + y_off)  mod texture_h
                         *-----------------------------------------------------*/

                        let col_h_px = (y_bot - y_top).max(1.0); // pixels
                        let step_v = dc.wall_h / col_h_px; // map-units per pixel
                        // Doom: start from one common “texturemid”
                        let center_y = self.height as f32 * 0.5;
                        let mut v_w = dc.texturemid_mu + (y0 as f32 - center_y) * step_v; // shared origin

                        let tex_w_i32 = tex.w as i32;
                        let tex_h_i32 = tex.h as i32;

                        /* horizontal coordinate is constant inside this column */
                        let u = ((uoz / inv_z) as i32).rem_euclid(tex_w_i32) as usize;

                        for y in y0..=y1 {
                            // texture row: (world offset + sidedef y_off) mod tex.h
                            let v_tex = (v_w as i32).rem_euclid(tex_h_i32) as usize;

                            fb[y as usize * w + col] = tex.pixels[v_tex * tex.w + u];
                            v_w += step_v; // advance one pixel
                        }

                        // now update the clip bands for this column
                        match dc.kind {
                            ClipKind::Solid => {
                                self.ceil_clip[col] = (y1 as i16).saturating_add(1);
                                self.floor_clip[col] = (y0 as i16).saturating_sub(1);
                            }
                            ClipKind::Upper => {
                                self.ceil_clip[col] = (y1 as i16).saturating_add(1);
                            }
                            ClipKind::Lower => {
                                self.floor_clip[col] = (y0 as i16).saturating_sub(1);
                            }
                        }
                    }
                }
            }

            // next column
            uoz += duoz;
            inv_z += dinvz;
            y_top += dytop;
            y_bot += dybot;
        }
    }

    fn end_frame(&mut self) {
        // pure software: nothing to flush
    }
}

/*======================================================================*/
/*                               Tests                                  */
/*======================================================================*/
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        renderer::RendererExt,
        world::texture::{Texture, TextureBank},
    };

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
        assert!(fb.iter().any(|&p| p == 0xFF_0000FF));
    }
}
