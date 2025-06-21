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

use crate::world::texture::{NO_TEXTURE, TextureBank, TextureId};

/// Pixel format of the software frame-buffer (0x00RRGGBB).
pub type Rgba = u32;

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

    pub wall_h: f32,        // ceiling_z - floor_z in map units
    pub texturemid_mu: f32, // (ceil_h − eyeZ) + y_off     in map units
}
impl Default for WallSpan {
    fn default() -> Self {
        Self {
            x_start: 0,
            x_end: 0,
            u0_over_z: 0.0,
            u1_over_z: 0.0,
            inv_z0: 1.0,
            inv_z1: 1.0,
            y_top0: 0.0,
            y_top1: 0.0,
            y_bot0: 0.0,
            y_bot1: 0.0,
            wall_h: 64.0,
            texturemid_mu: 0.0,
            tex_id: NO_TEXTURE,
        }
    }
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
    pub y: u16,
    pub x_start: u16,
    pub x_end: u16,
}

/// Reference to the shared front-to-back clip state.
/// Software back-ends read it, GPU ones will ignore it.
pub struct ClipBands {
    pub ceil: Vec<i16>,
    pub floor: Vec<i16>,
}

/// One-direction streaming renderer.
pub trait Renderer {
    /// Allocate/clear internal buffers.
    fn begin_frame(&mut self, w: usize, h: usize);

    /// Rasterise a (already-clipped) wall span.
    fn draw_wall(&mut self, span: &WallSpan, bands: &ClipBands, bank: &TextureBank);

    /// Rasterise a floor/ceiling span (future work – unchanged idea).
    fn draw_plane(&mut self, span: &PlaneSpan, bank: &TextureBank);

    /// for debug
    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32);

    /// Present the finished frame.
    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize);
}

pub mod software;
