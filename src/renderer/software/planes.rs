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

#[derive(Clone, Debug)]
pub struct PlaneSpan {
    pub tex_id: TextureId,
    pub light: i16,
    /* perspective-correct UV/z at span edges */
    pub u0_over_z: f32,
    pub v0_over_z: f32,
    pub u1_over_z: f32,
    pub v1_over_z: f32,
    pub inv_z0: f32,
    pub inv_z1: f32,
    /* screen extents */
    pub y: u16,
    pub x_start: u16,
    pub x_end: u16,
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

        for x in intrl..=intrh {
            if plane.top[x as usize] != u16::MAX {
                return false;
            }
        }

        plane.min_x = unionl;
        plane.max_x = unionh;

        // use the same one
        return true;
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
                        self.emit_span(
                            &cam_fwd,
                            &cam_right,
                            &cam_base,
                            vp,
                            y as u16,
                            xs,
                            x - 1,
                            bank,
                        );
                        xs = u16::MAX;
                    }
                }

                if xs != u16::MAX {
                    // tail-run
                    self.emit_span(
                        &cam_fwd, &cam_right, &cam_base, vp, y as u16, xs, vp.max_x, bank,
                    );
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
        let inv_z = 1.0 / z;

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
        let du = *cam_right * (step_scr * ratio); // **signed**

        // endpoints -----------------------------------------------------------
        let world0 = base;
        let world1 = base + du * w_px;

        let u0oz = world0.x * inv_z;
        let v0oz = world0.y * inv_z;
        let u1oz = world1.x * inv_z; // z is constant, reuse inv_z
        let v1oz = world1.y * inv_z;

        self.draw_plane(
            PlaneSpan {
                tex_id: vp.tex,
                light: vp.light,
                u0_over_z: u0oz,
                v0_over_z: v0oz,
                u1_over_z: u1oz,
                v1_over_z: v1oz,
                inv_z0: inv_z, // identical at both ends
                inv_z1: inv_z,
                y,
                x_start,
                x_end,
            },
            bank,
        );
    }

    #[inline(always)]
    fn draw_plane(&mut self, span: PlaneSpan, bank: &TextureBank) {
        let tex = bank
            .texture(span.tex_id)
            .unwrap_or_else(|_| bank.texture(NO_TEXTURE).unwrap());

        // per-pixel deltas in 1/z-space
        let w = (span.x_end - span.x_start).max(1) as f32;
        let du = (span.u1_over_z - span.u0_over_z) / w;
        let dv = (span.v1_over_z - span.v0_over_z) / w;
        let dz = (span.inv_z1 - span.inv_z0) / w;

        let mut uoz = span.u0_over_z;
        let mut voz = span.v0_over_z;
        let mut iz = span.inv_z0;

        let row_idx = span.y as usize * self.width;
        let row = &mut self.scratch[row_idx..][..self.width];

        let base_sh = ((255 - span.light) >> 3) as usize;

        // -------- render in small groups to reuse a single reciprocal ----------
        const G: usize = 8; // group size
        let mut x = span.x_start as usize;
        while x + (G as usize) - 1 <= span.x_end as usize {
            // one reciprocal gives ≈7–8 ulp accuracy after one NR step
            let mut w = iz.recip(); // fast (≈4 cycles)
            w = w * (2.0 - iz * w); // Newton–Raphson refine

            for g in 0..G {
                let u = ((uoz * w) as i32).rem_euclid(tex.w as i32) as usize;
                let v = ((voz * w) as i32).rem_euclid(tex.h as i32) as usize;
                let col = tex.pixels[v * tex.w + u];

                // let dist_idx = ((1.0 / iz) / DIST_FADE_FULL * 31.0).min(31.0) as usize;
                // let shade = (base_sh + dist_idx).min(31) as u8;
                let shade = base_sh as u8;

                row[x + g] = bank.get_color(shade, col);

                uoz += du;
                voz += dv;
                iz += dz;
            }
            x += G;
        }

        // tail ( < G pixels ) — fall back to the scalar path
        for x in x..=span.x_end as usize {
            let w = iz.recip();
            let u = ((uoz * w) as i32).rem_euclid(tex.w as i32) as usize;
            let v = ((voz * w) as i32).rem_euclid(tex.h as i32) as usize;
            let col = tex.pixels[v * tex.w + u];

            // let dist_idx = ((1.0 / iz) / DIST_FADE_FULL * 31.0).min(31.0) as usize;
            // let shade = (base_sh + dist_idx).min(31) as u8;
            let shade = base_sh as u8;

            row[x] = bank.get_color(shade, col);

            uoz += du;
            voz += dv;
            iz += dz;
        }
    }
}
