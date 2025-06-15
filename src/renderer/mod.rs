//! Rendering abstraction layer – the game never touches a pixel buffer
//! directly.  It produces a list of [`DrawCall`]s and hands them to an
//! object that implements [`Renderer`].

use crate::world::texture::{TextureBank, TextureId};

/// 0x00RRGGBB pixel.
pub type Rgba = u32;

/*──────────────────────────── DrawCall ───────────────────────────────*/
/// One vertical wall-slice batch (front-to-back order).
#[derive(Clone, Debug)]
pub struct DrawCall {
    pub tex_id: TextureId,
    /* perspective-correct texture coords (already divided by z) */
    pub u0_over_z: f32,
    pub u1_over_z: f32,
    pub inv_z0: f32,
    pub inv_z1: f32,
    /* screen extents */
    pub x_start: i32,
    pub x_end: i32,
    pub y_top0: f32,
    pub y_top1: f32,
    pub y_bot0: f32,
    pub y_bot1: f32,
}

/*──────────────────────────── Renderer trait ─────────────────────────*/
pub trait Renderer {
    fn begin_frame(&mut self, target: Option<&mut [Rgba]>, w: usize, h: usize);
    fn draw_wall(&mut self, dc: &DrawCall, textures: &TextureBank);
    fn end_frame(&mut self);
}

/* blanket helper */
pub trait RendererExt: Renderer {
    fn draw_frame(
        &mut self,
        fb: &mut [Rgba],
        w: usize,
        h: usize,
        calls: &[DrawCall],
        tex: &TextureBank,
    ) {
        self.begin_frame(Some(fb), w, h);
        for c in calls {
            self.draw_wall(c, tex);
        }
        self.end_frame();
    }
}
impl<T: Renderer + ?Sized> RendererExt for T {}

/*──────────────────────────── Dummy backend ──────────────────────────*/
#[derive(Default)]
pub struct Dummy;
impl Renderer for Dummy {
    fn begin_frame(&mut self, _: Option<&mut [Rgba]>, _: usize, _: usize) {}
    fn draw_wall(&mut self, _: &DrawCall, _: &TextureBank) {}
    fn end_frame(&mut self) {}
}

/*──────────────────────────── Software backend ───────────────────────*/
pub mod software {
    use super::*;

    /// Tiny Doom-style column renderer (fills an `&mut [u32]` FB).
    pub struct Software {
        fb: *mut Rgba,
        len: usize,
        w: usize,
        h: usize,
    }
    impl Default for Software {
        fn default() -> Self {
            Self {
                fb: core::ptr::null_mut(),
                len: 0,
                w: 0,
                h: 0,
            }
        }
    }

    impl Renderer for Software {
        fn begin_frame(&mut self, tgt: Option<&mut [Rgba]>, w: usize, h: usize) {
            let buf = tgt.expect("Software backend needs a CPU frame-buffer");
            self.fb = buf.as_mut_ptr();
            self.len = buf.len();
            self.w = w;
            self.h = h;
            buf.fill(0x00202020); // dark-grey clear
        }

        fn draw_wall(&mut self, c: &DrawCall, bank: &TextureBank) {
            let Ok(tex) = bank.texture(c.tex_id) else {
                return;
            };
            let fb = unsafe { std::slice::from_raw_parts_mut(self.fb, self.len) };

            let span = (c.x_end - c.x_start).max(1) as f32;
            let duoz = (c.u1_over_z - c.u0_over_z) / span;
            let dinvz = (c.inv_z1 - c.inv_z0) / span;
            let dty = (c.y_top1 - c.y_top0) / span;
            let dby = (c.y_bot1 - c.y_bot0) / span;

            let mut uoz = c.u0_over_z;
            let mut invz = c.inv_z0;
            let mut ty = c.y_top0;
            let mut by = c.y_bot0;

            for x in c.x_start..=c.x_end {
                if x < 0 || x >= self.w as i32 {
                    // X-clip
                    uoz += duoz;
                    invz += dinvz;
                    ty += dty;
                    by += dby;
                    continue;
                }
                let u = ((uoz / invz) as i32).rem_euclid(tex.w as i32) as usize;

                let y0 = ty.max(0.0) as i32;
                let y1 = by.min(self.h as f32 - 1.0) as i32;
                if y0 < y1 {
                    let col = x as usize;
                    let wall_h = (by - ty).max(1.0);
                    for y in y0..=y1 {
                        let v = (((y as f32 - ty) / wall_h) * tex.h as f32) as usize % tex.h;
                        fb[y as usize * self.w + col] = tex.pixels[v * tex.w + u];
                    }
                }
                uoz += duoz;
                invz += dinvz;
                ty += dty;
                by += dby;
            }
        }
        fn end_frame(&mut self) {}
    }
}
