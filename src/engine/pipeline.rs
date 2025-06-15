//! Refactored DrawCall builder with decomposed responsibilities
use glam::{Vec2, vec2};

use crate::{
    renderer::DrawCall,
    world::{
        bsp::{CHILD_MASK, SUBSECTOR_BIT},
        camera::Camera,
        geometry::{Level, LinedefFlags, Seg},
        texture::TextureId,
    },
};

/// Bundles viewport parameters
struct ViewParams {
    half_w: f32,
    half_h: f32,
    focal: f32,
    view_w: usize,
}

/// Holds current per‐column clip bands
struct ClipBuffers<'a> {
    ceil: &'a mut [i16],
    floor: &'a mut [i16],
}

/// How this span affects clip bands
#[derive(Copy, Clone)]
enum ClipKind {
    Solid,
    Upper,
    Lower,
}

/// Interpolated span parameters for a wall slice
struct Span {
    x_l: usize,
    x_r: usize,
    uoz_l: f32,
    uoz_r: f32,
    invz_l: f32,
    invz_r: f32,
    y_top_l: f32,
    y_top_r: f32,
    y_bot_l: f32,
    y_bot_r: f32,
    dt: f32,
    db: f32,
    tex: TextureId,
    kind: ClipKind,
}

impl Span {
    /// Check if any column in this span is visible given current clips
    fn is_visible(&self, clips: &ClipBuffers) -> bool {
        for col in self.x_l..=self.x_r {
            let dx = (col - self.x_l) as f32;
            let yt = self.y_top_l + self.dt * dx;
            let yb = self.y_bot_l + self.db * dx;
            if yt < clips.floor[col] as f32 && yb > clips.ceil[col] as f32 {
                return true;
            }
        }
        false
    }

    /// Apply clip updates for all covered columns
    fn apply_clips(&self, clips: &mut ClipBuffers) {
        for col in self.x_l..=self.x_r {
            let dx = (col - self.x_l) as f32;
            let yt = self.y_top_l + self.dt * dx;
            let yb = self.y_bot_l + self.db * dx;
            if yt < clips.floor[col] as f32 && yb > clips.ceil[col] as f32 {
                match self.kind {
                    ClipKind::Solid => {
                        clips.ceil[col] = (yb.floor() as i16).saturating_add(1);
                        clips.floor[col] = (yt.ceil() as i16).saturating_sub(1);
                    }
                    ClipKind::Upper => {
                        clips.ceil[col] = (yb.floor() as i16).saturating_add(1);
                    }
                    ClipKind::Lower => {
                        clips.floor[col] = (yt.ceil() as i16).saturating_sub(1);
                    }
                }
            }
        }
    }

    /// Convert to a DrawCall for the renderer
    fn to_drawcall(&self) -> DrawCall {
        DrawCall {
            tex_id: self.tex,
            u0_over_z: self.uoz_l,
            u1_over_z: self.uoz_r,
            inv_z0: self.invz_l,
            inv_z1: self.invz_r,
            x_start: self.x_l as i32,
            x_end: self.x_r as i32,
            y_top0: self.y_top_l,
            y_top1: self.y_top_r,
            y_bot0: self.y_bot_l,
            y_bot1: self.y_bot_r,
        }
    }
}

