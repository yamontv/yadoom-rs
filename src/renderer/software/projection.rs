use crate::renderer::software::Software;
use crate::world::camera::Camera;
use crate::world::geometry::{Level, SegmentId};

#[derive(Clone, Copy)]
pub struct Edge {
    pub x_l: i32,
    pub x_r: i32,
    pub invz_l: f32,
    pub invz_r: f32,
    pub uoz_l: f32,
    pub uoz_r: f32,
}

impl Software {
    pub fn project_seg(&self, seg_idx: SegmentId, level: &Level, camera: &Camera) -> Option<Edge> {
        let seg = &level.segs[seg_idx as usize];
        let v1 = &level.vertices[seg.v1 as usize].pos;
        let v2 = &level.vertices[seg.v2 as usize].pos;

        // ──────────────────────────────────────────────────────────────────────
        // 1. camera-space endpoints
        // ──────────────────────────────────────────────────────────────────────
        let mut p1 = camera.to_cam(v1);
        let mut p2 = camera.to_cam(v2);
        debug_assert!(p1.y != 0.0 && p2.y != 0.0);

        // ──────────────────────────────────────────────────────────────────────
        // 2. near-plane clip
        // ──────────────────────────────────────────────────────────────────────
        let mut t1 = 0.0;
        let mut t2 = 1.0;
        if !Self::clip_near(&mut p1, &mut p2, &mut t1, &mut t2, camera.near()) {
            return None;
        }

        // ──────────────────────────────────────────────────────────────────────
        // 3. horizontal FOV / screen-x mapping
        // ──────────────────────────────────────────────────────────────────────
        let sx1 = self.half_w + p1.x * self.focal / p1.y;
        let sx2 = self.half_w + p2.x * self.focal / p2.y;

        // Entirely to the left OR right of the viewport?
        let right_lim = self.width_f - 1.0;
        if (sx1 < 0.0 && sx2 < 0.0) || (sx1 > right_lim && sx2 > right_lim) {
            return None;
        }

        // ──────────────────────────────────────────────────────────────────────
        // 4. force left-to-right order in screen space
        // ──────────────────────────────────────────────────────────────────────
        let (sx1, sx2, p1, p2, t1, t2) = if sx1 <= sx2 {
            (sx1, sx2, p1, p2, t1, t2)
        } else {
            (sx2, sx1, p2, p1, t2, t1)
        };

        // ──────────────────────────────────────────────────────────────────────
        // 5. clip to viewport X range, early-out degenerate
        // ──────────────────────────────────────────────────────────────────────
        let x_l = sx1.max(0.0) as i32;
        let x_r = sx2.min(self.width_f - 1.0) as i32;
        if x_l >= x_r {
            return None;
        }

        // ──────────────────────────────────────────────────────────────────────
        // 6. solid-seg occlusion test
        // ──────────────────────────────────────────────────────────────────────
        // Invariant: solid_segs is sorted; we can bail as soon as we find a
        // span whose `last` ≥ x_r.
        if let Some(seg) = self.solid_segs.iter().find(|s| s.last >= x_r)
        // first candidate that can cover
        {
            if x_l >= seg.first && x_r <= seg.last {
                return None; // fully hidden
            }
        }

        // ──────────────────────────────────────────────────────────────────────
        // 7. perspective coefficients for the surviving span
        // ──────────────────────────────────────────────────────────────────────
        let span = sx2 - sx1;
        if span <= 1.0 {
            return None;
        }
        let invz_p1 = 1.0 / p1.y;
        let invz_p2 = 1.0 / p2.y;
        let wall_len = (v2 - v1).length();
        let uoz_p1 = t1 * wall_len * invz_p1;
        let uoz_p2 = t2 * wall_len * invz_p2;

        let frac_l = (x_l as f32 - sx1) / span;
        let frac_r = (x_r as f32 - sx1) / span;

        Some(Edge {
            x_l,
            x_r,
            invz_l: invz_p1 + (invz_p2 - invz_p1) * frac_l,
            invz_r: invz_p1 + (invz_p2 - invz_p1) * frac_r,
            uoz_l: uoz_p1 + (uoz_p2 - uoz_p1) * frac_l,
            uoz_r: uoz_p1 + (uoz_p2 - uoz_p1) * frac_r,
        })
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
