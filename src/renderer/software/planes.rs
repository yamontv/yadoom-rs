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

        if lo <= hi {
            if plane.top[lo..=hi].iter().any(|&v| v != u16::MAX) {
                return false; // part of the span already drawn
            }
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

impl Software {
    pub fn flush_planes(&mut self, cam: &Camera, bank: &TextureBank) {
        let cam_fwd = cam.forward();
        let cam_right = cam.right();
        let cam_base = cam.pos.truncate();

        let plane_map = std::mem::take(&mut self.visplane_map);

        // The original Doom drew floors & ceilings *after* the walls, so we
        // simply iterate as we stored them (front parts first = back parts last).
        for vp in plane_map.iter() {
            if vp.tex == NO_TEXTURE || !vp.modified {
                continue;
            }
            for y in 0..self.height as u16 {
                let mut xs = u16::MAX; // sentinel “no run”

                for x in vp.min_x..=vp.max_x {
                    let col = x as usize;

                    let inside = vp.top[col] <= y && vp.bottom[col] >= y;

                    if inside {
                        if xs == u16::MAX {
                            // run starts
                            xs = x;
                        }
                    } else if xs != u16::MAX {
                        // run ends
                        self.emit_span(&cam_fwd, &cam_right, &cam_base, vp, y, xs, x - 1, bank);
                        xs = u16::MAX;
                    }
                }

                if xs != u16::MAX {
                    // tail-run
                    self.emit_span(&cam_fwd, &cam_right, &cam_base, vp, y, xs, vp.max_x, bank);
                }
            }
        }
    }

    /// Convert one horizontal pixel run into a perspective-correct [`PlaneSpan`]
    /// and forward it to the backend renderer.
    #[inline(always)]
    fn emit_span(
        &mut self,
        cam_fwd: &Vec2,
        cam_right: &Vec2,
        cam_base: &Vec2,
        vp: &VisPlane,
        y: u16,
        x_start: u16,
        x_end: u16,
        bank: &TextureBank,
    ) {
        // signed quantities ----------------------------------------------------
        let plane_height = vp.height as f32 - self.view_z; // <0 floor, >0 ceil
        let dy = (y as f32 + 0.5) - self.half_h; // <0 upper half, >0 lower
        let inv_dy = 1.0 / dy; // signed
        let ratio = plane_height * inv_dy; // signed  (key!)

        // positive distance along view direction ------------------------------
        let z = self.focal * ratio.abs(); // == |plane_h| * f / |dy|

        // screen-space helpers -------------------------------------------------
        let left_scr = (x_start as f32 + 0.5) - self.half_w;
        let right_scr = (x_end as f32 + 0.5) - self.half_w;
        let w_px = (x_end - x_start).max(1) as f32;
        let step_scr = (right_scr - left_scr) / w_px;

        // world position at the left edge of the span -------------------------
        let base = cam_base
             + *cam_fwd   * z                                   // forward component
             + *cam_right * (left_scr * ratio); // **signed** lateral shift

        // world-space step per pixel along X ----------------------------------
        let d_world = *cam_right * (step_scr * ratio); // **signed**

        // endpoints -----------------------------------------------------------
        let world_left = base;
        let world_right = base + d_world * w_px;

        // Δ texture U,V per screen pixel – pre-multiplied so the inner loop needs
        // just one fused-add operation.
        let step = UVStep {
            du: (world_right.x - world_left.x) / w_px,
            dv: (world_right.y - world_left.y) / w_px,
        };

        let cursor = UVCursor {
            u: world_left.x,
            v: world_left.y,
        };

        self.draw_plane(vp.tex, vp.light, y, x_start, x_end, step, cursor, bank);
    }

    #[inline(always)]
    fn draw_plane(
        &mut self,
        tex_id: TextureId,
        light: i16,
        y_row: u16,
        x0: u16,
        x1: u16,
        step: UVStep,
        mut cursor: UVCursor,
        bank: &TextureBank,
    ) {
        let tex = bank
            .texture(tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        // per-pixel deltas in 1/z-space
        let row_idx = y_row as usize * self.width;
        let row = &mut self.scratch[row_idx..][..self.width];

        let base_sh = (255u16.saturating_sub(light as u16) >> 3) as u8;

        debug_assert!(
            tex.w.is_power_of_two() && tex.h.is_power_of_two(),
            "textures must be POT"
        );

        let u_mask = (tex.w - 1) as i32;
        let v_mask = (tex.h - 1) as i32;

        for x in x0..=x1 {
            let u = ((cursor.u as i32) & u_mask) as usize;
            let v = ((cursor.v as i32) & v_mask) as usize;
            let col = tex.pixels[v * tex.w + u];

            row[x as usize] = bank.get_color(base_sh, col);

            cursor.advance(&step);
        }
    }
}