/// Top‐level entry: builds sorted DrawCall list
pub fn build_drawcalls(level: &Level, cam: &Camera, w: usize, h: usize) -> Vec<DrawCall> {
    let mut out = Vec::with_capacity(2048);
    let mut ceil_clip = vec![0i16; w];
    let mut floor_clip = vec![h as i16 - 1; w];

    let view = ViewParams {
        half_w: w as f32 * 0.5,
        half_h: h as f32 * 0.5,
        focal: cam.screen_scale(w),
        view_w: w,
    };

    recurse_node(
        level.bsp_root() as u16,
        level,
        cam,
        &mut out,
        &mut ClipBuffers {
            ceil: &mut ceil_clip,
            floor: &mut floor_clip,
        },
        &view,
    );

    // back-to-front sort
    out.sort_unstable_by(|a, b| {
        let za = 2.0 / (a.inv_z0 + a.inv_z1);
        let zb = 2.0 / (b.inv_z0 + b.inv_z1);
        zb.partial_cmp(&za).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn recurse_node(
    idx: u16,
    lvl: &Level,
    cam: &Camera,
    out: &mut Vec<DrawCall>,
    clips: &mut ClipBuffers,
    view: &ViewParams,
) {
    if idx & SUBSECTOR_BIT != 0 {
        draw_subsector(idx & CHILD_MASK, lvl, cam, out, clips, view);
    } else {
        let node = &lvl.nodes[idx as usize];
        let side = node.point_side(cam.pos().truncate()) as usize;
        let front = node.child[side];
        let back = node.child[side ^ 1];
        recurse_node(front, lvl, cam, out, clips, view);
        recurse_node(back, lvl, cam, out, clips, view);
    }
}

fn draw_subsector(
    ss_idx: u16,
    lvl: &Level,
    cam: &Camera,
    out: &mut Vec<DrawCall>,
    clips: &mut ClipBuffers,
    view: &ViewParams,
) {
    let cam_floor_z = get_floor_height(lvl, cam.pos().truncate());
    for seg_idx in lvl.segs_of_subsector(ss_idx) {
        let seg = &lvl.segs[seg_idx as usize];
        for span in compute_spans(lvl, cam, seg, view, cam_floor_z) {
            if span.is_visible(clips) {
                span.apply_clips(clips);
                out.push(span.to_drawcall());
            }
        }
    }
}

/// All screen‐space spans for a BSP segment (solid, upper portals, lower portals)
fn compute_spans(
    lvl: &Level,
    cam: &Camera,
    seg: &Seg,
    view: &ViewParams,
    cam_floor_z: f32,
) -> Vec<Span> {
    let mut spans = Vec::new();

    // 0. Back-face cull
    let v1 = lvl.vertices[seg.v1 as usize].pos;
    let v2 = lvl.vertices[seg.v2 as usize].pos;
    let wall = v2 - v1;
    let mut normal = vec2(wall.y, -wall.x);
    if seg.dir != 0 {
        normal = -normal;
    }
    if normal.dot(cam.pos().truncate() - v1) <= 0.0 {
        return spans;
    }

    // 1. Camera space
    let mut p1 = cam.to_cam(v1);
    let mut p2 = cam.to_cam(v2);
    if p1.y <= cam.near() && p2.y <= cam.near() {
        return spans;
    }

    // 2. Near-plane clip
    let mut t1 = 0.0f32;
    let mut t2 = 1.0f32;
    if p1.y < cam.near() {
        let t = (cam.near() - p1.y) / (p2.y - p1.y);
        p1 = p1 + (p2 - p1) * t;
        p1.y = cam.near();
        t1 = t;
    }
    if p2.y < cam.near() {
        let t = (cam.near() - p2.y) / (p1.y - p2.y);
        p2 = p2 + (p1 - p2) * t;
        p2.y = cam.near();
        t2 = 1.0 - t;
    }

    // 3. X projection
    let sx1 = view.half_w + p1.x * view.focal / p1.y;
    let sx2 = view.half_w + p2.x * view.focal / p2.y;
    if (sx1 < 0.0 && sx2 < 0.0) || (sx1 >= view.half_w * 2.0 && sx2 >= view.half_w * 2.0) {
        return spans;
    }
    let (sx1, sx2, p1, p2, t1, t2) = if sx1 > sx2 {
        (sx2, sx1, p2, p1, t2, t1)
    } else {
        (sx1, sx2, p1, p2, t1, t2)
    };
    let x_l = sx1.max(0.0) as i32;
    let x_r = sx2.min(view.view_w as f32 - 1.0) as i32;
    if x_l >= x_r {
        return spans;
    }

    // 3½. Interpolate edge params
    let span_w = (sx2 - sx1).max(1.0);
    let frac_l = (x_l as f32 - sx1) / span_w;
    let frac_r = (x_r as f32 - sx1) / span_w;
    let wall_len = wall.length();

    let ld = &lvl.linedefs[seg.linedef as usize];
    let (f_idx, b_idx) = if seg.dir == 0 {
        (ld.right_sidedef, ld.left_sidedef)
    } else {
        (ld.left_sidedef, ld.right_sidedef)
    };
    let sd_f = lvl.sidedefs[f_idx.unwrap() as usize].clone();
    let sec_f = &lvl.sectors[sd_f.sector as usize];
    let (have_back, sec_b) = if let Some(idx) = b_idx {
        if let Some(sd_b) = lvl.sidedefs.get(idx as usize) {
            (true, &lvl.sectors[sd_b.sector as usize])
        } else {
            (false, sec_f)
        }
    } else {
        (false, sec_f)
    };

    // perspective params
    let invz1 = 1.0 / p1.y;
    let invz2 = 1.0 / p2.y;
    let uoz1 = (sd_f.x_off as f32 + wall_len * t1) * invz1;
    let uoz2 = (sd_f.x_off as f32 + wall_len * t2) * invz2;
    let invz_l = invz1 + (invz2 - invz1) * frac_l;
    let invz_r = invz1 + (invz2 - invz1) * frac_r;
    let uoz_l = uoz1 + (uoz2 - uoz1) * frac_l;
    let uoz_r = uoz1 + (uoz2 - uoz1) * frac_r;

    // compute screen Ys for a given world height
    let make_yt = |world_h: f32, invz: f32| {
        view.half_h - (world_h - (cam_floor_z + cam.pos().z)) * view.focal * invz
    };

    let mut push = |tex, ceil_h, floor_h, kind| {
        // re‐project the *actual* top/bottom for this portal span:
        let yt_l = make_yt(ceil_h, invz_l);
        let yt_r = make_yt(ceil_h, invz_r);
        let yb_l = make_yt(floor_h, invz_l);
        let yb_r = make_yt(floor_h, invz_r);

        // compute per‐span slopes:
        let dt = (yt_r - yt_l) / span_w;
        let db = (yb_r - yb_l) / span_w;

        spans.push(Span {
            x_l: x_l as usize,
            x_r: x_r as usize,
            uoz_l,
            uoz_r,
            invz_l,
            invz_r,
            y_top_l: yt_l,
            y_top_r: yt_r,
            y_bot_l: yb_l,
            y_bot_r: yb_r,
            dt,
            db,
            tex,
            kind,
        });
    };

    if have_back && ld.flags.contains(LinedefFlags::TWO_SIDED) {
        if sec_b.ceil_h < sec_f.ceil_h {
            push(
                sd_f.upper,
                sec_f.ceil_h as f32,
                sec_b.ceil_h as f32,
                ClipKind::Upper,
            );
        }
        if sec_b.floor_h > sec_f.floor_h {
            push(
                sd_f.lower,
                sec_b.floor_h as f32,
                sec_f.floor_h as f32,
                ClipKind::Lower,
            );
        }
    } else {
        push(
            sd_f.middle,
            sec_f.ceil_h as f32,
            sec_f.floor_h as f32,
            ClipKind::Solid,
        );
    }

    spans
}

// Helper to locate the subsector containing a point
fn find_subsector_for_point(level: &Level, point: Vec2) -> usize {
    // Walk the BSP tree down to a subsector leaf
    let mut idx = level.bsp_root() as u16;
    loop {
        if idx & SUBSECTOR_BIT != 0 {
            return (idx & CHILD_MASK) as usize;
        }
        let node = &level.nodes[idx as usize];
        let side = node.point_side(point) as usize;
        idx = node.child[side];
    }
}

/// Once you know the subsector, grab its first seg → sidedef → sector.
fn get_floor_height(level: &Level, cam_xy: Vec2) -> f32 {
    let ss_idx = find_subsector_for_point(level, cam_xy);
    let ss = &level.subsectors[ss_idx];
    let seg = &level.segs[ss.first_seg as usize];
    let ld = &level.linedefs[seg.linedef as usize];

    let sd_index = if seg.dir == 0 {
        ld.right_sidedef
    } else {
        ld.left_sidedef
    }
    .expect("subsector seg must have a sidedef");
    let sd = &level.sidedefs[sd_index as usize];
    let sec = &level.sectors[sd.sector as usize];
    sec.floor_h as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::texture::NO_TEXTURE;

    /// Helper to create a Span for testing with constant Y extents and no slope
    fn make_test_span(x_l: usize, x_r: usize, y_top: f32, y_bot: f32, kind: ClipKind) -> Span {
        Span {
            x_l,
            x_r,
            uoz_l: 0.0,
            uoz_r: 0.0,
            invz_l: 1.0,
            invz_r: 1.0,
            y_top_l: y_top,
            y_top_r: y_top,
            y_bot_l: y_bot,
            y_bot_r: y_bot,
            dt: 0.0,
            db: 0.0,
            tex: NO_TEXTURE,
            kind,
        }
    }

    #[test]
    fn span_visible_within_clips() {
        let span = make_test_span(1, 3, 5.0, 10.0, ClipKind::Solid);
        let mut ceil = vec![0i16; 5];
        let mut floor = vec![20i16; 5];
        let clips = ClipBuffers {
            ceil: &mut ceil,
            floor: &mut floor,
        };
        assert!(span.is_visible(&clips));
    }

    #[test]
    fn span_not_visible_when_clipped() {
        let span = make_test_span(0, 2, 4.0, 8.0, ClipKind::Solid);
        let mut ceil = vec![9i16; 3];
        let mut floor = vec![20i16; 3];
        let clips = ClipBuffers {
            ceil: &mut ceil,
            floor: &mut floor,
        };
        assert!(!span.is_visible(&clips));
    }

    #[test]
    fn apply_clips_solid_updates_both() {
        let span = make_test_span(2, 4, 3.0, 12.0, ClipKind::Solid);
        let mut ceil = vec![0i16; 6];
        let mut floor = vec![20i16; 6];
        {
            let mut clips = ClipBuffers {
                ceil: &mut ceil,
                floor: &mut floor,
            };
            span.apply_clips(&mut clips);
        }
        for x in 2..=4 {
            assert_eq!(ceil[x], (span.y_bot_l.floor() as i16) + 1);
            assert_eq!(floor[x], (span.y_top_l.ceil() as i16) - 1);
        }
    }

    #[test]
    fn apply_clips_upper_only_updates_ceil() {
        let span = make_test_span(0, 1, 2.5, 9.5, ClipKind::Upper);
        let mut ceil = vec![0i16; 3];
        let mut floor = vec![20i16; 3];
        {
            let mut clips = ClipBuffers {
                ceil: &mut ceil,
                floor: &mut floor,
            };
            span.apply_clips(&mut clips);
        }
        for x in 0..=1 {
            assert_eq!(ceil[x], (span.y_bot_l.floor() as i16) + 1);
            assert_eq!(floor[x], 20i16);
        }
    }

    #[test]
    fn apply_clips_lower_only_updates_floor() {
        let span = make_test_span(0, 2, 4.2, 11.7, ClipKind::Lower);
        let mut ceil = vec![0i16; 4];
        let mut floor = vec![20i16; 4];
        {
            let mut clips = ClipBuffers {
                ceil: &mut ceil,
                floor: &mut floor,
            };
            span.apply_clips(&mut clips);
        }
        for x in 0..=2 {
            assert_eq!(floor[x], (span.y_top_l.ceil() as i16) - 1);
            assert_eq!(ceil[x], 0i16);
        }
    }
}
