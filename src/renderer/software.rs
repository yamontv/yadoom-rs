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
    renderer::{DrawCall, Renderer, Rgba},
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
}

impl Default for Software {
    fn default() -> Self {
        Self {
            fb_ptr: core::ptr::null_mut(),
            fb_len: 0,
            width: 0,
            height: 0,
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
    }

    fn draw_wall(&mut self, dc: &DrawCall, bank: &TextureBank) {
        // Safety: ptr/len originate from slice given to begin_frame
        let fb: &mut [u32] = unsafe { core::slice::from_raw_parts_mut(self.fb_ptr, self.fb_len) };

        let w = self.width;
        let h = self.height as f32;

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
            if (0..w as i32).contains(&x) {
                let col = x as usize;
                let u = ((uoz / inv_z) as i32).rem_euclid(tex.w as i32) as usize;

                let y0 = y_top.max(0.0) as i32;
                let y1 = y_bot.min(h - 1.0) as i32;
                if y0 < y1 {
                    let wall_h = (y_bot - y_top).max(1.0);
                    for y in y0..=y1 {
                        let frac = (y as f32 - y_top) / wall_h;
                        let v = ((frac * tex.h as f32) as i32).rem_euclid(tex.h as i32) as usize;
                        fb[y as usize * w + col] = tex.pixels[v * tex.w + u];
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
