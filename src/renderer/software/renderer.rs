use crate::{
    renderer::software::{
        planes::PlaneMap,
        sprites::{DrawSeg, FrameScratch, VisSprite},
    },
    renderer::{Renderer, Rgba},
    sim::TicRunner,
    world::camera::Camera,
    world::geometry::{Level, SubsectorId},
    world::texture::TextureBank,
};

#[derive(Default)]
pub struct ClipBands {
    pub ceil: Vec<i16>,
    pub floor: Vec<i16>,
}

#[derive(Default, PartialEq, Debug)]
pub struct ClipRange {
    pub first: i32,
    pub last: i32,
}

#[derive(Default)]
pub struct Software {
    pub scratch: Vec<Rgba>,
    pub clip_bands: ClipBands,
    pub visplane_map: PlaneMap,
    pub solid_segs: Vec<ClipRange>,
    pub sprites: Vec<VisSprite>,
    pub drawsegs: Vec<DrawSeg>,
    pub frame_scratch: FrameScratch,

    pub width: usize,
    pub height: usize,

    pub width_f: f32,
    pub height_f: f32,
    pub half_w: f32,
    pub half_h: f32,
    pub focal: f32,
    pub view_z: f32,
}

impl Renderer for Software {
    fn begin_frame(&mut self, w: usize, h: usize) {
        if w != self.width || h != self.height {
            self.width = w;
            self.height = h;
            self.width_f = w as f32;
            self.height_f = h as f32;
            self.half_w = self.width_f * 0.5;
            self.half_h = self.height_f * 0.5;
            self.scratch.resize(w * h, 0);
            self.clip_bands.ceil.resize(w, i16::MIN);
            self.clip_bands.floor.resize(w, i16::MAX);
        }
        // dark‑grey clear
        self.scratch.fill(0xFF_20_20_20);

        // fully open clips at start of frame
        self.clip_bands.ceil.fill(i16::MIN);
        self.clip_bands.floor.fill(i16::MAX);

        self.init_solid_segs();

        self.visplane_map.clear(self.width);

        self.sprites.clear();
        self.drawsegs.clear();
        self.frame_scratch.reset();
    }

    fn draw_level(
        &mut self,
        subsectors: &[SubsectorId],
        level: &Level,
        sim: &TicRunner,
        camera: &Camera,
        texture_bank: &mut TextureBank,
    ) {
        // win.update_with_buffer(&self.scratch, self.width, self.height);
        if subsectors.is_empty() {
            return;
        }

        self.focal = camera.screen_scale(self.width);

        let sec0_idx = level.subsectors[subsectors[0] as usize].sector;
        let floor_z = level.sectors[sec0_idx as usize].floor_h;
        self.view_z = camera.pos.z + floor_z;

        for ss_idx in subsectors.iter().copied() {
            let ss = &level.subsectors[ss_idx as usize];
            let start = ss.first_line;
            let end = start + ss.num_lines;

            self.collect_sprites_for_subsector(ss_idx, sim, camera, texture_bank);

            for seg_idx in start..end {
                if let Some(edge) = self.project_seg(seg_idx, level, camera) {
                    self.draw_edge(edge, seg_idx, level, texture_bank);
                }
            }
        }

        self.flush_planes(camera, texture_bank);

        self.draw_sprites(level, texture_bank);
    }

    fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32) {
        let mut x0 = x0;
        let mut y0 = y0;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if (0..self.width as i32).contains(&x0) && (0..self.height as i32).contains(&y0) {
                self.scratch[y0 as usize * self.width + x0 as usize] = col;
            }
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    fn end_frame<F>(&mut self, submit: F)
    where
        F: FnOnce(&[Rgba], usize, usize),
    {
        submit(&self.scratch, self.width, self.height);
    }
}

impl Software {
    pub fn init_solid_segs(&mut self) {
        let w = self.width as i32;
        self.solid_segs.clear();
        // Two sentinels so our add routine never has to worry
        // about running off the ends of the array.
        self.solid_segs.push(ClipRange {
            first: -w,
            last: -1,
        });
        self.solid_segs.push(ClipRange {
            first: w,
            last: w * 2,
        });
    }

    pub fn add_solid_seg(&mut self, first: i32, last: i32) {
        let mut i = 0;
        // 1) skip all segments that end *before* ours minus one
        while i < self.solid_segs.len() && self.solid_segs[i].last < first - 1 {
            i += 1;
        }

        // if the new segment is completely swallowed by an existing one,
        // we’re done early
        if i < self.solid_segs.len()
            && first >= self.solid_segs[i].first
            && last <= self.solid_segs[i].last
        {
            return;
        }

        // 2) merge any overlapping or adjacent segments:
        let mut new_first = first;
        let mut new_last = last;
        while i < self.solid_segs.len() && self.solid_segs[i].first <= new_last + 1 {
            new_first = new_first.min(self.solid_segs[i].first);
            new_last = new_last.max(self.solid_segs[i].last);
            self.solid_segs.remove(i);
        }

        // 3) insert the coalesced segment in its sorted place
        self.solid_segs.insert(
            i,
            ClipRange {
                first: new_first,
                last: new_last,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{ClipRange, Software}; // or whatever your types are called

    /// Regression test for the “new_last not updated” bug in add_solid_seg().
    #[test]
    fn merge_chain_of_touching_spans() {
        let mut sw = Software::default();
        // helper: build the initial solid‐seg list
        let segs = vec![
            ClipRange { first: 0, last: 5 },
            ClipRange { first: 8, last: 12 },
            ClipRange {
                first: 13,
                last: 20,
            },
        ];

        sw.solid_segs = segs;

        // new wall span that should close BOTH gaps (5-6 and 12-13)
        sw.add_solid_seg(6, 9);

        // after the fix we expect ONE merged span covering 0‥20
        let expected = vec![ClipRange { first: 0, last: 20 }];
        assert_eq!(
            sw.solid_segs, expected,
            "solid_segs should be fully coalesced after inserting a bridging span"
        );
    }
}
