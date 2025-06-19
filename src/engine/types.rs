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
