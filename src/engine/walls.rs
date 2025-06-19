use crate::{
    engine::types::{Edge, Screen, Viewer},
    renderer::{ClipBands, Renderer, WallSpan},
    world::{
        camera::Camera,
        geometry::{Level, LinedefFlags},
        texture::{TextureBank, TextureId},
    },
};

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

/// Tells the draw routine whether this is a solid wall slice,
/// an upper-portal slice (ceiling of back sector), or a lower-portal slice.
#[derive(Clone, Copy)]
enum ClipKind {
    Solid,
    Upper,
    Lower,
}

fn emit_and_clip<R: Renderer>(
    proto: &WallSpan,
    kind: ClipKind,
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
            match kind {
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

pub fn build_spans<R: Renderer>(
    edge: Edge,
    lvl: &Level,
    cam: &Camera,
    screen: &Screen,
    view: &Viewer,
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
                y_top0: screen.half_h - (ceil_h - eye_z) * view.focal * edge.invz_l,
                y_top1: screen.half_h - (ceil_h - eye_z) * view.focal * edge.invz_r,
                y_bot0: screen.half_h - (floor_h - eye_z) * view.focal * edge.invz_l,
                y_bot1: screen.half_h - (floor_h - eye_z) * view.focal * edge.invz_r,
                /* tiling */
                wall_h,
                texturemid_mu: tm_mu,
            },
            kind,
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
