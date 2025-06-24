use glam::Vec2;

use crate::{
    renderer::SegmentCS,
    world::bsp::{CHILD_MASK, SUBSECTOR_BIT},
    world::camera::Camera,
    world::geometry::{Aabb, Level},
};

pub struct Engine {
    pub level: Level,
    pub segments: Vec<SegmentCS>,
}

impl Engine {
    pub fn new(level: Level) -> Self {
        Self {
            level,
            segments: Vec::new(),
        }
    }

    pub fn build_frame(&mut self, camera: &Camera) {
        self.segments.clear();

        self.walk_bsp(self.level.bsp_root(), camera);
    }

    fn walk_bsp(&mut self, child: u16, camera: &Camera) {
        if child & SUBSECTOR_BIT != 0 {
            self.push_subsector(child & CHILD_MASK, camera);
            return;
        }

        // Internal node ──────
        let node = &self.level.nodes[child as usize];
        let front = node.point_side(camera.pos.truncate()) as usize; // 0: front, 1: back
        let near = node.child[front];
        let back = node.child[front ^ 1];
        let back_visible = Self::bbox_in_fov(&node.bbox[front ^ 1], camera);

        // Near side first …
        self.walk_bsp(near, camera);

        // … far side only if its bounding box might be visible.
        if back_visible {
            self.walk_bsp(back, camera);
        }
    }

    /// Return the floor height (Z) of the sector the player is currently in.
    pub fn floor_height_under_player(&self, pos: Vec2) -> f32 {
        let ss_idx = self.level.locate_subsector(pos);
        let ss = &self.level.subsectors[ss_idx as usize];
        let seg = &self.level.segs[ss.first_seg as usize];
        let ld = &self.level.linedefs[seg.linedef as usize];
        let sd_idx = if seg.dir == 0 {
            ld.right_sidedef
        } else {
            ld.left_sidedef
        }
        .expect("subsector SEG must have a sidedef");
        let sector = &self.level.sectors[self.level.sidedefs[sd_idx as usize].sector as usize];
        sector.floor_h as f32
    }

    fn bbox_in_fov(b: &Aabb, cam: &Camera) -> bool {
        use std::f32::consts::PI;

        let half_fov = cam.fov * 0.5;

        // Fast accept when camera inside bbox
        if cam.pos.x >= b.min.x
            && cam.pos.x <= b.max.x
            && cam.pos.y >= b.min.y
            && cam.pos.y <= b.max.y
        {
            return true;
        }

        // 1. collect the four corner angles (wrapped to [-π, π])
        let rel = [
            Vec2::new(b.min.x - cam.pos.x, b.min.y - cam.pos.y),
            Vec2::new(b.max.x - cam.pos.x, b.min.y - cam.pos.y),
            Vec2::new(b.min.x - cam.pos.x, b.max.y - cam.pos.y),
            Vec2::new(b.max.x - cam.pos.x, b.max.y - cam.pos.y),
        ];

        let mut left = PI;
        let mut right = -PI;
        for v in &rel {
            let mut a = v.y.atan2(v.x) - cam.yaw;
            if a > PI {
                a -= 2.0 * PI;
            }
            if a < -PI {
                a += 2.0 * PI;
            }
            left = left.min(a);
            right = right.max(a);
        }

        let span = right - left;
        if span > PI {
            // Wedge crosses the ±π seam.  The "big" interval is visible
            // unless the whole FOV falls into the small complement.
            return !(right < -half_fov && left > half_fov);
        }

        // Normal case: does [left,right] overlap [-half_fov, +half_fov] ?
        right >= -half_fov && left <= half_fov
    }
}
