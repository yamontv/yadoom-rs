use crate::{
    renderer::software::Software,
    world::{
        camera::Camera,
        texture::{NO_TEXTURE, TextureBank, TextureId},
    },
};
use glam::Vec2;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::RangeInclusive;

pub type VisplaneId = u16;

pub const NO_PLANE: VisplaneId = u16::MAX;

#[derive(Clone)]
pub struct VisPlane {
    pub height: i16,
    pub tex: TextureId,
    pub light: i16,

    /// Inclusive horizontal range that the plane touches.
    pub min_x: u16,
    pub max_x: u16,

    /// For every screen column we remember the highest and lowest pixel that is
    /// still uncovered **after** drawing the front geometry.
    pub top: Vec<u16>,
    pub bottom: Vec<u16>,

    pub modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PlaneKey {
    height: i16,
    tex: TextureId,
    light: i16,
}

#[derive(Default)]
pub struct PlaneMap {
    map: HashMap<PlaneKey, Vec<VisplaneId>>,
    planes: Vec<VisPlane>,
    width: usize,
}

impl PlaneMap {
    pub fn clear(&mut self, width: usize) {
        self.map.clear();
        self.planes.clear();
        self.width = width;
    }

    pub fn get(&mut self, id: VisplaneId) -> Option<&mut VisPlane> {
        if id == NO_PLANE {
            None
        } else {
            self.planes.get_mut(id as usize)
        }
    }

    #[inline]
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &VisPlane> + '_ {
        self.planes.iter()
    }

    pub fn find(
        &mut self,
        height: i16,
        tex: TextureId,
        light: i16,
        min_x: u16,
        max_x: u16,
    ) -> VisplaneId {
        let key = PlaneKey { height, tex, light };

        let ids = match self.map.entry(key) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => v.insert(Vec::new()),
        };

        for &pid in ids.iter() {
            let plane = &mut self.planes[pid as usize];
            if Self::merge_plane(plane, min_x, max_x) {
                return pid;
            }
        }

        assert!(self.planes.len() < u16::MAX as usize);

        let new_id = self.planes.len() as VisplaneId;

        let new_plane = VisPlane {
            height,
            tex,
            light,
            min_x,
            max_x,
            top: vec![u16::MAX; self.width],
            bottom: vec![u16::MIN; self.width],
            modified: false,
        };

        self.planes.push(new_plane);
        ids.push(new_id);
        new_id
    }

    fn merge_plane(plane: &mut VisPlane, min_x: u16, max_x: u16) -> bool {
        let intrl = min_x.max(plane.min_x);
        let intrh = max_x.min(plane.max_x);
        let unionl = min_x.min(plane.min_x);
        let unionh = max_x.max(plane.max_x);

        let lo = intrl as usize;
        let hi = intrh as usize;

        if lo <= hi && plane.top[lo..=hi].iter().any(|&v| v != u16::MAX) {
            return false; // part of the span already drawn
        }

        plane.min_x = unionl;
        plane.max_x = unionh;

        // use the same one
        true
    }
}

/// (u, v) coordinate *at the current pixel*.
#[derive(Copy, Clone)]
struct UVCursor {
    u: f32,
    v: f32,
}

/// Δ(u, v) when you advance exactly one screen-pixel to the right.
#[derive(Copy, Clone)]
struct UVStep {
    du: f32,
    dv: f32,
}

impl UVCursor {
    /// Advance the cursor one screen pixel to the right.
    #[inline(always)]
    fn advance(&mut self, s: &UVStep) {
        self.u += s.du;
        self.v += s.dv;
    }
}

/// Context shared by all spans rendered in a single `flush_planes()` call.
///
/// Bundling these fields lets us avoid the long parameter lists that triggered
/// Clippy's `too_many_arguments` lint while keeping the code easy to read.
struct SpanContext<'a> {
    cam_fwd: Vec2,
    cam_right: Vec2,
    cam_base: Vec2,
    bank: &'a TextureBank,
}

/// All data required to draw a single horizontal span of a visplane.
struct PlaneDrawParams {
    tex_id: TextureId,
    light: i16,
    y_row: u16,
    x_range: RangeInclusive<u16>,
    step: UVStep,
    cursor: UVCursor,
}

