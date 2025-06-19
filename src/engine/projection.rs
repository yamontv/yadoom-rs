use crate::{
    engine::types::{Edge, Screen, Viewer},
    world::{camera::Camera, geometry::Level},
};

pub fn project_seg(
    seg_idx: u16,
    lvl: &Level,
    cam: &Camera,
    screen: &Screen,
    view: &Viewer,
) -> Option<Edge> {
    let seg = &lvl.segs[seg_idx as usize];
    // World endpoints → camera space
    let v1 = lvl.vertices[seg.v1 as usize].pos;
    let v2 = lvl.vertices[seg.v2 as usize].pos;
    let mut p1 = cam.to_cam(v1);
    let mut p2 = cam.to_cam(v2);

    debug_assert!(p1.y != 0.0 && p2.y != 0.0);

    // Near-plane clip (track tex-coord t1,t2)
    let mut t1 = 0.0;
    let mut t2 = 1.0;
    if !clip_near(&mut p1, &mut p2, &mut t1, &mut t2, cam) {
        return None;
    }

    // Project to screen X
    let mut sx1 = screen.half_w + p1.x * view.focal / p1.y;
    let mut sx2 = screen.half_w + p2.x * view.focal / p2.y;
    if (sx1 < 0.0 && sx2 < 0.0) || (sx1 >= screen.half_w * 2.0 && sx2 >= screen.half_w * 2.0) {
        return None; // completely off-screen
    }

    // Ensure  p1 → p2 is left → right in screen space
    if sx1 > sx2 {
        core::mem::swap(&mut sx1, &mut sx2);
        core::mem::swap(&mut p1, &mut p2);
        core::mem::swap(&mut t1, &mut t2);
    }

    let x_l = sx1.max(0.0) as i32;
    let x_r = sx2.min(screen.w as f32 - 1.0) as i32;
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

/// Clip a segment to the near plane. Returns false if completely behind.
fn clip_near(
    p1: &mut glam::Vec2,
    p2: &mut glam::Vec2,
    t1: &mut f32,
    t2: &mut f32,
    cam: &Camera,
) -> bool {
    let near = cam.near();
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
