//! Rendering abstraction layer.
//!
//! *The rest of the engine never touches a pixel buffer directly.*
//! It produces a list of [`DrawCall`]s (front-to-back) and hands them to a
//! type that implements [`Renderer`].
//!
//! * You can plug multiple back-ends (`renderer::sw`, `renderer::gl`, …)
//!   without changing game logic.
//! * A helper blanket‐impl [`RendererExt`] adds `draw_frame` so call-sites
//!   stay short.
//!
//! **Current limitation**: only textured walls.  Flats, sprites, dynamic
//! lights will extend [`DrawCall`] later.

use crate::world::texture::{TextureBank, TextureId};

/// Pixel format of the software frame-buffer (0x00RRGGBB).
pub type Rgba = u32;

/// Tells the draw routine whether this is a solid wall slice,
/// an upper-portal slice (ceiling of back sector), or a lower-portal slice.
#[derive(Copy, Clone, Debug)]
pub enum ClipKind {
    Solid,
    Upper,
    Lower,
}

/// Non-clipped information for one vertical wall slice batch.
/// `x_start ..= x_end` maps to screen columns.
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
    pub y_top0: f32, // ceiling at x_start
    pub y_top1: f32, // ceiling at x_end
    pub y_bot0: f32, // floor   at x_start
    pub y_bot1: f32, // floor   at x_end

    pub kind: ClipKind,

    pub wall_h: f32,        // ceiling_z - floor_z in map units
    pub texturemid_mu: f32, // (ceil_h − eyeZ) + y_off     in map units
}

/// Backend-agnostic rendering interface.
pub trait Renderer {
    /// Allocate / reuse internal scratch buffer and reset state.
    fn begin_frame(&mut self, width: usize, height: usize);

    /// Rasterise one wall-span into the **internal** buffer.
    fn draw_wall(&mut self, call: &DrawCall, textures: &TextureBank);

    /// Finish the frame.
    ///
    /// * **software back-ends** expect `target = Some(fb)` and must copy the
    ///   scratch buffer into that slice.
    /// * **GPU back-ends** receive `None` and simply present / swap buffers.
    fn end_frame(&mut self, target: Option<&mut [Rgba]>);
}

/// Convenience blanket-impl with a one-liner `draw_frame` adaptor.
pub trait RendererExt: Renderer {
    fn draw_frame(
        &mut self,
        target: &mut [Rgba],
        w: usize,
        h: usize,
        calls: &[DrawCall],
        bank: &TextureBank,
    ) {
        self.begin_frame(w, h);
        for dc in calls {
            self.draw_wall(dc, bank);
        }
        self.end_frame(Some(target));
    }
}
impl<T: Renderer + ?Sized> RendererExt for T {}

/// Stub backend that does nothing – handy for headless tests.
#[derive(Default)]
pub struct Dummy;
impl Renderer for Dummy {
    fn begin_frame(&mut self, _w: usize, _h: usize) {}
    fn draw_wall(&mut self, _c: &DrawCall, _tex: &TextureBank) {}
    fn end_frame(&mut self, _tgt: Option<&mut [Rgba]>) {}
}

pub mod software;
