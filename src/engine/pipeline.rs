//! ---------------------------------------------------------------------------
//! BSP → DrawCalls for the software column renderer
//!
//! * Walks the BSP **front-to-back** so the later span-drawer can cull overdraw
//!   with a per-column clip buffer.
//! * Emits **one DrawCall per wall span** (upper portal, lower portal, solid).
//! * Computes perspective-correct u/z once per edge.
//!
//! TODO: visplanes, sprites, lighting tables
//! ---------------------------------------------------------------------------

use glam::{Vec2, vec2};

use crate::{
    renderer::{ClipBands, ClipKind, Renderer, Rgba, WallSpan},
    world::{
        bsp::{CHILD_MASK, SUBSECTOR_BIT},
        camera::Camera,
        geometry::{Level, LinedefFlags},
        texture::{TextureBank, TextureId},
    },
};

/*──────────────────────────── View helpers ───────────────────────────*/

/// Everything the renderer needs to turn world units into screen pixels.
#[derive(Clone, Copy)]
struct ViewParams {
    half_w: f32,
    half_h: f32,
    focal: f32,
    view_w: usize,
    eye_floor_z: f32, // player’s Z on the floor under their feet
}

/*──────────────────────────── Entry point ────────────────────────────*/
pub fn render_frame<R: Renderer>(
    renderer: &mut R,
    level: &Level,
    cam: &Camera,
    bank: &TextureBank,
    w: usize,
    h: usize,
    submit: impl FnOnce(&[Rgba], usize, usize),
) {
    renderer.begin_frame(w, h);

    let mut ceil_clip = vec![0_i32; w];
    let mut floor_clip = vec![h as i32 - 1; w];
    let mut bands = ClipBands {
        ceil: &mut ceil_clip,
        floor: &mut floor_clip,
    };

    let view = ViewParams {
        half_w: w as f32 * 0.5,
        half_h: h as f32 * 0.5,
        focal: cam.screen_scale(w),
        view_w: w,
        eye_floor_z: floor_height_under_player(level, cam.pos().truncate()),
    };

    walk_bsp(
        level.bsp_root() as u16,
        level,
        cam,
        &view,
        &mut bands,
        renderer,
        bank,
    );

    renderer.end_frame(submit);
}

/*──────────────────────────── BSP traversal ──────────────────────────*/

fn walk_bsp<R: Renderer>(
    child: u16,
    lvl: &Level,
    cam: &Camera,
    view: &ViewParams,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
) {
    if child & SUBSECTOR_BIT != 0 {
        draw_subsector(child & CHILD_MASK, lvl, cam, view, bands, renderer, bank);
    } else {
        let node = &lvl.nodes[child as usize];
        let front = node.point_side(cam.pos().truncate()) as usize;
        let back = front ^ 1;

        walk_bsp(node.child[front], lvl, cam, view, bands, renderer, bank); // near

        if bbox_visible(&node.bbox[back], cam, view) {
            walk_bsp(node.child[back], lvl, cam, view, bands, renderer, bank); // far
        }
    }
}

/// Fast conservative test: returns **true** if any part of `bbox`
/// can project inside the screen rectangle.
///
/// Doom’s original uses angle tables; here we re-implement it by
/// transforming the four corners into camera space and checking their
/// projected X range.
fn bbox_visible(bbox: &[i16; 4], cam: &Camera, view: &ViewParams) -> bool {
    // Doom stores (top, bottom, left, right) – convert to floats
    let (mut x1, mut x2) = (bbox[2] as f32, bbox[3] as f32); // left, right
    let (mut y1, mut y2) = (bbox[1] as f32, bbox[0] as f32); // bottom, top
    if x1 > x2 {
        core::mem::swap(&mut x1, &mut x2);
    }
    if y1 > y2 {
        core::mem::swap(&mut y1, &mut y2);
    }

    const CORNERS: [(usize, usize); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];
    let near = cam.near();
    let mut min_sx = f32::INFINITY;
    let mut max_sx = -f32::INFINITY;
    let mut any_in_front = false;

    for (ix, iy) in CORNERS {
        let p_world = vec2(if ix == 0 { x1 } else { x2 }, if iy == 0 { y1 } else { y2 });
        let p_cam = cam.to_cam(p_world);
        if p_cam.y <= near {
            continue;
        } // behind near plane
        any_in_front = true;
        let sx = view.half_w + p_cam.x * view.focal / p_cam.y;
        min_sx = min_sx.min(sx);
        max_sx = max_sx.max(sx);
    }
    if !any_in_front {
        return false;
    } // whole box behind us
    // off-screen to the left or right?
    if max_sx < 0.0 || min_sx >= view.view_w as f32 {
        return false;
    }
    true
}

/*───────────────────────── subsector → spans ─────────────────────────*/

