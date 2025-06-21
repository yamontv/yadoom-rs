use crate::{engine::engine::Engine, engine::types::Edge, renderer::Renderer};

impl<R: Renderer> Engine<R> {
    pub fn project_seg(&self, seg_idx: u16) -> Option<Edge> {
        let seg = &self.level.segs[seg_idx as usize];
        // World endpoints → camera space
        let v1 = self.level.vertices[seg.v1 as usize].pos;
        let v2 = self.level.vertices[seg.v2 as usize].pos;
        let mut p1 = self.camera.to_cam(v1);
        let mut p2 = self.camera.to_cam(v2);

        debug_assert!(p1.y != 0.0 && p2.y != 0.0);

        // Near-plane clip (track tex-coord t1,t2)
        let mut t1 = 0.0;
        let mut t2 = 1.0;
        if !Self::clip_near(&mut p1, &mut p2, &mut t1, &mut t2, self.camera.near()) {
            return None;
        }

        // Project to screen X
        let mut sx1 = self.screen.half_w + p1.x * self.view.focal / p1.y;
        let mut sx2 = self.screen.half_w + p2.x * self.view.focal / p2.y;
        if (sx1 < 0.0 && sx2 < 0.0) || (sx1 >= self.screen.w as f32 && sx2 >= self.screen.w as f32)
        {
            return None; // completely off-screen
        }

        // Ensure  p1 → p2 is left → right in screen space
        if sx1 > sx2 {
            core::mem::swap(&mut sx1, &mut sx2);
            core::mem::swap(&mut p1, &mut p2);
            core::mem::swap(&mut t1, &mut t2);
        }

        let x_l = sx1.max(0.0) as i32;
        let x_r = sx2.min(self.screen.w as f32 - 1.0) as i32;
        if x_l >= x_r {
            return None;
        }

        // Perspective helpers shared by all spans on this edge
        let invz_p1 = 1.0 / p1.y;
        let invz_p2 = 1.0 / p2.y;
        let wall_len = (v2 - v1).length();
        let uoz_p1 = t1 * wall_len * invz_p1;
        let uoz_p2 = t2 * wall_len * invz_p2;

        let span = sx2 - sx1;
        let frac_l = (x_l as f32 - sx1) / span;
        let frac_r = (x_r as f32 - sx1) / span;

        Some(Edge {
            x_l,
            x_r,
            invz_l: invz_p1 + (invz_p2 - invz_p1) * frac_l,
            invz_r: invz_p1 + (invz_p2 - invz_p1) * frac_r,
            uoz_l: uoz_p1 + (uoz_p2 - uoz_p1) * frac_l,
            uoz_r: uoz_p1 + (uoz_p2 - uoz_p1) * frac_r,
            seg_idx: seg_idx as u16,
        })
    }

    pub fn back_facing_seg(&self, seg_idx: u16) -> bool {
        let seg = &self.level.segs[seg_idx as usize];
        let cam_pos = self.camera.pos().truncate();

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

    /// Complete Doom-style bbox visibility test (steps 1–8) in f32.
    pub fn bbox_visible(&self, bbox: &[i16; 4]) -> bool {
        use std::f32::consts::PI;

        // 1) Unpack & normalize bbox corners
        let left = bbox[2] as f32;
        let right = bbox[3] as f32;
        let bottom = bbox[1] as f32;
        let top = bbox[0] as f32;

        // 2) World-space corners: (L,T), (R,T), (L,B), (R,B)
        let world = [(left, top), (right, top), (left, bottom), (right, bottom)];

        // 3) Compute each corner’s view-relative angle in [-π, π)
        let mut angle = [0.0f32; 4];
        for i in 0..4 {
            let dx = world[i].0 - self.camera.pos().x;
            let dy = world[i].1 - self.camera.pos().y;
            let mut a = dy.atan2(dx) - self.camera.yaw;
            if a <= -PI {
                a += 2.0 * PI;
            } else if a > PI {
                a -= 2.0 * PI;
            }
            angle[i] = a;
        }

        // 4) Find the corner-pair (i1,i2) with the largest positive wrapped span
        let mut best_span = -1.0f32;
        let (mut i1, mut i2) = (0, 0);
        for i in 0..4 {
            for j in 0..4 {
                if i == j {
                    continue;
                }
                let mut d = angle[i] - angle[j];
                if d < 0.0 {
                    d += 2.0 * PI;
                }
                if d > best_span {
                    best_span = d;
                    (i1, i2) = (i, j);
                }
            }
        }

        // 5) If span ≥ 180°, box surrounds camera → visible
        if best_span >= PI {
            return true;
        }

        // Extract the two extreme angles
        let a1 = angle[i1];
        let a2 = angle[i2];

        // 6) Clip that wedge to the horizontal FOV
        let clipangle = (self.screen.half_w / self.view.focal).atan();
        let two_clip = 2.0 * clipangle;
        let a1c = a1.max(-clipangle);
        let a2c = a2.min(clipangle);
        // Entirely off-screen?
        if a2c < -clipangle || a1c > clipangle {
            return false;
        }

        // 7) Map clamped angles to screen-column indices
        let sx1f = (a1c + clipangle) / two_clip * (self.screen.w as f32);
        let sx2f = (a2c + clipangle) / two_clip * (self.screen.w as f32);
        let sx1 = sx1f.floor() as i32;
        let sx2 = sx2f.ceil() as i32 - 1;
        if sx1 > sx2 {
            return false;
        }

        // 8) Occlusion test against `solid_segs`
        let mut idx = 0;
        while idx < self.solid_segs.len() && self.solid_segs[idx].last < sx2 {
            idx += 1;
        }
        if idx < self.solid_segs.len() {
            let seg = &self.solid_segs[idx];
            if sx1 >= seg.first && sx2 <= seg.last {
                return false;
            }
        }

        // At least partly visible
        true
    }

    /// Clip a segment to the near plane. Returns false if completely behind.
    fn clip_near(
        p1: &mut glam::Vec2,
        p2: &mut glam::Vec2,
        t1: &mut f32,
        t2: &mut f32,
        near: f32,
    ) -> bool {
        if p1.y <= near && p2.y <= near {
            return false;
        }
        if p1.y < near {
            let t = (near - p1.y) / (p2.y - p1.y);
            *p1 += (*p2 - *p1) * t;
            p1.y = near;
            *t1 = t;
        }
        if p2.y < near {
            let t = (near - p2.y) / (p1.y - p2.y);
            *p2 += (*p1 - *p2) * t;
            p2.y = near;
            *t2 = 1.0 - t;
        }
        true
    }
}
