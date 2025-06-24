//! BSP helpers.
//!
//! Public API you can rely on:
//! ```text
//! Level::bsp_root()
//! Level::locate_subsector()
//! Level::finalise_bsp()
//! Level::segs_of_subsector()
//! Level::linedefs_of_sector()
//! Node::point_side()
//! ```

use crate::world::camera::Camera;
use crate::world::geometry::{Aabb, Level, Node, SegmentId};
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

    /// Build `sector_of_subsector` once after load / edit.
    pub fn finalise_bsp(&mut self) {
        if self.sector_of_subsector.is_empty() {
            self.sector_of_subsector = self
                .subsectors
                .iter()
                .map(|ss| {
                    let seg = &self.segs[ss.first_seg as usize];
                    let ld = &self.linedefs[seg.linedef as usize];
                    let side = if seg.dir == 0 {
                        ld.right_sidedef
                    } else {
                        ld.left_sidedef
                    };
                    side.and_then(|s| self.sidedefs.get(s as usize))
                        .map(|sd| sd.sector)
                        .unwrap_or(0)
                })
                .collect();
        }
    }

    /// Iterate **seg indices** that form subsector `ss_idx`.
    pub fn segs_of_subsector<'a>(&'a self, ss_idx: u16) -> impl Iterator<Item = u16> + 'a {
        let ss = &self.subsectors[ss_idx as usize];
        let start = ss.first_seg as usize;
        let end = start + ss.seg_count as usize;
        (start..end).map(|i| i as u16)
    }

    /// Iterate **linedef indices** bordering sector `sector_idx`.
    pub fn linedefs_of_sector<'a>(&'a self, sector_idx: u16) -> impl Iterator<Item = u16> + 'a {
        self.linedefs
            .iter()
            .enumerate()
            .filter(move |(_, ld)| {
                ld.right_sidedef
                    .and_then(|s| self.sidedefs.get(s as usize))
                    .is_some_and(|sd| sd.sector == sector_idx)
                    || ld
                        .left_sidedef
                        .and_then(|s| self.sidedefs.get(s as usize))
                        .is_some_and(|sd| sd.sector == sector_idx)
            })
            .map(|(i, _)| i as u16)
    }

    pub fn fill_active_segments(&self, camera: &Camera, segments: &mut Vec<SegmentId>) {
        segments.clear();

        self.walk_bsp(self.bsp_root(), camera, segments);
    }

    fn collect_subsector_segments(
        &self,
        ss_idx: u16,
        camera: &Camera,
        segments: &mut Vec<SegmentId>,
    ) {
        let ss = &self.subsectors[ss_idx as usize];
        let start = ss.first_seg;
        let end = start + ss.seg_count;

        for seg_idx in start..end {
            // Back‑face cull
            if !self.back_facing_seg(seg_idx, camera) {
                segments.push(seg_idx);
            }
        }
    }

    fn walk_bsp(&self, child: u16, camera: &Camera, segments: &mut Vec<SegmentId>) {
        if child & SUBSECTOR_BIT != 0 {
            self.collect_subsector_segments(child & CHILD_MASK, camera, segments);
            return;
        }

        // Internal node ──────
        let node = &self.nodes[child as usize];
        let front = node.point_side(camera.pos.truncate()) as usize; // 0: front, 1: back
        let near = node.child[front];
        let back = node.child[front ^ 1];
        let back_visible = node.bbox[front ^ 1].bbox_in_fov(camera);

        // Near side first …
        self.walk_bsp(near, camera, segments);

        // … far side only if its bounding box might be visible.
        if back_visible {
            self.walk_bsp(back, camera, segments);
        }
    }

    fn back_facing_seg(&self, seg_idx: u16, camera: &Camera) -> bool {
        let seg = &self.segs[seg_idx as usize];
        let cam_pos = camera.pos.truncate();

        // endpoint positions in 2D
        let p1 = self.vertices[seg.v1 as usize].pos;
        let p2 = self.vertices[seg.v2 as usize].pos;

        // vectors from camera to each endpoint
        let v1 = p1 - cam_pos;
        let v2 = p2 - cam_pos;

        // compute angles in [–π, π]
        let a1 = v1.y.atan2(v1.x);
        let a2 = v2.y.atan2(v2.x);

        // delta, normalized into [0, 2π)
        let span = (a1 - a2).rem_euclid(2.0 * std::f32::consts::PI);

        // if the angular span ≥ π, the wall is fully behind us
        span >= std::f32::consts::PI
    }

    /// Return the floor height (Z) of the sector the player is currently in.
    pub fn floor_height_under_player(&self, pos: Vec2) -> f32 {
        let ss_idx = self.locate_subsector(pos);
        let ss = &self.subsectors[ss_idx as usize];
        let seg = &self.segs[ss.first_seg as usize];
        let ld = &self.linedefs[seg.linedef as usize];
        let sd_idx = if seg.dir == 0 {
            ld.right_sidedef
        } else {
            ld.left_sidedef
        }
        .expect("subsector SEG must have a sidedef");
        let sector = &self.sectors[self.sidedefs[sd_idx as usize].sector as usize];
        sector.floor_h
    }
}

// ──────────────────────────────────────────────────────────────────────────
//                       Node geometry helpers
// ──────────────────────────────────────────────────────────────────────────
impl Node {
    /// 0 = *front* of splitter, 1 = *back*.
    #[inline(always)]
    pub fn point_side(&self, p: Vec2) -> i32 {
        let d = (p.x - self.x as f32) * self.dy as f32 - (p.y - self.y as f32) * self.dx as f32;
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
        wad::{raw::Wad, loader},
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

    #[test]
    fn methods_return_expected_ranges() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let mut bank = TextureBank::default_with_checker();
        let mut lvl = loader::load_level(&wad, wad.level_indices()[0], &mut bank).unwrap();
        lvl.finalise_bsp();

        let ss0 = lvl.locate_subsector(lvl.things[0].pos);
        assert!(lvl.segs_of_subsector(ss0).count() > 0);

        let sect = lvl.sector_of_subsector[ss0 as usize];
        assert!(lvl.linedefs_of_sector(sect).count() > 0);
    }
}