fn draw_subsector<R: Renderer>(
    ss_idx: u16,
    lvl: &Level,
    cam: &Camera,
    view: &ViewParams,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
) {
    for seg_idx in lvl.segs_of_subsector(ss_idx) {
        if back_facing(seg_idx, lvl, cam) {
            continue;
        }

        if let Some(edge) = project_seg(seg_idx, lvl, cam, view) {
            build_spans(edge, lvl, cam, view, bands, renderer, bank);
        }
    }
}

/*──────────────────────── back-face cull ─────────────────────────────*/

fn back_facing(seg_idx: u16, lvl: &Level, cam: &Camera) -> bool {
    let seg = &lvl.segs[seg_idx as usize];
    let a = lvl.vertices[seg.v1 as usize].pos;
    let b = lvl.vertices[seg.v2 as usize].pos;
    let wall = b - a;
    let mut n = vec2(wall.y, -wall.x); // right-hand normal
    if seg.dir != 0 {
        n = -n;
    } // flip for left-hand segs
    n.dot(cam.pos().truncate() - a) <= 0.0 // ≤ 0 ⇒ facing away
}

/*───────────────────── Geometry → screen X band ─────────────────────*/

/// Everything the clipping / span builder needs for one visible edge.
struct Edge {
    x_l: i32,
    x_r: i32,
    invz_l: f32,
    invz_r: f32,
    uoz_l: f32,
    uoz_r: f32,
    seg_idx: u16,
}

