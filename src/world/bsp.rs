use crate::world::camera::Camera;
use crate::world::geometry::{Aabb, Level, Node, SubsectorId};
use glam::Vec2;

pub const CHILD_MASK: u16 = 0x7FFF;

pub const SUBSECTOR_BIT: u16 = 0x8000;

// ──────────────────────────────────────────────────────────────────────────
//                       Level – public helpers
// ──────────────────────────────────────────────────────────────────────────
impl Level {
    /// Index of the BSP root (`nodes.len()-1` in Doom).
    #[inline(always)]
    pub fn bsp_root(&self) -> u16 {
        assert!(!self.nodes.is_empty());
        (self.nodes.len() - 1) as u16
    }

    /// Walk the BSP and return the subsector id containing `p`.
    pub fn locate_subsector(&self, p: Vec2) -> u16 {
        let mut idx = self.bsp_root();
        loop {
            let node = &self.nodes[idx as usize];
            let child = node.child[node.point_side(p) as usize];
            if child & SUBSECTOR_BIT != 0 {
                return child & CHILD_MASK;
            }
            idx = child;
        }
    }

    pub fn finalise_bsp(&mut self) {
        for ss in self.subsectors.iter_mut() {
            let seg = &self.segs[ss.first_line as usize];
            let ld = &self.linedefs[seg.linedef as usize];
            let side = if seg.dir == 0 {
                ld.right_sidedef
            } else {
                ld.left_sidedef
            };
            ss.sector = side
                .and_then(|s| self.sidedefs.get(s as usize))
                .map(|sd| sd.sector)
                .unwrap_or(u16::MAX);
        }

        let ss_for_thing: Vec<u16> = self
            .things
            .iter()
            .map(|t| self.locate_subsector(t.pos))
            .collect();

        for (thing, ss) in self.things.iter_mut().zip(ss_for_thing) {
            thing.sub_sector = ss;
        }

        for (thing_idx, thing) in self.things.iter().enumerate() {
            self.subsectors[thing.sub_sector as usize].things.push(thing_idx as u16);
        }
    }

    pub fn fill_active_subsectors(&self, camera: &Camera, subsectors: &mut Vec<SubsectorId>) {
        subsectors.clear();

        self.walk_bsp(self.bsp_root(), camera, subsectors);
    }

    fn walk_bsp(&self, child: u16, camera: &Camera, subsectors: &mut Vec<SubsectorId>) {
        if child & SUBSECTOR_BIT != 0 {
            subsectors.push(child & CHILD_MASK);
            return;
        }

        // Internal node ──────
        let node = &self.nodes[child as usize];
        let front = node.point_side(camera.pos.truncate()) as usize; // 0: front, 1: back
        let near = node.child[front];
        let back = node.child[front ^ 1];
        let back_visible = node.bbox[front ^ 1].bbox_in_fov(camera);

        // Near side first …
        self.walk_bsp(near, camera, subsectors);

        // … far side only if its bounding box might be visible.
        if back_visible {
            self.walk_bsp(back, camera, subsectors);
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
//                       Node geometry helpers
// ──────────────────────────────────────────────────────────────────────────
impl Node {
    /// 0 = *front* of splitter, 1 = *back*.
    #[inline(always)]
    pub fn point_side(&self, p: Vec2) -> i32 {
        let d = (p.x - self.x) * self.dy - (p.y - self.y) * self.dx;
        if d >= 0.0 { 0 } else { 1 }
    }
}

// ──────────────────────────────────────────────────────────────────────────
//                       Aabb geometry helpers
// ──────────────────────────────────────────────────────────────────────────
impl Aabb {
    pub fn bbox_in_fov(&self, cam: &Camera) -> bool {
        use std::f32::consts::PI;

        let half_fov = cam.fov * 0.5;

        // Fast accept when camera inside bbox
        if cam.pos.x >= self.min.x
            && cam.pos.x <= self.max.x
            && cam.pos.y >= self.min.y
            && cam.pos.y <= self.max.y
        {
            return true;
        }

        // 1. collect the four corner angles (wrapped to [-π, π])
        let rel = [
            Vec2::new(self.min.x - cam.pos.x, self.min.y - cam.pos.y),
            Vec2::new(self.max.x - cam.pos.x, self.min.y - cam.pos.y),
            Vec2::new(self.min.x - cam.pos.x, self.max.y - cam.pos.y),
            Vec2::new(self.max.x - cam.pos.x, self.max.y - cam.pos.y),
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

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use crate::{
        wad::{loader, raw::Wad},
        world::texture::TextureBank,
    };
    use std::path::PathBuf;

    fn doom_wad() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("doom.wad")
    }

    #[test]
    fn point_side_matches_bbox() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let mut bank = TextureBank::default_with_checker();
        let lvl = loader::load_level(&wad, wad.level_indices()[0], &mut bank).unwrap();
        let root = &lvl.nodes[lvl.bsp_root() as usize];

        for side in 0..=1 {
            let bb = &root.bbox[side];
            let mid = (bb.min + bb.max) * 0.5;
            assert_eq!(root.point_side(mid), side as i32);
        }
    }
}
