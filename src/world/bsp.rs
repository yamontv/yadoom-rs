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
//! Node::bbox()
//! Aabb
//! ```

use crate::world::geometry::{Level, Node};
use glam::{Vec2, vec2};

pub const CHILD_MASK: u16 = 0x7FFF;

pub const SUBSECTOR_BIT: u16 = 0x8000;

// ──────────────────────────────────────────────────────────────────────────
//                       Level – public helpers
// ──────────────────────────────────────────────────────────────────────────
impl Level {
    /// Index of the BSP root (`nodes.len()-1` in Doom).
    #[inline(always)]
    pub fn bsp_root(&self) -> u16 {
        assert!(self.nodes.len() != 0);
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
                    .map_or(false, |sd| sd.sector == sector_idx)
                    || ld
                        .left_sidedef
                        .and_then(|s| self.sidedefs.get(s as usize))
                        .map_or(false, |sd| sd.sector == sector_idx)
            })
            .map(|(i, _)| i as u16)
    }
}

// ──────────────────────────────────────────────────────────────────────────
//                       Node geometry helpers
// ──────────────────────────────────────────────────────────────────────────

/// Axis-aligned bounding box (map units).
#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: Vec2,
    pub max: Vec2,
}

impl Node {
    /// 0 = *front* of splitter, 1 = *back*.
    #[inline(always)]
    pub fn point_side(&self, p: Vec2) -> i32 {
        let d = (p.x - self.x as f32) * self.dy as f32 - (p.y - self.y as f32) * self.dx as f32;
        if d >= 0.0 { 0 } else { 1 }
    }

    /// Bounding box of child `side` (0 front, 1 back).
    pub fn bbox(&self, side: usize) -> Aabb {
        // [ top, bottom, left, right ]
        // y-max, y-min, x-min, x-max
        let bb = self.bbox[side];
        Aabb {
            min: vec2(bb[2] as f32, bb[1] as f32), // x-min, y-min
            max: vec2(bb[3] as f32, bb[0] as f32), // x-max, y-max
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use crate::{
        wad::{Wad, loader},
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
            let bb = root.bbox(side);
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