impl Software {
    /// Draw and clear all cached visplanes.
    ///
    /// *Public signature unchanged – internal helpers were refactored to keep
    ///  argument lists short and Clippy‑friendly.*
    pub fn flush_planes(&mut self, cam: &Camera, bank: &TextureBank) {
        let cam_fwd = cam.forward();
        let cam_right = cam.right();
        let cam_base = cam.pos.truncate();

        let ctx = SpanContext {
            cam_fwd,
            cam_right,
            cam_base,
            bank,
        };

        // Retrieve and replace the plane map so we can iterate without
        // borrowing issues.
        let plane_map = std::mem::take(&mut self.visplane_map);

        // The original Doom drew floors & ceilings *after* the walls, so we
        // simply iterate as we stored them (front parts first = back parts last).
        for vp in plane_map.iter() {
            if vp.tex == NO_TEXTURE || !vp.modified {
                continue;
            }

            for y in 0..self.height as u16 {
                // Track the start of a run (inclusive) while scanning the row.
                let mut run_start: Option<u16> = None;

                for x in vp.min_x..=vp.max_x {
                    let col = x as usize;
                    let inside = vp.top[col] <= y && vp.bottom[col] >= y;

                    match (inside, run_start) {
                        (true, None) => run_start = Some(x), // run starts
                        (false, Some(xs)) => {
                            // run ends *before* this x
                            self.emit_span(&ctx, vp, y, xs..=x - 1);
                            run_start = None;
                        }
                        _ => {}
                    }
                }

                // Tail‑run up to the visplane's right edge
                if let Some(xs) = run_start.take() {
                    self.emit_span(&ctx, vp, y, xs..=vp.max_x);
                }
            }
        }

        // Put the (now cleared) map back so the rest of the engine can keep
        // using the same allocation.
        self.visplane_map = plane_map;
    }

    /// Convert a horizontal pixel run into a perspective‑correct span and hand
    /// it over to the inner draw routine.
    #[inline(always)]
    fn emit_span(
        &mut self,
        ctx: &SpanContext,
        vp: &VisPlane,
        y: u16,
        x_range: RangeInclusive<u16>,
    ) {
        // signed quantities ----------------------------------------------------
        let plane_height = vp.height as f32 - self.view_z; // <0 floor, >0 ceil
        let dy = (y as f32 + 0.5) - self.half_h; // <0 upper half, >0 lower
        let inv_dy = 1.0 / dy; // signed
        let ratio = plane_height * inv_dy; // signed  (key!)

        // positive distance along view direction ------------------------------
        let z = self.focal * ratio.abs(); // == |plane_h| * f / |dy|

        // screen-space helpers -------------------------------------------------
        let x_start = *x_range.start() as f32;
        let x_end = *x_range.end() as f32;

        let left_scr = (x_start + 0.5) - self.half_w;
        let right_scr = (x_end + 0.5) - self.half_w;
        let w_px = (x_end - x_start).max(1.0);
        let step_scr = (right_scr - left_scr) / w_px;

        // world position at the left edge of the span -------------------------
        let base = ctx.cam_base
            + ctx.cam_fwd * z // forward component
            + ctx.cam_right * (left_scr * ratio); // **signed** lateral shift

        // world-space step per pixel along X ----------------------------------
        let d_world = ctx.cam_right * (step_scr * ratio); // **signed**

        // endpoints -----------------------------------------------------------
        let world_left = base;
        let world_right = base + d_world * w_px;

        // Δ texture U,V per screen pixel – pre-multiplied so the inner loop needs
        // just one fused‑add operation.
        let step = UVStep {
            du: (world_right.x - world_left.x) / w_px,
            dv: (world_right.y - world_left.y) / w_px,
        };

        let cursor = UVCursor {
            u: world_left.x,
            v: world_left.y,
        };

        let params = PlaneDrawParams {
            tex_id: vp.tex,
            light: vp.light,
            y_row: y,
            x_range,
            step,
            cursor,
        };

        self.draw_plane(ctx, params);
    }

    #[inline(always)]
    fn draw_plane(&mut self, ctx: &SpanContext, params: PlaneDrawParams) {
        let tex = ctx
            .bank
            .texture(params.tex_id)
            .unwrap_or_else(|_| ctx.bank.texture(NO_TEXTURE).unwrap());

        // Row in the scratch buffer for this scanline
        let row_idx = params.y_row as usize * self.width;
        let row = &mut self.scratch[row_idx..][..self.width];

        let base_sh = (255u16.saturating_sub(params.light as u16) >> 3) as u8;

        debug_assert!(
            tex.w.is_power_of_two() && tex.h.is_power_of_two(),
            "textures must be POT"
        );

        let u_mask = (tex.w - 1) as i32;
        let v_mask = (tex.h - 1) as i32;

        let mut cursor = params.cursor;
        let step = params.step;

        for x in params.x_range.clone() {
            let u = ((cursor.u as i32) & u_mask) as usize;
            let v = ((cursor.v as i32) & v_mask) as usize;
            let col = tex.pixels[v * tex.w + u];

            row[x as usize] = ctx.bank.get_color(base_sh, col);

            cursor.advance(&step);
        }
    }
}
