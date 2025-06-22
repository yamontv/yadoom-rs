use crate::{
    engine::types::{Screen, Viewer, VisPlane},
    renderer::{PlaneSpan, Renderer},
    world::{
        camera::Camera,
        geometry::Level,
        texture::{NO_TEXTURE, TextureBank, TextureId},
    },
};
use glam::Vec2;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

pub type VisplaneId = u16;

pub const NO_PLANE: VisplaneId = u16::MAX;

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
    pub fn new(width: usize) -> Self {
        let mut map = Self::default();
        map.width = width;
        map
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.planes.clear();
    }

    pub fn get(&mut self, id: VisplaneId) -> Option<&mut VisPlane> {
        if id == NO_PLANE {
            None
        } else {
            self.planes.get_mut(id as usize)
        }
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

    pub fn draw_all<R: Renderer>(
        &self,
        renderer: &mut R,
        lvl: &Level,
        cam: &Camera,
        screen: &Screen,
        view: &Viewer,
        bank: &TextureBank,
    ) {
        let cam_fwd = cam.forward();
        let cam_right = cam.right();
        let cam_base = cam.pos().truncate();

        // The original Doom drew floors & ceilings *after* the walls, so we
        // simply iterate as we stored them (front parts first = back parts last).
        for vp in self.planes.iter() {
            if vp.tex == NO_TEXTURE || !vp.modified {
                continue;
            }
            for y in 0..screen.h as u16 {
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
                        Self::emit_span(
                            renderer,
                            lvl,
                            &cam_fwd,
                            &cam_right,
                            &cam_base,
                            screen,
                            view,
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
                    Self::emit_span(
                        renderer, lvl, &cam_fwd, &cam_right, &cam_base, screen, view, vp, y as u16,
                        xs, vp.max_x, bank,
                    );
                }
            }
        }
    }

    /// Convert one horizontal pixel run into a perspective-correct [`PlaneSpan`]
    /// and forward it to the backend renderer.
    #[inline(always)]
    fn emit_span<R: Renderer>(
        renderer: &mut R,
        _lvl: &Level,
        cam_fwd: &Vec2,
        cam_right: &Vec2,
        cam_base: &Vec2,
        screen: &Screen,
        view: &Viewer,
        vp: &VisPlane,
        y: u16,
        x_start: u16,
        x_end: u16,
        bank: &TextureBank,
    ) {
        // signed quantities ----------------------------------------------------
        let plane_height = vp.height as f32 - view.view_z; // <0 floor, >0 ceil
        let dy = (y as f32 + 0.5) - screen.half_h; // <0 upper half, >0 lower
        let inv_dy = 1.0 / dy; // signed
        let ratio = plane_height * inv_dy; // signed  (key!)

        // positive distance along view direction ------------------------------
        let z = view.focal * ratio.abs(); // == |plane_h| * f / |dy|
        let inv_z = 1.0 / z;

        // screen-space helpers -------------------------------------------------
        let left_scr = (x_start as f32 + 0.5) - screen.half_w;
        let right_scr = (x_end as f32 + 0.5) - screen.half_w;
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

        renderer.draw_plane(
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
}
