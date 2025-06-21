use crate::world::texture::TextureId;

/// Constants that depend on the *frame-buffer*, not on the map.
#[derive(Clone, Copy)]
pub struct Screen {
    pub w: usize,
    pub h: usize,
    pub half_h: f32, // pre-derived for speed
    pub half_w: f32, // pre-derived for speed
}

/// Camera state reused by every raster unit.
#[derive(Clone, Copy, Default)]
pub struct Viewer {
    pub focal: f32,
    pub floor_z: f32,
    pub view_z: f32, // height of the eye above the sector floor
}

/// Everything the clipping / span builder needs for one visible edge.
#[derive(Clone, Copy)]
pub struct Edge {
    pub x_l: i32,
    pub x_r: i32,
    pub invz_l: f32,
    pub invz_r: f32,
    pub uoz_l: f32,
    pub uoz_r: f32,
    pub seg_idx: u16,
}

/// One “flat” that is visible somewhere on screen.
#[derive(Clone)]
pub struct VisPlane {
    pub height: i16,
    pub tex: TextureId,
    pub light: i16,

    /// Inclusive horizontal range that the plane touches.
    pub min_x: u16,
    pub max_x: u16,

    /// For every screen column we remember the highest and lowest pixel that is
    /// still uncovered **after** drawing the front geometry.
    pub top: Vec<u16>,
    pub bottom: Vec<u16>,
}

#[derive(Debug, Clone)]
pub struct ClipRange {
    pub first: i32,
    pub last: i32,
}