fn project_seg(seg_idx: u16, lvl: &Level, cam: &Camera, view: &ViewParams) -> Option<Edge> {
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
    let mut sx1 = view.half_w + p1.x * view.focal / p1.y;
    let mut sx2 = view.half_w + p2.x * view.focal / p2.y;
    if (sx1 < 0.0 && sx2 < 0.0) || (sx1 >= view.half_w * 2.0 && sx2 >= view.half_w * 2.0) {
        return None; // completely off-screen
    }

    // Ensure  p1 → p2 is left → right in screen space
    if sx1 > sx2 {
        core::mem::swap(&mut sx1, &mut sx2);
        core::mem::swap(&mut p1, &mut p2);
        core::mem::swap(&mut t1, &mut t2);
    }

    let x_l = sx1.max(0.0) as i32;
    let x_r = sx2.min(view.view_w as f32 - 1.0) as i32;
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

/*────────────────────── span construction helpers ───────────────────*/
#[derive(Clone, Copy)]
struct ColumnStep {
    duoz: f32,
    dinvz: f32,
    dyt: f32,
    dyb: f32,
}

#[derive(Clone, Copy)]
struct ColumnCursor {
    uoz: f32,
    invz: f32,
    y_t: f32,
    y_b: f32,
}

impl ColumnStep {
    fn from_span(s: &WallSpan) -> Self {
        let w = (s.x_end - s.x_start).max(1) as f32;
        Self {
            duoz: (s.u1_over_z - s.u0_over_z) / w,
            dinvz: (s.inv_z1 - s.inv_z0) / w,
            dyt: (s.y_top1 - s.y_top0) / w,
            dyb: (s.y_bot1 - s.y_bot0) / w,
        }
    }
}

impl ColumnCursor {
    fn from_span(s: &WallSpan) -> Self {
        Self {
            uoz: s.u0_over_z,
            invz: s.inv_z0,
            y_t: s.y_top0,
            y_b: s.y_bot0,
        }
    }
    #[inline]
    fn step(&mut self, d: &ColumnStep) {
        self.uoz += d.duoz;
        self.invz += d.dinvz;
        self.y_t += d.dyt;
        self.y_b += d.dyb;
    }
}

pub fn emit_and_clip<R: Renderer>(
    proto: &WallSpan,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
) {
    // 1 ─── draw first, while bands still contain the old limits
    renderer.draw_wall(proto, bands, bank);

    // 2 ─── now update bands for every column that was really drawn
    let step = ColumnStep::from_span(proto);
    let mut cur = ColumnCursor::from_span(proto);

    for x in proto.x_start..=proto.x_end {
        let col = x as usize;

        // remember the *old* limits before we overwrite them
        let old_ceil = bands.ceil[col];
        let old_floor = bands.floor[col];

        // part of the wall that was visible in this column
        let y0 = cur.y_t.max(old_ceil as f32).ceil() as i32;
        let y1 = cur.y_b.min(old_floor as f32).floor() as i32;

        if y0 <= y1 {
            match proto.kind {
                ClipKind::Solid => {
                    bands.ceil[col] = (y1 as i32).saturating_add(1);
                    bands.floor[col] = (y0 as i32).saturating_sub(1);
                }
                ClipKind::Upper => bands.ceil[col] = (y1 as i32).saturating_add(1),
                ClipKind::Lower => bands.floor[col] = (y0 as i32).saturating_sub(1),
            }
        }

        cur.step(&step);
    }
}

fn build_spans<R: Renderer>(
    edge: Edge,
    lvl: &Level,
    cam: &Camera,
    view: &ViewParams,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
) {
    // Resolve sidedefs / sectors ------------------------------------------------
    let seg = &lvl.segs[edge.seg_idx as usize];
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

    // Closure that pushes ONE vertical span ------------------------------------
    let eye_z = view.eye_floor_z + cam.pos().z;
    let mut push = |tex: TextureId, ceil_h: f32, floor_h: f32, kind: ClipKind| {
        let wall_h = (ceil_h - floor_h).abs();
        let tm_mu = texturemid(
            kind,
            ld.flags,
            ceil_h,
            floor_h,
            eye_z,
            sd_front.y_off as f32,
        );

        emit_and_clip(
            &WallSpan {
                /* projection */
                tex_id: tex,
                u0_over_z: edge.uoz_l,
                u1_over_z: edge.uoz_r,
                inv_z0: edge.invz_l,
                inv_z1: edge.invz_r,
                x_start: edge.x_l,
                x_end: edge.x_r,
                y_top0: view.half_h - (ceil_h - eye_z) * view.focal * edge.invz_l,
                y_top1: view.half_h - (ceil_h - eye_z) * view.focal * edge.invz_r,
                y_bot0: view.half_h - (floor_h - eye_z) * view.focal * edge.invz_l,
                y_bot1: view.half_h - (floor_h - eye_z) * view.focal * edge.invz_r,
                kind,
                /* tiling */
                wall_h,
                texturemid_mu: tm_mu,
            },
            bands,
            renderer,
            bank,
        );
    };

    // Decide which spans to draw -----------------------------------------------
    if have_back && ld.flags.contains(LinedefFlags::TWO_SIDED) {
        // ─ upper portal
        if sec_back.ceil_h < sec_front.ceil_h {
            push(
                sd_front.upper,
                sec_front.ceil_h as f32,
                sec_back.ceil_h as f32,
                ClipKind::Upper,
            );
        }
        // ─ lower portal
        if sec_back.floor_h > sec_front.floor_h {
            push(
                sd_front.lower,
                sec_back.floor_h as f32,
                sec_front.floor_h as f32,
                ClipKind::Lower,
            );
        }
    } else {
        // ─ one-sided wall
        push(
            sd_front.middle,
            sec_front.ceil_h as f32,
            sec_front.floor_h as f32,
            ClipKind::Solid,
        );
    }
}

/*────────────────── vertical-pegging (vanilla Doom) ──────────────────*/

fn texturemid(
    kind: ClipKind,
    flags: LinedefFlags,
    ceil_h: f32,
    floor_h: f32,
    eye_z: f32,
    y_off: f32,
) -> f32 {
    match kind {
        // Mid texture: peg to ceiling unless LOWER_UNPEGGED
        ClipKind::Solid => {
            if flags.contains(LinedefFlags::LOWER_UNPEGGED) {
                (floor_h - eye_z) + y_off
            } else {
                (ceil_h - eye_z) + y_off
            }
        }
        // Upper portal: peg to ceiling unless UPPER_UNPEGGED
        ClipKind::Upper => {
            if flags.contains(LinedefFlags::UPPER_UNPEGGED) {
                (floor_h - eye_z) + y_off
            } else {
                (ceil_h - eye_z) + y_off
            }
        }
        // Lower portal: peg to floor unless UPPER_UNPEGGED
        ClipKind::Lower => {
            if flags.contains(LinedefFlags::UPPER_UNPEGGED) {
                (ceil_h - eye_z) + y_off
            } else {
                (floor_h - eye_z) + y_off
            }
        }
    }
}

/*──────────────────── floor height under the player ──────────────────*/

/// Find which subsector the player stands in and return its sector’s floor Z.
fn floor_height_under_player(level: &Level, pos: Vec2) -> f32 {
    let ss_idx = find_subsector(level, pos);
    let ss = &level.subsectors[ss_idx];
    let seg = &level.segs[ss.first_seg as usize];
    let ld = &level.linedefs[seg.linedef as usize];
    let sd_idx = if seg.dir == 0 {
        ld.right_sidedef
    } else {
        ld.left_sidedef
    }
    .expect("subsector seg must have a sidedef");
    let sector = &level.sectors[level.sidedefs[sd_idx as usize].sector as usize];
    sector.floor_h as f32
}

fn find_subsector(level: &Level, node_idx: Vec2) -> usize {
    // BSP walk until we hit a subsector leaf
    let mut idx = level.bsp_root() as u16;
    loop {
        if idx & SUBSECTOR_BIT != 0 {
            return (idx & CHILD_MASK) as usize;
        }
        let node = &level.nodes[idx as usize];
        let side = node.point_side(node_idx) as usize;
        idx = node.child[side];
    }
}
