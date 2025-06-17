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
pub struct WallSpan {
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

/// Horizontal span along one scan-line (y) that shares the same
/// floor/ceiling plane and texture.
#[derive(Clone, Debug)]
pub struct PlaneSpan {
    pub tex_id: TextureId,
    /* perspective-correct UV/z at span edges */
    pub u0_over_z: f32,
    pub v0_over_z: f32,
    pub u1_over_z: f32,
    pub v1_over_z: f32,
    pub inv_z0: f32,
    pub inv_z1: f32,
    /* screen extents */
    pub y: i32,
    pub x_start: i32,
    pub x_end: i32,
    /* is this the floor or the ceiling? */
    pub is_floor: bool,
}

pub enum DrawCall {
    Wall(WallSpan),
    Plane(PlaneSpan),
}

/// A renderer that owns an internal scratch buffer for the whole frame.
///
/// `end_frame` hands the finished buffer to a user-supplied closure.
/// Software callers typically forward it to their window-manager;
/// GPU back-ends can ignore the slice because they never allocate it.
pub trait Renderer {
    /// (Re)allocate internal scratch for the requested resolution and clear it.
    fn begin_frame(&mut self, width: usize, height: usize);

    /// Rasterise one textured wall span into the internal buffer.
    fn draw_wall(&mut self, wall_span: &WallSpan, bank: &TextureBank);

    /// Rasterise one textured plane span into the internal buffer.
    fn draw_plane(&mut self, plane_span: &PlaneSpan, bank: &TextureBank);

    /// Finish the frame and **loan** the finished buffer to `submit`.
    ///
    /// * `submit(&[Rgba], w, h)` is run exactly once per frame.
    /// * Software caller passes `|fb, w, h| window.update_with_buffer(fb, w, h)`.
    /// * GPU back-end simply calls the closure with an empty slice:
    ///   `submit(&[], width, height)`.
    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize);
}

/// Convenience blanket-impl with a one-liner `draw_frame` adaptor.
pub trait RendererExt: Renderer {
    fn draw_frame<F>(
        &mut self,
        width: usize,
        height: usize,
        calls: &[DrawCall],
        bank: &TextureBank,
        submit: F,
    ) where
        F: FnOnce(&[Rgba], usize, usize),
    {
        self.begin_frame(width, height);
        for c in calls {
            match c {
                DrawCall::Wall(w) => self.draw_wall(w, bank),
                DrawCall::Plane(p) => self.draw_plane(p, bank),
            }
        }
        self.end_frame(submit);
    }
}
impl<T: Renderer + ?Sized> RendererExt for T {}

pub mod software;
