//! Rendering abstraction layer.
use crate::world::{
    camera::Camera,
    geometry::{Level, SubsectorId},
    texture::TextureBank,
};
use minifb::Window;

/// Pixel format of the software frame-buffer (0x00RRGGBB).
pub type Rgba = u32;

pub trait Renderer {
    fn begin_frame(&mut self, w: usize, h: usize);

    fn draw_subsectors(
        &mut self,
        subsectors: &[SubsectorId],
        level: &Level,
        camera: &Camera,
        texture_bank: &TextureBank,
        win: &mut Window,
    );

    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize);
}

pub mod software;
