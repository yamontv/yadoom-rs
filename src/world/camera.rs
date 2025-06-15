use glam::{Vec2, Vec3, vec2};

/// Player view-point in world space.
///
/// * Only **yaw** (heading) is simulated – Doom never tilts up/down.
/// * `z` holds eye height above floor, not absolute altitude.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pos: Vec3, // x,y in map-units; z = eye height above floor
    yaw: f32,  // radians (0 = east, counter-clockwise)
    fov: f32,  // horizontal FoV (radians, typical 90–110°)
}

impl Camera {
    /// Create a new camera at `pos`, facing `yaw`, with horizontal FoV `fov`.
    pub fn new(pos: Vec3, yaw: f32, fov: f32) -> Self {
        Self { pos, yaw, fov }
    }

    /// World-space eye position: (x, y) = map units, z = eye height above floor.
    #[inline]
    pub fn pos(&self) -> Vec3 {
        self.pos
    }

    /// Transform an X–Y point `p` into camera‐local coords:
    ///  .x = lateral offset (+ right)
    ///  .y = depth along forward axis
    #[inline]
    pub fn to_cam(&self, p: Vec2) -> Vec2 {
         // Translate into camera space
        let dx = p.x - self.pos.x;
        let dy = p.y - self.pos.y;
        // Precompute sin/cos of yaw
        let (s, c) = self.yaw.sin_cos();
        // Rotate by -yaw: align world so camera forward is +X
        let x_cam = dx * c + dy * s;
        let y_cam = dx * s - dy * c;
        vec2(y_cam, x_cam)
    }

    /*──────────────────────── derived vectors ───────────────────────*/

    /// Unit vector pointing where the camera looks on the X-Y plane.
    #[inline(always)]
    pub fn forward(self) -> Vec2 {
        let (s, c) = self.yaw.sin_cos();
        Vec2::new(c, s) // 0 rad = +X (east), CCW positive
    }

    /// Unit vector pointing to the camera's right on the X-Y plane.
    #[inline(always)]
    pub fn right(self) -> Vec2 {
        // Perpendicular to forward: (x, y) -> (y, -x)
        self.forward().perp()
    }

    /*──────────────────────── movement helpers ──────────────────────*/

    /// Move by `forward` units and `side` (strafe), preserving eye-height.
    pub fn step(&mut self, forward: f32, side: f32) {
        let f = self.forward();
        let r = self.right();
        self.pos.x += f.x * forward + r.x * side;
        self.pos.y += f.y * forward + r.y * side;
    }

    /// Rotate around Z-axis (positive = turn left).
    pub fn turn(&mut self, delta_yaw: f32) {
        self.yaw = (self.yaw + delta_yaw).rem_euclid(std::f32::consts::TAU);
    }

    /*───────────────── projection / frustum helpers ─────────────────*/

    /// Pixel-per-map-unit scale for viewport width `w`.
    ///
    /// ```text
    /// focal = w / (2 * tan(fov/2))
    /// ```
    #[inline]
    pub fn screen_scale(self, w: usize) -> f32 {
        (w as f32) * 0.5 / (self.fov * 0.5).tan()
    }

    /// Near-plane distance (fixed small constant in classic Doom).
    #[inline(always)]
    pub fn near(self) -> f32 {
        1.0
    }
}

/*====================================================================*/
/*                                Tests                                */
/*====================================================================*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn forward_and_right_are_orthonormal() {
        let cam = Camera::new(Vec3::ZERO, 0.3, 1.57);
        let f = cam.forward();
        let r = cam.right();
        assert!((f.length() - 1.0).abs() < 1e-5);
        assert!((r.length() - 1.0).abs() < 1e-5);
        assert!((f.dot(r)).abs() < 1e-5);
    }

    #[test]
    fn screen_scale_at_90_deg() {
        let cam = Camera::new(Vec3::ZERO, 0.0, FRAC_PI_2);
        assert!((cam.screen_scale(640) - 320.0).abs() < 1e-3);
    }

    #[test]
    fn to_cam_axes_align() {
        let cam = Camera::new(Vec3::ZERO, 0.0, FRAC_PI_2);
        // Point straight ahead at (10, 0) → (lateral=0, forward=10)
        assert!((cam.to_cam(vec2(10.0, 0.0)) - vec2(0.0, 10.0)).length() < 1e-5);
        // Point to the right at (0, 5) → (lateral=5, forward=0)
        assert!((cam.to_cam(vec2(0.0, 5.0)) - vec2(5.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn to_cam_rotated_yaw() {
        let cam = Camera::new(Vec3::ZERO, FRAC_PI_2, FRAC_PI_2);
        // Yaw = 90°: forward is +Y; (0,10) → (lateral=0, forward=10)
        assert!((cam.to_cam(vec2(0.0, 10.0)) - vec2(0.0, 10.0)).length() < 1e-5);
    }
}
