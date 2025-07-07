//! Rendering abstraction layer.
use crate::{
    sim::TicRunner,
    world::{
        camera::Camera,
        geometry::{Level, SubsectorId},
        texture::TextureBank,
    },
};

/// Pixel format of the software frame-buffer (0x00RRGGBB).
pub type Rgba = u32;

pub trait Renderer {
    fn begin_frame(&mut self, w: usize, h: usize);

    fn draw_level(
        &mut self,
        subsectors: &[SubsectorId],
        level: &Level,
        sim: &TicRunner,
        camera: &Camera,
        texture_bank: &mut TextureBank,
    );

    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32);

    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize);
}

pub mod software;
