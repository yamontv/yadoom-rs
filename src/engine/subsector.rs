use crate::{
    engine::engine::Engine,
    renderer::{SectorCS, SegmentCS},
    world::{
        camera::Camera,
        geometry::{Linedef, LinedefFlags, Sector, Seg, Sidedef},
    },
};

impl Engine {
    fn back_facing_seg(&self, seg_idx: u16, camera: &Camera) -> bool {
        let seg = &self.level.segs[seg_idx as usize];
        let cam_pos = camera.pos.truncate();

        // endpoint positions in 2D
        let p1 = self.level.vertices[seg.v1 as usize].pos;
        let p2 = self.level.vertices[seg.v2 as usize].pos;

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

    fn sectors_for_seg(&self, seg: &Seg) -> (&Sidedef, Option<&Sector>, &Linedef) {
        let ld = &self.level.linedefs[seg.linedef as usize];
        let (sd_front_idx, sd_back_idx) = if seg.dir == 0 {
            (ld.right_sidedef, ld.left_sidedef)
        } else {
            (ld.left_sidedef, ld.right_sidedef)
        };
        let front = &self.level.sidedefs[sd_front_idx.unwrap() as usize];
        let back = sd_back_idx
            .and_then(|i| self.level.sidedefs.get(i as usize))
            .map(|sd| &self.level.sectors[sd.sector as usize]);
        (front, back, ld)
    }

    pub fn push_subsector(&mut self, ss_idx: u16, camera: &Camera) {
        let ss = &self.level.subsectors[ss_idx as usize];
        let start = ss.first_seg;
        let end = start + ss.seg_count;

        for seg_idx in start..end {
            // Back‑face cull
            if self.back_facing_seg(seg_idx, camera) {
                continue;
            }

            let seg = &self.level.segs[seg_idx as usize];
            let (sd_front, sec_back_opt, ld) = self.sectors_for_seg(seg);
            let sec_front = &self.level.sectors[sd_front.sector as usize];

            self.segments.push(SegmentCS {
                vertex1: self.level.vertices[seg.v1 as usize].pos,
                vertex2: self.level.vertices[seg.v2 as usize].pos,
                front_sector: SectorCS {
                    floor_h: sec_front.floor_h as f32,
                    ceil_h: sec_front.ceil_h as f32,
                    floor_tex: sec_front.floor_tex,
                    ceil_tex: sec_front.ceil_tex,
                    light: sec_front.light,
                },
                back_sector: sec_back_opt.map_or(SectorCS::default(), |sec_back| SectorCS {
                    floor_h: sec_back.floor_h as f32,
                    ceil_h: sec_back.ceil_h as f32,
                    floor_tex: sec_back.floor_tex,
                    ceil_tex: sec_back.ceil_tex,
                    light: sec_back.light,
                }),
                two_sided: sec_back_opt.is_some() && ld.flags.contains(LinedefFlags::TWO_SIDED),
                lower_unpegged: ld.flags.contains(LinedefFlags::LOWER_UNPEGGED),
                upper_unpegged: ld.flags.contains(LinedefFlags::UPPER_UNPEGGED),
                low_texture: sd_front.lower,
                middle_texture: sd_front.middle,
                upper_texture: sd_front.upper,
                y_offset: sd_front.y_off as f32,
                seg_idx,
            });
        }
    }
}
