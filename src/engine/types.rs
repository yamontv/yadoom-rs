/// Constants that depend on the *frame-buffer*, not on the map.
#[derive(Clone, Copy)]
pub struct Screen {
    pub w: usize,
    pub h: usize,
    pub half_h: f32, // pre-derived for speed
    pub half_w: f32, // pre-derived for speed
}

/// Camera state reused by every raster unit.
#[derive(Clone, Copy)]
pub struct Viewer {
    pub focal: f32,
    pub eye_floor_z: f32, // height of the eye above the sector floor
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
