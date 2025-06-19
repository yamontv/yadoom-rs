//! ----------------------------------------------------------------------------
//! **BSP front‑to‑back traversal**
//!
//! Responsible for
//! * finding visible subsectors and their SEGs in **front‑to‑back** order
//! * handing every *front‑facing* SEG to `engine::walls::render_seg`
//!
//! It deliberately **does not** perform any of the following:
//! * clipping / span building (handled in `walls`)
//! * clip‑buffer maintenance (also `walls`)
//! * visplanes / sprites (todo)
//!
//! Keeping traversal and raster preparation separate means we can swap the
//! renderer backend (software, OpenGL, Vulkan…) and still reuse the BSP walk.
//! ----------------------------------------------------------------------------

use glam::{Vec2, vec2};

use crate::{
    engine::projection::project_seg,
    engine::types::{Screen, Viewer},
    engine::visplane::PlaneCollector,
    engine::walls::build_spans,
    renderer::{ClipBands, Renderer, Rgba},
    world::{
        bsp::{CHILD_MASK, SUBSECTOR_BIT},
        camera::Camera,
        geometry::Level,
        texture::TextureBank,
    },
};

/*──────────────────────────── Entry point ────────────────────────────*/

/// High‑level frame routine. The public signature stays unchanged so nothing
/// outside the pipeline has to be updated.
pub fn render_frame<R: Renderer>(
    renderer: &mut R,
    level: &Level,
    cam: &Camera,
    bank: &TextureBank,
    w: usize,
    h: usize,
    submit: impl FnOnce(&[Rgba], usize, usize),
) {
    // 1 ─ clear or re‑allocate the renderer’s scratch framebuffer
    renderer.begin_frame(w, h);

    // initialise plane collector
    let mut planes = PlaneCollector::new();
    planes.begin_frame(w, h);

    // 2 ─ allocate per‑column clip bands (fully open at start of frame)
    let mut ceil = vec![0_i32; w];
    let mut floor = vec![h as i32 - 1; w];
    let mut bands = ClipBands {
        ceil: &mut ceil,
        floor: &mut floor,
    };

    let screen = Screen {
        w,
        h,
        half_w: w as f32 * 0.5,
        half_h: h as f32 * 0.5,
    };

    let view = Viewer {
        focal: cam.screen_scale(w),
        eye_floor_z: floor_height_under_player(level, cam.pos().truncate()),
    };

    // 4 ─ BSP traversal (front‑to‑back)
    walk_bsp(
        level.bsp_root() as u16,
        level,
        cam,
        &screen,
        &view,
        &mut bands,
        renderer,
        bank,
        &mut planes,
    );

    // 5 ─ draw the flats *behind* everything else
    planes.draw_all(renderer, level, cam, &screen, &view, bank);

    // 6 ─ hand the filled frame to the caller (window, video encoder, …)
    renderer.end_frame(submit);
}

/*────────────────────────── BSP traversal ────────────────────────────*/

/// Recursively walk the BSP tree front‑to‑back.  For each subsector it
/// processes all SEGs that face the camera.
fn walk_bsp<R: Renderer>(
    child: u16,
    lvl: &Level,
    cam: &Camera,
    screen: &Screen,
    view: &Viewer,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
    planes: &mut PlaneCollector,
) {
    // Leaf? ──────
    if child & SUBSECTOR_BIT != 0 {
        draw_subsector(
            child & CHILD_MASK,
            lvl,
            cam,
            screen,
            view,
            bands,
            renderer,
            bank,
            planes,
        );
        return;
    }

    // Internal node ──────
    let node = &lvl.nodes[child as usize];
    let front = node.point_side(cam.pos().truncate()) as usize; // 0: front, 1: back
    let back = front ^ 1;

    // Near side first …
    walk_bsp(
        node.child[front],
        lvl,
        cam,
        screen,
        view,
        bands,
        renderer,
        bank,
        planes,
    );

    // … far side only if its bounding box might be visible.
    if bbox_visible(&node.bbox[back], cam, screen, view) {
        walk_bsp(
            node.child[back],
            lvl,
            cam,
            screen,
            view,
            bands,
            renderer,
            bank,
            planes,
        );
    }
}

/*────────────────────────── Leaf processing ──────────────────────────*/

