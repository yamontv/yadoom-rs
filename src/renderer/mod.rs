//! Rendering abstraction layer.
use glam::Vec2;

use crate::world::{
    camera::Camera,
    texture::{TextureBank, TextureId},
};

/// Pixel format of the software frame-buffer (0x00RRGGBB).
pub type Rgba = u32;

#[derive(Clone, Debug, Default)]
pub struct SectorCS {
    pub floor_h: f32,
    pub ceil_h: f32,
    pub floor_tex: TextureId,
    pub ceil_tex: TextureId,
    pub light: f32,
}

#[derive(Clone, Debug)]
pub struct SegmentCS {
    pub vertex1: Vec2,
    pub vertex2: Vec2,
    pub front_sector: SectorCS,
    pub back_sector: SectorCS,
    pub two_sided: bool,
    pub lower_unpegged: bool,
    pub upper_unpegged: bool,
    pub low_texture: TextureId,
    pub middle_texture: TextureId,
    pub upper_texture: TextureId,
    pub y_offset: f32,
    pub seg_idx: u16,
}

pub trait Renderer {
    fn begin_frame(&mut self, w: usize, h: usize);

    fn draw_segments(
        &mut self,
        segments: &Vec<SegmentCS>,
        camera: &Camera,
        texture_bank: &TextureBank,
    );

    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize);
}

pub mod software;
