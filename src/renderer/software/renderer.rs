use crate::{
    renderer::software::planes::PlaneMap,
    renderer::{Renderer, Rgba},
    world::camera::Camera,
    world::geometry::{Level, SegmentId},
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
    }

    fn draw_segments(
        &mut self,
        segments: &[SegmentId],
        level: &Level,
        camera: &Camera,
        texture_bank: &TextureBank,
    ) {
        self.focal = camera.screen_scale(self.width);
        self.view_z = camera.pos.z;

        for segment in segments.iter().copied() {
            if let Some(edge) = self.project_seg(segment, level, camera) {
                self.draw_edge(edge, segment, level, texture_bank);
            }
        }
        self.flush_planes(camera, texture_bank);
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
