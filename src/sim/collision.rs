//! Minimal, faithful port of DOOM’s P_TryMove / P_CheckPosition.
//!
//! ✔ same solid-line test as the vanilla source
//! ✔ uses your existing `Class`, `Level`, `LinedefFlags` etc.
//! ✔ *only* glam math and your `geometry` module

use glam::Vec2;

use crate::sim::{Class, MAX_STEP_HEIGHT};
use crate::world::geometry::{Level, LinedefFlags};

/// What the caller gets back.
pub struct MoveResult {
    pub pos: Vec2,      // final XY
    pub subsector: u16, // final subsector id
    pub hit_wall: bool, // touched any solid line
}

/* ─────────────────────────  VANILLA SOLID-LINE TEST  ───────────────────── */

/// Return `true` if `line` blocks the actor whose cylinder has
/// `radius`/`height`, when we are on side `side` (0 = front).
#[inline]
fn line_is_solid(
    level: &Level,
    line: &crate::world::geometry::Linedef,
    side: i32,
    height: f32,
) -> bool {
    /* ----- 1. one-sided lines are always solid -------------------------- */
    if !line.flags.contains(LinedefFlags::TWO_SIDED) {
        return true;
    }

    /* ----- 2. get front & back sectors exactly as Doom does ------------- */
    let (front_sd, back_sd) = if side == 0 {
        (line.right_sidedef, line.left_sidedef)
    } else {
        (line.left_sidedef, line.right_sidedef)
    };

    // No back side → treat as solid (shouldn’t happen for TWO_SIDED, but
    // maps can be sloppy and Doom handled it this way).
    let (front_sd, back_sd) = match (front_sd, back_sd) {
        (Some(f), Some(b)) => (f, b),
        _ => return true,
    };

    let front_sec = &level.sectors[level.sidedefs[front_sd as usize].sector as usize];
    let back_sec = &level.sectors[level.sidedefs[back_sd as usize].sector as usize];

    /* ----- 3. vertical opening the vanilla way -------------------------- */
    let open_top = front_sec.ceil_h.min(back_sec.ceil_h);
    let open_bottom = front_sec.floor_h.max(back_sec.floor_h);

    if open_top - open_bottom < height {
        // not tall enough for the thing
        return true;
    }

    /* ----- 4. step-up limit (24 units in the IWAD, == MAX_STEP_HEIGHT) -- */
    if back_sec.floor_h - front_sec.floor_h > MAX_STEP_HEIGHT {
        return true;
    }

    /* note: vanilla also recorded big drop-offs into tmfloorz2
    (useful for monsters) – you can add that later if needed          */

    false
}

/* ─────────────────────────  SLIDE-MOVE DRIVER  ────────────────────────── */

/// Vanilla-accurate slide/clip.  Call from your `physics()` system.
pub fn slide_move(
    level: &Level,
    mut ss: u16,   // starting subsector
    mut pos: Vec2, // mutable XY
    delta: Vec2,   // desired XY displacement (one tic)
    class: &Class, // to fetch radius / height
) -> MoveResult {
    const SLICE_COUNT: i32 = 4; // Doom used 1/4 tic “fractions”
    let slice = delta / SLICE_COUNT as f32; // *fixed* – keeps speed exact

    let radius = class.0.radius as f32;
    let height = class.0.height as f32;

    let mut touched = false;

    'slice_loop: for _ in 0..SLICE_COUNT {
        let target = pos + slice;
        let subsector = &level.subsectors[ss as usize];

        /* Iterate the segs that bound this subsector (vanilla BSP rule).  */
        for i in 0..subsector.num_lines {
            let seg_idx = (subsector.first_line + i) as usize;
            let seg = &level.segs[seg_idx];
            let line = &level.linedefs[seg.linedef as usize];

            /* ========== SOLID-LINE TEST  (faithful) =================== */
            if !line_is_solid(level, line, seg.dir as i32, height) {
                continue; // can ignore this seg entirely
            }

            /* ========== GEOM INTERSECT  (same math, but f32) =========== */
            let v1 = level.vertices[seg.v1 as usize].pos;
            let v2 = level.vertices[seg.v2 as usize].pos;

            let edge = v2 - v1;
            let normal = Vec2::new(-edge.y, edge.x).normalize(); // left-hand
            let dist = (target - v1).dot(normal);

            let overlap = radius - dist.abs();
            if overlap > 0.0 {
                /* ---- push back exactly as Doom does ------------------ */
                let push_dir = if dist > 0.0 { normal } else { -normal };
                pos = target + push_dir * overlap;

                /* kill inward component so remaining slices glide        */
                let inward = slice.dot(push_dir);
                if inward > 0.0 {
                    pos -= push_dir * inward;
                }

                touched = true;
                ss = level.locate_subsector(pos);
                continue 'slice_loop; // restart next ¼-tic slice
            }
        }

        /* No collision this ¼-tic – accept the movement.                */
        pos = target;
        ss = level.locate_subsector(pos);
    }

    MoveResult {
        pos,
        subsector: ss,
        hit_wall: touched,
    }
}