fn draw_subsector<R: Renderer>(
    ss_idx: u16,
    lvl: &Level,
    cam: &Camera,
    screen: &Screen,
    view: &Viewer,
    bands: &mut ClipBands,
    renderer: &mut R,
    bank: &TextureBank,
    planes: &mut PlaneCollector,
) {
    /* --- draw all walls, track horizontal coverage ----------------------- */
    let mut min_x = screen.w as i32; // start outside valid range
    let mut max_x = -1i32;

    for seg_idx in lvl.segs_of_subsector(ss_idx) {
        // Back‑face cull in *world* space: if the viewer is on the back side
        // of the SEG’s plane, skip it.
        if back_facing(seg_idx, lvl, cam) {
            continue;
        }

        if let Some(edge) = project_seg(seg_idx, lvl, cam, screen, view) {
            build_spans(edge, lvl, cam, screen, view, bands, renderer, bank);
            min_x = min_x.min(edge.x_l);
            max_x = max_x.max(edge.x_r);
        }
    }

    /* --- nothing hit the screen?  then nothing to do --------------------- */
    if max_x < min_x {
        return;
    }

    // ---------------- sector of this subsector ----------------
    let ss = &lvl.subsectors[ss_idx as usize];
    let seg0 = &lvl.segs[ss.first_seg as usize];
    let ld0 = &lvl.linedefs[seg0.linedef as usize];

    // pick the front-facing sidedef of seg0 (right for dir==0, else left)
    let sd_idx = if seg0.dir == 0 {
        ld0.right_sidedef
    } else {
        ld0.left_sidedef
    }
    .expect("first SEG of subsector must have a sidedef");

    let sec = &lvl.sectors[lvl.sidedefs[sd_idx as usize].sector as usize];

    // On-screen horizontal extent of the subsector (reuse the min/max from SEGs)
    // let (min_x, max_x) = /* ← computed while iterating SEGs, store in locals */;

    planes.add_subsector_plane(
        (sec.floor_h as f32) - view.eye_floor_z, // dz below eye
        sec.floor_tex,
        true,
        bands,
        min_x..=max_x,
    );
    planes.add_subsector_plane(
        view.eye_floor_z - (sec.ceil_h as f32), // dz above eye
        sec.ceil_tex,
        false,
        bands,
        min_x..=max_x,
    );
}

/*──────────────────────────── Utilities ───────────────────────────────*/

/// True if the viewer is on the back side of `seg_idx`.
fn back_facing(seg_idx: u16, lvl: &Level, cam: &Camera) -> bool {
    let seg = &lvl.segs[seg_idx as usize];
    let a = lvl.vertices[seg.v1 as usize].pos;
    let b = lvl.vertices[seg.v2 as usize].pos;
    let wall = b - a;
    let mut n = vec2(wall.y, -wall.x); // right‑hand normal
    if seg.dir != 0 {
        n = -n;
    } // flip for left‑hand SEGs
    n.dot(cam.pos().truncate() - a) <= 0.0 // ≤0 ⇒ viewer behind plane
}

/// Conservative check whether `bbox` could appear on the screen.
/// Re‑implementation of the original DOOM angle maths using projection.
fn bbox_visible(bbox: &[i16; 4], cam: &Camera, screen: &Screen, view: &Viewer) -> bool {
    // Doom stores (top, bottom, left, right) – convert & normalise.
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
    let mut max_sx = f32::NEG_INFINITY;
    let mut any_in_front = false;

    for (ix, iy) in CORNERS {
        let p_world = vec2(if ix == 0 { x1 } else { x2 }, if iy == 0 { y1 } else { y2 });
        let p_cam = cam.to_cam(p_world);
        if p_cam.y <= near {
            continue;
        } // behind near plane
        any_in_front = true;
        let sx = screen.half_w + p_cam.x * view.focal / p_cam.y;
        min_sx = min_sx.min(sx);
        max_sx = max_sx.max(sx);
    }

    if !any_in_front {
        return false;
    } // box completely behind
    if max_sx < 0.0 || min_sx >= screen.w as f32 {
        return false;
    } // off‑screen
    true
}

/*────────────────── Helpers for view‑space constants ──────────────────*/

/// Return the floor height (Z) of the sector the player is currently in.
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
    .expect("subsector SEG must have a sidedef");
    let sector = &level.sectors[level.sidedefs[sd_idx as usize].sector as usize];
    sector.floor_h as f32
}

/// Walk the BSP until we hit a subsector leaf that contains `pos`.
fn find_subsector(level: &Level, pos: Vec2) -> usize {
    let mut idx = level.bsp_root() as u16;
    loop {
        if idx & SUBSECTOR_BIT != 0 {
            return (idx & CHILD_MASK) as usize;
        }
        let node = &level.nodes[idx as usize];
        let side = node.point_side(pos) as usize;
        idx = node.child[side];
    }
}
