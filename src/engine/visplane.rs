//! ----------------------------------------------------------------------------
//!  “Vis-plane” collector
//!
//!  ▸ Runs **during** BSP traversal: every time a subsector becomes visible we
//!    register its floor / ceiling columns that are still uncovered in `ClipBands`.
//!  ▸ Runs **after** the walls: builds perspective-correct [`PlaneSpan`]s and
//!    feeds them to the active [`Renderer`] in back-to-front order.
//!
//!  The code is deliberately self-contained so nothing outside the engine needs
//!  to know how vis-planes work.
//!
//! ----------------------------------------------------------------------------
use crate::{
    engine::types::{Screen, Viewer},
    renderer::{ClipBands, PlaneSpan, Renderer},
    world::{camera::Camera, geometry::Level, texture::TextureBank, texture::TextureId},
};

/// Maximum number of different planes the old vanilla engine could handle.
/// We allocate dynamically, so this is just a sanity limit to avoid surprises.
const MAX_VISPLANES: usize = 1 << 12;

/// One “flat” that is visible somewhere on screen.
#[derive(Clone)]
struct VisPlane {
    /// Floor height minus viewer eye-Z  ( < 0  = below eye,  > 0  = above eye )
    dz: f32,
    tex: TextureId,
    is_floor: bool,

    /// Inclusive horizontal range that the plane touches.
    min_x: i32,
    max_x: i32,

    /// For every screen column we remember the highest and lowest pixel that is
    /// still uncovered **after** drawing the front geometry.
    top: Vec<i16>,
    bot: Vec<i16>,
}

/*──────────────────────────── Frame state ─────────────────────────────*/

#[derive(Default)]
pub struct PlaneCollector {
    planes: Vec<VisPlane>,
    w: usize,
    h: usize,
}

impl PlaneCollector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call once per frame when the renderer clears its buffers.
    pub fn begin_frame(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        self.planes.clear();
    }

    /// During BSP traversal, once **all SEGs of one subsector** have been drawn
    /// and `bands` therefore contains the new clip limits, call this method
    /// once for the *floor* and once for the *ceiling* of that subsector.
    pub fn add_subsector_plane(
        &mut self,
        dz: f32,
        tex: TextureId,
        is_floor: bool,
        bands: &ClipBands,
        // inclusive horizontal range that the subsector touched in screen space
        x_range: core::ops::RangeInclusive<i32>,
    ) {
        let min_x = *x_range.start().max(&0);
        let max_x = *x_range.end().min(&((self.w as i32) - 1));
        if min_x > max_x {
            return;
        }

        // 1 ─ find existing vis-plane we can merge into … ---------------------------------
        let mut idx = None;
        for (i, vp) in self.planes.iter().enumerate() {
            if vp.dz == dz && vp.tex == tex && vp.is_floor == is_floor {
                idx = Some(i);
                break;
            }
        }

        // 2 ─ or allocate a new one otherwise --------------------------------------------
        let vp = if let Some(i) = idx {
            &mut self.planes[i]
        } else {
            if self.planes.len() >= MAX_VISPLANES {
                // Soft-fail: ignore additional planes instead of panicking.
                return;
            }
            self.planes.push(VisPlane {
                dz,
                tex,
                is_floor,
                min_x,
                max_x,
                top: vec![i16::MAX; self.w],
                bot: vec![i16::MIN; self.w],
            });
            self.planes.last_mut().unwrap()
        };

        // 3 ─ store the vertical coverage for every column -------------------------------
        for x in min_x..=max_x {
            let col = x as usize;
            if is_floor {
                // floor spans start at current clip-floor and run to the bottom edge
                vp.top[col] = vp.top[col].min(bands.floor[col] as i16 + 1);
                vp.bot[col] = (self.h as i16 - 1).max(vp.bot[col]);
            } else {
                // ceiling spans start at the top of the screen and end at clip-ceiling
                vp.top[col] = 0;
                vp.bot[col] = vp.bot[col].max(bands.ceil[col] as i16 - 1);
            }
            vp.min_x = vp.min_x.min(x);
            vp.max_x = vp.max_x.max(x);
        }
    }

    /*──────────────────── After BSP traversal ────────────────────*/

    /// Build perspective-correct [`PlaneSpan`]s for *all* planes and feed them
    /// to the active renderer in **back-to-front** order.
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
            // For every scan-line build contiguous X-spans so the renderer gets
            // as few draw calls as possible.  This exactly mirrors R_DrawPlanes.
            for y in vp.top.iter().zip(&vp.bot).enumerate() {
                let (y, (&top, &bot)) = y;
                if bot < top {
                    continue; // nothing visible on this scan-line
                }
                let mut run_start = None;
                for x in vp.min_x..=vp.max_x {
                    let col = x as usize;
                    if (vp.top[col] as usize) <= y && (vp.bot[col] as usize) >= y {
                        // inside the visible column
                        run_start.get_or_insert(x);
                    } else if let Some(xs) = run_start {
                        self.emit_span(
                            renderer,
                            lvl,
                            cam,
                            screen,
                            view,
                            vp,
                            y as i32,
                            xs,
                            x - 1,
                            bank,
                        );
                        run_start = None;
                    }
                }
                if let Some(xs) = run_start {
                    self.emit_span(
                        renderer, lvl, cam, screen, view, vp, y as i32, xs, vp.max_x, bank,
                    );
                }
            }
        }
    }

    /// Convert one horizontal pixel run into a perspective-correct [`PlaneSpan`]
    /// and forward it to the backend renderer.
    fn emit_span<R: Renderer>(
        &self,
        renderer: &mut R,
        _lvl: &Level,
        cam: &Camera,
        screen: &Screen,
        view: &Viewer,
        vp: &VisPlane,
        y: i32,
        x_start: i32,
        x_end: i32,
        bank: &TextureBank,
    ) {
        // For every end-point we need (u/z, v/z, 1/z).  Use the same maths that
        // the classic engine used: treat the floor/ceiling as an infinite plane
        // ───────────────────────────────────────────────────────────────────
        let plane_height = vp.dz;
        let center_y = screen.half_h;
        let screen_y = y as f32 + 0.5; // sample at pixel center
        let inv_p_y = 1.0 / (screen_y - center_y);

        // Pre-compute helpers shared by both ends of the span
        let dist_scale = view.focal * plane_height * inv_p_y; // distance along view dir
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
        let dv = fwd * (plane_height * step / (screen_y - center_y));

        // Compute *once per span* the tex-coord / z and 1/z at both ends
        let mut world_p0 = base;
        let mut world_p1 = base + du * (x_end - x_start) as f32;

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
                u0_over_z,
                v0_over_z,
                u1_over_z,
                v1_over_z,
                inv_z0,
                inv_z1,
                y,
                x_start,
                x_end,
                is_floor: vp.is_floor,
            },
            // planes never update ClipBands
            &ClipBands {
                ceil: &mut [],
                floor: &mut [],
            },
            bank, /* unused by sw renderer */
        );
    }
}
