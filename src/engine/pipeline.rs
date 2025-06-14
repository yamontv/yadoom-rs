//! -------------------------------------------------------------------------
//! BSP → [DrawCall] list for the **software column renderer**
//!
//! * Walks the BSP _front-to-back_ so overdraw can be culled later with a
//!   solid-column buffer.
//! * Emits **one DrawCall per visible wall section**
//!   (upper-portal, lower-portal, solid middle).
//! * Computes perspective-correct u/z once per edge.
//!
//! TODO (future work): visplanes, sprites, light-levels, unpegged flags
//! -------------------------------------------------------------------------
use glam::{Vec2, vec2};

use crate::{
    renderer::{ClipKind, DrawCall},
    world::{
        bsp::{CHILD_MASK, SUBSECTOR_BIT},
        camera::Camera,
        geometry::{Level, LinedefFlags},
        texture::TextureId,
    },
};

/// Bundles viewport parameters
struct ViewParams {
    half_w: f32,
    half_h: f32,
    focal: f32,
    view_w: usize,
    cam_floor_z: f32,
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

/*=======================================================================*/
/*                           Public entry                                */
/*=======================================================================*/

/// Build visibility-sorted wall spans.
///
/// w, h – viewport size in pixels (Y projection is done here so the
///            back-end loop stays a simple vertical span-drawer).
pub fn build_drawcalls(level: &Level, cam: &Camera, w: usize, h: usize) -> Vec<DrawCall> {
    let mut out = Vec::<DrawCall>::with_capacity(2048);

    let view = ViewParams {
        half_w: w as f32 * 0.5,
        half_h: h as f32 * 0.5,
        focal: cam.screen_scale(w),
        view_w: w,
        cam_floor_z: get_floor_height(level, cam.pos().truncate()),
    };

    recurse_node(level.bsp_root() as u16, level, cam, &mut out, &view);
    out
}

/*=======================================================================*/
/*                         BSP recursion                                 */
/*=======================================================================*/

fn recurse_node(child: u16, lvl: &Level, cam: &Camera, out: &mut Vec<DrawCall>, view: &ViewParams) {
    if child & SUBSECTOR_BIT != 0 {
        draw_subsector(child & CHILD_MASK, lvl, cam, out, view);
    } else {
        let node = &lvl.nodes[child as usize];
        let side = node.point_side(cam.pos().truncate()) as usize; // 0 front, 1 back
        let first = node.child[side]; // front first
        let back = node.child[side ^ 1]; // then far side
        recurse_node(first, lvl, cam, out, view);
        recurse_node(back, lvl, cam, out, view);
    }
}

/*=======================================================================*/
/*                          Leaf drawing                                 */
/*=======================================================================*/
/// Rasterise one BSP subsector into a set of wall-slice [DrawCall]s.
fn draw_subsector(
    ss_idx: u16,
    lvl: &Level,
    cam: &Camera,
    out: &mut Vec<DrawCall>,
    view: &ViewParams,
) {
    /*--------------------------------------------------------------*/
    for seg_idx in lvl.segs_of_subsector(ss_idx) {
        let seg = &lvl.segs[seg_idx as usize];

        /*----- 0.  back-face cull  --------------------------------------*/
        // Build the 2-D wall vector in *map* space.
        let a = lvl.vertices[seg.v1 as usize].pos;
        let b = lvl.vertices[seg.v2 as usize].pos;
        let wall = b - a; // points v1 → v2

        // The normal that should face the player is “wall rotated +90°”.
        // SEG::dir==0 ⇒ the *right* sidedef is the visible face,
        // so the outward normal is  ( dy , -dx ).
        // SEG::dir==1 ⇒ left side is visible  ⇒ flip normal.
        let mut n = vec2(wall.y, -wall.x); // right-hand normal
        if seg.dir != 0 {
            n = -n;
        } // flip for left-hand segs

        // If the normal points away (dot≤0) the wall is a back-face → skip.
        if n.dot(cam.pos().truncate() - a) <= 0.0 {
            continue;
        }

        /*-- 1. endpoints in camera space ---------------------------*/
        let v1 = lvl.vertices[seg.v1 as usize].pos;
        let v2 = lvl.vertices[seg.v2 as usize].pos;
        let mut p1 = cam.to_cam(v1);
        let mut p2 = cam.to_cam(v2);

        let near = cam.near() + 1e-3;

        if p1.y <= near && p2.y <= near {
            continue; // both behind the near-plane
        }

        /*-- 2. near-plane clip  (track t so we can compute tex-U) --*/
        let mut t1 = 0.0;
        let mut t2 = 1.0;
        if p1.y < cam.near() {
            let t = (cam.near() - p1.y) / (p2.y - p1.y);
            p1 += (p2 - p1) * t;
            p1.y = cam.near();
            t1 = t;
        }
        if p2.y < cam.near() {
            let t = (cam.near() - p2.y) / (p1.y - p2.y);
            p2 += (p1 - p2) * t;
            p2.y = cam.near();
            t2 = 1.0 - t;
        }

        /*-- 3. project to screen X --------------------------------*/
        let mut sx1 = view.half_w + p1.x * view.focal / p1.y;
        let mut sx2 = view.half_w + p2.x * view.focal / p2.y;

        if (sx1 < 0.0 && sx2 < 0.0) || (sx1 >= view.half_w * 2.0 && sx2 >= view.half_w * 2.0) {
            continue; // completely off-screen
        }

        if sx1 > sx2 {
            core::mem::swap(&mut sx1, &mut sx2);
            core::mem::swap(&mut p1, &mut p2);
            core::mem::swap(&mut t1, &mut t2);
        }

        let x_l = sx1.max(0.0) as i32;
        let x_r = sx2.min(view.view_w as f32 - 1.0) as i32;
        if x_l >= x_r {
            continue;
        }

        /*-- 3½. interpolate edge params at the clip limits ---------*/
        let span_full = sx2 - sx1;
        let frac_l = (x_l as f32 - sx1) / span_full;
        let frac_r = (x_r as f32 - sx1) / span_full;

        let wall_len = (v2 - v1).length();
        let ld = &lvl.linedefs[seg.linedef as usize];
        let (sd_f, sd_b) = if seg.dir == 0 {
            (ld.right_sidedef, ld.left_sidedef)
        } else {
            (ld.left_sidedef, ld.right_sidedef)
        };

        let sd_front = sd_f
            .and_then(|i| lvl.sidedefs.get(i as usize))
            .expect("front sidedef must exist");
        let sec_front = &lvl.sectors[sd_front.sector as usize];

        let (have_back, sec_back) = if let Some(idx) = sd_b {
            if let Some(sd) = lvl.sidedefs.get(idx as usize) {
                (true, &lvl.sectors[sd.sector as usize])
            } else {
                (false, sec_front)
            }
        } else {
            (false, sec_front)
        };

        /* perspective helpers shared by all spans in this slice */
        let invz_p1 = 1.0 / p1.y;
        let invz_p2 = 1.0 / p2.y;
        let uoz_p1 = (sd_front.x_off as f32 + wall_len * t1) * invz_p1;
        let uoz_p2 = (sd_front.x_off as f32 + wall_len * t2) * invz_p2;

        let invz_l = invz_p1 + (invz_p2 - invz_p1) * frac_l;
        let invz_r = invz_p1 + (invz_p2 - invz_p1) * frac_r;
        let uoz_l = uoz_p1 + (uoz_p2 - uoz_p1) * frac_l;
        let uoz_r = uoz_p1 + (uoz_p2 - uoz_p1) * frac_r;

        /*-------------- inner helper to push ONE span ----------------*/
        let mut push_span = |tex: TextureId, ceil_h: f32, floor_h: f32, how: ClipKind| {
            let eye_world_z = view.cam_floor_z + cam.pos().z;
            //  + sd_front.y_off as f32;

            /* 1. project to screen-Y */
            let y_top_l = view.half_h - (ceil_h - eye_world_z) * view.focal * invz_l;
            let y_top_r = view.half_h - (ceil_h - eye_world_z) * view.focal * invz_r;
            let y_bot_l = view.half_h - (floor_h - eye_world_z) * view.focal * invz_l;
            let y_bot_r = view.half_h - (floor_h - eye_world_z) * view.focal * invz_r;

            /* 3. finally emit the DrawCall */
            out.push(DrawCall {
                tex_id: tex,
                u0_over_z: uoz_l,
                u1_over_z: uoz_r,
                inv_z0: invz_l,
                inv_z1: invz_r,
                x_start: x_l,
                x_end: x_r,
                y_top0: y_top_l,
                y_top1: y_top_r,
                y_bot0: y_bot_l,
                y_bot1: y_bot_r,
                kind: how,
            });
        };

        /*-------------- decide which spans to draw ------------------*/
        if have_back && ld.flags.contains(LinedefFlags::TWO_SIDED) {
            /* upper portal */
            if sec_back.ceil_h < sec_front.ceil_h {
                push_span(
                    sd_front.upper,
                    sec_front.ceil_h as f32,
                    sec_back.ceil_h as f32,
                    ClipKind::Upper,
                );
            }
            /* lower portal */
            if sec_back.floor_h > sec_front.floor_h {
                push_span(
                    sd_front.lower,
                    sec_back.floor_h as f32,
                    sec_front.floor_h as f32,
                    ClipKind::Lower,
                );
            }
        } else {
            /* one-sided */
            push_span(
                sd_front.middle,
                sec_front.ceil_h as f32,
                sec_front.floor_h as f32,
                ClipKind::Solid,
            );
        }
    }
}

/*=======================================================================*/
/*                               Tests                                   */
/*=======================================================================*/
#[cfg(test)]
mod tests {
    use super::*;
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
    fn pipeline_produces_some_calls() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let mut bank = TextureBank::default_with_checker();
        let lvl = loader::load_level(&wad, wad.level_indices()[0], &mut bank).unwrap();

        // Spawn camera at thing type-1 (single-player start)
        let start = lvl
            .things
            .iter()
            .find(|t| t.type_id == 1)
            .expect("no player start")
            .pos;

        let cam = Camera::new(glam::Vec3::new(start.x, start.y, 41.0), 0.0, 1.57);

        assert!(
            !build_drawcalls(&lvl, &cam, 640, 400).is_empty(),
            "no walls were emitted"
        );
    }

    #[test]
    fn y_top_is_above_y_bot() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let mut bank = TextureBank::default_with_checker();
        let lvl = loader::load_level(&wad, wad.level_indices()[0], &mut bank).unwrap();
        let cam = Camera::new(glam::Vec3::new(0.0, 0.0, 41.0), 0.0, 1.57);
        for dc in build_drawcalls(&lvl, &cam, 640, 400) {
            assert!(dc.y_top0 <= dc.y_bot0 && dc.y_top1 <= dc.y_bot1);
        }
    }
}
