use crate::{
    engine::types::{Screen, Viewer, VisPlane},
    renderer::{PlaneSpan, Renderer},
    world::{
        camera::Camera,
        geometry::Level,
        texture::{NO_TEXTURE, TextureBank, TextureId},
    },
};
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
        // The original Doom drew floors & ceilings *after* the walls, so we
        // simply iterate as we stored them (front parts first = back parts last).
        for vp in &self.planes {
            if vp.tex == NO_TEXTURE {
                continue;
            }
            for y in 0..screen.h as u16 {
                let mut run_start = None;

                for x in vp.min_x..=vp.max_x {
                    let col = x as usize;

                    // is the current (col,row) inside the unclipped part of the plane?
                    if vp.top[col] <= y && vp.bottom[col] >= y {
                        run_start.get_or_insert(x);
                    } else if let Some(xs) = run_start {
                        Self::emit_span(
                            renderer,
                            lvl,
                            cam,
                            screen,
                            view,
                            vp,
                            y as u16,
                            xs,
                            x - 1,
                            bank,
                        );
                        run_start = None;
                    }
                }

                if let Some(xs) = run_start {
                    Self::emit_span(
                        renderer, lvl, cam, screen, view, vp, y as u16, xs, vp.max_x, bank,
                    );
                }
            }
        }
    }

    /// Convert one horizontal pixel run into a perspective-correct [`PlaneSpan`]
    /// and forward it to the backend renderer.
    fn emit_span<R: Renderer>(
        renderer: &mut R,
        _lvl: &Level,
        cam: &Camera,
        screen: &Screen,
        view: &Viewer,
        vp: &VisPlane,
        y: u16,
        x_start: u16,
        x_end: u16,
        bank: &TextureBank,
    ) {
        // For every end-point we need (u/z, v/z, 1/z).  Use the same maths that
        // the classic engine used: treat the floor/ceiling as an infinite plane
        // ───────────────────────────────────────────────────────────────────
        let plane_height = vp.height as f32 - view.view_z;
        let center_y = screen.half_h;
        let screen_y = y as f32 + 0.5; // sample at pixel center
        let inv_p_y = 1.0 / (screen_y - center_y);

        // Pre-compute helpers shared by both ends of the span
        let dist_scale = view.focal * plane_height.abs() * inv_p_y.abs(); // distance along view dir
        let leftmost = (x_start as f32 + 0.5) - screen.half_w;
        let rightmost = (x_end as f32 + 0.5) - screen.half_w;
        let step = (rightmost - leftmost) / (x_end - x_start).max(1) as f32;

        // At x = 0, the world position is eye + forward*dist + right*offset
        let fwd = cam.forward();
        let right = cam.right();
        let base = cam.pos().truncate()
            + fwd * dist_scale
            + right * (leftmost * plane_height / (screen_y - center_y));

        // (u, v) map-units per pixel
        let du = right * (plane_height * step / (screen_y - center_y));
        // let dv = fwd * (plane_height * step / (screen_y - center_y));

        // Compute *once per span* the tex-coord / z and 1/z at both ends
        let world_p0 = base;
        let world_p1 = base + du * (x_end - x_start) as f32;

        let z0 = dist_scale; // distance along view dir doubles as z in camera space
        let z1 = dist_scale; // constant along the span (plane is perpendicular to view)

        let inv_z0 = 1.0 / z0;
        let inv_z1 = 1.0 / z1;

        let u0_over_z = world_p0.x * inv_z0;
        let v0_over_z = world_p0.y * inv_z0;
        let u1_over_z = world_p1.x * inv_z1;
        let v1_over_z = world_p1.y * inv_z1;

        renderer.draw_plane(
            &PlaneSpan {
                tex_id: vp.tex,
                light: vp.light,
                u0_over_z,
                v0_over_z,
                u1_over_z,
                v1_over_z,
                inv_z0,
                inv_z1,
                y,
                x_start,
                x_end,
            },
            bank, /* unused by sw renderer */
        );
    }
}
